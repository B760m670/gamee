//! "Seal a secret to a node's public key" — the one asymmetric primitive
//! path-update transport needs. This is HPKE (RFC 9180) in its base mode,
//! specialized to this crate's ciphersuite: DHKEM(X25519, HKDF-SHA256) for
//! the key encapsulation, ChaCha20-Poly1305 for the AEAD. Kept minimal on
//! purpose — a full HPKE library would carry modes (PSK, auth) and
//! ciphersuite negotiation this project's fixed choice never uses.
//!
//! The shape: to seal to recipient public key `pk_r`, generate an
//! ephemeral X25519 keypair `(sk_e, pk_e)`, compute the DH shared secret
//! `sk_e · pk_r`, run it (bound to both public keys) through HKDF into an
//! AEAD key+nonce, and encrypt. The recipient recovers the identical
//! shared secret as `sk_r · pk_e` and decrypts. What TreeKEM needs from
//! this: anyone holding a copath node's private key — i.e. exactly the
//! members under it — can open a path secret sealed to that node, and
//! nobody else can.

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use hkdf::Hkdf;
use rand_core::CryptoRngCore;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

/// A sealed secret: the ephemeral public key ("enc" in HPKE terms)
/// followed by the AEAD ciphertext. Opaque bytes to the transport layer.
#[derive(Clone, PartialEq, Eq)]
pub struct SealedSecret {
    pub enc: [u8; 32],
    pub ciphertext: Vec<u8>,
}

impl std::fmt::Debug for SealedSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The ciphertext is safe to show; there's nothing secret in a
        // sealed blob, but keep it terse in test output.
        f.debug_struct("SealedSecret").field("bytes", &(32 + self.ciphertext.len())).finish()
    }
}

const HPKE_LABEL: &[u8] = b"spiritchat-mls-hpke-v1";
const NONCE_LEN: usize = 12;

/// Derives the AEAD key and nonce from the DH shared secret, binding both
/// public keys in so a ciphertext can't be replayed against a different
/// recipient/ephemeral pairing.
fn derive_key_nonce(shared: &[u8; 32], enc: &[u8; 32], pk_r: &[u8; 32]) -> ([u8; 32], [u8; NONCE_LEN]) {
    let hk = Hkdf::<Sha256>::new(Some(shared), &[]);
    let mut info = Vec::with_capacity(HPKE_LABEL.len() + 64);
    info.extend_from_slice(HPKE_LABEL);
    info.extend_from_slice(enc);
    info.extend_from_slice(pk_r);
    let mut okm = [0u8; 32 + NONCE_LEN];
    hk.expand(&info, &mut okm).expect("44 bytes is a valid HKDF-SHA256 output length");
    let mut key = [0u8; 32];
    let mut nonce = [0u8; NONCE_LEN];
    key.copy_from_slice(&okm[..32]);
    nonce.copy_from_slice(&okm[32..]);
    (key, nonce)
}

/// Seals `plaintext` to `recipient_public`. `aad` is authenticated but not
/// encrypted — the transport binds the group id / epoch / target node into
/// it so a sealed secret can't be lifted into a different context.
pub fn seal(
    rng: &mut impl CryptoRngCore,
    recipient_public: &PublicKey,
    plaintext: &[u8],
    aad: &[u8],
) -> SealedSecret {
    let ephemeral_secret = StaticSecret::random_from_rng(rng);
    let enc = PublicKey::from(&ephemeral_secret).to_bytes();
    let shared = ephemeral_secret.diffie_hellman(recipient_public).to_bytes();
    let (key, nonce) = derive_key_nonce(&shared, &enc, &recipient_public.to_bytes());
    let cipher = ChaCha20Poly1305::new((&key).into());
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: plaintext, aad })
        .expect("ChaCha20-Poly1305 encryption of a bounded plaintext does not fail");
    SealedSecret { enc, ciphertext }
}

/// Opens a sealed secret with the recipient's private key. `None` on any
/// authentication failure — a sealed secret that doesn't open under this
/// key was addressed to a different node (or tampered), which the caller
/// treats exactly like "not for me".
pub fn open(recipient_secret: &StaticSecret, sealed: &SealedSecret, aad: &[u8]) -> Option<Vec<u8>> {
    let enc_public = PublicKey::from(sealed.enc);
    let shared = recipient_secret.diffie_hellman(&enc_public).to_bytes();
    let recipient_public = PublicKey::from(recipient_secret).to_bytes();
    let (key, nonce) = derive_key_nonce(&shared, &sealed.enc, &recipient_public);
    let cipher = ChaCha20Poly1305::new((&key).into());
    cipher
        .decrypt(Nonce::from_slice(&nonce), Payload { msg: &sealed.ciphertext, aad })
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    fn recipient(seed: u64) -> (StaticSecret, PublicKey) {
        let secret = StaticSecret::random_from_rng(ChaCha20Rng::seed_from_u64(seed));
        let public = PublicKey::from(&secret);
        (secret, public)
    }

    #[test]
    fn a_sealed_secret_opens_for_the_right_recipient() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let (sk, pk) = recipient(2);
        let sealed = seal(&mut rng, &pk, b"a path secret", b"group:epoch:node");
        assert_eq!(open(&sk, &sealed, b"group:epoch:node").unwrap(), b"a path secret");
    }

    #[test]
    fn the_wrong_recipient_cannot_open_it() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let (_, pk) = recipient(2);
        let (wrong_sk, _) = recipient(3);
        let sealed = seal(&mut rng, &pk, b"secret", b"aad");
        assert!(open(&wrong_sk, &sealed, b"aad").is_none());
    }

    #[test]
    fn a_wrong_aad_is_rejected() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let (sk, pk) = recipient(2);
        let sealed = seal(&mut rng, &pk, b"secret", b"group:epoch:5");
        assert!(open(&sk, &sealed, b"group:epoch:6").is_none());
    }

    #[test]
    fn tampering_with_the_ciphertext_is_detected() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let (sk, pk) = recipient(2);
        let mut sealed = seal(&mut rng, &pk, b"secret", b"aad");
        let last = sealed.ciphertext.len() - 1;
        sealed.ciphertext[last] ^= 0x01;
        assert!(open(&sk, &sealed, b"aad").is_none());
    }

    #[test]
    fn each_seal_uses_a_fresh_ephemeral_key() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let (_, pk) = recipient(2);
        let a = seal(&mut rng, &pk, b"secret", b"aad");
        let b = seal(&mut rng, &pk, b"secret", b"aad");
        assert_ne!(a.enc, b.enc);
        assert_ne!(a.ciphertext, b.ciphertext);
    }
}
