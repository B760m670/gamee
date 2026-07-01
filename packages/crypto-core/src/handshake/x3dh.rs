//! X3DH ("Extended Triple Diffie-Hellman"), the same key-agreement design
//! Signal uses, adapted for a serverless/QR world: the "prekey bundle" that
//! X3DH normally fetches from a server is instead scanned from a QR code or
//! received directly from the peer.
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

use hkdf::Hkdf;
use rand_core::CryptoRngCore;
use sha2::Sha256;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

use crate::error::{CryptoError, Result};
use crate::identity::{AgreementKeyPair, IdentityKeyPair, IdentityPublicKey, SignedAgreementKeyPublic};
use crate::prekey::{PrekeyBundle, PrekeyStore};

use super::shared_secret::SharedSecret;

/// What the initiator sends the responder so the responder can complete the
/// same X3DH computation. Self-contained: the responder needs nothing else
/// beyond this message plus their own private prekey material.
#[derive(Clone, Copy, Debug)]
pub struct InitialMessage {
    pub initiator_identity: IdentityPublicKey,
    pub initiator_agreement_key: SignedAgreementKeyPublic,
    pub ephemeral_key: X25519PublicKey,
    pub used_one_time_prekey: Option<X25519PublicKey>,
}

#[derive(Debug)]
pub struct HandshakeResult {
    pub shared_secret: SharedSecret,
    pub initial_message: InitialMessage,
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

    let ephemeral_secret = StaticSecret::random_from_rng(rng);
    let ephemeral_public = X25519PublicKey::from(&ephemeral_secret);

    let dh1 = my_agreement
        .secret()
        .diffie_hellman(&their_bundle.signed_prekey.public);
    let dh2 = ephemeral_secret.diffie_hellman(&their_bundle.identity_agreement_key.public);
    let dh3 = ephemeral_secret.diffie_hellman(&their_bundle.signed_prekey.public);
    let dh4 = their_bundle
        .one_time_prekey
        .map(|opk| ephemeral_secret.diffie_hellman(&opk));

    let shared_secret = derive_shared_secret(&dh1, &dh2, &dh3, dh4.as_ref());

    Ok(HandshakeResult {
        shared_secret,
        initial_message: InitialMessage {
            initiator_identity: my_identity.public_key(),
            initiator_agreement_key: my_agreement.sign_with(my_identity),
            ephemeral_key: ephemeral_public,
            used_one_time_prekey: their_bundle.one_time_prekey,
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
    let dh4 = one_time_secret.map(|secret| secret.diffie_hellman(&message.ephemeral_key));

    Ok(derive_shared_secret(&dh1, &dh2, &dh3, dh4.as_ref()))
}

const KDF_INFO: &[u8] = b"SpiritChat-X3DH-v1";

fn derive_shared_secret(
    dh1: &x25519_dalek::SharedSecret,
    dh2: &x25519_dalek::SharedSecret,
    dh3: &x25519_dalek::SharedSecret,
    dh4: Option<&x25519_dalek::SharedSecret>,
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
}
