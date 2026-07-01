//! The Double Ratchet state machine, combining the DH ratchet (a new
//! Diffie-Hellman key pair generated every time the conversation direction
//! flips) with a symmetric-key ratchet per direction (advancing on every
//! message). Losing a single message key only exposes that one message;
//! losing a whole chain key only exposes messages after the last DH step.

use rand_core::OsRng;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};
use zeroize::ZeroizeOnDrop;

use crate::error::{CryptoError, Result};
use crate::handshake::SharedSecret;

use super::aead;
use super::chain::ChainKey;
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
