//! The one error type every exported function/method can return. UniFFI
//! needs error types to be plain enough to convert into Kotlin/Swift, so
//! this wraps [`spiritchat_crypto_core::error::CryptoError`] as a message
//! string instead of exposing the core crate's error type directly.

use spiritchat_crypto_core::error::CryptoError;

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiError {
    #[error("{message}")]
    Crypto { message: String },
}

impl From<CryptoError> for FfiError {
    fn from(err: CryptoError) -> Self {
        FfiError::Crypto {
            message: err.to_string(),
        }
    }
}

pub type FfiResult<T> = Result<T, FfiError>;
