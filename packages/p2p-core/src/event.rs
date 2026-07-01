use libp2p::{Multiaddr, PeerId};

/// What the rest of the app learns about, translated from raw libp2p swarm
/// events into the handful of things a messenger actually cares about.
#[derive(Debug, Clone)]
pub enum P2pEvent {
    /// This node is now listening for inbound connections on `address`
    /// (a direct address, or one reachable through a peer-run circuit
    /// relay after `Command::ReserveRelaySlot`).
    ListeningOn(Multiaddr),

    /// Found `peer` on the local network (mDNS) — safe to dial
    /// immediately, no DHT lookup needed.
    PeerDiscoveredLocally(PeerId),

    /// A connection to `peer` is up; envelopes can be sent.
    PeerConnected(PeerId),

    /// The identify handshake with `peer` completed and its address was
    /// recorded in this node's Kademlia routing table. Being connected to
    /// a peer (`PeerConnected`) does not by itself mean the DHT knows how
    /// to reach them — this is the separate, slightly later signal that a
    /// `put_record`/`get_record` involving `peer` now has someone to ask.
    PeerIdentified(PeerId),

    /// The connection to `peer` dropped.
    PeerDisconnected(PeerId),

    /// `Command::Dial` did not result in a connection — `peer` is `None`
    /// when the failure was rejected before libp2p even knew who it was
    /// trying to reach (e.g. an empty address list).
    DialFailed { peer: Option<PeerId>, reason: String },

    /// `Command::ReserveRelaySlot` was rejected immediately (e.g. a
    /// malformed relay address). A relay that accepts the reservation but
    /// later drops it is not yet surfaced as an event by this crate.
    RelayReservationFailed { reason: String },

    /// An encrypted envelope arrived from `from`. This layer does not
    /// decrypt it — `bytes` is exactly what `spiritchat_crypto_core`
    /// produced on the sending side (an X3DH initial message or a Double
    /// Ratchet ciphertext envelope).
    EnvelopeReceived { from: PeerId, bytes: Vec<u8> },

    /// `Command::SendEnvelope` to `to` was acknowledged by the transport
    /// (delivered to the peer's envelope handler) or failed outright. This
    /// is transport-level delivery, not read receipts — those are an
    /// application-layer concern above this crate.
    EnvelopeDelivered { to: PeerId },
    EnvelopeDeliveryFailed { to: PeerId, reason: String },

    /// A DHT lookup for a peer's advertised addresses (see `rendezvous`)
    /// finished, with or without a result.
    PeerAddressesResolved { peer: PeerId, addresses: Vec<Multiaddr> },
    PeerAddressResolutionFailed { peer: PeerId },

    /// `Command::AnnounceAddresses` finished publishing to the DHT (or
    /// failed to reach the required quorum). Other peers'
    /// `ResolvePeerAddresses` lookups only have something to find once
    /// this has happened at least once.
    AddressesAnnounced,
    AddressAnnouncementFailed { reason: String },
}
