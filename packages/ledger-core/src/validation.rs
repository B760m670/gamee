//! Every rule a block must satisfy to be accepted — a pure function of
//! `(chain, candidate block, current time)`, called by
//! `Chain::try_apply` before it mutates anything. Kept separate from
//! `chain_state.rs` so the *rules* and the *bookkeeping* (fork tracking,
//! reorg, materialized state) can be read and reasoned about
//! independently.

use std::collections::HashSet;

use crate::block::{Block, MAX_BLOCK_BYTES, MAX_TXS_PER_BLOCK};
use crate::chain_state::Chain;
use crate::difficulty::expand_target;
use crate::error::{LedgerError, Result};
use crate::transaction::Transaction;

/// How old a transaction's `anchor_block_hash` is allowed to be relative
/// to the block including it — roughly 12h at the 5-minute target block
/// time. Bounds how long a signed-but-unmined claim stays valid to
/// include, so a very stale claim can't suddenly reappear and collide
/// with something claimed in the meantime.
pub const ANCHOR_MAX_AGE_BLOCKS: u64 = 144;

/// How far into the future a block's timestamp may claim to be, to allow
/// for reasonable clock drift between phones without letting a miner
/// timestamp a block arbitrarily far ahead (which would otherwise let them
/// manipulate the next retarget).
pub const MAX_FUTURE_DRIFT_SECS: u64 = 2 * 60 * 60;

pub fn validate_block(chain: &Chain, block: &Block, now: u64) -> Result<()> {
    if block.transactions.len() > MAX_TXS_PER_BLOCK {
        return Err(LedgerError::TooManyTransactions {
            actual: block.transactions.len(),
            max: MAX_TXS_PER_BLOCK,
        });
    }
    let size = block.approx_size_bytes();
    if size > MAX_BLOCK_BYTES {
        return Err(LedgerError::BlockTooLarge { actual: size, max: MAX_BLOCK_BYTES });
    }
    if block.header.tx_commitment != Block::compute_tx_commitment(&block.transactions) {
        return Err(LedgerError::TxCommitmentMismatch);
    }

    let parent_meta = chain
        .get_meta(&block.header.prev_hash)
        .ok_or(LedgerError::UnknownParent(block.header.prev_hash))?;
    if block.header.height != parent_meta.height + 1 {
        return Err(LedgerError::HeightMismatch { parent: parent_meta.height, actual: block.header.height });
    }

    let expected_target = chain.expected_difficulty(&block.header.prev_hash)?;
    if block.header.difficulty_target != expected_target {
        return Err(LedgerError::DifficultyMismatch { height: block.header.height });
    }

    // Never trust a miner's self-declared target for the actual PoW check
    // — `expected_target` (just verified above) is what's authoritative;
    // this checks the block's hash against that same value.
    let target = expand_target(expected_target);
    if !block.hash().meets_target(&target) {
        return Err(LedgerError::ProofOfWorkInvalid);
    }

    let median_time_past = chain.median_time_past(&block.header.prev_hash)?;
    if block.header.timestamp <= median_time_past {
        return Err(LedgerError::TimestampTooOld);
    }
    if block.header.timestamp > now + MAX_FUTURE_DRIFT_SECS {
        return Err(LedgerError::TimestampTooFarInFuture);
    }

    validate_transactions(chain, block)?;

    Ok(())
}

/// Whether `tx` could validly be included in a block extending the current
/// tip at `candidate_height` right now — the same anchor/ownership checks
/// `validate_transactions` applies inside a block, evaluated standalone
/// against the tip. A miner assembling a candidate uses this to filter its
/// mempool down to transactions that won't make the whole block invalid
/// (`validate_block` rejects a block entirely if *any* transaction in it is
/// invalid) — it is not a substitute for `validate_block`, which is still
/// what actually decides whether a mined block gets accepted.
pub fn is_valid_for_mempool(chain: &Chain, tx: &Transaction, candidate_height: u64) -> bool {
    if tx.verify_self_contained().is_err() {
        return false;
    }
    let tip_hash = chain.tip_hash();
    let Ok(ancestry) = chain.ancestors(&tip_hash) else { return false };
    if !ancestry.contains(&tx.anchor_block_hash) {
        return false;
    }
    let Some(anchor_meta) = chain.get_meta(&tx.anchor_block_hash) else { return false };
    if candidate_height.saturating_sub(anchor_meta.height) > ANCHOR_MAX_AGE_BLOCKS {
        return false;
    }
    match chain.state_at(&tip_hash) {
        Ok(state) => !state.contains_key(&tx.username),
        Err(_) => false,
    }
}

fn validate_transactions(chain: &Chain, block: &Block) -> Result<()> {
    if block.transactions.is_empty() {
        return Ok(());
    }

    let parent_state = chain.state_at(&block.header.prev_hash)?;
    let ancestry: HashSet<_> = chain.ancestors(&block.header.prev_hash)?.into_iter().collect();
    let mut claimed_in_this_block = HashSet::new();

    for tx in &block.transactions {
        tx.verify_self_contained()?;

        if !ancestry.contains(&tx.anchor_block_hash) {
            return Err(LedgerError::AnchorInvalid);
        }
        let anchor_meta = chain.get_meta(&tx.anchor_block_hash).ok_or(LedgerError::AnchorInvalid)?;
        if block.header.height.saturating_sub(anchor_meta.height) > ANCHOR_MAX_AGE_BLOCKS {
            return Err(LedgerError::AnchorInvalid);
        }

        if parent_state.contains_key(&tx.username) {
            return Err(LedgerError::UsernameAlreadyClaimed(tx.username.clone()));
        }
        if !claimed_in_this_block.insert(tx.username.clone()) {
            return Err(LedgerError::DuplicateClaimInBlock(tx.username.clone()));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain_state::{ApplyOutcome, Chain};
    use crate::difficulty::{expand_target, work_of, INITIAL_DIFFICULTY_BITS, TARGET_BLOCK_TIME_SECS};
    use crate::hash::Hash32;
    use crate::test_support::{identity, mine_block};
    use crate::transaction::Transaction;

    #[test]
    fn a_solo_mined_empty_block_extends_the_tip() {
        let mut chain = Chain::new();
        let block = mine_block(&chain, vec![], 2_000_000_000);
        let outcome = chain.try_apply(block, 2_000_000_000).unwrap();
        assert_eq!(outcome, ApplyOutcome::ExtendedTip);
        assert_eq!(chain.tip_height(), 1);
    }

    #[test]
    fn a_block_with_a_valid_claim_updates_username_owner() {
        let mut chain = Chain::new();
        let alice = identity(1);
        let now = 2_000_000_000;
        let tx = Transaction::new_claim(&alice, "alice", 1, chain.tip_hash(), [1; 8]).unwrap();
        let block = mine_block(&chain, vec![tx], now);
        chain.try_apply(block, now).unwrap();

        let owner = chain.username_owner("alice").expect("alice should be claimed");
        assert_eq!(owner.owner_public_key, alice.public_key().to_bytes());
        assert_eq!(owner.claimed_at_height, 1);
    }

    #[test]
    fn rejects_a_block_whose_hash_does_not_meet_the_target() {
        let chain = Chain::new();
        let mut block = mine_block(&chain, vec![], 2_000_000_000);
        // Corrupt the nonce so the PoW no longer holds, without touching
        // anything else validate_block checks first.
        block.header.nonce = block.header.nonce.wrapping_add(1);
        let mut chain = chain;
        assert_eq!(
            chain.try_apply(block, 2_000_000_000).unwrap_err(),
            LedgerError::ProofOfWorkInvalid
        );
    }

    #[test]
    fn rejects_a_block_with_an_unknown_parent() {
        let mut chain = Chain::new();
        let mut block = mine_block(&chain, vec![], 2_000_000_000);
        block.header.prev_hash = Hash32::of(b"not a real block");
        // Re-mine so PoW still holds against the (still-correct) expected
        // target — only the parent linkage should be what's wrong.
        let target = expand_target(block.header.difficulty_target);
        while !block.header.hash().meets_target(&target) {
            block.header.nonce += 1;
        }
        assert!(matches!(
            chain.try_apply(block, 2_000_000_000).unwrap_err(),
            LedgerError::UnknownParent(_)
        ));
    }

    #[test]
    fn rejects_a_timestamp_at_or_before_the_median_time_past() {
        let mut chain = Chain::new();
        let mut block = mine_block(&chain, vec![], crate::block::GENESIS_TIMESTAMP);
        // A single ancestor (genesis) means median-time-past equals
        // genesis's own timestamp — anything not strictly after it must
        // be rejected.
        let target = expand_target(block.header.difficulty_target);
        while !block.header.hash().meets_target(&target) {
            block.header.nonce += 1;
        }
        assert_eq!(
            chain.try_apply(block, crate::block::GENESIS_TIMESTAMP).unwrap_err(),
            LedgerError::TimestampTooOld
        );
    }

    #[test]
    fn rejects_a_timestamp_too_far_in_the_future() {
        let mut chain = Chain::new();
        let far_future = crate::block::GENESIS_TIMESTAMP + MAX_FUTURE_DRIFT_SECS + 10_000;
        let block = mine_block(&chain, vec![], far_future);
        assert_eq!(
            chain.try_apply(block, crate::block::GENESIS_TIMESTAMP).unwrap_err(),
            LedgerError::TimestampTooFarInFuture
        );
    }

    #[test]
    fn rejects_two_transactions_claiming_the_same_username_in_one_block() {
        let mut chain = Chain::new();
        let alice = identity(1);
        let bob = identity(2);
        let now = 2_000_000_000;
        let tx_a = Transaction::new_claim(&alice, "shared", 1, chain.tip_hash(), [1; 8]).unwrap();
        let tx_b = Transaction::new_claim(&bob, "shared", 1, chain.tip_hash(), [2; 8]).unwrap();
        let block = mine_block(&chain, vec![tx_a, tx_b], now);
        assert_eq!(
            chain.try_apply(block, now).unwrap_err(),
            LedgerError::DuplicateClaimInBlock("shared".to_string())
        );
    }

    #[test]
    fn rejects_a_second_claim_for_an_already_owned_username_in_a_later_block() {
        let mut chain = Chain::new();
        let alice = identity(1);
        let mallory = identity(2);
        let now = 2_000_000_000;

        let first = Transaction::new_claim(&alice, "prize", 1, chain.tip_hash(), [1; 8]).unwrap();
        let block_one = mine_block(&chain, vec![first], now);
        chain.try_apply(block_one, now).unwrap();

        let second = Transaction::new_claim(&mallory, "prize", 2, chain.tip_hash(), [2; 8]).unwrap();
        let block_two = mine_block(&chain, vec![second], now + 400);
        assert_eq!(
            chain.try_apply(block_two, now + 400).unwrap_err(),
            LedgerError::UsernameAlreadyClaimed("prize".to_string())
        );
        // Alice keeps her claim — the invalid block was never applied.
        assert_eq!(chain.username_owner("prize").unwrap().owner_public_key, alice.public_key().to_bytes());
    }

    #[test]
    fn rejects_a_claim_anchored_to_an_unknown_block() {
        let mut chain = Chain::new();
        let alice = identity(1);
        let now = 2_000_000_000;
        let tx = Transaction::new_claim(&alice, "alice", 1, Hash32::of(b"nowhere"), [1; 8]).unwrap();
        let block = mine_block(&chain, vec![tx], now);
        assert_eq!(chain.try_apply(block, now).unwrap_err(), LedgerError::AnchorInvalid);
    }

    #[test]
    fn rejects_a_tx_commitment_that_does_not_match_the_actual_transactions() {
        let mut chain = Chain::new();
        let alice = identity(1);
        let now = 2_000_000_000;
        let tx = Transaction::new_claim(&alice, "alice", 1, chain.tip_hash(), [1; 8]).unwrap();
        let mut block = mine_block(&chain, vec![tx], now);
        block.header.tx_commitment = Hash32::of(b"wrong");
        // Re-mine so the PoW check doesn't mask the commitment failure.
        let target = expand_target(block.header.difficulty_target);
        while !block.header.hash().meets_target(&target) {
            block.header.nonce += 1;
        }
        assert_eq!(chain.try_apply(block, now).unwrap_err(), LedgerError::TxCommitmentMismatch);
    }

    #[test]
    fn a_heavier_fork_reorgs_the_canonical_tip_and_recomputes_state() {
        let mut chain = Chain::new();
        let alice = identity(1);
        let mallory = identity(2);
        let now = 2_000_000_000;

        // Branch A: alice claims "prize" at height 1.
        let claim_a = Transaction::new_claim(&alice, "prize", 1, chain.tip_hash(), [1; 8]).unwrap();
        let block_a1 = mine_block(&chain, vec![claim_a], now);

        // Branch B: an alternate, competing height-1 block with no claims
        // (built against the same genesis parent, so it's a real fork).
        let block_b1 = mine_block(&chain, vec![], now + 1);

        chain.try_apply(block_a1.clone(), now).unwrap();
        assert_eq!(chain.username_owner("prize").unwrap().owner_public_key, alice.public_key().to_bytes());

        let outcome_b1 = chain.try_apply(block_b1.clone(), now + 1).unwrap();
        // Both blocks have identical work (same difficulty) — B does not
        // overtake A just by arriving second with equal work.
        assert_eq!(outcome_b1, ApplyOutcome::AddedToFork);
        assert_eq!(chain.tip_hash(), block_a1.hash());

        // Extend branch B past branch A's height — now B has strictly
        // more cumulative work and must become canonical, taking alice's
        // claim with it (branch B never included it).
        let mut mallory_chain_view = Chain::new();
        mallory_chain_view.try_apply(block_b1.clone(), now + 1).unwrap();
        let claim_b2 = Transaction::new_claim(&mallory, "prize", 2, block_b1.hash(), [2; 8]).unwrap();
        let block_b2 = mine_block(&mallory_chain_view, vec![claim_b2], now + 2);

        chain.try_apply(block_b2.clone(), now + 2).unwrap();
        let outcome = chain.try_apply(block_b2, now + 2);
        // Second application of the same block is a no-op, not a reorg —
        // the real reorg already happened on the first apply above.
        assert_eq!(outcome.unwrap(), ApplyOutcome::AlreadyKnown);

        assert_eq!(chain.tip_height(), 2);
        assert_eq!(
            chain.username_owner("prize").unwrap().owner_public_key,
            mallory.public_key().to_bytes(),
            "reorg should have replaced alice's claim with mallory's from the heavier branch"
        );
    }

    #[test]
    fn cumulative_work_only_ever_increases_with_height() {
        let mut chain = Chain::new();
        let now = 2_000_000_000;
        for i in 0..5u64 {
            let block = mine_block(&chain, vec![], now + i * (TARGET_BLOCK_TIME_SECS as u64));
            chain.try_apply(block, now + i * (TARGET_BLOCK_TIME_SECS as u64)).unwrap();
        }
        let tip_work = chain.get_meta(&chain.tip_hash()).unwrap().cumulative_work;
        assert!(tip_work >= work_of(INITIAL_DIFFICULTY_BITS) * 5.0 * 0.99);
    }
}
