//! Group messaging (Sender Keys) — see
//! `spiritchat_crypto_core::sender_key`'s own doc comment for the scheme.
//! Wrapped in a [`std::sync::Mutex`] for the same reason [`crate::ratchet::FfiRatchet`]
//! is: encrypting/decrypting both advance internal state, which UniFFI
//! objects can only do through `&self`.

use std::sync::{Arc, Mutex};

use spiritchat_crypto_core::sender_key::{envelope, SenderKeyDistribution, SenderKeyReceiverState, SenderKeyState};

use crate::error::FfiResult;

/// One group member's own outgoing Sender Key chain.
#[derive(uniffi::Object)]
pub struct FfiSenderKeyState(Mutex<SenderKeyState>);

#[uniffi::export]
impl FfiSenderKeyState {
    /// Starts a brand new chain — call once per group this identity
    /// creates or joins, and again any time this member's chain needs to
    /// be rotated (e.g. after a membership change) for forward secrecy.
    #[uniffi::constructor]
    pub fn generate() -> Arc<Self> {
        Arc::new(Self(Mutex::new(SenderKeyState::generate_from_os_rng())))
    }

    /// Restores a chain from bytes previously returned by
    /// [`Self::to_bytes`].
    #[uniffi::constructor]
    pub fn from_bytes(bytes: Vec<u8>) -> FfiResult<Arc<Self>> {
        Ok(Arc::new(Self(Mutex::new(SenderKeyState::from_bytes(&bytes)?))))
    }

    /// This member's own chain state — persist this exactly as securely
    /// as any other secret key material (it contains a live signing key).
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.lock().expect("sender key state mutex poisoned").to_bytes()
    }

    /// This member's current chain state, encoded to send to one other
    /// group member over an *existing* pairwise session — see
    /// `SenderKeyDistribution`'s own doc comment. The same bytes go to
    /// every other member; nothing here is recipient-specific.
    pub fn to_distribution_bytes(&self) -> Vec<u8> {
        self.0.lock().expect("sender key state mutex poisoned").to_distribution().encode()
    }

    /// Encrypts `plaintext`, returning a single self-contained wire
    /// message (header + signature + ciphertext) ready to fan out to
    /// every other group member's transport. `associated_data` is
    /// authenticated but not encrypted (e.g. a group id) — pass an empty
    /// vec if there is none.
    pub fn encrypt(&self, plaintext: Vec<u8>, associated_data: Vec<u8>) -> FfiResult<Vec<u8>> {
        let mut state = self.0.lock().expect("sender key state mutex poisoned");
        let (header, signed_ciphertext) = state.encrypt(&plaintext, &associated_data)?;
        Ok(envelope::encode(&header, &signed_ciphertext))
    }
}

/// What a group member keeps for *each other* member's Sender Key chain.
#[derive(uniffi::Object)]
pub struct FfiSenderKeyReceiverState(Mutex<SenderKeyReceiverState>);

#[uniffi::export]
impl FfiSenderKeyReceiverState {
    /// Bootstraps from a distribution received from another group member
    /// (see [`FfiSenderKeyState::to_distribution_bytes`]) — over their
    /// existing pairwise session, decrypted the same way any other 1:1
    /// message is before ever reaching this constructor.
    #[uniffi::constructor]
    pub fn from_distribution_bytes(bytes: Vec<u8>) -> FfiResult<Arc<Self>> {
        let distribution = SenderKeyDistribution::decode(&bytes)?;
        Ok(Arc::new(Self(Mutex::new(SenderKeyReceiverState::from_distribution(&distribution)?))))
    }

    /// Restores this receiver-side state from bytes previously returned
    /// by [`Self::to_bytes`].
    #[uniffi::constructor]
    pub fn from_bytes(bytes: Vec<u8>) -> FfiResult<Arc<Self>> {
        Ok(Arc::new(Self(Mutex::new(SenderKeyReceiverState::from_bytes(&bytes)?))))
    }

    /// This state — persist it so out-of-order/skipped keys and chain
    /// position survive an app restart.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.lock().expect("sender key receiver state mutex poisoned").to_bytes()
    }

    /// Verifies and decrypts a wire message produced by the sender's
    /// `FfiSenderKeyState::encrypt`, handling out-of-order delivery
    /// transparently. Fails if the message wasn't actually signed by the
    /// member this state was built from — even though the underlying
    /// chain key is shared with the whole group.
    pub fn decrypt(&self, message: Vec<u8>, associated_data: Vec<u8>) -> FfiResult<Vec<u8>> {
        let (header, signed_ciphertext) = envelope::decode(&message)?;
        let mut state = self.0.lock().expect("sender key receiver state mutex poisoned");
        Ok(state.decrypt(&header, signed_ciphertext, &associated_data)?)
    }
}
