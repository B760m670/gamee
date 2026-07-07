//! The transcript hash chain — RFC 9420 section 8.2. This is what makes
//! group membership *cryptographically agreed* rather than merely
//! asserted: every commit is folded into a running hash, and each epoch's
//! `confirmation_tag` is a MAC over that hash under the epoch's
//! `confirmation_key` (from the key schedule). Two members who applied any
//! different sequence of commits — a different add, a dropped removal, a
//! reordering — arrive at different transcript hashes, so their
//! confirmation tags disagree and the divergence is caught immediately
//! instead of festering into two silently different "groups".
//!
//!   confirmed[n] = Hash(interim[n-1] ‖ commit_content)
//!   tag[n]       = MAC(confirmation_key[n], confirmed[n])
//!   interim[n]   = Hash(confirmed[n] ‖ tag[n])
//!
//! `interim[0]` seeds from the group id, so two groups can't share a
//! transcript even if their commit histories happen to coincide.

use hmac::{Mac, SimpleHmac};
use sha2::{Digest, Sha256};

/// The rolling transcript state a member carries between epochs — just the
/// interim hash; the confirmed hash is transient per commit.
#[derive(Clone, PartialEq, Eq)]
pub struct Transcript {
    interim: [u8; 32],
}

impl std::fmt::Debug for Transcript {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Transcript({:02x}{:02x}..)", self.interim[0], self.interim[1])
    }
}

fn hash2(a: &[u8], b: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(a);
    h.update(b);
    h.finalize().into()
}

/// HMAC-SHA256(confirmation_key, confirmed_transcript_hash).
pub fn confirmation_tag(confirmation_key: &[u8; 32], confirmed: &[u8; 32]) -> [u8; 32] {
    let mut mac = <SimpleHmac<Sha256> as Mac>::new_from_slice(confirmation_key)
        .expect("HMAC accepts any key length");
    mac.update(confirmed);
    mac.finalize().into_bytes().into()
}

impl Transcript {
    /// The genesis interim hash, seeded from the group id.
    pub fn new(group_id: &[u8]) -> Self {
        let mut h = Sha256::new();
        h.update(b"spiritchat-mls-transcript-v1:");
        h.update(group_id);
        Transcript { interim: h.finalize().into() }
    }

    pub fn interim(&self) -> &[u8; 32] {
        &self.interim
    }

    /// The confirmed transcript hash for a commit whose content is
    /// `commit_content` — what the confirmation tag is taken over. Does
    /// not advance the chain (a receiver computes this to *check* the
    /// sender's tag before committing to the new epoch).
    pub fn confirmed(&self, commit_content: &[u8]) -> [u8; 32] {
        hash2(&self.interim, commit_content)
    }

    /// Advances the chain into the next epoch, given the confirmed hash
    /// and the tag that authenticated it. Called only after the tag has
    /// verified.
    pub fn advance(&mut self, confirmed: &[u8; 32], tag: &[u8; 32]) {
        self.interim = hash2(confirmed, tag);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_histories_produce_identical_transcripts() {
        let mut a = Transcript::new(b"group-1");
        let mut b = Transcript::new(b"group-1");
        for commit in [&b"c1"[..], b"c2", b"c3"] {
            let ca = a.confirmed(commit);
            let cb = b.confirmed(commit);
            assert_eq!(ca, cb);
            let ta = confirmation_tag(&[1u8; 32], &ca);
            let tb = confirmation_tag(&[1u8; 32], &cb);
            assert_eq!(ta, tb);
            a.advance(&ca, &ta);
            b.advance(&cb, &tb);
        }
        assert_eq!(a, b);
    }

    #[test]
    fn a_diverging_commit_forks_the_transcript_permanently() {
        let mut a = Transcript::new(b"group-1");
        let mut b = Transcript::new(b"group-1");
        // b applies a different second commit...
        for (ca_in, cb_in) in [(&b"c1"[..], &b"c1"[..]), (b"c2", b"c2-different")] {
            let ca = a.confirmed(ca_in);
            let cb = b.confirmed(cb_in);
            let ta = confirmation_tag(&[1u8; 32], &ca);
            let tb = confirmation_tag(&[1u8; 32], &cb);
            a.advance(&ca, &ta);
            b.advance(&cb, &tb);
        }
        assert_ne!(a, b);
        // ...and every subsequent epoch stays forked, even on identical
        // later commits — the whole point: a past disagreement can never
        // silently re-converge.
        let ca = a.confirmed(b"c3");
        let cb = b.confirmed(b"c3");
        assert_ne!(ca, cb);
    }

    #[test]
    fn the_confirmation_tag_depends_on_the_key() {
        let t = Transcript::new(b"g");
        let confirmed = t.confirmed(b"commit");
        assert_ne!(
            confirmation_tag(&[1u8; 32], &confirmed),
            confirmation_tag(&[2u8; 32], &confirmed)
        );
    }

    #[test]
    fn different_groups_never_share_a_transcript() {
        let a = Transcript::new(b"group-a");
        let b = Transcript::new(b"group-b");
        assert_ne!(a.confirmed(b"same-commit"), b.confirmed(b"same-commit"));
    }
}
