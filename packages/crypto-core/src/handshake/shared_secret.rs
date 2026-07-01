use zeroize::ZeroizeOnDrop;

use crate::error::{CryptoError, Result};

/// The 32-byte secret X3DH produces. Feeds directly into the Double
/// Ratchet as the initial root key — nothing else should ever consume it.
#[derive(ZeroizeOnDrop, Clone)]
pub struct SharedSecret(pub(crate) [u8; 32]);

impl SharedSecret {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Reconstructs a shared secret carried across an FFI boundary, e.g.
    /// after [`crate::handshake::respond`] ran on the Kotlin/Swift side of
    /// a call and only the raw bytes came back. Prefer computing a
    /// [`SharedSecret`] via [`crate::handshake::initiate`] or
    /// [`crate::handshake::respond`] directly when possible.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let array: [u8; 32] = bytes
            .try_into()
            .map_err(|_| CryptoError::InvalidKeyLength {
                expected: 32,
                actual: bytes.len(),
            })?;
        Ok(Self(array))
    }
}

// Deliberately hand-written instead of derived: printing the actual bytes
// would defeat the point of zeroizing them, e.g. if this ever ends up in a
// log line via `{:?}` on an enclosing struct.
impl std::fmt::Debug for SharedSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SharedSecret(REDACTED)")
    }
}

#[cfg(test)]
impl PartialEq for SharedSecret {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_bytes() {
        let secret = SharedSecret([9u8; 32]);
        let restored = SharedSecret::from_bytes(secret.as_bytes()).unwrap();
        assert_eq!(secret, restored);
    }

    #[test]
    fn rejects_wrong_length_bytes() {
        assert!(SharedSecret::from_bytes(&[0u8; 10]).is_err());
    }
}
