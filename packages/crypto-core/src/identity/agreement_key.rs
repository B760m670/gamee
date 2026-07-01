use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};
use zeroize::ZeroizeOnDrop;

use super::keypair::{IdentityKeyPair, IdentityPublicKey};
use crate::error::Result;

const AGREEMENT_KEY_CONTEXT: &[u8] = b"SpiritChat-AgreementKey-v1";

/// The long-term X25519 key used for Diffie-Hellman in the X3DH handshake
/// (Signal calls this "IK"). Kept as a separate key from the Ed25519
/// signing identity ([`IdentityKeyPair`]) on purpose: a key should do one
/// cryptographic job, not double as both a signature key and a DH key.
#[derive(ZeroizeOnDrop)]
pub struct AgreementKeyPair {
    secret: StaticSecret,
}

/// The public half, signed by the owner's identity so a peer who already
/// trusts the identity can trust this key too.
#[derive(Clone, Copy, Debug)]
pub struct SignedAgreementKeyPublic {
    pub public: X25519PublicKey,
    pub signature: [u8; 64],
}

impl AgreementKeyPair {
    pub fn generate(rng: &mut impl rand_core::CryptoRngCore) -> Self {
        Self {
            secret: StaticSecret::random_from_rng(rng),
        }
    }

    pub fn secret(&self) -> &StaticSecret {
        &self.secret
    }

    pub fn public(&self) -> X25519PublicKey {
        X25519PublicKey::from(&self.secret)
    }

    /// Signs this key's public part with `identity`, producing the form
    /// that gets published (e.g. embedded in a QR contact card).
    pub fn sign_with(&self, identity: &IdentityKeyPair) -> SignedAgreementKeyPublic {
        let public = self.public();
        SignedAgreementKeyPublic {
            public,
            signature: identity.sign(&signing_message(&public)),
        }
    }
}

impl SignedAgreementKeyPublic {
    pub fn verify(&self, identity: &IdentityPublicKey) -> Result<()> {
        identity.verify(&signing_message(&self.public), &self.signature)
    }
}

fn signing_message(public: &X25519PublicKey) -> Vec<u8> {
    let mut message = Vec::with_capacity(AGREEMENT_KEY_CONTEXT.len() + 32);
    message.extend_from_slice(AGREEMENT_KEY_CONTEXT);
    message.extend_from_slice(public.as_bytes());
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    #[test]
    fn accepts_its_own_signature() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let identity = IdentityKeyPair::generate(&mut rng);
        let agreement = AgreementKeyPair::generate(&mut rng);
        let signed = agreement.sign_with(&identity);
        signed.verify(&identity.public_key()).unwrap();
    }

    #[test]
    fn rejects_a_swapped_key_under_the_same_signature() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let identity = IdentityKeyPair::generate(&mut rng);
        let agreement = AgreementKeyPair::generate(&mut rng);
        let mut signed = agreement.sign_with(&identity);

        let other_agreement = AgreementKeyPair::generate(&mut rng);
        signed.public = other_agreement.public();

        assert!(signed.verify(&identity.public_key()).is_err());
    }
}
