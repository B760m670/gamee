//! A scanned or received contact card (an X3DH prekey bundle). Parsing
//! always verifies the embedded signatures — there is no way to obtain an
//! [`FfiContactCard`] whose signatures haven't already checked out, so
//! callers can't accidentally skip that step before starting a handshake.

use std::sync::Arc;

use spiritchat_crypto_core::prekey::PrekeyBundle;

use crate::error::FfiResult;

#[derive(uniffi::Object)]
pub struct FfiContactCard(pub(crate) PrekeyBundle);

#[uniffi::export]
impl FfiContactCard {
    /// Parses bytes scanned from a QR code (or otherwise received from a
    /// peer) and verifies every signature inside. Returns an error if the
    /// bytes are malformed or any signature is invalid — including one
    /// that's been tampered with to substitute an attacker's key.
    #[uniffi::constructor]
    pub fn parse(bytes: Vec<u8>) -> FfiResult<Arc<Self>> {
        let bundle = PrekeyBundle::from_bytes(&bytes)?;
        bundle.verify()?;
        Ok(Arc::new(Self(bundle)))
    }

    pub fn identity_public_key_bytes(&self) -> Vec<u8> {
        self.0.identity.to_bytes().to_vec()
    }

    /// The card owner's signed prekey public key — this is the responder's
    /// initial Double Ratchet key, so pass this to
    /// `FfiRatchet::init_initiator` as `remote_ratchet_public_bytes`.
    pub fn signed_prekey_public_bytes(&self) -> Vec<u8> {
        self.0.signed_prekey.public.as_bytes().to_vec()
    }

    pub fn has_one_time_prekey(&self) -> bool {
        self.0.one_time_prekey.is_some()
    }

    /// The card owner's fingerprint, for the initiator to compare out of
    /// band before trusting the handshake this card would start.
    pub fn fingerprint(&self) -> String {
        spiritchat_crypto_core::identity::Fingerprint::of(&self.0.identity)
            .as_str()
            .to_string()
    }
}
