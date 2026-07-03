//! How a peer is found by PeerId alone, with no fixed address: each node
//! periodically publishes its own current addresses into the public DHT
//! (`kad::put_record`) under a key derived from its own PeerId; anyone who
//! wants to reach it looks that key up (`kad::get_record`). This is the
//! piece that makes "here's a contact card with your identity key" enough
//! to eventually connect, without either side needing a static address —
//! addresses change constantly on mobile (new IP on every network switch),
//! so nothing tied to one is embedded in the contact card itself.

use libp2p::kad::RecordKey;
use libp2p::{Multiaddr, PeerId};

/// Namespaces this project's use of the shared public DHT so a record key
/// can never collide with another application's unrelated use of the same
/// key space (e.g. IPFS content-addressing, which also stores records in
/// this DHT).
const KEY_PREFIX: &[u8] = b"/spiritchat/addrs/1/";

pub fn record_key_for(peer: &PeerId) -> RecordKey {
    let mut bytes = KEY_PREFIX.to_vec();
    bytes.extend_from_slice(&peer.to_bytes());
    RecordKey::new(&bytes)
}

/// Multiaddr text representations never contain `\n`, so a simple
/// newline-joined list is enough — no need for a serialization crate for
/// something this small.
pub fn encode_addresses(addresses: &[Multiaddr]) -> Vec<u8> {
    addresses
        .iter()
        .map(Multiaddr::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes()
}

pub fn decode_addresses(bytes: &[u8]) -> Vec<Multiaddr> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Vec::new();
    };
    text.lines().filter_map(|line| line.parse().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_keys_differ_between_peers() {
        let a = PeerId::random();
        let b = PeerId::random();
        assert_ne!(record_key_for(&a), record_key_for(&b));
    }

    #[test]
    fn record_keys_are_deterministic() {
        let peer = PeerId::random();
        assert_eq!(record_key_for(&peer), record_key_for(&peer));
    }

    #[test]
    fn addresses_round_trip_through_encoding() {
        let addresses: Vec<Multiaddr> = vec![
            "/ip4/127.0.0.1/tcp/4001".parse().unwrap(),
            "/ip4/203.0.113.5/udp/4001/quic-v1".parse().unwrap(),
        ];
        let encoded = encode_addresses(&addresses);
        let decoded = decode_addresses(&encoded);
        assert_eq!(decoded, addresses);
    }

    #[test]
    fn decoding_garbage_yields_no_addresses_instead_of_panicking() {
        assert_eq!(decode_addresses(b"not a multiaddr\n\xff\xfe"), Vec::<Multiaddr>::new());
    }

    #[test]
    fn decoding_empty_bytes_yields_an_empty_list() {
        assert_eq!(decode_addresses(b""), Vec::<Multiaddr>::new());
    }
}
