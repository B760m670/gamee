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

    /// Registers `bytes` as a blob this node will serve to any peer that
    /// asks for it by `id` (e.g. this device's own current avatar,
    /// content-addressed by its own hash) — until `ClearLocalBlob` removes
    /// it, it's replaced by another `SetLocalBlob` with the same `id`, or
    /// this node restarts (nothing is persisted by this crate). Peers fetch
    /// it directly over a connection to this node; nothing is uploaded
    /// anywhere in advance.
    SetLocalBlob { id: Vec<u8>, bytes: Vec<u8> },

    /// Stops serving the blob registered under `id`.
    ClearLocalBlob { id: Vec<u8> },

    /// Requests the blob `id` from `peer`, who must have it registered via
    /// `SetLocalBlob` (or be a third party choosing to cache and re-serve a
    /// copy — an app-layer policy, not something this crate arranges).
    /// Dial first if not already connected. Answered by
    /// `P2pEvent::BlobFetched`/`BlobFetchFailed`.
    FetchBlob { peer: PeerId, id: Vec<u8> },

    /// Publishes `claim` under the DHT key derived from `username` (see
    /// `username::record_key_for`). `claim` is opaque to this crate — the
    /// app layer is responsible for making it self-certifying (e.g. a
    /// public key plus a signature over the username, so nobody but the
    /// key's owner can publish a claim for it), since this crate has no
    /// cryptographic verification of its own. A DHT alone cannot arbitrate
    /// *who claimed a name first* the way a blockchain or a server could —
    /// publishing here does not reserve the name against a determined
    /// second claimant, only against accidental or casual collisions.
    /// Callers that care should `ResolveUsername` first and treat a
    /// different existing claim as "taken".
    AnnounceUsername { username: String, claim: Vec<u8> },

    /// Looks up whatever claim is currently published for `username`.
    /// Answered by `P2pEvent::UsernameResolved`/`UsernameResolutionFailed`.
    ResolveUsername { username: String },

    /// Cleanly stops the event loop — after this, `next_event` returns
    /// `None` and the node can no longer be used; a new identity needs a
    /// new `P2pNode::spawn`, not a reused one. For an app-level "sign out":
    /// dropping every `P2pNode`/`FfiP2pNode` reference alone doesn't stop a
    /// node whose event loop is still being actively polled by something
    /// (e.g. a background task awaiting `next_event`) — that task holds its
    /// own reference alive for as long as it keeps looping. This command
    /// breaks that loop explicitly instead of relying on reference counting
    /// to happen to reach zero.
    Shutdown,
}
