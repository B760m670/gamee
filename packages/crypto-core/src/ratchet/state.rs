//! The Double Ratchet state machine, combining the DH ratchet (a new
//! Diffie-Hellman key pair generated every time the conversation direction
//! flips) with a symmetric-key ratchet per direction (advancing on every
//! message). Losing a single message key only exposes that one message;
//! losing a whole chain key only exposes messages after the last DH step.

use rand_core::OsRng;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};
use zeroize::ZeroizeOnDrop;

use crate::encoding::{varint_decode, varint_encode};
use crate::error::{CryptoError, Result};
use crate::handshake::SharedSecret;

use super::aead;
use super::chain::{ChainKey, MessageKey};
use super::header::MessageHeader;
use super::root_kdf;
use super::skipped_keys::SkippedKeyStore;

#[derive(Clone, ZeroizeOnDrop)]
pub struct DoubleRatchet {
    dh_self: StaticSecret,
    #[zeroize(skip)] // Public, derived from dh_self; not secret on its own.
    dh_self_public: X25519PublicKey,
    #[zeroize(skip)]
    dh_remote: Option<X25519PublicKey>,
    root_key: [u8; 32],
    sending_chain: Option<ChainKey>,
    receiving_chain: Option<ChainKey>,
    send_count: u32,
    recv_count: u32,
    previous_sending_chain_length: u32,
    #[zeroize(skip)] // SkippedKeyStore's own MessageKey entries zeroize themselves.
    skipped: SkippedKeyStore,
}

impl DoubleRatchet {
    /// Initializes the initiator's ("Alice's") side right after her X3DH
    /// handshake. `remote_ratchet_public` is the responder's initial
    /// ratchet key — in our design that is their X3DH signed prekey,
    /// reused as their first Double Ratchet key since they have not sent
    /// anything yet to establish a dedicated one.
    pub fn init_initiator(
        rng: &mut impl rand_core::CryptoRngCore,
        shared_secret: &SharedSecret,
        remote_ratchet_public: X25519PublicKey,
    ) -> Self {
        let dh_self = StaticSecret::random_from_rng(rng);
        let dh_self_public = X25519PublicKey::from(&dh_self);

        let dh_output = dh_self.diffie_hellman(&remote_ratchet_public);
        let (root_key, sending_chain) = root_kdf::derive(shared_secret.as_bytes(), dh_output.as_bytes());

        Self {
            dh_self,
            dh_self_public,
            dh_remote: Some(remote_ratchet_public),
            root_key,
            sending_chain: Some(sending_chain),
            receiving_chain: None,
            send_count: 0,
            recv_count: 0,
            previous_sending_chain_length: 0,
            skipped: SkippedKeyStore::new(),
        }
    }

    /// Initializes the responder's ("Bob's") side. `my_ratchet_secret` is
    /// the private half of the key the initiator used as
    /// `remote_ratchet_public` (our X3DH signed prekey secret) — Bob has no
    /// sending chain yet; he can only reply after processing Alice's first
    /// message, which is what triggers his first DH ratchet step.
    pub fn init_responder(shared_secret: &SharedSecret, my_ratchet_secret: StaticSecret) -> Self {
        let dh_self_public = X25519PublicKey::from(&my_ratchet_secret);
        Self {
            dh_self: my_ratchet_secret,
            dh_self_public,
            dh_remote: None,
            root_key: *shared_secret.as_bytes(),
            sending_chain: None,
            receiving_chain: None,
            send_count: 0,
            recv_count: 0,
            previous_sending_chain_length: 0,
            skipped: SkippedKeyStore::new(),
        }
    }

    /// [`Self::init_initiator`] taking raw key bytes instead of typed keys,
    /// and a random ephemeral DH key generated internally from [`OsRng`]
    /// instead of a caller-supplied RNG — for callers across an FFI
    /// boundary, where a caller-supplied `impl CryptoRngCore` can't cross
    /// the boundary and there is no need for the determinism the typed
    /// `rng`-parameterized constructor exists to support in tests.
    pub fn init_initiator_from_bytes(
        shared_secret: &SharedSecret,
        remote_ratchet_public_bytes: &[u8],
    ) -> Result<Self> {
        let remote_ratchet_public =
            crate::prekey::x25519_public_from_bytes(remote_ratchet_public_bytes)?;
        Ok(Self::init_initiator(
            &mut OsRng,
            shared_secret,
            remote_ratchet_public,
        ))
    }

    /// [`Self::init_responder`] taking a raw key byte slice instead of a
    /// typed [`StaticSecret`], for callers across an FFI boundary.
    pub fn init_responder_from_bytes(
        shared_secret: &SharedSecret,
        my_ratchet_secret_bytes: &[u8],
    ) -> Result<Self> {
        let array: [u8; 32] =
            my_ratchet_secret_bytes
                .try_into()
                .map_err(|_| CryptoError::InvalidKeyLength {
                    expected: 32,
                    actual: my_ratchet_secret_bytes.len(),
                })?;
        Ok(Self::init_responder(shared_secret, StaticSecret::from(array)))
    }

    /// Encrypts `plaintext`, advancing the sending chain by one step.
    /// `associated_data` is authenticated but not encrypted (e.g. a
    /// conversation ID) — pass `&[]` if there is none.
    pub fn encrypt(&mut self, plaintext: &[u8], associated_data: &[u8]) -> Result<(MessageHeader, Vec<u8>)> {
        let chain = self
            .sending_chain
            .as_ref()
            .ok_or(CryptoError::MessageKeyUnavailable)?;
        let (message_key, next_chain) = chain.ratchet();
        self.sending_chain = Some(next_chain);

        let header = MessageHeader {
            dh_public: self.dh_self_public.to_bytes(),
            previous_chain_length: self.previous_sending_chain_length,
            message_number: self.send_count,
        };
        self.send_count += 1;

        let aad = full_associated_data(associated_data, &header);
        let ciphertext = aead::encrypt(&mut OsRng, &message_key, plaintext, &aad)?;
        Ok((header, ciphertext))
    }

    /// Decrypts a message given its header, handling out-of-order delivery
    /// and DH ratchet steps transparently.
    ///
    /// Every step here — consuming a skipped key, rotating the DH ratchet,
    /// advancing the receiving chain — operates on a scratch clone of the
    /// state, not `self`, until the AEAD tag actually verifies. A header is
    /// unauthenticated data: an attacker (or a corrupted packet) could name
    /// a bogus `dh_public`/`message_number`, and without this staging a
    /// failed decrypt would still leave the real ratchet state rotated or
    /// advanced, permanently breaking the session for messages that were
    /// never actually lost.
    pub fn decrypt(
        &mut self,
        header: &MessageHeader,
        ciphertext: &[u8],
        associated_data: &[u8],
    ) -> Result<Vec<u8>> {
        let mut working = self.clone();
        let remote_key = header.dh_public_key();
        let aad = full_associated_data(associated_data, header);

        let plaintext = if let Some(message_key) = working.skipped.take(&remote_key, header.message_number) {
            aead::decrypt(&message_key, ciphertext, &aad)?
        } else {
            if working.dh_remote != Some(remote_key) {
                working.skip_message_keys(header.previous_chain_length)?;
                working.dh_ratchet_step(remote_key);
            }

            working.skip_message_keys(header.message_number)?;

            let chain = working
                .receiving_chain
                .as_ref()
                .ok_or(CryptoError::MessageKeyUnavailable)?;
            let (message_key, next_chain) = chain.ratchet();
            working.receiving_chain = Some(next_chain);
            working.recv_count += 1;

            aead::decrypt(&message_key, ciphertext, &aad)?
        };

        *self = working;
        Ok(plaintext)
    }

    pub fn local_ratchet_public(&self) -> X25519PublicKey {
        self.dh_self_public
    }

    /// Serializes the full ratchet state — both chain keys, the root key,
    /// every skipped message key, and all bookkeeping counters — so a
    /// conversation can survive an app restart. This is as secret as any
    /// individual message key it contains; store it exactly as securely.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.dh_self.to_bytes());
        write_optional_public_key(&mut out, self.dh_remote);
        out.extend_from_slice(&self.root_key);
        write_optional_chain_key(&mut out, self.sending_chain.as_ref());
        write_optional_chain_key(&mut out, self.receiving_chain.as_ref());
        varint_encode(self.send_count as u64, &mut out);
        varint_encode(self.recv_count as u64, &mut out);
        varint_encode(self.previous_sending_chain_length as u64, &mut out);

        let skipped = self.skipped.entries();
        varint_encode(skipped.len() as u64, &mut out);
        for (dh_public, message_number, key) in skipped {
            out.extend_from_slice(&dh_public);
            varint_encode(message_number as u64, &mut out);
            out.extend_from_slice(key.as_bytes());
        }

        out
    }

    pub fn from_bytes(input: &[u8]) -> Result<Self> {
        let mut offset = 0;

        let dh_self = StaticSecret::from(read_fixed(input, &mut offset)?);
        let dh_self_public = X25519PublicKey::from(&dh_self);
        let dh_remote = read_optional_public_key(input, &mut offset)?;
        let root_key = read_fixed(input, &mut offset)?;
        let sending_chain = read_optional_chain_key(input, &mut offset)?;
        let receiving_chain = read_optional_chain_key(input, &mut offset)?;
        let send_count = read_varint_u32(input, &mut offset)?;
        let recv_count = read_varint_u32(input, &mut offset)?;
        let previous_sending_chain_length = read_varint_u32(input, &mut offset)?;

        let skipped_count = read_varint_u32(input, &mut offset)?;
        let mut skipped_entries = Vec::with_capacity(skipped_count as usize);
        for _ in 0..skipped_count {
            let dh_public = read_fixed(input, &mut offset)?;
            let message_number = read_varint_u32(input, &mut offset)?;
            let key = MessageKey(read_fixed(input, &mut offset)?);
            skipped_entries.push((dh_public, message_number, key));
        }

        Ok(Self {
            dh_self,
            dh_self_public,
            dh_remote,
            root_key,
            sending_chain,
            receiving_chain,
            send_count,
            recv_count,
            previous_sending_chain_length,
            skipped: SkippedKeyStore::from_entries(skipped_entries),
        })
    }

    /// Ratchets the current receiving chain forward from `recv_count` up to
    /// (but not including) `until`, stashing each derived key for later
    /// out-of-order delivery. A no-op if we have no receiving chain yet
    /// (e.g. Bob, before his first DH step).
    fn skip_message_keys(&mut self, until: u32) -> Result<()> {
        let (Some(remote_key), Some(mut chain)) = (self.dh_remote, self.receiving_chain.clone())
        else {
            return Ok(());
        };

        while self.recv_count < until {
            let (message_key, next_chain) = chain.ratchet();
            self.skipped.insert(remote_key, self.recv_count, message_key)?;
            chain = next_chain;
            self.recv_count += 1;
        }

        self.receiving_chain = Some(chain);
        Ok(())
    }

    /// Performs a DH ratchet step on receiving a new remote ratchet key:
    /// first derives our new receiving chain (with our *old* DH key against
    /// their *new* one), then generates a fresh DH key pair for ourselves
    /// and derives a new sending chain from it — matching the Double
    /// Ratchet spec's `DHRatchet` procedure.
    fn dh_ratchet_step(&mut self, remote_key: X25519PublicKey) {
        self.previous_sending_chain_length = self.send_count;
        self.send_count = 0;
        self.recv_count = 0;
        self.dh_remote = Some(remote_key);

        let recv_dh_output = self.dh_self.diffie_hellman(&remote_key);
        let (root_after_recv, receiving_chain) = root_kdf::derive(&self.root_key, recv_dh_output.as_bytes());
        self.root_key = root_after_recv;
        self.receiving_chain = Some(receiving_chain);

        self.dh_self = StaticSecret::random_from_rng(OsRng);
        self.dh_self_public = X25519PublicKey::from(&self.dh_self);

        let send_dh_output = self.dh_self.diffie_hellman(&remote_key);
        let (root_after_send, sending_chain) = root_kdf::derive(&self.root_key, send_dh_output.as_bytes());
        self.root_key = root_after_send;
        self.sending_chain = Some(sending_chain);
    }
}

fn full_associated_data(caller_aad: &[u8], header: &MessageHeader) -> Vec<u8> {
    let header_bytes = header.encode();
    let mut out = Vec::with_capacity(caller_aad.len() + header_bytes.len());
    out.extend_from_slice(caller_aad);
    out.extend_from_slice(&header_bytes);
    out
}

fn write_optional_public_key(out: &mut Vec<u8>, key: Option<X25519PublicKey>) {
    match key {
        Some(key) => {
            out.push(1);
            out.extend_from_slice(key.as_bytes());
        }
        None => out.push(0),
    }
}

fn write_optional_chain_key(out: &mut Vec<u8>, key: Option<&ChainKey>) {
    match key {
        Some(key) => {
            out.push(1);
            out.extend_from_slice(key.as_bytes());
        }
        None => out.push(0),
    }
}

fn read_fixed(input: &[u8], offset: &mut usize) -> Result<[u8; 32]> {
    let end = *offset + 32;
    let bytes = input
        .get(*offset..end)
        .ok_or(CryptoError::Decode("truncated ratchet state"))?;
    *offset = end;
    Ok(bytes.try_into().unwrap())
}

fn read_flag(input: &[u8], offset: &mut usize) -> Result<bool> {
    let byte = *input
        .get(*offset)
        .ok_or(CryptoError::Decode("truncated ratchet state"))?;
    *offset += 1;
    match byte {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(CryptoError::Decode("invalid optional-field flag byte")),
    }
}

fn read_optional_public_key(input: &[u8], offset: &mut usize) -> Result<Option<X25519PublicKey>> {
    if read_flag(input, offset)? {
        Ok(Some(X25519PublicKey::from(read_fixed(input, offset)?)))
    } else {
        Ok(None)
    }
}

fn read_optional_chain_key(input: &[u8], offset: &mut usize) -> Result<Option<ChainKey>> {
    if read_flag(input, offset)? {
        Ok(Some(ChainKey::new(read_fixed(input, offset)?)))
    } else {
        Ok(None)
    }
}

fn read_varint_u32(input: &[u8], offset: &mut usize) -> Result<u32> {
    let remaining = input
        .get(*offset..)
        .ok_or(CryptoError::Decode("truncated ratchet state"))?;
    let (value, consumed) = varint_decode(remaining)?;
    *offset += consumed;
    Ok(value as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    fn shared_secret(byte: u8) -> SharedSecret {
        // `SharedSecret`'s inner field is `pub(crate)`, so any module in
        // this crate can build one directly — these tests only care about
        // ratchet plumbing, not about how a real shared secret was derived.
        SharedSecret([byte; 32])
    }

    #[test]
    fn round_trips_an_initiator_through_bytes_before_any_messages() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let remote_public = X25519PublicKey::from(&StaticSecret::random_from_rng(&mut rng));
        let ratchet = DoubleRatchet::init_initiator(&mut rng, &shared_secret(1), remote_public);

        let restored = DoubleRatchet::from_bytes(&ratchet.to_bytes()).unwrap();
        assert_eq!(restored.dh_self_public, ratchet.dh_self_public);
        assert_eq!(restored.root_key, ratchet.root_key);
    }

    #[test]
    fn a_restored_ratchet_can_continue_the_conversation() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let bob_secret = StaticSecret::random_from_rng(&mut rng);
        let bob_public = X25519PublicKey::from(&bob_secret);

        let mut alice = DoubleRatchet::init_initiator(&mut rng, &shared_secret(7), bob_public);
        let mut bob = DoubleRatchet::init_responder(&shared_secret(7), bob_secret);

        let (h1, c1) = alice.encrypt(b"before restart", b"").unwrap();
        assert_eq!(bob.decrypt(&h1, &c1, b"").unwrap(), b"before restart");

        // Simulate an app restart: both sides reload from persisted bytes.
        let mut alice = DoubleRatchet::from_bytes(&alice.to_bytes()).unwrap();
        let mut bob = DoubleRatchet::from_bytes(&bob.to_bytes()).unwrap();

        let (h2, c2) = bob.encrypt(b"after restart", b"").unwrap();
        assert_eq!(alice.decrypt(&h2, &c2, b"").unwrap(), b"after restart");

        let (h3, c3) = alice.encrypt(b"still works", b"").unwrap();
        assert_eq!(bob.decrypt(&h3, &c3, b"").unwrap(), b"still works");
    }

    #[test]
    fn skipped_message_keys_survive_a_restart_and_can_still_decrypt_a_late_arrival() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let bob_secret = StaticSecret::random_from_rng(&mut rng);
        let bob_public = X25519PublicKey::from(&bob_secret);

        let mut alice = DoubleRatchet::init_initiator(&mut rng, &shared_secret(3), bob_public);
        let mut bob = DoubleRatchet::init_responder(&shared_secret(3), bob_secret);

        let lost = alice.encrypt(b"delayed", b"").unwrap();
        let delivered = alice.encrypt(b"on time", b"").unwrap();
        assert_eq!(bob.decrypt(&delivered.0, &delivered.1, b"").unwrap(), b"on time");

        // "lost" is still outstanding when bob's state gets persisted and reloaded.
        let mut bob = DoubleRatchet::from_bytes(&bob.to_bytes()).unwrap();
        assert_eq!(bob.decrypt(&lost.0, &lost.1, b"").unwrap(), b"delayed");
    }

    #[test]
    fn rejects_truncated_bytes() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let remote_public = X25519PublicKey::from(&StaticSecret::random_from_rng(&mut rng));
        let ratchet = DoubleRatchet::init_initiator(&mut rng, &shared_secret(1), remote_public);

        let mut bytes = ratchet.to_bytes();
        bytes.truncate(10);
        assert!(DoubleRatchet::from_bytes(&bytes).is_err());
    }
}
