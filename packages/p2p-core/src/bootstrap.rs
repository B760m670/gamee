//! Entry points into the public IPFS Kademlia DHT — an already-running,
//! shared network operated by Protocol Labs and the wider IPFS/libp2p
//! community, not infrastructure this project runs or controls. Joining it
//! is what lets a peer be found by PeerId alone, without either side
//! needing a fixed address or either project running a relay/rendezvous
//! server of its own.
//!
//! `libp2p_kad::Behaviour::new` defaults to the `/ipfs/kad/1.0.0` protocol
//! (see `libp2p_kad::protocol::DEFAULT_PROTO_NAME`), the same protocol
//! these addresses' nodes already speak — so a node configured with the
//! defaults genuinely joins that DHT, not a private lookalike of it.
//!
//! These four addresses are the standard public bootstrap set published at
//! <https://github.com/ipfs/kubo/blob/master/config/bootstrap_peers.go> and
//! used by default in every stock IPFS/Kubo node.

use libp2p::Multiaddr;

pub fn public_dht_bootstrap_addresses() -> Vec<Multiaddr> {
    [
        "/dnsaddr/bootstrap.libp2p.io/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
        "/dnsaddr/bootstrap.libp2p.io/p2p/QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa",
        "/dnsaddr/bootstrap.libp2p.io/p2p/QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb",
        "/dnsaddr/bootstrap.libp2p.io/p2p/QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt",
    ]
    .iter()
    .map(|addr| addr.parse().expect("hardcoded bootstrap addresses are valid multiaddrs"))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_hardcoded_address_parses() {
        assert_eq!(public_dht_bootstrap_addresses().len(), 4);
    }
}
