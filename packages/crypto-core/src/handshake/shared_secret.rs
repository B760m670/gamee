use zeroize::ZeroizeOnDrop;

/// The 32-byte secret X3DH produces. Feeds directly into the Double
/// Ratchet as the initial root key — nothing else should ever consume it.
#[derive(ZeroizeOnDrop, Clone)]
pub struct SharedSecret(pub(crate) [u8; 32]);

impl SharedSecret {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
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
