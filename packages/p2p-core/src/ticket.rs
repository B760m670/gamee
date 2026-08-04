//! Contact tickets — proof that one identity deliberately approached
//! another, without proving anything about what was said.
//!
//! Phase 2 of `docs/consent-and-moderation.md`. The design rests on
//! repudiability being *divisible*: what must stay deniable is the content of
//! a message, and this module never touches content. What a ticket attests is
//! only that key `A` spent work and addressed key `B` during a given epoch.
//!
//! Two things follow from that, and they are the whole reason the module
//! exists:
//!
//! - **Unsolicited contact acquires a price.** A first approach to a stranger
//!   costs proof of work, which is nothing for one message and ruinous for a
//!   million. Same Hashcash reasoning as [`crate::mailbox`]'s deposit stamps,
//!   and deliberately the same difficulty vocabulary.
//! - **Accusations become unforgeable.** To claim `A` approached you, you must
//!   exhibit a ticket `A` signed naming you — and you cannot make one without
//!   `A`'s key. A farm of fabricated identities therefore cannot produce a
//!   single complaint against someone who never contacted them. The right to
//!   complain is issued by the party complained about, through their own act.
//!
//! What a ticket is *not*: evidence of abuse. Nothing here says the approach
//! was unwelcome, and this module makes no such claim. It establishes only
//! that the approach happened, and leaves judgement to the recipient.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::error::{P2pError, Result};

/// Separates the value being signed from every other signature this project
/// produces, so a ticket can never be replayed as a prekey signature, a
/// username claim, or anything else.
const TICKET_DOMAIN: &[u8] = b"SpiritChat-ContactTicket-v1";
/// A second, distinct domain for the proof-of-work hash. The signed value and
/// the work value must never be the same bytes, or grinding the work would
/// be grinding the signature's input.
const TICKET_WORK_DOMAIN: &[u8] = b"SpiritChat-ContactTicketWork-v1";

/// How many leading zero bits a ticket's work hash must have by default.
/// Matches [`crate::mailbox::MAILBOX_POW_LEADING_ZERO_BITS`] on purpose:
/// both are "cheap once, expensive in bulk" gates on unsolicited traffic, and
/// having two different numbers for the same idea would be a needless second
/// thing to reason about.
///
/// A recipient may demand more (phase 3 raises it for senders their own graph
/// distrusts), so verification takes the requirement as a parameter rather
/// than reading this constant directly.
pub const TICKET_BASE_LEADING_ZERO_BITS: u32 = 20;

/// How far a ticket's epoch may sit from the verifier's own before it is
/// refused. One epoch either side absorbs clock skew and the case of a
/// message mined just before a rollover and delivered just after, without
/// letting a ticket be stockpiled.
pub const MAX_EPOCH_DRIFT: u64 = 1;

/// The canonical value a ticket signs: one per (sender, recipient, epoch).
///
/// The nonce is deliberately **outside** this. If grinding could vary the
/// signed value, a sender could produce many distinct tickets for one
/// recipient, and a recipient could then present them as complaints from
/// several different people. Fixing the signed value at exactly one per pair
/// per epoch makes that impossible by construction.
pub fn ticket_base(sender_public_key: &[u8; 32], recipient_public_key: &[u8; 32], epoch: u64) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(TICKET_DOMAIN);
    hasher.update(sender_public_key);
    hasher.update(recipient_public_key);
    hasher.update(epoch.to_be_bytes());
    hasher.finalize().into()
}

fn work_hash(base: &[u8; 32], nonce: [u8; 8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(TICKET_WORK_DOMAIN);
    hasher.update(base);
    hasher.update(nonce);
    hasher.finalize().into()
}

fn leading_zero_bits(hash: &[u8; 32]) -> u32 {
    let mut count = 0;
    for byte in hash {
        if *byte == 0 {
            count += 8;
            continue;
        }
        count += byte.leading_zeros();
        break;
    }
    count
}

/// A completed contact ticket, as carried alongside a first message.
///
/// Every field is public. The recipient's key is *not* among them: a verifier
/// who does not already know it cannot recover it from `base`, which is what
/// lets a receipt be shown to a third party without disclosing who was
/// approached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContactTicket {
    pub base: [u8; 32],
    pub nonce: [u8; 8],
    pub epoch: u64,
    pub sender_public_key: [u8; 32],
    pub signature: [u8; 64],
}

/// Serialized size — fixed, so framing needs no length prefix.
pub const TICKET_LEN: usize = 32 + 8 + 8 + 32 + 64;

impl ContactTicket {
    pub fn to_bytes(&self) -> [u8; TICKET_LEN] {
        let mut out = [0u8; TICKET_LEN];
        out[0..32].copy_from_slice(&self.base);
        out[32..40].copy_from_slice(&self.nonce);
        out[40..48].copy_from_slice(&self.epoch.to_be_bytes());
        out[48..80].copy_from_slice(&self.sender_public_key);
        out[80..144].copy_from_slice(&self.signature);
        out
    }

    pub fn from_bytes(input: &[u8]) -> Result<Self> {
        if input.len() != TICKET_LEN {
            return Err(P2pError::Ticket("contact ticket has the wrong length".into()));
        }
        let mut base = [0u8; 32];
        base.copy_from_slice(&input[0..32]);
        let mut nonce = [0u8; 8];
        nonce.copy_from_slice(&input[32..40]);
        let epoch = u64::from_be_bytes(input[40..48].try_into().unwrap());
        let mut sender_public_key = [0u8; 32];
        sender_public_key.copy_from_slice(&input[48..80]);
        let mut signature = [0u8; 64];
        signature.copy_from_slice(&input[80..144]);
        Ok(Self { base, nonce, epoch, sender_public_key, signature })
    }

    /// Checks the signature and the work, but **not** who the ticket is for —
    /// that is [`Self::verify_for`]'s job. Split deliberately: a third party
    /// judging a receipt can do exactly this much and no more, since it does
    /// not know, and must not learn, the recipient.
    pub fn verify_standalone(&self, required_bits: u32, now_epoch: u64) -> Result<()> {
        if self.epoch.abs_diff(now_epoch) > MAX_EPOCH_DRIFT {
            return Err(P2pError::Ticket("contact ticket epoch is out of range".into()));
        }
        if leading_zero_bits(&work_hash(&self.base, self.nonce)) < required_bits {
            return Err(P2pError::Ticket("contact ticket does not meet the required work".into()));
        }
        let key = VerifyingKey::from_bytes(&self.sender_public_key)
            .map_err(|err| P2pError::Ticket(format!("malformed sender key: {err}")))?;
        let signature = Signature::from_bytes(&self.signature);
        let mut signed = Vec::with_capacity(TICKET_DOMAIN.len() + 32);
        signed.extend_from_slice(TICKET_DOMAIN);
        signed.extend_from_slice(&self.base);
        key.verify(&signed, &signature)
            .map_err(|_| P2pError::Ticket("contact ticket signature is invalid".into()))?;
        Ok(())
    }

    /// Full check for the party being approached: everything
    /// [`Self::verify_standalone`] does, plus that `base` really is the
    /// canonical value for this sender addressing *this* recipient in this
    /// epoch. Without this a ticket minted for someone else would pass.
    pub fn verify_for(
        &self,
        recipient_public_key: &[u8; 32],
        required_bits: u32,
        now_epoch: u64,
    ) -> Result<()> {
        self.verify_standalone(required_bits, now_epoch)?;
        let expected = ticket_base(&self.sender_public_key, recipient_public_key, self.epoch);
        if expected != self.base {
            return Err(P2pError::Ticket("contact ticket is addressed to someone else".into()));
        }
        Ok(())
    }
}

/// Grinds a ticket for `recipient_public_key`. Pure CPU work: like
/// [`crate::mailbox::mine_stamp`], this belongs on a blocking thread, never on
/// the async event loop. At the base difficulty it finishes in well under a
/// second, and only ever runs for a *first* approach to someone new.
pub fn mine_ticket(
    signing_key: &SigningKey,
    recipient_public_key: &[u8; 32],
    epoch: u64,
    required_bits: u32,
) -> ContactTicket {
    let sender_public_key = signing_key.verifying_key().to_bytes();
    let base = ticket_base(&sender_public_key, recipient_public_key, epoch);

    let mut nonce: u64 = 0;
    let found = loop {
        let candidate = nonce.to_le_bytes();
        if leading_zero_bits(&work_hash(&base, candidate)) >= required_bits {
            break candidate;
        }
        nonce = nonce.wrapping_add(1);
    };

    let mut signed = Vec::with_capacity(TICKET_DOMAIN.len() + 32);
    signed.extend_from_slice(TICKET_DOMAIN);
    signed.extend_from_slice(&base);
    let signature: Signature = signing_key.sign(&signed);

    ContactTicket {
        base,
        nonce: found,
        epoch,
        sender_public_key,
        signature: signature.to_bytes(),
    }
}

/// What a recipient knows about someone approaching them, as far as it is
/// knowable without reading anything. Every field is derived from the
/// recipient's *own* view — their contacts, their consent decisions, receipts
/// that reached them through their own graph — never from a global verdict.
#[derive(Clone, Copy, Debug, Default)]
pub struct ContactSignals {
    /// An existing correspondent: already a contact, or a conversation this
    /// device has consented to. The common case, and the one that must cost
    /// nothing at all.
    pub established: bool,
    /// This device blocked them. Its own decision, and the only one that
    /// closes the door.
    pub blocked_by_me: bool,
    /// How many of *this device's* contacts have blocked them, as learned
    /// through receipts. Zero when nothing is known, which is also the state
    /// of a network where nobody publishes receipts at all.
    pub contacts_who_blocked: u32,
    /// Shares at least one contact with this device — weak evidence of being
    /// a real participant rather than a fresh throwaway.
    pub shares_contacts: bool,
    /// Holds a `@username`, which costs proof of work to claim. Also weak,
    /// and for the same reason: it is a cost signal, not an endorsement.
    pub holds_username: bool,
}

/// What a sender must pay to be heard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactRequirement {
    /// No ticket at all — an established conversation.
    None,
    /// A ticket meeting this many leading zero bits.
    Work(u32),
    /// Nothing will be accepted.
    Refused,
}

/// Cheapest a stranger can ever get. Never free: "cheap" and "free" differ by
/// exactly the property that makes bulk contact expensive.
pub const TICKET_MIN_LEADING_ZERO_BITS: u32 = 18;
/// Dearest the ladder climbs. Around a minute of one core — heavy enough to
/// make mass approach pointless, bounded so it can never become a life
/// sentence handed down by other people.
pub const TICKET_MAX_LEADING_ZERO_BITS: u32 = 28;

/// The sanction ladder, as arithmetic.
///
/// One property here is deliberate and worth naming, because the obvious
/// implementation gets it wrong: **other people's blocks raise the price,
/// they never impose silence.** However many of your contacts have blocked
/// someone, the result is a harder ticket, never `Refused`. Only
/// `blocked_by_me` closes the door. Otherwise a graph could be assembled into
/// a mob veto, and a moderation mechanism that can be aimed is a weapon
/// rather than a defence — the same reasoning that keeps the whole design
/// subjective (see `docs/consent-and-moderation.md` §1).
pub fn required_work(signals: &ContactSignals) -> ContactRequirement {
    if signals.blocked_by_me {
        return ContactRequirement::Refused;
    }
    if signals.established {
        return ContactRequirement::None;
    }

    let mut bits = TICKET_BASE_LEADING_ZERO_BITS as i64;

    // Costly identity and a shared social edge each shave a little. Small on
    // purpose: they are evidence of effort, not of good intent, and treating
    // them as more would make a `@username` a licence to spam.
    if signals.holds_username {
        bits -= 1;
    }
    if signals.shares_contacts {
        bits -= 1;
    }

    // Each block from someone this device actually trusts roughly quadruples
    // the work. Saturating, since the cap does the bounding.
    bits += 4 * i64::from(signals.contacts_who_blocked.min(8));

    ContactRequirement::Work(
        bits.clamp(TICKET_MIN_LEADING_ZERO_BITS as i64, TICKET_MAX_LEADING_ZERO_BITS as i64) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    /// Low difficulty everywhere in tests: these assert structure, and paying
    /// for real work in every case would make the suite slow for no gain.
    const TEST_BITS: u32 = 8;

    fn key(seed: u8) -> SigningKey {
        SigningKey::generate(&mut ChaCha20Rng::seed_from_u64(seed as u64))
    }

    #[test]
    fn a_mined_ticket_verifies_for_its_recipient() {
        let alice = key(1);
        let bob = key(2).verifying_key().to_bytes();
        let ticket = mine_ticket(&alice, &bob, 100, TEST_BITS);
        ticket.verify_for(&bob, TEST_BITS, 100).unwrap();
    }

    #[test]
    fn a_ticket_is_worthless_against_a_different_recipient() {
        // The property that stops one ticket being reused to approach the
        // whole network: work paid for Bob buys nothing with Carol.
        let alice = key(1);
        let bob = key(2).verifying_key().to_bytes();
        let carol = key(3).verifying_key().to_bytes();
        let ticket = mine_ticket(&alice, &bob, 100, TEST_BITS);
        assert!(ticket.verify_for(&carol, TEST_BITS, 100).is_err());
    }

    #[test]
    fn nobody_can_forge_a_ticket_from_a_key_they_do_not_hold() {
        // The property the whole accusation model rests on: an attacker who
        // wants to make it look as though Alice approached them cannot,
        // because the signature is Alice's to make.
        let mallory = key(9);
        let victim_key = key(1).verifying_key().to_bytes();
        let target = key(2).verifying_key().to_bytes();

        let mut forged = mine_ticket(&mallory, &target, 100, TEST_BITS);
        forged.sender_public_key = victim_key; // claim it came from Alice
        assert!(forged.verify_for(&target, TEST_BITS, 100).is_err());
    }

    #[test]
    fn there_is_exactly_one_signed_value_per_pair_and_epoch() {
        // Why the nonce sits outside the signed value: if grinding could
        // change it, a recipient could present several tickets from one
        // approach as complaints from several different people.
        let alice = key(1);
        let bob = key(2).verifying_key().to_bytes();
        let first = mine_ticket(&alice, &bob, 100, TEST_BITS);
        let second = mine_ticket(&alice, &bob, 100, TEST_BITS);
        assert_eq!(first.base, second.base);
    }

    #[test]
    fn a_different_epoch_produces_a_different_ticket() {
        let alice = key(1);
        let bob = key(2).verifying_key().to_bytes();
        assert_ne!(
            mine_ticket(&alice, &bob, 100, TEST_BITS).base,
            mine_ticket(&alice, &bob, 101, TEST_BITS).base,
        );
    }

    #[test]
    fn a_stale_ticket_is_refused() {
        let alice = key(1);
        let bob = key(2).verifying_key().to_bytes();
        let ticket = mine_ticket(&alice, &bob, 100, TEST_BITS);
        assert!(ticket.verify_for(&bob, TEST_BITS, 100 + MAX_EPOCH_DRIFT + 1).is_err());
        // Within the drift window it is still fine.
        ticket.verify_for(&bob, TEST_BITS, 100 + MAX_EPOCH_DRIFT).unwrap();
    }

    #[test]
    fn insufficient_work_is_refused() {
        let alice = key(1);
        let bob = key(2).verifying_key().to_bytes();
        let ticket = mine_ticket(&alice, &bob, 100, 4);
        // A recipient demanding far more than was paid must reject it —
        // this is what phase 3's per-recipient difficulty rests on.
        assert!(ticket.verify_for(&bob, 24, 100).is_err());
    }

    #[test]
    fn a_tampered_nonce_invalidates_the_work() {
        let alice = key(1);
        let bob = key(2).verifying_key().to_bytes();
        let mut ticket = mine_ticket(&alice, &bob, 100, TEST_BITS);
        ticket.nonce = [0xff; 8];
        assert!(ticket.verify_for(&bob, TEST_BITS, 100).is_err());
    }

    #[test]
    fn a_third_party_can_check_a_ticket_without_knowing_the_recipient() {
        // What makes a receipt shareable: the standalone check is enough to
        // establish that the sender really did approach *someone*, and it
        // never touches the recipient's key.
        let alice = key(1);
        let bob = key(2).verifying_key().to_bytes();
        let ticket = mine_ticket(&alice, &bob, 100, TEST_BITS);
        ticket.verify_standalone(TEST_BITS, 100).unwrap();
    }

    #[test]
    fn round_trips_through_bytes() {
        let alice = key(1);
        let bob = key(2).verifying_key().to_bytes();
        let ticket = mine_ticket(&alice, &bob, 100, TEST_BITS);
        let restored = ContactTicket::from_bytes(&ticket.to_bytes()).unwrap();
        assert_eq!(restored, ticket);
        restored.verify_for(&bob, TEST_BITS, 100).unwrap();
    }

    #[test]
    fn an_established_correspondent_pays_nothing() {
        let signals = ContactSignals { established: true, ..Default::default() };
        assert_eq!(required_work(&signals), ContactRequirement::None);
    }

    #[test]
    fn only_my_own_block_closes_the_door() {
        // The property that keeps this a defence rather than a weapon: no
        // number of other people's blocks can silence someone for me.
        let mobbed = ContactSignals { contacts_who_blocked: 1000, ..Default::default() };
        assert!(matches!(required_work(&mobbed), ContactRequirement::Work(_)));

        let mine = ContactSignals { blocked_by_me: true, ..Default::default() };
        assert_eq!(required_work(&mine), ContactRequirement::Refused);
    }

    #[test]
    fn a_stranger_pays_the_base_rate() {
        let stranger = ContactSignals::default();
        assert_eq!(
            required_work(&stranger),
            ContactRequirement::Work(TICKET_BASE_LEADING_ZERO_BITS),
        );
    }

    #[test]
    fn blocks_from_my_contacts_raise_the_price_monotonically() {
        let price = |n| match required_work(&ContactSignals {
            contacts_who_blocked: n,
            ..Default::default()
        }) {
            ContactRequirement::Work(bits) => bits,
            other => panic!("expected work, got {other:?}"),
        };
        assert!(price(0) < price(1));
        assert!(price(1) < price(2));
    }

    #[test]
    fn the_price_is_bounded_at_both_ends() {
        // Never free, however trusted the approach looks...
        let trusted = ContactSignals {
            shares_contacts: true,
            holds_username: true,
            ..Default::default()
        };
        match required_work(&trusted) {
            ContactRequirement::Work(bits) => assert!(bits >= TICKET_MIN_LEADING_ZERO_BITS),
            other => panic!("expected work, got {other:?}"),
        }
        // ...and never unpayable, however many people dislike them.
        match required_work(&ContactSignals { contacts_who_blocked: u32::MAX, ..Default::default() }) {
            ContactRequirement::Work(bits) => assert_eq!(bits, TICKET_MAX_LEADING_ZERO_BITS),
            other => panic!("expected work, got {other:?}"),
        }
    }

    #[test]
    fn a_costly_identity_helps_a_little_but_is_not_a_licence() {
        // A @username makes an approach cheaper, never free and never enough
        // to offset real negative signal — otherwise buying a name would buy
        // the right to spam.
        let named = ContactSignals { holds_username: true, ..Default::default() };
        let named_but_blocked = ContactSignals {
            holds_username: true,
            contacts_who_blocked: 2,
            ..Default::default()
        };
        let plain = ContactSignals::default();
        let bits = |s| match required_work(&s) {
            ContactRequirement::Work(b) => b,
            other => panic!("expected work, got {other:?}"),
        };
        assert!(bits(named) < bits(plain));
        assert!(bits(named_but_blocked) > bits(plain));
    }

    #[test]
    fn wrong_length_is_refused() {
        assert!(ContactTicket::from_bytes(&[0u8; 10]).is_err());
        assert!(ContactTicket::from_bytes(&[0u8; TICKET_LEN - 1]).is_err());
    }
}
