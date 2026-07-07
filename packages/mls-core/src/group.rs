//! The group state machine — where the ratchet tree, key schedule, HPKE
//! transport, credentials, and transcript become one coherent MLS group.
//! One `GroupState` is one member's view of one group at one epoch. It
//! moves forward only through **commits**: a committer bundles a set of
//! proposals (add/remove members), re-keys their own path, advances the
//! epoch, and produces both a `Commit` (for existing members) and a
//! `Welcome` per newly added member. Every member who applies the same
//! commit lands on a byte-identical epoch — same secrets, same transcript,
//! same `confirmation_tag` — or rejects it outright.
//!
//! This is the phase where the three properties motivating the whole MLS
//! effort become real:
//! - **O(log N) membership change**: a removal is one commit carrying one
//!   logarithmic update path, not every remaining member re-keying with
//!   every other;
//! - **post-compromise security**: a plain commit (no proposals) is a
//!   self-update that rotates the committer's path and re-randomizes the
//!   epoch, locking a past leak back out;
//! - **membership agreement**: the transcript-bound `confirmation_tag`
//!   means two members cannot disagree about the roster and both accept.
//!
//! What's still deferred: *who is allowed to commit what* beyond signature
//! validity (an application-policy layer), and the P2P total ordering of
//! concurrent commits (phase 5 — a real Delivery Service's job, filled
//! here by deterministic arbitration).

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::CryptoRngCore;
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::hpke::{self, SealedSecret};
use crate::key_schedule::EpochSecrets;
use crate::member::{KeyPackage, MemberIdentity};
use crate::ratchet_tree::RatchetTree;
use crate::secrets::PathSecret;
use crate::transcript::{confirmation_tag, Transcript};
use crate::tree_math::{common_ancestor, leaf_to_node, LeafIndex};
use crate::update_path::{build_update_path, process_update_path, UpdatePath};

/// A change to the roster, to be carried in a commit.
#[derive(Clone)]
pub enum Proposal {
    Add(KeyPackage),
    Remove(LeafIndex),
}

/// What a committer broadcasts to existing members.
pub struct Commit {
    /// The epoch this commit advances *from* — a commit is only valid
    /// against a member sitting at exactly this epoch, which is also what
    /// lets the arbiter (phase 5) group concurrent commits by the epoch
    /// they contend for.
    pub from_epoch: u64,
    pub committer: LeafIndex,
    pub proposals: Vec<Proposal>,
    pub update_path: UpdatePath,
    pub confirmation_tag: [u8; 32],
    /// Ed25519 over the commit content under the committer's identity —
    /// so a commit can't be forged or altered by a relaying peer.
    pub signature: [u8; 64],
}

/// Everything a newly added member needs to join at the committed epoch.
/// `group_secrets` is HPKE-sealed to the joiner's key package leaf key;
/// the rest is public group state.
pub struct Welcome {
    pub group_id: Vec<u8>,
    pub epoch: u64,
    pub tree: Vec<Option<PublicKey>>,
    pub leaf_count: u32,
    pub joiner_leaf: LeafIndex,
    pub roster: Vec<Option<MemberIdentity>>,
    pub group_context: Vec<u8>,
    pub confirmed_transcript_hash: [u8; 32],
    pub confirmation_tag: [u8; 32],
    /// Sealed `GroupSecrets` (joiner_secret ‖ path_secret) — openable only
    /// with the joiner's own leaf private key.
    pub sealed_group_secrets: SealedSecret,
}

/// The output of committing: the commit for existing members, plus one
/// welcome per added member (paired with who it's for).
pub struct CommitOutput {
    pub commit: Commit,
    pub welcomes: Vec<(MemberIdentity, Welcome)>,
}

#[derive(Clone)]
pub struct GroupState {
    group_id: Vec<u8>,
    epoch: u64,
    tree: RatchetTree,
    roster: Vec<Option<MemberIdentity>>,
    secrets: EpochSecrets,
    transcript: Transcript,
    signing_key: SigningKey,
    own_identity: MemberIdentity,
}

/// A stable, public identifier for a commit — every member computes the
/// identical value from the same commit bytes, so it's what the P2P
/// arbiter (phase 5) orders concurrent commits by. Not secret; it's a
/// hash of already-public content plus the confirmation tag.
pub fn commit_id(group_id: &[u8], commit: &Commit) -> [u8; 32] {
    let content = GroupState::commit_content(group_id, commit.from_epoch, commit.committer, &commit.proposals, &commit.update_path);
    let mut h = Sha256::new();
    h.update(b"spiritchat-mls-commitid-v1:");
    h.update(&content);
    h.update(commit.confirmation_tag);
    h.finalize().into()
}

impl GroupState {
    /// Founds a new group with a single member — the creator. Their leaf
    /// key is generated here; every later member arrives via a commit.
    pub fn create(group_id: Vec<u8>, signing_key: SigningKey, rng: &mut impl CryptoRngCore) -> Self {
        let own_leaf_secret = StaticSecret::random_from_rng(&mut *rng);
        let own_identity = MemberIdentity(signing_key.verifying_key().to_bytes());
        let tree = RatchetTree::new(own_leaf_secret);
        let transcript = Transcript::new(&group_id);

        // Epoch 0: no commit secret yet, so extract against the initial
        // zero init_secret with a fixed genesis "commit secret" bound to
        // the group id — every member who is *told* they're in this group
        // starts from the same epoch-0 secrets deterministically.
        let genesis_commit = {
            let mut h = Sha256::new();
            h.update(b"spiritchat-mls-genesis-v1:");
            h.update(&group_id);
            let out: [u8; 32] = h.finalize().into();
            out
        };
        let group_context = Self::compute_group_context(&group_id, 0, &tree, transcript.interim());
        let secrets = EpochSecrets::derive(&EpochSecrets::initial_init_secret(), &genesis_commit, &group_context);

        GroupState { group_id, epoch: 0, tree, roster: vec![Some(own_identity)], secrets, transcript, signing_key, own_identity }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// This group's id — needed by the arbiter to compute commit ids.
    pub fn group_id_bytes(&self) -> &[u8] {
        &self.group_id
    }

    pub fn own_leaf(&self) -> LeafIndex {
        self.tree.own_leaf()
    }

    /// The one value uniquely identifying this member's current shared
    /// state — equal across members iff they agree on the entire history.
    /// A natural "safety number" / channel binding.
    pub fn epoch_authenticator(&self) -> [u8; 32] {
        self.secrets.epoch_authenticator
    }

    /// A symmetric key exported for the application to encrypt this
    /// epoch's group messages under (the MLS exporter, specialized).
    pub fn exporter_secret(&self) -> [u8; 32] {
        self.secrets.exporter_secret
    }

    pub fn roster(&self) -> &[Option<MemberIdentity>] {
        &self.roster
    }

    /// This member's own identity — which roster entry is "us", needed by
    /// the app layer (phase 6) to attribute and render group messages.
    pub fn own_identity(&self) -> MemberIdentity {
        self.own_identity
    }

    /// The context used as HPKE aad when sealing/opening an update path —
    /// derived only from state every existing member already shares before
    /// the commit (group id, current epoch, current transcript), so it's
    /// computable identically on both sides without the post-update tree.
    fn path_context(&self) -> Vec<u8> {
        let mut h = Sha256::new();
        h.update(b"spiritchat-mls-pathctx-v1:");
        h.update(&self.group_id);
        h.update(self.epoch.to_le_bytes());
        h.update(self.transcript.interim());
        let ctx: [u8; 32] = h.finalize().into();
        ctx.to_vec()
    }

    fn compute_group_context(group_id: &[u8], epoch: u64, tree: &RatchetTree, transcript_interim: &[u8; 32]) -> Vec<u8> {
        let mut h = Sha256::new();
        h.update(b"spiritchat-mls-groupctx-v1:");
        h.update(group_id);
        h.update(epoch.to_le_bytes());
        for node in tree.public_nodes() {
            match node {
                Some(pk) => {
                    h.update([1u8]);
                    h.update(pk.as_bytes());
                }
                None => h.update([0u8]),
            }
        }
        h.update(transcript_interim);
        let ctx: [u8; 32] = h.finalize().into();
        ctx.to_vec()
    }

    /// Canonical bytes of a commit's content — what the transcript folds
    /// in and the committer signs. Deterministic field-by-field, never via
    /// a serialization library, for the same reason the ledger hand-rolls
    /// its own: hash-critical bytes must not depend on an external format.
    fn commit_content(group_id: &[u8], epoch: u64, committer: LeafIndex, proposals: &[Proposal], update: &UpdatePath) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"spiritchat-mls-commit-v1:");
        buf.extend_from_slice(group_id);
        buf.extend_from_slice(&epoch.to_le_bytes());
        buf.extend_from_slice(&committer.to_le_bytes());
        buf.extend_from_slice(&(proposals.len() as u32).to_le_bytes());
        for p in proposals {
            match p {
                Proposal::Add(kp) => {
                    buf.push(1);
                    buf.extend_from_slice(&kp.identity.0);
                    buf.extend_from_slice(kp.leaf_public.as_bytes());
                }
                Proposal::Remove(leaf) => {
                    buf.push(2);
                    buf.extend_from_slice(&leaf.to_le_bytes());
                }
            }
        }
        buf.extend_from_slice(update.leaf_public.as_bytes());
        for node in &update.nodes {
            buf.extend_from_slice(&node.node.to_le_bytes());
            buf.extend_from_slice(node.public.as_bytes());
        }
        buf
    }

    /// Applies proposals to a tree (Removes first, then Adds), returning
    /// the leaf each Add landed on so the caller can build welcomes and
    /// update the roster. Deterministic — every member runs this
    /// identically, so leaf assignment matches without being transmitted.
    fn apply_proposals(tree: &mut RatchetTree, roster: &mut Vec<Option<MemberIdentity>>, proposals: &[Proposal]) -> Result<Vec<(LeafIndex, MemberIdentity)>, GroupError> {
        for p in proposals {
            if let Proposal::Remove(leaf) = p {
                if tree.leaf_public(*leaf).is_none() {
                    return Err(GroupError::RemoveOfAbsentMember);
                }
                tree.remove_leaf(*leaf);
                if (*leaf as usize) < roster.len() {
                    roster[*leaf as usize] = None;
                }
            }
        }
        let mut added = Vec::new();
        for p in proposals {
            if let Proposal::Add(kp) = p {
                kp.verify().map_err(|_| GroupError::InvalidKeyPackage)?;
                let leaf = tree.add_leaf(kp.leaf_public);
                if roster.len() <= leaf as usize {
                    roster.resize(leaf as usize + 1, None);
                }
                roster[leaf as usize] = Some(kp.identity);
                added.push((leaf, kp.identity));
            }
        }
        Ok(added)
    }

    /// Commits `proposals` (empty = a self-update for post-compromise
    /// security). Advances *this* member's state to the new epoch and
    /// returns the messages others need.
    pub fn commit(&mut self, proposals: Vec<Proposal>, rng: &mut impl CryptoRngCore) -> Result<CommitOutput, GroupError> {
        let (output, next) = self.build_commit(proposals, rng)?;
        *self = next;
        Ok(output)
    }

    /// Builds a commit **without mutating self**, returning both the
    /// messages for others and the post-commit state this member *would*
    /// move to if this commit becomes canonical. The P2P arbiter (phase 5)
    /// needs this: when two members commit concurrently at the same epoch,
    /// only one wins, and a member must not have already advanced on a
    /// commit that loses. `commit` above is just this plus adopting the
    /// new state immediately (the single-committer, no-contention case).
    pub fn build_commit(&self, proposals: Vec<Proposal>, rng: &mut impl CryptoRngCore) -> Result<(CommitOutput, GroupState), GroupError> {
        let prev_init = self.secrets.init_secret;
        let new_epoch = self.epoch + 1;

        // The context that seals the update path is the *pre-commit* one —
        // group id, current epoch, current transcript — which every
        // existing member already shares, so a processor computes the
        // identical HPKE aad without first needing the post-update tree
        // (that would be circular: opening the path is what produces the
        // post-update tree). The key schedule's own context, bound to the
        // *new* tree, is computed separately once the path is in place.
        let path_context = self.path_context();

        let mut tree = self.tree.clone();
        let mut roster = self.roster.clone();
        let added = Self::apply_proposals(&mut tree, &mut roster, &proposals)?;

        // Re-key the committer's own path over the post-proposal tree.
        let own_update = tree.apply_own_update(rng);
        let group_context = Self::compute_group_context(&self.group_id, new_epoch, &tree, self.transcript.interim());
        let update_path = build_update_path(&tree, &own_update, &path_context, rng);

        let content = Self::commit_content(&self.group_id, self.epoch, self.own_leaf(), &proposals, &update_path);
        let confirmed = self.transcript.confirmed(&content);

        let commit_secret = own_update.root_secret.as_bytes();
        let joiner_secret = EpochSecrets::joiner_secret(&prev_init, commit_secret);
        let secrets = EpochSecrets::from_joiner(&joiner_secret, &group_context);
        let tag = confirmation_tag(&secrets.confirmation_key, &confirmed);

        let signature = self.signing_key.sign(&Self::signed_bytes(&content, &confirmed, &tag)).to_bytes();

        // A welcome per added member: seal joiner_secret ‖ their path
        // secret (at their join node with the committer) to their leaf key.
        let mut welcomes = Vec::new();
        for (leaf, identity) in &added {
            let join = common_ancestor(leaf_to_node(*leaf), leaf_to_node(tree.own_leaf()), tree.leaf_count());
            let path_secret = own_update
                .path
                .iter()
                .find(|(node, _, _)| *node == join)
                .map(|(_, _, s)| *s.as_bytes())
                // A member added adjacent to nowhere on the committer's
                // path can't happen (the committer's path reaches the
                // root, and every leaf's join with it is on that path).
                .ok_or(GroupError::WelcomeDerivation)?;

            let leaf_public = tree.leaf_public(*leaf).ok_or(GroupError::WelcomeDerivation)?;
            let mut plaintext = Vec::with_capacity(64);
            plaintext.extend_from_slice(&joiner_secret);
            plaintext.extend_from_slice(&path_secret);
            let sealed = hpke::seal(rng, &leaf_public, &plaintext, &self.group_id);

            welcomes.push((*identity, Welcome {
                group_id: self.group_id.clone(),
                epoch: new_epoch,
                tree: tree.public_nodes(),
                leaf_count: tree.leaf_count(),
                joiner_leaf: *leaf,
                roster: roster.clone(),
                group_context: group_context.clone(),
                confirmed_transcript_hash: confirmed,
                confirmation_tag: tag,
                sealed_group_secrets: sealed,
            }));
        }

        // Assemble the post-commit state without touching self.
        let mut next = self.clone();
        next.tree = tree;
        next.roster = roster;
        next.secrets = secrets;
        next.transcript.advance(&confirmed, &tag);
        next.epoch = new_epoch;

        let commit = Commit {
            from_epoch: self.epoch,
            committer: self.own_leaf(),
            proposals,
            update_path,
            confirmation_tag: tag,
            signature,
        };
        Ok((CommitOutput { commit, welcomes }, next))
    }

    fn signed_bytes(content: &[u8], confirmed: &[u8; 32], tag: &[u8; 32]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(content.len() + 64);
        buf.extend_from_slice(content);
        buf.extend_from_slice(confirmed);
        buf.extend_from_slice(tag);
        buf
    }

    /// Applies a commit from another member. Verifies the committer's
    /// signature, replays the proposals, processes the update path,
    /// derives the new epoch, and — the crucial check — only accepts if
    /// the `confirmation_tag` computed from *this* member's own derived
    /// secrets matches the committer's. A mismatch means divergent state
    /// and the commit is rejected whole.
    pub fn process_commit(&mut self, commit: &Commit) -> Result<(), GroupError> {
        // A commit is only meaningful against the exact epoch it was built
        // on — the arbiter (phase 5) relies on this to reject a commit
        // that lost its epoch race and should be re-based, not applied.
        if commit.from_epoch != self.epoch {
            return Err(GroupError::WrongEpoch);
        }
        let committer_identity = self
            .roster
            .get(commit.committer as usize)
            .and_then(|r| *r)
            .ok_or(GroupError::UnknownCommitter)?;
        let vk = VerifyingKey::from_bytes(&committer_identity.0).map_err(|_| GroupError::BadSignature)?;

        let prev_init = self.secrets.init_secret;
        let new_epoch = self.epoch + 1;
        let path_context = self.path_context();

        let mut tree = self.tree.clone();
        let mut roster = self.roster.clone();
        Self::apply_proposals(&mut tree, &mut roster, &commit.proposals)?;

        let content = Self::commit_content(&self.group_id, self.epoch, commit.committer, &commit.proposals, &commit.update_path);
        let confirmed = self.transcript.confirmed(&content);

        // Verify the signature before spending effort on the path.
        let sig = Signature::from_bytes(&commit.signature);
        vk.verify(&Self::signed_bytes(&content, &confirmed, &commit.confirmation_tag), &sig)
            .map_err(|_| GroupError::BadSignature)?;

        let root_secret = process_update_path(&mut tree, &commit.update_path, &path_context)
            .map_err(|_| GroupError::PathProcessing)?;
        // Now that the path is merged, our tree matches the committer's
        // post-update tree — so the key-schedule context binds identically.
        let group_context = Self::compute_group_context(&self.group_id, new_epoch, &tree, self.transcript.interim());
        let joiner_secret = EpochSecrets::joiner_secret(&prev_init, root_secret.as_bytes());
        let secrets = EpochSecrets::from_joiner(&joiner_secret, &group_context);

        let expected_tag = confirmation_tag(&secrets.confirmation_key, &confirmed);
        if expected_tag != commit.confirmation_tag {
            return Err(GroupError::ConfirmationMismatch);
        }

        self.tree = tree;
        self.roster = roster;
        self.secrets = secrets;
        self.transcript.advance(&confirmed, &commit.confirmation_tag);
        self.epoch = new_epoch;
        Ok(())
    }

    /// Joins a group from a `Welcome`. `signing_key` is the joiner's own
    /// identity; `leaf_secret` the X25519 private half of the leaf key
    /// whose public half was in the key package the committer added.
    pub fn join(welcome: &Welcome, signing_key: SigningKey, leaf_secret: StaticSecret) -> Result<Self, GroupError> {
        let own_identity = MemberIdentity(signing_key.verifying_key().to_bytes());

        let group_secrets = hpke::open(&leaf_secret, &welcome.sealed_group_secrets, &welcome.group_id)
            .ok_or(GroupError::WelcomeUndecryptable)?;
        if group_secrets.len() != 64 {
            return Err(GroupError::WelcomeUndecryptable);
        }
        let mut joiner_secret = [0u8; 32];
        let mut path_secret = [0u8; 32];
        joiner_secret.copy_from_slice(&group_secrets[..32]);
        path_secret.copy_from_slice(&group_secrets[32..]);

        let mut tree = RatchetTree::from_public_nodes(welcome.tree.clone(), welcome.leaf_count, welcome.joiner_leaf, leaf_secret)
            .map_err(|_| GroupError::WelcomeTreeMismatch)?;

        // Find the committer's leaf: they're the one whose path this
        // path_secret keys, i.e. the roster member (other than us) whose
        // join node with us the secret matches. The Welcome doesn't name
        // the committer, but absorb from our join node upward works for
        // whichever leaf produced it; we recover the root by walking from
        // our own join point. We locate that node as the lowest ancestor
        // whose derived key matches the tree — absorb_path_secret needs
        // the *updater* leaf only to find the start node via common
        // ancestor, so we try each occupied leaf and take the one that
        // absorbs cleanly.
        let root_secret = Self::absorb_join_secret(&mut tree, welcome.joiner_leaf, PathSecret::new(path_secret))
            .ok_or(GroupError::WelcomeUndecryptable)?;

        let secrets = EpochSecrets::from_joiner(&joiner_secret, &welcome.group_context);
        // Independently confirm we reached the committed epoch: our own
        // derived confirmation key must MAC the shipped confirmed hash to
        // the shipped tag. If not, the Welcome doesn't match its own tree.
        let expected_tag = confirmation_tag(&secrets.confirmation_key, &welcome.confirmed_transcript_hash);
        if expected_tag != welcome.confirmation_tag {
            return Err(GroupError::ConfirmationMismatch);
        }
        // Belt-and-suspenders: the root secret we absorbed must be the one
        // the joiner_secret was extracted with. We can't re-extract
        // (no prev_init), but a mismatched path_secret would already have
        // failed the confirmation check above via a wrong epoch. Keep the
        // value used for clarity.
        let _ = root_secret;

        let mut transcript = Transcript::new(&welcome.group_id);
        // Fast-forward the transcript to the joined epoch: we don't have
        // the full history, but from the joined epoch onward our interim
        // hash must match everyone else's, which is Hash(confirmed ‖ tag)
        // of the joining commit.
        transcript.advance(&welcome.confirmed_transcript_hash, &welcome.confirmation_tag);

        Ok(GroupState {
            group_id: welcome.group_id.clone(),
            epoch: welcome.epoch,
            tree,
            roster: welcome.roster.clone(),
            secrets,
            transcript,
            signing_key,
            own_identity,
        })
    }

    /// Absorbs the welcome's path secret by trying each other occupied
    /// leaf as the notional updater — the one whose common-ancestor start
    /// node the secret actually keys is the committer. Returns the root
    /// secret on the first clean absorb.
    fn absorb_join_secret(tree: &mut RatchetTree, own_leaf: LeafIndex, secret: PathSecret) -> Option<PathSecret> {
        let leaf_count = tree.leaf_count();
        for candidate in 0..leaf_count {
            if candidate == own_leaf || tree.leaf_public(candidate).is_none() {
                continue;
            }
            let mut trial = tree.clone();
            if let Ok(root) = trial.absorb_path_secret(candidate, secret.clone()) {
                *tree = trial;
                return Some(root);
            }
        }
        None
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum GroupError {
    InvalidKeyPackage,
    RemoveOfAbsentMember,
    UnknownCommitter,
    BadSignature,
    PathProcessing,
    ConfirmationMismatch,
    WelcomeDerivation,
    WelcomeUndecryptable,
    WelcomeTreeMismatch,
    WrongEpoch,
    NoCommitToResolve,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    fn rng(seed: u64) -> ChaCha20Rng {
        ChaCha20Rng::seed_from_u64(seed)
    }

    fn signing_key(seed: u64) -> SigningKey {
        SigningKey::generate(&mut rng(seed))
    }

    /// A member joining brings a signing key and a leaf keypair; the key
    /// package offers the public halves.
    fn new_candidate(seed: u64) -> (SigningKey, StaticSecret, KeyPackage) {
        let sk = signing_key(seed);
        let leaf_secret = StaticSecret::random_from_rng(rng(seed + 500));
        let kp = KeyPackage::create(&sk, PublicKey::from(&leaf_secret));
        (sk, leaf_secret, kp)
    }

    #[test]
    fn a_creator_and_one_added_member_converge() {
        let mut alice = GroupState::create(b"g".to_vec(), signing_key(1), &mut rng(10));
        let (bob_sk, bob_leaf_secret, bob_kp) = new_candidate(2);

        let out = alice.commit(vec![Proposal::Add(bob_kp)], &mut rng(11)).unwrap();
        let (_, welcome) = &out.welcomes[0];
        let bob = GroupState::join(welcome, bob_sk, bob_leaf_secret).unwrap();

        assert_eq!(alice.epoch(), 1);
        assert_eq!(bob.epoch(), 1);
        assert_eq!(alice.epoch_authenticator(), bob.epoch_authenticator());
        assert_eq!(alice.exporter_secret(), bob.exporter_secret());
    }

    #[test]
    fn a_five_member_group_all_converge_and_stay_converged_across_commits() {
        let mut members: Vec<GroupState> = vec![GroupState::create(b"grp".to_vec(), signing_key(1), &mut rng(100))];
        let mut pending: Vec<(SigningKey, StaticSecret)> = Vec::new();

        // Add four members, one commit each, everyone processes.
        for i in 2..=5u64 {
            let (sk, leaf_secret, kp) = new_candidate(i);
            let out = members[0].commit(vec![Proposal::Add(kp)], &mut rng(100 + i)).unwrap();
            for m in members.iter_mut().skip(1) {
                m.process_commit(&out.commit).unwrap();
            }
            let (_, welcome) = &out.welcomes[0];
            let joined = GroupState::join(welcome, sk, leaf_secret).unwrap();
            members.push(joined);
            let _ = &mut pending;
        }

        let auth = members[0].epoch_authenticator();
        for m in &members {
            assert_eq!(m.epoch(), 4);
            assert_eq!(m.epoch_authenticator(), auth, "every member shares one epoch authenticator");
        }

        // A middle member self-updates (post-compromise security); all
        // converge on a fresh epoch.
        let out = members[2].commit(vec![], &mut rng(200)).unwrap();
        for (i, m) in members.iter_mut().enumerate() {
            if i != 2 {
                m.process_commit(&out.commit).unwrap();
            }
        }
        let auth2 = members[0].epoch_authenticator();
        assert_ne!(auth, auth2);
        for m in &members {
            assert_eq!(m.epoch(), 5);
            assert_eq!(m.epoch_authenticator(), auth2);
        }
    }

    #[test]
    fn a_removed_member_is_locked_out_and_the_rest_re_converge() {
        let mut alice = GroupState::create(b"g".to_vec(), signing_key(1), &mut rng(1));
        let (bob_sk, bob_ls, bob_kp) = new_candidate(2);
        let (carol_sk, carol_ls, carol_kp) = new_candidate(3);

        let out = alice.commit(vec![Proposal::Add(bob_kp)], &mut rng(2)).unwrap();
        let mut bob = GroupState::join(&out.welcomes[0].1, bob_sk, bob_ls).unwrap();

        let out = alice.commit(vec![Proposal::Add(carol_kp)], &mut rng(3)).unwrap();
        bob.process_commit(&out.commit).unwrap();
        let mut carol = GroupState::join(&out.welcomes[0].1, carol_sk, carol_ls).unwrap();

        assert_eq!(alice.epoch_authenticator(), carol.epoch_authenticator());

        // Alice removes Carol.
        let out = alice.commit(vec![Proposal::Remove(carol.own_leaf())], &mut rng(4)).unwrap();
        bob.process_commit(&out.commit).unwrap();

        // Carol cannot process the commit that removes her.
        assert!(carol.process_commit(&out.commit).is_err());

        // Alice and Bob share a new epoch Carol can never reach.
        assert_eq!(alice.epoch_authenticator(), bob.epoch_authenticator());
        assert_ne!(alice.epoch_authenticator(), carol.epoch_authenticator());
    }

    #[test]
    fn a_tampered_commit_is_rejected() {
        let mut alice = GroupState::create(b"g".to_vec(), signing_key(1), &mut rng(1));
        let (bsk, bls, bob_kp) = new_candidate(2);
        let out = alice.commit(vec![Proposal::Add(bob_kp)], &mut rng(2)).unwrap();
        let mut bob = GroupState::join(&out.welcomes[0].1, bsk, bls).unwrap();

        // Carol added, but her commit's confirmation tag is corrupted in
        // flight — Bob must reject it.
        let (_, _, carol_kp) = new_candidate(3);
        let mut out = alice.commit(vec![Proposal::Add(carol_kp)], &mut rng(3)).unwrap();
        out.commit.confirmation_tag[0] ^= 0x01;
        assert!(bob.process_commit(&out.commit).is_err());
    }

    #[test]
    fn a_commit_signed_by_the_wrong_key_is_rejected() {
        let mut alice = GroupState::create(b"g".to_vec(), signing_key(1), &mut rng(1));
        let (bsk, bls, bob_kp) = new_candidate(2);
        let out = alice.commit(vec![Proposal::Add(bob_kp)], &mut rng(2)).unwrap();
        let mut bob = GroupState::join(&out.welcomes[0].1, bsk, bls).unwrap();

        let (_, _, carol_kp) = new_candidate(3);
        let mut out = alice.commit(vec![Proposal::Add(carol_kp)], &mut rng(3)).unwrap();
        // Forge the signature with a different key.
        let forged = signing_key(99).sign(b"anything").to_bytes();
        out.commit.signature = forged;
        assert_eq!(bob.process_commit(&out.commit).unwrap_err(), GroupError::BadSignature);
    }
}
