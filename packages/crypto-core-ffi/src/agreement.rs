//! The long-term X25519 key used for the X3DH handshake. Kept as its own
//! FFI object, mirroring the core crate's split between the signing
//! identity and this DH-only key.

use std::sync::Arc;

use rand_core::OsRng;
use spiritchat_crypto_core::identity::AgreementKeyPair;

use crate::error::FfiResult;

#[derive(uniffi::Object)]
pub struct FfiAgreementKey(pub(crate) AgreementKeyPair);

#[uniffi::export]
impl FfiAgreementKey {
    #[uniffi::constructor]
    pub fn generate() -> Arc<Self> {
        Arc::new(Self(AgreementKeyPair::generate(&mut OsRng)))
    }

    /// Restores an agreement key from its 32-byte secret scalar, as
    /// previously returned by [`Self::secret_bytes`].
    #[uniffi::constructor]
    pub fn from_secret_bytes(bytes: Vec<u8>) -> FfiResult<Arc<Self>> {
        Ok(Arc::new(Self(AgreementKeyPair::from_bytes(&bytes)?)))
    }

    /// The 32-byte secret scalar. Persist alongside the identity's own
    /// secret bytes, with the same level of protection.
    pub fn secret_bytes(&self) -> Vec<u8> {
        self.0.to_bytes().to_vec()
    }

    pub fn public_bytes(&self) -> Vec<u8> {
        self.0.public().as_bytes().to_vec()
    }
}
