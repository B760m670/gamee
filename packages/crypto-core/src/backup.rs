//! Encrypted account-recovery backup — the "cloud backup" of a messenger
//! that has no cloud. A small blob of account data (display name, saved
//! contacts, whatever the app layer chooses to include) is encrypted here
//! and published into the public DHT by `spiritchat-p2p-core`, so a fresh
//! install restoring from a recovery phrase can pull it back — from the
//! network, not from any server.
//!
//! The key is derived from the same 32-byte identity seed the BIP39
//! phrase already produces (`identity::mnemonic`), HKDF-domain-separated
//! so backup encryption never reuses raw key material another purpose
//! already uses (the same discipline as `mix::routing_keypair_from_seed`).
//! That derivation is the whole security argument, stated plainly:
//! **exactly the set of people who can restore the account (phrase
//! holders) can read the backup — nobody else**, including every DHT node
//! that stores or replicates the ciphertext. There is deliberately no
//! second password: the phrase already *is* the root secret of the
//! account, and a weaker extra secret would only add a weaker way in.
//!
//! Forward secrecy is deliberately **not** claimed here, and can't be:
//! a backup's entire purpose is that the phrase decrypts it later. That's
//! why session/ratchet state must never be put inside one — only data
//! that is already long-lived under the identity itself. The app layer
//! owns that judgment; this module just refuses nothing.

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use hkdf::Hkdf;
use rand_core::CryptoRngCore;
use sha2::Sha256;

use crate::error::{CryptoError, Result};

const NONCE_LEN: usize = 12;
const KEY_INFO: &[u8] = b"spiritchat-recovery-backup-key-v1";
const AAD: &[u8] = b"spiritchat-recovery-backup-v1";

/// The backup encryption key for `identity_seed` — deterministic, so a
/// restored device derives the same key from the same phrase.
fn backup_key(identity_seed: &[u8; 32]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(None, identity_seed);
    let mut key = [0u8; 32];
    hk.expand(KEY_INFO, &mut key)
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    key
}

/// Encrypts `plaintext` for publication: `nonce (12) || ciphertext+tag`.
/// A fresh random nonce per encryption — unlike the ratchet's single-use
/// message keys, this key is long-lived and encrypts every republish, so
/// the nonce actually carries the uniqueness burden here.
pub fn encrypt_backup(
    rng: &mut impl CryptoRngCore,
    identity_seed: &[u8; 32],
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(backup_key(identity_seed).as_slice().into());
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, Payload { msg: plaintext, aad: AAD })
        .map_err(|_| CryptoError::DecryptionFailed)?;
    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypts a published backup. Failure means "not ours or tampered" —
/// the DHT is a public, writable store, so a record that doesn't
/// authenticate under our key is treated exactly like no record at all
/// (Poly1305 is what makes a forged or corrupted record impossible to
/// mistake for a real one).
pub fn decrypt_backup(identity_seed: &[u8; 32], bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() < NONCE_LEN {
        return Err(CryptoError::Decode("backup shorter than its nonce"));
    }
    let (nonce_bytes, ciphertext) = bytes.split_at(NONCE_LEN);
    let cipher = ChaCha20Poly1305::new(backup_key(identity_seed).as_slice().into());
    cipher
        .decrypt(Nonce::from_slice(nonce_bytes), Payload { msg: ciphertext, aad: AAD })
        .map_err(|_| CryptoError::DecryptionFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    fn rng() -> ChaCha20Rng {
        ChaCha20Rng::seed_from_u64(7)
    }

    #[test]
    fn a_backup_round_trips_under_the_same_seed() {
        let seed = [42u8; 32];
        let encrypted = encrypt_backup(&mut rng(), &seed, b"display name, contacts, etc").unwrap();
        assert_eq!(decrypt_backup(&seed, &encrypted).unwrap(), b"display name, contacts, etc");
    }

    #[test]
    fn a_different_seed_cannot_decrypt_it() {
        let encrypted = encrypt_backup(&mut rng(), &[42u8; 32], b"secret").unwrap();
        assert!(decrypt_backup(&[43u8; 32], &encrypted).is_err());
    }

    #[test]
    fn a_tampered_backup_is_rejected_not_garbled() {
        let seed = [42u8; 32];
        let mut encrypted = encrypt_backup(&mut rng(), &seed, b"secret").unwrap();
        let last = encrypted.len() - 1;
        encrypted[last] ^= 0x01;
        assert!(decrypt_backup(&seed, &encrypted).is_err());
    }

    #[test]
    fn a_truncated_backup_is_rejected() {
        assert!(decrypt_backup(&[42u8; 32], &[0u8; 5]).is_err());
    }

    #[test]
    fn two_encryptions_of_the_same_plaintext_differ_on_the_wire() {
        // Fresh random nonce per republish — an observer watching the DHT
        // record change must not be able to tell "same contents" from
        // "different contents".
        let seed = [42u8; 32];
        let mut r = rng();
        let a = encrypt_backup(&mut r, &seed, b"same").unwrap();
        let b = encrypt_backup(&mut r, &seed, b"same").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn the_key_is_domain_separated_from_the_raw_seed() {
        assert_ne!(backup_key(&[42u8; 32]), [42u8; 32]);
        assert_ne!(&backup_key(&[42u8; 32])[..], &Hkdf::<Sha256>::new(None, &[42u8; 32]).expand_to_vec());
    }

    // Tiny helper so the domain-separation test reads clearly.
    trait ExpandToVec {
        fn expand_to_vec(&self) -> Vec<u8>;
    }
    impl ExpandToVec for Hkdf<Sha256> {
        fn expand_to_vec(&self) -> Vec<u8> {
            let mut out = vec![0u8; 32];
            self.expand(b"spiritchat-mix-routing-key-v1", &mut out).unwrap();
            out
        }
    }
}
