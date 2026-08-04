//! PQXDH ("Post-Quantum Extended Triple Diffie-Hellman"), the hybrid
//! key-agreement Signal deploys, adapted for a serverless/QR world: the
//! "prekey bundle" that X3DH normally fetches from a server is instead
//! scanned from a QR code or received directly from the peer.
//!
//! Four Diffie-Hellman computations combine a long-term identity key, a
//! medium-term signed prekey, and (when available) a single-use one-time
//! prekey, so that compromising any one of them alone does not expose the
//! session:
//!
//!   DH1 = DH(IK_initiator,  SPK_responder)
//!   DH2 = DH(EK_initiator,  IK_responder)
//!   DH3 = DH(EK_initiator,  SPK_responder)
//!   DH4 = DH(EK_initiator,  OPK_responder)   [only if an OPK was offered]
//!
//! On top of those, the initiator encapsulates an ML-KEM-768 shared secret to
//! the responder's post-quantum prekey (a one-time PQ prekey if offered, else
//! the signed "last resort" PQ prekey) and folds it into the same key
//! schedule:
//!
//!   SS_pq = ML-KEM-Encaps(PQ_prekey_responder)
//!
//! A quantum computer breaks every DH above but has no known efficient attack
//! on ML-KEM, so a session negotiated with a PQ-capable peer is safe against
//! "harvest now, decrypt later". When the peer's bundle predates PQ support
//! (`signed_pq_prekey` absent), the handshake gracefully degrades to
//! classical X3DH.

use hkdf::Hkdf;
use rand_core::CryptoRngCore;
use sha2::Sha256;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

use crate::error::{CryptoError, Result};
use crate::identity::{AgreementKeyPair, IdentityKeyPair, IdentityPublicKey, SignedAgreementKeyPublic};
use crate::pqkem::{PqCiphertext, PqSharedSecret};
use crate::prekey::{PrekeyBundle, PrekeyStore};

use super::shared_secret::SharedSecret;

/// What the initiator sends the responder so the responder can complete the
/// same PQXDH computation. Self-contained: the responder needs nothing else
/// beyond this message plus their own private prekey material.
#[derive(Clone, Debug)]
pub struct InitialMessage {
    pub initiator_identity: IdentityPublicKey,
    pub initiator_agreement_key: SignedAgreementKeyPublic,
    pub ephemeral_key: X25519PublicKey,
    pub used_one_time_prekey: Option<X25519PublicKey>,
    /// The ML-KEM ciphertext the responder decapsulates to recover the
    /// post-quantum shared secret. `None` only when the responder's bundle
    /// had no PQ prekey (a legacy peer), in which case the handshake was
    /// classical X3DH.
    pub pq_ciphertext: Option<PqCiphertext>,
}

#[derive(Debug)]
pub struct HandshakeResult {
    pub shared_secret: SharedSecret,
    pub initial_message: InitialMessage,
}

/// Wire-format version tag for the initial message. `1` = classical (no PQ
/// ciphertext); `2` = PQXDH (carries the ML-KEM ciphertext).
const MSG_V1: u8 = 1;
const MSG_V2: u8 = 2;

impl InitialMessage {
    /// The wire format sent to the responder to complete the handshake:
    /// every field is public (it's exactly what a passive network observer
    /// would see anyway). Prefixed with a version byte so a PQ ciphertext can
    /// be appended without breaking a legacy decoder.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + 32 + 32 + 64 + 32 + 1 + 32 + 1 + 1088);
        out.push(MSG_V2);
        out.extend_from_slice(&self.initiator_identity.to_bytes());
        out.extend_from_slice(self.initiator_agreement_key.public.as_bytes());
        out.extend_from_slice(&self.initiator_agreement_key.signature);
        out.extend_from_slice(self.ephemeral_key.as_bytes());
        match self.used_one_time_prekey {
            Some(otp) => {
                out.push(1);
                out.extend_from_slice(otp.as_bytes());
            }
            None => out.push(0),
        }
        match &self.pq_ciphertext {
            Some(ct) => {
                out.push(1);
                out.extend_from_slice(ct.as_bytes());
            }
            None => out.push(0),
        }
        out
    }

    pub fn from_bytes(input: &[u8]) -> Result<Self> {
        // Legacy unversioned messages are 161 (no OTP) or 193 (with OTP)
        // bytes — sizes a v2 message (which carries the version byte and, when
        // present, a 1088-byte ciphertext) can't collide with, so length
        // disambiguates an old message from a versioned one.
        match input.len() {
            161 | 193 => return Self::from_bytes_body(input, false),
            _ => {}
        }
        match input.first().copied() {
            Some(MSG_V2) => Self::from_bytes_body(&input[1..], true),
            Some(MSG_V1) => Self::from_bytes_body(&input[1..], false),
            _ => Err(CryptoError::Decode("unrecognized initial message version")),
        }
    }

    /// Shared decoder. `has_pq` selects whether a trailing PQ-ciphertext block
    /// is expected after the one-time-prekey field.
    fn from_bytes_body(input: &[u8], has_pq: bool) -> Result<Self> {
        const FIXED_LEN: usize = 32 + 32 + 64 + 32 + 1;
        if input.len() < FIXED_LEN {
            return Err(CryptoError::Decode("initial message too short"));
        }

        let initiator_identity = IdentityPublicKey::from_bytes(&input[0..32])?;
        let agreement_public = crate::prekey::x25519_public_from_bytes(&input[32..64])?;
        let agreement_signature: [u8; 64] = input[64..128].try_into().unwrap();
        let ephemeral_key = crate::prekey::x25519_public_from_bytes(&input[128..160])?;

        let mut offset = 160;
        let used_one_time_prekey = match input[offset] {
            0 => {
                offset += 1;
                None
            }
            1 => {
                offset += 1;
                let otp_bytes = input
                    .get(offset..offset + 32)
                    .ok_or(CryptoError::Decode("truncated one-time prekey reference"))?;
                offset += 32;
                Some(crate::prekey::x25519_public_from_bytes(otp_bytes)?)
            }
            _ => return Err(CryptoError::Decode("invalid one-time-prekey flag byte")),
        };

        let pq_ciphertext = if has_pq {
            match input.get(offset).copied() {
                Some(0) => None,
                Some(1) => {
                    offset += 1;
                    let ct_bytes = input
                        .get(offset..offset + crate::pqkem::PQ_CIPHERTEXT_LEN)
                        .ok_or(CryptoError::Decode("truncated PQ ciphertext"))?;
                    Some(PqCiphertext::from_bytes(ct_bytes)?)
                }
                _ => return Err(CryptoError::Decode("invalid PQ-ciphertext flag byte")),
            }
        } else {
            None
        };

        Ok(Self {
            initiator_identity,
            initiator_agreement_key: SignedAgreementKeyPublic {
                public: agreement_public,
                signature: agreement_signature,
            },
            ephemeral_key,
            used_one_time_prekey,
            pq_ciphertext,
        })
    }
}

/// Runs the initiator ("Alice") side: verifies the peer's bundle, performs
/// the four DH computations against a fresh ephemeral key, and returns both
/// the derived secret and the message to send the responder.
pub fn initiate(
    rng: &mut impl CryptoRngCore,
    my_identity: &IdentityKeyPair,
    my_agreement: &AgreementKeyPair,
    their_bundle: &PrekeyBundle,
) -> Result<HandshakeResult> {
    their_bundle.verify()?;

    let ephemeral_secret = StaticSecret::random_from_rng(&mut *rng);
    let ephemeral_public = X25519PublicKey::from(&ephemeral_secret);

    let dh1 = my_agreement
        .secret()
        .diffie_hellman(&their_bundle.signed_prekey.public);
    let dh2 = ephemeral_secret.diffie_hellman(&their_bundle.identity_agreement_key.public);
    let dh3 = ephemeral_secret.diffie_hellman(&their_bundle.signed_prekey.public);
    let dh4 = their_bundle
        .one_time_prekey
        .map(|opk| ephemeral_secret.diffie_hellman(&opk));

    // Post-quantum half: encapsulate to the one-time PQ prekey if the bundle
    // offered one, otherwise to the signed ("last resort") PQ prekey. A
    // legacy bundle with no PQ prekey at all degrades to classical X3DH.
    let pq_target = their_bundle
        .one_time_pq_prekey
        .as_ref()
        .or(their_bundle.signed_pq_prekey.as_ref().map(|s| &s.public));
    let (pq_ciphertext, pq_shared) = match pq_target {
        Some(pk) => {
            let (ct, ss) = pk.encapsulate(rng)?;
            (Some(ct), Some(ss))
        }
        None => (None, None),
    };

    let shared_secret = derive_shared_secret(&dh1, &dh2, &dh3, dh4.as_ref(), pq_shared.as_ref());

    Ok(HandshakeResult {
        shared_secret,
        initial_message: InitialMessage {
            initiator_identity: my_identity.public_key(),
            initiator_agreement_key: my_agreement.sign_with(my_identity),
            ephemeral_key: ephemeral_public,
            used_one_time_prekey: their_bundle.one_time_prekey,
            pq_ciphertext,
        },
    })
}

/// Runs the responder ("Bob") side against an [`InitialMessage`] received
/// from the initiator, consuming the referenced one-time prekey (if any)
/// from `my_prekey_store`. Produces the same shared secret `initiate`
/// produced, provided both sides' keys line up.
pub fn respond(
    my_agreement: &AgreementKeyPair,
    my_prekey_store: &mut PrekeyStore,
    message: &InitialMessage,
) -> Result<SharedSecret> {
    message
        .initiator_agreement_key
        .verify(&message.initiator_identity)?;

    let one_time_secret = match message.used_one_time_prekey {
        Some(public) => Some(
            my_prekey_store
                .take_one_time_secret(&public)
                .ok_or(CryptoError::PrekeyExhausted)?,
        ),
        None => None,
    };

    let dh1 = my_prekey_store
        .signed_prekey_secret()
        .diffie_hellman(&message.initiator_agreement_key.public);
    let dh2 = my_agreement.secret().diffie_hellman(&message.ephemeral_key);
    let dh3 = my_prekey_store
        .signed_prekey_secret()
        .diffie_hellman(&message.ephemeral_key);
    let dh4 = one_time_secret
        .as_ref()
        .map(|secrets| secrets.x25519.diffie_hellman(&message.ephemeral_key));

    // Post-quantum half: decapsulate with the one-time PQ secret if a
    // one-time prekey was used, otherwise the signed PQ secret. Both sides
    // must agree on which prekey — the `used_one_time_prekey` reference above
    // is exactly that agreement.
    let pq_shared = match &message.pq_ciphertext {
        Some(ct) => {
            let pq_secret = one_time_secret
                .as_ref()
                .map(|s| &s.pq_secret)
                .unwrap_or_else(|| my_prekey_store.signed_pq_secret());
            Some(pq_secret.decapsulate(ct)?)
        }
        None => None,
    };

    Ok(derive_shared_secret(
        &dh1,
        &dh2,
        &dh3,
        dh4.as_ref(),
        pq_shared.as_ref(),
    ))
}

const KDF_INFO: &[u8] = b"SpiritChat-X3DH-v1";

fn derive_shared_secret(
    dh1: &x25519_dalek::SharedSecret,
    dh2: &x25519_dalek::SharedSecret,
    dh3: &x25519_dalek::SharedSecret,
    dh4: Option<&x25519_dalek::SharedSecret>,
    pq_shared: Option<&PqSharedSecret>,
) -> SharedSecret {
    // A leading run of 0xFF bytes, the same length as a curve point,
    // guarantees full-entropy HKDF input even in the (cryptographically
    // negligible but real) case that a DH output lands on a low-order
    // point — the standard X3DH mitigation.
    let mut ikm = vec![0xffu8; 32];
    ikm.extend_from_slice(dh1.as_bytes());
    ikm.extend_from_slice(dh2.as_bytes());
    ikm.extend_from_slice(dh3.as_bytes());
    if let Some(dh4) = dh4 {
        ikm.extend_from_slice(dh4.as_bytes());
    }
    // The ML-KEM shared secret is appended last, so a classical-only handshake
    // (legacy peer, no PQ secret) derives exactly the same key it did before
    // PQ support existed — the KDF domain is unchanged and back-compatible.
    if let Some(pq) = pq_shared {
        ikm.extend_from_slice(pq.as_bytes());
    }

    let hk = Hkdf::<Sha256>::new(Some(&[0u8; 32]), &ikm);
    let mut okm = [0u8; 32];
    hk.expand(KDF_INFO, &mut okm)
        .expect("32 bytes is a valid HKDF-SHA256 output length");

    SharedSecret(okm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prekey::PrekeyStore;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    struct Party {
        identity: IdentityKeyPair,
        agreement: AgreementKeyPair,
        prekeys: PrekeyStore,
    }

    impl Party {
        fn new(seed: u64, one_time_count: usize) -> Self {
            let mut rng = ChaCha20Rng::seed_from_u64(seed);
            let identity = IdentityKeyPair::generate(&mut rng);
            let agreement = AgreementKeyPair::generate(&mut rng);
            let prekeys = PrekeyStore::generate(&identity, &mut rng, one_time_count);
            Self {
                identity,
                agreement,
                prekeys,
            }
        }

        fn bundle(&mut self) -> PrekeyBundle {
            let signed_agreement = self.agreement.sign_with(&self.identity);
            self.prekeys
                .public_bundle(self.identity.public_key(), signed_agreement)
        }
    }

    #[test]
    fn both_sides_derive_the_same_secret_with_a_one_time_prekey() {
        let alice = Party::new(1, 1);
        let mut bob = Party::new(2, 1);
        let mut rng = ChaCha20Rng::seed_from_u64(99);

        let bob_bundle = bob.bundle();
        assert!(bob_bundle.one_time_prekey.is_some());

        let result =
            initiate(&mut rng, &alice.identity, &alice.agreement, &bob_bundle).unwrap();

        let bob_secret =
            respond(&bob.agreement, &mut bob.prekeys, &result.initial_message).unwrap();

        assert_eq!(result.shared_secret.as_bytes(), bob_secret.as_bytes());
    }

    #[test]
    fn both_sides_derive_the_same_secret_without_a_one_time_prekey() {
        let alice = Party::new(1, 0);
        let mut bob = Party::new(2, 0);
        let mut rng = ChaCha20Rng::seed_from_u64(99);

        let bob_bundle = bob.bundle();
        assert!(bob_bundle.one_time_prekey.is_none());

        let result =
            initiate(&mut rng, &alice.identity, &alice.agreement, &bob_bundle).unwrap();
        let bob_secret =
            respond(&bob.agreement, &mut bob.prekeys, &result.initial_message).unwrap();

        assert_eq!(result.shared_secret.as_bytes(), bob_secret.as_bytes());
    }

    #[test]
    fn different_handshakes_produce_different_secrets() {
        let alice = Party::new(1, 2);
        let mut bob = Party::new(2, 2);
        let mut rng = ChaCha20Rng::seed_from_u64(99);

        let first = initiate(&mut rng, &alice.identity, &alice.agreement, &bob.bundle()).unwrap();
        let second = initiate(&mut rng, &alice.identity, &alice.agreement, &bob.bundle()).unwrap();

        assert_ne!(first.shared_secret.as_bytes(), second.shared_secret.as_bytes());
    }

    #[test]
    fn responder_rejects_a_replayed_one_time_prekey_reference() {
        let alice = Party::new(1, 1);
        let mut bob = Party::new(2, 1);
        let mut rng = ChaCha20Rng::seed_from_u64(99);

        let bob_bundle = bob.bundle();
        let result =
            initiate(&mut rng, &alice.identity, &alice.agreement, &bob_bundle).unwrap();

        respond(&bob.agreement, &mut bob.prekeys, &result.initial_message).unwrap();
        // Same InitialMessage delivered twice (e.g. a network replay) must
        // fail the second time: the one-time prekey is already consumed.
        let replay = respond(&bob.agreement, &mut bob.prekeys, &result.initial_message);
        assert_eq!(replay.unwrap_err(), CryptoError::PrekeyExhausted);
    }

    #[test]
    fn initiator_rejects_a_bundle_with_a_forged_signed_prekey() {
        let alice = Party::new(1, 1);
        let mut bob = Party::new(2, 1);
        let mallory = Party::new(3, 1).bundle();
        let mut rng = ChaCha20Rng::seed_from_u64(99);

        let mut tampered_bundle = bob.bundle();
        tampered_bundle.signed_prekey = mallory.signed_prekey;

        let err = initiate(&mut rng, &alice.identity, &alice.agreement, &tampered_bundle)
            .unwrap_err();
        assert_eq!(err, CryptoError::SignatureInvalid);
    }

    #[test]
    fn initial_message_round_trips_through_bytes_and_still_completes_the_handshake() {
        let alice = Party::new(1, 1);
        let mut bob = Party::new(2, 1);
        let mut rng = ChaCha20Rng::seed_from_u64(99);

        let result = initiate(&mut rng, &alice.identity, &alice.agreement, &bob.bundle()).unwrap();
        let restored = InitialMessage::from_bytes(&result.initial_message.to_bytes()).unwrap();

        let bob_secret = respond(&bob.agreement, &mut bob.prekeys, &restored).unwrap();
        assert_eq!(result.shared_secret.as_bytes(), bob_secret.as_bytes());
    }

    #[test]
    fn a_pq_capable_handshake_carries_a_kem_ciphertext() {
        let alice = Party::new(1, 1);
        let mut bob = Party::new(2, 1);
        let mut rng = ChaCha20Rng::seed_from_u64(99);

        let result = initiate(&mut rng, &alice.identity, &alice.agreement, &bob.bundle()).unwrap();
        assert!(
            result.initial_message.pq_ciphertext.is_some(),
            "a handshake against a PQ-capable bundle must include an ML-KEM ciphertext"
        );
    }

    #[test]
    fn corrupting_the_kem_ciphertext_makes_the_two_sides_diverge() {
        // ML-KEM's implicit rejection means a tampered ciphertext doesn't
        // error on decapsulation — it yields an unrelated secret, so the two
        // sides simply derive different session keys and the ratchet never
        // syncs. That's the intended failure mode.
        let alice = Party::new(1, 1);
        let mut bob = Party::new(2, 1);
        let mut rng = ChaCha20Rng::seed_from_u64(99);

        let mut result =
            initiate(&mut rng, &alice.identity, &alice.agreement, &bob.bundle()).unwrap();
        let ct = result.initial_message.pq_ciphertext.as_mut().unwrap();
        ct.0[0] ^= 0xff;

        let bob_secret =
            respond(&bob.agreement, &mut bob.prekeys, &result.initial_message).unwrap();
        assert_ne!(result.shared_secret.as_bytes(), bob_secret.as_bytes());
    }

    #[test]
    fn falls_back_to_classical_x3dh_against_a_legacy_bundle() {
        // A peer whose bundle predates PQ support (no signed PQ prekey) must
        // still complete a handshake — classically — and both sides agree.
        let alice = Party::new(1, 0);
        let mut bob = Party::new(2, 0);
        let mut rng = ChaCha20Rng::seed_from_u64(99);

        let mut bob_bundle = bob.bundle();
        bob_bundle.signed_pq_prekey = None; // simulate a pre-PQ peer
        bob_bundle.one_time_pq_prekey = None;

        let result = initiate(&mut rng, &alice.identity, &alice.agreement, &bob_bundle).unwrap();
        assert!(
            result.initial_message.pq_ciphertext.is_none(),
            "no PQ prekey on offer means no ciphertext"
        );

        let bob_secret =
            respond(&bob.agreement, &mut bob.prekeys, &result.initial_message).unwrap();
        assert_eq!(result.shared_secret.as_bytes(), bob_secret.as_bytes());
    }
}
