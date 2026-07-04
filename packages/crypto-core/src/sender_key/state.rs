//! Sender Keys: the group-messaging scheme Signal and WhatsApp both use
//! in production — not a novel design, and deliberately not a full
//! MLS/TreeKEM implementation, which this project isn't taking on. Each
//! member encrypts once, under their own single ratcheting chain, for
//! every other member to decrypt (distributed via
//! [`super::distribution::SenderKeyDistribution`] over the *existing*
//! pairwise [`crate::ratchet::DoubleRatchet`] sessions this crate already
//! has) — cheaper than re-encrypting per-recipient the way 1:1 messaging
//! does, at a real, disclosed cost: everyone in the group holds the same
//! chain key, so **AEAD confidentiality alone can't say which member sent
//! a given message** (anyone could forge a validly-encrypted ciphertext
//! under a key they also hold). That's what the Ed25519 signature over
//! every message buys back: authorship within the group, verified against
//! the specific sending member's own signing key, never distributed to
//! anyone but that member. Membership changes (someone leaves) require
//! whoever's responsible for the group to have remaining members generate
//! a fresh [`SenderKeyState`] and redistribute it — this module only
//! provides the chain/signing primitives; deciding *when* to rotate is an
//! application-layer policy, the same way this crate never decides when a
//! 1:1 conversation should re-key either.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::{CryptoRngCore, OsRng};
use zeroize::ZeroizeOnDrop;

use crate::encoding::{varint_decode, varint_encode};
use crate::error::{CryptoError, Result};
use crate::ratchet::aead;
use crate::ratchet::chain::{ChainKey, MessageKey};

use super::distribution::SenderKeyDistribution;
use super::header::SenderKeyHeader;
use super::skipped::SkippedIterations;

const SIGNATURE_LEN: usize = 64;

/// One group member's own outgoing Sender Key state — what encrypts the
/// messages *this* identity sends into a group.
#[derive(Clone, ZeroizeOnDrop)]
pub struct SenderKeyState {
    chain_key: ChainKey,
    iteration: u32,
    #[zeroize(skip)] // SigningKey zeroizes its own inner secret on drop.
    signing_key: SigningKey,
}

impl SenderKeyState {
    /// Starts a brand new chain with a fresh random chain key and a fresh
    /// signing keypair — call once per group this identity creates or
    /// joins, and again any time the group's membership changes and this
    /// member's chain needs to be rotated for forward secrecy.
    pub fn generate(rng: &mut impl CryptoRngCore) -> Self {
        let mut chain_key_bytes = [0u8; 32];
        rng.fill_bytes(&mut chain_key_bytes);
        Self {
            chain_key: ChainKey::new(chain_key_bytes),
            iteration: 0,
            signing_key: SigningKey::generate(rng),
        }
    }

    /// [`Self::generate`], for callers across an FFI boundary where a
    /// caller-supplied `impl CryptoRngCore` can't cross the boundary —
    /// mirrors [`crate::ratchet::DoubleRatchet::init_initiator_from_bytes`]'s
    /// own reasoning for defaulting to [`OsRng`] at that boundary.
    pub fn generate_from_os_rng() -> Self {
        Self::generate(&mut OsRng)
    }

    /// This member's current chain state, to send to one other group
    /// member over an existing pairwise session (see this module's own
    /// doc comment) — every other member independently gets the same
    /// [`SenderKeyDistribution`], since it names nothing recipient-
    /// specific.
    pub fn to_distribution(&self) -> SenderKeyDistribution {
        SenderKeyDistribution {
            chain_key: *self.chain_key.as_bytes(),
            iteration: self.iteration,
            signing_public_key: self.signing_key.verifying_key().to_bytes(),
        }
    }

    /// Encrypts `plaintext`, advancing the chain by one step.
    /// `associated_data` is authenticated but not encrypted (e.g. a group
    /// id) — pass `&[]` if there is none. Returns a header and a single
    /// opaque blob (`signature || ciphertext`) to send, mirroring
    /// [`crate::ratchet::DoubleRatchet::encrypt`]'s shape.
    pub fn encrypt(&mut self, plaintext: &[u8], associated_data: &[u8]) -> Result<(SenderKeyHeader, Vec<u8>)> {
        let (message_key, next_chain) = self.chain_key.ratchet();
        let header = SenderKeyHeader { iteration: self.iteration };
        self.iteration += 1;
        self.chain_key = next_chain;

        let aad = full_associated_data(associated_data, &header);
        let ciphertext = aead::encrypt(&mut OsRng, &message_key, plaintext, &aad)?;
        let signature = self.signing_key.sign(&signed_bytes(&aad, &ciphertext));

        let mut out = Vec::with_capacity(SIGNATURE_LEN + ciphertext.len());
        out.extend_from_slice(&signature.to_bytes());
        out.extend_from_slice(&ciphertext);
        Ok((header, out))
    }

    /// Serializes this member's own outgoing state so it survives an app
    /// restart — as secret as the signing key it contains (anyone holding
    /// it could impersonate this member's future messages in the group).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + 5 + 32);
        out.extend_from_slice(self.chain_key.as_bytes());
        varint_encode(self.iteration as u64, &mut out);
        out.extend_from_slice(&self.signing_key.to_bytes());
        out
    }

    pub fn from_bytes(input: &[u8]) -> Result<Self> {
        if input.len() < 32 {
            return Err(CryptoError::Decode("sender key state shorter than a chain key"));
        }
        let chain_key = ChainKey::new(input[..32].try_into().unwrap());
        let (iteration, consumed) = varint_decode(&input[32..])?;
        let signing_key_bytes = &input[32 + consumed..];
        let signing_key_bytes: [u8; 32] = signing_key_bytes
            .try_into()
            .map_err(|_| CryptoError::Decode("sender key state has a malformed signing key"))?;
        Ok(Self { chain_key, iteration: iteration as u32, signing_key: SigningKey::from_bytes(&signing_key_bytes) })
    }
}

/// What a group member keeps for *each other* member's Sender Key chain —
/// received via [`SenderKeyDistribution`], never generated locally.
#[derive(Clone, ZeroizeOnDrop)]
pub struct SenderKeyReceiverState {
    chain_key: ChainKey,
    iteration: u32,
    #[zeroize(skip)] // Public; not secret on its own.
    verifying_key: VerifyingKey,
    #[zeroize(skip)] // Its own MessageKey entries zeroize themselves.
    skipped: SkippedIterations,
}

impl SenderKeyReceiverState {
    pub fn from_distribution(distribution: &SenderKeyDistribution) -> Result<Self> {
        let verifying_key = VerifyingKey::from_bytes(&distribution.signing_public_key)
            .map_err(|_| CryptoError::Decode("invalid Ed25519 public key"))?;
        Ok(Self {
            chain_key: ChainKey::new(distribution.chain_key),
            iteration: distribution.iteration,
            verifying_key,
            skipped: SkippedIterations::new(),
        })
    }

    /// Verifies and decrypts a message, handling out-of-order delivery
    /// transparently (fast-forwarding the chain and caching skipped keys,
    /// the same shape as
    /// [`crate::ratchet::DoubleRatchet::decrypt`]'s own handling, minus a
    /// DH ratchet step this chain never takes).
    ///
    /// The signature is checked *before* touching any chain state — an
    /// attacker who doesn't hold the signing key must not be able to
    /// force this receiver to derive and cache keys at all, since
    /// `iteration` in the header is otherwise unauthenticated input.
    pub fn decrypt(&mut self, header: &SenderKeyHeader, signed_ciphertext: &[u8], associated_data: &[u8]) -> Result<Vec<u8>> {
        if signed_ciphertext.len() < SIGNATURE_LEN {
            return Err(CryptoError::Decode("message shorter than a signature"));
        }
        let (signature_bytes, ciphertext) = signed_ciphertext.split_at(SIGNATURE_LEN);
        let signature = Signature::from_bytes(signature_bytes.try_into().unwrap());
        let aad = full_associated_data(associated_data, header);
        self.verifying_key
            .verify(&signed_bytes(&aad, ciphertext), &signature)
            .map_err(|_| CryptoError::SignatureInvalid)?;

        let mut working = self.clone();
        let plaintext = if let Some(message_key) = working.skipped.take(header.iteration) {
            aead::decrypt(&message_key, ciphertext, &aad)?
        } else {
            working.skip_to(header.iteration)?;
            let (message_key, next_chain) = working.chain_key.ratchet();
            working.chain_key = next_chain;
            working.iteration += 1;
            aead::decrypt(&message_key, ciphertext, &aad)?
        };

        *self = working;
        Ok(plaintext)
    }

    /// Ratchets the chain forward from the current `iteration` up to (but
    /// not including) `until`, stashing each derived key for a still-later
    /// out-of-order arrival. A no-op if `until <= iteration` (already at
    /// or past that point — [`Self::decrypt`] itself handles "already
    /// past and skip-cached" via `skipped.take` before ever calling this).
    fn skip_to(&mut self, until: u32) -> Result<()> {
        while self.iteration < until {
            let (message_key, next_chain) = self.chain_key.ratchet();
            self.skipped.insert(self.iteration, message_key)?;
            self.chain_key = next_chain;
            self.iteration += 1;
        }
        Ok(())
    }

    /// Serializes this receiver-side state (another member's chain, as
    /// currently known to us) so it survives an app restart.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(self.chain_key.as_bytes());
        varint_encode(self.iteration as u64, &mut out);
        out.extend_from_slice(&self.verifying_key.to_bytes());

        let skipped = self.skipped.entries();
        varint_encode(skipped.len() as u64, &mut out);
        for (iteration, key) in skipped {
            varint_encode(iteration as u64, &mut out);
            out.extend_from_slice(key.as_bytes());
        }
        out
    }

    pub fn from_bytes(input: &[u8]) -> Result<Self> {
        let mut offset = 0;
        let chain_key = ChainKey::new(read_fixed(input, &mut offset)?);
        let iteration = read_varint_u32(input, &mut offset)?;
        let verifying_key_bytes = read_fixed(input, &mut offset)?;
        let verifying_key =
            VerifyingKey::from_bytes(&verifying_key_bytes).map_err(|_| CryptoError::Decode("invalid Ed25519 public key"))?;

        let skipped_count = read_varint_u32(input, &mut offset)?;
        let mut entries = Vec::with_capacity(skipped_count as usize);
        for _ in 0..skipped_count {
            let iteration = read_varint_u32(input, &mut offset)?;
            let key = MessageKey(read_fixed(input, &mut offset)?);
            entries.push((iteration, key));
        }

        Ok(Self { chain_key, iteration, verifying_key, skipped: SkippedIterations::from_entries(entries) })
    }
}

fn full_associated_data(caller_aad: &[u8], header: &SenderKeyHeader) -> Vec<u8> {
    let header_bytes = header.encode();
    let mut out = Vec::with_capacity(caller_aad.len() + header_bytes.len());
    out.extend_from_slice(caller_aad);
    out.extend_from_slice(&header_bytes);
    out
}

/// What actually gets signed: the same authenticated-data bytes the AEAD
/// tag already covers, plus the ciphertext itself — binding the
/// signature to the exact bytes a recipient will decrypt, not just to the
/// header/AAD in isolation.
fn signed_bytes(aad: &[u8], ciphertext: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(aad.len() + ciphertext.len());
    out.extend_from_slice(aad);
    out.extend_from_slice(ciphertext);
    out
}

fn read_fixed(input: &[u8], offset: &mut usize) -> Result<[u8; 32]> {
    let end = *offset + 32;
    let bytes = input.get(*offset..end).ok_or(CryptoError::Decode("truncated sender key receiver state"))?;
    *offset = end;
    Ok(bytes.try_into().unwrap())
}

fn read_varint_u32(input: &[u8], offset: &mut usize) -> Result<u32> {
    let remaining = input.get(*offset..).ok_or(CryptoError::Decode("truncated sender key receiver state"))?;
    let (value, consumed) = varint_decode(remaining)?;
    *offset += consumed;
    Ok(value as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    fn test_rng() -> ChaCha20Rng {
        ChaCha20Rng::seed_from_u64(1)
    }

    #[test]
    fn a_message_encrypted_by_the_sender_decrypts_for_a_receiver_built_from_its_distribution() {
        let mut sender = SenderKeyState::generate(&mut test_rng());
        let mut receiver = SenderKeyReceiverState::from_distribution(&sender.to_distribution()).unwrap();

        let (header, signed_ciphertext) = sender.encrypt(b"hello group", b"group-id").unwrap();
        let plaintext = receiver.decrypt(&header, &signed_ciphertext, b"group-id").unwrap();
        assert_eq!(plaintext, b"hello group");
    }

    #[test]
    fn consecutive_messages_use_different_keys_and_still_decrypt_in_order() {
        let mut sender = SenderKeyState::generate(&mut test_rng());
        let mut receiver = SenderKeyReceiverState::from_distribution(&sender.to_distribution()).unwrap();

        let (h1, c1) = sender.encrypt(b"first", b"").unwrap();
        let (h2, c2) = sender.encrypt(b"second", b"").unwrap();
        assert_ne!(c1, c2);
        assert_eq!(receiver.decrypt(&h1, &c1, b"").unwrap(), b"first");
        assert_eq!(receiver.decrypt(&h2, &c2, b"").unwrap(), b"second");
    }

    #[test]
    fn an_out_of_order_message_still_decrypts_and_the_skipped_one_can_arrive_later() {
        let mut sender = SenderKeyState::generate(&mut test_rng());
        let mut receiver = SenderKeyReceiverState::from_distribution(&sender.to_distribution()).unwrap();

        let (h1, c1) = sender.encrypt(b"delayed", b"").unwrap();
        let (h2, c2) = sender.encrypt(b"on time", b"").unwrap();

        assert_eq!(receiver.decrypt(&h2, &c2, b"").unwrap(), b"on time");
        assert_eq!(receiver.decrypt(&h1, &c1, b"").unwrap(), b"delayed");
    }

    #[test]
    fn a_second_group_member_with_their_own_chain_decrypts_independently() {
        let mut alice = SenderKeyState::generate(&mut test_rng());
        let mut bob = SenderKeyState::generate(&mut ChaCha20Rng::seed_from_u64(2));
        let mut receiver_of_alice = SenderKeyReceiverState::from_distribution(&alice.to_distribution()).unwrap();
        let mut receiver_of_bob = SenderKeyReceiverState::from_distribution(&bob.to_distribution()).unwrap();

        let (ha, ca) = alice.encrypt(b"from alice", b"group").unwrap();
        let (hb, cb) = bob.encrypt(b"from bob", b"group").unwrap();

        assert_eq!(receiver_of_alice.decrypt(&ha, &ca, b"group").unwrap(), b"from alice");
        assert_eq!(receiver_of_bob.decrypt(&hb, &cb, b"group").unwrap(), b"from bob");
        // Cross-wiring must fail: bob's message never verifies against
        // alice's chain/signing key, even though both target the same group.
        assert!(receiver_of_alice.decrypt(&hb, &cb, b"group").is_err());
    }

    #[test]
    fn a_forged_message_under_a_shared_chain_key_still_fails_without_the_real_signing_key() {
        // The whole reason this scheme signs at all: anyone who has
        // received the distribution knows the chain key (it's shared),
        // so they *could* derive a valid-looking AEAD ciphertext at some
        // iteration. They still can't forge the signature without the
        // real sender's private signing key.
        let mut sender = SenderKeyState::generate(&mut test_rng());
        let mut receiver = SenderKeyReceiverState::from_distribution(&sender.to_distribution()).unwrap();
        let (header, signed_ciphertext) = sender.encrypt(b"hello", b"").unwrap();

        let mut forged = signed_ciphertext.clone();
        // Corrupt only the signature bytes, leaving a still-decryptable
        // ciphertext underneath — proving the signature check, not AEAD
        // failure, is what rejects this.
        forged[0] ^= 0xff;
        assert!(receiver.decrypt(&header, &forged, b"").is_err());
    }

    #[test]
    fn tampering_with_the_ciphertext_is_rejected() {
        let mut sender = SenderKeyState::generate(&mut test_rng());
        let mut receiver = SenderKeyReceiverState::from_distribution(&sender.to_distribution()).unwrap();
        let (header, mut signed_ciphertext) = sender.encrypt(b"hello", b"").unwrap();

        *signed_ciphertext.last_mut().unwrap() ^= 0xff;
        assert!(receiver.decrypt(&header, &signed_ciphertext, b"").is_err());
    }

    #[test]
    fn mismatched_associated_data_is_rejected() {
        let mut sender = SenderKeyState::generate(&mut test_rng());
        let mut receiver = SenderKeyReceiverState::from_distribution(&sender.to_distribution()).unwrap();
        let (header, signed_ciphertext) = sender.encrypt(b"hello", b"right-group").unwrap();

        assert!(receiver.decrypt(&header, &signed_ciphertext, b"wrong-group").is_err());
    }

    #[test]
    fn sender_state_round_trips_through_bytes_and_keeps_sending() {
        let mut sender = SenderKeyState::generate(&mut test_rng());
        let _ = sender.encrypt(b"burn one iteration", b"").unwrap();

        let restored = SenderKeyState::from_bytes(&sender.to_bytes()).unwrap();
        let mut sender = restored;
        let mut receiver = SenderKeyReceiverState::from_distribution(&sender.to_distribution()).unwrap();

        let (header, signed_ciphertext) = sender.encrypt(b"after restart", b"").unwrap();
        assert_eq!(receiver.decrypt(&header, &signed_ciphertext, b"").unwrap(), b"after restart");
    }

    #[test]
    fn receiver_state_round_trips_through_bytes_including_skipped_keys() {
        let mut sender = SenderKeyState::generate(&mut test_rng());
        let mut receiver = SenderKeyReceiverState::from_distribution(&sender.to_distribution()).unwrap();

        let (h1, c1) = sender.encrypt(b"delayed", b"").unwrap();
        let (h2, c2) = sender.encrypt(b"on time", b"").unwrap();
        assert_eq!(receiver.decrypt(&h2, &c2, b"").unwrap(), b"on time");

        // "delayed" is still outstanding when the receiver's state gets
        // persisted and reloaded.
        let mut receiver = SenderKeyReceiverState::from_bytes(&receiver.to_bytes()).unwrap();
        assert_eq!(receiver.decrypt(&h1, &c1, b"").unwrap(), b"delayed");
    }

    #[test]
    fn rejects_truncated_sender_state_bytes() {
        let sender = SenderKeyState::generate(&mut test_rng());
        let mut bytes = sender.to_bytes();
        bytes.truncate(10);
        assert!(SenderKeyState::from_bytes(&bytes).is_err());
    }

    #[test]
    fn a_member_who_joins_mid_conversation_only_gets_the_current_position_onward() {
        let mut sender = SenderKeyState::generate(&mut test_rng());
        let (h1, c1) = sender.encrypt(b"before the new member joined", b"").unwrap();

        // The new member's receiver state is built from a distribution
        // taken *after* the message above — it must not be able to
        // decrypt something encrypted at an earlier iteration than it
        // was ever told about, since it never learns the chain's past
        // (forward secrecy: its chain key is already past that point and
        // cannot be run backwards).
        let mut late_joiner = SenderKeyReceiverState::from_distribution(&sender.to_distribution()).unwrap();
        assert!(late_joiner.decrypt(&h1, &c1, b"").is_err());

        let (h2, c2) = sender.encrypt(b"after the new member joined", b"").unwrap();
        assert_eq!(late_joiner.decrypt(&h2, &c2, b"").unwrap(), b"after the new member joined");
    }
}
