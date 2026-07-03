//! The X3DH handshake: turns a scanned contact card into a shared secret
//! plus the message to send back so the other side derives the same
//! secret.

use spiritchat_crypto_core::handshake;

use crate::agreement::FfiAgreementKey;
use crate::contact_card::FfiContactCard;
use crate::error::FfiResult;
use crate::identity::FfiIdentity;
use crate::prekeys::FfiPrekeyStore;

#[derive(uniffi::Record)]
pub struct FfiHandshakeResult {
    /// Feed this straight into `FfiRatchet::init_initiator`.
    pub shared_secret: Vec<u8>,
    /// Send this to the peer; they pass it to `x3dh_respond`.
    pub initial_message: Vec<u8>,
}

/// Runs the initiator side against a scanned [`FfiContactCard`].
#[uniffi::export]
pub fn x3dh_initiate(
    identity: &FfiIdentity,
    agreement: &FfiAgreementKey,
    card: &FfiContactCard,
) -> FfiResult<FfiHandshakeResult> {
    let mut rng = rand_core::OsRng;
    let result = handshake::initiate(&mut rng, &identity.0, &agreement.0, &card.0)?;
    Ok(FfiHandshakeResult {
        shared_secret: result.shared_secret.as_bytes().to_vec(),
        initial_message: result.initial_message.to_bytes(),
    })
}

#[derive(uniffi::Record)]
pub struct FfiRespondResult {
    /// Feed this into `FfiRatchet::init_responder`.
    pub shared_secret: Vec<u8>,
    /// The initiator's identity public key, read out of the initial
    /// message itself — the responder has no other way to learn who just
    /// started a session with them (an incoming envelope only otherwise
    /// carries a PeerId, not the Ed25519 key a fingerprint is computed
    /// from).
    pub initiator_identity_bytes: Vec<u8>,
}

/// Runs the responder side against the bytes an initiator's
/// `x3dh_initiate` produced, consuming the referenced one-time prekey (if
/// any) from `prekeys`. Returns the same shared secret bytes the
/// initiator has, to feed into `FfiRatchet::init_responder`, plus the
/// initiator's identity so the responder can tell who just messaged them.
#[uniffi::export]
pub fn x3dh_respond(
    agreement: &FfiAgreementKey,
    prekeys: &FfiPrekeyStore,
    initial_message_bytes: Vec<u8>,
) -> FfiResult<FfiRespondResult> {
    let message = handshake::InitialMessage::from_bytes(&initial_message_bytes)?;
    let mut store = prekeys.0.lock().expect("prekey store mutex poisoned");
    let shared_secret = handshake::respond(&agreement.0, &mut store, &message)?;
    Ok(FfiRespondResult {
        shared_secret: shared_secret.as_bytes().to_vec(),
        initiator_identity_bytes: message.initiator_identity.to_bytes().to_vec(),
    })
}
