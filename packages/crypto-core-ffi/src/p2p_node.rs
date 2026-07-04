//! UniFFI bridge for `spiritchat_p2p_core::P2pNode`.
//!
//! UniFFI's async export mechanism polls futures with its own
//! executor-agnostic scheduler — driven by callbacks from the calling
//! Swift/Kotlin side, not by an ambient Tokio reactor — so anything that
//! touches p2p-core's internal `tokio::spawn`/channels has to explicitly
//! run on a real Tokio runtime owned by this module, not just be awaited
//! inline as if one were already there.

use std::str::FromStr;
use std::sync::{Arc, OnceLock};

use spiritchat_ledger_core::{Block, Transaction};
use spiritchat_p2p_core::{Command, Multiaddr, P2pNode, PeerId};
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, Mutex as AsyncMutex};

use crate::error::{FfiError, FfiResult};
use crate::p2p_event::FfiP2pEvent;

fn ledger_err(reason: impl std::fmt::Display) -> FfiError {
    FfiError::P2p { reason: reason.to_string() }
}

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to start the P2P runtime")
    })
}

/// Derives the libp2p `PeerId` a raw 32-byte Ed25519 public key would
/// produce as a network identity. Lets a resolved username claim (which
/// only carries a public key, see `identity_fingerprint_of_public_key`) be
/// turned into something dialable via `dial`, without the claim needing to
/// separately publish its PeerId.
#[uniffi::export]
pub fn p2p_peer_id_from_public_key(public_key: Vec<u8>) -> FfiResult<String> {
    spiritchat_p2p_core::peer_id_from_public_key(&public_key)
        .map(|peer_id| peer_id.to_string())
        .map_err(|err| FfiError::P2p { reason: err.to_string() })
}

/// The value `deposit_to_mailbox`/`retrieve_from_mailbox` both need as
/// `shared_material` — a stable, order-independent combination of two
/// peers' long-term identity public keys, computable by either side
/// without ever having talked to the other yet. See
/// `spiritchat_p2p_core::shared_material_from_identity_keys`'s own doc
/// comment for why order-independence is what makes this work at all.
#[uniffi::export]
pub fn p2p_mailbox_shared_material(own_identity_public_key: Vec<u8>, peer_identity_public_key: Vec<u8>) -> Vec<u8> {
    spiritchat_p2p_core::shared_material_from_identity_keys(&own_identity_public_key, &peer_identity_public_key)
}

/// A rough, honest lower-bound estimate (in bytes/hour) of this device's
/// own background data cost from Loopix dummy traffic alone, at the real
/// cadence this crate actually uses (`mix_dummy_traffic_interval_secs`,
/// not a duplicated/hardcoded copy of it) — real messaging traffic on top
/// of this is unaccounted for, deliberately, since it varies with actual
/// usage rather than being a constant hum. See
/// `spiritchat_p2p_core::estimated_dummy_traffic_bytes_per_hour`'s own doc
/// comment for exactly what this does and doesn't count. Meant for an
/// honesty-first Settings display, the same spirit as this project's
/// existing ledger/mailbox disclosures, not a hard guarantee.
#[uniffi::export]
pub fn p2p_estimated_mix_dummy_traffic_bytes_per_hour() -> u64 {
    let interval = std::time::Duration::from_secs(spiritchat_p2p_core::mix_dummy_traffic_interval_secs());
    spiritchat_p2p_core::estimated_dummy_traffic_bytes_per_hour(interval)
}

fn parse_peer_id(text: &str) -> FfiResult<PeerId> {
    PeerId::from_str(text).map_err(|err| FfiError::P2p {
        reason: format!("invalid peer id {text:?}: {err}"),
    })
}

fn parse_multiaddr(text: &str) -> FfiResult<Multiaddr> {
    Multiaddr::from_str(text).map_err(|err| FfiError::P2p {
        reason: format!("invalid multiaddr {text:?}: {err}"),
    })
}

fn parse_multiaddrs(texts: &[String]) -> FfiResult<Vec<Multiaddr>> {
    texts.iter().map(|text| parse_multiaddr(text)).collect()
}

#[derive(uniffi::Object)]
pub struct FfiP2pNode {
    command_tx: mpsc::UnboundedSender<Command>,
    // Only ever locked by `next_event` — commands go straight through
    // `command_tx` above without needing this at all, so a `next_event`
    // loop running continuously never blocks a `dial`/`send_envelope` call
    // (or vice versa).
    event_loop: Arc<AsyncMutex<P2pNode>>,
    local_peer_id: PeerId,
}

#[uniffi::export]
impl FfiP2pNode {
    /// Starts a node using `identity_seed` — the same 32-byte seed as
    /// `FfiIdentity::secret_bytes()` — and joins the public IPFS DHT for
    /// global reachability (see `spiritchat_p2p_core::bootstrap`).
    /// `ledger_data_dir` is a real, writable, per-identity directory for
    /// the `@username` ledger's on-disk database — the first state this
    /// crate persists directly rather than leaving to the app (see
    /// `spiritchat_p2p_core::P2pNode::spawn`'s doc comment); the caller
    /// (Swift on iOS) is responsible for picking it, the same way
    /// `BlobStore.swift` already picks its own cache directory.
    #[uniffi::constructor]
    pub fn spawn(identity_seed: Vec<u8>, ledger_data_dir: String) -> FfiResult<Arc<Self>> {
        let seed: [u8; 32] = identity_seed.try_into().map_err(|bytes: Vec<u8>| FfiError::P2p {
            reason: format!("identity seed must be exactly 32 bytes, got {}", bytes.len()),
        })?;

        // P2pNode::spawn's internal tokio::spawn call needs an active
        // runtime context on the calling thread; `.enter()` provides that
        // without needing a full `block_on` since the call itself is
        // synchronous.
        let _guard = runtime().enter();
        let node = P2pNode::spawn(seed, ledger_data_dir.into())?;

        Ok(Arc::new(Self {
            command_tx: node.command_sender(),
            local_peer_id: node.local_peer_id(),
            event_loop: Arc::new(AsyncMutex::new(node)),
        }))
    }

    /// This node's PeerId, base58-encoded (libp2p's standard text form).
    pub fn local_peer_id(&self) -> String {
        self.local_peer_id.to_string()
    }

    pub fn dial(&self, peer_id: String, known_addresses: Vec<String>) -> FfiResult<()> {
        let peer = parse_peer_id(&peer_id)?;
        let known_addresses = parse_multiaddrs(&known_addresses)?;
        self.send(Command::Dial { peer, known_addresses })
    }

    /// Looks up `peer_id`'s currently-advertised addresses in the public
    /// DHT. Answered by a `PeerAddressesResolved`/`PeerAddressResolutionFailed`
    /// event from `next_event`.
    pub fn resolve_peer_addresses(&self, peer_id: String) -> FfiResult<()> {
        let peer = parse_peer_id(&peer_id)?;
        self.send(Command::ResolvePeerAddresses { peer })
    }

    /// Publishes this node's own current addresses to the DHT. Re-run
    /// periodically and after the address set changes (e.g. a new relay
    /// reservation) — DHT records expire and other peers'
    /// `resolve_peer_addresses` calls only find what was last published.
    pub fn announce_addresses(&self, addresses: Vec<String>) -> FfiResult<()> {
        let addresses = parse_multiaddrs(&addresses)?;
        self.send(Command::AnnounceAddresses { addresses })
    }

    /// Sends an already end-to-end-encrypted envelope (an X3DH initial
    /// message or Double Ratchet ciphertext from `spiritchat_crypto_core`)
    /// to a connected peer. Dial first if not already connected — this
    /// does not implicitly dial.
    pub fn send_envelope(&self, peer_id: String, bytes: Vec<u8>) -> FfiResult<()> {
        let peer = parse_peer_id(&peer_id)?;
        self.send(Command::SendEnvelope { to: peer, bytes })
    }

    /// Asks a relay-capable peer, reachable at `relay_address`, to reserve
    /// a slot so this node can be dialed through it while behind NAT. That
    /// peer is just another network participant who opted in to relaying
    /// — not infrastructure this project runs.
    pub fn reserve_relay_slot(&self, relay_address: String) -> FfiResult<()> {
        let relay_address = parse_multiaddr(&relay_address)?;
        self.send(Command::ReserveRelaySlot { relay_address })
    }

    /// Registers `bytes` as a blob this node will serve to any peer that
    /// asks for it by `id` — the content-addressed hosting mechanism for
    /// things like this device's own avatar. There is no server or CDN:
    /// peers fetch it directly from this device over the same connection
    /// used for everything else. Held only in memory; call this again on
    /// every launch (the app already has the bytes on disk).
    pub fn set_local_blob(&self, id: Vec<u8>, bytes: Vec<u8>) -> FfiResult<()> {
        self.send(Command::SetLocalBlob { id, bytes })
    }

    /// Stops serving the blob registered under `id`.
    pub fn clear_local_blob(&self, id: Vec<u8>) -> FfiResult<()> {
        self.send(Command::ClearLocalBlob { id })
    }

    /// Requests the blob `id` from `peer_id`, who must have registered it
    /// via `set_local_blob` (or be caching a copy). Dial first if not
    /// already connected. Answered by a `BlobFetched`/`BlobFetchFailed`
    /// event from `next_event`.
    pub fn fetch_blob(&self, peer_id: String, id: Vec<u8>) -> FfiResult<()> {
        let peer = parse_peer_id(&peer_id)?;
        self.send(Command::FetchBlob { peer, id })
    }

    /// Publishes this node's own current contact card into the public
    /// DHT, keyed by `owner_identity_public_key` — unlike `set_local_blob`
    /// (which needs a live connection to fetch), this survives the
    /// publisher going offline, which is what makes a *first* message to
    /// someone currently offline possible at all. Re-run periodically
    /// (DHT records expire). Answered by a
    /// `ContactCardAnnounced`/`ContactCardAnnouncementFailed` event.
    pub fn announce_contact_card(&self, owner_identity_public_key: Vec<u8>, card: Vec<u8>) -> FfiResult<()> {
        self.send(Command::AnnounceContactCard { owner_identity_public_key, card })
    }

    /// Looks up whatever contact card is currently published for
    /// `owner_identity_public_key` — the fallback once a direct dial/blob
    /// fetch has failed and no ratchet session exists yet. Answered by a
    /// `ContactCardResolved`/`ContactCardResolutionFailed` event.
    pub fn resolve_contact_card(&self, owner_identity_public_key: Vec<u8>) -> FfiResult<()> {
        self.send(Command::ResolveContactCard { owner_identity_public_key })
    }

    /// Publishes `claim` under the DHT key derived from `username`.
    /// `claim` should be self-certifying (e.g. a public key plus a
    /// signature over the username — see `identity_verify`) since this
    /// layer doesn't verify it; a plain DHT can't arbitrate who claimed a
    /// name first, so this doesn't reserve it against a determined second
    /// claimant, only against accidental collisions. Answered by
    /// `UsernameAnnounced`/`UsernameAnnouncementFailed`.
    pub fn announce_username(&self, username: String, claim: Vec<u8>) -> FfiResult<()> {
        self.send(Command::AnnounceUsername { username, claim })
    }

    /// Looks up whatever claim is currently published for `username`.
    /// Answered by `UsernameResolved`/`UsernameResolutionFailed` — verify
    /// the returned claim (`identity_verify`) before trusting it; this
    /// layer only fetches whatever bytes are stored, it doesn't check them.
    pub fn resolve_username(&self, username: String) -> FfiResult<()> {
        self.send(Command::ResolveUsername { username })
    }

    /// Broadcasts an already-signed `@username` claim (see
    /// `ledger_build_username_claim`) to the ledger's mempool topic, so
    /// any connected peer's miner — not just this node's own, if it mines
    /// at all — can pick it up and include it in a block. This alone does
    /// not confirm the name; watch for `ChainTipChanged` events and
    /// re-check via `query_username_owner` to see whether/when it lands.
    pub fn submit_username_claim(&self, transaction_bytes: Vec<u8>) -> FfiResult<()> {
        let transaction: Transaction = bincode::deserialize(&transaction_bytes).map_err(ledger_err)?;
        self.send(Command::SubmitUsernameClaim { transaction })
    }

    /// Submits an already-mined, `bincode`-encoded ledger block — for the
    /// mining loop (see `start_mining`) to publish what it finds, and for
    /// tests. Validates and applies it locally exactly like a block
    /// received over gossip, then gossips it onward.
    pub fn submit_mined_block(&self, block_bytes: Vec<u8>) -> FfiResult<()> {
        let block: Block = bincode::deserialize(&block_bytes).map_err(ledger_err)?;
        self.send(Command::SubmitMinedBlock { block })
    }

    /// Answers from this node's own local materialized ledger state only
    /// — no network round trip, since once synced this node's view of the
    /// chain *is* the answer. Answered by
    /// `UsernameOwnerResolved`/`UsernameOwnerNotFound`.
    pub fn query_username_owner(&self, username: String) -> FfiResult<()> {
        self.send(Command::QueryUsernameOwner { username })
    }

    /// Asks `peer_id` for its current ledger chain tip and, if it's
    /// heavier than this node's own, fetches and applies whatever blocks
    /// are missing. Answered by `ChainSyncCompleted`/`ChainSyncFailed`.
    /// Dial first if not already connected.
    pub fn request_chain_sync(&self, peer_id: String) -> FfiResult<()> {
        let peer = parse_peer_id(&peer_id)?;
        self.send(Command::RequestChainSync { peer })
    }

    /// This node's own current ledger chain tip — needed to build a new
    /// claim's anchor (see `ledger_build_username_claim`). Answered
    /// synchronously (no network round trip) by a `ChainTipChanged` event
    /// on `next_event`.
    pub fn query_chain_tip(&self) -> FfiResult<()> {
        self.send(Command::QueryChainTip)
    }

    /// Starts (or restarts, if already mining) this node's mining loop,
    /// attributing any block it mines to `public_key` — a raw 32-byte
    /// Ed25519 key, not necessarily the same key as this node's own
    /// identity (a miner is an attribution/reward target, not a claimant).
    /// Runs continuously until `stop_mining`. The app is responsible for
    /// deciding *when* mining should run (e.g. only foreground + charging
    /// — see the project plan's `MiningController`); this call does not
    /// itself gate on either. Successful blocks surface as `NewBlockMined`
    /// from `next_event`.
    pub fn start_mining(&self, public_key: Vec<u8>) -> FfiResult<()> {
        let public_key: [u8; 32] = public_key.try_into().map_err(|bytes: Vec<u8>| FfiError::P2p {
            reason: format!("mining public key must be exactly 32 bytes, got {}", bytes.len()),
        })?;
        self.send(Command::StartMining { public_key })
    }

    /// Stops the mining loop started by `start_mining`. A no-op if not
    /// currently mining.
    pub fn stop_mining(&self) -> FfiResult<()> {
        self.send(Command::StopMining)
    }

    /// Deposits `envelope` into whichever mix relay ends up as the
    /// Sphinx path's final hop — serverless offline delivery for when a
    /// direct `send_envelope` genuinely fails. `shared_material` should
    /// come from `p2p_mailbox_shared_material` (or, once a session
    /// exists, an X3DH shared secret) — see that function's own doc
    /// comment. Fire-and-forget: success isn't itself confirmed, only a
    /// routing failure surfaces, as `MixForwardFailed` from `next_event`.
    pub fn deposit_to_mailbox(&self, shared_material: Vec<u8>, envelope: Vec<u8>) -> FfiResult<()> {
        self.send(Command::DepositToMailbox { shared_material, envelope })
    }

    /// Anonymously asks whether anything is currently queued under
    /// `shared_material`'s current-epoch mailbox tag — the exact tag a
    /// matching `deposit_to_mailbox` would have used. A match surfaces as
    /// `MailboxEnvelopeRetrieved` from `next_event`; no reply at all
    /// means nothing is currently queued (there is no explicit "not
    /// found" answer — see `Command::RetrieveFromMailbox`'s own doc
    /// comment for why). Answers with at most one envelope per call; call
    /// again after receiving one to check for another.
    pub fn retrieve_from_mailbox(&self, shared_material: Vec<u8>) -> FfiResult<()> {
        self.send(Command::RetrieveFromMailbox { shared_material })
    }

    /// Broadcasts this node's own Sphinx routing public key so other
    /// nodes can discover it as a usable mix relay — opt-in mix
    /// participation. The app layer decides whether/when to call this
    /// (e.g. gated the same way mining is, on foreground + charging).
    pub fn announce_mix_relay(&self) -> FfiResult<()> {
        self.send(Command::AnnounceMixRelay)
    }

    /// Turns this node's sustained Loopix dummy-traffic generation on or
    /// off — off by default at spawn. Meant to be called alongside
    /// `announce_mix_relay`, gated by the same policy (e.g.
    /// `MixRelayController` on iOS): passive forwarding for others needs
    /// no gate at all (cheap, only happens when asked), but continuously
    /// originating cover/loop packets is an ongoing cost worth an explicit
    /// on/off switch.
    pub fn set_mix_dummy_traffic_active(&self, enabled: bool) -> FfiResult<()> {
        self.send(Command::SetMixDummyTrafficActive { enabled })
    }

    /// Cleanly stops this node — after this, `next_event` returns `None`.
    /// For "sign out": the identity this node was built from is going
    /// away, and a new one needs a new node, not a reused one. Dropping
    /// every `FfiP2pNode` reference alone isn't enough while something
    /// (e.g. an active `next_event` polling loop) still holds one.
    pub fn shutdown(&self) -> FfiResult<()> {
        self.send(Command::Shutdown)
    }

    /// Waits for the next event. Call this in a loop — it never stops on
    /// its own; it only returns `None` after `shutdown()`.
    pub async fn next_event(&self) -> Option<FfiP2pEvent> {
        let event_loop = Arc::clone(&self.event_loop);
        runtime()
            .spawn(async move {
                let mut node = event_loop.lock().await;
                node.next_event().await
            })
            .await
            .expect("the p2p event loop task panicked")
            .map(FfiP2pEvent::from)
    }
}

impl FfiP2pNode {
    fn send(&self, command: Command) -> FfiResult<()> {
        self.command_tx.send(command).map_err(|_| FfiError::P2p {
            reason: "the p2p node has shut down".to_string(),
        })
    }
}
