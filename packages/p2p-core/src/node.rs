//! The background task that owns the `Swarm` and drives it: the rest of
//! this crate (and the app above it) only ever talks to a `P2pNode`
//! through `Command`s in and `P2pEvent`s out, never touching the swarm
//! directly — it lives entirely inside `run_event_loop`'s task.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::StreamExt;
use libp2p::identify;
use libp2p::kad::{self, GetRecordOk, PutRecordOk, QueryId, QueryResult, Quorum, Record};
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, OutboundRequestId};
use libp2p::swarm::SwarmEvent;
use libp2p::{gossipsub, noise, tcp, yamux, Multiaddr, PeerId, Swarm};
use rand::seq::IteratorRandom;
use rand::Rng;
use sphinx_packet::route::{DestinationAddressBytes, NodeAddressBytes};
use sphinx_packet::surb::SURB;
use sphinx_packet::SphinxPacket;
use spiritchat_ledger_core::block::MAX_TXS_PER_BLOCK;
use spiritchat_ledger_core::difficulty::expand_target;
use spiritchat_ledger_core::{ApplyOutcome, Block, BlockHeader, ChainStore, Hash32, Transaction};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::behaviour::{self, Behaviour, BehaviourEvent, BlobResponse, MixMessage};
use crate::bootstrap;
use crate::command::Command;
use crate::error::{P2pError, Result};
use crate::event::P2pEvent;
use crate::identity;
use crate::ledger::{self, ChainSyncRequest, ChainSyncResponse};
use crate::mailbox;
use crate::mix;
use crate::rendezvous;
use crate::username;

/// How often this node emits one piece of Loopix-style dummy traffic
/// (drop cover or loop, chosen at random each time) toward a randomly
/// chosen currently-connected mix peer — deliberately modest for now.
/// Real Loopix tuning (balancing unlinkability strength against battery/
/// data cost) is deferred to hardening (the plan's Phase 8); this exists
/// so cover/loop traffic exists and is exercised at all, not to hit a
/// specific published rate yet.
const MIX_DUMMY_TRAFFIC_INTERVAL: Duration = Duration::from_secs(30);

/// The average per-hop delay this node uses for traffic *it originates*
/// (dummy packets here; real deposits will use their own value once
/// wired in a later phase) — mirrors `MIX_DUMMY_TRAFFIC_INTERVAL` in
/// being a placeholder magnitude, not a tuned constant.
const MIX_DUMMY_TRAFFIC_HOP_DELAY: Duration = Duration::from_millis(100);

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
    /// same key. `ledger_data_dir` is where the `@username` ledger's redb
    /// database lives — the first on-disk state this crate owns directly
    /// (everything else stays app-managed); the caller (Swift on iOS) is
    /// responsible for pointing this at a real, writable, per-identity
    /// directory, mirroring how `BlobStore.swift` already picks its own
    /// cache directory under `applicationSupportDirectory`.
    pub fn spawn(identity_seed: [u8; 32], ledger_data_dir: PathBuf) -> Result<Self> {
        Self::spawn_with_bootstrap(identity_seed, bootstrap::public_dht_bootstrap_addresses(), ledger_data_dir)
    }

    /// `spawn`, but with an explicit bootstrap set instead of the public
    /// IPFS DHT — for tests, and for anyone who wants this node reachable
    /// only within a network they've already connected it to some member
    /// of (pass `vec![]` for neither: local-network mDNS discovery still
    /// works either way).
    pub fn spawn_with_bootstrap(
        identity_seed: [u8; 32],
        bootstrap_addresses: Vec<Multiaddr>,
        ledger_data_dir: PathBuf,
    ) -> Result<Self> {
        let keypair = identity::keypair_from_seed(&identity_seed)?;
        let local_peer_id = keypair.public().to_peer_id();
        let mut swarm = build_swarm(keypair)?;
        let (mix_secret, mix_public) = mix::routing_keypair_from_seed(&identity_seed);

        let chain_store = ChainStore::open(&ledger_data_dir)?;
        // A dedicated file, sibling to the ledger's own — see
        // `mailbox::MailboxStore::open`'s own doc comment. Deriving this
        // from `ledger_data_dir` rather than taking a whole extra
        // parameter keeps every existing caller (tests, the FFI layer,
        // Swift) working unchanged.
        let mailbox_data_dir = ledger_data_dir.with_file_name("mailbox.redb");
        let mailbox_store = mailbox::MailboxStore::open(&mailbox_data_dir)?;

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
        let task = tokio::spawn(run_event_loop(swarm, chain_store, mailbox_store, mix_secret, mix_public, command_rx, event_tx));

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
        //
        // Deliberately `.with_dns_config(...)` over the plain `.with_dns()`
        // shortcut: the latter reads the OS's `/etc/resolv.conf` via
        // `libp2p_dns::tokio::Transport::system` — a file libp2p-dns's own
        // docs warn "fails (panics even!) if it does not exist" on
        // platforms without one (they call out Android; a sandboxed iOS
        // process — this app's sideloaded LiveContainer target — is the
        // same story, and every P2P/ledger startup was failing outright
        // over exactly this before it was ever noticed, since the failure
        // used to be reported as a generic swarm-setup error with no
        // reachable diagnostic). A hardcoded public resolver config needs
        // no filesystem access at all, at the cost of not respecting
        // whatever custom DNS the OS is actually configured with — an
        // acceptable trade here since this transport only ever resolves
        // the handful of `/dnsaddr/...` bootstrap addresses in
        // `bootstrap.rs`, not arbitrary user-facing lookups.
        .with_dns_config(libp2p::dns::ResolverConfig::cloudflare(), libp2p::dns::ResolverOpts::default())
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

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock is before 1970").as_secs()
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
    /// A `RequestChainSync`'s first step: waiting on `peer`'s tip.
    chain_sync_tip: HashMap<OutboundRequestId, PeerId>,
    /// A `RequestChainSync`'s follow-up: waiting on a batch of blocks from
    /// `peer` after learning its tip is heavier than ours.
    chain_sync_blocks: HashMap<OutboundRequestId, PeerId>,
}

/// One in-flight `spawn_blocking` nonce search — tagged with a generation
/// number so a result arriving after it's been superseded (a newer
/// candidate started, or mining was stopped and restarted) can be told
/// apart from the one still being waited on.
struct MiningState {
    public_key: [u8; 32],
    stop: Arc<AtomicBool>,
    generation: u64,
}

/// This node's mining loop, if enabled. Only one attempt ever runs at a
/// time (deliberately single-core, for thermal/battery reasons — see the
/// project plan) — `restart_if_active` is how every place that changes the
/// tip or the mempool asks the in-flight attempt to abandon its now-stale
/// candidate and start over, keeping mining continuous without the event
/// loop itself needing to know when that's necessary.
struct Mining {
    active: Option<MiningState>,
    next_generation: u64,
    result_tx: mpsc::UnboundedSender<(u64, Block)>,
}

impl Mining {
    fn new(result_tx: mpsc::UnboundedSender<(u64, Block)>) -> Self {
        Mining { active: None, next_generation: 0, result_tx }
    }

    fn stop(&mut self) {
        if let Some(state) = self.active.take() {
            state.stop.store(true, Ordering::Relaxed);
        }
    }

    fn start(&mut self, public_key: [u8; 32], chain_store: &ChainStore, mempool: &HashMap<Hash32, Transaction>) {
        self.stop();
        self.next_generation += 1;
        let stop = Arc::new(AtomicBool::new(false));
        self.active = Some(MiningState { public_key, stop: stop.clone(), generation: self.next_generation });
        spawn_mining_attempt(chain_store, mempool, public_key, stop, self.next_generation, self.result_tx.clone());
    }

    /// A no-op unless mining is currently enabled — kills whatever attempt
    /// is in flight and starts a fresh one against `chain_store`/`mempool`
    /// as they stand right now.
    fn restart_if_active(&mut self, chain_store: &ChainStore, mempool: &HashMap<Hash32, Transaction>) {
        if let Some(state) = &self.active {
            let public_key = state.public_key;
            self.start(public_key, chain_store, mempool);
        }
    }
}

/// Assembles a candidate block extending the current tip — selecting up to
/// `MAX_TXS_PER_BLOCK` mempool transactions (deterministic tx-id-ascending
/// order, so two nodes with the same mempool build the same candidate),
/// re-checked against the tip's own state so a candidate never bundles a
/// transaction that would make the whole block invalid — then hands the
/// actual nonce search to a blocking thread, since SHA-256 grinding must
/// never share a thread with the async event loop driving the swarm.
fn spawn_mining_attempt(
    chain_store: &ChainStore,
    mempool: &HashMap<Hash32, Transaction>,
    public_key: [u8; 32],
    stop: Arc<AtomicBool>,
    generation: u64,
    result_tx: mpsc::UnboundedSender<(u64, Block)>,
) {
    let (Ok(difficulty_target), Ok(median_time_past)) =
        (chain_store.expected_difficulty(), chain_store.median_time_past())
    else {
        // The tip is momentarily in a state these can't be computed for
        // (shouldn't happen in practice — the tip is always a validated
        // block) — nothing sensible to mine against.
        return;
    };
    let prev_hash = chain_store.tip_hash();
    let height = chain_store.tip_height() + 1;

    let mut candidates: Vec<&Transaction> = mempool.values().collect();
    candidates.sort_by_key(|tx| tx.id());
    let mut selected = Vec::with_capacity(MAX_TXS_PER_BLOCK);
    let mut usernames_in_candidate = std::collections::HashSet::new();
    for tx in candidates {
        if selected.len() >= MAX_TXS_PER_BLOCK {
            break;
        }
        if !usernames_in_candidate.insert(tx.username.clone()) {
            continue; // a second mempool tx racing for the same name this attempt already took
        }
        if chain_store.is_valid_candidate_transaction(tx, height) {
            selected.push(tx.clone());
        }
    }

    let tx_commitment = Block::compute_tx_commitment(&selected);
    // Must land strictly after median-time-past; never *behind* real time
    // either, since that would just mean an immediate re-check failure the
    // moment this block reaches any other node's clock.
    let timestamp = now_unix().max(median_time_past + 1);

    let header = BlockHeader {
        version: 1,
        height,
        prev_hash,
        timestamp,
        tx_commitment,
        difficulty_target,
        nonce: 0,
        miner_public_key: public_key,
    };

    tokio::task::spawn_blocking(move || mine(header, selected, stop, generation, result_tx));
}

/// The actual nonce search — pure CPU work, deliberately kept free of any
/// `ChainStore`/`Swarm` access so it only ever needs what's captured here.
/// Checks `stop` and refreshes the timestamp (so a long-running search
/// doesn't end up submitting a block timestamped from when it started)
/// only every `CHECK_INTERVAL` attempts — checking every single nonce would
/// waste real hashing time on synchronization instead of hashing.
fn mine(
    mut header: BlockHeader,
    transactions: Vec<Transaction>,
    stop: Arc<AtomicBool>,
    generation: u64,
    result_tx: mpsc::UnboundedSender<(u64, Block)>,
) {
    const CHECK_INTERVAL: u64 = 50_000;
    let target = expand_target(header.difficulty_target);
    let mut attempts: u64 = 0;
    loop {
        if header.hash().meets_target(&target) {
            let _ = result_tx.send((generation, Block { header, transactions }));
            return;
        }
        header.nonce = header.nonce.wrapping_add(1);
        attempts += 1;
        if attempts.is_multiple_of(CHECK_INTERVAL) {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            header.timestamp = header.timestamp.max(now_unix());
        }
    }
}

async fn run_event_loop(
    mut swarm: Swarm<Behaviour>,
    mut chain_store: ChainStore,
    mut mailbox_store: mailbox::MailboxStore,
    mix_secret: StaticSecret,
    mix_public: PublicKey,
    mut commands: mpsc::UnboundedReceiver<Command>,
    events: mpsc::UnboundedSender<P2pEvent>,
) {
    let local_peer_id = *swarm.local_peer_id();
    let mut pending = Pending::default();
    // Blobs this node currently serves (e.g. its own avatar), set via
    // Command::SetLocalBlob. Lives only in memory for the life of this
    // task — persisting them across restarts, if desired, is the app's
    // job (it already has the bytes; it just re-issues SetLocalBlob).
    let mut local_blobs: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();
    // Which currently-connected peers answer to which Sphinx routing
    // address (`mix::node_address_for`) — how a mix hop resolves a peeled
    // packet's `next_hop_address` back into someone it can actually dial.
    // Deliberately just "peers we're connected to right now", not a
    // separate discovery/directory mechanism: relay *selection* (who to
    // route new packets through) is a sender-side concern for a later
    // phase; forwarding an already-built packet only ever needs to reach
    // whichever specific peer the path already named.
    let mut known_mix_relays: HashMap<NodeAddressBytes, PeerId> = HashMap::new();
    // Sphinx routing public keys learned from peers this node has
    // exchanged mix traffic with (real or dummy) — see `MixMessage`'s own
    // doc comment for why riding this alongside the packet bytes, rather
    // than a separate directory lookup, is enough to originate loop/cover
    // traffic through peers already reachable this way. Real deposit path
    // *selection* through peers not yet exchanged-with is a later phase's
    // job (a published relay directory), not this one's.
    let mut known_mix_routing_keys: HashMap<PeerId, PublicKey> = HashMap::new();
    // A Forward outcome's own `Delay` (chosen by whoever originated the
    // packet, revealed to this hop only by peeling) must actually be
    // honored before re-sending — otherwise "Poisson mixing" is just a
    // label with no effect on real timing. Since the event loop can never
    // block waiting on a single delay, each Forward spawns its own sleep
    // and reports back over this channel once it's actually time to send.
    let (mix_forward_tx, mut mix_forward_rx) = mpsc::unbounded_channel::<(PeerId, MixMessage)>();
    let mut dummy_traffic_interval = tokio::time::interval(MIX_DUMMY_TRAFFIC_INTERVAL);
    // Gated by `Command::SetMixDummyTrafficActive` — off until the app
    // layer says otherwise (see that command's own doc comment for why).
    let mut mix_dummy_traffic_active = false;
    // `Command::DepositToMailbox`'s PoW mining runs on a blocking thread
    // (mirrors the ledger's own mining loop) and reports the finished,
    // stamped deposit back here so the event loop itself can pick a mix
    // path and send it — mining must never share a thread with the async
    // loop driving the swarm.
    let (deposit_tx, mut deposit_rx) = mpsc::unbounded_channel::<mailbox::MailboxDeposit>();
    // Not-yet-mined @username claims this node knows about (submitted
    // locally or received over gossip) — never persisted, matching every
    // other in-memory-only piece of state in this crate; a mempool only
    // ever needs to survive until *someone's* miner picks it up, and if
    // this process restarts before that happens, the claim's original
    // submitter still has it and can resubmit.
    let mut mempool: HashMap<Hash32, Transaction> = HashMap::new();

    let (mining_result_tx, mut mining_result_rx) = mpsc::unbounded_channel::<(u64, Block)>();
    let mut mining = Mining::new(mining_result_tx);

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
                    handle_command(&mut swarm, &mut chain_store, &mut pending, &mut local_blobs, &mut mempool, &mut mining, &known_mix_relays, &known_mix_routing_keys, local_peer_id, mix_public, &deposit_tx, &mut mix_dummy_traffic_active, &events, command);
                }
            }
            swarm_event = swarm.select_next_some() => {
                if !dht_ready && matches!(swarm_event, SwarmEvent::ConnectionEstablished { .. }) {
                    dht_ready = true;
                    for command in deferred_commands.drain(..) {
                        handle_command(&mut swarm, &mut chain_store, &mut pending, &mut local_blobs, &mut mempool, &mut mining, &known_mix_relays, &known_mix_routing_keys, local_peer_id, mix_public, &deposit_tx, &mut mix_dummy_traffic_active, &events, command);
                    }
                }
                handle_swarm_event(
                    &mut swarm, &mut chain_store, &mut pending, &local_blobs, &mut mempool, &mut mining,
                    &mut known_mix_relays, &mut known_mix_routing_keys, &mut mailbox_store, &mix_secret, mix_public, &mix_forward_tx,
                    &events, swarm_event,
                );
            }
            Some((next_peer, mix_message)) = mix_forward_rx.recv() => {
                // The Poisson delay this Forward's own Sphinx header
                // specified has now actually elapsed (see
                // `handle_mix_event`) — only now does the re-encrypted
                // packet actually leave this node.
                swarm.behaviour_mut().mix.send_request(&next_peer, mix_message);
            }
            _ = dummy_traffic_interval.tick() => {
                if mix_dummy_traffic_active {
                    emit_dummy_mix_traffic(&mut swarm, &known_mix_relays, &known_mix_routing_keys, local_peer_id, mix_public);
                }
            }
            Some(deposit) = deposit_rx.recv() => {
                // The blocking PoW mining `Command::DepositToMailbox`
                // kicked off has finished — now pick a path and actually
                // send it into the mix.
                send_mailbox_deposit(&mut swarm, &known_mix_relays, &known_mix_routing_keys, mix_public, deposit, &events);
            }
            Some((generation, block)) = mining_result_rx.recv() => {
                let is_current = mining.active.as_ref().map(|state| state.generation) == Some(generation);
                if is_current {
                    let height = block.header.height;
                    let outcome = apply_and_broadcast_block(&mut swarm, &mut chain_store, &mut mempool, &mut mining, &events, block);
                    match outcome {
                        Some(ApplyOutcome::ExtendedTip) | Some(ApplyOutcome::ReorgedTo { .. }) => {
                            let _ = events.send(P2pEvent::NewBlockMined { height });
                        }
                        _ => {
                            // Lost a race to another miner, or otherwise
                            // didn't advance the tip. Either way this
                            // attempt has now finished and
                            // apply_and_broadcast_block only restarts
                            // mining when the tip actually changed, so
                            // mining would otherwise stall here forever.
                            mining.restart_if_active(&chain_store, &mempool);
                        }
                    }
                }
                // A stale result from an attempt already superseded by a
                // newer generation — silently dropped, the superseding
                // attempt is already running.
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

// One parameter per independent piece of event-loop state this function can
// touch (mirrors run_event_loop's own locals) — bundling them into a struct
// would just move the same eight names one level down without reducing
// what a caller needs to reason about.
#[allow(clippy::too_many_arguments)]
fn handle_command(
    swarm: &mut Swarm<Behaviour>,
    chain_store: &mut ChainStore,
    pending: &mut Pending,
    local_blobs: &mut HashMap<Vec<u8>, Vec<u8>>,
    mempool: &mut HashMap<Hash32, Transaction>,
    mining: &mut Mining,
    known_mix_relays: &HashMap<NodeAddressBytes, PeerId>,
    known_mix_routing_keys: &HashMap<PeerId, PublicKey>,
    local_peer_id: PeerId,
    mix_public: PublicKey,
    deposit_tx: &mpsc::UnboundedSender<mailbox::MailboxDeposit>,
    mix_dummy_traffic_active: &mut bool,
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

        Command::SendMixPacket { first_hop, packet_bytes } => {
            let message = MixMessage { packet_bytes, sender_routing_public_key: mix_public.to_bytes() };
            swarm.behaviour_mut().mix.send_request(&first_hop, message);
        }

        Command::AnnounceMixRelay => {
            let announcement = behaviour::MixRelayAnnouncement { routing_public_key: mix_public.to_bytes() };
            if let Ok(bytes) = bincode::serialize(&announcement) {
                let _ = swarm.behaviour_mut().ledger_gossip.publish(behaviour::mix_relay_directory_topic(), bytes);
            }
        }

        Command::DepositToMailbox { shared_material, envelope } => {
            let deposit_tx = deposit_tx.clone();
            // Mining the PoW stamp is pure CPU grinding — same reasoning
            // as the ledger's own mining loop for never running it on the
            // thread driving the swarm.
            tokio::task::spawn_blocking(move || {
                let deposited_at = mailbox::now_unix();
                let tag = mailbox::mailbox_tag(&shared_material, mailbox::epoch_for(deposited_at));
                let pow_nonce = mailbox::mine_stamp(&tag, &envelope, deposited_at);
                let _ = deposit_tx.send(mailbox::MailboxDeposit { tag, envelope, deposited_at, pow_nonce });
            });
        }

        Command::RetrieveFromMailbox { shared_material } => {
            send_mailbox_retrieval_query(swarm, known_mix_relays, known_mix_routing_keys, local_peer_id, mix_public, shared_material, events);
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

        Command::SubmitUsernameClaim { transaction } => {
            if let Err(err) = transaction.verify_self_contained() {
                let _ = events.send(P2pEvent::LedgerSubmissionRejected { reason: err.to_string() });
                return;
            }
            mempool.insert(transaction.id(), transaction.clone());
            if let Ok(bytes) = bincode::serialize(&transaction) {
                let _ = swarm.behaviour_mut().ledger_gossip.publish(ledger::txs_topic(), bytes);
            }
        }

        Command::SubmitMinedBlock { block } => {
            apply_and_broadcast_block(swarm, chain_store, mempool, mining, events, block);
        }

        Command::QueryUsernameOwner { username } => {
            match chain_store.username_owner(&username) {
                Some(owner) => {
                    let _ = events.send(P2pEvent::UsernameOwnerResolved {
                        username,
                        owner_public_key: owner.owner_public_key.to_vec(),
                        claimed_at_height: owner.claimed_at_height,
                    });
                }
                None => {
                    let _ = events.send(P2pEvent::UsernameOwnerNotFound { username });
                }
            }
        }

        Command::RequestChainSync { peer } => {
            let request_id = swarm.behaviour_mut().ledger_sync.send_request(&peer, ChainSyncRequest::GetTip);
            pending.chain_sync_tip.insert(request_id, peer);
        }

        Command::QueryChainTip => {
            let _ = events.send(P2pEvent::ChainTipChanged {
                height: chain_store.tip_height(),
                hash: format!("{}", chain_store.tip_hash()),
            });
        }

        Command::StartMining { public_key } => {
            mining.start(public_key, chain_store, mempool);
        }

        Command::StopMining => {
            mining.stop();
        }

        Command::SetMixDummyTrafficActive { enabled } => {
            *mix_dummy_traffic_active = enabled;
        }

        // Handled in run_event_loop before this function is ever called —
        // present only because Command's match must stay exhaustive.
        Command::Shutdown => {}
    }
}

/// Shared by a locally-mined block (`Command::SubmitMinedBlock`) and one
/// received over gossip: validate, apply, and — only for a block this node
/// didn't already have — clear its transactions out of the mempool, tell
/// the app the tip may have moved, and (if the tip actually changed) ask
/// any in-flight mining attempt to restart against the new tip rather than
/// keep grinding toward a now-stale parent. Returns the outcome so a caller
/// that cares (the mining-result branch in `run_event_loop`, to tell a real
/// new tip from a lost race) doesn't have to re-derive it.
fn apply_and_broadcast_block(
    swarm: &mut Swarm<Behaviour>,
    chain_store: &mut ChainStore,
    mempool: &mut HashMap<Hash32, Transaction>,
    mining: &mut Mining,
    events: &mpsc::UnboundedSender<P2pEvent>,
    block: Block,
) -> Option<ApplyOutcome> {
    match chain_store.try_apply(block.clone(), now_unix()) {
        Ok(ApplyOutcome::AlreadyKnown) => Some(ApplyOutcome::AlreadyKnown),
        Ok(outcome @ (ApplyOutcome::ExtendedTip | ApplyOutcome::AddedToFork | ApplyOutcome::ReorgedTo { .. })) => {
            for tx in &block.transactions {
                mempool.remove(&tx.id());
            }
            if let Ok(bytes) = bincode::serialize(&block) {
                let _ = swarm.behaviour_mut().ledger_gossip.publish(ledger::blocks_topic(), bytes);
            }
            if !matches!(outcome, ApplyOutcome::AddedToFork) {
                let _ = events.send(P2pEvent::ChainTipChanged {
                    height: chain_store.tip_height(),
                    hash: format!("{}", chain_store.tip_hash()),
                });
                mining.restart_if_active(chain_store, mempool);
            }
            Some(outcome)
        }
        Err(err) => {
            let _ = events.send(P2pEvent::LedgerSubmissionRejected { reason: err.to_string() });
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_swarm_event(
    swarm: &mut Swarm<Behaviour>,
    chain_store: &mut ChainStore,
    pending: &mut Pending,
    local_blobs: &HashMap<Vec<u8>, Vec<u8>>,
    mempool: &mut HashMap<Hash32, Transaction>,
    mining: &mut Mining,
    known_mix_relays: &mut HashMap<NodeAddressBytes, PeerId>,
    known_mix_routing_keys: &mut HashMap<PeerId, PublicKey>,
    mailbox_store: &mut mailbox::MailboxStore,
    mix_secret: &StaticSecret,
    mix_public: PublicKey,
    mix_forward_tx: &mpsc::UnboundedSender<(PeerId, MixMessage)>,
    events: &mpsc::UnboundedSender<P2pEvent>,
    event: SwarmEvent<BehaviourEvent>,
) {
    match event {
        SwarmEvent::NewListenAddr { address, .. } => {
            let _ = events.send(P2pEvent::ListeningOn(address));
        }

        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
            known_mix_relays.insert(mix::node_address_for(&peer_id.to_bytes()), peer_id);
            let _ = events.send(P2pEvent::PeerConnected(peer_id));
        }

        SwarmEvent::ConnectionClosed { peer_id, .. } => {
            known_mix_relays.remove(&mix::node_address_for(&peer_id.to_bytes()));
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

        SwarmEvent::Behaviour(BehaviourEvent::Mix(mix_event)) => {
            handle_mix_event(swarm, known_mix_relays, known_mix_routing_keys, mailbox_store, mix_secret, mix_public, mix_forward_tx, events, mix_event);
        }

        SwarmEvent::Behaviour(BehaviourEvent::LedgerGossip(gossip_event)) => {
            handle_ledger_gossip_event(swarm, chain_store, mempool, mining, known_mix_routing_keys, events, gossip_event);
        }

        SwarmEvent::Behaviour(BehaviourEvent::LedgerSync(sync_event)) => {
            handle_ledger_sync_event(swarm, chain_store, pending, mempool, mining, events, sync_event);
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

/// Received a raw Sphinx packet, whether from the original sender (this
/// node is the first hop) or from a previous relay. Peeling with this
/// node's own mix routing secret reveals only what this one layer was
/// encrypted to say — either "forward this (different, re-encrypted)
/// packet to whoever answers to this address next" or "you're the last
/// hop, here's the payload" — never anything about hops further along the
/// path. Deterministic immediate-forward for now (no Poisson/cover
/// traffic scheduling yet — that's the next phase); this is the hop-by-hop
/// wire mechanics being proven correct in isolation first.
#[allow(clippy::too_many_arguments)]
fn handle_mix_event(
    swarm: &mut Swarm<Behaviour>,
    known_mix_relays: &HashMap<NodeAddressBytes, PeerId>,
    known_mix_routing_keys: &mut HashMap<PeerId, PublicKey>,
    mailbox_store: &mut mailbox::MailboxStore,
    mix_secret: &StaticSecret,
    mix_public: PublicKey,
    mix_forward_tx: &mpsc::UnboundedSender<(PeerId, MixMessage)>,
    events: &mpsc::UnboundedSender<P2pEvent>,
    event: request_response::Event<MixMessage, ()>,
) {
    match event {
        request_response::Event::Message { peer, message, .. } => match message {
            request_response::Message::Request { request, channel, .. } => {
                // The mix protocol's "response" carries no information of
                // its own, same as the envelope protocol's — it exists
                // only so the sending peer's outbound request resolves.
                let _ = swarm.behaviour_mut().mix.send_response(channel, ());

                // Learned organically from real traffic, not a directory
                // lookup — see `MixMessage`'s own doc comment. Recorded
                // even if peeling below fails: knowing this peer's routing
                // key is still useful for future traffic regardless of
                // whether this one packet was corrupt or not meant for us.
                known_mix_routing_keys.insert(peer, PublicKey::from(request.sender_routing_public_key));

                let Ok(packet) = SphinxPacket::from_bytes(&request.packet_bytes) else {
                    let _ = events.send(P2pEvent::MixForwardFailed { reason: "malformed Sphinx packet".into() });
                    return;
                };
                match mix::peel(packet, mix_secret) {
                    Ok(mix::PeelOutcome::Forward { next_hop_packet, next_hop_address, delay }) => {
                        match known_mix_relays.get(&next_hop_address) {
                            Some(&next_peer) => {
                                // Loopix mixing: actually wait out the
                                // delay this packet's own Sphinx header
                                // specified before sending it onward,
                                // rather than forwarding the instant it
                                // arrives — otherwise "the header carries a
                                // delay" would be true but meaningless. The
                                // event loop itself must never block on
                                // this, so the wait happens on its own
                                // task, reporting back over a channel once
                                // it's actually time to send.
                                let forward_tx = mix_forward_tx.clone();
                                let message = MixMessage {
                                    packet_bytes: next_hop_packet.to_bytes(),
                                    sender_routing_public_key: mix_public.to_bytes(),
                                };
                                tokio::spawn(async move {
                                    tokio::time::sleep(delay.to_duration()).await;
                                    let _ = forward_tx.send((next_peer, message));
                                });
                            }
                            None => {
                                let _ = events.send(P2pEvent::MixForwardFailed {
                                    reason: "next hop is not a currently reachable peer".into(),
                                });
                            }
                        }
                    }
                    Ok(mix::PeelOutcome::Final { payload, .. }) => {
                        if mix::is_dummy_payload(&payload) {
                            // Loop/cover traffic — either this node's own,
                            // having made it back around, or a peer's,
                            // sent through this node as an intermediate
                            // hop earlier in its path. Either way it was
                            // never meant to be surfaced as a real
                            // message.
                        } else if let Some(deposit) = parse_mailbox_deposit(&payload) {
                            // A mailbox deposit routed to this node as its
                            // final hop — this node is the caching relay
                            // now, never told (and structurally unable to
                            // learn) who the real recipient is, only the
                            // unlinkable tag they'll look it up under
                            // later. Silently dropped if it fails
                            // validation: replying with a rejection reason
                            // would need routing a response back to an
                            // anonymous sender, which this crate doesn't
                            // yet support (no SURB use yet) — and would
                            // arguably leak more than it's worth to a
                            // sender who can already tell locally whether
                            // their own stamp/timestamp were valid before
                            // ever sending.
                            if mailbox::validate(&deposit, mailbox::now_unix()).is_ok()
                                && mailbox_store.accept(deposit).is_ok()
                            {
                                let _ = events.send(P2pEvent::MailboxDepositStored);
                            }
                        } else if let Some((tag, surb)) = parse_mailbox_query(&payload) {
                            // This node is being asked, as an anonymous
                            // mix relay, whether it's holding anything
                            // for `tag` — never anything about who's
                            // asking, only the SURB needed to answer them.
                            // Silence (not answering at all) is the
                            // correct response to "nothing queued," the
                            // same way a piece of dummy traffic gets no
                            // reply either — answering only real matches
                            // is what keeps a query cheap for the network
                            // regardless of whether it turns anything up.
                            if let Ok(deposits) = mailbox_store.for_tag(&tag) {
                                if let Some(oldest) = deposits.into_iter().next() {
                                    if let Ok((reply_packet, next_hop_address)) =
                                        mix::use_surb(surb, &frame_mailbox_reply(&oldest.envelope))
                                    {
                                        if let Some(&next_peer) = known_mix_relays.get(&next_hop_address) {
                                            let message = MixMessage {
                                                packet_bytes: reply_packet.to_bytes(),
                                                sender_routing_public_key: mix_public.to_bytes(),
                                            };
                                            swarm.behaviour_mut().mix.send_request(&next_peer, message);
                                        }
                                    }
                                }
                            }
                        } else if let Some(envelope) = parse_mailbox_reply(&payload) {
                            // This node was the one asking — a relay
                            // routed a queued envelope back through the
                            // SURB this node itself built and sent out
                            // with its own query.
                            let _ = events.send(P2pEvent::MailboxEnvelopeRetrieved { envelope });
                        } else {
                            let _ = events.send(P2pEvent::MixPacketArrived { payload });
                        }
                    }
                    Err(err) => {
                        let _ = events.send(P2pEvent::MixForwardFailed { reason: err.to_string() });
                    }
                }
            }
            request_response::Message::Response { .. } => {}
        },
        request_response::Event::OutboundFailure { error, .. } => {
            let _ = events.send(P2pEvent::MixForwardFailed { reason: error.to_string() });
        }
        _ => {}
    }
}

/// A single leading byte distinguishing a mailbox deposit's framed
/// payload from anything else a Sphinx packet's final hop might carry —
/// `mix.rs` itself stays agnostic of what a payload means (only
/// `is_dummy_payload`'s prefix check is its own concern); this framing
/// belongs here, in the one place that already knows about both `mix.rs`
/// and `mailbox.rs`. Never collides with `DUMMY_PAYLOAD_MARKER` (an ASCII
/// string) since this is a single non-ASCII-leading byte.
const MIX_PAYLOAD_DEPOSIT_TAG: u8 = 0x01;

fn frame_mailbox_deposit(deposit: &mailbox::MailboxDeposit) -> Option<Vec<u8>> {
    let mut out = vec![MIX_PAYLOAD_DEPOSIT_TAG];
    out.extend(bincode::serialize(deposit).ok()?);
    Some(out)
}

fn parse_mailbox_deposit(payload: &[u8]) -> Option<mailbox::MailboxDeposit> {
    let (&tag, rest) = payload.split_first()?;
    if tag != MIX_PAYLOAD_DEPOSIT_TAG {
        return None;
    }
    bincode::deserialize(rest).ok()
}

/// A mailbox *retrieval query* — "does anyone have anything queued under
/// this tag, and if so, please send it back via this SURB." Distinguished
/// from `MIX_PAYLOAD_DEPOSIT_TAG` the same way that is from
/// `DUMMY_PAYLOAD_MARKER`.
const MIX_PAYLOAD_QUERY_TAG: u8 = 0x02;

fn frame_mailbox_query(tag: &[u8; mailbox::MAILBOX_TAG_LEN], surb: &SURB) -> Vec<u8> {
    let mut out = vec![MIX_PAYLOAD_QUERY_TAG];
    out.extend_from_slice(tag);
    out.extend(surb.to_bytes());
    out
}

fn parse_mailbox_query(payload: &[u8]) -> Option<([u8; mailbox::MAILBOX_TAG_LEN], SURB)> {
    let (&marker, rest) = payload.split_first()?;
    if marker != MIX_PAYLOAD_QUERY_TAG {
        return None;
    }
    if rest.len() <= mailbox::MAILBOX_TAG_LEN {
        return None;
    }
    let (tag_bytes, surb_bytes) = rest.split_at(mailbox::MAILBOX_TAG_LEN);
    let tag: [u8; mailbox::MAILBOX_TAG_LEN] = tag_bytes.try_into().ok()?;
    let surb = SURB::from_bytes(surb_bytes).ok()?;
    Some((tag, surb))
}

/// A mailbox query's *answer* — one queued envelope, routed back through
/// the querier's own SURB. Framed the same way a deposit or a query is,
/// so the querier's own final-hop parsing (which sees exactly the same
/// kind of raw payload any other final hop does — a reply arrives as an
/// ordinary Sphinx packet, not through some separate channel) can tell it
/// apart from a fresh deposit/query someone is sending *to* this node.
const MIX_PAYLOAD_REPLY_TAG: u8 = 0x03;

fn frame_mailbox_reply(envelope: &[u8]) -> Vec<u8> {
    let mut out = vec![MIX_PAYLOAD_REPLY_TAG];
    out.extend_from_slice(envelope);
    out
}

fn parse_mailbox_reply(payload: &[u8]) -> Option<Vec<u8>> {
    let (&tag, rest) = payload.split_first()?;
    if tag != MIX_PAYLOAD_REPLY_TAG {
        return None;
    }
    Some(rest.to_vec())
}

/// Picks a peer usable as a mix hop *this node itself is originating a
/// packet through* — meaning both currently connected (`known_mix_relays`,
/// so a hop can actually be dialed) and a peer whose routing key this node
/// has actually learned (`known_mix_routing_keys`, needed to build the
/// packet's Diffie-Hellman layer at all). Shared by every place this node
/// builds a brand new packet (deposits, retrieval queries, dummy traffic)
/// — forwarding an already-built packet is a different, simpler lookup
/// (`known_mix_relays` alone; see `handle_mix_event`'s `Forward` arm).
fn pick_mix_relay(
    known_mix_relays: &HashMap<NodeAddressBytes, PeerId>,
    known_mix_routing_keys: &HashMap<PeerId, PublicKey>,
    rng: &mut impl Rng,
) -> Option<(NodeAddressBytes, PeerId, PublicKey)> {
    known_mix_relays
        .iter()
        .filter_map(|(&address, &peer)| known_mix_routing_keys.get(&peer).map(|&key| (address, peer, key)))
        .choose(rng)
}

/// Routes an already-stamped `deposit` into the mix, addressed so that
/// whichever relay ends up as the path's final hop stores it — see
/// `Command::DepositToMailbox`'s own doc comment. Picks a relay via
/// `pick_mix_relay`, the same selection `emit_dummy_mix_traffic` and
/// `send_mailbox_retrieval_query` also use; a real deposit deserves the
/// same "only route through what's actually usable right now" discipline
/// dummy traffic already follows, not a separate, laxer rule. A
/// single-hop path (straight to the chosen relay) for now — multi-hop
/// path selection through peers this node isn't itself directly
/// connected to is deferred, same as it is for dummy traffic, to
/// whenever a real relay-liveness story beyond direct connectivity
/// exists.
fn send_mailbox_deposit(
    swarm: &mut Swarm<Behaviour>,
    known_mix_relays: &HashMap<NodeAddressBytes, PeerId>,
    known_mix_routing_keys: &HashMap<PeerId, PublicKey>,
    mix_public: PublicKey,
    deposit: mailbox::MailboxDeposit,
    events: &mpsc::UnboundedSender<P2pEvent>,
) {
    let mut rng = rand::thread_rng();
    let Some((relay_address, relay_peer, relay_public)) = pick_mix_relay(known_mix_relays, known_mix_routing_keys, &mut rng) else {
        let _ = events.send(P2pEvent::MixForwardFailed { reason: "no mix relay is currently known and reachable".into() });
        return;
    };

    let Some(payload) = frame_mailbox_deposit(&deposit) else {
        let _ = events.send(P2pEvent::MixForwardFailed { reason: "failed to encode the mailbox deposit".into() });
        return;
    };
    let path = [mix::MixHop { address: relay_address, public_key: relay_public }];
    // The destination address/identifier a Sphinx packet's final hop
    // reports back are meaningless here — this node never asks the relay
    // to reply, and `mailbox::MailboxDeposit` already carries its own tag
    // for later retrieval — so both are simply random filler.
    let destination_address = DestinationAddressBytes::from_bytes(rng.gen());
    let identifier = rng.gen();
    let Ok(packet) = mix::build_packet(&payload, &path, destination_address, identifier, MIX_DUMMY_TRAFFIC_HOP_DELAY) else {
        let _ = events.send(P2pEvent::MixForwardFailed { reason: "failed to build the deposit's Sphinx packet".into() });
        return;
    };

    let message = MixMessage { packet_bytes: packet.to_bytes(), sender_routing_public_key: mix_public.to_bytes() };
    swarm.behaviour_mut().mix.send_request(&relay_peer, message);
}

/// Asks whichever relay `pick_mix_relay` chooses whether anything is
/// currently queued under `shared_material`'s current-epoch tag, with a
/// SURB attached so the relay can answer without learning who's asking.
/// The SURB's own "route" is just this node's own address — a single hop
/// straight back — so answering costs the relay nothing beyond one more
/// `send_request`, the same as it already pays for a forwarded packet.
/// See `Command::RetrieveFromMailbox`'s own doc comment for the full
/// round trip this kicks off.
fn send_mailbox_retrieval_query(
    swarm: &mut Swarm<Behaviour>,
    known_mix_relays: &HashMap<NodeAddressBytes, PeerId>,
    known_mix_routing_keys: &HashMap<PeerId, PublicKey>,
    local_peer_id: PeerId,
    mix_public: PublicKey,
    shared_material: Vec<u8>,
    events: &mpsc::UnboundedSender<P2pEvent>,
) {
    let mut rng = rand::thread_rng();
    let Some((relay_address, relay_peer, relay_public)) = pick_mix_relay(known_mix_relays, known_mix_routing_keys, &mut rng) else {
        let _ = events.send(P2pEvent::MixForwardFailed { reason: "no mix relay is currently known and reachable".into() });
        return;
    };

    let tag = mailbox::mailbox_tag(&shared_material, mailbox::epoch_for(mailbox::now_unix()));

    let self_hop = mix::MixHop { address: mix::node_address_for(&local_peer_id.to_bytes()), public_key: mix_public };
    let Ok(surb) = mix::build_surb(
        &[self_hop],
        DestinationAddressBytes::from_bytes(rng.gen()),
        rng.gen(),
        MIX_DUMMY_TRAFFIC_HOP_DELAY,
    ) else {
        let _ = events.send(P2pEvent::MixForwardFailed { reason: "failed to build the retrieval query's SURB".into() });
        return;
    };

    let payload = frame_mailbox_query(&tag, &surb);
    let path = [mix::MixHop { address: relay_address, public_key: relay_public }];
    let Ok(packet) = mix::build_packet(&payload, &path, DestinationAddressBytes::from_bytes(rng.gen()), rng.gen(), MIX_DUMMY_TRAFFIC_HOP_DELAY) else {
        let _ = events.send(P2pEvent::MixForwardFailed { reason: "failed to build the retrieval query's Sphinx packet".into() });
        return;
    };

    let message = MixMessage { packet_bytes: packet.to_bytes(), sender_routing_public_key: mix_public.to_bytes() };
    swarm.behaviour_mut().mix.send_request(&relay_peer, message);
}

/// Sends one piece of Loopix-style dummy traffic — indistinguishable on
/// the wire from a real deposit/query — toward a randomly chosen
/// currently-connected mix peer whose routing key this node has already
/// learned (see `MixMessage`). A no-op if there isn't at least one such
/// peer yet (e.g. right after startup, or a node with mix relaying
/// disabled). Chooses between the two shapes Loopix itself defines:
/// **drop cover** (a single hop, addressed directly to the chosen peer —
/// they are the final hop and simply discard it) and **loop** (two hops,
/// out through the chosen peer and back to this node itself as the final
/// hop — the same self-monitoring traffic Loopix's own design describes).
/// Failures are deliberately not surfaced as `P2pEvent::MixForwardFailed`:
/// a dropped piece of cover traffic isn't a failure worth telling the app
/// about, only a real deposit/query failing to route is.
fn emit_dummy_mix_traffic(
    swarm: &mut Swarm<Behaviour>,
    known_mix_relays: &HashMap<NodeAddressBytes, PeerId>,
    known_mix_routing_keys: &HashMap<PeerId, PublicKey>,
    local_peer_id: PeerId,
    mix_public: PublicKey,
) {
    let mut rng = rand::thread_rng();
    let Some((relay_address, relay_peer, relay_public)) = pick_mix_relay(known_mix_relays, known_mix_routing_keys, &mut rng) else {
        return;
    };
    let relay_hop = mix::MixHop { address: relay_address, public_key: relay_public };

    let path = if rng.gen_bool(0.5) {
        // Drop cover: one hop, the chosen peer is the destination.
        vec![relay_hop]
    } else {
        // Loop: two hops, back to this node itself.
        let self_hop = mix::MixHop { address: mix::node_address_for(&local_peer_id.to_bytes()), public_key: mix_public };
        vec![relay_hop, self_hop]
    };

    let destination_address = DestinationAddressBytes::from_bytes(rng.gen());
    let identifier = rng.gen();
    let Ok(packet) = mix::build_dummy_packet(&path, destination_address, identifier, MIX_DUMMY_TRAFFIC_HOP_DELAY) else {
        return;
    };
    let message = MixMessage { packet_bytes: packet.to_bytes(), sender_routing_public_key: mix_public.to_bytes() };
    swarm.behaviour_mut().mix.send_request(&relay_peer, message);
}

/// New blocks/claims arriving over gossip. Deliberately does **not**
/// distinguish "valid" from "invalid" at the gossip layer itself (no
/// custom `report_message_validation_result` hookup) — every receiving
/// node still independently validates through the exact same
/// `ChainStore::try_apply`/`verify_self_contained` any locally-submitted
/// block or claim goes through before accepting it, so a bad message
/// propagating a hop further than strictly necessary wastes a little
/// bandwidth but can never corrupt anyone's actual chain state. Rejecting
/// invalid messages at the gossip layer itself (so they stop propagating
/// immediately, rather than merely being ignored on arrival) is a real
/// hardening opportunity, deferred to Phase 5.
#[allow(clippy::too_many_arguments)]
fn handle_ledger_gossip_event(
    swarm: &mut Swarm<Behaviour>,
    chain_store: &mut ChainStore,
    mempool: &mut HashMap<Hash32, Transaction>,
    mining: &mut Mining,
    known_mix_routing_keys: &mut HashMap<PeerId, PublicKey>,
    events: &mpsc::UnboundedSender<P2pEvent>,
    event: gossipsub::Event,
) {
    let gossipsub::Event::Message { message, .. } = event else {
        return;
    };

    if message.topic == ledger::blocks_topic().hash() {
        if let Ok(block) = bincode::deserialize::<Block>(&message.data) {
            apply_and_broadcast_block(swarm, chain_store, mempool, mining, events, block);
        }
    } else if message.topic == ledger::txs_topic().hash() {
        if let Ok(transaction) = bincode::deserialize::<Transaction>(&message.data) {
            if transaction.verify_self_contained().is_ok() {
                mempool.insert(transaction.id(), transaction);
                // A newly arrived claim might be includable in the block
                // this node is already grinding on — restart so it isn't
                // stuck waiting for the *next* attempt to notice it.
                mining.restart_if_active(chain_store, mempool);
            }
        }
    } else if message.topic == behaviour::mix_relay_directory_topic().hash() {
        if let Some(peer) = record_mix_relay_announcement(known_mix_routing_keys, message.source, &message.data) {
            let _ = events.send(P2pEvent::MixRelayDiscovered { peer });
        }
    }
}

/// Parses a `mix_relay_directory_topic()` gossip message and records the
/// announcing peer's routing key, if the message is well-formed and
/// actually has a signed source (gossipsub's `Signed` authenticity mode,
/// already used by this crate for every topic, guarantees the latter).
/// Pure and swarm-free on purpose — separated out from
/// `handle_ledger_gossip_event` specifically so this parsing/bookkeeping
/// logic is unit-testable without standing up a real swarm. Returns the
/// peer only when this was a genuinely new discovery (an update to an
/// already-known peer's key returns `None`), which is what decides
/// whether `P2pEvent::MixRelayDiscovered` fires.
fn record_mix_relay_announcement(
    known_mix_routing_keys: &mut HashMap<PeerId, PublicKey>,
    source: Option<PeerId>,
    data: &[u8],
) -> Option<PeerId> {
    let peer = source?;
    let announcement = bincode::deserialize::<behaviour::MixRelayAnnouncement>(data).ok()?;
    let key = PublicKey::from(announcement.routing_public_key);
    match known_mix_routing_keys.insert(peer, key) {
        Some(_) => None,
        None => Some(peer),
    }
}

fn handle_ledger_sync_event(
    swarm: &mut Swarm<Behaviour>,
    chain_store: &mut ChainStore,
    pending: &mut Pending,
    mempool: &mut HashMap<Hash32, Transaction>,
    mining: &mut Mining,
    events: &mpsc::UnboundedSender<P2pEvent>,
    event: request_response::Event<ChainSyncRequest, ChainSyncResponse>,
) {
    match event {
        request_response::Event::Message { message, .. } => match message {
            request_response::Message::Request { request, channel, .. } => {
                let response = build_chain_sync_response(chain_store, request);
                let _ = swarm.behaviour_mut().ledger_sync.send_response(channel, response);
            }
            request_response::Message::Response { request_id, response } => {
                handle_chain_sync_response(swarm, chain_store, pending, mempool, mining, events, request_id, response);
            }
        },
        request_response::Event::OutboundFailure { request_id, error, .. } => {
            if let Some(peer) = pending.chain_sync_tip.remove(&request_id).or_else(|| pending.chain_sync_blocks.remove(&request_id)) {
                let _ = events.send(P2pEvent::ChainSyncFailed { peer, reason: error.to_string() });
            }
        }
        _ => {}
    }
}

fn build_chain_sync_response(chain_store: &ChainStore, request: ChainSyncRequest) -> ChainSyncResponse {
    match request {
        ChainSyncRequest::GetTip => ChainSyncResponse::Tip {
            height: chain_store.tip_height(),
            hash: *chain_store.tip_hash().as_bytes(),
            cumulative_work: chain_store.tip_cumulative_work(),
        },
        ChainSyncRequest::GetBlocks { from_height, count } => {
            let count = count.min(ledger::MAX_SYNC_BATCH);
            let mut blocks = Vec::with_capacity(count as usize);
            for height in from_height..from_height.saturating_add(count as u64) {
                match chain_store.canonical_block_at(height) {
                    Some(block) => blocks.push(block.clone()),
                    // A gap (already pruned, or past our own tip) means
                    // this batch can't be served in full — the requester
                    // falls back to `GetUsernameOwnerSnapshot` for
                    // anything this old instead of getting a silently
                    // incomplete answer.
                    None => return ChainSyncResponse::NotAvailable,
                }
            }
            ChainSyncResponse::Blocks(blocks)
        }
        // Full independent header-chain reverification arbitrarily far
        // back (beyond what any peer's retained block bodies cover) is a
        // real, deliberate Phase 3 boundary — this crate's own metadata
        // doesn't yet retain every header field a from-scratch PoW replay
        // would need (see `spiritchat_ledger_core::chain_state::BlockMeta`).
        // A brand-new node instead trusts `GetUsernameOwnerSnapshot` for
        // history older than what `GetBlocks` can serve, and fully
        // verifies everything within the retained window — the same
        // trust-on-first-use model already documented for bootstrapping.
        ChainSyncRequest::GetHeaders { .. } => ChainSyncResponse::NotAvailable,
        ChainSyncRequest::GetUsernameOwnerSnapshot => {
            ChainSyncResponse::UsernameOwnerSnapshot(chain_store.checkpoint_at_tip())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_chain_sync_response(
    swarm: &mut Swarm<Behaviour>,
    chain_store: &mut ChainStore,
    pending: &mut Pending,
    mempool: &mut HashMap<Hash32, Transaction>,
    mining: &mut Mining,
    events: &mpsc::UnboundedSender<P2pEvent>,
    request_id: OutboundRequestId,
    response: ChainSyncResponse,
) {
    if let Some(peer) = pending.chain_sync_tip.remove(&request_id) {
        let ChainSyncResponse::Tip { height, cumulative_work, .. } = response else {
            let _ = events.send(P2pEvent::ChainSyncFailed { peer, reason: "peer gave a malformed tip response".to_string() });
            return;
        };
        if cumulative_work <= chain_store.tip_cumulative_work() {
            // Our own chain is already at least as heavy — nothing to
            // catch up on.
            let _ = events.send(P2pEvent::ChainSyncCompleted { height: chain_store.tip_height() });
            return;
        }
        let from_height = chain_store.tip_height() + 1;
        let count = (height - chain_store.tip_height()).min(ledger::MAX_SYNC_BATCH as u64) as u32;
        let request_id = swarm.behaviour_mut().ledger_sync.send_request(&peer, ChainSyncRequest::GetBlocks { from_height, count });
        pending.chain_sync_blocks.insert(request_id, peer);
        return;
    }

    if let Some(peer) = pending.chain_sync_blocks.remove(&request_id) {
        match response {
            ChainSyncResponse::Blocks(blocks) => {
                let tip_before = chain_store.tip_hash();
                for block in blocks {
                    // Applied one at a time, in the order the peer sent
                    // them (ascending height) — `try_apply` rejects a
                    // block whose parent it hasn't seen yet, so an
                    // out-of-order or gappy batch simply stops making
                    // progress rather than corrupting anything.
                    for tx in &block.transactions {
                        mempool.remove(&tx.id());
                    }
                    if let Err(err) = chain_store.try_apply(block, now_unix()) {
                        let _ = events.send(P2pEvent::ChainSyncFailed { peer, reason: err.to_string() });
                        return;
                    }
                }
                let _ = events.send(P2pEvent::ChainTipChanged {
                    height: chain_store.tip_height(),
                    hash: format!("{}", chain_store.tip_hash()),
                });
                let _ = events.send(P2pEvent::ChainSyncCompleted { height: chain_store.tip_height() });
                if chain_store.tip_hash() != tip_before {
                    mining.restart_if_active(chain_store, mempool);
                }
            }
            _ => {
                let _ = events.send(P2pEvent::ChainSyncFailed {
                    peer,
                    reason: "peer had no blocks available for this range".to_string(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_well_formed_announcement_from_a_signed_source_is_recorded_as_a_new_discovery() {
        let mut known = HashMap::new();
        let peer = PeerId::random();
        let (_secret, public) = mix::routing_keypair_from_seed(&[3u8; 32]);
        let bytes = bincode::serialize(&behaviour::MixRelayAnnouncement { routing_public_key: public.to_bytes() }).unwrap();

        let discovered = record_mix_relay_announcement(&mut known, Some(peer), &bytes);

        assert_eq!(discovered, Some(peer));
        assert_eq!(known.get(&peer), Some(&public));
    }

    #[test]
    fn a_second_announcement_from_an_already_known_peer_is_not_reported_as_a_new_discovery() {
        let mut known = HashMap::new();
        let peer = PeerId::random();
        let (_secret, public) = mix::routing_keypair_from_seed(&[3u8; 32]);
        let bytes = bincode::serialize(&behaviour::MixRelayAnnouncement { routing_public_key: public.to_bytes() }).unwrap();

        assert!(record_mix_relay_announcement(&mut known, Some(peer), &bytes).is_some());
        assert_eq!(record_mix_relay_announcement(&mut known, Some(peer), &bytes), None);
    }

    #[test]
    fn an_announcement_with_no_signed_source_is_ignored() {
        let mut known = HashMap::new();
        let (_secret, public) = mix::routing_keypair_from_seed(&[3u8; 32]);
        let bytes = bincode::serialize(&behaviour::MixRelayAnnouncement { routing_public_key: public.to_bytes() }).unwrap();

        assert_eq!(record_mix_relay_announcement(&mut known, None, &bytes), None);
        assert!(known.is_empty());
    }

    #[test]
    fn malformed_announcement_bytes_are_ignored_rather_than_panicking() {
        let mut known = HashMap::new();
        let peer = PeerId::random();

        assert_eq!(record_mix_relay_announcement(&mut known, Some(peer), b"not a real announcement"), None);
        assert!(known.is_empty());
    }
}
