//! UniFFI bindings for `spiritchat-crypto-core`, generating one reviewed
//! Kotlin binding (for Android, loaded via JNI) and one Swift binding (for
//! iOS, loaded via the C ABI) from the same interface definition. This
//! crate holds no cryptographic logic of its own — every operation here is
//! a thin, byte-oriented wrapper around the core crate — so the two mobile
//! apps never re-implement (or subtly diverge on) the handshake or ratchet.

mod agreement;
mod contact_card;
mod error;
mod handshake;
mod identity;
mod prekeys;
mod ratchet;

pub use agreement::FfiAgreementKey;
pub use contact_card::FfiContactCard;
pub use error::FfiError;
pub use handshake::{x3dh_initiate, x3dh_respond, FfiHandshakeResult};
pub use identity::FfiIdentity;
pub use prekeys::FfiPrekeyStore;
pub use ratchet::FfiRatchet;

uniffi::setup_scaffolding!();
