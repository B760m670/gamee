//! The long-term identity key pair, exposed as raw bytes at the boundary
//! so the app can hand them to platform secure storage (Android Keystore,
//! iOS Keychain) without this crate ever touching a filesystem or keychain
//! API itself.

use std::sync::Arc;

use rand_core::OsRng;
use spiritchat_crypto_core::identity::{Fingerprint, IdentityKeyPair, IdentityPublicKey};

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

    /// Signs `message` with this identity's long-term key. Used, e.g., to
    /// bind a claimed `@username` to this identity in a way nobody else can
    /// forge — a DHT node storing the claim can't produce a valid signature
    /// without this key, whoever they are.
    pub fn sign(&self, message: Vec<u8>) -> Vec<u8> {
        self.0.sign(&message).to_vec()
    }
}

/// Verifies `signature` over `message` was produced by the identity whose
/// public key is `public_key` — standalone (not a method on `FfiIdentity`)
/// because verifying doesn't require *owning* an identity, only knowing a
/// public key to check against, e.g. after resolving a `@username` claim
/// from the DHT and needing to confirm it wasn't forged before trusting it.
/// Returns `false` for a malformed `public_key`/`signature`, not an error —
/// "not a valid claim" is an expected, ordinary outcome here, not
/// exceptional.
#[uniffi::export]
pub fn identity_verify(public_key: Vec<u8>, message: Vec<u8>, signature: Vec<u8>) -> bool {
    let Ok(public_key) = IdentityPublicKey::from_bytes(&public_key) else {
        return false;
    };
    let Ok(signature): std::result::Result<[u8; 64], _> = signature.try_into() else {
        return false;
    };
    public_key.verify(&message, &signature).is_ok()
}

/// The same human-comparable fingerprint `FfiIdentity::fingerprint`
/// produces, computed from just a public key — for a resolved `@username`
/// claim, where the app only ever has the other person's public key, never
/// their secret, so it can't construct a full `FfiIdentity` to call that
/// method on.
#[uniffi::export]
pub fn identity_fingerprint_of_public_key(public_key: Vec<u8>) -> FfiResult<String> {
    let public_key = IdentityPublicKey::from_bytes(&public_key)?;
    Ok(Fingerprint::of(&public_key).as_str().to_string())
}
