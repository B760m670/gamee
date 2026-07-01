use libp2p::{Multiaddr, PeerId};

/// What the app asks this crate's event loop to do. Sent over a channel
/// rather than called directly, since the `Swarm` only exists inside the
/// background task driving it (see `node.rs`).
#[derive(Debug)]
pub enum Command {
    /// Connect to `peer`. If `known_addresses` is empty, dials via
    /// whatever addresses the DHT/identify/mDNS have already learned for
    /// `peer` — call `ResolvePeerAddresses` first if none are known yet.
    Dial { peer: PeerId, known_addresses: Vec<Multiaddr> },

    /// Look up `peer`'s currently-advertised addresses in the public DHT
    /// (see `rendezvous.rs`). Answered with
    /// `P2pEvent::PeerAddressesResolved`/`PeerAddressResolutionFailed`.
    ResolvePeerAddresses { peer: PeerId },

    /// Publish this node's own current addresses to the DHT under its own
    /// PeerId, so `ResolvePeerAddresses` from another peer can find them.
    /// Re-run periodically (DHT records expire) and after the address set
    /// changes (e.g. a new relay reservation).
    AnnounceAddresses { addresses: Vec<Multiaddr> },

    /// Send an already-encrypted envelope to a connected peer. Queue a
    /// `Dial` first if not yet connected — this does not implicitly dial.
    SendEnvelope { to: PeerId, bytes: Vec<u8> },

    /// Ask a relay-capable peer, reachable at `relay_address`, to reserve
    /// a slot so this node can be dialed through it while behind NAT. That
    /// peer is just another participant on the network that opted in to
    /// relaying traffic for others — not infrastructure this project runs.
    /// Success shows up as `P2pEvent::ListeningOn` with a `/p2p-circuit`
    /// address; publish it via `AnnounceAddresses` so others can reach it.
    ReserveRelaySlot { relay_address: Multiaddr },
}
