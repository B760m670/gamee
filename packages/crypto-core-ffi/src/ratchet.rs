//! The Double Ratchet session: encrypts/decrypts individual messages once
//! a shared secret exists, and persists across app restarts via
//! `to_bytes`/`from_bytes`. Wrapped in a [`std::sync::Mutex`] since
//! encrypting and decrypting both advance internal state, which UniFFI
//! objects can only do through `&self`.

use std::sync::{Arc, Mutex};

use spiritchat_crypto_core::handshake::SharedSecret;
use spiritchat_crypto_core::ratchet::{envelope, DoubleRatchet};

use crate::error::FfiResult;

#[derive(uniffi::Object)]
pub struct FfiRatchet(Mutex<DoubleRatchet>);

#[uniffi::export]
impl FfiRatchet {
    /// Bootstraps the initiator's ("Alice's") side right after
    /// `x3dh_initiate`. `remote_ratchet_public_bytes` is the responder's
    /// signed prekey public key, taken from the same [`crate::contact_card::FfiContactCard`]
    /// that was passed to `x3dh_initiate`.
    #[uniffi::constructor]
    pub fn init_initiator(
        shared_secret_bytes: Vec<u8>,
        remote_ratchet_public_bytes: Vec<u8>,
    ) -> FfiResult<Arc<Self>> {
        let shared_secret = SharedSecret::from_bytes(&shared_secret_bytes)?;
        let ratchet = DoubleRatchet::init_initiator_from_bytes(
            &shared_secret,
            &remote_ratchet_public_bytes,
        )?;
        Ok(Arc::new(Self(Mutex::new(ratchet))))
    }

    /// Bootstraps the responder's ("Bob's") side right after
    /// `x3dh_respond`. `my_ratchet_secret_bytes` is
    /// `FfiPrekeyStore::signed_prekey_secret_bytes` from the same store
    /// `x3dh_respond` was called against.
    #[uniffi::constructor]
    pub fn init_responder(
        shared_secret_bytes: Vec<u8>,
        my_ratchet_secret_bytes: Vec<u8>,
    ) -> FfiResult<Arc<Self>> {
        let shared_secret = SharedSecret::from_bytes(&shared_secret_bytes)?;
        let ratchet =
            DoubleRatchet::init_responder_from_bytes(&shared_secret, &my_ratchet_secret_bytes)?;
        Ok(Arc::new(Self(Mutex::new(ratchet))))
    }

    /// Restores a session from bytes previously returned by
    /// [`Self::to_bytes`].
    #[uniffi::constructor]
    pub fn from_bytes(bytes: Vec<u8>) -> FfiResult<Arc<Self>> {
        Ok(Arc::new(Self(Mutex::new(DoubleRatchet::from_bytes(
            &bytes,
        )?))))
    }

    /// The full session state — persist this exactly as securely as any
    /// other secret key material; it contains live chain keys and, if any
    /// messages are still outstanding, buffered message keys.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.lock().expect("ratchet mutex poisoned").to_bytes()
    }

    /// Encrypts `plaintext`, returning a single self-contained wire
    /// message (header + ciphertext) ready to hand to the transport.
    /// `associated_data` is authenticated but not encrypted — pass an
    /// empty vec if there is none.
    pub fn encrypt(&self, plaintext: Vec<u8>, associated_data: Vec<u8>) -> FfiResult<Vec<u8>> {
        let mut ratchet = self.0.lock().expect("ratchet mutex poisoned");
        let (header, ciphertext) = ratchet.encrypt(&plaintext, &associated_data)?;
        Ok(envelope::encode(&header, &ciphertext))
    }

    /// Decrypts a wire message produced by the peer's `encrypt`, handling
    /// out-of-order delivery and DH ratchet steps transparently.
    pub fn decrypt(&self, message: Vec<u8>, associated_data: Vec<u8>) -> FfiResult<Vec<u8>> {
        let (header, ciphertext) = envelope::decode(&message)?;
        let mut ratchet = self.0.lock().expect("ratchet mutex poisoned");
        Ok(ratchet.decrypt(&header, ciphertext, &associated_data)?)
    }

    /// This side's current DH ratchet public key.
    pub fn local_ratchet_public_bytes(&self) -> Vec<u8> {
        self.0
            .lock()
            .expect("ratchet mutex poisoned")
            .local_ratchet_public()
            .as_bytes()
            .to_vec()
    }
}
