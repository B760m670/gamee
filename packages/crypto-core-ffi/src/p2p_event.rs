//! Mirrors `spiritchat_p2p_core::P2pEvent` for the FFI boundary. PeerIds
//! and Multiaddrs cross as their string forms (`PeerId`/`Multiaddr` both
//! have stable, parseable `Display` impls) rather than exposing libp2p's
//! own types to Kotlin/Swift — consistent with how the rest of this crate
//! stays byte/string-oriented at the boundary instead of leaking the
//! underlying Rust crates' types.

use spiritchat_p2p_core::P2pEvent;

#[derive(uniffi::Enum)]
pub enum FfiP2pEvent {
    ListeningOn { address: String },
    PeerDiscoveredLocally { peer_id: String },
    PeerConnected { peer_id: String },
    PeerIdentified { peer_id: String },
    PeerDisconnected { peer_id: String },
    DialFailed { peer_id: Option<String>, reason: String },
    RelayReservationFailed { reason: String },
    EnvelopeReceived { from_peer_id: String, bytes: Vec<u8> },
    EnvelopeDelivered { to_peer_id: String },
    EnvelopeDeliveryFailed { to_peer_id: String, reason: String },
    PeerAddressesResolved { peer_id: String, addresses: Vec<String> },
    PeerAddressResolutionFailed { peer_id: String },
    AddressesAnnounced,
    AddressAnnouncementFailed { reason: String },
    BlobFetched { peer_id: String, id: Vec<u8>, bytes: Vec<u8> },
    BlobFetchFailed { peer_id: String, id: Vec<u8>, reason: String },
    UsernameResolved { username: String, claim: Vec<u8> },
    UsernameResolutionFailed { username: String },
    UsernameAnnounced { username: String },
    UsernameAnnouncementFailed { username: String, reason: String },
    ChainTipChanged { height: u64, hash: String },
    LedgerSubmissionRejected { reason: String },
    UsernameOwnerResolved { username: String, owner_public_key: Vec<u8>, claimed_at_height: u64 },
    UsernameOwnerNotFound { username: String },
    ChainSyncCompleted { height: u64 },
    ChainSyncFailed { peer_id: String, reason: String },
    NewBlockMined { height: u64 },
    MixPacketArrived { payload: Vec<u8> },
    MixForwardFailed { reason: String },
    MixRelayDiscovered { peer_id: String },
}

impl From<P2pEvent> for FfiP2pEvent {
    fn from(event: P2pEvent) -> Self {
        match event {
            P2pEvent::ListeningOn(address) => Self::ListeningOn { address: address.to_string() },
            P2pEvent::PeerDiscoveredLocally(peer) => {
                Self::PeerDiscoveredLocally { peer_id: peer.to_string() }
            }
            P2pEvent::PeerConnected(peer) => Self::PeerConnected { peer_id: peer.to_string() },
            P2pEvent::PeerIdentified(peer) => Self::PeerIdentified { peer_id: peer.to_string() },
            P2pEvent::PeerDisconnected(peer) => Self::PeerDisconnected { peer_id: peer.to_string() },
            P2pEvent::DialFailed { peer, reason } => {
                Self::DialFailed { peer_id: peer.map(|p| p.to_string()), reason }
            }
            P2pEvent::RelayReservationFailed { reason } => Self::RelayReservationFailed { reason },
            P2pEvent::EnvelopeReceived { from, bytes } => {
                Self::EnvelopeReceived { from_peer_id: from.to_string(), bytes }
            }
            P2pEvent::EnvelopeDelivered { to } => Self::EnvelopeDelivered { to_peer_id: to.to_string() },
            P2pEvent::EnvelopeDeliveryFailed { to, reason } => {
                Self::EnvelopeDeliveryFailed { to_peer_id: to.to_string(), reason }
            }
            P2pEvent::PeerAddressesResolved { peer, addresses } => Self::PeerAddressesResolved {
                peer_id: peer.to_string(),
                addresses: addresses.iter().map(ToString::to_string).collect(),
            },
            P2pEvent::PeerAddressResolutionFailed { peer } => {
                Self::PeerAddressResolutionFailed { peer_id: peer.to_string() }
            }
            P2pEvent::AddressesAnnounced => Self::AddressesAnnounced,
            P2pEvent::AddressAnnouncementFailed { reason } => Self::AddressAnnouncementFailed { reason },
            P2pEvent::BlobFetched { peer, id, bytes } => {
                Self::BlobFetched { peer_id: peer.to_string(), id, bytes }
            }
            P2pEvent::BlobFetchFailed { peer, id, reason } => {
                Self::BlobFetchFailed { peer_id: peer.to_string(), id, reason }
            }
            P2pEvent::UsernameResolved { username, claim } => {
                Self::UsernameResolved { username, claim }
            }
            P2pEvent::UsernameResolutionFailed { username } => {
                Self::UsernameResolutionFailed { username }
            }
            P2pEvent::UsernameAnnounced { username } => Self::UsernameAnnounced { username },
            P2pEvent::UsernameAnnouncementFailed { username, reason } => {
                Self::UsernameAnnouncementFailed { username, reason }
            }
            P2pEvent::ChainTipChanged { height, hash } => Self::ChainTipChanged { height, hash },
            P2pEvent::LedgerSubmissionRejected { reason } => Self::LedgerSubmissionRejected { reason },
            P2pEvent::UsernameOwnerResolved { username, owner_public_key, claimed_at_height } => {
                Self::UsernameOwnerResolved { username, owner_public_key, claimed_at_height }
            }
            P2pEvent::UsernameOwnerNotFound { username } => Self::UsernameOwnerNotFound { username },
            P2pEvent::ChainSyncCompleted { height } => Self::ChainSyncCompleted { height },
            P2pEvent::ChainSyncFailed { peer, reason } => {
                Self::ChainSyncFailed { peer_id: peer.to_string(), reason }
            }
            P2pEvent::NewBlockMined { height } => Self::NewBlockMined { height },
            P2pEvent::MixPacketArrived { payload } => Self::MixPacketArrived { payload },
            P2pEvent::MixForwardFailed { reason } => Self::MixForwardFailed { reason },
            P2pEvent::MixRelayDiscovered { peer } => Self::MixRelayDiscovered { peer_id: peer.to_string() },
        }
    }
}
