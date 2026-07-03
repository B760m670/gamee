//! Serverless offline delivery: when a direct `SendEnvelope` genuinely
//! fails (the recipient isn't reachable right now), the sender can instead
//! gossip an already-encrypted envelope, stamped with a small
//! proof-of-work, for *any* node to temporarily hold and hand off once it
//! becomes reachable — the recipient no longer needs to be online at the
//! exact moment of sending, just at some point before the stamp expires.
//!
//! Deliberately **not** the `@username` ledger's block/mining machinery —
//! that chain is tuned for a low-volume namespace-claim workload (5-minute
//! blocks, a small per-block cap); routing chat messages through it would
//! make delivery slower, not faster, and would misuse infrastructure built
//! for a different job. This is a Hashcash-style stamp instead — proof of
//! work's original, pre-Bitcoin use: cheap to verify, deliberately
//! expensive to produce in bulk, just enough to make flooding the network
//! with junk deposits costly without needing any globally-agreed
//! difficulty or consensus. A caching node still validates every deposit
//! (PoW, size, timestamp) before storing it, whether or not it has chosen
//! to relay for others — otherwise the stamp gates nothing.
//!
//! Delivery from a caching node needs no new receive-side protocol at all:
//! it just calls the existing `Command::SendEnvelope` once it can reach
//! the recipient, and `EnvelopeReceived` on their end looks identical to a
//! direct send, since the bytes are the same untouched, already-encrypted
//! envelope this crate never inspects either way.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use libp2p::gossipsub::IdentTopic;
use libp2p::PeerId;
use redb::{Database, MultimapTableDefinition, ReadableMultimapTable, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{P2pError, Result};

/// Every node subscribes — accepting (or rejecting) a deposit is a purely
/// local decision each node makes from its own validation, not something
/// that needs a request/response round trip.
pub fn deposit_topic() -> IdentTopic {
    IdentTopic::new("/spiritchat/mailbox/1")
}

/// How many leading zero bits a deposit's PoW hash must have. Tuned like
/// the `@username` ledger's own initial difficulty was: enough that a
/// phone CPU needs on the order of 2^20 attempts (well under a couple of
/// seconds, pure-Rust SHA-256, no SIMD assumed) — trivial for one
/// legitimate failed message, deliberately annoying at bulk-spam volume.
/// Fixed, not retargeted: unlike the ledger, this doesn't need
/// globally-agreed difficulty, just a locally-checkable floor every node
/// applies the same way.
pub const MAILBOX_POW_LEADING_ZERO_BITS: u32 = 20;

/// Deposits older (or newer) than this relative to a validating node's own
/// clock are rejected outright — mirrors the `@username` ledger's own
/// future-drift guard against a forged timestamp being used to dodge
/// eviction ordering.
pub const MAX_CLOCK_DRIFT_SECS: u64 = 2 * 60 * 60;

/// Same ceiling `ChatManager`'s framing already implies for a single
/// envelope in practice — generous for text, and a deliberate cap on how
/// much of a stranger's storage/bandwidth one deposit can spend.
pub const MAX_ENVELOPE_BYTES: usize = 64 * 1024;

/// How long an accepted deposit is kept before it's evicted regardless of
/// cache pressure — long enough that "the recipient was offline for a
/// week" is still a normal, recoverable case, not indefinite (unlike a
/// block header, a stale queued message stops being useful eventually).
pub const RETENTION_SECS: u64 = 14 * 24 * 60 * 60;

/// Default total size every node budgets for *other people's* deposits —
/// runtime-adjustable (`Command::SetMailboxCacheLimitBytes`), not a hard
/// compile-time ceiling, since Settings needs to change it live.
pub const DEFAULT_CACHE_LIMIT_BYTES: u64 = 50 * 1024 * 1024;

/// What travels over gossip. `envelope` is exactly what `SendEnvelope`
/// already carries — no format change, and this layer never sees
/// plaintext here either, same as the direct-send path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxDeposit {
    pub recipient: Vec<u8>, // PeerId::to_bytes()
    pub envelope: Vec<u8>,
    pub deposited_at: u64,
    pub pow_nonce: [u8; 8],
}

pub fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock is before 1970").as_secs()
}

fn pow_preimage(recipient: &[u8], envelope: &[u8], deposited_at: u64, nonce: [u8; 8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 + recipient.len() + 32 + 8 + 8);
    out.extend_from_slice(b"spiritchat-mailbox-pow-v1");
    out.extend_from_slice(recipient);
    out.extend_from_slice(&Sha256::digest(envelope));
    out.extend_from_slice(&deposited_at.to_le_bytes());
    out.extend_from_slice(&nonce);
    out
}

fn pow_hash(recipient: &[u8], envelope: &[u8], deposited_at: u64, nonce: [u8; 8]) -> [u8; 32] {
    Sha256::digest(pow_preimage(recipient, envelope, deposited_at, nonce)).into()
}

fn leading_zero_bits(hash: &[u8; 32]) -> u32 {
    let mut count = 0;
    for byte in hash {
        if *byte == 0 {
            count += 8;
            continue;
        }
        count += byte.leading_zeros();
        break;
    }
    count
}

fn meets_pow(hash: &[u8; 32]) -> bool {
    leading_zero_bits(hash) >= MAILBOX_POW_LEADING_ZERO_BITS
}

/// Grinds nonces until `deposit`'s PoW stamp is valid — pure CPU work,
/// meant to run on a blocking thread (mirrors the ledger's own mining
/// loop in `node.rs`), never on the async event-loop thread. Deliberately
/// uses whatever `deposited_at` the caller already fixed, rather than
/// refreshing it mid-search: unlike block mining (which can run
/// indefinitely and needs a fresh timestamp to stay valid), this always
/// finishes in well under a second at the configured difficulty, so a
/// stale timestamp is never a real concern.
pub fn mine_stamp(recipient: &PeerId, envelope: &[u8], deposited_at: u64) -> [u8; 8] {
    let recipient_bytes = recipient.to_bytes();
    let mut nonce: u64 = 0;
    loop {
        let candidate = nonce.to_le_bytes();
        if meets_pow(&pow_hash(&recipient_bytes, envelope, deposited_at, candidate)) {
            return candidate;
        }
        nonce = nonce.wrapping_add(1);
    }
}

/// Structural + PoW validation only — does not check whether this node
/// has *room* for the deposit (that's `MailboxStore::accept`'s job, since
/// it needs to know current cache usage). Applied uniformly to every
/// incoming deposit, whether or not this node has relaying enabled: a node
/// with relaying off still needs to validate before dropping it, and
/// gossipsub itself has nothing to gate on otherwise.
pub fn validate(deposit: &MailboxDeposit, now: u64) -> Result<PeerId> {
    let recipient = PeerId::from_bytes(&deposit.recipient)
        .map_err(|err| P2pError::Mailbox(format!("malformed recipient peer id: {err}")))?;

    if deposit.envelope.is_empty() || deposit.envelope.len() > MAX_ENVELOPE_BYTES {
        return Err(P2pError::Mailbox("envelope size out of bounds".into()));
    }

    let drift = deposit.deposited_at.abs_diff(now);
    if drift > MAX_CLOCK_DRIFT_SECS {
        return Err(P2pError::Mailbox("deposit timestamp too far from local clock".into()));
    }

    let hash = pow_hash(&deposit.recipient, &deposit.envelope, deposit.deposited_at, deposit.pow_nonce);
    if !meets_pow(&hash) {
        return Err(P2pError::Mailbox("proof-of-work stamp does not meet the required difficulty".into()));
    }

    Ok(recipient)
}

// --- Storage -----------------------------------------------------------

/// deposit_key = deposited_at (8-byte BE, sorts oldest-first for free) ++
/// SHA-256(recipient || envelope || pow_nonce) (32 bytes) -> bincode(StoredDeposit).
/// Keying by insertion time this way means "evict the oldest" is just
/// "delete the first key" — no separate ordering index needed, the same
/// trick `ledger-core::store` uses keying blocks by height.
const DEPOSITS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("mailbox_deposits");
/// recipient_peer_id_bytes -> deposit_key — answers "what's queued for
/// this recipient" and "which distinct recipients do we have anything
/// for" (the periodic delivery sweep's own question) without scanning
/// every deposit.
const BY_RECIPIENT: MultimapTableDefinition<&[u8], &[u8]> = MultimapTableDefinition::new("mailbox_by_recipient");
/// Single-row running total of stored envelope bytes, maintained
/// incrementally so enforcing the cache cap never needs a full table scan.
const SINGLETON: TableDefinition<&str, u64> = TableDefinition::new("mailbox_singleton");
const TOTAL_BYTES_KEY: &str = "total_bytes";

#[derive(Serialize, Deserialize)]
struct StoredDeposit {
    recipient: Vec<u8>,
    envelope: Vec<u8>,
}

fn to_storage_err(err: impl std::fmt::Display) -> P2pError {
    P2pError::Mailbox(err.to_string())
}

fn deposit_key(deposit: &MailboxDeposit) -> [u8; 40] {
    let mut key = [0u8; 40];
    key[0..8].copy_from_slice(&deposit.deposited_at.to_be_bytes());
    let digest = Sha256::digest(pow_preimage(&deposit.recipient, &deposit.envelope, deposit.deposited_at, deposit.pow_nonce));
    key[8..40].copy_from_slice(&digest);
    key
}

/// A single accepted deposit, as returned to a caller walking what's
/// queued for a recipient (e.g. the periodic delivery sweep).
pub struct CachedDeposit {
    pub key: Vec<u8>,
    pub envelope: Vec<u8>,
}

pub struct MailboxStore {
    db: Database,
    limit_bytes: u64,
}

impl MailboxStore {
    /// Opens (or creates) the mailbox cache at `path` — a dedicated file,
    /// sibling to the `@username` ledger's own (see `LedgerDataDir.swift`'s
    /// convention), never shared with it: this is per-node cache state,
    /// not agreed-upon chain data.
    pub fn open(path: &Path) -> Result<Self> {
        let db = Database::create(path).map_err(to_storage_err)?;
        {
            let tx = db.begin_write().map_err(to_storage_err)?;
            tx.open_table(DEPOSITS).map_err(to_storage_err)?;
            tx.open_multimap_table(BY_RECIPIENT).map_err(to_storage_err)?;
            tx.open_table(SINGLETON).map_err(to_storage_err)?;
            tx.commit().map_err(to_storage_err)?;
        }
        Ok(MailboxStore { db, limit_bytes: DEFAULT_CACHE_LIMIT_BYTES })
    }

    pub fn set_limit_bytes(&mut self, limit: u64) {
        self.limit_bytes = limit;
    }

    fn total_bytes(&self) -> Result<u64> {
        let tx = self.db.begin_read().map_err(to_storage_err)?;
        let table = tx.open_table(SINGLETON).map_err(to_storage_err)?;
        Ok(table.get(TOTAL_BYTES_KEY).map_err(to_storage_err)?.map(|v| v.value()).unwrap_or(0))
    }

    /// Stores an already-`validate`d deposit, evicting the oldest entries
    /// first if it would push total usage over `limit_bytes` — including,
    /// if necessary, evicting older bytes to make room for this one
    /// (first-come-first-served isn't the goal; keeping the cache within
    /// its promised budget is).
    pub fn accept(&mut self, deposit: MailboxDeposit) -> Result<()> {
        let key = deposit_key(&deposit);
        let stored = StoredDeposit { recipient: deposit.recipient.clone(), envelope: deposit.envelope };
        let bytes = bincode::serialize(&stored).map_err(to_storage_err)?;
        let added = bytes.len() as u64;

        self.evict_until_room_for(added)?;

        let tx = self.db.begin_write().map_err(to_storage_err)?;
        {
            let mut deposits = tx.open_table(DEPOSITS).map_err(to_storage_err)?;
            deposits.insert(&key[..], bytes.as_slice()).map_err(to_storage_err)?;
            let mut by_recipient = tx.open_multimap_table(BY_RECIPIENT).map_err(to_storage_err)?;
            by_recipient.insert(stored.recipient.as_slice(), &key[..]).map_err(to_storage_err)?;
            let mut singleton = tx.open_table(SINGLETON).map_err(to_storage_err)?;
            let current = singleton.get(TOTAL_BYTES_KEY).map_err(to_storage_err)?.map(|v| v.value()).unwrap_or(0);
            singleton.insert(TOTAL_BYTES_KEY, current + added).map_err(to_storage_err)?;
        }
        tx.commit().map_err(to_storage_err)?;
        Ok(())
    }

    fn evict_until_room_for(&mut self, additional: u64) -> Result<()> {
        loop {
            let total = self.total_bytes()?;
            if total + additional <= self.limit_bytes {
                return Ok(());
            }
            if !self.evict_oldest()? {
                // Nothing left to evict but still over budget (a single
                // deposit larger than the whole cap) — accept it anyway
                // rather than refuse a validated, PoW-paid-for deposit
                // outright; the next `accept` will simply evict it first.
                return Ok(());
            }
        }
    }

    /// Deletes the single oldest deposit (lowest key = earliest
    /// `deposited_at`). Returns `false` if the cache was already empty.
    fn evict_oldest(&mut self) -> Result<bool> {
        let tx = self.db.begin_write().map_err(to_storage_err)?;
        let oldest = {
            let deposits = tx.open_table(DEPOSITS).map_err(to_storage_err)?;
            let Some(first) = deposits.iter().map_err(to_storage_err)?.next() else {
                return Ok(false);
            };
            let (key, value) = first.map_err(to_storage_err)?;
            let key = key.value().to_vec();
            let stored: StoredDeposit = bincode::deserialize(value.value()).map_err(to_storage_err)?;
            (key, stored)
        };
        self.remove_locked(&tx, &oldest.0, &oldest.1.recipient, oldest.1.envelope.len() as u64)?;
        tx.commit().map_err(to_storage_err)?;
        Ok(true)
    }

    fn remove_locked(&self, tx: &redb::WriteTransaction, key: &[u8], recipient: &[u8], envelope_len: u64) -> Result<()> {
        let mut deposits = tx.open_table(DEPOSITS).map_err(to_storage_err)?;
        deposits.remove(key).map_err(to_storage_err)?;
        let mut by_recipient = tx.open_multimap_table(BY_RECIPIENT).map_err(to_storage_err)?;
        by_recipient.remove(recipient, key).map_err(to_storage_err)?;
        let mut singleton = tx.open_table(SINGLETON).map_err(to_storage_err)?;
        let current = singleton.get(TOTAL_BYTES_KEY).map_err(to_storage_err)?.map(|v| v.value()).unwrap_or(0);
        // StoredDeposit's serialized size includes a little bincode
        // overhead beyond just the envelope; tracking the exact stored
        // byte count would need reading it back, which `evict_oldest`
        // already did — `envelope_len` here is a close enough proxy for
        // `accept`'s single-deposit removal path below, where exactness
        // matters less than never underflowing.
        singleton.insert(TOTAL_BYTES_KEY, current.saturating_sub(envelope_len)).map_err(to_storage_err)?;
        Ok(())
    }

    /// Removes one specific deposit by its key — for delivery success
    /// ("evict once `EnvelopeDelivered` confirms it"). A no-op if `key`
    /// isn't present (already delivered/evicted by something else).
    pub fn remove(&mut self, key: &[u8]) -> Result<()> {
        let tx = self.db.begin_write().map_err(to_storage_err)?;
        let existing: Option<Vec<u8>> = {
            let deposits = tx.open_table(DEPOSITS).map_err(to_storage_err)?;
            let value = deposits.get(key).map_err(to_storage_err)?;
            let bytes = value.as_ref().map(|v| v.value().to_vec());
            bytes
        };
        let Some(bytes) = existing else {
            return Ok(());
        };
        let stored: StoredDeposit = bincode::deserialize(&bytes).map_err(to_storage_err)?;
        self.remove_locked(&tx, key, &stored.recipient, stored.envelope.len() as u64)?;
        tx.commit().map_err(to_storage_err)?;
        Ok(())
    }

    /// Every deposit currently cached for `recipient`, oldest first —
    /// what the delivery sweep sends, in order, when it can reach them.
    pub fn for_recipient(&self, recipient: &PeerId) -> Result<Vec<CachedDeposit>> {
        let tx = self.db.begin_read().map_err(to_storage_err)?;
        let by_recipient = tx.open_multimap_table(BY_RECIPIENT).map_err(to_storage_err)?;
        let deposits = tx.open_table(DEPOSITS).map_err(to_storage_err)?;

        let mut keys: Vec<Vec<u8>> = Vec::new();
        for entry in by_recipient.get(recipient.to_bytes().as_slice()).map_err(to_storage_err)? {
            keys.push(entry.map_err(to_storage_err)?.value().to_vec());
        }
        keys.sort();

        let mut out = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some(value) = deposits.get(key.as_slice()).map_err(to_storage_err)? {
                let stored: StoredDeposit = bincode::deserialize(value.value()).map_err(to_storage_err)?;
                out.push(CachedDeposit { key, envelope: stored.envelope });
            }
        }
        Ok(out)
    }

    /// Every distinct recipient this node currently has at least one
    /// deposit cached for — what the periodic delivery sweep iterates to
    /// decide who to try dialing.
    pub fn distinct_recipients(&self) -> Result<Vec<PeerId>> {
        let tx = self.db.begin_read().map_err(to_storage_err)?;
        let by_recipient = tx.open_multimap_table(BY_RECIPIENT).map_err(to_storage_err)?;
        let mut out = Vec::new();
        for entry in by_recipient.iter().map_err(to_storage_err)? {
            let (recipient, _) = entry.map_err(to_storage_err)?;
            if let Ok(peer) = PeerId::from_bytes(recipient.value()) {
                out.push(peer);
            }
        }
        out.dedup();
        Ok(out)
    }

    /// Deletes every deposit older than `RETENTION_SECS` relative to
    /// `now` — run periodically by the same sweep that drives delivery
    /// retries, so an unreachable recipient's mail doesn't sit forever.
    pub fn evict_expired(&mut self, now: u64) -> Result<()> {
        loop {
            let tx = self.db.begin_write().map_err(to_storage_err)?;
            let expired = {
                let deposits = tx.open_table(DEPOSITS).map_err(to_storage_err)?;
                let Some(first) = deposits.iter().map_err(to_storage_err)?.next() else {
                    return Ok(());
                };
                let (key, value) = first.map_err(to_storage_err)?;
                let key_bytes = key.value();
                let deposited_at = u64::from_be_bytes(key_bytes[0..8].try_into().unwrap());
                if now.saturating_sub(deposited_at) <= RETENTION_SECS {
                    return Ok(());
                }
                let stored: StoredDeposit = bincode::deserialize(value.value()).map_err(to_storage_err)?;
                (key_bytes.to_vec(), stored)
            };
            self.remove_locked(&tx, &expired.0, &expired.1.recipient, expired.1.envelope.len() as u64)?;
            tx.commit().map_err(to_storage_err)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_peer(seed: u8) -> PeerId {
        let mut bytes = [0u8; 32];
        bytes[0] = seed;
        let keypair = libp2p::identity::Keypair::ed25519_from_bytes(bytes).unwrap();
        PeerId::from(keypair.public())
    }

    fn make_valid_deposit(recipient: &PeerId, envelope: &[u8], deposited_at: u64) -> MailboxDeposit {
        let pow_nonce = mine_stamp(recipient, envelope, deposited_at);
        MailboxDeposit { recipient: recipient.to_bytes(), envelope: envelope.to_vec(), deposited_at, pow_nonce }
    }

    #[test]
    fn a_mined_stamp_passes_validation() {
        let recipient = test_peer(1);
        let deposit = make_valid_deposit(&recipient, b"hello", now_unix());
        assert_eq!(validate(&deposit, now_unix()).unwrap(), recipient);
    }

    #[test]
    fn a_forged_stamp_is_rejected() {
        let recipient = test_peer(1);
        let mut deposit = make_valid_deposit(&recipient, b"hello", now_unix());
        deposit.pow_nonce = [0u8; 8];
        assert!(validate(&deposit, now_unix()).is_err());
    }

    #[test]
    fn an_oversized_envelope_is_rejected_even_with_a_valid_stamp() {
        let recipient = test_peer(1);
        let big = vec![0u8; MAX_ENVELOPE_BYTES + 1];
        // Not worth actually mining a stamp for an envelope this large in
        // a test — the size check must fail before the PoW check ever runs.
        let deposit = MailboxDeposit { recipient: recipient.to_bytes(), envelope: big, deposited_at: now_unix(), pow_nonce: [0; 8] };
        assert!(validate(&deposit, now_unix()).is_err());
    }

    #[test]
    fn a_deposit_far_in_the_future_is_rejected() {
        let recipient = test_peer(1);
        let deposit = make_valid_deposit(&recipient, b"hello", now_unix() + MAX_CLOCK_DRIFT_SECS + 1000);
        assert!(validate(&deposit, now_unix()).is_err());
    }

    #[test]
    fn accepted_deposits_round_trip_through_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MailboxStore::open(&dir.path().join("mailbox.redb")).unwrap();
        let recipient = test_peer(1);
        let deposit = make_valid_deposit(&recipient, b"hello", now_unix());

        store.accept(deposit).unwrap();

        let queued = store.for_recipient(&recipient).unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].envelope, b"hello");
        assert_eq!(store.distinct_recipients().unwrap(), vec![recipient]);
    }

    #[test]
    fn delivering_a_deposit_removes_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MailboxStore::open(&dir.path().join("mailbox.redb")).unwrap();
        let recipient = test_peer(1);
        let deposit = make_valid_deposit(&recipient, b"hello", now_unix());
        store.accept(deposit).unwrap();

        let key = store.for_recipient(&recipient).unwrap()[0].key.clone();
        store.remove(&key).unwrap();

        assert!(store.for_recipient(&recipient).unwrap().is_empty());
        assert!(store.distinct_recipients().unwrap().is_empty());
    }

    #[test]
    fn the_oldest_deposit_is_evicted_first_once_over_the_cache_limit() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MailboxStore::open(&dir.path().join("mailbox.redb")).unwrap();
        let recipient = test_peer(1);

        let first = make_valid_deposit(&recipient, b"first message body", now_unix());
        let first_size = bincode::serialize(&StoredDeposit { recipient: first.recipient.clone(), envelope: first.envelope.clone() }).unwrap().len() as u64;
        store.set_limit_bytes(first_size); // room for exactly one deposit like `first`
        store.accept(first).unwrap();

        let second = make_valid_deposit(&recipient, b"second message body!", now_unix() + 1);
        store.accept(second).unwrap();

        let queued = store.for_recipient(&recipient).unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].envelope, b"second message body!");
    }

    #[test]
    fn expired_deposits_are_evicted() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MailboxStore::open(&dir.path().join("mailbox.redb")).unwrap();
        let recipient = test_peer(1);
        let old_at = now_unix() - RETENTION_SECS - 10;
        let deposit = make_valid_deposit(&recipient, b"stale", old_at);
        store.accept(deposit).unwrap();

        store.evict_expired(now_unix()).unwrap();

        assert!(store.for_recipient(&recipient).unwrap().is_empty());
    }
}
