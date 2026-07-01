use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};
use zeroize::ZeroizeOnDrop;

use crate::encoding::{varint_decode, varint_encode};
use crate::error::{CryptoError, Result};
use crate::identity::{IdentityKeyPair, IdentityPublicKey, SignedAgreementKeyPublic};

use super::bundle::{sign_prekey, PrekeyBundle, SignedPrekeyPublic};

/// Holds the private half of a signed prekey plus one-time prekeys, and
/// hands out [`PrekeyBundle`]s (the public halves, for QR cards) while
/// keeping the private material to complete handshakes that reference them
/// later.
///
/// One-time prekeys move through two pools: `available` (never handed out)
/// and `pending` (handed out in a bundle, not yet consumed by a completed
/// handshake). A key must stay reachable in `pending` after being handed
/// out — otherwise the peer who received its public half in a bundle could
/// never complete a handshake that references it.
#[derive(ZeroizeOnDrop)]
pub struct PrekeyStore {
    signed_prekey_secret: StaticSecret,
    #[zeroize(skip)] // Public keys aren't secret; the signature already commits to them.
    signed_prekey_signature: [u8; 64],
    available_one_time_secrets: Vec<StaticSecret>,
    pending_one_time_secrets: Vec<StaticSecret>,
}

impl PrekeyStore {
    /// Generates a fresh signed prekey and `one_time_count` one-time
    /// prekeys, signing the signed prekey with `identity`.
    pub fn generate(
        identity: &IdentityKeyPair,
        rng: &mut impl rand_core::CryptoRngCore,
        one_time_count: usize,
    ) -> Self {
        let signed_prekey_secret = StaticSecret::random_from_rng(&mut *rng);
        let signed_prekey_public = X25519PublicKey::from(&signed_prekey_secret);
        let signed_prekey_signature = sign_prekey(identity, &signed_prekey_public);

        let available_one_time_secrets = (0..one_time_count)
            .map(|_| StaticSecret::random_from_rng(&mut *rng))
            .collect();

        Self {
            signed_prekey_secret,
            signed_prekey_signature,
            available_one_time_secrets,
            pending_one_time_secrets: Vec::new(),
        }
    }

    /// The public bundle to hand out (e.g. embed in a QR contact card).
    /// Moves one one-time prekey from `available` to `pending` each call,
    /// so the same one-time key is never handed to two different peers,
    /// while keeping its private half reachable for [`Self::take_one_time_secret`].
    pub fn public_bundle(
        &mut self,
        identity: IdentityPublicKey,
        identity_agreement_key: SignedAgreementKeyPublic,
    ) -> PrekeyBundle {
        let one_time_public = self.available_one_time_secrets.pop().map(|secret| {
            let public = X25519PublicKey::from(&secret);
            self.pending_one_time_secrets.push(secret);
            public
        });

        PrekeyBundle {
            identity,
            identity_agreement_key,
            signed_prekey: SignedPrekeyPublic {
                public: X25519PublicKey::from(&self.signed_prekey_secret),
                signature: self.signed_prekey_signature,
            },
            one_time_prekey: one_time_public,
        }
    }

    pub fn signed_prekey_secret(&self) -> &StaticSecret {
        &self.signed_prekey_secret
    }

    /// An owned copy of the signed prekey secret, for the one case that
    /// needs to hold onto it independently of this store: bootstrapping a
    /// responder's [`crate::ratchet::DoubleRatchet`], which reuses the
    /// signed prekey as its first ratchet key. The store keeps its own
    /// copy too, since the same signed prekey is meant to be handed out to
    /// many contacts until it is rotated.
    pub fn clone_signed_prekey_secret(&self) -> StaticSecret {
        self.signed_prekey_secret.clone()
    }

    /// Finds and removes the private one-time prekey matching `public`, if
    /// we still have it pending. Returns `None` if it was already consumed
    /// or never existed — callers must fall back to the
    /// no-one-time-prekey X3DH variant in that case, exactly as the
    /// initiator would if it never received a one-time prekey to begin
    /// with.
    pub fn take_one_time_secret(&mut self, public: &X25519PublicKey) -> Option<StaticSecret> {
        let index = self
            .pending_one_time_secrets
            .iter()
            .position(|secret| &X25519PublicKey::from(secret) == public)?;
        Some(self.pending_one_time_secrets.remove(index))
    }

    /// How many one-time prekeys are still available to hand out (does not
    /// count ones already handed out but not yet consumed).
    pub fn one_time_prekey_count(&self) -> usize {
        self.available_one_time_secrets.len()
    }

    /// Serializes the whole store — signed prekey, its signature, and both
    /// one-time prekey pools — so it can be persisted across app restarts.
    /// This is secret material; callers must store it as securely as the
    /// identity key.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.signed_prekey_secret.to_bytes());
        out.extend_from_slice(&self.signed_prekey_signature);
        write_secret_list(&mut out, &self.available_one_time_secrets);
        write_secret_list(&mut out, &self.pending_one_time_secrets);
        out
    }

    pub fn from_bytes(input: &[u8]) -> Result<Self> {
        if input.len() < 32 + 64 {
            return Err(CryptoError::Decode("prekey store bytes too short"));
        }
        let signed_prekey_secret = StaticSecret::from(read_key(&input[..32])?);
        let signed_prekey_signature: [u8; 64] = input[32..96].try_into().unwrap();

        let mut offset = 96;
        let (available_one_time_secrets, consumed) = read_secret_list(&input[offset..])?;
        offset += consumed;
        let (pending_one_time_secrets, _consumed) = read_secret_list(&input[offset..])?;

        Ok(Self {
            signed_prekey_secret,
            signed_prekey_signature,
            available_one_time_secrets,
            pending_one_time_secrets,
        })
    }
}

fn read_key(bytes: &[u8]) -> Result<[u8; 32]> {
    bytes.try_into().map_err(|_| CryptoError::InvalidKeyLength {
        expected: 32,
        actual: bytes.len(),
    })
}

fn write_secret_list(out: &mut Vec<u8>, secrets: &[StaticSecret]) {
    varint_encode(secrets.len() as u64, out);
    for secret in secrets {
        out.extend_from_slice(&secret.to_bytes());
    }
}

fn read_secret_list(input: &[u8]) -> Result<(Vec<StaticSecret>, usize)> {
    let (count, mut offset) = varint_decode(input)?;
    let mut secrets = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let end = offset + 32;
        let bytes = input
            .get(offset..end)
            .ok_or(CryptoError::Decode("truncated one-time prekey list"))?;
        secrets.push(StaticSecret::from(read_key(bytes)?));
        offset = end;
    }
    Ok((secrets, offset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::AgreementKeyPair;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    #[test]
    fn hands_out_a_one_time_prekey_and_consumes_it() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let identity = IdentityKeyPair::generate(&mut rng);
        let agreement = AgreementKeyPair::generate(&mut rng);
        let mut store = PrekeyStore::generate(&identity, &mut rng, 2);
        assert_eq!(store.one_time_prekey_count(), 2);

        let bundle =
            store.public_bundle(identity.public_key(), agreement.sign_with(&identity));
        let otp_public = bundle.one_time_prekey.expect("one-time prekey expected");
        assert_eq!(store.one_time_prekey_count(), 1);

        let taken = store.take_one_time_secret(&otp_public);
        assert!(taken.is_some());
        // Consumed — a second lookup for the same public key must miss.
        assert!(store.take_one_time_secret(&otp_public).is_none());
    }

    #[test]
    fn falls_back_to_no_one_time_prekey_once_pool_is_empty() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let identity = IdentityKeyPair::generate(&mut rng);
        let agreement = AgreementKeyPair::generate(&mut rng);
        let mut store = PrekeyStore::generate(&identity, &mut rng, 1);

        let signed_agreement = agreement.sign_with(&identity);
        let _first = store.public_bundle(identity.public_key(), signed_agreement);
        let second = store.public_bundle(identity.public_key(), signed_agreement);
        assert!(second.one_time_prekey.is_none());
    }

    #[test]
    fn round_trips_through_bytes_preserving_available_and_pending_pools() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let identity = IdentityKeyPair::generate(&mut rng);
        let agreement = AgreementKeyPair::generate(&mut rng);
        let mut store = PrekeyStore::generate(&identity, &mut rng, 3);

        let signed_agreement = agreement.sign_with(&identity);
        let bundle = store.public_bundle(identity.public_key(), signed_agreement);
        let handed_out_otp = bundle.one_time_prekey.unwrap();
        assert_eq!(store.one_time_prekey_count(), 2);

        let restored = PrekeyStore::from_bytes(&store.to_bytes()).unwrap();
        assert_eq!(restored.one_time_prekey_count(), 2);

        let mut restored = restored;
        let taken = restored.take_one_time_secret(&handed_out_otp);
        assert!(taken.is_some(), "pending prekey must survive the round trip");

        assert_eq!(
            X25519PublicKey::from(&restored.signed_prekey_secret),
            X25519PublicKey::from(&store.signed_prekey_secret)
        );
    }
}
