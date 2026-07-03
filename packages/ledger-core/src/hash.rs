//! A 32-byte SHA-256 digest, used both for block/transaction identity and
//! as the thing a block's proof-of-work is checked against. Reuses `sha2`
//! (already a dependency of `spiritchat-crypto-core`, see
//! `identity/fingerprint.rs`) rather than pulling in a second hash crate.

use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Hash32(pub [u8; 32]);

impl Hash32 {
    pub const ZERO: Hash32 = Hash32([0u8; 32]);

    pub fn of(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        Hash32(out)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Whether this hash, read as a big-endian 256-bit integer, is small
    /// enough to satisfy `target` — the actual proof-of-work check. Plain
    /// byte-array comparison is exactly big-endian integer comparison, no
    /// separate bignum type needed.
    pub fn meets_target(&self, target: &Hash32) -> bool {
        self.0 <= target.0
    }
}

impl fmt::Debug for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Display for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashing_is_deterministic() {
        assert_eq!(Hash32::of(b"hello"), Hash32::of(b"hello"));
    }

    #[test]
    fn different_input_hashes_differently() {
        assert_ne!(Hash32::of(b"hello"), Hash32::of(b"goodbye"));
    }

    #[test]
    fn meets_target_is_a_plain_less_than_or_equal_comparison() {
        let small = Hash32([0u8; 32]);
        let mut big = [0u8; 32];
        big[0] = 0xff;
        let big = Hash32(big);
        assert!(small.meets_target(&big));
        assert!(!big.meets_target(&small));
        assert!(small.meets_target(&small));
    }
}
