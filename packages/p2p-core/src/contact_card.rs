//! Publishing/discovering a contact card (prekey bundle) in the same
//! public DHT `rendezvous.rs` already uses for address records — keyed by
//! the owner's own long-term identity public key, not their `PeerId`
//! (which a contact wouldn't have yet either, before ever exchanging
//! anything with them — an identity key from a QR code or a ledger
//! username lookup is all a first contact ever starts from).
//!
//! This is what makes a *first* message to someone currently offline
//! possible at all: `SetLocalBlob`/`FetchBlob` (the normal way a contact
//! card is served) needs a *live connection* to the peer serving it — a
//! DHT record doesn't. Once published, it survives its publisher going
//! offline for as long as the handful of nodes Kademlia replicated it to
//! keep it (and, per this crate's shared `kad::Behaviour` config, keep
//! re-replicating it among themselves) — the same reason `AnnounceUsername`
//! claims and `AnnounceAddresses` records already outlive any one
//! publishing session.

use libp2p::kad::RecordKey;

const KEY_PREFIX: &[u8] = b"/spiritchat/contact-card/1/";

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
}
