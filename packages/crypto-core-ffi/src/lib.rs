//! UniFFI bindings for `spiritchat-crypto-core`, generating one reviewed
//! Kotlin binding (for Android, loaded via JNI) and one Swift binding (for
//! iOS, loaded via the C ABI) from the same interface definition. This
//! crate holds no cryptographic logic of its own — every operation here is
//! a thin, byte-oriented wrapper around the core crate — so the two mobile
//! apps never re-implement (or subtly diverge on) the handshake or ratchet.

mod agreement;
mod backup;
mod blob;
mod contact_card;
mod error;
mod handshake;
mod identity;
mod ledger;
mod media;
mod mls;
mod p2p_event;
mod p2p_node;
mod prekeys;
mod ratchet;
mod recovery_phrase;
mod sender_key;

pub use agreement::FfiAgreementKey;
pub use blob::blob_content_id;
pub use contact_card::FfiContactCard;
pub use error::FfiError;
pub use handshake::{x3dh_initiate, x3dh_respond, FfiHandshakeResult, FfiRespondResult};
pub use identity::{identity_fingerprint_of_public_key, identity_verify, FfiIdentity};
pub use ledger::ledger_build_username_claim;
pub use p2p_event::FfiP2pEvent;
pub use p2p_node::{p2p_peer_id_from_public_key, FfiP2pNode};
pub use prekeys::FfiPrekeyStore;
pub use ratchet::FfiRatchet;
pub use recovery_phrase::FfiRecoveryPhrase;
pub use sender_key::{FfiSenderKeyReceiverState, FfiSenderKeyState};

uniffi::setup_scaffolding!();
