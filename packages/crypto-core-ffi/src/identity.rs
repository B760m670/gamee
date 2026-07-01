//! The long-term identity key pair, exposed as raw bytes at the boundary
//! so the app can hand them to platform secure storage (Android Keystore,
//! iOS Keychain) without this crate ever touching a filesystem or keychain
//! API itself.

use std::sync::Arc;

use rand_core::OsRng;
use spiritchat_crypto_core::identity::{Fingerprint, IdentityKeyPair};

use crate::error::FfiResult;

#[derive(uniffi::Object)]
pub struct FfiIdentity(pub(crate) IdentityKeyPair);

#[uniffi::export]
impl FfiIdentity {
    /// Creates a brand new identity. Call this exactly once per account;
    /// everything else (agreement key, prekeys, fingerprint) is derived
    /// from or signed by it.
    #[uniffi::constructor]
    pub fn generate() -> Arc<Self> {
        Arc::new(Self(IdentityKeyPair::generate(&mut OsRng)))
    }

    /// Restores an identity from its 32-byte secret seed, as previously
    /// returned by [`Self::secret_bytes`] and retrieved from secure
    /// storage.
    #[uniffi::constructor]
    pub fn from_secret_bytes(bytes: Vec<u8>) -> FfiResult<Arc<Self>> {
        Ok(Arc::new(Self(IdentityKeyPair::from_bytes(&bytes)?)))
    }

    /// The 32-byte secret seed. The caller must persist this in secure
    /// storage — losing it means losing the account; anyone who obtains a
    /// copy can impersonate it.
    pub fn secret_bytes(&self) -> Vec<u8> {
        self.0.to_bytes().to_vec()
    }

    /// The 32-byte public identity, safe to share (e.g. it's what a QR
    /// contact card commits to).
    pub fn public_key_bytes(&self) -> Vec<u8> {
        self.0.public_key().to_bytes().to_vec()
    }

    /// A short, human-comparable fingerprint ("1234 5678 9012") for two
    /// people to read aloud and compare out of band, to rule out a
    /// man-in-the-middle QR swap.
    pub fn fingerprint(&self) -> String {
        Fingerprint::of(&self.0.public_key()).as_str().to_string()
    }
}
