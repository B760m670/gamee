//! A small, purpose-built, phone-run Proof-of-Work ledger whose only
//! transaction type is a `@username` claim. This crate has no networking
//! and no persistence of its own — like `spiritchat-crypto-core`, it's
//! pure logic, consumed by `spiritchat-p2p-core` for gossip/sync and by
//! this crate's own (later) `store.rs` for on-disk persistence.
//!
//! Exists because a plain DHT (see `spiritchat_p2p_core::username`) has no
//! way to arbitrate *who claimed a name first* — only a real consensus
//! mechanism can, and Proof-of-Work is the only Sybil-resistance tool
//! consistent with this project's constraints (no servers, no fees, no
//! external blockchain): identities are free to create, so any
//! voting/staking scheme without a trusted party is trivially attacked by
//! spinning up unlimited fake ones.

pub mod block;
pub mod chain_state;
pub mod difficulty;
pub mod error;
pub mod hash;
pub mod transaction;
pub mod validation;

pub use block::{Block, BlockHeader};
pub use chain_state::{ApplyOutcome, Chain, UsernameOwner};
pub use difficulty::{CompactTarget, INITIAL_DIFFICULTY_BITS, RETARGET_INTERVAL_BLOCKS, TARGET_BLOCK_TIME_SECS};
pub use error::{LedgerError, Result};
pub use hash::Hash32;
pub use transaction::Transaction;
