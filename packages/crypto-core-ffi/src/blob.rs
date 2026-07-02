//! Content addressing for blobs handed to `FfiP2pNode::set_local_blob`/
//! `fetch_blob` (e.g. avatar images). SHA-256 rather than something
//! p2p-core picks itself, since that crate is deliberately hash-agnostic —
//! this keeps every platform (iOS today, Android later) computing content
//! ids the same way, so a blob fetched from a peer can be verified against
//! the id it was requested by.

use sha2::{Digest, Sha256};

#[uniffi::export]
pub fn blob_content_id(bytes: Vec<u8>) -> Vec<u8> {
    Sha256::digest(&bytes).to_vec()
}
