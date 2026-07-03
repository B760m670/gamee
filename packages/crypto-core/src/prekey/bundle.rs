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

    /// The wire format that goes inside a QR contact card: every public
    /// field, concatenated with fixed-size framing plus a one-byte flag for
    /// the optional one-time prekey. All public data — safe to put in a QR
    /// code or a URL, but still unverified until [`Self::verify`] is
    /// called by whoever scans it.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + 32 + 64 + 32 + 64 + 1 + 32);
        out.extend_from_slice(&self.identity.to_bytes());
        out.extend_from_slice(self.identity_agreement_key.public.as_bytes());
        out.extend_from_slice(&self.identity_agreement_key.signature);
        out.extend_from_slice(self.signed_prekey.public.as_bytes());
        out.extend_from_slice(&self.signed_prekey.signature);
        match self.one_time_prekey {
            Some(otp) => {
                out.push(1);
                out.extend_from_slice(otp.as_bytes());
            }
            None => out.push(0),
        }
        out
    }

    pub fn from_bytes(input: &[u8]) -> Result<Self> {
        const FIXED_LEN: usize = 32 + 32 + 64 + 32 + 64 + 1;
        if input.len() < FIXED_LEN {
            return Err(CryptoError::Decode("contact card too short"));
        }

        let identity = IdentityPublicKey::from_bytes(&input[0..32])?;
        let agreement_public = x25519_public_from_bytes(&input[32..64])?;
        let agreement_signature: [u8; 64] = input[64..128].try_into().unwrap();
        let spk_public = x25519_public_from_bytes(&input[128..160])?;
        let spk_signature: [u8; 64] = input[160..224].try_into().unwrap();

        let one_time_prekey = match input[224] {
            0 => None,
            1 => {
                let otp_bytes = input
                    .get(225..257)
                    .ok_or(CryptoError::Decode("truncated one-time prekey"))?;
                Some(x25519_public_from_bytes(otp_bytes)?)
            }
            _ => return Err(CryptoError::Decode("invalid one-time-prekey flag byte")),
        };

        Ok(Self {
            identity,
            identity_agreement_key: SignedAgreementKeyPublic {
                public: agreement_public,
                signature: agreement_signature,
            },
            signed_prekey: SignedPrekeyPublic {
                public: spk_public,
                signature: spk_signature,
            },
            one_time_prekey,
        })
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

    #[test]
    fn round_trips_through_bytes_with_a_one_time_prekey() {
        let (identity, agreement, mut store) = make_bundle(1);
        let bundle = store.public_bundle(identity.public_key(), agreement.sign_with(&identity));
        assert!(bundle.one_time_prekey.is_some());

        let restored = super::PrekeyBundle::from_bytes(&bundle.to_bytes()).unwrap();
        restored.verify().unwrap();
        assert_eq!(restored.identity, bundle.identity);
        assert_eq!(
            restored.one_time_prekey.unwrap().as_bytes(),
            bundle.one_time_prekey.unwrap().as_bytes()
        );
    }

    #[test]
    fn round_trips_through_bytes_without_a_one_time_prekey() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let identity = IdentityKeyPair::generate(&mut rng);
        let agreement = AgreementKeyPair::generate(&mut rng);
        let mut store = PrekeyStore::generate(&identity, &mut rng, 0);
        let bundle = store.public_bundle(identity.public_key(), agreement.sign_with(&identity));
        assert!(bundle.one_time_prekey.is_none());

        let restored = super::PrekeyBundle::from_bytes(&bundle.to_bytes()).unwrap();
        restored.verify().unwrap();
        assert!(restored.one_time_prekey.is_none());
    }
}
