//! End-to-end proof that the public API composes correctly: two parties go
//! from nothing but a scanned QR bundle to a working encrypted
//! conversation, using only what a real caller (the Android/iOS app) would
//! have access to.

use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha20Rng;

use spiritchat_crypto_core::handshake;
use spiritchat_crypto_core::identity::{AgreementKeyPair, IdentityKeyPair};
use spiritchat_crypto_core::prekey::{PrekeyBundle, PrekeyStore};
use spiritchat_crypto_core::ratchet::DoubleRatchet;

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

    /// What would actually go inside a QR contact card.
    fn qr_bundle(&mut self) -> PrekeyBundle {
        let signed_agreement = self.agreement.sign_with(&self.identity);
        self.prekeys
            .public_bundle(self.identity.public_key(), signed_agreement)
    }
}

/// Sets up a fully established pair of ratchets: Alice scans Bob's QR
/// bundle, runs X3DH, and both sides bootstrap their Double Ratchet state
/// from the result — exactly the flow a real first contact goes through.
fn establish_session(alice: &mut Party, bob: &mut Party) -> (DoubleRatchet, DoubleRatchet) {
    let mut rng = ChaCha20Rng::seed_from_u64(1000);

    let bob_bundle = bob.qr_bundle();
    let bob_spk_public = bob_bundle.signed_prekey.public;

    let handshake::HandshakeResult {
        shared_secret: alice_secret,
        initial_message,
    } = handshake::initiate(&mut rng, &alice.identity, &alice.agreement, &bob_bundle).unwrap();

    let bob_secret = handshake::respond(&bob.agreement, &mut bob.prekeys, &initial_message).unwrap();
    assert_eq!(alice_secret.as_bytes(), bob_secret.as_bytes());

    let alice_ratchet = DoubleRatchet::init_initiator(&mut rng, &alice_secret, bob_spk_public);
    let bob_ratchet =
        DoubleRatchet::init_responder(&bob_secret, bob.prekeys.clone_signed_prekey_secret());

    (alice_ratchet, bob_ratchet)
}

#[test]
fn alice_sends_the_first_message_and_bob_reads_it() {
    let mut alice = Party::new(1, 1);
    let mut bob = Party::new(2, 1);
    let (mut alice_ratchet, mut bob_ratchet) = establish_session(&mut alice, &mut bob);

    let (header, ciphertext) = alice_ratchet.encrypt(b"hey bob", b"").unwrap();
    let plaintext = bob_ratchet.decrypt(&header, &ciphertext, b"").unwrap();

    assert_eq!(plaintext, b"hey bob");
}

#[test]
fn a_full_back_and_forth_conversation_round_trips() {
    let mut alice = Party::new(1, 1);
    let mut bob = Party::new(2, 1);
    let (mut alice_ratchet, mut bob_ratchet) = establish_session(&mut alice, &mut bob);

    let (h1, c1) = alice_ratchet.encrypt(b"hi", b"").unwrap();
    assert_eq!(bob_ratchet.decrypt(&h1, &c1, b"").unwrap(), b"hi");

    let (h2, c2) = bob_ratchet.encrypt(b"hi yourself", b"").unwrap();
    assert_eq!(alice_ratchet.decrypt(&h2, &c2, b"").unwrap(), b"hi yourself");

    let (h3, c3) = alice_ratchet.encrypt(b"how are you", b"").unwrap();
    assert_eq!(bob_ratchet.decrypt(&h3, &c3, b"").unwrap(), b"how are you");

    let (h4, c4) = bob_ratchet.encrypt(b"good, you?", b"").unwrap();
    assert_eq!(alice_ratchet.decrypt(&h4, &c4, b"").unwrap(), b"good, you?");
}

#[test]
fn messages_within_one_chain_survive_reordering() {
    let mut alice = Party::new(1, 1);
    let mut bob = Party::new(2, 1);
    let (mut alice_ratchet, mut bob_ratchet) = establish_session(&mut alice, &mut bob);

    let msg1 = alice_ratchet.encrypt(b"one", b"").unwrap();
    let msg2 = alice_ratchet.encrypt(b"two", b"").unwrap();
    let msg3 = alice_ratchet.encrypt(b"three", b"").unwrap();

    // Network reordered delivery: 3, 1, 2.
    assert_eq!(
        bob_ratchet.decrypt(&msg3.0, &msg3.1, b"").unwrap(),
        b"three"
    );
    assert_eq!(bob_ratchet.decrypt(&msg1.0, &msg1.1, b"").unwrap(), b"one");
    assert_eq!(bob_ratchet.decrypt(&msg2.0, &msg2.1, b"").unwrap(), b"two");
}

#[test]
fn a_lost_message_does_not_block_later_ones_and_cannot_be_replayed() {
    let mut alice = Party::new(1, 1);
    let mut bob = Party::new(2, 1);
    let (mut alice_ratchet, mut bob_ratchet) = establish_session(&mut alice, &mut bob);

    let lost = alice_ratchet.encrypt(b"never arrives", b"").unwrap();
    let delivered = alice_ratchet.encrypt(b"arrives fine", b"").unwrap();

    // "lost" is simply never handed to bob_ratchet.decrypt.
    assert_eq!(
        bob_ratchet.decrypt(&delivered.0, &delivered.1, b"").unwrap(),
        b"arrives fine"
    );

    // Even if it arrives very late, its key still works exactly once...
    assert_eq!(
        bob_ratchet.decrypt(&lost.0, &lost.1, b"").unwrap(),
        b"never arrives"
    );
    // ...but a duplicate delivery (replay) of the same message must fail,
    // since its message key was already consumed.
    assert!(bob_ratchet.decrypt(&lost.0, &lost.1, b"").is_err());
}

#[test]
fn conversation_survives_many_direction_changes_with_reordering_each_way() {
    let mut alice = Party::new(1, 1);
    let mut bob = Party::new(2, 1);
    let (mut alice_ratchet, mut bob_ratchet) = establish_session(&mut alice, &mut bob);

    for round in 0..10 {
        let a_text = format!("alice says {round}");
        let b_text = format!("bob says {round}");

        let a_msg1 = alice_ratchet.encrypt(a_text.as_bytes(), b"").unwrap();
        let a_msg2 = alice_ratchet.encrypt(a_text.as_bytes(), b"").unwrap();

        // Bob receives Alice's two messages out of order.
        assert_eq!(
            bob_ratchet.decrypt(&a_msg2.0, &a_msg2.1, b"").unwrap(),
            a_text.as_bytes()
        );
        assert_eq!(
            bob_ratchet.decrypt(&a_msg1.0, &a_msg1.1, b"").unwrap(),
            a_text.as_bytes()
        );

        let b_msg = bob_ratchet.encrypt(b_text.as_bytes(), b"").unwrap();
        assert_eq!(
            alice_ratchet.decrypt(&b_msg.0, &b_msg.1, b"").unwrap(),
            b_text.as_bytes()
        );
    }
}

#[test]
fn tampered_ciphertext_is_rejected_even_with_a_valid_header() {
    let mut alice = Party::new(1, 1);
    let mut bob = Party::new(2, 1);
    let (mut alice_ratchet, mut bob_ratchet) = establish_session(&mut alice, &mut bob);

    let (header, mut ciphertext) = alice_ratchet.encrypt(b"trust me", b"").unwrap();
    *ciphertext.last_mut().unwrap() ^= 0xff;

    assert!(bob_ratchet.decrypt(&header, &ciphertext, b"").is_err());
}

#[test]
fn associated_data_must_match_on_both_sides() {
    let mut alice = Party::new(1, 1);
    let mut bob = Party::new(2, 1);
    let (mut alice_ratchet, mut bob_ratchet) = establish_session(&mut alice, &mut bob);

    let (header, ciphertext) = alice_ratchet
        .encrypt(b"bound to a conversation id", b"conversation-42")
        .unwrap();

    assert!(bob_ratchet
        .decrypt(&header, &ciphertext, b"conversation-99")
        .is_err());
    assert_eq!(
        bob_ratchet
            .decrypt(&header, &ciphertext, b"conversation-42")
            .unwrap(),
        b"bound to a conversation id"
    );
}

#[test]
fn works_end_to_end_without_a_one_time_prekey_too() {
    let mut alice = Party::new(1, 0);
    let mut bob = Party::new(2, 0);
    let (mut alice_ratchet, mut bob_ratchet) = establish_session(&mut alice, &mut bob);

    let (header, ciphertext) = alice_ratchet.encrypt(b"no OPK needed", b"").unwrap();
    assert_eq!(
        bob_ratchet.decrypt(&header, &ciphertext, b"").unwrap(),
        b"no OPK needed"
    );
}
