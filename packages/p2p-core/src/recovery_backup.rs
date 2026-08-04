//! Publishing/retrieving an account's *encrypted recovery backup* in the
//! same public DHT `contact_card.rs` already uses — keyed by the owner's
//! long-term identity public key, since a fresh install restoring from a
//! recovery phrase has exactly that (derived from the phrase) and nothing
//! else. The record's contents are opaque ciphertext to this crate and to
//! every node that stores it: encryption/decryption live in
//! `spiritchat_crypto_core::backup`, under a key only the phrase holder
//! can derive. This is what turns "restore" from "same keys, empty
//! account" into "same keys, same account" — profile and contacts come
//! back from the network, not from any server.

use libp2p::kad::RecordKey;

const KEY_PREFIX: &[u8] = b"/spiritchat/recovery-backup/1/";

pub fn record_key_for(owner_identity_public_key: &[u8]) -> RecordKey {
    let mut bytes = KEY_PREFIX.to_vec();
    bytes.extend_from_slice(owner_identity_public_key);
    RecordKey::new(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_keys_are_deterministic() {
        let key = [7u8; 32];
        assert_eq!(record_key_for(&key), record_key_for(&key));
    }

    #[test]
    fn record_keys_differ_between_owners() {
        assert_ne!(record_key_for(&[1u8; 32]), record_key_for(&[2u8; 32]));
    }

    #[test]
    fn record_keys_do_not_collide_with_contact_cards() {
        // Same identity key, two different record kinds — the prefixes are
        // what keep them apart in the shared DHT namespace.
        let key = [7u8; 32];
        assert_ne!(record_key_for(&key), crate::contact_card::record_key_for(&key));
    }
}
