//! Building a `@username` ledger claim — the one piece of the ledger that
//! needs this crate's own types (`FfiIdentity`'s wrapped
//! `IdentityKeyPair`), everything else lives on `FfiP2pNode` in
//! `p2p_node.rs`. `Transaction`/`Block` cross the UniFFI boundary as
//! opaque `bincode`-encoded bytes (they're plain Rust structs, not UniFFI
//! objects/records) — Swift never needs to know their shape, only to pass
//! them straight back into `FfiP2pNode::submit_username_claim`.

use spiritchat_ledger_core::{Hash32, Transaction};

use crate::error::{FfiError, FfiResult};
use crate::identity::FfiIdentity;

fn ledger_err(reason: impl std::fmt::Display) -> FfiError {
    FfiError::P2p { reason: reason.to_string() }
}

/// Builds and signs a new `@username` claim, ready to hand to
/// `FfiP2pNode::submit_username_claim`. `anchor_block_hash` should be a
/// recent block on the chain this node currently considers canonical
/// (e.g. its current tip) — the signature is bound to that exact point in
/// that exact chain, so it can never be replayed as valid on a different
/// fork whose block at that position hashes differently. `nonce` should
/// be 8 random bytes (dedupes otherwise-identical-looking claims, e.g. a
/// resubmission).
#[uniffi::export]
pub fn ledger_build_username_claim(
    identity: &FfiIdentity,
    username: String,
    anchor_height: u64,
    anchor_block_hash: Vec<u8>,
    nonce: Vec<u8>,
) -> FfiResult<Vec<u8>> {
    let anchor_block_hash: [u8; 32] =
        anchor_block_hash.try_into().map_err(|_| ledger_err("anchor_block_hash must be exactly 32 bytes"))?;
    let nonce: [u8; 8] = nonce.try_into().map_err(|_| ledger_err("nonce must be exactly 8 bytes"))?;

    let transaction =
        Transaction::new_claim(&identity.0, &username, anchor_height, Hash32(anchor_block_hash), nonce)
            .map_err(ledger_err)?;

    bincode::serialize(&transaction).map_err(ledger_err)
}
