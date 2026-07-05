//! A `redb`-backed `Chain` that survives an app restart — the first
//! on-disk persistence anywhere in this codebase (everything else, Rust
//! or Swift, has always been either in-memory-only or owned entirely by
//! the app layer). `redb` was chosen over `sled`/rocksdb/LMDB: pure Rust
//! (no C toolchain step added to this repo's iOS/Android cross-compile
//! CI), ACID via a copy-on-write B-tree with a clear crash-safety
//! contract, and an actively maintained 1.x+-stable release — `sled`'s
//! own issue tracker still describes its durability guarantees as
//! unsettled, and rocksdb/LMDB both require linking a C library.
//!
//! Every block acceptance is one `redb` write transaction spanning every
//! table it touches, committed only after `Chain::try_apply` has already
//! validated the block in memory — a crash mid-write rolls back to the
//! last consistent on-disk state, and a block that was never durably
//! committed simply gets re-received over gossip on next launch (this is
//! a P2P system; that's the normal, expected recovery path, not a bug).

use std::path::Path;
use std::time::Duration;

use redb::{Database, DatabaseError, ReadableTable, TableDefinition};

use crate::block::Block;
use crate::chain_state::{ApplyOutcome, Chain, Checkpoint};
use crate::difficulty::CompactTarget;
use crate::error::{LedgerError, Result};
use crate::hash::Hash32;
use crate::transaction::Transaction;
use crate::validation;

/// hash (32 bytes) -> bincode(Block) — full bodies, pruned outside the
/// retention window.
const BLOCKS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("blocks");
/// hash (32 bytes) -> bincode(BlockMeta) — every block ever accepted,
/// never pruned.
const HEADERS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("headers");
/// height -> hash (32 bytes) — canonical chain only, rewritten on reorg.
const CANONICAL_HEIGHTS: TableDefinition<u64, &[u8]> = TableDefinition::new("canonical_heights");
/// A handful of named singleton values: the current tip hash, and (once
/// pruning has happened at least once) the active checkpoint.
const SINGLETON: TableDefinition<&str, &[u8]> = TableDefinition::new("singleton");

const TIP_HASH_KEY: &str = "tip_hash";
const CHECKPOINT_KEY: &str = "checkpoint";
/// The highest height `CANONICAL_HEIGHTS` has ever recorded an entry for
/// — needed because a reorg to a *heavier-but-shorter* branch (possible:
/// fork choice is by cumulative work, not block count, and a branch that
/// happened to retarget to higher difficulty can out-work a longer one)
/// would otherwise leave stale entries above the new tip pointing at
/// blocks that are no longer canonical at all.
const MAX_HEIGHT_KEY: &str = "canonical_max_height";

/// How many blocks of full bodies to keep behind the tip for reorg
/// safety — older bodies get pruned to metadata-only. Comfortably larger
/// than both `RETARGET_INTERVAL_BLOCKS` (288) and `ANCHOR_MAX_AGE_BLOCKS`
/// (144), so pruning never removes a body a normal validation still
/// needs.
pub const RETENTION_WINDOW_BLOCKS: u64 = 2016; // ~1 week at 5-minute blocks

fn to_storage_err(err: impl std::fmt::Display) -> LedgerError {
    LedgerError::Storage(err.to_string())
}

fn encode<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    bincode::serialize(value).map_err(to_storage_err)
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    bincode::deserialize(bytes).map_err(to_storage_err)
}

pub struct ChainStore {
    db: Database,
    chain: Chain,
}

impl ChainStore {
    /// Opens (or creates, if the path doesn't exist yet or is empty) a
    /// durable chain at `path`. On a fresh database this starts from
    /// genesis and persists it immediately; on an existing one it
    /// restores whatever checkpoint was last saved (real genesis if
    /// pruning has never happened) and replays every canonical block
    /// stored above it, ending up in exactly the state the chain was in
    /// when it was last closed.
    pub fn open(path: &Path) -> Result<Self> {
        let db = Self::create_with_retry(path)?;
        Self::ensure_tables_exist(&db)?;

        let tip_hash_bytes = Self::read_singleton(&db, TIP_HASH_KEY)?;

        let chain = match tip_hash_bytes {
            None => Chain::new(),
            Some(_) => Self::restore(&db)?,
        };

        let mut store = ChainStore { db, chain };
        if tip_hash_bytes.is_none() {
            store.persist_after_apply(&Block::genesis())?;
        }
        Ok(store)
    }

    /// `Database::create`, retrying briefly on `DatabaseAlreadyOpen`
    /// specifically — this crate's own file lock, not a stale one left
    /// by some other process. The previous owner of this same path
    /// (e.g., on iOS, `FfiP2pNode::shutdown`'s background event loop
    /// task, mid-way through actually stopping) can still hold it for a
    /// short moment after a caller already considers that node "shut
    /// down": `shutdown()` only *requests* the stop and returns
    /// immediately, it doesn't wait for the task to finish dropping its
    /// own `Database` handle. Retried, not treated as fatal on the first
    /// failure, since that gap is normally milliseconds, not something a
    /// caller (e.g. switching accounts right after signing out of the
    /// last one) should have to work around itself. Any other error
    /// (including `DatabaseAlreadyOpen` that still hasn't cleared after
    /// the whole budget) is returned immediately/as the last attempt's
    /// error, not retried indefinitely.
    fn create_with_retry(path: &Path) -> Result<Database> {
        const MAX_ATTEMPTS: u32 = 20;
        const RETRY_DELAY: Duration = Duration::from_millis(50);
        for attempt in 1..=MAX_ATTEMPTS {
            match Database::create(path) {
                Ok(db) => return Ok(db),
                Err(DatabaseError::DatabaseAlreadyOpen) if attempt < MAX_ATTEMPTS => {
                    std::thread::sleep(RETRY_DELAY);
                }
                Err(err) => return Err(to_storage_err(err)),
            }
        }
        unreachable!("the loop above always returns on its final attempt")
    }

    fn ensure_tables_exist(db: &Database) -> Result<()> {
        let write_txn = db.begin_write().map_err(to_storage_err)?;
        {
            write_txn.open_table(BLOCKS).map_err(to_storage_err)?;
            write_txn.open_table(HEADERS).map_err(to_storage_err)?;
            write_txn.open_table(CANONICAL_HEIGHTS).map_err(to_storage_err)?;
            write_txn.open_table(SINGLETON).map_err(to_storage_err)?;
        }
        write_txn.commit().map_err(to_storage_err)
    }

    fn read_singleton(db: &Database, key: &str) -> Result<Option<Vec<u8>>> {
        let read_txn = db.begin_read().map_err(to_storage_err)?;
        let table = read_txn.open_table(SINGLETON).map_err(to_storage_err)?;
        Ok(table.get(key).map_err(to_storage_err)?.map(|guard| guard.value().to_vec()))
    }

    /// Rebuilds an in-memory `Chain` from whatever's on disk: the last
    /// saved checkpoint (or real genesis, if none was ever saved) plus
    /// every canonical block stored above it, replayed in height order
    /// through the exact same `Chain::try_apply` a live block goes
    /// through — this is a restore, not a shortcut, so it re-derives
    /// `username_owner` rather than trusting a separately-stored copy of
    /// it (only the checkpoint's own snapshot, which predates anything
    /// this restore replays, is trusted as-is).
    fn restore(db: &Database) -> Result<Chain> {
        let read_txn = db.begin_read().map_err(to_storage_err)?;
        let singleton = read_txn.open_table(SINGLETON).map_err(to_storage_err)?;
        let heights = read_txn.open_table(CANONICAL_HEIGHTS).map_err(to_storage_err)?;
        let blocks = read_txn.open_table(BLOCKS).map_err(to_storage_err)?;

        let mut chain = match singleton.get(CHECKPOINT_KEY).map_err(to_storage_err)? {
            Some(guard) => Chain::from_checkpoint(decode::<Checkpoint>(guard.value())?),
            None => Chain::new(),
        };

        let mut height = chain.floor_height() + 1;
        loop {
            let Some(hash_guard) = heights.get(height).map_err(to_storage_err)? else { break };
            let hash = hash_from_slice(hash_guard.value())?;
            let block_guard = blocks
                .get(hash.as_bytes().as_slice())
                .map_err(to_storage_err)?
                .ok_or(LedgerError::UnknownBlock(hash))?;
            let block: Block = decode(block_guard.value())?;
            let replay_now = block.header.timestamp;
            chain.try_apply(block, replay_now)?;
            height += 1;
        }

        Ok(chain)
    }

    pub fn tip_hash(&self) -> Hash32 {
        self.chain.tip_hash()
    }

    pub fn tip_height(&self) -> u64 {
        self.chain.tip_height()
    }

    pub fn username_owner(&self, username: &str) -> Option<&crate::chain_state::UsernameOwner> {
        self.chain.username_owner(username)
    }

    pub fn tip_cumulative_work(&self) -> f64 {
        self.chain.get_meta(&self.chain.tip_hash()).expect("tip is always known").cumulative_work
    }

    /// The full block at canonical `height`, if this node still has its
    /// body (i.e. `height` is at or above the current floor) — used to
    /// answer a peer's `GetBlocks` sync request.
    pub fn canonical_block_at(&self, height: u64) -> Option<&Block> {
        self.chain.canonical_block_at(height)
    }

    /// A checkpoint at the current tip, for answering a peer's
    /// `GetUsernameOwnerSnapshot` sync request — the requester treats this
    /// as an untrusted candidate until independently corroborated, never
    /// trusted outright (see `ledger.rs`'s doc comment in `p2p-core`).
    pub fn checkpoint_at_tip(&self) -> Checkpoint {
        self.chain.checkpoint_at_tip()
    }

    /// The `difficulty_target` a block extending the current tip must
    /// have — what a miner assembling a new candidate needs, computed the
    /// same way `try_apply` itself independently re-derives it (never
    /// trusting a miner's self-declared value).
    pub fn expected_difficulty(&self) -> Result<CompactTarget> {
        self.chain.expected_difficulty(&self.chain.tip_hash())
    }

    /// The timestamp floor (exclusive) a block extending the current tip
    /// must be strictly after.
    pub fn median_time_past(&self) -> Result<u64> {
        self.chain.median_time_past(&self.chain.tip_hash())
    }

    /// Whether `tx` could be included in a block extending the current tip
    /// at `candidate_height` right now — see
    /// `validation::is_valid_for_mempool` for what this actually checks. A
    /// miner uses this to filter its mempool before assembling a candidate.
    pub fn is_valid_candidate_transaction(&self, tx: &Transaction, candidate_height: u64) -> bool {
        validation::is_valid_for_mempool(&self.chain, tx, candidate_height)
    }

    /// Validates and durably accepts `block`. Returns the same
    /// `ApplyOutcome` `Chain::try_apply` would, and only touches disk at
    /// all if the outcome isn't `AlreadyKnown`.
    pub fn try_apply(&mut self, block: Block, now: u64) -> Result<ApplyOutcome> {
        let outcome = self.chain.try_apply(block.clone(), now)?;
        if outcome != ApplyOutcome::AlreadyKnown {
            self.persist_after_apply(&block)?;
        }
        Ok(outcome)
    }

    /// Writes `block`'s body and header, and — since every block that
    /// reaches here was just accepted onto the canonical chain — the
    /// current full canonical height index and tip pointer, all in one
    /// transaction.
    fn persist_after_apply(&mut self, block: &Block) -> Result<()> {
        let hash = block.hash();
        let meta = *self.chain.get_meta(&hash).expect("just applied, so its meta must exist");
        let canonical_hashes = self.chain.ancestors(&self.chain.tip_hash()).expect("tip is always known");
        let new_tip_height = self.chain.tip_height();

        let write_txn = self.db.begin_write().map_err(to_storage_err)?;
        {
            let mut blocks_table = write_txn.open_table(BLOCKS).map_err(to_storage_err)?;
            blocks_table.insert(hash.as_bytes().as_slice(), encode(block)?.as_slice()).map_err(to_storage_err)?;

            let mut headers_table = write_txn.open_table(HEADERS).map_err(to_storage_err)?;
            headers_table.insert(hash.as_bytes().as_slice(), encode(&meta)?.as_slice()).map_err(to_storage_err)?;

            let mut singleton_table = write_txn.open_table(SINGLETON).map_err(to_storage_err)?;

            let mut heights_table = write_txn.open_table(CANONICAL_HEIGHTS).map_err(to_storage_err)?;
            for (offset, ancestor_hash) in canonical_hashes.iter().enumerate() {
                let height = self.chain.floor_height() + offset as u64;
                heights_table.insert(height, ancestor_hash.as_bytes().as_slice()).map_err(to_storage_err)?;
            }

            // A reorg onto a heavier-but-shorter branch would otherwise
            // leave stale entries above the new tip still pointing at
            // blocks that are no longer canonical at all — remove them.
            let previous_max_height = singleton_table
                .get(MAX_HEIGHT_KEY)
                .map_err(to_storage_err)?
                .map(|guard| u64::from_le_bytes(guard.value().try_into().unwrap_or_default()))
                .unwrap_or(0);
            for stale_height in (new_tip_height + 1)..=previous_max_height {
                heights_table.remove(stale_height).map_err(to_storage_err)?;
            }

            singleton_table
                .insert(MAX_HEIGHT_KEY, new_tip_height.max(previous_max_height).to_le_bytes().as_slice())
                .map_err(to_storage_err)?;
            singleton_table
                .insert(TIP_HASH_KEY, self.chain.tip_hash().as_bytes().as_slice())
                .map_err(to_storage_err)?;
        }
        write_txn.commit().map_err(to_storage_err)
    }

    /// Prunes full bodies older than `RETENTION_WINDOW_BLOCKS` behind the
    /// current tip. A thin wrapper around `prune_with_retention` — see
    /// there for the actual logic and why it takes a parameter at all.
    pub fn prune(&mut self) -> Result<()> {
        self.prune_with_retention(RETENTION_WINDOW_BLOCKS)
    }

    /// Prunes full bodies older than `retention` blocks behind the
    /// current tip, persisting the resulting checkpoint durably before
    /// deleting anything from disk — so a crash between the two can only
    /// ever leave *extra* (still-valid, just no-longer-needed) bodies on
    /// disk, never lose the ability to restore. A no-op if the chain
    /// isn't yet deep enough for pruning to apply. Takes an explicit
    /// `retention` (rather than always using `RETENTION_WINDOW_BLOCKS`)
    /// so tests can exercise real pruning without mining thousands of
    /// blocks first.
    pub fn prune_with_retention(&mut self, retention: u64) -> Result<()> {
        let tip_height = self.chain.tip_height();
        let Some(new_floor_height) = tip_height.checked_sub(retention) else { return Ok(()) };
        if new_floor_height <= self.chain.floor_height() {
            return Ok(());
        }

        let checkpoint = self.chain.checkpoint_at(new_floor_height)?;

        let write_txn = self.db.begin_write().map_err(to_storage_err)?;
        {
            let mut singleton_table = write_txn.open_table(SINGLETON).map_err(to_storage_err)?;
            singleton_table.insert(CHECKPOINT_KEY, encode(&checkpoint)?.as_slice()).map_err(to_storage_err)?;
        }
        write_txn.commit().map_err(to_storage_err)?;

        // Only after the checkpoint is durably committed: actually free
        // the now-redundant bodies, in memory and on disk.
        self.chain.prune_bodies_up_to(new_floor_height)?;
        let write_txn = self.db.begin_write().map_err(to_storage_err)?;
        {
            let mut blocks_table = write_txn.open_table(BLOCKS).map_err(to_storage_err)?;
            let stale_hashes: Vec<Hash32> = blocks_table
                .iter()
                .map_err(to_storage_err)?
                .filter_map(|entry| entry.ok())
                .filter_map(|(key, value)| {
                    let block: Block = decode(value.value()).ok()?;
                    (block.header.height <= new_floor_height).then(|| hash_from_slice(key.value()).ok()).flatten()
                })
                .collect();
            for hash in stale_hashes {
                blocks_table.remove(hash.as_bytes().as_slice()).map_err(to_storage_err)?;
            }
        }
        write_txn.commit().map_err(to_storage_err)
    }
}

fn hash_from_slice(bytes: &[u8]) -> Result<Hash32> {
    let array: [u8; 32] = bytes.try_into().map_err(|_| LedgerError::Storage("corrupt 32-byte hash".to_string()))?;
    Ok(Hash32(array))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{identity, mine_block};
    use crate::transaction::Transaction;

    #[test]
    fn a_fresh_store_starts_at_genesis() {
        let dir = tempfile::tempdir().unwrap();
        let store = ChainStore::open(&dir.path().join("chain.redb")).unwrap();
        assert_eq!(store.tip_height(), 0);
    }

    #[test]
    fn open_retries_past_a_transient_database_already_open_error() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("chain.redb");
        ChainStore::open(&db_path).unwrap();

        // Hold the file's OS-level lock open on a background thread for a
        // window that outlasts at least one retry attempt (50ms) but fits
        // comfortably inside the retry budget (20 * 50ms = 1s) — mirrors
        // the real account-switch race this fix targets: the previous
        // owner (there, `FfiP2pNode::shutdown`'s event loop task, still
        // mid-way through actually stopping; here, this thread) still
        // holds the lock for a short moment after the next opener has
        // already started trying. Without `create_with_retry`'s loop,
        // `ChainStore::open` below would fail immediately with
        // `DatabaseAlreadyOpen` instead of waiting this out.
        let held = Database::create(&db_path).unwrap();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            drop(held);
        });

        let store = ChainStore::open(&db_path).unwrap();
        assert_eq!(store.tip_height(), 0);

        handle.join().unwrap();
    }

    #[test]
    fn a_reopened_store_remembers_accepted_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("chain.redb");
        let alice = identity(1);
        let now = 2_000_000_000;

        {
            let mut store = ChainStore::open(&db_path).unwrap();
            let tx = Transaction::new_claim(&alice, "alice", 1, store.tip_hash(), [1; 8]).unwrap();
            let block = mine_block(&store.chain, vec![tx], now);
            store.try_apply(block, now).unwrap();
            assert_eq!(store.tip_height(), 1);
        }

        let store = ChainStore::open(&db_path).unwrap();
        assert_eq!(store.tip_height(), 1);
        assert_eq!(store.username_owner("alice").unwrap().owner_public_key, alice.public_key().to_bytes());
    }

    #[test]
    fn a_reopened_store_survives_several_blocks_and_a_claim_in_the_middle() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("chain.redb");
        let alice = identity(1);
        let bob = identity(2);
        let now = 2_000_000_000;

        {
            let mut store = ChainStore::open(&db_path).unwrap();
            for i in 0..4u64 {
                let txs = if i == 2 {
                    vec![Transaction::new_claim(&alice, "alice", i, store.tip_hash(), [1; 8]).unwrap()]
                } else {
                    vec![]
                };
                let block = mine_block(&store.chain, txs, now + i * 400);
                store.try_apply(block, now + i * 400).unwrap();
            }
            let tx_bob = Transaction::new_claim(&bob, "bobby", 5, store.tip_hash(), [2; 8]).unwrap();
            let block = mine_block(&store.chain, vec![tx_bob], now + 4 * 400);
            store.try_apply(block, now + 4 * 400).unwrap();
        }

        let store = ChainStore::open(&db_path).unwrap();
        assert_eq!(store.tip_height(), 5);
        assert_eq!(store.username_owner("alice").unwrap().owner_public_key, alice.public_key().to_bytes());
        assert_eq!(store.username_owner("bobby").unwrap().owner_public_key, bob.public_key().to_bytes());
    }

    #[test]
    fn pruning_through_the_real_public_api_then_reopening_still_restores_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("chain.redb");
        let alice = identity(1);
        let now = 2_000_000_000;

        {
            let mut store = ChainStore::open(&db_path).unwrap();
            let tx = Transaction::new_claim(&alice, "alice", 1, store.tip_hash(), [1; 8]).unwrap();
            let block = mine_block(&store.chain, vec![tx], now);
            let oldest_hash = block.hash();
            store.try_apply(block, now).unwrap();

            for i in 1..10u64 {
                let block = mine_block(&store.chain, vec![], now + i * 400);
                store.try_apply(block, now + i * 400).unwrap();
            }
            assert_eq!(store.tip_height(), 10);

            // A retention window of 3 makes height 10-3=7 the new floor —
            // comfortably pruning height 1's body through the real,
            // public `prune_with_retention` path (production always uses
            // the real `RETENTION_WINDOW_BLOCKS` via `prune()`; this test
            // only shrinks the window so it doesn't need to mine
            // thousands of blocks to exercise the same code path).
            store.prune_with_retention(3).unwrap();
            assert!(store.chain.get_block(&oldest_hash).is_none(), "body should be pruned");
            assert_eq!(store.chain.floor_height(), 7);
            // The live view must be completely unaffected by pruning.
            assert_eq!(store.username_owner("alice").unwrap().owner_public_key, alice.public_key().to_bytes());
        }

        // Reopening restores from the checkpoint `prune_with_retention`
        // durably saved (not real genesis), which must still land on the
        // exact same tip and username ownership.
        let store = ChainStore::open(&db_path).unwrap();
        assert_eq!(store.tip_height(), 10);
        assert_eq!(store.username_owner("alice").unwrap().owner_public_key, alice.public_key().to_bytes());
    }
}
