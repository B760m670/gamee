use x25519_dalek::PublicKey as X25519PublicKey;

use crate::error::{CryptoError, Result};
use crate::identity::{IdentityPublicKey, SignedAgreementKeyPublic};

/// A signed X25519 public key: the identity owner vouches ("signs") for this
/// agreement key, so a peer who trusts the identity can trust the key too.
#[derive(Clone, Copy, Debug)]
pub struct SignedPrekeyPublic {
    pub public: X25519PublicKey,
    pub signature: [u8; 64],
}

/// Everything a peer needs to start an X3DH handshake with someone,
/// entirely offline — this is the payload that goes inside a QR contact
/// card. `one_time_prekey` is consumed by the first handshake that uses it;
/// omitting it (None) still yields a secure handshake, just without the
/// extra forward-secrecy margin a one-time key adds.
#[derive(Clone, Copy, Debug)]
pub struct PrekeyBundle {
    pub identity: IdentityPublicKey,
    pub identity_agreement_key: SignedAgreementKeyPublic,
    pub signed_prekey: SignedPrekeyPublic,
    pub one_time_prekey: Option<X25519PublicKey>,
}

/// Domain-separation prefix mixed into every signed-prekey signature, so a
/// signature can never be replayed as proof of a different kind of claim.
const SIGNED_PREKEY_CONTEXT: &[u8] = b"SpiritChat-SignedPrekey-v1";

impl PrekeyBundle {
    /// Verifies both the signed prekey and the identity agreement key
    /// against `identity`. Callers MUST call this before using a bundle
    /// scanned from a QR code or received from a peer — an unverified
    /// bundle lets an attacker substitute their own agreement keys.
    pub fn verify(&self) -> Result<()> {
        self.identity_agreement_key.verify(&self.identity)?;

        let mut message = Vec::with_capacity(SIGNED_PREKEY_CONTEXT.len() + 32);
        message.extend_from_slice(SIGNED_PREKEY_CONTEXT);
        message.extend_from_slice(self.signed_prekey.public.as_bytes());
        self.identity
            .verify(&message, &self.signed_prekey.signature)
    }
}

pub(crate) fn sign_prekey(
    identity: &crate::identity::IdentityKeyPair,
    prekey_public: &X25519PublicKey,
) -> [u8; 64] {
    let mut message = Vec::with_capacity(SIGNED_PREKEY_CONTEXT.len() + 32);
    message.extend_from_slice(SIGNED_PREKEY_CONTEXT);
    message.extend_from_slice(prekey_public.as_bytes());
    identity.sign(&message)
}

pub fn x25519_public_from_bytes(bytes: &[u8]) -> Result<X25519PublicKey> {
    let array: [u8; 32] = bytes
        .try_into()
        .map_err(|_| CryptoError::InvalidKeyLength {
            expected: 32,
            actual: bytes.len(),
        })?;
    Ok(X25519PublicKey::from(array))
}

#[cfg(test)]
mod tests {
    use crate::identity::{AgreementKeyPair, IdentityKeyPair};
    use crate::prekey::store::PrekeyStore;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    fn make_bundle(seed: u64) -> (IdentityKeyPair, AgreementKeyPair, PrekeyStore) {
        let mut rng = ChaCha20Rng::seed_from_u64(seed);
        let identity = IdentityKeyPair::generate(&mut rng);
        let agreement = AgreementKeyPair::generate(&mut rng);
        let store = PrekeyStore::generate(&identity, &mut rng, 1);
        (identity, agreement, store)
    }

    #[test]
    fn accepts_a_correctly_signed_bundle() {
        let (identity, agreement, mut store) = make_bundle(1);
        let bundle = store.public_bundle(identity.public_key(), agreement.sign_with(&identity));
        bundle
            .verify()
            .expect("bundle signed by its own identity must verify");
    }

    #[test]
    fn rejects_a_bundle_signed_by_someone_else() {
        let (identity, agreement, mut store) = make_bundle(1);
        let attacker_identity = IdentityKeyPair::generate(&mut ChaCha20Rng::seed_from_u64(2));
        let mut forged =
            store.public_bundle(identity.public_key(), agreement.sign_with(&identity));

        // Attacker swaps in their own identity but keeps the original signature.
        forged.identity = attacker_identity.public_key();

        assert!(forged.verify().is_err());
    }

    #[test]
    fn rejects_a_swapped_signed_prekey() {
        let (identity, agreement, mut store) = make_bundle(1);
        let mut bundle =
            store.public_bundle(identity.public_key(), agreement.sign_with(&identity));

        let (other_identity, other_agreement, mut other_store) = make_bundle(2);
        let other_bundle = other_store
            .public_bundle(other_identity.public_key(), other_agreement.sign_with(&other_identity));
        bundle.signed_prekey = other_bundle.signed_prekey;

        assert!(bundle.verify().is_err());
    }
}
