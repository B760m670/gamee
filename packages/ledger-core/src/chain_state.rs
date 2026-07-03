//! The in-memory chain: every block this node has ever accepted, which one
//! is canonical, and the materialized `username -> owner` view of the
//! canonical chain. No persistence here — see `store.rs` (Phase 2) for the
//! redb-backed wrapper that survives a restart; this is the reference
//! implementation Phase 2 is built and tested against.
//!
//! Every block's metadata (height, parent, timestamp, difficulty,
//! cumulative work) lives in `meta` and is **never pruned** — it's tiny
//! (a few dozen bytes) and every validation rule except replaying
//! transaction history only ever needs it, never a block's full body. Full
//! bodies (`blocks`, which hold the actual transactions) are the only
//! thing Phase 2 prunes once they're older than the retention window,
//! which is exactly what a `floor` (see `set_floor`) represents: "trust
//! this materialized `username_owner` snapshot as of this height instead
//! of being able to replay any further back." A pristine `Chain::new()`
//! has its floor at real genesis with an empty snapshot, so nothing about
//! its behavior changes from a chain that's never been checkpointed.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::block::Block;
use crate::difficulty::{work_of, CompactTarget};
use crate::error::{LedgerError, Result};
use crate::hash::Hash32;
use crate::transaction::normalize_username;
use crate::validation::validate_block;

pub const MEDIAN_TIME_PAST_WINDOW: usize = 11;

/// Everything about a block validation ever needs *except* its
/// transactions — kept forever, for every block, regardless of whether
/// the full body has been pruned.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BlockMeta {
    pub height: u64,
    pub prev_hash: Hash32,
    pub timestamp: u64,
    pub difficulty_target: CompactTarget,
    pub cumulative_work: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsernameOwner {
    pub owner_public_key: [u8; 32],
    pub claimed_at_height: u64,
    pub claimed_in_block: Hash32,
}

/// A trusted starting point for a `Chain` that isn't real genesis — the
/// materialized state as of some block, standing in for everything before
/// it. Used to restore a chain from a Phase 2 checkpoint after old block
/// bodies have been pruned; a `Chain::new()` chain's floor is real genesis
/// with an empty snapshot, so this concept is invisible unless pruning has
/// actually happened.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub height: u64,
    pub hash: Hash32,
    pub timestamp: u64,
    pub difficulty_target: CompactTarget,
    pub cumulative_work: f64,
    pub username_owner: HashMap<String, UsernameOwner>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// This exact block was already accepted — applying it again is a
    /// no-op, not an error (gossip naturally delivers duplicates).
    AlreadyKnown,
    /// The common case: this block's parent was already the tip, and it
    /// became the new tip.
    ExtendedTip,
    /// Accepted, but its chain doesn't (yet) have more work than the
    /// current canonical tip — stored in case a sibling block later makes
    /// this branch the heavier one.
    AddedToFork,
    /// This block's chain overtook the canonical tip from a different
    /// branch — `username_owner` has been recomputed for the new tip.
    ReorgedTo { new_height: u64 },
}

pub struct Chain {
    blocks: HashMap<Hash32, Block>,
    meta: HashMap<Hash32, BlockMeta>,
    tip_hash: Hash32,
    canonical_index: BTreeMap<u64, Hash32>,
    username_owner: HashMap<String, UsernameOwner>,
    floor_hash: Hash32,
    floor_height: u64,
    floor_username_owner: HashMap<String, UsernameOwner>,
}

impl Chain {
    pub fn new() -> Self {
        let genesis = Block::genesis();
        let hash = genesis.hash();

        let mut blocks = HashMap::new();
        blocks.insert(hash, genesis);

        let mut meta = HashMap::new();
        meta.insert(
            hash,
            BlockMeta {
                height: 0,
                prev_hash: Hash32::ZERO, // unused: ancestors() stops at the floor before reading this
                timestamp: crate::block::GENESIS_TIMESTAMP,
                difficulty_target: crate::difficulty::INITIAL_DIFFICULTY_BITS,
                cumulative_work: 0.0,
            },
        );

        let mut canonical_index = BTreeMap::new();
        canonical_index.insert(0, hash);

        Chain {
            blocks,
            meta,
            tip_hash: hash,
            canonical_index,
            username_owner: HashMap::new(),
            floor_hash: hash,
            floor_height: 0,
            floor_username_owner: HashMap::new(),
        }
    }

    /// Restores a chain from a Phase 2 checkpoint instead of real genesis
    /// — `checkpoint.username_owner` is trusted as-is (Phase 2 is
    /// responsible for having derived it correctly before pruning the
    /// history that produced it); nothing before `checkpoint.height` can
    /// ever be re-derived or re-validated by this `Chain` again. The
    /// checkpoint block itself needs no body — its effects are already
    /// folded into `username_owner`.
    pub fn from_checkpoint(checkpoint: Checkpoint) -> Self {
        let mut meta = HashMap::new();
        meta.insert(
            checkpoint.hash,
            BlockMeta {
                height: checkpoint.height,
                prev_hash: Hash32::ZERO, // unused: ancestors() stops here before reading this
                timestamp: checkpoint.timestamp,
                difficulty_target: checkpoint.difficulty_target,
                cumulative_work: checkpoint.cumulative_work,
            },
        );

        let mut canonical_index = BTreeMap::new();
        canonical_index.insert(checkpoint.height, checkpoint.hash);

        Chain {
            blocks: HashMap::new(),
            meta,
            tip_hash: checkpoint.hash,
            canonical_index,
            username_owner: checkpoint.username_owner.clone(),
            floor_hash: checkpoint.hash,
            floor_height: checkpoint.height,
            floor_username_owner: checkpoint.username_owner,
        }
    }

    pub fn tip_hash(&self) -> Hash32 {
        self.tip_hash
    }

    pub fn tip_height(&self) -> u64 {
        self.meta[&self.tip_hash].height
    }

    pub fn floor_height(&self) -> u64 {
        self.floor_height
    }

    /// The materialized state as of the current floor — the snapshot
    /// everything before it has been collapsed into.
    pub fn floor_username_owner(&self) -> &HashMap<String, UsernameOwner> {
        &self.floor_username_owner
    }

    pub fn get_block(&self, hash: &Hash32) -> Option<&Block> {
        self.blocks.get(hash)
    }

    pub fn get_meta(&self, hash: &Hash32) -> Option<&BlockMeta> {
        self.meta.get(hash)
    }

    pub fn canonical_block_at(&self, height: u64) -> Option<&Block> {
        let hash = self.canonical_index.get(&height)?;
        self.blocks.get(hash)
    }

    /// The current owner of `username` on the canonical chain, if any.
    pub fn username_owner(&self, username: &str) -> Option<&UsernameOwner> {
        self.username_owner.get(&normalize_username(username))
    }

    /// A checkpoint at the current tip — everything Phase 2 needs to
    /// prune all bodies at or before this height and still be able to
    /// restore an equivalent chain later via `from_checkpoint`.
    pub fn checkpoint_at_tip(&self) -> Checkpoint {
        let tip_meta = self.meta[&self.tip_hash];
        Checkpoint {
            height: tip_meta.height,
            hash: self.tip_hash,
            timestamp: tip_meta.timestamp,
            difficulty_target: tip_meta.difficulty_target,
            cumulative_work: tip_meta.cumulative_work,
            username_owner: self.username_owner.clone(),
        }
    }

    /// A checkpoint at any canonical `height` behind (or at) the tip —
    /// what real pruning uses, since it always targets a height well
    /// behind the tip (`tip_height - RETENTION_WINDOW`), never the tip
    /// itself.
    pub fn checkpoint_at(&self, height: u64) -> Result<Checkpoint> {
        let hash = *self.canonical_index.get(&height).ok_or(LedgerError::HeightNotFound(height))?;
        let meta = *self.meta.get(&hash).ok_or(LedgerError::UnknownBlock(hash))?;
        Ok(Checkpoint {
            height: meta.height,
            hash,
            timestamp: meta.timestamp,
            difficulty_target: meta.difficulty_target,
            cumulative_work: meta.cumulative_work,
            username_owner: self.state_at(&hash)?,
        })
    }

    /// Drops full bodies (but never metadata) for every block at or below
    /// `height`, moving the floor forward to `height` — after this,
    /// `state_at`/`ancestors` can no longer see anything before it.
    /// Computes the real materialized state as of `height` *before*
    /// dropping anything, so this is safe to call at any canonical height
    /// behind the tip, not just the tip itself (the realistic case: real
    /// pruning always targets a height well behind the current tip, e.g.
    /// `tip_height - RETENTION_WINDOW`, never the tip itself).
    pub fn prune_bodies_up_to(&mut self, height: u64) -> Result<()> {
        if height <= self.floor_height {
            return Ok(());
        }
        let Some(&new_floor_hash) = self.canonical_index.get(&height) else { return Ok(()) };
        let new_floor_state = self.state_at(&new_floor_hash)?;
        self.blocks.retain(|_, block| block.header.height > height);
        self.floor_username_owner = new_floor_state;
        self.floor_hash = new_floor_hash;
        self.floor_height = height;
        Ok(())
    }

    /// `hash`'s ancestry back to the current floor (inclusive), floor
    /// first, `hash` itself last — `ancestors(hash)[i]` holds the block at
    /// height `floor_height + i`, since every block has exactly one
    /// parent. Only reads `meta`, so this works even for a chain rebuilt
    /// from a checkpoint whose bodies aren't `blocks`-resident.
    pub fn ancestors(&self, hash: &Hash32) -> Result<Vec<Hash32>> {
        let mut path = Vec::new();
        let mut current = *hash;
        loop {
            let meta = self.meta.get(&current).ok_or(LedgerError::UnknownBlock(current))?;
            path.push(current);
            if current == self.floor_hash {
                break;
            }
            current = meta.prev_hash;
        }
        path.reverse();
        Ok(path)
    }

    /// The materialized `username -> owner` state as of (and including)
    /// `hash` — the floor snapshot, replayed forward through every block
    /// between the floor and `hash` that still has a body. Blocks already
    /// in `self.blocks` were fully validated before being accepted, so
    /// this just replays already-trusted history rather than re-checking
    /// anything.
    pub fn state_at(&self, hash: &Hash32) -> Result<HashMap<String, UsernameOwner>> {
        let mut state = self.floor_username_owner.clone();
        for ancestor_hash in self.ancestors(hash)? {
            if ancestor_hash == self.floor_hash {
                continue; // its effects are already folded into the floor snapshot
            }
            let block = self.blocks.get(&ancestor_hash).ok_or(LedgerError::UnknownBlock(ancestor_hash))?;
            for tx in &block.transactions {
                state.insert(
                    tx.username.clone(),
                    UsernameOwner {
                        owner_public_key: tx.owner_public_key,
                        claimed_at_height: block.header.height,
                        claimed_in_block: ancestor_hash,
                    },
                );
            }
        }
        Ok(state)
    }

    /// The median timestamp of the last `MEDIAN_TIME_PAST_WINDOW` blocks
    /// ending at (and including) `parent_hash` — a new block extending
    /// `parent_hash` must have a timestamp strictly after this, which
    /// resists a miner backdating a block to game the next retarget.
    pub fn median_time_past(&self, parent_hash: &Hash32) -> Result<u64> {
        let ancestry = self.ancestors(parent_hash)?;
        let window_start = ancestry.len().saturating_sub(MEDIAN_TIME_PAST_WINDOW);
        let mut timestamps: Vec<u64> =
            ancestry[window_start..].iter().map(|hash| self.meta[hash].timestamp).collect();
        timestamps.sort_unstable();
        Ok(timestamps[timestamps.len() / 2])
    }

    /// The `difficulty_target` a block extending `parent_hash` must have:
    /// unchanged unless the new height lands exactly on a retarget
    /// boundary, in which case it's recomputed from how long the just-
    /// completed interval actually took versus its target duration.
    pub fn expected_difficulty(&self, parent_hash: &Hash32) -> Result<CompactTarget> {
        use crate::difficulty::{retarget, RETARGET_INTERVAL_BLOCKS, TARGET_BLOCK_TIME_SECS};

        let parent_meta = *self.meta.get(parent_hash).ok_or(LedgerError::UnknownBlock(*parent_hash))?;
        let new_height = parent_meta.height + 1;

        if new_height % RETARGET_INTERVAL_BLOCKS != 0 {
            return Ok(parent_meta.difficulty_target);
        }

        let interval_start_height = new_height - RETARGET_INTERVAL_BLOCKS;
        let ancestry = self.ancestors(parent_hash)?; // floor..=parent, index i == height floor_height+i
        let idx = interval_start_height.checked_sub(self.floor_height).ok_or(LedgerError::UnknownBlock(*parent_hash))?;
        let interval_start_meta = self.meta[&ancestry[idx as usize]];

        let actual_span = (parent_meta.timestamp as i64 - interval_start_meta.timestamp as i64).max(1);
        let expected_span = RETARGET_INTERVAL_BLOCKS as i64 * TARGET_BLOCK_TIME_SECS;

        Ok(retarget(parent_meta.difficulty_target, actual_span, expected_span))
    }

    /// Validates and, if valid, accepts `block` — updating the canonical
    /// tip and `username_owner` if (and only if) this makes `block`'s
    /// chain the heaviest one known. `now` is passed in rather than read
    /// from the system clock so this stays fully deterministic and
    /// testable.
    pub fn try_apply(&mut self, block: Block, now: u64) -> Result<ApplyOutcome> {
        let hash = block.hash();
        if self.meta.contains_key(&hash) {
            return Ok(ApplyOutcome::AlreadyKnown);
        }

        validate_block(self, &block, now)?;

        let parent_hash = block.header.prev_hash;
        let parent_meta = *self
            .meta
            .get(&parent_hash)
            .expect("validate_block already confirmed the parent is known");
        let cumulative_work = parent_meta.cumulative_work + work_of(block.header.difficulty_target);
        let height = block.header.height;

        self.meta.insert(
            hash,
            BlockMeta {
                height,
                prev_hash: parent_hash,
                timestamp: block.header.timestamp,
                difficulty_target: block.header.difficulty_target,
                cumulative_work,
            },
        );
        self.blocks.insert(hash, block);

        let tip_work = self.meta[&self.tip_hash].cumulative_work;
        if cumulative_work <= tip_work {
            return Ok(ApplyOutcome::AddedToFork);
        }

        let extended_tip = parent_hash == self.tip_hash;
        self.set_canonical_tip(hash)?;

        if extended_tip {
            Ok(ApplyOutcome::ExtendedTip)
        } else {
            Ok(ApplyOutcome::ReorgedTo { new_height: height })
        }
    }

    fn set_canonical_tip(&mut self, hash: Hash32) -> Result<()> {
        let ancestry = self.ancestors(&hash)?;
        self.canonical_index.clear();
        for (offset, ancestor_hash) in ancestry.iter().enumerate() {
            self.canonical_index.insert(self.floor_height + offset as u64, *ancestor_hash);
        }
        self.username_owner = self.state_at(&hash)?;
        self.tip_hash = hash;
        Ok(())
    }
}

impl Default for Chain {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{identity, mine_block};
    use crate::transaction::Transaction;

    #[test]
    fn a_new_chain_starts_at_genesis() {
        let chain = Chain::new();
        assert_eq!(chain.tip_height(), 0);
        assert_eq!(chain.tip_hash(), Block::genesis().hash());
    }

    #[test]
    fn applying_genesis_again_is_a_no_op() {
        let mut chain = Chain::new();
        let outcome = chain.try_apply(Block::genesis(), 0).unwrap();
        assert_eq!(outcome, ApplyOutcome::AlreadyKnown);
        assert_eq!(chain.tip_height(), 0);
    }

    #[test]
    fn pruning_drops_bodies_but_keeps_meta_and_the_materialized_state() {
        let mut chain = Chain::new();
        let alice = identity(1);
        let now = 2_000_000_000;

        let tx = Transaction::new_claim(&alice, "alice", 1, chain.tip_hash(), [1; 8]).unwrap();
        let block_one = mine_block(&chain, vec![tx], now);
        let block_one_hash = block_one.hash();
        chain.try_apply(block_one, now).unwrap();

        let block_two = mine_block(&chain, vec![], now + 400);
        chain.try_apply(block_two, now + 400).unwrap();

        chain.prune_bodies_up_to(1).unwrap();

        assert!(chain.get_block(&block_one_hash).is_none(), "pruned body must be gone");
        assert!(chain.get_meta(&block_one_hash).is_some(), "meta must survive pruning");
        assert_eq!(
            chain.username_owner("alice").unwrap().owner_public_key,
            alice.public_key().to_bytes(),
            "materialized state must survive pruning its own evidence"
        );
        assert_eq!(chain.floor_height(), 1);
    }

    #[test]
    fn pruning_well_behind_the_tip_still_computes_the_correct_state_at_that_height() {
        // The realistic case: pruning always targets a height far behind
        // the current tip, never the tip itself — this is what the buggy
        // first version of `prune_bodies_up_to` (which reused the *tip's*
        // materialized state regardless of the target height) would have
        // gotten wrong.
        let mut chain = Chain::new();
        let alice = identity(1);
        let bob = identity(2);
        let now = 2_000_000_000;

        let claim_alice = Transaction::new_claim(&alice, "alice", 1, chain.tip_hash(), [1; 8]).unwrap();
        let block_one = mine_block(&chain, vec![claim_alice], now);
        chain.try_apply(block_one, now).unwrap();
        let checkpoint_one = chain.checkpoint_at(1).unwrap();

        // Several more blocks land after height 1, including a second,
        // unrelated claim — none of this should affect what "the state at
        // height 1" was.
        for i in 1..5u64 {
            let claim = if i == 2 {
                vec![Transaction::new_claim(&bob, "bobby", i + 1, chain.tip_hash(), [2; 8]).unwrap()]
            } else {
                vec![]
            };
            let block = mine_block(&chain, claim, now + i * 400);
            chain.try_apply(block, now + i * 400).unwrap();
        }
        assert_eq!(chain.tip_height(), 5);

        chain.prune_bodies_up_to(1).unwrap();

        // The checkpoint computed *before* pruning (state as of height 1)
        // must match what pruning itself computed for the same height.
        assert_eq!(checkpoint_one.username_owner.get("alice"), chain.floor_username_owner().get("alice"));
        assert!(
            !chain.floor_username_owner().contains_key("bobby"),
            "bob's claim (height 3) is after the floor (height 1) and must not have leaked into it"
        );
        // The live tip state must still reflect everything, pruning or not.
        assert!(chain.username_owner("bobby").is_some());
        assert!(chain.username_owner("alice").is_some());
    }

    #[test]
    fn a_chain_restored_from_a_checkpoint_matches_the_original_at_that_height() {
        let mut chain = Chain::new();
        let alice = identity(1);
        let now = 2_000_000_000;

        let tx = Transaction::new_claim(&alice, "alice", 1, chain.tip_hash(), [1; 8]).unwrap();
        let block_one = mine_block(&chain, vec![tx], now);
        chain.try_apply(block_one, now).unwrap();

        let checkpoint = chain.checkpoint_at_tip();
        let restored = Chain::from_checkpoint(checkpoint);

        assert_eq!(restored.tip_height(), chain.tip_height());
        assert_eq!(restored.tip_hash(), chain.tip_hash());
        assert_eq!(
            restored.username_owner("alice").unwrap().owner_public_key,
            alice.public_key().to_bytes(),
        );
    }

    #[test]
    fn a_restored_chain_can_keep_accepting_new_blocks() {
        let mut chain = Chain::new();
        let alice = identity(1);
        let bob = identity(2);
        let now = 2_000_000_000;

        let tx = Transaction::new_claim(&alice, "alice", 1, chain.tip_hash(), [1; 8]).unwrap();
        let block_one = mine_block(&chain, vec![tx], now);
        chain.try_apply(block_one, now).unwrap();

        let mut restored = Chain::from_checkpoint(chain.checkpoint_at_tip());

        let tx2 = Transaction::new_claim(&bob, "bobby", 2, restored.tip_hash(), [2; 8]).unwrap();
        let block_two = mine_block(&restored, vec![tx2], now + 400);
        let outcome = restored.try_apply(block_two, now + 400).unwrap();

        assert_eq!(outcome, ApplyOutcome::ExtendedTip);
        assert_eq!(restored.tip_height(), 2);
        assert_eq!(restored.username_owner("alice").unwrap().owner_public_key, alice.public_key().to_bytes());
        assert_eq!(restored.username_owner("bobby").unwrap().owner_public_key, bob.public_key().to_bytes());
    }

    #[test]
    fn pruning_below_the_current_floor_is_a_no_op() {
        let mut chain = Chain::new();
        chain.prune_bodies_up_to(0).unwrap(); // floor is already 0 — nothing to do
        assert_eq!(chain.floor_height(), 0);
        assert!(chain.get_block(&chain.tip_hash()).is_some());
    }
}
