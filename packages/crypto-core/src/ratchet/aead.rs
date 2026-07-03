//! Wraps ChaCha20-Poly1305 for encrypting a single message under a
//! single-use [`MessageKey`]. Kept separate from the ratchet state machine
//! so the stepping logic in `state.rs` reads as key management, not a mix
//! of key management and cipher plumbing.

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use rand_core::CryptoRngCore;

use crate::error::{CryptoError, Result};

use super::chain::MessageKey;

pub const NONCE_LEN: usize = 12;

/// Encrypts `plaintext` under `key`, prepending a fresh random nonce to the
/// returned ciphertext. Each [`MessageKey`] is used for exactly one
/// message — that is the whole point of the ratchet — so nonce reuse under
/// the same key cannot happen even with a predictable nonce; we still
/// randomize it, which costs nothing and removes any need to reason about
/// counter bookkeeping being correct.
pub fn encrypt(
    rng: &mut impl CryptoRngCore,
    key: &MessageKey,
    plaintext: &[u8],
    associated_data: &[u8],
) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(key.as_bytes().into());
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad: associated_data,
            },
        )
        .map_err(|_| CryptoError::DecryptionFailed)?;

    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub fn decrypt(key: &MessageKey, input: &[u8], associated_data: &[u8]) -> Result<Vec<u8>> {
    if input.len() < NONCE_LEN {
        return Err(CryptoError::Decode("ciphertext shorter than a nonce"));
    }
    let (nonce_bytes, ciphertext) = input.split_at(NONCE_LEN);
    let cipher = ChaCha20Poly1305::new(key.as_bytes().into());
    cipher
        .decrypt(
            Nonce::from_slice(nonce_bytes),
            Payload {
                msg: ciphertext,
                aad: associated_data,
            },
        )
        .map_err(|_| CryptoError::DecryptionFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    #[test]
    fn round_trips_plaintext() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let key = MessageKey([5u8; 32]);
        let ciphertext = encrypt(&mut rng, &key, b"hello ratchet", b"aad").unwrap();
        let plaintext = decrypt(&key, &ciphertext, b"aad").unwrap();
        assert_eq!(plaintext, b"hello ratchet");
    }

    #[test]
    fn rejects_tampered_ciphertext() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let key = MessageKey([5u8; 32]);
        let mut ciphertext = encrypt(&mut rng, &key, b"hello", b"aad").unwrap();
        *ciphertext.last_mut().unwrap() ^= 0xff;
        assert!(decrypt(&key, &ciphertext, b"aad").is_err());
    }

    #[test]
    fn rejects_mismatched_associated_data() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let key = MessageKey([5u8; 32]);
        let ciphertext = encrypt(&mut rng, &key, b"hello", b"correct-aad").unwrap();
        assert!(decrypt(&key, &ciphertext, b"wrong-aad").is_err());
    }

    #[test]
    fn rejects_wrong_key() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let key = MessageKey([5u8; 32]);
        let wrong_key = MessageKey([6u8; 32]);
        let ciphertext = encrypt(&mut rng, &key, b"hello", b"aad").unwrap();
        assert!(decrypt(&wrong_key, &ciphertext, b"aad").is_err());
    }

    #[test]
    fn two_encryptions_of_the_same_plaintext_differ() {
        // Confirms the nonce is actually randomized per call.
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let key = MessageKey([5u8; 32]);
        let a = encrypt(&mut rng, &key, b"hello", b"aad").unwrap();
        let b = encrypt(&mut rng, &key, b"hello", b"aad").unwrap();
        assert_ne!(a, b);
    }
}
