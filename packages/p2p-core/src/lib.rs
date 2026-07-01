//! Serverless peer-to-peer transport for SpiritChat. This crate operates
//! no infrastructure of its own — no relay, no VPS, no rendezvous server —
//! and never sees plaintext: it only moves the already end-to-end-
//! encrypted envelopes `spiritchat_crypto_core` produces between peers.
//!
//! Peers are found two ways:
//! - **Locally** via mDNS, for devices on the same network.
//! - **Globally** via the public IPFS Kademlia DHT (`bootstrap.rs`) — an
//!   already-running network operated by the wider IPFS/libp2p community,
//!   not by this project. Each peer periodically publishes its own current
//!   addresses into that DHT under a key derived from its own identity
//!   (`rendezvous.rs`), so a contact card only ever needs to carry an
//!   identity key, never a fixed address.
//!
//! Peers behind NAT (the mobile-network default) become reachable through
//! a circuit relay reservation with another participating peer who opted
//! in to relaying (`relay`), then upgrade to a direct connection via hole
//! punching where possible (`dcutr`) — both are libp2p protocols any peer
//! can run, not a role this project's infrastructure fills.

mod behaviour;
mod bootstrap;
mod command;
mod error;
mod event;
mod identity;
mod node;
mod rendezvous;

pub use bootstrap::public_dht_bootstrap_addresses;
pub use command::Command;
pub use error::{P2pError, Result};
pub use event::P2pEvent;
pub use identity::{keypair_from_seed, peer_id_from_seed};
pub use node::P2pNode;

pub use libp2p::{Multiaddr, PeerId};
