use x25519_dalek::PublicKey as X25519PublicKey;

use crate::error::{CryptoError, Result};
use crate::identity::{IdentityPublicKey, SignedAgreementKeyPublic};
use crate::pqkem::{PqKemPublicKey, PQ_PUBLIC_KEY_LEN};

/// A signed X25519 public key: the identity owner vouches ("signs") for this
/// agreement key, so a peer who trusts the identity can trust the key too.
#[derive(Clone, Copy, Debug)]
pub struct SignedPrekeyPublic {
    pub public: X25519PublicKey,
    pub signature: [u8; 64],
}

/// A signed ML-KEM-768 public key — the post-quantum counterpart of
/// [`SignedPrekeyPublic`]. The identity signs the KEM public key so a peer
/// can trust it came from the same owner, exactly as with the X25519 signed
/// prekey. This is the "last resort" PQ prekey: it's used to encapsulate the
/// quantum-safe half of the handshake whenever no one-time PQ prekey is on
/// offer.
#[derive(Clone, Debug)]
pub struct SignedPqPrekeyPublic {
    pub public: PqKemPublicKey,
    pub signature: [u8; 64],
}

/// Everything a peer needs to start a PQXDH handshake with someone, entirely
/// offline — this is the "contact card" payload, distributed as a
/// content-addressed blob over the P2P network (so its size is unconstrained;
/// the post-quantum keys make it a few kilobytes).
///
/// `one_time_prekey`/`one_time_pq_prekey` are a matched pair consumed by the
/// first handshake that uses them; both are present or both absent. Omitting
/// them still yields a secure handshake (the classical signed prekey and the
/// signed PQ prekey carry it), just without the extra forward-secrecy margin
/// a one-time key adds.
#[derive(Clone, Debug)]
pub struct PrekeyBundle {
    pub identity: IdentityPublicKey,
    pub identity_agreement_key: SignedAgreementKeyPublic,
    pub signed_prekey: SignedPrekeyPublic,
    /// The post-quantum signed prekey. Always present in a bundle this
    /// version produces; `None` only when decoding a legacy (pre-PQ) bundle
    /// from an old peer, in which case the handshake falls back to classical
    /// X3DH.
    pub signed_pq_prekey: Option<SignedPqPrekeyPublic>,
    pub one_time_prekey: Option<X25519PublicKey>,
    /// The PQ one-time prekey paired with `one_time_prekey` — present iff
    /// `one_time_prekey` is present and this is a PQ-capable bundle.
    pub one_time_pq_prekey: Option<PqKemPublicKey>,
}

/// Domain-separation prefix mixed into every signed-prekey signature, so a
/// signature can never be replayed as proof of a different kind of claim.
const SIGNED_PREKEY_CONTEXT: &[u8] = b"SpiritChat-SignedPrekey-v1";
/// Same idea for the post-quantum signed prekey — a distinct context so a PQ
/// prekey signature can never be confused with an X25519 prekey signature.
const SIGNED_PQ_PREKEY_CONTEXT: &[u8] = b"SpiritChat-SignedPqPrekey-v1";

/// Wire-format version tag. `1` = classical X3DH bundle (no PQ fields); `2` =
/// PQXDH bundle carrying the signed PQ prekey and optional PQ one-time prekey.
const BUNDLE_V1: u8 = 1;
const BUNDLE_V2: u8 = 2;

impl PrekeyBundle {
    /// Verifies the signed prekey, the identity agreement key, and (when
    /// present) the signed PQ prekey against `identity`. Callers MUST call
    /// this before using a bundle scanned from a QR code or received from a
    /// peer — an unverified bundle lets an attacker substitute their own keys.
    pub fn verify(&self) -> Result<()> {
        self.identity_agreement_key.verify(&self.identity)?;

        let mut message = Vec::with_capacity(SIGNED_PREKEY_CONTEXT.len() + 32);
        message.extend_from_slice(SIGNED_PREKEY_CONTEXT);
        message.extend_from_slice(self.signed_prekey.public.as_bytes());
        self.identity
            .verify(&message, &self.signed_prekey.signature)?;

        if let Some(pq) = &self.signed_pq_prekey {
            let mut pq_message = Vec::with_capacity(SIGNED_PQ_PREKEY_CONTEXT.len() + PQ_PUBLIC_KEY_LEN);
            pq_message.extend_from_slice(SIGNED_PQ_PREKEY_CONTEXT);
            pq_message.extend_from_slice(&pq.public.to_bytes());
            self.identity.verify(&pq_message, &pq.signature)?;
        }

        Ok(())
    }

    /// The wire format that goes inside a QR contact card: a version byte
    /// followed by every public field. All public data — safe to put in a QR
    /// code or a URL, but still unverified until [`Self::verify`] is called
    /// by whoever scans it.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(BUNDLE_V2);
        out.extend_from_slice(&self.identity.to_bytes());
        out.extend_from_slice(self.identity_agreement_key.public.as_bytes());
        out.extend_from_slice(&self.identity_agreement_key.signature);
        out.extend_from_slice(self.signed_prekey.public.as_bytes());
        out.extend_from_slice(&self.signed_prekey.signature);

        // Signed PQ prekey (present in every v2 bundle we produce).
        match &self.signed_pq_prekey {
            Some(pq) => {
                out.push(1);
                out.extend_from_slice(&pq.public.to_bytes());
                out.extend_from_slice(&pq.signature);
            }
            None => out.push(0),
        }

        // One-time prekey pair.
        match (self.one_time_prekey, &self.one_time_pq_prekey) {
            (Some(otp), Some(pq_otp)) => {
                out.push(1);
                out.extend_from_slice(otp.as_bytes());
                out.extend_from_slice(&pq_otp.to_bytes());
            }
            _ => out.push(0),
        }
        out
    }

    /// Legacy unversioned bundles are exactly 225 (no one-time prekey) or 257
    /// (with one) bytes — sizes that can never collide with a v2 bundle,
    /// which always carries the 1184-byte signed PQ prekey and is well over a
    /// kilobyte. That lets us disambiguate an old contact card (whose first
    /// byte is an identity key that might happen to equal a version tag) from
    /// a versioned one purely by length, with the version byte authoritative
    /// for everything else.
    pub fn from_bytes(input: &[u8]) -> Result<Self> {
        if input.len() == 225 || input.len() == 257 {
            return Self::from_bytes_v1(input);
        }
        match input.first().copied() {
            Some(BUNDLE_V2) => Self::from_bytes_v2(&input[1..]),
            Some(BUNDLE_V1) => Self::from_bytes_v1(&input[1..]),
            _ => Err(CryptoError::Decode("unrecognized prekey bundle version")),
        }
    }

    /// Legacy classical-X3DH bundle: identity ‖ agreement ‖ SPK ‖ 1-byte OTP flag.
    fn from_bytes_v1(input: &[u8]) -> Result<Self> {
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
            signed_pq_prekey: None,
            one_time_prekey,
            one_time_pq_prekey: None,
        })
    }

    /// PQXDH bundle (version tag already stripped).
    fn from_bytes_v2(input: &[u8]) -> Result<Self> {
        const HEAD_LEN: usize = 32 + 32 + 64 + 32 + 64;
        if input.len() < HEAD_LEN + 1 {
            return Err(CryptoError::Decode("contact card too short"));
        }

        let identity = IdentityPublicKey::from_bytes(&input[0..32])?;
        let agreement_public = x25519_public_from_bytes(&input[32..64])?;
        let agreement_signature: [u8; 64] = input[64..128].try_into().unwrap();
        let spk_public = x25519_public_from_bytes(&input[128..160])?;
        let spk_signature: [u8; 64] = input[160..224].try_into().unwrap();

        let mut offset = HEAD_LEN;
        let signed_pq_prekey = match input[offset] {
            0 => {
                offset += 1;
                None
            }
            1 => {
                offset += 1;
                let pk_end = offset + PQ_PUBLIC_KEY_LEN;
                let pk = PqKemPublicKey::from_bytes(
                    input
                        .get(offset..pk_end)
                        .ok_or(CryptoError::Decode("truncated signed PQ prekey"))?,
                )?;
                let sig_end = pk_end + 64;
                let sig: [u8; 64] = input
                    .get(pk_end..sig_end)
                    .ok_or(CryptoError::Decode("truncated signed PQ prekey signature"))?
                    .try_into()
                    .unwrap();
                offset = sig_end;
                Some(SignedPqPrekeyPublic {
                    public: pk,
                    signature: sig,
                })
            }
            _ => return Err(CryptoError::Decode("invalid signed-PQ-prekey flag byte")),
        };

        let (one_time_prekey, one_time_pq_prekey) = match input.get(offset).copied() {
            Some(0) => (None, None),
            Some(1) => {
                offset += 1;
                let otp_end = offset + 32;
                let otp = x25519_public_from_bytes(
                    input
                        .get(offset..otp_end)
                        .ok_or(CryptoError::Decode("truncated one-time prekey"))?,
                )?;
                let pq_end = otp_end + PQ_PUBLIC_KEY_LEN;
                let pq_otp = PqKemPublicKey::from_bytes(
                    input
                        .get(otp_end..pq_end)
                        .ok_or(CryptoError::Decode("truncated one-time PQ prekey"))?,
                )?;
                (Some(otp), Some(pq_otp))
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
            signed_pq_prekey,
            one_time_prekey,
            one_time_pq_prekey,
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

pub(crate) fn sign_pq_prekey(
    identity: &crate::identity::IdentityKeyPair,
    prekey_public: &PqKemPublicKey,
) -> [u8; 64] {
    let mut message = Vec::with_capacity(SIGNED_PQ_PREKEY_CONTEXT.len() + PQ_PUBLIC_KEY_LEN);
    message.extend_from_slice(SIGNED_PQ_PREKEY_CONTEXT);
    message.extend_from_slice(&prekey_public.to_bytes());
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
        assert!(bundle.signed_pq_prekey.is_some());
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
    fn rejects_a_swapped_signed_pq_prekey() {
        let (identity, agreement, mut store) = make_bundle(1);
        let mut bundle =
            store.public_bundle(identity.public_key(), agreement.sign_with(&identity));

        let (other_identity, other_agreement, mut other_store) = make_bundle(2);
        let other_bundle = other_store
            .public_bundle(other_identity.public_key(), other_agreement.sign_with(&other_identity));
        bundle.signed_pq_prekey = other_bundle.signed_pq_prekey;

        assert!(bundle.verify().is_err(), "a PQ prekey signed by a different identity must be rejected");
    }

    #[test]
    fn round_trips_through_bytes_with_a_one_time_prekey() {
        let (identity, agreement, mut store) = make_bundle(1);
        let bundle = store.public_bundle(identity.public_key(), agreement.sign_with(&identity));
        assert!(bundle.one_time_prekey.is_some());
        assert!(bundle.one_time_pq_prekey.is_some());

        let restored = super::PrekeyBundle::from_bytes(&bundle.to_bytes()).unwrap();
        restored.verify().unwrap();
        assert_eq!(restored.identity, bundle.identity);
        assert_eq!(
            restored.one_time_prekey.unwrap().as_bytes(),
            bundle.one_time_prekey.unwrap().as_bytes()
        );
        assert_eq!(
            restored.one_time_pq_prekey.as_ref().unwrap().to_bytes(),
            bundle.one_time_pq_prekey.as_ref().unwrap().to_bytes()
        );
        assert_eq!(
            restored.signed_pq_prekey.as_ref().unwrap().public.to_bytes(),
            bundle.signed_pq_prekey.as_ref().unwrap().public.to_bytes()
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
        assert!(bundle.one_time_pq_prekey.is_none());
        assert!(bundle.signed_pq_prekey.is_some());

        let restored = super::PrekeyBundle::from_bytes(&bundle.to_bytes()).unwrap();
        restored.verify().unwrap();
        assert!(restored.one_time_prekey.is_none());
        assert!(restored.one_time_pq_prekey.is_none());
    }

    #[test]
    fn decodes_a_legacy_v1_bundle_without_pq_fields() {
        // A hand-built classical bundle (as an old peer would send) must
        // still decode, with the PQ fields simply absent.
        let (identity, agreement, mut store) = make_bundle(1);
        let bundle = store.public_bundle(identity.public_key(), agreement.sign_with(&identity));

        // Re-serialize in the legacy v1 layout: no version byte, no PQ.
        let mut v1 = Vec::new();
        v1.extend_from_slice(&bundle.identity.to_bytes());
        v1.extend_from_slice(bundle.identity_agreement_key.public.as_bytes());
        v1.extend_from_slice(&bundle.identity_agreement_key.signature);
        v1.extend_from_slice(bundle.signed_prekey.public.as_bytes());
        v1.extend_from_slice(&bundle.signed_prekey.signature);
        v1.push(0); // no one-time prekey

        let restored = super::PrekeyBundle::from_bytes(&v1).unwrap();
        restored.verify().unwrap();
        assert!(restored.signed_pq_prekey.is_none());
        assert!(restored.one_time_prekey.is_none());
    }
}
