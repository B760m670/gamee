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

/// The reference a media message actually carries inside its (end-to-end
/// encrypted) chat payload — everything the recipient needs to fetch and
/// decrypt the media, but not the media bytes themselves. Those travel
/// separately over the content-addressed blob transport: one blob per
/// encrypted chunk, keyed by its content id in `chunk_ids` (in order), so
/// large media reuses the existing blob protocol unchanged and gets
/// per-chunk integrity, dedup and resumable fetch for free.
///
/// `key` decrypts the chunks (see `media_decrypt_chunk`); `thumbnail`, if
/// present, is a small already-decryptable preview the app can show before
/// the full media is fetched. The whole manifest is small, so it fits
/// through the mailbox for offline delivery even when the media bytes
/// (too big for the 64 KiB mailbox) can't — the recipient learns about
/// the media offline and fetches its bytes once a holder is reachable.
#[derive(uniffi::Record, serde::Serialize, serde::Deserialize)]
pub struct FfiMediaManifest {
    pub key: Vec<u8>,
    /// MIME type, e.g. "image/jpeg", "video/mp4", "audio/opus".
    pub mime: String,
    /// Plaintext size in bytes — for a progress bar and a size label.
    pub total_size: u64,
    /// Content id of each encrypted chunk blob, in order.
    pub chunk_ids: Vec<Vec<u8>>,
    pub filename: Option<String>,
    /// Playback length for voice/video, in milliseconds.
    pub duration_ms: Option<u32>,
    /// A small inline preview (e.g. a blurred JPEG) shown before the full
    /// media is fetched. Kept small enough to ride inside the message.
    pub thumbnail: Option<Vec<u8>>,
}

/// Canonical bytes for a manifest — what the app puts inside a media
/// message's plaintext (behind its own frame tag). Hand-agreed format so
/// sender and receiver never drift, the same discipline the ledger and
/// MLS wire formats use.
#[uniffi::export]
pub fn media_manifest_encode(manifest: FfiMediaManifest) -> Vec<u8> {
    bincode::serialize(&manifest).expect("a media manifest serializes")
}

#[uniffi::export]
pub fn media_manifest_decode(bytes: Vec<u8>) -> FfiResult<FfiMediaManifest> {
    bincode::deserialize(&bytes).map_err(|_| media_err("malformed media manifest"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_media_encrypted_under_a_generated_key_round_trips_via_ffi() {
        let key = media_generate_key();
        let ct = media_encrypt(key.clone(), b"voice bytes".to_vec()).unwrap();
        assert_eq!(media_decrypt(key, ct).unwrap(), b"voice bytes".to_vec());
    }

    #[test]
    fn a_manifest_round_trips_through_its_canonical_encoding() {
        let manifest = FfiMediaManifest {
            key: vec![7u8; 32],
            mime: "audio/opus".to_string(),
            total_size: 4096,
            chunk_ids: vec![vec![1, 2, 3], vec![4, 5, 6]],
            filename: Some("note.opus".to_string()),
            duration_ms: Some(3200),
            thumbnail: None,
        };
        let bytes = media_manifest_encode(manifest);
        let decoded = media_manifest_decode(bytes).unwrap();
        assert_eq!(decoded.mime, "audio/opus");
        assert_eq!(decoded.chunk_ids.len(), 2);
        assert_eq!(decoded.duration_ms, Some(3200));
        assert_eq!(decoded.key, vec![7u8; 32]);
    }

    #[test]
    fn a_malformed_manifest_is_rejected() {
        assert!(media_manifest_decode(vec![0xff, 0x00, 0x13]).is_err());
    }
}
