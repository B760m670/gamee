//! Signed + one-time prekeys, and the contact-card bytes built from them.
//! Wrapped in a [`std::sync::Mutex`] because handing out a card consumes a
//! one-time prekey — a mutation UniFFI objects can't express through `&mut
//! self` (exported objects are always shared behind an `Arc`).

use std::sync::{Arc, Mutex};

use rand_core::OsRng;
use spiritchat_crypto_core::prekey::PrekeyStore;

use crate::agreement::FfiAgreementKey;
use crate::error::FfiResult;
use crate::identity::FfiIdentity;

#[derive(uniffi::Object)]
pub struct FfiPrekeyStore(pub(crate) Mutex<PrekeyStore>);

#[uniffi::export]
impl FfiPrekeyStore {
    /// Generates a fresh signed prekey plus `one_time_count` one-time
    /// prekeys, all signed by `identity`.
    #[uniffi::constructor]
    pub fn generate(identity: &FfiIdentity, one_time_count: u32) -> Arc<Self> {
        Arc::new(Self(Mutex::new(PrekeyStore::generate(
            &identity.0,
            &mut OsRng,
            one_time_count as usize,
        ))))
    }

    #[uniffi::constructor]
    pub fn from_bytes(bytes: Vec<u8>) -> FfiResult<Arc<Self>> {
        Ok(Arc::new(Self(Mutex::new(PrekeyStore::from_bytes(&bytes)?))))
    }

    /// The whole store, including private key material — persist this
    /// exactly as securely as the identity and agreement secrets.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.lock().expect("prekey store mutex poisoned").to_bytes()
    }

    /// The bytes to embed in a QR contact card: identity, agreement key,
    /// signed prekey, and (if any remain) a fresh one-time prekey. Each
    /// call may hand out a different one-time prekey — generate a new card
    /// per contact rather than reusing one.
    pub fn contact_card(&self, identity: &FfiIdentity, agreement: &FfiAgreementKey) -> Vec<u8> {
        let signed_agreement = agreement.0.sign_with(&identity.0);
        let mut store = self.0.lock().expect("prekey store mutex poisoned");
        store
            .public_bundle(identity.0.public_key(), signed_agreement)
            .to_bytes()
    }

    /// How many one-time prekeys are still available to hand out.
    pub fn one_time_prekey_count(&self) -> u32 {
        self.0.lock().expect("prekey store mutex poisoned").one_time_prekey_count() as u32
    }

    /// The signed prekey's private half, for bootstrapping this identity's
    /// side of a [`crate::ratchet::FfiRatchet`] as a responder — the
    /// Double Ratchet spec reuses the X3DH signed prekey as the
    /// responder's first ratchet key, since they haven't sent anything yet
    /// to establish a dedicated one.
    pub fn signed_prekey_secret_bytes(&self) -> Vec<u8> {
        self.0
            .lock()
            .expect("prekey store mutex poisoned")
            .clone_signed_prekey_secret()
            .to_bytes()
            .to_vec()
    }
}
