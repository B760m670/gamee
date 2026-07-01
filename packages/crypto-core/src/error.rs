use thiserror::Error;

/// Every fallible operation in this crate returns this error type.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("invalid key length: expected {expected} bytes, got {actual}")]
    InvalidKeyLength { expected: usize, actual: usize },

    #[error("signature verification failed")]
    SignatureInvalid,

    #[error("malformed encoded data: {0}")]
    Decode(&'static str),

    #[error("AEAD authentication failed (message tampered or wrong key)")]
    DecryptionFailed,

    #[error("no one-time prekey available")]
    PrekeyExhausted,

    #[error("message key for this counter was already used or is unavailable")]
    MessageKeyUnavailable,

    #[error("too many skipped message keys in one chain (possible replay/DoS)")]
    TooManySkippedKeys,
}

pub type Result<T> = core::result::Result<T, CryptoError>;
