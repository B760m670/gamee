use libp2p::{Multiaddr, PeerId};
use spiritchat_ledger_core::{Block, Transaction};

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

    /// Sends an already-built Sphinx packet (`mix::build_packet`) to
    /// `first_hop`, the first node in whatever path the caller chose.
    /// Every hop after that is handled automatically by this crate's own
    /// event loop, peeling and re-forwarding as `MixPacket` requests arrive
    /// — the caller never talks to intermediate hops directly, only
    /// receives `P2pEvent::MixPacketArrived` if and when this node itself
    /// ends up being a path's final hop. Requires an existing connection
    /// to `first_hop`, same as `SendEnvelope`.
    SendMixPacket { first_hop: PeerId, packet_bytes: Vec<u8> },

    /// Broadcasts this node's own Sphinx routing public key to
    /// `behaviour::mix_relay_directory_topic()`, so other nodes can
    /// discover it as a usable mix hop even before ever directly
    /// exchanging mix traffic with it. Purely a discovery signal — a
    /// sender still needs to `Dial` a chosen relay for the first hop of
    /// any path, and intermediate-hop forwarding still resolves through
    /// each relay's own connectivity-based bookkeeping, not this
    /// announcement. The app layer decides when/whether to call this at
    /// all (mix-relay participation is opt-in).
    AnnounceMixRelay,

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
    ///
    /// Deprecated: this is the DHT-based best-effort claim system (see
    /// `username.rs`'s own doc comment for exactly what it can't
    /// guarantee). It's being replaced by the `@username` ledger below
    /// (`SubmitUsernameClaim`/`QueryUsernameOwner`), which gives a real
    /// first-claim-wins guarantee instead of an advisory one — kept for
    /// now only so the already-shipped DHT-based feature keeps working
    /// until the app layer cuts over to the ledger.
    ResolveUsername { username: String },

    /// Broadcasts an already-signed `@username` claim to the ledger's
    /// mempool topic, so *any* connected peer's miner (not just this
    /// node's own, if it mines at all) can pick it up and include it in a
    /// block. This alone does not reserve or confirm the name — it only
    /// gets the claim in front of the network; watch for
    /// `P2pEvent::ChainTipChanged` to see whether/when it actually lands.
    SubmitUsernameClaim { transaction: Transaction },

    /// Submits an already-mined, already-valid `@username` ledger block —
    /// validates and applies it locally exactly like one received over
    /// gossip, then gossips it onward. This is the primitive both the
    /// real mining loop (Phase 5) and multi-node tests use; it does not
    /// mine anything itself.
    SubmitMinedBlock { block: Block },

    /// Answers from this node's own local materialized ledger state only
    /// — no network round trip, since once synced this node's view of the
    /// chain *is* the answer, not something to ask a peer for on every
    /// lookup. Answered by
    /// `P2pEvent::UsernameOwnerResolved`/`UsernameOwnerNotFound`.
    QueryUsernameOwner { username: String },

    /// Asks `peer` for its current chain tip and, if it's heavier than
    /// this node's own, fetches and applies whatever blocks are missing.
    /// Answered by `P2pEvent::ChainSyncCompleted`/`ChainSyncFailed`. Dial
    /// first if not already connected.
    RequestChainSync { peer: PeerId },

    /// This node's own current ledger chain tip — for building a new
    /// claim's `anchor_block_hash`/`anchor_height` (see
    /// `spiritchat_ledger_core::Transaction::new_claim`), since a claim
    /// must anchor to a real, recent block. Answered synchronously (no
    /// network round trip) by `P2pEvent::ChainTipChanged`, the same event
    /// a real tip change fires — querying and changing produce the same
    /// shape of answer either way.
    QueryChainTip,

    /// Starts (or restarts, if already mining) this node's mining loop,
    /// attributing any block it successfully mines to `public_key`. Mining
    /// runs continuously — one attempt at a time, restarted with a fresh
    /// candidate whenever the tip changes (locally mined, gossiped, or
    /// synced) or a prior attempt finishes — until `StopMining`. The app
    /// layer is responsible for deciding *when* this should be active
    /// (e.g. only in the foreground while charging); this crate just does
    /// what it's told. Successful blocks surface as
    /// `P2pEvent::NewBlockMined`, same as any other new tip.
    StartMining { public_key: [u8; 32] },

    /// Stops the mining loop started by `StartMining`. A no-op if not
    /// currently mining.
    StopMining,

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
