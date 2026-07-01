//! The combined set of libp2p protocols a SpiritChat node speaks. Kept as
//! its own module since it's purely wiring — what each protocol *does* is
//! implemented by the libp2p crates themselves; `node.rs` is where this
//! project's own logic (reacting to their events) lives.

use std::time::Duration;

use libp2p::{
    dcutr, identify, kad, mdns, relay,
    request_response::{self, ProtocolSupport},
    swarm::NetworkBehaviour,
    PeerId, StreamProtocol,
};

/// The wire protocol for delivering an already-encrypted message envelope
/// (an X3DH initial message or a Double Ratchet ciphertext, produced by
/// `spiritchat_crypto_core`) from one connected peer to another. This layer
/// never sees plaintext — it only moves opaque bytes.
pub const ENVELOPE_PROTOCOL: StreamProtocol = StreamProtocol::new("/spiritchat/envelope/1.0.0");

/// The identify protocol's advertised name — lets peers recognize a
/// SpiritChat node as such, purely informational (no protocol negotiation
/// depends on it; `ENVELOPE_PROTOCOL` support is what actually matters).
pub const IDENTIFY_PROTOCOL_VERSION: &str = "/spiritchat/1.0.0";

#[derive(NetworkBehaviour)]
pub struct Behaviour {
    pub kad: kad::Behaviour<kad::store::MemoryStore>,
    pub mdns: mdns::tokio::Behaviour,
    pub identify: identify::Behaviour,
    pub relay_client: relay::client::Behaviour,
    pub dcutr: dcutr::Behaviour,
    pub envelope: request_response::cbor::Behaviour<Vec<u8>, Vec<u8>>,
}

pub fn build(
    key: &libp2p::identity::Keypair,
    relay_client: relay::client::Behaviour,
) -> Result<Behaviour, Box<dyn std::error::Error + Send + Sync>> {
    let peer_id = PeerId::from(key.public());

    // libp2p-kad defaults to Mode::Client, only promoting itself to
    // Mode::Server once it's independently confirmed its own external
    // reachability (e.g. via AutoNAT) — a reasonable default for a node
    // that mostly just wants DHT *lookups*. A Client-mode node does not
    // serve inbound Kademlia requests, including put_record — meaning
    // nobody could ever store a rendezvous record with it. Since every
    // peer here is relied on to store other peers' address records (there
    // is no dedicated server pulling that weight), every node must run in
    // Server mode explicitly, not wait to earn it.
    let mut kad = kad::Behaviour::new(peer_id, kad::store::MemoryStore::new(peer_id));
    kad.set_mode(Some(kad::Mode::Server));

    Ok(Behaviour {
        kad,
        mdns: mdns::tokio::Behaviour::new(mdns::Config::default(), peer_id)?,
        identify: identify::Behaviour::new(identify::Config::new(
            IDENTIFY_PROTOCOL_VERSION.to_string(),
            key.public(),
        )),
        relay_client,
        dcutr: dcutr::Behaviour::new(peer_id),
        envelope: request_response::cbor::Behaviour::new(
            [(ENVELOPE_PROTOCOL, ProtocolSupport::Full)],
            request_response::Config::default().with_request_timeout(Duration::from_secs(30)),
        ),
    })
}
