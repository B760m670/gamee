//! Post-quantum key encapsulation — ML-KEM-768 (FIPS 203), the quantum-safe
//! half of the hybrid PQXDH handshake.
//!
//! ML-KEM (formerly CRYSTALS-Kyber) is a lattice KEM: instead of a
//! Diffie-Hellman shared secret from two public keys, the initiator
//! *encapsulates* against the responder's public key, producing a ciphertext
//! plus a shared secret; the responder *decapsulates* that ciphertext with
//! their secret key to recover the same shared secret. A quantum computer
//! running Shor's algorithm breaks X25519 (and every elliptic-curve DH) but
//! has no known efficient attack on Module-LWE, so folding an ML-KEM shared
//! secret into the X3DH key schedule defends against "harvest now, decrypt
//! later": traffic recorded today can't be decrypted by a future quantum
//! adversary even though the classical DH parts eventually fall.
//!
//! We don't roll our own lattice crypto — this wraps the audited pure-Rust
//! `ml-kem` crate (RustCrypto) with a small, byte-oriented API that the rest
//! of the crate serializes and stores like any other key. ML-KEM-768 is the
//! NIST category-3 parameter set (the same one Signal's PQXDH and Apple's
//! iMessage PQ3 deploy).

use ml_kem::kem::{Decapsulate, Encapsulate};
use ml_kem::{EncodedSizeUser, KemCore, MlKem768};
use rand_core::CryptoRngCore;
use zeroize::Zeroize;

use crate::error::{CryptoError, Result};

type Ek = <MlKem768 as KemCore>::EncapsulationKey;
type Dk = <MlKem768 as KemCore>::DecapsulationKey;

/// Encoded length of an ML-KEM-768 encapsulation (public) key, in bytes.
pub const PQ_PUBLIC_KEY_LEN: usize = 1184;
/// Encoded length of an ML-KEM-768 decapsulation (secret) key, in bytes.
pub const PQ_SECRET_KEY_LEN: usize = 2400;
/// Length of an ML-KEM-768 ciphertext (what the initiator sends), in bytes.
pub const PQ_CIPHERTEXT_LEN: usize = 1088;
/// Length of the shared secret both sides derive, in bytes.
pub const PQ_SHARED_SECRET_LEN: usize = 32;

/// An ML-KEM-768 encapsulation (public) key — the quantum-safe prekey a peer
/// publishes so others can encapsulate a shared secret to them. All public:
/// safe to put in a QR contact card alongside the X25519 prekeys.
#[derive(Clone)]
pub struct PqKemPublicKey(Ek);

impl PqKemPublicKey {
    pub fn to_bytes(&self) -> [u8; PQ_PUBLIC_KEY_LEN] {
        let encoded = self.0.as_bytes();
        let mut out = [0u8; PQ_PUBLIC_KEY_LEN];
        out.copy_from_slice(&encoded);
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let encoded = ml_kem::Encoded::<Ek>::try_from(bytes).map_err(|_| {
            CryptoError::InvalidKeyLength {
                expected: PQ_PUBLIC_KEY_LEN,
                actual: bytes.len(),
            }
        })?;
        Ok(Self(Ek::from_bytes(&encoded)))
    }

    /// Encapsulates a fresh shared secret to this public key, returning the
    /// ciphertext to send the key's owner and the shared secret to fold into
    /// the handshake. The owner recovers the same secret via
    /// [`PqKemSecretKey::decapsulate`].
    pub fn encapsulate(
        &self,
        rng: &mut impl CryptoRngCore,
    ) -> Result<(PqCiphertext, PqSharedSecret)> {
        let (ct, ss) = self
            .0
            .encapsulate(rng)
            .map_err(|_| CryptoError::PqEncapsulationFailed)?;
        let mut ct_bytes = [0u8; PQ_CIPHERTEXT_LEN];
        ct_bytes.copy_from_slice(&ct);
        let mut ss_bytes = [0u8; PQ_SHARED_SECRET_LEN];
        ss_bytes.copy_from_slice(&ss);
        Ok((PqCiphertext(ct_bytes), PqSharedSecret(ss_bytes)))
    }
}

/// The secret half of an ML-KEM-768 prekey. Held only by the key's owner and
/// zeroized on drop — the inner `ml-kem` `DecapsulationKey` implements
/// `ZeroizeOnDrop` (crate `zeroize` feature), so this wrapper inherits it.
/// This is what recovers the shared secret from a peer's ciphertext.
pub struct PqKemSecretKey(Dk);

impl PqKemSecretKey {
    /// Recovers the shared secret from a ciphertext an initiator encapsulated
    /// against the matching public key.
    pub fn decapsulate(&self, ciphertext: &PqCiphertext) -> Result<PqSharedSecret> {
        let encoded = ml_kem::Ciphertext::<MlKem768>::try_from(ciphertext.0.as_slice())
            .map_err(|_| CryptoError::Decode("invalid ML-KEM ciphertext length"))?;
        let ss = self
            .0
            .decapsulate(&encoded)
            .map_err(|_| CryptoError::PqDecapsulationFailed)?;
        let mut ss_bytes = [0u8; PQ_SHARED_SECRET_LEN];
        ss_bytes.copy_from_slice(&ss);
        Ok(PqSharedSecret(ss_bytes))
    }

    pub fn to_bytes(&self) -> [u8; PQ_SECRET_KEY_LEN] {
        let encoded = self.0.as_bytes();
        let mut out = [0u8; PQ_SECRET_KEY_LEN];
        out.copy_from_slice(&encoded);
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let encoded = ml_kem::Encoded::<Dk>::try_from(bytes).map_err(|_| {
            CryptoError::InvalidKeyLength {
                expected: PQ_SECRET_KEY_LEN,
                actual: bytes.len(),
            }
        })?;
        Ok(Self(Dk::from_bytes(&encoded)))
    }
}

/// A freshly generated ML-KEM-768 keypair.
pub struct PqKemKeyPair {
    pub public: PqKemPublicKey,
    pub secret: PqKemSecretKey,
}

impl PqKemKeyPair {
    pub fn generate(rng: &mut impl CryptoRngCore) -> Self {
        let (dk, ek) = MlKem768::generate(rng);
        Self {
            public: PqKemPublicKey(ek),
            secret: PqKemSecretKey(dk),
        }
    }
}

/// An ML-KEM-768 ciphertext — public, sent from initiator to responder.
#[derive(Clone)]
pub struct PqCiphertext(pub [u8; PQ_CIPHERTEXT_LEN]);

impl PqCiphertext {
    pub fn as_bytes(&self) -> &[u8; PQ_CIPHERTEXT_LEN] {
        &self.0
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let array: [u8; PQ_CIPHERTEXT_LEN] =
            bytes.try_into().map_err(|_| CryptoError::InvalidKeyLength {
                expected: PQ_CIPHERTEXT_LEN,
                actual: bytes.len(),
            })?;
        Ok(Self(array))
    }
}

/// The 32-byte shared secret both sides derive. Zeroized on drop; folded into
/// the X3DH key schedule, never used directly as a message key.
pub struct PqSharedSecret([u8; PQ_SHARED_SECRET_LEN]);

impl PqSharedSecret {
    pub fn as_bytes(&self) -> &[u8; PQ_SHARED_SECRET_LEN] {
        &self.0
    }
}

impl Drop for PqSharedSecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    #[test]
    fn encapsulate_then_decapsulate_agrees() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let kp = PqKemKeyPair::generate(&mut rng);
        let (ct, ss_sender) = kp.public.encapsulate(&mut rng).unwrap();
        let ss_receiver = kp.secret.decapsulate(&ct).unwrap();
        assert_eq!(ss_sender.as_bytes(), ss_receiver.as_bytes());
    }

    #[test]
    fn public_key_round_trips_through_bytes() {
        let mut rng = ChaCha20Rng::seed_from_u64(2);
        let kp = PqKemKeyPair::generate(&mut rng);
        let restored = PqKemPublicKey::from_bytes(&kp.public.to_bytes()).unwrap();
        // The restored key must still encapsulate to a secret the original
        // secret key can recover.
        let (ct, ss_sender) = restored.encapsulate(&mut rng).unwrap();
        let ss_receiver = kp.secret.decapsulate(&ct).unwrap();
        assert_eq!(ss_sender.as_bytes(), ss_receiver.as_bytes());
    }

    #[test]
    fn secret_key_round_trips_through_bytes() {
        let mut rng = ChaCha20Rng::seed_from_u64(3);
        let kp = PqKemKeyPair::generate(&mut rng);
        let (ct, ss_sender) = kp.public.encapsulate(&mut rng).unwrap();

        let restored_secret = PqKemSecretKey::from_bytes(&kp.secret.to_bytes()).unwrap();
        let ss_receiver = restored_secret.decapsulate(&ct).unwrap();
        assert_eq!(ss_sender.as_bytes(), ss_receiver.as_bytes());
    }

    #[test]
    fn ciphertext_round_trips_through_bytes() {
        let mut rng = ChaCha20Rng::seed_from_u64(4);
        let kp = PqKemKeyPair::generate(&mut rng);
        let (ct, ss_sender) = kp.public.encapsulate(&mut rng).unwrap();

        let restored_ct = PqCiphertext::from_bytes(ct.as_bytes()).unwrap();
        let ss_receiver = kp.secret.decapsulate(&restored_ct).unwrap();
        assert_eq!(ss_sender.as_bytes(), ss_receiver.as_bytes());
    }

    #[test]
    fn different_encapsulations_yield_different_secrets() {
        let mut rng = ChaCha20Rng::seed_from_u64(5);
        let kp = PqKemKeyPair::generate(&mut rng);
        let (_ct1, ss1) = kp.public.encapsulate(&mut rng).unwrap();
        let (_ct2, ss2) = kp.public.encapsulate(&mut rng).unwrap();
        assert_ne!(ss1.as_bytes(), ss2.as_bytes());
    }

    #[test]
    fn wrong_secret_key_decapsulates_to_a_different_secret() {
        // ML-KEM is IND-CCA2: decapsulating with the wrong key doesn't error
        // (implicit rejection) but yields an unrelated secret, so the
        // handshake KDF diverges and the session simply fails to establish —
        // exactly the desired outcome.
        let mut rng = ChaCha20Rng::seed_from_u64(6);
        let kp = PqKemKeyPair::generate(&mut rng);
        let other = PqKemKeyPair::generate(&mut rng);
        let (ct, ss_sender) = kp.public.encapsulate(&mut rng).unwrap();
        let ss_wrong = other.secret.decapsulate(&ct).unwrap();
        assert_ne!(ss_sender.as_bytes(), ss_wrong.as_bytes());
    }

    #[test]
    fn rejects_wrong_length_public_key() {
        assert!(PqKemPublicKey::from_bytes(&[0u8; 10]).is_err());
        assert!(PqKemPublicKey::from_bytes(&[0u8; PQ_PUBLIC_KEY_LEN - 1]).is_err());
    }
}
