//! UniFFI bridge for `spiritchat_crypto_core::backup` — encrypting and
//! decrypting the account-recovery backup the P2P layer publishes into
//! the DHT. Free functions (no object to hold): the key is derived from
//! the identity seed on every call, and the seed already lives in the
//! Keychain on the Swift side.

use rand_core::OsRng;
use spiritchat_crypto_core::backup;

use crate::error::{FfiError, FfiResult};

fn seed_array(identity_seed: Vec<u8>) -> FfiResult<[u8; 32]> {
    identity_seed.try_into().map_err(|bytes: Vec<u8>| FfiError::Crypto {
        reason: format!("identity seed must be 32 bytes, got {}", bytes.len()),
    })
}

/// Encrypts `plaintext` (the app's serialized profile/contacts snapshot)
/// under a key only this identity's recovery-phrase holder can derive —
/// what `FfiP2pNode::announce_recovery_backup` publishes.
#[uniffi::export]
pub fn recovery_backup_encrypt(identity_seed: Vec<u8>, plaintext: Vec<u8>) -> FfiResult<Vec<u8>> {
    Ok(backup::encrypt_backup(&mut OsRng, &seed_array(identity_seed)?, &plaintext)?)
}

/// Decrypts a backup fetched from the DHT. An error means "not ours or
/// tampered" — treat it exactly like no backup existing at all.
#[uniffi::export]
pub fn recovery_backup_decrypt(identity_seed: Vec<u8>, backup_bytes: Vec<u8>) -> FfiResult<Vec<u8>> {
    Ok(backup::decrypt_backup(&seed_array(identity_seed)?, &backup_bytes)?)
}
