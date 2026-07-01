//! Identity, key agreement, and Double Ratchet messaging core for
//! SpiritChat — a serverless, peer-to-peer, end-to-end-encrypted
//! messenger. This crate has no networking and no storage; it only turns
//! keys and plaintext into ciphertext (and back), so it can be linked into
//! the Android app (via JNI) and the iOS app (via a C ABI) from one
//! reviewed, tested implementation.

pub mod error;

pub mod encoding;
pub mod identity;
pub mod prekey;

pub mod handshake;
pub mod ratchet;
