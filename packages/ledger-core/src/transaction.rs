//! A `@username` claim — this chain's only transaction type. Reuses
//! `spiritchat_crypto_core::identity`'s Ed25519 signing/verification
//! directly rather than inventing a second signature scheme; the whole
//! point of this ledger existing is to bind a username to the exact same
//! identity key everything else in the app already trusts.

use serde::{Deserialize, Serialize};
use spiritchat_crypto_core::identity::{IdentityKeyPair, IdentityPublicKey};

use crate::error::{LedgerError, Result};
use crate::hash::Hash32;

pub const MIN_USERNAME_LEN: usize = 5;
pub const MAX_USERNAME_LEN: usize = 32;

/// Case-insensitive, whitespace-trimmed — mirrors
/// `apps/mobile/ios/utils/username.ts`'s rules exactly, since a name that
/// isn't valid there should never make it into a transaction at all.
pub fn normalize_username(username: &str) -> String {
    username.trim().to_lowercase()
}

pub fn validate_username_format(username: &str) -> Result<()> {
    if username.len() < MIN_USERNAME_LEN {
        return Err(LedgerError::InvalidUsername("shorter than the 5-character minimum"));
    }
    if username.len() > MAX_USERNAME_LEN {
        return Err(LedgerError::InvalidUsername("longer than the 32-character maximum"));
    }
    if !username.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') {
        return Err(LedgerError::InvalidUsername("must be lowercase letters, digits, or '_' only"));
    }
    Ok(())
}

const CLAIM_DOMAIN_TAG: &[u8] = b"spiritchat-ledger-claim-v1";

/// A signed claim binding `owner_public_key` to `username`. `nonce`
/// dedupes otherwise-identical-looking claims (e.g. the same person
/// re-submitting); `anchor_block_hash` binds the signature to a specific
/// point in a specific chain, so a claim signed while anchored to one
/// fork's block can never be replayed as valid on a different fork whose
/// block at that height hashes differently.
///
/// `signature` is a `Vec<u8>` (always exactly 64 bytes, checked in
/// `verify_self_contained`) rather than `[u8; 64]` only because `serde`'s
/// built-in fixed-array support tops out well short of 64 elements —
/// nothing here treats it as variable-length.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    pub version: u8,
    pub username: String,
    pub owner_public_key: [u8; 32],
    pub claimed_at_height_hint: u64,
    pub anchor_block_hash: Hash32,
    pub nonce: [u8; 8],
    pub signature: Vec<u8>,
}

impl Transaction {
    fn signing_preimage(
        username: &str,
        owner_public_key: &[u8; 32],
        claimed_at_height_hint: u64,
        anchor_block_hash: &Hash32,
        nonce: &[u8; 8],
    ) -> Vec<u8> {
        let username_bytes = username.as_bytes();
        let mut buf = Vec::with_capacity(
            CLAIM_DOMAIN_TAG.len() + 2 + username_bytes.len() + 32 + 8 + 32 + 8,
        );
        buf.extend_from_slice(CLAIM_DOMAIN_TAG);
        buf.extend_from_slice(&(username_bytes.len() as u16).to_le_bytes());
        buf.extend_from_slice(username_bytes);
        buf.extend_from_slice(owner_public_key);
        buf.extend_from_slice(&claimed_at_height_hint.to_le_bytes());
        buf.extend_from_slice(anchor_block_hash.as_bytes());
        buf.extend_from_slice(nonce);
        buf
    }

    /// Builds and signs a new claim. `username` need not already be
    /// normalized/validated — this does both itself, so an invalid
    /// transaction is impossible to construct rather than merely rejected
    /// later during block validation.
    pub fn new_claim(
        identity: &IdentityKeyPair,
        username: &str,
        claimed_at_height_hint: u64,
        anchor_block_hash: Hash32,
        nonce: [u8; 8],
    ) -> Result<Self> {
        let username = normalize_username(username);
        validate_username_format(&username)?;
        let owner_public_key = identity.public_key().to_bytes();
        let preimage = Self::signing_preimage(
            &username,
            &owner_public_key,
            claimed_at_height_hint,
            &anchor_block_hash,
            &nonce,
        );
        let signature = identity.sign(&preimage).to_vec();
        Ok(Self {
            version: 1,
            username,
            owner_public_key,
            claimed_at_height_hint,
            anchor_block_hash,
            nonce,
            signature,
        })
    }

    /// The exact bytes this transaction's id is hashed from — every field
    /// except nothing is omitted, so two transactions that differ in any
    /// way (including just the signature) get different ids.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut buf = Self::signing_preimage(
            &self.username,
            &self.owner_public_key,
            self.claimed_at_height_hint,
            &self.anchor_block_hash,
            &self.nonce,
        );
        buf.extend_from_slice(&self.signature);
        buf
    }

    fn signature_array(&self) -> Result<[u8; 64]> {
        self.signature
            .as_slice()
            .try_into()
            .map_err(|_| LedgerError::SignatureInvalid)
    }

    pub fn id(&self) -> Hash32 {
        Hash32::of(&self.canonical_bytes())
    }

    /// Checks everything about this transaction that doesn't require
    /// chain context: the username is a normalized, valid format, and the
    /// signature actually matches `owner_public_key` over these exact
    /// fields. Does **not** check that `anchor_block_hash` is a real
    /// ancestor, or that `username` isn't already claimed — both need the
    /// including block's position in the chain, see `validation.rs`.
    pub fn verify_self_contained(&self) -> Result<()> {
        validate_username_format(&self.username)?;
        if self.username != normalize_username(&self.username) {
            return Err(LedgerError::InvalidUsername("not already normalized"));
        }
        let public_key = IdentityPublicKey::from_bytes(&self.owner_public_key)
            .map_err(|_| LedgerError::SignatureInvalid)?;
        let preimage = Self::signing_preimage(
            &self.username,
            &self.owner_public_key,
            self.claimed_at_height_hint,
            &self.anchor_block_hash,
            &self.nonce,
        );
        let signature = self.signature_array()?;
        public_key
            .verify(&preimage, &signature)
            .map_err(|_| LedgerError::SignatureInvalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    fn test_identity(seed: u64) -> IdentityKeyPair {
        IdentityKeyPair::generate(&mut ChaCha20Rng::seed_from_u64(seed))
    }

    #[test]
    fn normalize_lowercases_and_trims() {
        assert_eq!(normalize_username("  Alice  "), "alice");
    }

    #[test]
    fn validates_length_bounds() {
        assert!(validate_username_format("abcd").is_err()); // 4 chars, too short
        assert!(validate_username_format("abcde").is_ok()); // 5 chars, ok
        assert!(validate_username_format(&"a".repeat(32)).is_ok());
        assert!(validate_username_format(&"a".repeat(33)).is_err());
    }

    #[test]
    fn rejects_disallowed_characters() {
        assert!(validate_username_format("alice!").is_err());
        assert!(validate_username_format("alice bob").is_err());
        assert!(validate_username_format("Alice").is_err()); // must already be lowercase
        assert!(validate_username_format("alice_bob_1").is_ok());
    }

    #[test]
    fn a_freshly_built_claim_verifies() {
        let identity = test_identity(1);
        let tx = Transaction::new_claim(&identity, "Alice", 0, Hash32::ZERO, [7; 8]).unwrap();
        assert_eq!(tx.username, "alice"); // normalized on the way in
        tx.verify_self_contained().unwrap();
    }

    #[test]
    fn rejects_an_invalid_username_before_signing() {
        let identity = test_identity(1);
        assert!(Transaction::new_claim(&identity, "no", 0, Hash32::ZERO, [0; 8]).is_err());
    }

    #[test]
    fn rejects_a_tampered_username() {
        let identity = test_identity(1);
        let mut tx = Transaction::new_claim(&identity, "alice", 0, Hash32::ZERO, [0; 8]).unwrap();
        tx.username = "mallory".to_string();
        assert_eq!(tx.verify_self_contained().unwrap_err(), LedgerError::SignatureInvalid);
    }

    #[test]
    fn rejects_a_claim_from_a_different_signer() {
        let alice = test_identity(1);
        let bob = test_identity(2);
        let mut tx = Transaction::new_claim(&alice, "alice", 0, Hash32::ZERO, [0; 8]).unwrap();
        tx.owner_public_key = bob.public_key().to_bytes();
        assert_eq!(tx.verify_self_contained().unwrap_err(), LedgerError::SignatureInvalid);
    }

    #[test]
    fn rejects_a_claim_replayed_under_a_different_anchor() {
        let identity = test_identity(1);
        let mut tx = Transaction::new_claim(&identity, "alice", 0, Hash32::ZERO, [0; 8]).unwrap();
        tx.anchor_block_hash = Hash32::of(b"a different fork's block");
        assert_eq!(tx.verify_self_contained().unwrap_err(), LedgerError::SignatureInvalid);
    }

    #[test]
    fn different_transactions_have_different_ids() {
        let identity = test_identity(1);
        let a = Transaction::new_claim(&identity, "alice", 0, Hash32::ZERO, [0; 8]).unwrap();
        let b = Transaction::new_claim(&identity, "bobby", 0, Hash32::ZERO, [0; 8]).unwrap();
        assert_ne!(a.id(), b.id());
    }

    #[test]
    fn the_same_transaction_has_a_deterministic_id() {
        let identity = test_identity(1);
        let tx = Transaction::new_claim(&identity, "alice", 0, Hash32::ZERO, [0; 8]).unwrap();
        assert_eq!(tx.id(), tx.id());
    }
}
