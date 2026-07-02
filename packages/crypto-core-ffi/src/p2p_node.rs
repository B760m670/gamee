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

use spiritchat_p2p_core::{Command, Multiaddr, P2pNode, PeerId};
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, Mutex as AsyncMutex};

use crate::error::{FfiError, FfiResult};
use crate::p2p_event::FfiP2pEvent;

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to start the P2P runtime")
    })
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
    #[uniffi::constructor]
    pub fn spawn(identity_seed: Vec<u8>) -> FfiResult<Arc<Self>> {
        let seed: [u8; 32] = identity_seed.try_into().map_err(|bytes: Vec<u8>| FfiError::P2p {
            reason: format!("identity seed must be exactly 32 bytes, got {}", bytes.len()),
        })?;

        // P2pNode::spawn's internal tokio::spawn call needs an active
        // runtime context on the calling thread; `.enter()` provides that
        // without needing a full `block_on` since the call itself is
        // synchronous.
        let _guard = runtime().enter();
        let node = P2pNode::spawn(seed)?;

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

    /// Waits for the next event. Call this in a loop — it never stops on
    /// its own; it only returns `None` if the node has been shut down
    /// (dropping every `FfiP2pNode` reference stops it).
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
