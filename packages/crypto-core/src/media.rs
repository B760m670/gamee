//! Encrypting media (photos, video, voice) for chat — the same design
//! Signal/WhatsApp and MLS itself use, deliberately *not* the blockchain
//! (a namespace ledger has nothing to do with moving bytes): a large file
//! is encrypted under a fresh random per-file key, chunk by chunk, and
//! that key travels in an ordinary end-to-end message (Double Ratchet 1:1,
//! MLS epoch key for groups) while the ciphertext moves over the existing
//! content-addressed blob transport. This module is only the file
//! encryption; key delivery and transport live above it.
//!
//! Chunked on purpose: a video must never be held whole in memory, on
//! either side. Each chunk is independently AEAD-sealed under the file key
//! with a counter nonce (safe — the key is unique per file, so a counter
//! never repeats a (key, nonce) pair), and its **index and last-chunk
//! flag are authenticated** so the three attacks a naive chunked scheme
//! invites are all caught:
//!   - reordering  — the index is in the AAD, so a chunk decrypted at the
//!     wrong position fails;
//!   - truncation  — only the final chunk carries the `last` marker, so a
//!     stream cut short never terminates cleanly;
//!   - splicing    — a chunk from another file (different key) never
//!     authenticates.
//! End-to-end integrity of the whole ciphertext is additionally the blob
//! transport's job (it is content-addressed — fetched by the hash of these
//! exact bytes), so a tampered blob is rejected before decryption even
//! starts.

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use rand_core::CryptoRngCore;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{CryptoError, Result};

/// The plaintext bytes per chunk. 64 KiB balances per-chunk overhead
/// (16-byte tag) against memory: a receiver only ever holds one chunk of
/// plaintext at a time. Fixed and versioned via the AAD domain tag, so a
/// future size change can't be confused with the current one.
pub const MEDIA_CHUNK_SIZE: usize = 64 * 1024;

const AAD_DOMAIN: &[u8] = b"spiritchat-media-v1";
const NONCE_LEN: usize = 12;

/// A per-file media key — random, used once for exactly one file, then
/// sent to the recipient over an end-to-end channel. Zeroized on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct MediaKey([u8; 32]);

impl MediaKey {
    pub fn generate(rng: &mut impl CryptoRngCore) -> Self {
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The raw key, to place inside the end-to-end message that references
    /// this media. As secret as the message it rides in.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for MediaKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MediaKey(<redacted>)")
    }
}

fn nonce_for(chunk_index: u32) -> [u8; NONCE_LEN] {
    let mut nonce = [0u8; NONCE_LEN];
    nonce[..4].copy_from_slice(&chunk_index.to_le_bytes());
    nonce
}

fn aad_for(chunk_index: u32, is_last: bool) -> Vec<u8> {
    let mut aad = Vec::with_capacity(AAD_DOMAIN.len() + 5);
    aad.extend_from_slice(AAD_DOMAIN);
    aad.extend_from_slice(&chunk_index.to_le_bytes());
    aad.push(if is_last { 1 } else { 0 });
    aad
}

/// Seals one chunk. `chunk_index` must increase from 0; `is_last` marks
/// the final chunk of the file. Returns `nonce`-free ciphertext (the nonce
/// is derived from the index, so it needn't be stored).
pub fn encrypt_chunk(key: &MediaKey, chunk_index: u32, is_last: bool, plaintext: &[u8]) -> Vec<u8> {
    let cipher = ChaCha20Poly1305::new(key.0.as_slice().into());
    cipher
        .encrypt(
            Nonce::from_slice(&nonce_for(chunk_index)),
            Payload { msg: plaintext, aad: &aad_for(chunk_index, is_last) },
        )
        .expect("ChaCha20-Poly1305 encryption of a bounded chunk does not fail")
}

/// Opens one chunk. Fails if the key is wrong, the chunk was tampered
/// with, or it's being decrypted at the wrong index / with the wrong
/// last-chunk expectation (reorder / truncation / splice).
pub fn decrypt_chunk(key: &MediaKey, chunk_index: u32, is_last: bool, ciphertext: &[u8]) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(key.0.as_slice().into());
    cipher
        .decrypt(
            Nonce::from_slice(&nonce_for(chunk_index)),
            Payload { msg: ciphertext, aad: &aad_for(chunk_index, is_last) },
        )
        .map_err(|_| CryptoError::DecryptionFailed)
}

/// The framed size of a chunk's ciphertext, so a streaming reader can
/// split a concatenated blob back into chunks without a length prefix per
/// chunk: every chunk but the last is exactly this size.
pub const CHUNK_CIPHERTEXT_SIZE: usize = MEDIA_CHUNK_SIZE + 16; // + Poly1305 tag

/// Whole-buffer convenience for small media (voice notes, thumbnails) and
/// for tests — chunks internally, returns the concatenated ciphertext. A
/// large video should stream `encrypt_chunk` from disk instead of loading
/// the whole file, which is exactly why the per-chunk API above is the
/// real primitive and this is the convenience.
pub fn encrypt_media(key: &MediaKey, plaintext: &[u8]) -> Vec<u8> {
    if plaintext.is_empty() {
        // A zero-length file is still one (empty, final) chunk, so an
        // empty media can't be forged as "no chunks at all".
        return encrypt_chunk(key, 0, true, &[]);
    }
    let chunks: Vec<&[u8]> = plaintext.chunks(MEDIA_CHUNK_SIZE).collect();
    let last = chunks.len() - 1;
    let mut out = Vec::with_capacity(plaintext.len() + chunks.len() * 16);
    for (i, chunk) in chunks.iter().enumerate() {
        out.extend_from_slice(&encrypt_chunk(key, i as u32, i == last, chunk));
    }
    out
}

/// Inverse of [`encrypt_media`]. Walks the concatenated ciphertext chunk
/// by chunk, enforcing that exactly the final chunk is marked last — so a
/// truncated or over-long blob is rejected, not silently accepted.
pub fn decrypt_media(key: &MediaKey, ciphertext: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    let mut index: u32 = 0;
    loop {
        let remaining = ciphertext.len() - offset;
        // A full-size ciphertext chunk means more may follow; anything
        // shorter must be the final chunk.
        let is_last = remaining <= CHUNK_CIPHERTEXT_SIZE;
        let take = if is_last { remaining } else { CHUNK_CIPHERTEXT_SIZE };
        if take < 16 {
            // Not even room for an auth tag — malformed.
            return Err(CryptoError::Decode("media chunk shorter than its auth tag"));
        }
        let chunk = &ciphertext[offset..offset + take];
        out.extend_from_slice(&decrypt_chunk(key, index, is_last, chunk)?);
        offset += take;
        index = index.checked_add(1).ok_or(CryptoError::Decode("too many media chunks"))?;
        if is_last {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    fn key(seed: u64) -> MediaKey {
        MediaKey::generate(&mut ChaCha20Rng::seed_from_u64(seed))
    }

    #[test]
    fn small_media_round_trips() {
        let k = key(1);
        let data = b"a voice note's opus bytes".to_vec();
        let ct = encrypt_media(&k, &data);
        assert_eq!(decrypt_media(&k, &ct).unwrap(), data);
    }

    #[test]
    fn multi_chunk_media_round_trips() {
        let k = key(1);
        // 2.5 chunks worth, so the last chunk is a partial one.
        let data: Vec<u8> = (0..(MEDIA_CHUNK_SIZE * 2 + 123)).map(|i| (i % 251) as u8).collect();
        let ct = encrypt_media(&k, &data);
        assert_eq!(decrypt_media(&k, &ct).unwrap(), data);
    }

    #[test]
    fn an_exact_multiple_of_the_chunk_size_round_trips() {
        // The tricky boundary: the last chunk is exactly full, so
        // is_last can't be inferred from a short final chunk — it's the
        // `remaining <= CHUNK_CIPHERTEXT_SIZE` check that catches it.
        let k = key(1);
        let data: Vec<u8> = (0..(MEDIA_CHUNK_SIZE * 2)).map(|i| (i % 251) as u8).collect();
        let ct = encrypt_media(&k, &data);
        assert_eq!(decrypt_media(&k, &ct).unwrap(), data);
    }

    #[test]
    fn an_empty_media_round_trips() {
        let k = key(1);
        let ct = encrypt_media(&k, &[]);
        assert_eq!(decrypt_media(&k, &ct).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn the_wrong_key_cannot_decrypt() {
        let ct = encrypt_media(&key(1), b"secret photo");
        assert!(decrypt_media(&key(2), &ct).is_err());
    }

    #[test]
    fn a_tampered_chunk_is_rejected() {
        let k = key(1);
        let mut ct = encrypt_media(&k, b"secret photo");
        let last = ct.len() - 1;
        ct[last] ^= 0x01;
        assert!(decrypt_media(&k, &ct).is_err());
    }

    #[test]
    fn a_reordered_chunk_is_rejected() {
        let k = key(1);
        let data: Vec<u8> = (0..(MEDIA_CHUNK_SIZE * 2)).map(|i| (i % 251) as u8).collect();
        // Swap chunk 0 and chunk 1's ciphertexts — each is a full
        // CHUNK_CIPHERTEXT_SIZE block. Decryption must fail because the
        // index is authenticated in the AAD.
        let mut ct = encrypt_media(&k, &data);
        let (a, b) = ct.split_at_mut(CHUNK_CIPHERTEXT_SIZE);
        a[..].swap_with_slice(&mut b[..CHUNK_CIPHERTEXT_SIZE]);
        assert!(decrypt_media(&k, &ct).is_err());
    }

    #[test]
    fn truncation_is_detected() {
        let k = key(1);
        let data: Vec<u8> = (0..(MEDIA_CHUNK_SIZE * 3)).map(|i| (i % 251) as u8).collect();
        let ct = encrypt_media(&k, &data);
        // Drop the final chunk: what remains ends on a chunk that was
        // sealed as non-last, so decrypting its tail as "last" fails.
        let truncated = &ct[..CHUNK_CIPHERTEXT_SIZE * 2];
        assert!(decrypt_media(&k, truncated).is_err());
    }

    #[test]
    fn a_final_chunk_cannot_be_passed_off_as_non_final() {
        // Directly at the chunk API: a chunk sealed as last won't open as
        // non-last and vice versa — the flag is authenticated.
        let k = key(1);
        let sealed_last = encrypt_chunk(&k, 0, true, b"end");
        assert!(decrypt_chunk(&k, 0, false, &sealed_last).is_err());
        let sealed_mid = encrypt_chunk(&k, 0, false, b"mid");
        assert!(decrypt_chunk(&k, 0, true, &sealed_mid).is_err());
    }

    #[test]
    fn chunks_from_two_files_cannot_be_spliced() {
        let data: Vec<u8> = (0..(MEDIA_CHUNK_SIZE * 2)).map(|i| (i % 251) as u8).collect();
        let ct_a = encrypt_media(&key(1), &data);
        let ct_b = encrypt_media(&key(2), &data);
        // Take file A's first chunk and file B's tail — different keys,
        // so A's chunk can't authenticate under B's key.
        let mut spliced = ct_a[..CHUNK_CIPHERTEXT_SIZE].to_vec();
        spliced.extend_from_slice(&ct_b[CHUNK_CIPHERTEXT_SIZE..]);
        assert!(decrypt_media(&key(2), &spliced).is_err());
    }
}
