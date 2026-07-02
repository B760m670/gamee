//! Shared test-only fixtures — a deterministic test identity and a
//! brute-force miner standing in for the real mining loop (Phase 5),
//! used by more than one module's test suite.

#![cfg(test)]

use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha20Rng;
use spiritchat_crypto_core::identity::IdentityKeyPair;

use crate::block::{Block, BlockHeader};
use crate::chain_state::Chain;
use crate::difficulty::expand_target;
use crate::transaction::Transaction;

pub fn identity(seed: u64) -> IdentityKeyPair {
    IdentityKeyPair::generate(&mut ChaCha20Rng::seed_from_u64(seed))
}

/// Mines a valid block extending `chain`'s current tip with
/// `transactions`, by brute-forcing a nonce.
pub fn mine_block(chain: &Chain, transactions: Vec<Transaction>, timestamp: u64) -> Block {
    let parent_hash = chain.tip_hash();
    let parent_meta = *chain.get_meta(&parent_hash).unwrap();
    let difficulty_target = chain.expected_difficulty(&parent_hash).unwrap();
    let target = expand_target(difficulty_target);

    let mut header = BlockHeader {
        version: 1,
        height: parent_meta.height + 1,
        prev_hash: parent_hash,
        timestamp,
        tx_commitment: Block::compute_tx_commitment(&transactions),
        difficulty_target,
        nonce: 0,
        miner_public_key: [0u8; 32],
    };
    loop {
        if header.hash().meets_target(&target) {
            break;
        }
        header.nonce += 1;
    }
    Block { header, transactions }
}
