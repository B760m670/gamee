use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

use crate::encoding::{varint_decode, varint_encode};
use crate::error::{CryptoError, Result};
use crate::identity::{IdentityKeyPair, IdentityPublicKey, SignedAgreementKeyPublic};
use crate::pqkem::{
    PqKemKeyPair, PqKemPublicKey, PqKemSecretKey, PQ_PUBLIC_KEY_LEN, PQ_SECRET_KEY_LEN,
};

use super::bundle::{sign_pq_prekey, sign_prekey, PrekeyBundle, SignedPqPrekeyPublic, SignedPrekeyPublic};

/// A matched one-time prekey pair: an X25519 one-time prekey and the ML-KEM
/// one-time prekey handed out alongside it. They are generated, offered, and
/// consumed together, so a single `used_one_time_prekey` reference in the
/// initial message identifies both halves.
struct OneTimePrekey {
    x25519: StaticSecret,
    pq_secret: PqKemSecretKey,
    pq_public: PqKemPublicKey,
}

impl OneTimePrekey {
    fn generate(rng: &mut impl rand_core::CryptoRngCore) -> Self {
        let x25519 = StaticSecret::random_from_rng(&mut *rng);
        let pq = PqKemKeyPair::generate(&mut *rng);
        Self {
            x25519,
            pq_secret: pq.secret,
            pq_public: pq.public,
        }
    }

    fn x25519_public(&self) -> X25519PublicKey {
        X25519PublicKey::from(&self.x25519)
    }
}

/// The private material recovered when a responder consumes a one-time
/// prekey: the X25519 secret (for the classical DH) and the ML-KEM secret
/// (to decapsulate the initiator's ciphertext).
pub struct OneTimeSecrets {
    pub x25519: StaticSecret,
    pub pq_secret: PqKemSecretKey,
}

/// Holds the private half of the signed prekeys (classical + post-quantum)
/// plus one-time prekey pairs, and hands out [`PrekeyBundle`]s (the public
/// halves, for QR cards) while keeping the private material to complete
/// handshakes that reference them later.
///
/// One-time prekeys move through two pools: `available` (never handed out)
/// and `pending` (handed out in a bundle, not yet consumed by a completed
/// handshake). A key must stay reachable in `pending` after being handed
/// out — otherwise the peer who received its public half in a bundle could
/// never complete a handshake that references it.
///
/// Every secret-bearing field self-zeroizes on drop — `StaticSecret` (dalek)
/// and the inner ML-KEM `DecapsulationKey` inside [`PqKemSecretKey`] both
/// implement `ZeroizeOnDrop` — so this struct needs no explicit zeroizing
/// `Drop` of its own; dropping it drops each field, which wipes its secret.
pub struct PrekeyStore {
    signed_prekey_secret: StaticSecret,
    signed_prekey_signature: [u8; 64],
    signed_pq_secret: PqKemSecretKey,
    signed_pq_public: PqKemPublicKey,
    signed_pq_signature: [u8; 64],
    available_one_time: Vec<OneTimePrekey>,
    pending_one_time: Vec<OneTimePrekey>,
}

/// Magic prefix identifying a PQXDH-era (v2) serialized store. A legacy v1
/// store (classical prekeys only) never starts with this, so `from_bytes`
/// can reject it unambiguously and let the caller regenerate — a v1 store
/// carries no PQ material and can't be upgraded in place without the identity
/// key needed to sign a fresh PQ prekey.
const STORE_MAGIC_V2: &[u8; 4] = b"SPK2";

impl PrekeyStore {
    /// Generates fresh signed prekeys (classical + post-quantum) and
    /// `one_time_count` one-time prekey pairs, signing both signed prekeys
    /// with `identity`.
    pub fn generate(
        identity: &IdentityKeyPair,
        rng: &mut impl rand_core::CryptoRngCore,
        one_time_count: usize,
    ) -> Self {
        let signed_prekey_secret = StaticSecret::random_from_rng(&mut *rng);
        let signed_prekey_public = X25519PublicKey::from(&signed_prekey_secret);
        let signed_prekey_signature = sign_prekey(identity, &signed_prekey_public);

        let signed_pq = PqKemKeyPair::generate(&mut *rng);
        let signed_pq_signature = sign_pq_prekey(identity, &signed_pq.public);

        let available_one_time = (0..one_time_count)
            .map(|_| OneTimePrekey::generate(&mut *rng))
            .collect();

        Self {
            signed_prekey_secret,
            signed_prekey_signature,
            signed_pq_secret: signed_pq.secret,
            signed_pq_public: signed_pq.public,
            signed_pq_signature,
            available_one_time,
            pending_one_time: Vec::new(),
        }
    }

    /// The public bundle to hand out (e.g. embed in a QR contact card).
    /// Moves one one-time prekey pair from `available` to `pending` each
    /// call, so the same one-time key is never handed to two different peers,
    /// while keeping its private halves reachable for
    /// [`Self::take_one_time_secret`].
    pub fn public_bundle(
        &mut self,
        identity: IdentityPublicKey,
        identity_agreement_key: SignedAgreementKeyPublic,
    ) -> PrekeyBundle {
        let (one_time_prekey, one_time_pq_prekey) = match self.available_one_time.pop() {
            Some(otp) => {
                let x_public = otp.x25519_public();
                let pq_public = otp.pq_public.clone();
                self.pending_one_time.push(otp);
                (Some(x_public), Some(pq_public))
            }
            None => (None, None),
        };

        PrekeyBundle {
            identity,
            identity_agreement_key,
            signed_prekey: SignedPrekeyPublic {
                public: X25519PublicKey::from(&self.signed_prekey_secret),
                signature: self.signed_prekey_signature,
            },
            signed_pq_prekey: Some(SignedPqPrekeyPublic {
                public: self.signed_pq_public.clone(),
                signature: self.signed_pq_signature,
            }),
            one_time_prekey,
            one_time_pq_prekey,
        }
    }

    pub fn signed_prekey_secret(&self) -> &StaticSecret {
        &self.signed_prekey_secret
    }

    /// The signed post-quantum prekey secret — used to decapsulate the
    /// initiator's ML-KEM ciphertext when no one-time PQ prekey was used.
    pub fn signed_pq_secret(&self) -> &PqKemSecretKey {
        &self.signed_pq_secret
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

    /// Finds and removes the one-time prekey pair matching `public` (the
    /// X25519 half, as referenced in the initial message), if we still have
    /// it pending. Returns `None` if it was already consumed or never
    /// existed — callers must fall back to the no-one-time-prekey variant in
    /// that case, using the signed prekeys instead.
    pub fn take_one_time_secret(&mut self, public: &X25519PublicKey) -> Option<OneTimeSecrets> {
        let index = self
            .pending_one_time
            .iter()
            .position(|otp| &otp.x25519_public() == public)?;
        let otp = self.pending_one_time.remove(index);
        Some(OneTimeSecrets {
            x25519: otp.x25519,
            pq_secret: otp.pq_secret,
        })
    }

    /// How many one-time prekeys are still available to hand out (does not
    /// count ones already handed out but not yet consumed).
    pub fn one_time_prekey_count(&self) -> usize {
        self.available_one_time.len()
    }

    /// Serializes the whole store so it can be persisted across app
    /// restarts. This is secret material; callers must store it as securely
    /// as the identity key. Prefixed with a magic tag so a legacy store is
    /// rejected rather than misread.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(STORE_MAGIC_V2);
        out.extend_from_slice(&self.signed_prekey_secret.to_bytes());
        out.extend_from_slice(&self.signed_prekey_signature);
        out.extend_from_slice(&self.signed_pq_secret.to_bytes());
        out.extend_from_slice(&self.signed_pq_public.to_bytes());
        out.extend_from_slice(&self.signed_pq_signature);
        write_one_time_list(&mut out, &self.available_one_time);
        write_one_time_list(&mut out, &self.pending_one_time);
        out
    }

    pub fn from_bytes(input: &[u8]) -> Result<Self> {
        if input.len() < 4 || &input[0..4] != STORE_MAGIC_V2 {
            // Either a legacy (pre-PQ) store or corrupt data. The caller
            // should regenerate: a v1 store has no PQ material to migrate.
            return Err(CryptoError::Decode(
                "legacy or unrecognized prekey store; regenerate",
            ));
        }
        let mut offset = 4;

        let signed_prekey_secret = StaticSecret::from(read_key(slice(input, &mut offset, 32)?)?);
        let signed_prekey_signature: [u8; 64] = slice(input, &mut offset, 64)?.try_into().unwrap();
        let signed_pq_secret = PqKemSecretKey::from_bytes(slice(input, &mut offset, PQ_SECRET_KEY_LEN)?)?;
        let signed_pq_public = PqKemPublicKey::from_bytes(slice(input, &mut offset, PQ_PUBLIC_KEY_LEN)?)?;
        let signed_pq_signature: [u8; 64] = slice(input, &mut offset, 64)?.try_into().unwrap();

        let (available_one_time, consumed) = read_one_time_list(&input[offset..])?;
        offset += consumed;
        let (pending_one_time, _consumed) = read_one_time_list(&input[offset..])?;

        Ok(Self {
            signed_prekey_secret,
            signed_prekey_signature,
            signed_pq_secret,
            signed_pq_public,
            signed_pq_signature,
            available_one_time,
            pending_one_time,
        })
    }
}

/// Advances `offset` by `len`, returning the slice it stepped over.
fn slice<'a>(input: &'a [u8], offset: &mut usize, len: usize) -> Result<&'a [u8]> {
    let end = *offset + len;
    let out = input
        .get(*offset..end)
        .ok_or(CryptoError::Decode("truncated prekey store"))?;
    *offset = end;
    Ok(out)
}

fn read_key(bytes: &[u8]) -> Result<[u8; 32]> {
    bytes.try_into().map_err(|_| CryptoError::InvalidKeyLength {
        expected: 32,
        actual: bytes.len(),
    })
}

fn write_one_time_list(out: &mut Vec<u8>, entries: &[OneTimePrekey]) {
    varint_encode(entries.len() as u64, out);
    for otp in entries {
        out.extend_from_slice(&otp.x25519.to_bytes());
        out.extend_from_slice(&otp.pq_secret.to_bytes());
        out.extend_from_slice(&otp.pq_public.to_bytes());
    }
}

fn read_one_time_list(input: &[u8]) -> Result<(Vec<OneTimePrekey>, usize)> {
    let (count, mut offset) = varint_decode(input)?;
    let mut entries = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let x25519 = StaticSecret::from(read_key(slice(input, &mut offset, 32)?)?);
        let pq_secret = PqKemSecretKey::from_bytes(slice(input, &mut offset, PQ_SECRET_KEY_LEN)?)?;
        let pq_public = PqKemPublicKey::from_bytes(slice(input, &mut offset, PQ_PUBLIC_KEY_LEN)?)?;
        entries.push(OneTimePrekey {
            x25519,
            pq_secret,
            pq_public,
        });
    }
    Ok((entries, offset))
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
        assert!(bundle.one_time_pq_prekey.is_some());
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
        assert!(second.one_time_pq_prekey.is_none());
        // The signed PQ prekey is always offered even without a one-time one.
        assert!(second.signed_pq_prekey.is_some());
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
        assert_eq!(
            restored.signed_pq_public.to_bytes(),
            store.signed_pq_public.to_bytes()
        );
    }

    #[test]
    fn rejects_a_legacy_store_so_the_caller_regenerates() {
        // A blob without the v2 magic (e.g. an old classical-only store) must
        // be rejected, not silently misparsed.
        let legacy = vec![0u8; 200];
        assert!(PrekeyStore::from_bytes(&legacy).is_err());
    }
}
