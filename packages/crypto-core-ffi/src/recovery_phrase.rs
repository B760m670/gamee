//! UniFFI bridge for `spiritchat_crypto_core::identity::RecoveryPhrase` —
//! the BIP39 mnemonic an account's identity key is derived from, so it can
//! be recovered on a fresh install with no server involved at all.

use std::sync::Arc;

use rand_core::OsRng;
use spiritchat_crypto_core::identity::RecoveryPhrase;

use crate::error::FfiResult;

#[derive(uniffi::Object)]
pub struct FfiRecoveryPhrase(RecoveryPhrase);

#[uniffi::export]
impl FfiRecoveryPhrase {
    /// Generates a brand-new 12-word phrase. Call this exactly once, when
    /// creating a new account — show the words to the user immediately
    /// afterward; this object holds them in memory only, nothing here
    /// writes them anywhere.
    #[uniffi::constructor]
    pub fn generate() -> Arc<Self> {
        Arc::new(Self(RecoveryPhrase::generate(&mut OsRng)))
    }

    /// Parses and checksum-validates a phrase the user typed back in. A
    /// typo in any single word is caught here, before it can silently
    /// derive the wrong identity.
    #[uniffi::constructor]
    pub fn from_words(words: String) -> FfiResult<Arc<Self>> {
        Ok(Arc::new(Self(RecoveryPhrase::parse(&words)?)))
    }

    /// The words, space-separated, to display once at account creation.
    pub fn words(&self) -> String {
        self.0.words()
    }

    /// The 32-byte identity seed this phrase derives — feed this straight
    /// into `FfiIdentity::from_secret_bytes` (and, for the P2P node,
    /// `FfiP2pNode::spawn`'s `identity_seed`) to recreate the exact same
    /// identity, fingerprint, and PeerId as the original device.
    pub fn derive_identity_seed(&self) -> Vec<u8> {
        self.0.derive_identity_seed().to_vec()
    }
}
