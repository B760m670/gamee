//! Publishing/discovering a peer's *current avatar content id* (not the
//! avatar bytes themselves — those stay genuinely content-addressed and
//! are still only ever served live via `SetLocalBlob`/`FetchBlob`, the
//! same as before) in the same public DHT `rendezvous.rs`/`contact_card.rs`
//! already use. Keyed by `PeerId` — mirroring `rendezvous.rs`'s own
//! address records exactly — rather than an identity public key the way
//! `contact_card.rs` is, since every avatar call site in the app already
//! has a `PeerId` on hand (a contact card, by contrast, has to be
//! resolvable from nothing but a raw identity public key, since that's
//! all a brand new, never-yet-contacted peer starts from).
//!
//! An avatar's real bytes are still only ever fetched over a live
//! connection — this DHT record only removes the live-connection
//! requirement from *learning which content id to ask for*, not from
//! fetching the bytes themselves. A peer who's fully offline right now
//! still can't hand over image bytes; this just means a never-yet-seen
//! avatar's current id can be discovered (or a stale cached one
//! refreshed) even while its owner is currently offline — the bytes
//! themselves still arrive only once they're reachable.

use libp2p::kad::RecordKey;
use libp2p::PeerId;

const KEY_PREFIX: &[u8] = b"/spiritchat/avatar-pointer/1/";

pub fn record_key_for(owner: &PeerId) -> RecordKey {
    let mut bytes = KEY_PREFIX.to_vec();
    bytes.extend_from_slice(&owner.to_bytes());
    RecordKey::new(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_keys_are_deterministic() {
        let peer = PeerId::random();
        assert_eq!(record_key_for(&peer), record_key_for(&peer));
    }

    #[test]
    fn record_keys_differ_between_peers() {
        assert_ne!(record_key_for(&PeerId::random()), record_key_for(&PeerId::random()));
    }
}
