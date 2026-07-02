//! The in-memory chain: every block this node has ever accepted, which one
//! is canonical, and the materialized `username -> owner` view of the
//! canonical chain. No persistence here — see `store.rs` (Phase 2) for the
//! redb-backed wrapper that survives a restart; this is the pure reference
//! implementation Phase 2 is tested against.
//!
//! `state_at`/`ancestors` walk from genesis on every call — O(chain
//! depth) per validation. That's a deliberate simplification for this
//! pure, in-memory reference: Phase 2's persisted store maintains
//! `username_owner` incrementally instead of replaying from genesis every
//! time, which is what makes phone-bounded performance actually work at
//! real chain lengths.

use std::collections::{BTreeMap, HashMap};

use crate::block::Block;
use crate::difficulty::{work_of, CompactTarget};
use crate::error::{LedgerError, Result};
use crate::hash::Hash32;
use crate::transaction::normalize_username;
use crate::validation::validate_block;

pub const MEDIAN_TIME_PAST_WINDOW: usize = 11;

#[derive(Debug, Clone, Copy)]
pub struct BlockMeta {
    pub height: u64,
    pub cumulative_work: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsernameOwner {
    pub owner_public_key: [u8; 32],
    pub claimed_at_height: u64,
    pub claimed_in_block: Hash32,
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
}

impl Chain {
    pub fn new() -> Self {
        let genesis = Block::genesis();
        let hash = genesis.hash();

        let mut blocks = HashMap::new();
        blocks.insert(hash, genesis);

        let mut meta = HashMap::new();
        meta.insert(hash, BlockMeta { height: 0, cumulative_work: 0.0 });

        let mut canonical_index = BTreeMap::new();
        canonical_index.insert(0, hash);

        Chain { blocks, meta, tip_hash: hash, canonical_index, username_owner: HashMap::new() }
    }

    pub fn tip_hash(&self) -> Hash32 {
        self.tip_hash
    }

    pub fn tip_height(&self) -> u64 {
        self.meta[&self.tip_hash].height
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

    /// `hash`'s full ancestry, genesis first, `hash` itself last —
    /// `ancestors(hash)[height]` always holds the block at that height,
    /// since every block has exactly one parent.
    pub fn ancestors(&self, hash: &Hash32) -> Result<Vec<Hash32>> {
        let mut path = Vec::new();
        let mut current = *hash;
        loop {
            let block = self.blocks.get(&current).ok_or(LedgerError::UnknownBlock(current))?;
            path.push(current);
            if block.header.height == 0 {
                break;
            }
            current = block.header.prev_hash;
        }
        path.reverse();
        Ok(path)
    }

    /// The materialized `username -> owner` state as of (and including)
    /// `hash` — built by replaying `hash`'s ancestry from genesis. Blocks
    /// already in `self.blocks` were fully validated before being
    /// accepted, so this just replays already-trusted history rather than
    /// re-checking anything.
    pub fn state_at(&self, hash: &Hash32) -> Result<HashMap<String, UsernameOwner>> {
        let mut state = HashMap::new();
        for ancestor_hash in self.ancestors(hash)? {
            let block = &self.blocks[&ancestor_hash];
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
            ancestry[window_start..].iter().map(|hash| self.blocks[hash].header.timestamp).collect();
        timestamps.sort_unstable();
        Ok(timestamps[timestamps.len() / 2])
    }

    /// The `difficulty_target` a block extending `parent_hash` must have:
    /// unchanged unless the new height lands exactly on a retarget
    /// boundary, in which case it's recomputed from how long the just-
    /// completed interval actually took versus its target duration.
    pub fn expected_difficulty(&self, parent_hash: &Hash32) -> Result<CompactTarget> {
        use crate::difficulty::{retarget, RETARGET_INTERVAL_BLOCKS, TARGET_BLOCK_TIME_SECS};

        let parent = self.blocks.get(parent_hash).ok_or(LedgerError::UnknownBlock(*parent_hash))?;
        let new_height = parent.header.height + 1;

        if new_height % RETARGET_INTERVAL_BLOCKS != 0 {
            return Ok(parent.header.difficulty_target);
        }

        let interval_start_height = new_height - RETARGET_INTERVAL_BLOCKS;
        let ancestry = self.ancestors(parent_hash)?; // genesis..=parent, index == height
        let interval_start_block = &self.blocks[&ancestry[interval_start_height as usize]];

        let actual_span =
            (parent.header.timestamp as i64 - interval_start_block.header.timestamp as i64).max(1);
        let expected_span = RETARGET_INTERVAL_BLOCKS as i64 * TARGET_BLOCK_TIME_SECS;

        Ok(retarget(parent.header.difficulty_target, actual_span, expected_span))
    }

    /// Validates and, if valid, accepts `block` — updating the canonical
    /// tip and `username_owner` if (and only if) this makes `block`'s
    /// chain the heaviest one known. `now` is passed in rather than read
    /// from the system clock so this stays fully deterministic and
    /// testable.
    pub fn try_apply(&mut self, block: Block, now: u64) -> Result<ApplyOutcome> {
        let hash = block.hash();
        if self.blocks.contains_key(&hash) {
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

        self.blocks.insert(hash, block);
        self.meta.insert(hash, BlockMeta { height, cumulative_work });

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
        for (height, ancestor_hash) in ancestry.iter().enumerate() {
            self.canonical_index.insert(height as u64, *ancestor_hash);
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
}
