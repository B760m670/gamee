//! Application messages under the current epoch — the layer that actually
//! encrypts chat content, once TreeKEM has agreed an epoch secret. Each
//! message is keyed from the epoch's `encryption_secret` by the sender's
//! leaf and a per-sender generation counter (RFC 9420's sender-ratchet
//! idea, compact form): key = KDF(encryption_secret, sender ‖ generation).
//! Because every group member holds `encryption_secret`, AEAD alone can't
//! attribute a message — so, exactly as Sender Keys already does, every
//! message is Ed25519-signed by the sender's own identity key, verified
//! against the roster. Confidentiality from the epoch key, authorship from
//! the signature.
//!
//! Forward secrecy across epochs is automatic: a membership change or a
//! self-update rotates `encryption_secret`, so old message keys are
//! underivable afterward. Within an epoch, distinct generations give
//! distinct keys; deleting spent keys (true per-message FS inside an
//! epoch) is a persistence policy left to the app, as it is for the 1:1
//! ratchet.

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hkdf::Hkdf;
use sha2::Sha256;

use crate::member::MemberIdentity;
use crate::tree_math::LeafIndex;

const NONCE_LEN: usize = 12;

fn message_key_nonce(encryption_secret: &[u8; 32], sender: LeafIndex, generation: u32) -> ([u8; 32], [u8; NONCE_LEN]) {
    let hk = Hkdf::<Sha256>::from_prk(encryption_secret).expect("32-byte PRK is valid");
    let mut info = Vec::with_capacity(24 + 8);
    info.extend_from_slice(b"spiritchat-mls-appmsg-v1:");
    info.extend_from_slice(&sender.to_le_bytes());
    info.extend_from_slice(&generation.to_le_bytes());
    let mut okm = [0u8; 32 + NONCE_LEN];
    hk.expand(&info, &mut okm).expect("44 bytes is a valid HKDF output length");
    let mut key = [0u8; 32];
    let mut nonce = [0u8; NONCE_LEN];
    key.copy_from_slice(&okm[..32]);
    nonce.copy_from_slice(&okm[32..]);
    (key, nonce)
}

/// The bytes signed by (and verified against) the sender — binds the
/// ciphertext to its exact group/epoch/sender/generation position so it
/// can't be lifted elsewhere or reattributed.
fn signed_payload(group_id: &[u8], epoch: u64, sender: LeafIndex, generation: u32, ciphertext: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(group_id.len() + 16 + ciphertext.len());
    buf.extend_from_slice(b"spiritchat-mls-appsig-v1:");
    buf.extend_from_slice(group_id);
    buf.extend_from_slice(&epoch.to_le_bytes());
    buf.extend_from_slice(&sender.to_le_bytes());
    buf.extend_from_slice(&generation.to_le_bytes());
    buf.extend_from_slice(ciphertext);
    buf
}

/// A decrypted application message: who sent it (by leaf) and the
/// plaintext.
#[derive(Debug)]
pub struct OpenedMessage {
    pub sender: LeafIndex,
    pub plaintext: Vec<u8>,
}

/// Wire layout: sender(4) ‖ generation(4) ‖ signature(64) ‖ ciphertext.
pub fn seal_message(
    encryption_secret: &[u8; 32],
    group_id: &[u8],
    epoch: u64,
    sender: LeafIndex,
    generation: u32,
    signing_key: &SigningKey,
    plaintext: &[u8],
) -> Vec<u8> {
    let (key, nonce) = message_key_nonce(encryption_secret, sender, generation);
    let cipher = ChaCha20Poly1305::new((&key).into());
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: plaintext, aad: group_id })
        .expect("ChaCha20-Poly1305 encryption does not fail");
    let signature = signing_key.sign(&signed_payload(group_id, epoch, sender, generation, &ciphertext)).to_bytes();

    let mut out = Vec::with_capacity(8 + 64 + ciphertext.len());
    out.extend_from_slice(&sender.to_le_bytes());
    out.extend_from_slice(&generation.to_le_bytes());
    out.extend_from_slice(&signature);
    out.extend_from_slice(&ciphertext);
    out
}

/// Verifies and decrypts. `identity_of` resolves a sender leaf to the
/// identity the group's roster records there — a message whose signature
/// doesn't match that identity is rejected, even though its ciphertext
/// would decrypt under the shared epoch key.
pub fn open_message(
    encryption_secret: &[u8; 32],
    group_id: &[u8],
    epoch: u64,
    wire: &[u8],
    identity_of: impl Fn(LeafIndex) -> Option<MemberIdentity>,
) -> Result<OpenedMessage, MessageError> {
    if wire.len() < 72 {
        return Err(MessageError::Malformed);
    }
    let sender = LeafIndex::from_le_bytes(wire[0..4].try_into().unwrap());
    let generation = u32::from_le_bytes(wire[4..8].try_into().unwrap());
    let signature = Signature::from_bytes(wire[8..72].try_into().unwrap());
    let ciphertext = &wire[72..];

    let identity = identity_of(sender).ok_or(MessageError::UnknownSender)?;
    let vk = VerifyingKey::from_bytes(&identity.0).map_err(|_| MessageError::BadSignature)?;
    vk.verify(&signed_payload(group_id, epoch, sender, generation, ciphertext), &signature)
        .map_err(|_| MessageError::BadSignature)?;

    let (key, nonce) = message_key_nonce(encryption_secret, sender, generation);
    let cipher = ChaCha20Poly1305::new((&key).into());
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), Payload { msg: ciphertext, aad: group_id })
        .map_err(|_| MessageError::DecryptionFailed)?;
    Ok(OpenedMessage { sender, plaintext })
}

#[derive(Debug, PartialEq, Eq)]
pub enum MessageError {
    Malformed,
    UnknownSender,
    BadSignature,
    DecryptionFailed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    fn sk(seed: u64) -> SigningKey {
        SigningKey::generate(&mut ChaCha20Rng::seed_from_u64(seed))
    }

    #[test]
    fn a_signed_message_round_trips() {
        let secret = [3u8; 32];
        let signer = sk(1);
        let id = MemberIdentity(signer.verifying_key().to_bytes());
        let wire = seal_message(&secret, b"g", 4, 2, 0, &signer, b"hello group");
        let opened = open_message(&secret, b"g", 4, &wire, |leaf| if leaf == 2 { Some(id) } else { None }).unwrap();
        assert_eq!(opened.sender, 2);
        assert_eq!(opened.plaintext, b"hello group");
    }

    #[test]
    fn a_forged_signature_is_rejected_even_under_the_shared_key() {
        let secret = [3u8; 32];
        let real = sk(1);
        let attacker = sk(2);
        // Attacker knows the epoch key (they're in the group) and seals a
        // message claiming to be leaf 2, but signs with their own key.
        let wire = seal_message(&secret, b"g", 4, 2, 0, &attacker, b"impersonation");
        let real_id = MemberIdentity(real.verifying_key().to_bytes());
        let err = open_message(&secret, b"g", 4, &wire, |_| Some(real_id)).unwrap_err();
        assert_eq!(err, MessageError::BadSignature);
    }

    #[test]
    fn a_different_epoch_key_cannot_decrypt() {
        let signer = sk(1);
        let id = MemberIdentity(signer.verifying_key().to_bytes());
        let wire = seal_message(&[3u8; 32], b"g", 4, 2, 0, &signer, b"secret");
        // Wrong epoch secret (a past/future epoch) — signature might be
        // structurally checkable but the AEAD key won't match.
        assert!(open_message(&[4u8; 32], b"g", 4, &wire, |_| Some(id)).is_err());
    }

    #[test]
    fn distinct_generations_produce_distinct_ciphertexts() {
        let signer = sk(1);
        let a = seal_message(&[3u8; 32], b"g", 4, 2, 0, &signer, b"same");
        let b = seal_message(&[3u8; 32], b"g", 4, 2, 1, &signer, b"same");
        assert_ne!(a[72..], b[72..]);
    }
}
