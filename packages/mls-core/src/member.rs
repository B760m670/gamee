//! A member's public credential in the group — the minimal RFC 9420
//! `LeafNode`/`KeyPackage` this crate's fixed ciphersuite needs, with no
//! X.509: an identity is just an Ed25519 signing key (the same key the
//! rest of SpiritChat already binds a person to — their identity public
//! key), and a `KeyPackage` is that identity plus a fresh X25519 leaf key
//! it will occupy, self-signed so nobody can offer a key package on
//! someone else's behalf.
//!
//! "Self-signed" is the whole authentication story at this layer: the
//! signature is over (leaf key ‖ identity key) under the identity key, so
//! a valid key package proves its offerer controls the identity it names
//! and chose the leaf key inside it. Who is *allowed* to add whom is an
//! application-policy question the group layer enforces on top (phase 4's
//! commit validation), not something the credential itself decides.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use x25519_dalek::PublicKey;

const KEY_PACKAGE_LABEL: &[u8] = b"spiritchat-mls-keypackage-v1";

/// A member's public identity in the group: their long-term Ed25519
/// verifying key. Equality is by key bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MemberIdentity(pub [u8; 32]);

impl MemberIdentity {
    pub fn verifying_key(&self) -> Result<VerifyingKey, MemberError> {
        VerifyingKey::from_bytes(&self.0).map_err(|_| MemberError::BadIdentityKey)
    }
}

impl std::fmt::Debug for MemberIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MemberIdentity({:02x}{:02x}..)", self.0[0], self.0[1])
    }
}

/// A signed offer to occupy a leaf: the fresh X25519 leaf public key this
/// member will hold, bound to their identity. Verified before an Add
/// proposal naming it can be committed.
#[derive(Clone)]
pub struct KeyPackage {
    pub identity: MemberIdentity,
    pub leaf_public: PublicKey,
    pub signature: [u8; 64],
}

fn signing_payload(leaf_public: &PublicKey, identity: &MemberIdentity) -> Vec<u8> {
    let mut payload = Vec::with_capacity(KEY_PACKAGE_LABEL.len() + 64);
    payload.extend_from_slice(KEY_PACKAGE_LABEL);
    payload.extend_from_slice(leaf_public.as_bytes());
    payload.extend_from_slice(&identity.0);
    payload
}

impl KeyPackage {
    /// Builds and self-signs a key package. `signing_key` is this member's
    /// own Ed25519 identity key; `leaf_public` the X25519 key they'll hold
    /// in the tree (its private half stays with them, never here).
    pub fn create(signing_key: &SigningKey, leaf_public: PublicKey) -> Self {
        let identity = MemberIdentity(signing_key.verifying_key().to_bytes());
        let signature = signing_key.sign(&signing_payload(&leaf_public, &identity)).to_bytes();
        KeyPackage { identity, leaf_public, signature }
    }

    /// Checks the self-signature — a key package whose signature doesn't
    /// verify under the identity it names must never be added.
    pub fn verify(&self) -> Result<(), MemberError> {
        let vk = self.identity.verifying_key()?;
        let sig = Signature::from_bytes(&self.signature);
        vk.verify(&signing_payload(&self.leaf_public, &self.identity), &sig)
            .map_err(|_| MemberError::BadSignature)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum MemberError {
    BadIdentityKey,
    BadSignature,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;
    use x25519_dalek::StaticSecret;

    fn signing_key(seed: u64) -> SigningKey {
        SigningKey::generate(&mut ChaCha20Rng::seed_from_u64(seed))
    }

    fn leaf_key(seed: u64) -> PublicKey {
        PublicKey::from(&StaticSecret::random_from_rng(ChaCha20Rng::seed_from_u64(seed)))
    }

    #[test]
    fn a_self_signed_key_package_verifies() {
        let kp = KeyPackage::create(&signing_key(1), leaf_key(2));
        kp.verify().unwrap();
    }

    #[test]
    fn a_tampered_leaf_key_fails_verification() {
        let mut kp = KeyPackage::create(&signing_key(1), leaf_key(2));
        kp.leaf_public = leaf_key(3);
        assert_eq!(kp.verify().unwrap_err(), MemberError::BadSignature);
    }

    #[test]
    fn a_key_package_cannot_be_reattributed_to_another_identity() {
        let mut kp = KeyPackage::create(&signing_key(1), leaf_key(2));
        kp.identity = MemberIdentity(signing_key(9).verifying_key().to_bytes());
        assert_eq!(kp.verify().unwrap_err(), MemberError::BadSignature);
    }
}
