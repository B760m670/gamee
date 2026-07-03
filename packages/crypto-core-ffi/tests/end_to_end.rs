//! Exercises the FFI surface itself — not the core crate directly — so a
//! bug in the boundary layer (a wrong byte offset, a poisoned mutex, a
//! constructor that doesn't roundtrip) would show up here even though the
//! core crate's own test suite is green. This is exactly the object graph
//! a mobile app builds: generate identity → generate prekeys → exchange
//! contact cards → handshake → ratchet → persist → resume.

use spiritchat_crypto_core_ffi::{
    identity_verify, ledger_build_username_claim, x3dh_initiate, x3dh_respond, FfiAgreementKey,
    FfiContactCard, FfiIdentity, FfiPrekeyStore, FfiRatchet, FfiRecoveryPhrase,
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
    let bob_response =
        x3dh_respond(&bob.agreement, &bob.prekeys, handshake.initial_message.clone()).unwrap();
    assert_eq!(bob_response.shared_secret, handshake.shared_secret);
    assert_eq!(
        bob_response.initiator_identity_bytes,
        alice.identity.public_key_bytes()
    );

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
fn an_identity_recovered_from_its_recovery_phrase_has_the_same_fingerprint() {
    let phrase = FfiRecoveryPhrase::generate();
    assert_eq!(phrase.words().split_whitespace().count(), 12);

    let identity = FfiIdentity::from_secret_bytes(phrase.derive_identity_seed()).unwrap();

    // Simulates a fresh install: only the words survive, re-typed by the
    // user, nothing else about the original device carries over.
    let recovered_phrase = FfiRecoveryPhrase::from_words(phrase.words()).unwrap();
    let recovered_identity =
        FfiIdentity::from_secret_bytes(recovered_phrase.derive_identity_seed()).unwrap();

    assert_eq!(identity.fingerprint(), recovered_identity.fingerprint());
    assert_eq!(identity.public_key_bytes(), recovered_identity.public_key_bytes());
}

#[test]
fn a_typo_in_the_recovery_phrase_is_rejected_before_it_can_derive_a_wrong_identity() {
    let phrase = FfiRecoveryPhrase::generate();
    let words = phrase.words();
    let mut split: Vec<&str> = words.split_whitespace().collect();
    split[0] = "zzzznotarealbip39word";
    let mangled = split.join(" ");
    assert!(FfiRecoveryPhrase::from_words(mangled).is_err());
}

#[test]
fn a_username_claim_signed_by_one_identity_does_not_verify_against_another() {
    // This is the exact shape a @username DHT claim uses: sign the
    // username itself, so the claim can't be replayed under a different
    // name, and bind it to a specific public key nobody else can produce a
    // valid signature for.
    let alice = FfiIdentity::generate();
    let bob = FfiIdentity::generate();

    let username = b"alice".to_vec();
    let signature = alice.sign(username.clone());

    assert!(identity_verify(alice.public_key_bytes(), username.clone(), signature.clone()));
    assert!(!identity_verify(bob.public_key_bytes(), username.clone(), signature.clone()));

    // Signed for "alice" — must not verify as a claim for a different name.
    assert!(!identity_verify(alice.public_key_bytes(), b"bob".to_vec(), signature));
}

#[test]
fn ledger_build_username_claim_produces_a_transaction_that_deserializes_and_verifies() {
    let identity = FfiIdentity::generate();
    let anchor_hash = vec![7u8; 32];
    let nonce = vec![1u8; 8];

    let claim_bytes =
        ledger_build_username_claim(&identity, "alice".to_string(), 42, anchor_hash.clone(), nonce)
            .expect("a valid username should build a valid claim");

    let transaction: spiritchat_ledger_core::Transaction =
        bincode::deserialize(&claim_bytes).expect("must round-trip through bincode");

    assert_eq!(transaction.username, "alice");
    assert_eq!(transaction.owner_public_key.to_vec(), identity.public_key_bytes());
    assert_eq!(transaction.claimed_at_height_hint, 42);
    assert_eq!(transaction.anchor_block_hash.as_bytes().to_vec(), anchor_hash);
    transaction.verify_self_contained().expect("a freshly built claim must verify");
}

#[test]
fn ledger_build_username_claim_rejects_an_invalid_username() {
    let identity = FfiIdentity::generate();
    let err = ledger_build_username_claim(&identity, "no".to_string(), 0, vec![0u8; 32], vec![0u8; 8])
        .expect_err("a too-short username must be rejected before signing anything");
    assert!(err.to_string().contains("shorter than"), "unexpected error: {err}");
}

#[test]
fn ledger_build_username_claim_rejects_a_malformed_anchor_hash() {
    let identity = FfiIdentity::generate();
    let err = ledger_build_username_claim(&identity, "alice".to_string(), 0, vec![0u8; 31], vec![0u8; 8])
        .expect_err("a 31-byte anchor hash is not 32 bytes and must be rejected");
    assert!(err.to_string().contains("32 bytes"), "unexpected error: {err}");
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
