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
