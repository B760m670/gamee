//! Storage for message keys the ratchet skipped past because messages
//! arrived out of order. A network can reorder or drop packets, so a
//! recipient must be able to decrypt message N+2 before message N+1
//! shows up — this is where the key for that "not yet arrived" message N+1
//! waits.

use std::collections::HashMap;

use x25519_dalek::PublicKey as X25519PublicKey;

use crate::error::{CryptoError, Result};

use super::chain::MessageKey;

/// Refuses to store more than this many skipped keys at once. Without a
/// cap, a peer (or an attacker who can inject headers) could claim a huge
/// `message_number` and force us to derive and store an unbounded number
/// of keys — a memory-exhaustion denial of service.
const MAX_SKIPPED_KEYS: usize = 2000;

#[derive(Default, Clone)]
pub struct SkippedKeyStore {
    keys: HashMap<([u8; 32], u32), MessageKey>,
}

impl SkippedKeyStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, dh_public: X25519PublicKey, message_number: u32, key: MessageKey) -> Result<()> {
        if self.keys.len() >= MAX_SKIPPED_KEYS {
            return Err(CryptoError::TooManySkippedKeys);
        }
        self.keys
            .insert((dh_public.to_bytes(), message_number), key);
        Ok(())
    }

    /// Removes and returns the key for `(dh_public, message_number)` if we
    /// have it. Removal matters as much as lookup here: a message key must
    /// never be reused once consumed.
    pub fn take(&mut self, dh_public: &X25519PublicKey, message_number: u32) -> Option<MessageKey> {
        self.keys.remove(&(dh_public.to_bytes(), message_number))
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use x25519_dalek::StaticSecret;

    fn dummy_public(seed: u8) -> X25519PublicKey {
        X25519PublicKey::from(&StaticSecret::from([seed; 32]))
    }

    #[test]
    fn stores_and_consumes_a_key_exactly_once() {
        let mut store = SkippedKeyStore::new();
        let public = dummy_public(1);
        store.insert(public, 5, MessageKey([42u8; 32])).unwrap();

        let taken = store.take(&public, 5);
        assert!(taken.is_some());
        assert!(store.take(&public, 5).is_none());
    }

    #[test]
    fn distinguishes_by_both_key_and_message_number() {
        let mut store = SkippedKeyStore::new();
        let a = dummy_public(1);
        let b = dummy_public(2);
        store.insert(a, 1, MessageKey([1u8; 32])).unwrap();
        store.insert(a, 2, MessageKey([2u8; 32])).unwrap();
        store.insert(b, 1, MessageKey([3u8; 32])).unwrap();

        assert_eq!(store.take(&a, 1).unwrap().as_bytes(), &[1u8; 32]);
        assert_eq!(store.take(&a, 2).unwrap().as_bytes(), &[2u8; 32]);
        assert_eq!(store.take(&b, 1).unwrap().as_bytes(), &[3u8; 32]);
    }

    #[test]
    fn refuses_to_grow_past_the_cap() {
        let mut store = SkippedKeyStore::new();
        let public = dummy_public(1);
        for n in 0..MAX_SKIPPED_KEYS as u32 {
            store.insert(public, n, MessageKey([0u8; 32])).unwrap();
        }
        let err = store
            .insert(public, MAX_SKIPPED_KEYS as u32, MessageKey([0u8; 32]))
            .unwrap_err();
        assert_eq!(err, CryptoError::TooManySkippedKeys);
    }
}
