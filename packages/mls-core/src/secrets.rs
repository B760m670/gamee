//! Secret derivation for the ratchet tree — RFC 9420's path-secret chain
//! with this crate's fixed ciphersuite (HKDF-SHA256, X25519). The chain
//! is the algebraic heart of TreeKEM:
//!
//!   path_secret[0]   = fresh randomness at the updating member's leaf
//!   path_secret[i+1] = DeriveSecret(path_secret[i], "path")
//!   node_secret[i]   = DeriveSecret(path_secret[i], "node")
//!   node keypair[i]  = X25519 keypair from node_secret[i]
//!
//! One 32-byte seed therefore deterministically re-keys an entire path to
//! the root: whoever learns `path_secret[i]` (a member under that node,
//! via HPKE to their subtree — phase 3) can derive everything *above* it
//! but nothing *below* or *beside* it. That asymmetry — knowledge flows
//! only upward — is the entire reason a TreeKEM update costs O(log N)
//! ciphertexts yet still locks out everyone who shouldn't learn the new
//! root.

use hkdf::Hkdf;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// One link of the path-secret chain. Zeroed on drop: a path secret that
/// lingered in memory after an update would undo the post-compromise
/// security that update existed to provide.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct PathSecret([u8; 32]);

// Deliberately hand-written so the secret bytes never reach a log line or
// a test failure message — `unwrap()`/`assert_eq!` on a `Result` carrying
// one must not print it.
impl std::fmt::Debug for PathSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PathSecret(<redacted>)")
    }
}

impl PathSecret {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn random(rng: &mut impl rand_core::CryptoRngCore) -> Self {
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// `path_secret[i+1]` — what the next node up the tree is keyed from.
    pub fn next(&self) -> PathSecret {
        PathSecret(derive(&self.0, b"path"))
    }

    /// The X25519 keypair for the node this path secret keys. Derived,
    /// never stored: everyone entitled to this secret computes the same
    /// keypair independently, which is what lets an updater publish only
    /// public keys and one encrypted secret per copath subtree.
    pub fn node_keypair(&self) -> (StaticSecret, PublicKey) {
        let secret = StaticSecret::from(derive(&self.0, b"node"));
        let public = PublicKey::from(&secret);
        (secret, public)
    }
}

fn derive(input: &[u8; 32], label: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(None, input);
    let mut info = Vec::with_capacity(24 + label.len());
    info.extend_from_slice(b"spiritchat-mls-v1:");
    info.extend_from_slice(label);
    let mut out = [0u8; 32];
    hk.expand(&info, &mut out)
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    #[test]
    fn the_chain_is_deterministic() {
        let a = PathSecret::new([7u8; 32]);
        let b = PathSecret::new([7u8; 32]);
        assert_eq!(a.next().as_bytes(), b.next().as_bytes());
        assert_eq!(a.node_keypair().1, b.node_keypair().1);
    }

    #[test]
    fn each_link_differs_from_the_last() {
        let s = PathSecret::new([7u8; 32]);
        assert_ne!(s.as_bytes(), s.next().as_bytes());
        assert_ne!(s.next().as_bytes(), s.next().next().as_bytes());
    }

    #[test]
    fn node_and_path_derivations_are_domain_separated() {
        let s = PathSecret::new([7u8; 32]);
        let (node_secret, _) = s.node_keypair();
        assert_ne!(node_secret.to_bytes(), *s.next().as_bytes());
    }

    #[test]
    fn the_derived_keypair_is_a_real_x25519_pair() {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let (secret_a, public_a) = PathSecret::random(&mut rng).node_keypair();
        let (secret_b, public_b) = PathSecret::random(&mut rng).node_keypair();
        // Diffie-Hellman agreement must hold — this is what phase 3's
        // HPKE encryption to a node rests on.
        assert_eq!(
            secret_a.diffie_hellman(&public_b).to_bytes(),
            secret_b.diffie_hellman(&public_a).to_bytes()
        );
    }
}
