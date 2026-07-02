//! Wire types and protocol/topic constants for gossiping and syncing the
//! `@username` ledger (see `spiritchat_ledger_core`) — the replacement for
//! the plain-DHT `username` module's best-effort claims, since only a
//! real consensus mechanism (not a DHT) can arbitrate who claimed a name
//! first. This module only carries bytes between peers and hands them to
//! `spiritchat_ledger_core` for validation; it has no consensus logic of
//! its own.

use libp2p::gossipsub::IdentTopic;
use libp2p::StreamProtocol;
use serde::{Deserialize, Serialize};
use spiritchat_ledger_core::{Block, BlockHeader, Checkpoint};

/// New blocks are gossiped here as soon as they're mined/received — every
/// node subscribes, since receiving blocks (not just mining them) is how
/// a node stays in sync at all.
pub fn blocks_topic() -> IdentTopic {
    IdentTopic::new("/spiritchat/ledger/blocks/1")
}

/// Newly-submitted, not-yet-mined claims — gossiped so *any* peer's miner
/// (not just the submitter's own, if they mine at all) can pick one up
/// and include it in a block.
pub fn txs_topic() -> IdentTopic {
    IdentTopic::new("/spiritchat/ledger/txs/1")
}

/// Request/response protocol for catching a lagging or brand-new peer up
/// to the current chain tip — gossip alone only delivers new blocks going
/// forward, never fills in what a peer missed before it connected.
pub const CHAIN_SYNC_PROTOCOL: StreamProtocol = StreamProtocol::new("/spiritchat/chain-sync/1.0.0");

/// Caps how many blocks/headers a single `GetBlocks`/`GetHeaders`
/// response will carry, so a sync response can't be used to force an
/// unbounded amount of work/memory on the responder or requester.
pub const MAX_SYNC_BATCH: u32 = 500;

// Every variant reads naturally as "get X" (matching the request/response
// RPC-style naming used throughout this protocol) — not accidental
// repetition worth renaming away.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChainSyncRequest {
    /// "What's your current best chain?" — the first question a syncing
    /// node asks, of several peers, to decide whose chain to trust
    /// (trust-on-first-use: whichever tip a majority agree is heaviest,
    /// then independently verified — see `GetHeaders`/`GetBlocks` below).
    GetTip,
    /// Full blocks (bodies included) starting at `from_height`, up to
    /// `count` (capped at `MAX_SYNC_BATCH`) — used both for ordinary
    /// catch-up and, combined with `GetUsernameOwnerSnapshot`, for
    /// bootstrapping a brand-new node.
    GetBlocks { from_height: u64, count: u32 },
    /// Headers only (no transactions) — for independently re-verifying
    /// proof-of-work/retarget/timestamps across a long historical range
    /// without needing every block's full body, matching how this node's
    /// own storage keeps headers forever but prunes old bodies.
    GetHeaders { from_height: u64, count: u32 },
    /// The responder's current materialized `username -> owner` state —
    /// treated as an *untrusted candidate* checkpoint by the requester
    /// until independently corroborated (see `spiritchat_ledger_core`'s
    /// `Checkpoint`/`Chain::from_checkpoint`), never trusted outright.
    GetUsernameOwnerSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChainSyncResponse {
    Tip { height: u64, hash: [u8; 32], cumulative_work: f64 },
    Blocks(Vec<Block>),
    Headers(Vec<BlockHeader>),
    UsernameOwnerSnapshot(Checkpoint),
    /// The responder has nothing to answer with (e.g. asked for blocks
    /// below its own floor, already pruned).
    NotAvailable,
}
