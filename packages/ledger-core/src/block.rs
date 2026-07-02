//! Block header, hashing, and the hardcoded genesis block. No Merkle tree:
//! block bodies are always shipped in full over gossip (nobody needs a
//! partial-inclusion proof without also having the whole block, given how
//! small these blocks are), so a flat ordered-hash commitment over
//! transaction ids is enough — a Merkle tree would solve a problem this
//! design doesn't have.

use serde::{Deserialize, Serialize};

use crate::difficulty::{CompactTarget, INITIAL_DIFFICULTY_BITS};
use crate::hash::Hash32;
use crate::transaction::Transaction;

pub const MAX_TXS_PER_BLOCK: usize = 64;
pub const MAX_BLOCK_BYTES: usize = 16 * 1024;

/// Fixed at compile time — every node agrees on this out of band, the same
/// way every Bitcoin client has genesis hardcoded. Never validated, only
/// matched against. Deliberately set safely in the *past* relative to
/// this chain's real launch, not just "some round number" — a genesis
/// timestamp that turns out to be in the future relative to a real
/// device's clock would make every block's median-time-past check
/// (`timestamp > median of last 11 ancestors`) reject any honestly-timed
/// first block, since a real "now" would then be *earlier* than genesis
/// itself.
pub const GENESIS_TIMESTAMP: u64 = 1_750_000_000; // 2025-06-15T08:40:00Z

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockHeader {
    pub version: u8,
    pub height: u64,
    pub prev_hash: Hash32,
    pub timestamp: u64,
    pub tx_commitment: Hash32,
    pub difficulty_target: CompactTarget,
    pub nonce: u64,
    pub miner_public_key: [u8; 32],
}

impl BlockHeader {
    /// The exact byte layout hashed for proof-of-work — every field, in a
    /// fixed order. Deliberately hand-rolled rather than going through
    /// `serde`: hash-critical canonical bytes must never depend on a
    /// serialization library's own (potentially format-version-dependent)
    /// encoding choices.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(1 + 8 + 32 + 8 + 32 + 4 + 8 + 32);
        buf.push(self.version);
        buf.extend_from_slice(&self.height.to_le_bytes());
        buf.extend_from_slice(self.prev_hash.as_bytes());
        buf.extend_from_slice(&self.timestamp.to_le_bytes());
        buf.extend_from_slice(self.tx_commitment.as_bytes());
        buf.extend_from_slice(&self.difficulty_target.to_le_bytes());
        buf.extend_from_slice(&self.nonce.to_le_bytes());
        buf.extend_from_slice(&self.miner_public_key);
        buf
    }

    pub fn hash(&self) -> Hash32 {
        Hash32::of(&self.canonical_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
}

impl Block {
    pub fn hash(&self) -> Hash32 {
        self.header.hash()
    }

    /// SHA256 over the sorted, concatenated transaction ids.
    pub fn compute_tx_commitment(transactions: &[Transaction]) -> Hash32 {
        let mut ids: Vec<Hash32> = transactions.iter().map(Transaction::id).collect();
        ids.sort();
        let mut buf = Vec::with_capacity(ids.len() * 32);
        for id in &ids {
            buf.extend_from_slice(id.as_bytes());
        }
        Hash32::of(&buf)
    }

    /// The size check used for block-size validation. Not necessarily
    /// byte-identical to whatever wire format the networking layer ends up
    /// using (that's a gossip-transport concern, not a consensus one) —
    /// this only needs to be a stable, deterministic proxy every node
    /// computes the same way.
    pub fn approx_size_bytes(&self) -> usize {
        self.header.canonical_bytes().len()
            + self.transactions.iter().map(|tx| tx.canonical_bytes().len()).sum::<usize>()
    }

    /// The one block every node trusts axiomatically — `validation.rs`
    /// special-cases height 0 to match this exactly rather than running
    /// it through the normal proof-of-work/parent/timestamp checks (there
    /// is no parent to check against, and no PoW to require of a block
    /// that predates the network having any hashrate at all — every real
    /// blockchain's genesis is trusted the same way, not mined under its
    /// own rules).
    pub fn genesis() -> Block {
        let header = BlockHeader {
            version: 1,
            height: 0,
            prev_hash: Hash32::ZERO,
            timestamp: GENESIS_TIMESTAMP,
            tx_commitment: Block::compute_tx_commitment(&[]),
            difficulty_target: INITIAL_DIFFICULTY_BITS,
            nonce: 0,
            miner_public_key: [0u8; 32],
        };
        Block { header, transactions: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_has_a_stable_hash() {
        assert_eq!(Block::genesis().hash(), Block::genesis().hash());
    }

    #[test]
    fn genesis_prev_hash_is_zero() {
        assert_eq!(Block::genesis().header.prev_hash, Hash32::ZERO);
    }

    #[test]
    fn tx_commitment_is_order_independent() {
        // Sorting before hashing means the same set of transactions in a
        // different gossip-arrival order still produces the same block.
        use rand_chacha::rand_core::SeedableRng;
        use rand_chacha::ChaCha20Rng;
        use spiritchat_crypto_core::identity::IdentityKeyPair;

        let identity = IdentityKeyPair::generate(&mut ChaCha20Rng::seed_from_u64(1));
        let a = Transaction::new_claim(&identity, "alice", 0, Hash32::ZERO, [1; 8]).unwrap();
        let b = Transaction::new_claim(&identity, "bobby", 0, Hash32::ZERO, [2; 8]).unwrap();

        let forward = Block::compute_tx_commitment(&[a.clone(), b.clone()]);
        let backward = Block::compute_tx_commitment(&[b, a]);
        assert_eq!(forward, backward);
    }
}
