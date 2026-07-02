//! The background task that owns the `Swarm` and drives it: the rest of
//! this crate (and the app above it) only ever talks to a `P2pNode`
//! through `Command`s in and `P2pEvent`s out, never touching the swarm
//! directly — it lives entirely inside `run_event_loop`'s task.

use std::collections::HashMap;

use futures::StreamExt;
use libp2p::identify;
use libp2p::kad::{self, GetRecordOk, PutRecordOk, QueryId, QueryResult, Quorum, Record};
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, OutboundRequestId};
use libp2p::swarm::SwarmEvent;
use libp2p::{noise, tcp, yamux, Multiaddr, PeerId, Swarm};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::behaviour::{self, Behaviour, BehaviourEvent, BlobResponse};
use crate::bootstrap;
use crate::command::Command;
use crate::error::{P2pError, Result};
use crate::event::P2pEvent;
use crate::identity;
use crate::rendezvous;
use crate::username;

pub struct P2pNode {
    local_peer_id: PeerId,
    command_tx: mpsc::UnboundedSender<Command>,
    event_rx: mpsc::UnboundedReceiver<P2pEvent>,
    _task: JoinHandle<()>,
}

impl P2pNode {
    /// Builds the swarm and spawns the background task driving it, joining
    /// the public IPFS DHT for global reachability (see
    /// `bootstrap::public_dht_bootstrap_addresses`). `identity_seed` is the
    /// same 32-byte seed `spiritchat_crypto_core::identity::IdentityKeyPair`
    /// uses, so the network identity and the messaging identity are the
    /// same key.
    pub fn spawn(identity_seed: [u8; 32]) -> Result<Self> {
        Self::spawn_with_bootstrap(identity_seed, bootstrap::public_dht_bootstrap_addresses())
    }

    /// `spawn`, but with an explicit bootstrap set instead of the public
    /// IPFS DHT — for tests, and for anyone who wants this node reachable
    /// only within a network they've already connected it to some member
    /// of (pass `vec![]` for neither: local-network mDNS discovery still
    /// works either way).
    pub fn spawn_with_bootstrap(identity_seed: [u8; 32], bootstrap_addresses: Vec<Multiaddr>) -> Result<Self> {
        let keypair = identity::keypair_from_seed(&identity_seed)?;
        let local_peer_id = keypair.public().to_peer_id();
        let mut swarm = build_swarm(keypair)?;

        // Always listen, on an OS-assigned port over both transports, so
        // this node is directly dialable whenever it isn't behind a NAT
        // that blocks it outright (in which case ReserveRelaySlot is what
        // makes it reachable instead). The resulting address(es) surface
        // as P2pEvent::ListeningOn.
        swarm
            .listen_on("/ip4/0.0.0.0/tcp/0".parse().expect("valid multiaddr"))
            .map_err(|source| P2pError::Listen { addr: "tcp/0".into(), source })?;
        swarm
            .listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse().expect("valid multiaddr"))
            .map_err(|source| P2pError::Listen { addr: "udp/0/quic-v1".into(), source })?;

        let have_bootstrap_peers = !bootstrap_addresses.is_empty();
        for addr in bootstrap_addresses {
            if let Some(peer) = peer_id_of(&addr) {
                swarm.behaviour_mut().kad.add_address(&peer, addr);
            }
        }
        if have_bootstrap_peers {
            let _ = swarm.behaviour_mut().kad.bootstrap();
        }

        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(run_event_loop(swarm, command_rx, event_tx));

        Ok(Self {
            local_peer_id,
            command_tx,
            event_rx,
            _task: task,
        })
    }

    pub fn local_peer_id(&self) -> PeerId {
        self.local_peer_id
    }

    pub fn command(&self, command: Command) -> Result<()> {
        self.command_tx.send(command).map_err(|_| P2pError::NodeShutDown)
    }

    /// A cheaply cloneable, `Send + Sync` handle for issuing commands
    /// without needing `&self` (or any lock) at all — useful for callers
    /// that hold `next_event`'s `&mut self` behind a mutex (e.g. an async
    /// FFI boundary) and don't want command sends to contend with it.
    pub fn command_sender(&self) -> mpsc::UnboundedSender<Command> {
        self.command_tx.clone()
    }

    /// Waits for the next event. Returns `None` once the event loop task
    /// has stopped (it never stops on its own — only if the whole node is
    /// dropped, taking the channel with it).
    pub async fn next_event(&mut self) -> Option<P2pEvent> {
        self.event_rx.recv().await
    }
}

fn build_swarm(keypair: libp2p::identity::Keypair) -> Result<Swarm<Behaviour>> {
    let swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(tcp::Config::default(), noise::Config::new, yamux::Config::default)
        .map_err(|err| P2pError::Setup(err.to_string()))?
        .with_quic()
        // Without this, `.with_relay_client(...)` below silently calls its
        // own `.without_dns()` on the way to the relay phase (that's what
        // the type-state builder does when you skip straight past this
        // phase) and the transport ends up with no way to resolve a
        // `/dnsaddr/...` multiaddr at all. That's exactly the shape of the
        // four public IPFS bootstrap addresses in `bootstrap.rs` — without
        // this call they were never dialable, so this node could never
        // reach the public DHT, no matter how long anything downstream of
        // it waited.
        .with_dns()
        .map_err(|err| P2pError::Setup(err.to_string()))?
        .with_relay_client(noise::Config::new, yamux::Config::default)
        .map_err(|err| P2pError::Setup(err.to_string()))?
        .with_behaviour(behaviour::build)
        .map_err(|err| P2pError::Setup(err.to_string()))?
        .build();
    Ok(swarm)
}

fn peer_id_of(addr: &Multiaddr) -> Option<PeerId> {
    addr.iter().find_map(|protocol| match protocol {
        Protocol::P2p(peer) => Some(peer),
        _ => None,
    })
}

/// Tracks which outstanding DHT query or outbound request a given app-level
/// action corresponds to, since libp2p answers them asynchronously via
/// `SwarmEvent`s tagged only with an opaque `QueryId`/`OutboundRequestId`.
#[derive(Default)]
struct Pending {
    resolve_peer: HashMap<QueryId, PeerId>,
    announce: std::collections::HashSet<QueryId>,
    envelope_send: HashMap<OutboundRequestId, PeerId>,
    blob_fetch: HashMap<OutboundRequestId, (PeerId, Vec<u8>)>,
    resolve_username: HashMap<QueryId, String>,
    announce_username: HashMap<QueryId, String>,
}

async fn run_event_loop(
    mut swarm: Swarm<Behaviour>,
    mut commands: mpsc::UnboundedReceiver<Command>,
    events: mpsc::UnboundedSender<P2pEvent>,
) {
    let mut pending = Pending::default();
    // Blobs this node currently serves (e.g. its own avatar), set via
    // Command::SetLocalBlob. Lives only in memory for the life of this
    // task — persisting them across restarts, if desired, is the app's
    // job (it already has the bytes; it just re-issues SetLocalBlob).
    let mut local_blobs: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();

    // A put_record/get_record issued before this node has connected to
    // *anyone* fails immediately with "the quorum failed; needed 1 peers"
    // — Kademlia has nobody to even ask yet, since connecting to the
    // public DHT's bootstrap nodes (DNS resolution + handshake) takes real
    // wall-clock time after spawn, especially on a slow/lossy mobile
    // connection. Rather than surface that as a user-facing error the
    // first time a screen touches the DHT moments after launch, hold such
    // commands here and replay them once the first connection lands.
    let mut dht_ready = false;
    let mut deferred_commands: Vec<Command> = Vec::new();

    loop {
        tokio::select! {
            Some(command) = commands.recv() => {
                // Intercepted here rather than inside handle_command since
                // shutting down means *stopping the loop*, not something
                // handle_command's per-command effects can express — once
                // this breaks, `events` (and therefore event_tx) drops at
                // the end of this function, which is what makes
                // `P2pNode::next_event` start returning `None`.
                if matches!(command, Command::Shutdown) {
                    break;
                }
                if !dht_ready && needs_dht_peer(&command) {
                    deferred_commands.push(command);
                } else {
                    handle_command(&mut swarm, &mut pending, &mut local_blobs, &events, command);
                }
            }
            swarm_event = swarm.select_next_some() => {
                if !dht_ready && matches!(swarm_event, SwarmEvent::ConnectionEstablished { .. }) {
                    dht_ready = true;
                    for command in deferred_commands.drain(..) {
                        handle_command(&mut swarm, &mut pending, &mut local_blobs, &events, command);
                    }
                }
                handle_swarm_event(&mut swarm, &mut pending, &local_blobs, &events, swarm_event);
            }
            else => break,
        }
    }
}

/// Whether `command` needs at least one connected peer to have any real
/// chance of succeeding — the four commands that go straight to Kademlia's
/// `put_record`/`get_record`. Dialing itself is excluded: it's how a
/// connection gets made in the first place, so it must never be deferred.
fn needs_dht_peer(command: &Command) -> bool {
    matches!(
        command,
        Command::ResolvePeerAddresses { .. }
            | Command::AnnounceAddresses { .. }
            | Command::AnnounceUsername { .. }
            | Command::ResolveUsername { .. }
    )
}

fn handle_command(
    swarm: &mut Swarm<Behaviour>,
    pending: &mut Pending,
    local_blobs: &mut HashMap<Vec<u8>, Vec<u8>>,
    events: &mpsc::UnboundedSender<P2pEvent>,
    command: Command,
) {
    match command {
        Command::Dial { peer, known_addresses } => {
            for addr in &known_addresses {
                swarm.behaviour_mut().kad.add_address(&peer, addr.clone());
            }
            let opts = if known_addresses.is_empty() {
                peer.into()
            } else {
                libp2p::swarm::dial_opts::DialOpts::peer_id(peer)
                    .addresses(known_addresses)
                    .build()
            };
            // A synchronous rejection here (malformed opts, already
            // dialing) is the only case this crate can report immediately
            // — everything else surfaces later as
            // SwarmEvent::OutgoingConnectionError, handled in
            // handle_swarm_event.
            if let Err(err) = swarm.dial(opts) {
                let _ = events.send(P2pEvent::DialFailed { peer: Some(peer), reason: err.to_string() });
            }
        }

        Command::ResolvePeerAddresses { peer } => {
            let key = rendezvous::record_key_for(&peer);
            let query_id = swarm.behaviour_mut().kad.get_record(key);
            pending.resolve_peer.insert(query_id, peer);
        }

        Command::AnnounceAddresses { addresses } => {
            let local_peer = *swarm.local_peer_id();
            let key = rendezvous::record_key_for(&local_peer);
            let record = Record::new(key, rendezvous::encode_addresses(&addresses));
            if let Ok(query_id) = swarm.behaviour_mut().kad.put_record(record, Quorum::One) {
                pending.announce.insert(query_id);
            }
        }

        Command::SendEnvelope { to, bytes } => {
            let request_id = swarm.behaviour_mut().envelope.send_request(&to, bytes);
            pending.envelope_send.insert(request_id, to);
        }

        Command::ReserveRelaySlot { relay_address } => {
            let circuit_addr = relay_address.with(Protocol::P2pCircuit);
            // A successful reservation surfaces later as
            // P2pEvent::ListeningOn (a NewListenAddr with a /p2p-circuit
            // suffix); a *rejected* reservation (the relay refuses, the
            // circuit drops) surfaces as ListenerClosed, which — like
            // ListenerError — this crate does not yet translate into a
            // P2pEvent. Only the synchronous, immediate failure case
            // (malformed address) is reported here.
            if let Err(err) = swarm.listen_on(circuit_addr) {
                let _ = events.send(P2pEvent::RelayReservationFailed { reason: err.to_string() });
            }
        }

        Command::SetLocalBlob { id, bytes } => {
            local_blobs.insert(id, bytes);
        }

        Command::ClearLocalBlob { id } => {
            local_blobs.remove(&id);
        }

        Command::FetchBlob { peer, id } => {
            let request_id = swarm.behaviour_mut().blob.send_request(&peer, id.clone());
            pending.blob_fetch.insert(request_id, (peer, id));
        }

        Command::AnnounceUsername { username, claim } => {
            let key = username::record_key_for(&username);
            let record = Record::new(key, claim);
            if let Ok(query_id) = swarm.behaviour_mut().kad.put_record(record, Quorum::One) {
                pending.announce_username.insert(query_id, username);
            }
        }

        Command::ResolveUsername { username } => {
            let key = username::record_key_for(&username);
            let query_id = swarm.behaviour_mut().kad.get_record(key);
            pending.resolve_username.insert(query_id, username);
        }

        // Handled in run_event_loop before this function is ever called —
        // present only because Command's match must stay exhaustive.
        Command::Shutdown => {}
    }
}

fn handle_swarm_event(
    swarm: &mut Swarm<Behaviour>,
    pending: &mut Pending,
    local_blobs: &HashMap<Vec<u8>, Vec<u8>>,
    events: &mpsc::UnboundedSender<P2pEvent>,
    event: SwarmEvent<BehaviourEvent>,
) {
    match event {
        SwarmEvent::NewListenAddr { address, .. } => {
            let _ = events.send(P2pEvent::ListeningOn(address));
        }

        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
            let _ = events.send(P2pEvent::PeerConnected(peer_id));
        }

        SwarmEvent::ConnectionClosed { peer_id, .. } => {
            let _ = events.send(P2pEvent::PeerDisconnected(peer_id));
        }

        SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
            let _ = events.send(P2pEvent::DialFailed { peer: peer_id, reason: error.to_string() });
        }

        SwarmEvent::Behaviour(BehaviourEvent::Mdns(mdns_event)) => {
            handle_mdns_event(swarm, events, mdns_event);
        }

        SwarmEvent::Behaviour(BehaviourEvent::Kad(kad_event)) => {
            handle_kad_event(pending, events, kad_event);
        }

        // Kademlia's routing table is *not* populated automatically from
        // connections — a peer only becomes queryable/storable-to once
        // something feeds its address in. identify is what tells us a
        // connected peer's own listen addresses, which is the standard
        // libp2p pattern for bridging the two: without this, a node with
        // no mDNS/bootstrap-supplied addresses for a peer would have an
        // empty routing table even while directly connected to them, and
        // every put_record/get_record would fail for lack of anyone to
        // ask.
        SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Received {
            peer_id,
            info,
            ..
        })) => {
            for addr in info.listen_addrs {
                swarm.behaviour_mut().kad.add_address(&peer_id, addr);
            }
            let _ = events.send(P2pEvent::PeerIdentified(peer_id));
        }

        SwarmEvent::Behaviour(BehaviourEvent::Envelope(envelope_event)) => {
            handle_envelope_event(swarm, pending, events, envelope_event);
        }

        SwarmEvent::Behaviour(BehaviourEvent::Blob(blob_event)) => {
            handle_blob_event(swarm, pending, local_blobs, events, blob_event);
        }

        _ => {}
    }
}

fn handle_mdns_event(
    swarm: &mut Swarm<Behaviour>,
    events: &mpsc::UnboundedSender<P2pEvent>,
    event: libp2p::mdns::Event,
) {
    match event {
        libp2p::mdns::Event::Discovered(discovered) => {
            for (peer, addr) in discovered {
                swarm.behaviour_mut().kad.add_address(&peer, addr);
                let _ = events.send(P2pEvent::PeerDiscoveredLocally(peer));
            }
        }
        libp2p::mdns::Event::Expired(_) => {}
    }
}

fn handle_kad_event(
    pending: &mut Pending,
    events: &mpsc::UnboundedSender<P2pEvent>,
    event: kad::Event,
) {
    let kad::Event::OutboundQueryProgressed { id, result, .. } = event else {
        return;
    };

    match result {
        QueryResult::GetRecord(Ok(GetRecordOk::FoundRecord(found))) => {
            if let Some(peer) = pending.resolve_peer.remove(&id) {
                let addresses = rendezvous::decode_addresses(&found.record.value);
                let _ = events.send(P2pEvent::PeerAddressesResolved { peer, addresses });
            } else if let Some(username) = pending.resolve_username.remove(&id) {
                let _ = events.send(P2pEvent::UsernameResolved {
                    username,
                    claim: found.record.value,
                });
            }
        }
        QueryResult::GetRecord(Err(_)) => {
            if let Some(peer) = pending.resolve_peer.remove(&id) {
                let _ = events.send(P2pEvent::PeerAddressResolutionFailed { peer });
            } else if let Some(username) = pending.resolve_username.remove(&id) {
                let _ = events.send(P2pEvent::UsernameResolutionFailed { username });
            }
        }
        QueryResult::PutRecord(Ok(PutRecordOk { .. })) => {
            if pending.announce.remove(&id) {
                let _ = events.send(P2pEvent::AddressesAnnounced);
            } else if let Some(username) = pending.announce_username.remove(&id) {
                let _ = events.send(P2pEvent::UsernameAnnounced { username });
            }
        }
        QueryResult::PutRecord(Err(err)) => {
            if pending.announce.remove(&id) {
                let _ = events.send(P2pEvent::AddressAnnouncementFailed { reason: err.to_string() });
            } else if let Some(username) = pending.announce_username.remove(&id) {
                let _ = events.send(P2pEvent::UsernameAnnouncementFailed {
                    username,
                    reason: err.to_string(),
                });
            }
        }
        _ => {}
    }
}

fn handle_envelope_event(
    swarm: &mut Swarm<Behaviour>,
    pending: &mut Pending,
    events: &mpsc::UnboundedSender<P2pEvent>,
    event: request_response::Event<Vec<u8>, Vec<u8>>,
) {
    match event {
        request_response::Event::Message { peer, message, .. } => match message {
            request_response::Message::Request { request, channel, .. } => {
                let _ = events.send(P2pEvent::EnvelopeReceived { from: peer, bytes: request });
                // The envelope protocol is a request/response shape purely
                // because libp2p's request-response building block requires
                // a reply; the "response" carries no information of its
                // own. Acknowledge immediately so the sender's request
                // resolves.
                let _ = swarm.behaviour_mut().envelope.send_response(channel, Vec::new());
            }
            request_response::Message::Response { request_id, .. } => {
                if let Some(peer) = pending.envelope_send.remove(&request_id) {
                    let _ = events.send(P2pEvent::EnvelopeDelivered { to: peer });
                }
            }
        },
        request_response::Event::OutboundFailure { request_id, error, .. } => {
            if let Some(peer) = pending.envelope_send.remove(&request_id) {
                let _ = events.send(P2pEvent::EnvelopeDeliveryFailed {
                    to: peer,
                    reason: error.to_string(),
                });
            }
        }
        _ => {}
    }
}

fn handle_blob_event(
    swarm: &mut Swarm<Behaviour>,
    pending: &mut Pending,
    local_blobs: &HashMap<Vec<u8>, Vec<u8>>,
    events: &mpsc::UnboundedSender<P2pEvent>,
    event: request_response::Event<Vec<u8>, BlobResponse>,
) {
    match event {
        request_response::Event::Message { message, .. } => match message {
            request_response::Message::Request { request: id, channel, .. } => {
                let response = match local_blobs.get(&id) {
                    Some(bytes) => BlobResponse::Found(bytes.clone()),
                    None => BlobResponse::NotFound,
                };
                let _ = swarm.behaviour_mut().blob.send_response(channel, response);
            }
            request_response::Message::Response { request_id, response } => {
                if let Some((peer, id)) = pending.blob_fetch.remove(&request_id) {
                    match response {
                        BlobResponse::Found(bytes) => {
                            let _ = events.send(P2pEvent::BlobFetched { peer, id, bytes });
                        }
                        BlobResponse::NotFound => {
                            let _ = events.send(P2pEvent::BlobFetchFailed {
                                peer,
                                id,
                                reason: "peer does not have this blob".to_string(),
                            });
                        }
                    }
                }
            }
        },
        request_response::Event::OutboundFailure { request_id, error, .. } => {
            if let Some((peer, id)) = pending.blob_fetch.remove(&request_id) {
                let _ = events.send(P2pEvent::BlobFetchFailed { peer, id, reason: error.to_string() });
            }
        }
        _ => {}
    }
}
