//! The background task that owns the `Swarm` and drives it: the rest of
//! this crate (and the app above it) only ever talks to a `P2pNode`
//! through `Command`s in and `P2pEvent`s out, never touching the swarm
//! directly — it lives entirely inside `run_event_loop`'s task.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use futures::StreamExt;
use libp2p::identify;
use libp2p::kad::{self, GetRecordOk, PutRecordOk, QueryId, QueryResult, Quorum, Record};
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, OutboundRequestId};
use libp2p::swarm::SwarmEvent;
use libp2p::{gossipsub, noise, tcp, yamux, Multiaddr, PeerId, Swarm};
use spiritchat_ledger_core::block::MAX_TXS_PER_BLOCK;
use spiritchat_ledger_core::difficulty::expand_target;
use spiritchat_ledger_core::{ApplyOutcome, Block, BlockHeader, ChainStore, Hash32, Transaction};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::behaviour::{self, Behaviour, BehaviourEvent, BlobResponse};
use crate::bootstrap;
use crate::command::Command;
use crate::error::{P2pError, Result};
use crate::event::P2pEvent;
use crate::identity;
use crate::ledger::{self, ChainSyncRequest, ChainSyncResponse};
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

        let chain_store = ChainStore::open(&ledger_data_dir)?;

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
        let task = tokio::spawn(run_event_loop(swarm, chain_store, command_rx, event_tx));

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
    mut commands: mpsc::UnboundedReceiver<Command>,
    events: mpsc::UnboundedSender<P2pEvent>,
) {
    let mut pending = Pending::default();
    // Blobs this node currently serves (e.g. its own avatar), set via
    // Command::SetLocalBlob. Lives only in memory for the life of this
    // task — persisting them across restarts, if desired, is the app's
    // job (it already has the bytes; it just re-issues SetLocalBlob).
    let mut local_blobs: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();
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
                    handle_command(&mut swarm, &mut chain_store, &mut pending, &mut local_blobs, &mut mempool, &mut mining, &events, command);
                }
            }
            swarm_event = swarm.select_next_some() => {
                if !dht_ready && matches!(swarm_event, SwarmEvent::ConnectionEstablished { .. }) {
                    dht_ready = true;
                    for command in deferred_commands.drain(..) {
                        handle_command(&mut swarm, &mut chain_store, &mut pending, &mut local_blobs, &mut mempool, &mut mining, &events, command);
                    }
                }
                handle_swarm_event(&mut swarm, &mut chain_store, &mut pending, &local_blobs, &mut mempool, &mut mining, &events, swarm_event);
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

        SwarmEvent::Behaviour(BehaviourEvent::LedgerGossip(gossip_event)) => {
            handle_ledger_gossip_event(swarm, chain_store, mempool, mining, events, gossip_event);
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
fn handle_ledger_gossip_event(
    swarm: &mut Swarm<Behaviour>,
    chain_store: &mut ChainStore,
    mempool: &mut HashMap<Hash32, Transaction>,
    mining: &mut Mining,
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
