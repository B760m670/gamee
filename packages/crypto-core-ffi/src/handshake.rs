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

/// Runs the responder side against the bytes an initiator's
/// `x3dh_initiate` produced, consuming the referenced one-time prekey (if
/// any) from `prekeys`. Returns the same shared secret bytes the
/// initiator has, to feed into `FfiRatchet::init_responder`.
#[uniffi::export]
pub fn x3dh_respond(
    agreement: &FfiAgreementKey,
    prekeys: &FfiPrekeyStore,
    initial_message_bytes: Vec<u8>,
) -> FfiResult<Vec<u8>> {
    let message = handshake::InitialMessage::from_bytes(&initial_message_bytes)?;
    let mut store = prekeys.0.lock().expect("prekey store mutex poisoned");
    let shared_secret = handshake::respond(&agreement.0, &mut store, &message)?;
    Ok(shared_secret.as_bytes().to_vec())
}
