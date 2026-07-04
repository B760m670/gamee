//! DHT record keys for supplementary replication of mailbox deposits —
//! see `mailbox.rs`'s own module doc comment for the tag/deposit scheme
//! this augments. A deposit's real, sole record of truth is always
//! whichever relay `mix.rs`'s deterministic final-hop routing landed it
//! on (`MailboxStore::accept`, untouched by this module); what's added
//! here is that the same relay *also* replicates the envelope into the
//! public DHT (`kad::Behaviour`, the same one `rendezvous.rs`/
//! `contact_card.rs`/`avatar_pointer.rs`/`username.rs` already use) under
//! one of a small, fixed number of per-tag slots — so a recipient whose
//! mix-routed retrieval query can't currently reach that one relay (it's
//! temporarily offline, or briefly unreachable from this node) can still
//! recover a queued message from whichever of the DHT's own replica nodes
//! are holding it, the same "closest nodes to a key" replication Kademlia
//! already does for every other record this crate publishes.
//!
//! **Honestly bounded, not a full fix**: a fixed number of slots means
//! only that many concurrently-queued deposits per tag get this extra
//! redundancy at once — a busier mailbox tag's overflow still relies
//! solely on the one deterministic relay's own local cache, exactly as
//! before this existed. Two different deposits landing in the same slot
//! (a `1 in MAILBOX_DHT_SLOTS` coincidence) simply overwrite each other's
//! DHT replica — this node's own local `MailboxStore` is unaffected
//! either way, so no deposit is ever lost, only its *extra* DHT
//! redundancy.

use libp2p::kad::RecordKey;

use crate::mailbox::MAILBOX_TAG_LEN;

/// How many independent per-tag DHT replication slots exist — see this
/// module's own doc comment for what happens beyond this many concurrent
/// deposits under one tag.
pub const MAILBOX_DHT_SLOTS: u8 = 4;

const KEY_PREFIX: &[u8] = b"/spiritchat/mailbox-dht-slot/1/";

/// The DHT record key for `tag`'s `slot`-th replication slot
/// (`0..MAILBOX_DHT_SLOTS`). Deterministic and stateless: any node that
/// knows `tag` can compute every one of its slot keys independently,
/// without ever coordinating with whoever originally stored anything
/// there.
pub fn record_key_for(tag: &[u8; MAILBOX_TAG_LEN], slot: u8) -> RecordKey {
    let mut bytes = KEY_PREFIX.to_vec();
    bytes.extend_from_slice(tag);
    bytes.push(slot);
    RecordKey::new(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_keys_are_deterministic() {
        let tag = [1u8; MAILBOX_TAG_LEN];
        assert_eq!(record_key_for(&tag, 0), record_key_for(&tag, 0));
    }

    #[test]
    fn record_keys_differ_between_slots() {
        let tag = [1u8; MAILBOX_TAG_LEN];
        assert_ne!(record_key_for(&tag, 0), record_key_for(&tag, 1));
    }

    #[test]
    fn record_keys_differ_between_tags() {
        assert_ne!(record_key_for(&[1u8; MAILBOX_TAG_LEN], 0), record_key_for(&[2u8; MAILBOX_TAG_LEN], 0));
    }
}
