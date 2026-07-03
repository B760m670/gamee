//! A short, human-comparable fingerprint of an identity, e.g. for reading
//! aloud or comparing two QR-exchanged contacts out of band to rule out a
//! man-in-the-middle QR swap.

use sha2::{Digest, Sha256};

use super::keypair::IdentityPublicKey;

/// 12 decimal digits, grouped in 4s, derived from SHA-256(public_key).
/// Short enough to compare by eye, long enough (10^12 space) that a
/// collision attack isn't practical for this purpose.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Fingerprint(String);

impl Fingerprint {
    pub fn of(identity: &IdentityPublicKey) -> Self {
        let digest = Sha256::digest(identity.to_bytes());

        // Turn the first 5 bytes into a 40-bit number, then take it mod
        // 10^12 to get 12 decimal digits with roughly uniform distribution.
        let mut value: u64 = 0;
        for &byte in &digest[..5] {
            value = (value << 8) | byte as u64;
        }
        let digits = value % 1_000_000_000_000;

        let raw = format!("{digits:012}");
        let grouped = raw
            .as_bytes()
            .chunks(4)
            .map(|chunk| std::str::from_utf8(chunk).unwrap())
            .collect::<Vec<_>>()
            .join(" ");
        Self(grouped)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::keypair::IdentityKeyPair;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    #[test]
    fn is_deterministic_for_the_same_key() {
        let identity = IdentityKeyPair::generate(&mut ChaCha20Rng::seed_from_u64(7));
        let a = Fingerprint::of(&identity.public_key());
        let b = Fingerprint::of(&identity.public_key());
        assert_eq!(a, b);
    }

    #[test]
    fn differs_between_distinct_keys() {
        let alice = IdentityKeyPair::generate(&mut ChaCha20Rng::seed_from_u64(1));
        let bob = IdentityKeyPair::generate(&mut ChaCha20Rng::seed_from_u64(2));
        assert_ne!(
            Fingerprint::of(&alice.public_key()),
            Fingerprint::of(&bob.public_key())
        );
    }

    #[test]
    fn formats_as_three_groups_of_four_digits() {
        let identity = IdentityKeyPair::generate(&mut ChaCha20Rng::seed_from_u64(3));
        let fp = Fingerprint::of(&identity.public_key());
        let groups: Vec<&str> = fp.as_str().split(' ').collect();
        assert_eq!(groups.len(), 3);
        for group in groups {
            assert_eq!(group.len(), 4);
            assert!(group.chars().all(|c| c.is_ascii_digit()));
        }
    }
}
