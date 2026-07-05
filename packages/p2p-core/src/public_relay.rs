//! Discovery of *standing* relays — peers whose owners run them on
//! always-on machines specifically so the rest of the network has
//! somewhere to deposit mailbox mail and route mix traffic even when
//! every phone happens to be offline or freshly installed.
//!
//! Exists because the mix-relay directory (`behaviour::
//! mix_relay_directory_topic()`) is gossip, and gossip only reaches peers
//! *already connected* to at least one other SpiritChat node — a brand
//! new install, connected only to the public IPFS DHT's bootstrap nodes,
//! has no SpiritChat peer to hear announcements from, so its relay
//! directory stays empty and every `DepositToMailbox` dies with "no mix
//! relay is currently known and reachable". This module closes that
//! bootstrap gap with the one discovery mechanism that *doesn't* need a
//! prior SpiritChat connection: a Kademlia **provider record** under a
//! single well-known key, in the same public DHT every other record this
//! crate publishes already lives in. Any node can announce itself
//! (`Command::AnnouncePublicRelay`); any node can enumerate announcers
//! (`Command::DiscoverPublicRelays`) and dial them, at which point
//! ordinary gossip (the mix-relay directory, the ledger, everything else)
//! takes over exactly as if the two had met on a shared LAN.
//!
//! Deliberately still not "a server": a standing relay runs the exact
//! same code path as every opted-in phone (`Command::AnnounceMixRelay`,
//! passive mix forwarding, the mailbox cache), sees only Sphinx layers
//! and opaque tags like any other hop, and nothing in this crate treats
//! it as more trusted, more authoritative, or load-bearing for anything
//! but availability. Anyone can run one; the network uses however many
//! exist — same "strengthens as the network grows" honesty as the mix
//! relay directory itself.

use libp2p::kad::RecordKey;

/// The one well-known provider key every standing relay announces under
/// and every node discovers by — fixed, like `behaviour`'s own protocol
/// names, and versioned the same way so a future incompatible scheme can
/// coexist during a migration.
const PUBLIC_RELAY_PROVIDER_KEY: &[u8] = b"/spiritchat/public-relay/1";

pub fn provider_key() -> RecordKey {
    RecordKey::new(&PUBLIC_RELAY_PROVIDER_KEY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_provider_key_is_stable() {
        // Announcers and discoverers only ever meet through this exact
        // key — a change to it is a network-wide compatibility break and
        // must never happen by accident.
        assert_eq!(provider_key().to_vec(), b"/spiritchat/public-relay/1".to_vec());
    }
}
