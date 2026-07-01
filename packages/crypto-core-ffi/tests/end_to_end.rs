//! Exercises the FFI surface itself — not the core crate directly — so a
//! bug in the boundary layer (a wrong byte offset, a poisoned mutex, a
//! constructor that doesn't roundtrip) would show up here even though the
//! core crate's own test suite is green. This is exactly the object graph
//! a mobile app builds: generate identity → generate prekeys → exchange
//! contact cards → handshake → ratchet → persist → resume.

use spiritchat_crypto_core_ffi::{
    x3dh_initiate, x3dh_respond, FfiAgreementKey, FfiContactCard, FfiIdentity, FfiPrekeyStore,
    FfiRatchet,
};

struct Party {
    identity: std::sync::Arc<FfiIdentity>,
    agreement: std::sync::Arc<FfiAgreementKey>,
    prekeys: std::sync::Arc<FfiPrekeyStore>,
}

impl Party {
    fn new(one_time_count: u32) -> Self {
        let identity = FfiIdentity::generate();
        let agreement = FfiAgreementKey::generate();
        let prekeys = FfiPrekeyStore::generate(&identity, one_time_count);
        Self {
            identity,
            agreement,
            prekeys,
        }
    }

    fn contact_card_bytes(&self) -> Vec<u8> {
        self.prekeys.contact_card(&self.identity, &self.agreement)
    }
}

/// Runs the handshake and returns both sides' ratchets, exactly the flow a
/// real first contact goes through.
fn establish_session(
    alice: &Party,
    bob: &Party,
) -> (std::sync::Arc<FfiRatchet>, std::sync::Arc<FfiRatchet>) {
    let bob_card = FfiContactCard::parse(bob.contact_card_bytes()).unwrap();

    let handshake = x3dh_initiate(&alice.identity, &alice.agreement, &bob_card).unwrap();
    let bob_shared_secret =
        x3dh_respond(&bob.agreement, &bob.prekeys, handshake.initial_message.clone()).unwrap();
    assert_eq!(bob_shared_secret, handshake.shared_secret);

    let alice_ratchet = FfiRatchet::init_initiator(
        handshake.shared_secret.clone(),
        bob_card.signed_prekey_public_bytes(),
    )
    .unwrap();

    let bob_ratchet = FfiRatchet::init_responder(
        handshake.shared_secret,
        bob.prekeys.signed_prekey_secret_bytes(),
    )
    .unwrap();

    (alice_ratchet, bob_ratchet)
}

#[test]
fn a_full_conversation_through_the_ffi_object_graph() {
    let alice = Party::new(1);
    let bob = Party::new(1);

    let bob_card = FfiContactCard::parse(bob.contact_card_bytes()).unwrap();
    assert!(bob_card.has_one_time_prekey());
    assert_eq!(
        bob_card.identity_public_key_bytes(),
        bob.identity.public_key_bytes()
    );

    let (alice_ratchet, bob_ratchet) = establish_session(&alice, &bob);

    let message = alice_ratchet.encrypt(b"hey bob".to_vec(), vec![]).unwrap();
    let plaintext = bob_ratchet.decrypt(message, vec![]).unwrap();
    assert_eq!(plaintext, b"hey bob");

    let reply = bob_ratchet
        .encrypt(b"hey alice".to_vec(), vec![])
        .unwrap();
    let plaintext = alice_ratchet.decrypt(reply, vec![]).unwrap();
    assert_eq!(plaintext, b"hey alice");
}

#[test]
fn a_ratchet_survives_persisting_and_restoring_across_a_simulated_app_restart() {
    let alice = Party::new(1);
    let bob = Party::new(1);
    let (alice_ratchet, bob_ratchet) = establish_session(&alice, &bob);

    let message = alice_ratchet
        .encrypt(b"before restart".to_vec(), vec![])
        .unwrap();
    assert_eq!(
        bob_ratchet.decrypt(message, vec![]).unwrap(),
        b"before restart"
    );

    let alice_ratchet = FfiRatchet::from_bytes(alice_ratchet.to_bytes()).unwrap();
    let bob_ratchet = FfiRatchet::from_bytes(bob_ratchet.to_bytes()).unwrap();

    let message = bob_ratchet
        .encrypt(b"after restart".to_vec(), vec![])
        .unwrap();
    assert_eq!(
        alice_ratchet.decrypt(message, vec![]).unwrap(),
        b"after restart"
    );
}

#[test]
fn an_identity_and_agreement_key_survive_a_round_trip_through_their_secret_bytes() {
    let identity = FfiIdentity::generate();
    let restored = FfiIdentity::from_secret_bytes(identity.secret_bytes()).unwrap();
    assert_eq!(identity.public_key_bytes(), restored.public_key_bytes());
    assert_eq!(identity.fingerprint(), restored.fingerprint());

    let agreement = FfiAgreementKey::generate();
    let restored = FfiAgreementKey::from_secret_bytes(agreement.secret_bytes()).unwrap();
    assert_eq!(agreement.public_bytes(), restored.public_bytes());
}

#[test]
fn a_prekey_store_survives_a_round_trip_through_bytes_including_pending_prekeys() {
    let party = Party::new(2);
    let card_bytes = party.contact_card_bytes();
    let card = FfiContactCard::parse(card_bytes).unwrap();
    assert!(card.has_one_time_prekey());
    assert_eq!(party.prekeys.one_time_prekey_count(), 1);

    let restored = FfiPrekeyStore::from_bytes(party.prekeys.to_bytes()).unwrap();
    assert_eq!(restored.one_time_prekey_count(), 1);
    assert_eq!(
        restored.signed_prekey_secret_bytes(),
        party.prekeys.signed_prekey_secret_bytes()
    );
}

#[test]
fn parsing_a_tampered_contact_card_is_rejected() {
    let party = Party::new(0);
    let mut bytes = party.contact_card_bytes();
    *bytes.last_mut().unwrap() ^= 0xff;
    assert!(FfiContactCard::parse(bytes).is_err());
}

#[test]
fn a_replayed_initial_message_is_rejected_by_the_responder() {
    let alice = Party::new(1);
    let bob = Party::new(1);
    let bob_card = FfiContactCard::parse(bob.contact_card_bytes()).unwrap();

    let handshake = x3dh_initiate(&alice.identity, &alice.agreement, &bob_card).unwrap();
    x3dh_respond(&bob.agreement, &bob.prekeys, handshake.initial_message.clone()).unwrap();

    let replay = x3dh_respond(&bob.agreement, &bob.prekeys, handshake.initial_message);
    assert!(replay.is_err());
}
