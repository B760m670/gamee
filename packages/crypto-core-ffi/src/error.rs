//! The one error type every exported function/method can return. UniFFI
//! needs error types to be plain enough to convert into Kotlin/Swift, so
//! this wraps the underlying crates' error types as message strings
//! instead of exposing them directly.

use spiritchat_crypto_core::error::CryptoError;
use spiritchat_p2p_core::P2pError;

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiError {
    // Field is deliberately not named `message`: UniFFI's generated Kotlin
    // exception class extends `Throwable`, which already declares a
    // `message` property, and a same-named field collides with it.
    #[error("{reason}")]
    Crypto { reason: String },

    #[error("{reason}")]
    P2p { reason: String },
}

impl From<CryptoError> for FfiError {
    fn from(err: CryptoError) -> Self {
        FfiError::Crypto {
            reason: err.to_string(),
        }
    }
}

impl From<P2pError> for FfiError {
    fn from(err: P2pError) -> Self {
        FfiError::P2p {
            reason: err.to_string(),
        }
    }
}

pub type FfiResult<T> = Result<T, FfiError>;
