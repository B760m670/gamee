//! The combined set of libp2p protocols a SpiritChat node speaks. Kept as
//! its own module since it's purely wiring — what each protocol *does* is
//! implemented by the libp2p crates themselves; `node.rs` is where this
//! project's own logic (reacting to their events) lives.

use std::time::Duration;

use libp2p::{
    connection_limits, dcutr, gossipsub, identify, kad, mdns, relay,
    request_response::{self, ProtocolSupport},
    swarm::{behaviour::toggle::Toggle, NetworkBehaviour},
    PeerId, StreamProtocol,
};
use serde::{Deserialize, Serialize};

use crate::ledger::{self, ChainSyncRequest, ChainSyncResponse};

/// The wire protocol for delivering an already-encrypted message envelope
/// (an X3DH initial message or a Double Ratchet ciphertext, produced by
/// `spiritchat_crypto_core`) from one connected peer to another. This layer
/// never sees plaintext — it only moves opaque bytes.
pub const ENVELOPE_PROTOCOL: StreamProtocol = StreamProtocol::new("/spiritchat/envelope/1.0.0");

/// The identify protocol's advertised name — lets peers recognize a
/// SpiritChat node as such, purely informational (no protocol negotiation
/// depends on it; `ENVELOPE_PROTOCOL` support is what actually matters).
pub const IDENTIFY_PROTOCOL_VERSION: &str = "/spiritchat/1.0.0";

/// This project's own Kademlia, which is where every record this network
/// stores actually lives.
///
/// It would be simpler to keep using the public IPFS DHT for records too,
/// and that is what this did at first. It cannot work, for a reason that
/// has nothing to do with whether the IPFS nodes accept the writes.
///
/// Kademlia does not store a record "somewhere". It stores it on the
/// nodes whose ids are numerically closest to the record's key, and
/// answers a lookup by walking toward those same nodes. Closeness is
/// measured against everyone in the routing table — and in the public
/// DHT, that is on the order of twenty thousand IPFS nodes against a
/// handful of ours. The chance that one of ours lands among the closest
/// peers for any given key rounds to zero. So every record this network
/// wrote would be handed to strangers who have no interest in keeping
/// it, and every read would depend on those strangers having kept it.
///
/// The public DHT is not broken for this. It is simply somebody else's
/// store, sized for somebody else's keys.
///
/// A separate protocol name is the whole mechanism: a peer that does not
/// speak it never enters this routing table, so "closest" is measured
/// only among nodes that run this software and want these records. It
/// also takes this network's keys out of a public, crawlable index,
/// which is worth as much as the functional fix — mailbox tags and
/// rendezvous keys in the public DHT are metadata anyone may harvest.
pub const PRIVATE_DHT_PROTOCOL: StreamProtocol = StreamProtocol::new("/spiritchat/kad/1.0.0");

/// The wire protocol for fetching a content-addressed blob (e.g. an avatar
/// image) directly from the peer that registered it via
/// `Command::SetLocalBlob`. There is nowhere else these bytes live — no
/// pinning service, no CDN, no relay that stores a copy on this project's
/// behalf. A peer that has fetched a blob may choose to cache and re-serve
/// it (that's an app-layer policy, not something this crate does), which is
/// how availability improves over "only the owner has it" without any
/// party being a dedicated host.
pub const BLOB_PROTOCOL: StreamProtocol = StreamProtocol::new("/spiritchat/blob/1.0.0");

/// The wire protocol for carrying a raw Sphinx packet (see `mix.rs`) one
/// hop closer to its destination. This behaviour only ever moves opaque
/// bytes — a relay peels its own routing layer (in `node.rs`, not here)
/// and re-sends the result over this same protocol to whichever peer that
/// layer named as the next hop, so from this protocol's own point of view
/// every hop looks identical: bytes in, an empty ack out, and (sometimes)
/// bytes back out to someone else.
pub const MIX_PROTOCOL: StreamProtocol = StreamProtocol::new("/spiritchat/mix/1.0.0");

/// What travels over `MIX_PROTOCOL`. `sender_routing_public_key` rides
/// alongside the opaque `packet_bytes` in the clear — this is how a node
/// learns another peer's Sphinx routing public key at all, since nothing
/// about a `PeerId` reveals it. This is not a secret being leaked: which
/// nodes act as mix relays and what their routing keys are is meant to be
/// public information in a Loopix-style design (the same way a Tor relay's
/// own identity is public) — what stays hidden is which path any specific
/// packet took, never that a given node participates in the mix at all.
/// Whoever sends this is, from the receiving hop's perspective, simply
/// "the peer this arrived from" — which may be the original sender or any
/// earlier relay re-wrapping a forwarded hop under its own key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixMessage {
    pub packet_bytes: Vec<u8>,
    pub sender_routing_public_key: [u8; 32],
}

/// Where a mix-relay-enabled node periodically announces its own Sphinx
/// routing public key (`Command::AnnounceMixRelay`) — gossip, not the DHT,
/// since the point is *discovery* (finding some usable relays at all)
/// rather than looking up one already-known key: a flooded broadcast
/// naturally reaches every currently-online subscriber, which a targeted
/// `get_record` can't do without already knowing what key to ask for.
/// Publishing this is not a secret being leaked — see `MixMessage`'s own
/// doc comment on why a mix node's willingness to relay and its routing
/// key are meant to be public.
pub fn mix_relay_directory_topic() -> gossipsub::IdentTopic {
    gossipsub::IdentTopic::new("/spiritchat/mix/relays/1")
}

/// What's published on `mix_relay_directory_topic()` — nothing but the
/// publishing peer's own routing public key; gossipsub's own signed
/// `Message::source` is the peer identity, so there's no separate PeerId
/// field here to include (or for an impostor to forge).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixRelayAnnouncement {
    pub routing_public_key: [u8; 32],
}

/// The response half of the blob protocol. The request is just the raw
/// content id (whatever hash the app chose to identify the blob by) —
/// opaque to this crate either way.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BlobResponse {
    Found(Vec<u8>),
    NotFound,
}

#[derive(NetworkBehaviour)]
pub struct Behaviour {
    /// A ceiling on how many connections this node will hold, enforced
    /// before anything else gets to see them.
    ///
    /// Nothing bounded them before. That is survivable for a phone, which
    /// nothing is trying to reach, but a standing relay announces itself
    /// publicly in the DHT and invites the whole network to dial it — and
    /// it typically shares a machine with something its operator cares
    /// about. Unbounded, inbound connections grow until the process runs
    /// out of file descriptors or memory, and the first thing to notice is
    /// whatever else is on the box.
    ///
    /// These are a ceiling against unbounded growth, not a tuning knob,
    /// and the first attempt got that backwards. 256 established with 128
    /// inbound sounded conservative and was simply wrong: a standing relay
    /// in the public DHT settles at two hundred-odd peers, mostly inbound,
    /// so the inbound limit was reached in ordinary operation. Past it,
    /// libp2p refuses the connection mid-handshake and the dialling peer
    /// sees only "Handshake failed: unexpected end of file" — which is how
    /// a phone was unable to reach a relay that was working perfectly.
    ///
    /// The limit has to sit well above what a healthy node reaches on its
    /// own, or it stops being a safety net and becomes the fault. Worth
    /// pairing with a raised `LimitNOFILE`, since every connection costs
    /// descriptors; the relay's own systemd unit sets 65536.
    ///
    /// `max_established_per_peer` is above one on purpose — two peers that
    /// discover each other simultaneously legitimately open a connection
    /// each before one is dropped (see node.rs's `ConnectionClosed`
    /// handling).
    pub connection_limits: connection_limits::Behaviour,
    /// This network's own DHT — every `put_record`/`get_record` in this
    /// crate goes here. See `PRIVATE_DHT_PROTOCOL` for why it cannot be
    /// the public one.
    pub kad: kad::Behaviour<kad::store::MemoryStore>,
    /// The public IPFS DHT, kept for exactly one job: *finding* the peers
    /// that speak `PRIVATE_DHT_PROTOCOL`, via the standing-relay provider
    /// key, plus the bootstrap walk that makes that lookup possible.
    ///
    /// Provider records are the one thing the public DHT does do for us.
    /// They are announced and found by content routing, which the IPFS
    /// nodes serve for anyone, and a relay really did announce itself
    /// this way — so a fresh install with no contacts and no local peers
    /// still has somewhere to start. Nothing of this network's own data
    /// is written here.
    pub kad_public: kad::Behaviour<kad::store::MemoryStore>,
    /// Local-network peer discovery, **optional at runtime**.
    ///
    /// mDNS is the one behaviour here that touches the OS when it is
    /// built: it opens a UDP socket on 5353 and joins a multicast group.
    /// Android refuses that to ordinary apps, so on a phone — in Termux,
    /// or any container sharing the app's network namespace — the call
    /// fails with `EPERM` and, when it was mandatory, took the entire node
    /// down with "failed to build behaviour: Permission denied" before a
    /// single connection was attempted.
    ///
    /// Losing it costs only discovery of peers on the same local network.
    /// Everything that makes a node useful — the DHT, the mix, mailboxes,
    /// the ledger — is unaffected, and a phone is the last place where
    /// same-LAN discovery was going to matter. Refusing to start at all
    /// over it was badly out of proportion.
    pub mdns: Toggle<mdns::tokio::Behaviour>,
    pub identify: identify::Behaviour,
    pub relay_client: relay::client::Behaviour,
    pub dcutr: dcutr::Behaviour,
    pub envelope: request_response::cbor::Behaviour<Vec<u8>, Vec<u8>>,
    pub blob: request_response::cbor::Behaviour<Vec<u8>, BlobResponse>,
    /// See `MIX_PROTOCOL`/`MixMessage`.
    pub mix: request_response::cbor::Behaviour<MixMessage, ()>,
    /// Propagates new `@username` ledger blocks and not-yet-mined claims
    /// (see `ledger.rs`) — gossip, not the DHT, since these need to reach
    /// every node eventually, not be looked up on demand by key. One
    /// gossipsub instance serves multiple topics (that's what it's built
    /// for), so this also carries `mix_relay_directory_topic()`
    /// announcements — the field name predates that second use and is kept
    /// rather than renamed purely for churn's sake.
    pub ledger_gossip: gossipsub::Behaviour,
    /// Catches a lagging or brand-new peer up on the ledger — gossip
    /// alone only ever delivers new blocks going forward.
    pub ledger_sync: request_response::cbor::Behaviour<ChainSyncRequest, ChainSyncResponse>,
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
    // libp2p-kad's own default query timeout is 60s — reasonable as a
    // generic library default, but this crate's put_record/get_record
    // calls are all small, single-record lookups on a narrow custom
    // keyspace, not general content routing. A "not found" result (the
    // common case when checking whether a username is free) is the
    // *slowest* outcome for Kademlia to reach, since it has to actually
    // walk to the key's closest peers before concluding nobody has it —
    // capping this at a shorter, still realistic bound keeps that worst
    // case from stretching a UI "checking..." state out past a minute.
    let mut kad_config = kad::Config::new(PRIVATE_DHT_PROTOCOL);
    kad_config.set_query_timeout(Duration::from_secs(25));
    let mut kad = kad::Behaviour::with_config(peer_id, kad::store::MemoryStore::new(peer_id), kad_config);
    kad.set_mode(Some(kad::Mode::Server));

    // The public instance keeps the stock `/ipfs/kad/1.0.0` protocol,
    // because speaking IPFS's own name is the entire point of it. Its
    // longer timeout is deliberate: a walk across the public DHT visits
    // far more hops than one across this network, and the 25s above was
    // chosen for a lookup that fails fast on a small keyspace, not for
    // content routing among twenty thousand strangers.
    let mut public_config = kad::Config::default();
    public_config.set_query_timeout(Duration::from_secs(60));
    let mut kad_public =
        kad::Behaviour::with_config(peer_id, kad::store::MemoryStore::new(peer_id), public_config);
    // Server mode here is a courtesy to IPFS, not a requirement of ours:
    // nothing of this network's is stored in it either way.
    kad_public.set_mode(Some(kad::Mode::Server));

    // Signed (not Anonymous) message authenticity: `propagation_source` on
    // a received `Message` is still just "who forwarded it to us" either
    // way, but signing lets misbehaving-peer scoring attribute a message
    // to the peer that actually authored it, not just whoever relayed it
    // last — a real (if partial) mitigation for the ledger's own honest
    // early-network 51%-hashrate exposure, since it at least makes
    // spamming implausible blocks/claims attributable.
    let mut ledger_gossip = gossipsub::Behaviour::new(gossipsub::MessageAuthenticity::Signed(key.clone()), gossipsub::Config::default())
        .map_err(|err| -> Box<dyn std::error::Error + Send + Sync> { err.into() })?;
    ledger_gossip.subscribe(&ledger::blocks_topic())?;
    ledger_gossip.subscribe(&ledger::txs_topic())?;
    ledger_gossip.subscribe(&mix_relay_directory_topic())?;

    Ok(Behaviour {
        connection_limits: connection_limits::Behaviour::new(
            connection_limits::ConnectionLimits::default()
                .with_max_established(Some(1024))
                .with_max_established_incoming(Some(768))
                .with_max_pending_incoming(Some(128))
                .with_max_pending_outgoing(Some(128))
                .with_max_established_per_peer(Some(4)),
        ),
        kad,
        kad_public,
        // Built best-effort: see the field's own doc comment for why a
        // refusal here must not stop the node from starting.
        mdns: Toggle::from(match mdns::tokio::Behaviour::new(mdns::Config::default(), peer_id) {
            Ok(behaviour) => Some(behaviour),
            Err(err) => {
                // No tracing in this crate, and the caller gets a node that
                // works — so this is the only record that local discovery
                // is off, which beats silence when someone wonders why two
                // devices on one network cannot see each other.
                eprintln!("[p2p-core] local-network discovery (mDNS) unavailable, continuing without it: {err}");
                None
            }
        }),
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
        // A blob transfer moves more data over a possibly slower/relayed
        // path than a single envelope, so it gets a longer timeout.
        blob: request_response::cbor::Behaviour::new(
            [(BLOB_PROTOCOL, ProtocolSupport::Full)],
            request_response::Config::default().with_request_timeout(Duration::from_secs(60)),
        ),
        mix: request_response::cbor::Behaviour::new(
            [(MIX_PROTOCOL, ProtocolSupport::Full)],
            request_response::Config::default().with_request_timeout(Duration::from_secs(30)),
        ),
        ledger_gossip,
        ledger_sync: request_response::cbor::Behaviour::new(
            [(ledger::CHAIN_SYNC_PROTOCOL, ProtocolSupport::Full)],
            request_response::Config::default().with_request_timeout(Duration::from_secs(30)),
        ),
    })
}
