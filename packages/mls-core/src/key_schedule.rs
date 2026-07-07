//! The epoch key schedule — RFC 9420 section 8, specialized to
//! HKDF-SHA256. Every commit produces one *commit secret* (the root
//! path secret from the ratchet-tree update, phase 2); the key schedule
//! folds it, together with the *previous* epoch's `init_secret` and the
//! new group context, into the secrets that key the epoch:
//!
//!   joiner_secret = Extract(init_secret[n-1], commit_secret)
//!   epoch_secret  = DeriveSecret(joiner_secret, "epoch" || group_context)
//!   ...then each leaf secret = DeriveSecret(epoch_secret, <label>)
//!
//! Chaining through the previous `init_secret` is what makes epochs a
//! *ratchet*: recovering one epoch's secrets tells you nothing about any
//! earlier epoch (forward secrecy), and a single healed commit secret
//! re-randomizes everything downstream (post-compromise security). The
//! new `init_secret` this produces is the next epoch's input, closing
//! the loop.

use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// HKDF-Extract, RFC 5869. `salt` is the previous `init_secret`.
fn extract(salt: &[u8; 32], ikm: &[u8; 32]) -> [u8; 32] {
    let (prk, _) = Hkdf::<Sha256>::extract(Some(salt), ikm);
    let mut out = [0u8; 32];
    out.copy_from_slice(&prk);
    out
}

/// RFC 9420's DeriveSecret = ExpandWithLabel(secret, label, ""), specialized:
/// one 32-byte output, label domain-separated under this crate's own tag.
fn derive_secret(secret: &[u8; 32], label: &[u8], context: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::from_prk(secret).expect("32-byte PRK is valid");
    let mut info = Vec::with_capacity(24 + label.len() + context.len());
    info.extend_from_slice(b"spiritchat-mls-ks-v1:");
    info.extend_from_slice(label);
    info.push(b':');
    info.extend_from_slice(context);
    let mut out = [0u8; 32];
    hk.expand(&info, &mut out).expect("32 bytes is a valid HKDF-SHA256 output length");
    out
}

/// The secrets that key one epoch. Every field is derived from the same
/// `epoch_secret`, so they're independent (learning one reveals nothing
/// about the others) yet all reproducible by every member who reached
/// this epoch's root secret.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct EpochSecrets {
    /// Feeds the *next* epoch's key schedule as the Extract salt — the
    /// link that makes epochs a forward-secret ratchet.
    pub init_secret: [u8; 32],
    /// Root of per-message key derivation for application messages sent
    /// this epoch (phase 4+ rides group message encryption on this).
    pub encryption_secret: [u8; 32],
    /// For deriving symmetric keys to hand to callers (e.g. an
    /// application-defined "seal a file to the group" — the MLS exporter).
    pub exporter_secret: [u8; 32],
    /// Authenticates the commit that created this epoch: a MAC under this
    /// key over the confirmed transcript hash is what proves every member
    /// applied the *same* commit (phase 4's transcript machinery).
    pub confirmation_key: [u8; 32],
    /// A single value uniquely identifying this epoch's shared state —
    /// two members agree on it iff they agree on the entire history that
    /// produced it. Handy as a channel-binding / safety-number input.
    pub epoch_authenticator: [u8; 32],
}

impl std::fmt::Debug for EpochSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EpochSecrets(<redacted>)")
    }
}

impl EpochSecrets {
    /// Derives the epoch from a commit. `prev_init_secret` is the last
    /// epoch's `init_secret` (for the genesis epoch of a group, a fixed
    /// all-zero seed — see `initial`); `commit_secret` is the ratchet
    /// tree's new root secret; `group_context` is the hash binding the
    /// group id, epoch number, tree, and transcript so far (phase 4
    /// assembles the real thing — any stable per-epoch context works
    /// here and is tested as such).
    pub fn derive(prev_init_secret: &[u8; 32], commit_secret: &[u8; 32], group_context: &[u8]) -> Self {
        let joiner_secret = extract(prev_init_secret, commit_secret);
        let epoch_secret = derive_secret(&joiner_secret, b"epoch", group_context);
        EpochSecrets {
            init_secret: derive_secret(&epoch_secret, b"init", &[]),
            encryption_secret: derive_secret(&epoch_secret, b"encryption", &[]),
            exporter_secret: derive_secret(&epoch_secret, b"exporter", &[]),
            confirmation_key: derive_secret(&epoch_secret, b"confirm", &[]),
            epoch_authenticator: derive_secret(&epoch_secret, b"authentication", &[]),
        }
    }

    /// The all-zero `init_secret` a brand-new group's first epoch extracts
    /// against — RFC 9420's convention for the epoch-0 predecessor.
    pub fn initial_init_secret() -> [u8; 32] {
        [0u8; 32]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_schedule_is_deterministic() {
        let a = EpochSecrets::derive(&[0u8; 32], &[7u8; 32], b"ctx");
        let b = EpochSecrets::derive(&[0u8; 32], &[7u8; 32], b"ctx");
        assert_eq!(a.init_secret, b.init_secret);
        assert_eq!(a.epoch_authenticator, b.epoch_authenticator);
    }

    #[test]
    fn every_derived_secret_is_distinct() {
        let e = EpochSecrets::derive(&[0u8; 32], &[7u8; 32], b"ctx");
        let all = [
            e.init_secret,
            e.encryption_secret,
            e.exporter_secret,
            e.confirmation_key,
            e.epoch_authenticator,
        ];
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                assert_ne!(all[i], all[j], "labels must domain-separate every leaf secret");
            }
        }
    }

    #[test]
    fn a_different_commit_secret_changes_the_whole_epoch() {
        let a = EpochSecrets::derive(&[0u8; 32], &[7u8; 32], b"ctx");
        let b = EpochSecrets::derive(&[0u8; 32], &[8u8; 32], b"ctx");
        assert_ne!(a.epoch_authenticator, b.epoch_authenticator);
        assert_ne!(a.init_secret, b.init_secret);
    }

    #[test]
    fn a_different_group_context_changes_the_epoch() {
        // Two groups (or two epochs) that happened to share a commit
        // secret must still not collide — the context binds them apart.
        let a = EpochSecrets::derive(&[0u8; 32], &[7u8; 32], b"group-a:epoch-1");
        let b = EpochSecrets::derive(&[0u8; 32], &[7u8; 32], b"group-b:epoch-1");
        assert_ne!(a.epoch_authenticator, b.epoch_authenticator);
    }

    #[test]
    fn epochs_chain_forward_through_init_secret() {
        // Epoch 1 feeds epoch 2; a fresh commit secret at epoch 2 must
        // produce yet another distinct epoch, and reproducibly so.
        let e1 = EpochSecrets::derive(&EpochSecrets::initial_init_secret(), &[7u8; 32], b"e1");
        let e2 = EpochSecrets::derive(&e1.init_secret, &[9u8; 32], b"e2");
        let e2_again = EpochSecrets::derive(&e1.init_secret, &[9u8; 32], b"e2");
        assert_eq!(e2.epoch_authenticator, e2_again.epoch_authenticator);
        assert_ne!(e1.epoch_authenticator, e2.epoch_authenticator);
    }

    #[test]
    fn recovering_one_epoch_does_not_reveal_the_previous_one() {
        // Forward secrecy at the schedule level: epoch 2's secrets are a
        // one-way function of epoch 1's init_secret, so holding all of
        // epoch 2 gives no algebraic route back to epoch 1's own leaf
        // secrets (distinctness stands in for the one-wayness HKDF
        // guarantees — there's no function here from e2 back to e1).
        let e1 = EpochSecrets::derive(&[0u8; 32], &[1u8; 32], b"e1");
        let e2 = EpochSecrets::derive(&e1.init_secret, &[2u8; 32], b"e2");
        assert_ne!(e1.encryption_secret, e2.encryption_secret);
        assert_ne!(e1.exporter_secret, e2.exporter_secret);
    }
}
