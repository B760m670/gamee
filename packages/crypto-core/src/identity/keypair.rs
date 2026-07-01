use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::CryptoRngCore;
use zeroize::ZeroizeOnDrop;

use crate::error::{CryptoError, Result};

/// A long-term Ed25519 identity. This is the "who you are" of an account —
/// it never changes and is what SimpleX-style QR contact cards commit to.
/// It signs prekey bundles; it never encrypts messages directly.
#[derive(ZeroizeOnDrop)]
pub struct IdentityKeyPair {
    #[zeroize(skip)] // SigningKey zeroizes its own inner secret on drop.
    signing_key: SigningKey,
}

/// The public half of an [`IdentityKeyPair`] — safe to share, e.g. inside a
/// QR contact card.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IdentityPublicKey(VerifyingKey);

impl IdentityKeyPair {
    pub fn generate(rng: &mut impl CryptoRngCore) -> Self {
        Self {
            signing_key: SigningKey::generate(rng),
        }
    }

    pub fn public_key(&self) -> IdentityPublicKey {
        IdentityPublicKey(self.signing_key.verifying_key())
    }

    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.signing_key.sign(message).to_bytes()
    }
}

impl IdentityPublicKey {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let array: [u8; 32] =
            bytes
                .try_into()
                .map_err(|_| CryptoError::InvalidKeyLength {
                    expected: 32,
                    actual: bytes.len(),
                })?;
        VerifyingKey::from_bytes(&array)
            .map(IdentityPublicKey)
            .map_err(|_| CryptoError::Decode("invalid Ed25519 public key"))
    }

    pub fn to_bytes(self) -> [u8; 32] {
        self.0.to_bytes()
    }

    pub fn verify(&self, message: &[u8], signature: &[u8; 64]) -> Result<()> {
        let signature = Signature::from_bytes(signature);
        self.0
            .verify(message, &signature)
            .map_err(|_| CryptoError::SignatureInvalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    fn test_rng() -> ChaCha20Rng {
        ChaCha20Rng::seed_from_u64(1)
    }

    #[test]
    fn signs_and_verifies() {
        let identity = IdentityKeyPair::generate(&mut test_rng());
        let signature = identity.sign(b"hello");
        identity
            .public_key()
            .verify(b"hello", &signature)
            .expect("valid signature must verify");
    }

    #[test]
    fn rejects_tampered_message() {
        let identity = IdentityKeyPair::generate(&mut test_rng());
        let signature = identity.sign(b"hello");
        let err = identity
            .public_key()
            .verify(b"goodbye", &signature)
            .unwrap_err();
        assert_eq!(err, CryptoError::SignatureInvalid);
    }

    #[test]
    fn rejects_wrong_signer() {
        let alice = IdentityKeyPair::generate(&mut test_rng());
        let bob = IdentityKeyPair::generate(&mut ChaCha20Rng::seed_from_u64(2));
        let signature = alice.sign(b"hello");
        assert!(bob.public_key().verify(b"hello", &signature).is_err());
    }

    #[test]
    fn public_key_round_trips_through_bytes() {
        let identity = IdentityKeyPair::generate(&mut test_rng());
        let bytes = identity.public_key().to_bytes();
        let restored = IdentityPublicKey::from_bytes(&bytes).unwrap();
        assert_eq!(identity.public_key(), restored);
    }

    #[test]
    fn rejects_wrong_length_bytes() {
        let err = IdentityPublicKey::from_bytes(&[0u8; 10]).unwrap_err();
        assert_eq!(
            err,
            CryptoError::InvalidKeyLength {
                expected: 32,
                actual: 10
            }
        );
    }
}
