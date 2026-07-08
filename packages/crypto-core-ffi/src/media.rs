//! UniFFI bridge for `spiritchat_crypto_core::media` — encrypting media
//! (photo/video/voice) for chat. Free functions, no state: a media key is
//! random per file and the Swift side owns the streaming (read a file
//! chunk, encrypt it, write it to the encrypted blob; reverse on receive),
//! so the boundary is just "seal/open these bytes at this index".
//!
//! The typical flow, for reference:
//!   send:    key = media_generate_key(); for each 64 KiB chunk i of the
//!            file → media_encrypt_chunk(key, i, isLast, chunk) → append to
//!            the encrypted blob; register that blob content-addressed;
//!            send { key, content_id, mime, size, ... } over the chat.
//!   receive: fetch the blob by content_id (transport verifies the hash);
//!            for each chunk → media_decrypt_chunk(key, i, isLast, chunk).

use spiritchat_crypto_core::media::{
    self, MediaKey, CHUNK_CIPHERTEXT_SIZE, MEDIA_CHUNK_SIZE,
};

use crate::error::{FfiError, FfiResult};

fn media_err(reason: &str) -> FfiError {
    FfiError::Crypto { reason: format!("media: {reason}") }
}

fn key_from(bytes: Vec<u8>) -> FfiResult<MediaKey> {
    let arr: [u8; 32] = bytes.try_into().map_err(|_| media_err("media key must be 32 bytes"))?;
    Ok(MediaKey::from_bytes(arr))
}

/// A fresh per-file key. Store it in the media-pointer message you send
/// over the chat; never reuse it for another file.
#[uniffi::export]
pub fn media_generate_key() -> Vec<u8> {
    MediaKey::generate(&mut rand_core::OsRng).as_bytes().to_vec()
}

/// The plaintext chunk size (64 KiB) — how much of a file to read per
/// `media_encrypt_chunk` call.
#[uniffi::export]
pub fn media_chunk_size() -> u32 {
    MEDIA_CHUNK_SIZE as u32
}

/// The ciphertext size of a full (non-final) chunk — how much of an
/// encrypted blob to read per `media_decrypt_chunk` call while more chunks
/// remain.
#[uniffi::export]
pub fn media_chunk_ciphertext_size() -> u32 {
    CHUNK_CIPHERTEXT_SIZE as u32
}

/// Seals one chunk. `chunk_index` counts from 0; `is_last` marks the final
/// chunk (its index + last flag are authenticated, so reorder/truncation
/// are caught on decrypt).
#[uniffi::export]
pub fn media_encrypt_chunk(key: Vec<u8>, chunk_index: u32, is_last: bool, plaintext: Vec<u8>) -> FfiResult<Vec<u8>> {
    Ok(media::encrypt_chunk(&key_from(key)?, chunk_index, is_last, &plaintext))
}

/// Opens one chunk. Fails on the wrong key, tampering, or a wrong
/// index/last-flag (reorder/truncation/splice).
#[uniffi::export]
pub fn media_decrypt_chunk(key: Vec<u8>, chunk_index: u32, is_last: bool, ciphertext: Vec<u8>) -> FfiResult<Vec<u8>> {
    Ok(media::decrypt_chunk(&key_from(key)?, chunk_index, is_last, &ciphertext)?)
}

/// Whole-buffer seal for small media (voice notes, thumbnails). A large
/// video should stream `media_encrypt_chunk` instead of loading the file.
#[uniffi::export]
pub fn media_encrypt(key: Vec<u8>, plaintext: Vec<u8>) -> FfiResult<Vec<u8>> {
    Ok(media::encrypt_media(&key_from(key)?, &plaintext))
}

/// Whole-buffer open, inverse of `media_encrypt` — enforces that exactly
/// the final chunk is marked last, so a truncated or over-long blob is
/// rejected.
#[uniffi::export]
pub fn media_decrypt(key: Vec<u8>, ciphertext: Vec<u8>) -> FfiResult<Vec<u8>> {
    Ok(media::decrypt_media(&key_from(key)?, &ciphertext)?)
}
