//! Storage for message keys a [`super::state::SenderKeyReceiverState`]
//! skipped past because group messages arrived out of order — the same
//! problem [`crate::ratchet::skipped_keys::SkippedKeyStore`] solves for
//! the Double Ratchet, simplified down to one dimension: a Sender Key
//! chain never re-keys mid-stream (no DH ratchet step to also key by), so
//! `iteration` alone identifies a skipped entry.

use std::collections::HashMap;

use crate::error::{CryptoError, Result};
use crate::ratchet::chain::MessageKey;

/// Refuses to store more than this many skipped keys at once, for the
/// same reason [`crate::ratchet::skipped_keys::SkippedKeyStore`] caps
/// itself: an attacker (or a group member) naming a huge `iteration`
/// must not be able to force unbounded key derivation and storage.
const MAX_SKIPPED_ITERATIONS: usize = 2000;

#[derive(Default, Clone)]
pub struct SkippedIterations {
    keys: HashMap<u32, MessageKey>,
}

impl SkippedIterations {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, iteration: u32, key: MessageKey) -> Result<()> {
        if self.keys.len() >= MAX_SKIPPED_ITERATIONS {
            return Err(CryptoError::TooManySkippedKeys);
        }
        self.keys.insert(iteration, key);
        Ok(())
    }

    /// Removes and returns the key for `iteration` if we have it — removed
    /// as much as looked up: a message key must never be reused once
    /// consumed.
    pub fn take(&mut self, iteration: u32) -> Option<MessageKey> {
        self.keys.remove(&iteration)
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// All entries as owned tuples, for serializing alongside the rest of
    /// a [`super::state::SenderKeyReceiverState`].
    pub fn entries(&self) -> Vec<(u32, MessageKey)> {
        self.keys.iter().map(|(&iteration, key)| (iteration, key.clone())).collect()
    }

    /// Rebuilds a store from entries produced by [`Self::entries`]. Does
    /// not re-check the cap against the input length — a store that was
    /// valid when serialized stays valid on the way back in.
    pub fn from_entries(entries: Vec<(u32, MessageKey)>) -> Self {
        Self { keys: entries.into_iter().collect() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_consumes_a_key_exactly_once() {
        let mut store = SkippedIterations::new();
        store.insert(5, MessageKey([42u8; 32])).unwrap();

        assert!(store.take(5).is_some());
        assert!(store.take(5).is_none());
    }

    #[test]
    fn distinguishes_by_iteration() {
        let mut store = SkippedIterations::new();
        store.insert(1, MessageKey([1u8; 32])).unwrap();
        store.insert(2, MessageKey([2u8; 32])).unwrap();

        assert_eq!(store.take(1).unwrap().as_bytes(), &[1u8; 32]);
        assert_eq!(store.take(2).unwrap().as_bytes(), &[2u8; 32]);
    }

    #[test]
    fn refuses_to_grow_past_the_cap() {
        let mut store = SkippedIterations::new();
        for n in 0..MAX_SKIPPED_ITERATIONS as u32 {
            store.insert(n, MessageKey([0u8; 32])).unwrap();
        }
        let err = store.insert(MAX_SKIPPED_ITERATIONS as u32, MessageKey([0u8; 32])).unwrap_err();
        assert_eq!(err, CryptoError::TooManySkippedKeys);
    }

    #[test]
    fn round_trips_through_entries() {
        let mut store = SkippedIterations::new();
        store.insert(1, MessageKey([1u8; 32])).unwrap();
        store.insert(2, MessageKey([2u8; 32])).unwrap();

        let mut restored = SkippedIterations::from_entries(store.entries());
        assert_eq!(restored.len(), 2);
        assert_eq!(restored.take(1).unwrap().as_bytes(), &[1u8; 32]);
        assert_eq!(restored.take(2).unwrap().as_bytes(), &[2u8; 32]);
    }
}
