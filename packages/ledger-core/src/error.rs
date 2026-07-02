use thiserror::Error;

use crate::hash::Hash32;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LedgerError {
    #[error("invalid username: {0}")]
    InvalidUsername(&'static str),

    #[error("transaction signature does not verify against its own owner_public_key")]
    SignatureInvalid,

    #[error("block contains {actual} transactions, more than the maximum of {max}")]
    TooManyTransactions { actual: usize, max: usize },

    #[error("block is {actual} bytes, more than the maximum of {max}")]
    BlockTooLarge { actual: usize, max: usize },

    #[error("block's tx_commitment does not match the hash of its actual transactions")]
    TxCommitmentMismatch,

    #[error("block's proof-of-work hash does not meet its own difficulty_target")]
    ProofOfWorkInvalid,

    #[error("block's difficulty_target does not match the retarget expected at height {height}")]
    DifficultyMismatch { height: u64 },

    #[error("block references an unknown parent {0:?} — sync that block first")]
    UnknownParent(Hash32),

    #[error("block height {actual} is not its parent's height {parent} plus one")]
    HeightMismatch { parent: u64, actual: u64 },

    #[error("block timestamp is not after the median of its last 11 ancestors")]
    TimestampTooOld,

    #[error("block timestamp is too far in the future")]
    TimestampTooFarInFuture,

    #[error("transaction's anchor_block_hash is not a real ancestor of this block within the allowed window")]
    AnchorInvalid,

    #[error("username {0:?} is already claimed by an earlier block in this chain")]
    UsernameAlreadyClaimed(String),

    #[error("block contains more than one claim for username {0:?}")]
    DuplicateClaimInBlock(String),

    #[error("a block hash was referenced that this chain has never seen: {0:?}")]
    UnknownBlock(Hash32),

    #[error("no canonical block at height {0} — it's either not yet mined or already pruned")]
    HeightNotFound(u64),

    #[error("candidate genesis block does not match the hardcoded genesis")]
    GenesisMismatch,

    #[error("persistent storage error: {0}")]
    Storage(String),
}

pub type Result<T> = core::result::Result<T, LedgerError>;
