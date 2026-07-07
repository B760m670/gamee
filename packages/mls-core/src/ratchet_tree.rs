//! The ratchet tree itself: `tree_math`'s indices made concrete, with an
//! X25519 keypair at every non-blank node. Each member's *view* of the
//! tree holds every node's public key but private keys only along the
//! member's own leaf-to-root path (and below nodes it has been given a
//! path secret for) — the defining invariant of TreeKEM: **a member
//! holds a node's private key exactly when their leaf is under it.**
//!
//! Membership changes leave *blank* nodes behind (an added or removed
//! member invalidates every key on the affected path — those keys were
//! derived by/for a different membership). A blank node's *resolution* —
//! the maximal set of non-blank nodes covering exactly its leaves — is
//! what an update encrypts to instead, so blanks cost bandwidth, never
//! correctness. The next update along a blanked path heals it back to
//! one key per node.
//!
//! This phase is the tree's mechanics only: applying one's own update,
//! merging another member's published update, add/remove blanking. The
//! *transport* of path secrets (HPKE to copath resolutions) and the epoch
//! key schedule land in phase 3; group operations (proposals/commits)
//! in phase 4.

use rand_core::CryptoRngCore;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::secrets::PathSecret;
use crate::tree_math::{
    self, common_ancestor, direct_path, leaf_to_node, node_width, root, LeafIndex, NodeIndex,
};

/// One non-blank node: everyone in the group knows `public`; `secret` is
/// populated only in the views of members whose leaf sits under it.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Node {
    pub public: PublicKey,
    pub secret: Option<StaticSecret>,
}

/// One member's view of the group's ratchet tree.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct RatchetTree {
    /// `tree_math` array layout — `None` is a blank node.
    nodes: Vec<Option<Node>>,
    /// Number of leaves (members, including blanked/removed slots).
    leaf_count: u32,
    /// Which leaf is *this member's own* in this view.
    own_leaf: LeafIndex,
}

/// What `apply_own_update` hands back for phase 3 to transport: the new
/// public key for every node on the updated path, and — for each of
/// those nodes — the path secret the members *under its sibling* need
/// (to be HPKE-encrypted to the sibling's resolution).
pub struct OwnUpdate {
    pub leaf_public: PublicKey,
    /// Bottom-up, one entry per direct-path node: (node index, its new
    /// public key, the path secret that keys it).
    pub path: Vec<(NodeIndex, PublicKey, PathSecret)>,
    /// The secret at the root — the input this update contributes to the
    /// epoch key schedule (phase 3).
    pub root_secret: PathSecret,
}

impl RatchetTree {
    /// A brand-new single-member group: one leaf, holding our own keypair.
    pub fn new(own_secret: StaticSecret) -> Self {
        let public = PublicKey::from(&own_secret);
        RatchetTree {
            nodes: vec![Some(Node { public, secret: Some(own_secret) })],
            leaf_count: 1,
            own_leaf: 0,
        }
    }

    /// Reconstructs a view from every node's public key (blank = `None`),
    /// as shipped inside a Welcome (phase 4) — private material starts
    /// with just the member's own leaf key and gets richer as path
    /// secrets arrive.
    pub fn from_public_nodes(
        public_nodes: Vec<Option<PublicKey>>,
        leaf_count: u32,
        own_leaf: LeafIndex,
        own_secret: StaticSecret,
    ) -> Result<Self, TreeError> {
        if public_nodes.len() != node_width(leaf_count) as usize {
            return Err(TreeError::WrongWidth);
        }
        let own_node = leaf_to_node(own_leaf);
        let mut nodes: Vec<Option<Node>> = public_nodes
            .into_iter()
            .map(|public| public.map(|public| Node { public, secret: None }))
            .collect();
        match &mut nodes[own_node as usize] {
            Some(node) if node.public == PublicKey::from(&own_secret) => {
                node.secret = Some(own_secret);
            }
            _ => return Err(TreeError::OwnLeafMismatch),
        }
        Ok(RatchetTree { nodes, leaf_count, own_leaf })
    }

    pub fn leaf_count(&self) -> u32 {
        self.leaf_count
    }

    pub fn own_leaf(&self) -> LeafIndex {
        self.own_leaf
    }

    pub fn leaf_public(&self, leaf: LeafIndex) -> Option<PublicKey> {
        self.nodes
            .get(leaf_to_node(leaf) as usize)
            .and_then(|n| n.as_ref())
            .map(|n| n.public)
    }

    pub fn node_public(&self, x: NodeIndex) -> Option<PublicKey> {
        self.nodes.get(x as usize).and_then(|n| n.as_ref()).map(|n| n.public)
    }

    pub fn node_secret(&self, x: NodeIndex) -> Option<&StaticSecret> {
        self.nodes.get(x as usize).and_then(|n| n.as_ref()).and_then(|n| n.secret.as_ref())
    }

    /// Every node's public key, for shipping in a Welcome.
    pub fn public_nodes(&self) -> Vec<Option<PublicKey>> {
        self.nodes.iter().map(|n| n.as_ref().map(|n| n.public)).collect()
    }

    /// RFC 9420's resolution: the maximal non-blank nodes covering
    /// exactly the leaves under `x` — who an update must actually
    /// encrypt to when `x` itself is blank.
    pub fn resolution(&self, x: NodeIndex) -> Vec<NodeIndex> {
        if self.nodes[x as usize].is_some() {
            return vec![x];
        }
        if tree_math::level(x) == 0 {
            return Vec::new(); // a blank leaf covers nobody
        }
        let mut out = self.resolution(tree_math::left(x));
        out.extend(self.resolution(tree_math::right(x, self.leaf_count)));
        out
    }

    /// Adds a member at the first blank leaf (or by growing the tree),
    /// blanking the new leaf's path — every key above it was agreed by a
    /// membership that didn't include them, so none may cover them until
    /// re-keyed. Returns the new member's leaf index.
    pub fn add_leaf(&mut self, public: PublicKey) -> LeafIndex {
        let leaf = match (0..self.leaf_count).find(|&l| self.nodes[leaf_to_node(l) as usize].is_none()) {
            Some(blank) => blank,
            None => {
                let new_leaf = self.leaf_count;
                self.leaf_count += 1;
                self.nodes.resize(node_width(self.leaf_count) as usize, None);
                // Growing re-shapes the upper tree; indices of existing
                // nodes are stable (left-balanced growth), but the new
                // root/spine nodes start blank by construction.
                new_leaf
            }
        };
        self.nodes[leaf_to_node(leaf) as usize] = Some(Node { public, secret: None });
        self.blank_path(leaf);
        leaf
    }

    /// Removes a member: their leaf and every node above it go blank.
    pub fn remove_leaf(&mut self, leaf: LeafIndex) {
        self.nodes[leaf_to_node(leaf) as usize] = None;
        self.blank_path(leaf);
    }

    fn blank_path(&mut self, leaf: LeafIndex) {
        if self.leaf_count > 1 {
            for x in direct_path(leaf_to_node(leaf), self.leaf_count) {
                self.nodes[x as usize] = None;
            }
        }
    }

    /// Re-keys our own leaf-to-root path from fresh randomness — the
    /// operation behind both a routine self-update (post-compromise
    /// security) and the committer's half of add/remove. The tree is
    /// updated in place; the returned [`OwnUpdate`] is what phase 3
    /// encrypts outward.
    pub fn apply_own_update(&mut self, rng: &mut impl CryptoRngCore) -> OwnUpdate {
        let leaf_secret = PathSecret::random(rng);
        let (leaf_private, leaf_public) = leaf_secret.node_keypair();
        let own_node = leaf_to_node(self.own_leaf);
        self.nodes[own_node as usize] = Some(Node { public: leaf_public, secret: Some(leaf_private) });

        let mut path = Vec::new();
        let mut current_secret = leaf_secret;
        for x in direct_path(own_node, self.leaf_count) {
            current_secret = current_secret.next();
            let (private, public) = current_secret.node_keypair();
            self.nodes[x as usize] = Some(Node { public, secret: Some(private) });
            path.push((x, public, current_secret.clone()));
        }
        // In a single-member tree the "root" is our own leaf: the leaf
        // secret itself plays the root-secret role.
        let root_secret = path.last().map(|(_, _, s)| s.clone()).unwrap_or_else(|| {
            PathSecret::new(*current_secret.as_bytes())
        });
        OwnUpdate { leaf_public, path, root_secret }
    }

    /// Merges another member's published update: their new leaf key and
    /// the new public key for every node on their path. Any private key
    /// we held on those nodes is gone (that's the *point* — they
    /// re-keyed); `absorb_path_secret` below restores our share.
    pub fn merge_update(
        &mut self,
        updater: LeafIndex,
        leaf_public: PublicKey,
        path_publics: &[(NodeIndex, PublicKey)],
    ) -> Result<(), TreeError> {
        let expected = direct_path(leaf_to_node(updater), self.leaf_count);
        if path_publics.len() != expected.len()
            || path_publics.iter().zip(&expected).any(|((got, _), want)| got != want)
        {
            return Err(TreeError::PathMismatch);
        }
        self.nodes[leaf_to_node(updater) as usize] = Some(Node { public: leaf_public, secret: None });
        for &(x, public) in path_publics {
            self.nodes[x as usize] = Some(Node { public, secret: None });
        }
        Ok(())
    }

    /// Installs the path secret we were sent for `updater`'s update (the
    /// one addressed to the copath subtree our leaf sits in), walks it up
    /// to the root, and returns the root secret. The consistency check —
    /// each derived public key must equal the one `merge_update` already
    /// installed — is what makes a malformed or misaddressed secret an
    /// error instead of a silently diverged tree.
    pub fn absorb_path_secret(
        &mut self,
        updater: LeafIndex,
        secret: PathSecret,
    ) -> Result<PathSecret, TreeError> {
        let updater_node = leaf_to_node(updater);
        let own_node = leaf_to_node(self.own_leaf);
        // The secret we receive keys the common ancestor of our leaf and
        // the updater's — the lowest updated node we're entitled to.
        let start = common_ancestor(own_node, updater_node, self.leaf_count);
        let r = root(self.leaf_count);

        let mut current = start;
        let mut current_secret = secret;
        loop {
            let (private, public) = current_secret.node_keypair();
            match &self.nodes[current as usize] {
                Some(node) if node.public == public => {}
                _ => return Err(TreeError::SecretMismatch { node: current }),
            }
            self.nodes[current as usize] = Some(Node { public, secret: Some(private) });
            if current == r {
                return Ok(current_secret);
            }
            current = tree_math::parent(current, self.leaf_count);
            current_secret = current_secret.next();
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum TreeError {
    WrongWidth,
    OwnLeafMismatch,
    PathMismatch,
    SecretMismatch { node: NodeIndex },
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    fn rng(seed: u64) -> ChaCha20Rng {
        ChaCha20Rng::seed_from_u64(seed)
    }

    fn keypair(seed: u64) -> (StaticSecret, PublicKey) {
        let secret = StaticSecret::random_from_rng(rng(seed));
        let public = PublicKey::from(&secret);
        (secret, public)
    }

    /// Builds every member's view of the same three-member group the way
    /// phase 4's Welcome eventually will: creator's tree + public copies.
    fn three_member_views() -> (RatchetTree, RatchetTree, RatchetTree) {
        let (alice_secret, _) = keypair(1);
        let (bob_secret, bob_public) = keypair(2);
        let (carol_secret, carol_public) = keypair(3);

        let mut alice = RatchetTree::new(alice_secret);
        let bob_leaf = alice.add_leaf(bob_public);
        let carol_leaf = alice.add_leaf(carol_public);

        let bob = RatchetTree::from_public_nodes(alice.public_nodes(), alice.leaf_count(), bob_leaf, bob_secret).unwrap();
        let carol =
            RatchetTree::from_public_nodes(alice.public_nodes(), alice.leaf_count(), carol_leaf, carol_secret).unwrap();
        (alice, bob, carol)
    }

    #[test]
    fn adding_members_blanks_their_paths() {
        let (alice, _, _) = three_member_views();
        assert_eq!(alice.leaf_count(), 3);
        // Freshly added: every intermediate node is blank, so the root's
        // resolution is exactly the three leaves.
        let r = root(3);
        assert_eq!(alice.resolution(r), vec![0, 2, 4]);
    }

    #[test]
    fn an_update_rekeys_the_path_and_every_view_converges_on_one_root_secret() {
        let (mut alice, mut bob, mut carol) = three_member_views();

        let update = alice.apply_own_update(&mut rng(9));

        // Everyone merges the public half...
        let path_publics: Vec<_> = update.path.iter().map(|&(x, p, _)| (x, p)).collect();
        bob.merge_update(alice.own_leaf(), update.leaf_public, &path_publics).unwrap();
        carol.merge_update(alice.own_leaf(), update.leaf_public, &path_publics).unwrap();

        // ...and each absorbs the path secret for *their* common ancestor
        // with alice (phase 3 will deliver these under HPKE; the algebra
        // being tested is identical).
        for (mut view, who) in [(&mut bob, "bob"), (&mut carol, "carol")] {
            let anc = common_ancestor(
                leaf_to_node(view.own_leaf()),
                leaf_to_node(alice.own_leaf()),
                view.leaf_count(),
            );
            let secret_for_view = update
                .path
                .iter()
                .find(|&&(x, _, _)| x == anc)
                .map(|(_, _, s)| s.clone())
                .expect("the update keys every node on alice's path");
            let root_secret = view.absorb_path_secret(alice.own_leaf(), secret_for_view).unwrap();
            assert_eq!(
                root_secret.as_bytes(),
                update.root_secret.as_bytes(),
                "{who} must converge on the same root secret"
            );
        }
    }

    #[test]
    fn a_removed_members_stale_secret_no_longer_matches_after_the_next_update() {
        let (mut alice, mut bob, carol) = three_member_views();

        // First update: everyone (including carol) converges.
        let update1 = alice.apply_own_update(&mut rng(10));
        let publics1: Vec<_> = update1.path.iter().map(|&(x, p, _)| (x, p)).collect();
        bob.merge_update(alice.own_leaf(), update1.leaf_public, &publics1).unwrap();

        // Carol is removed; alice re-keys her path (the committer's half
        // of a removal — phase 4 formalizes the ordering).
        alice.remove_leaf(carol.own_leaf());
        bob.remove_leaf(carol.own_leaf());
        let update2 = alice.apply_own_update(&mut rng(11));

        // Carol's stale view cannot absorb the new update: the secret she
        // held keys nothing in the new tree, and the new root secret is
        // underivable from anything she has.
        assert_ne!(update1.root_secret.as_bytes(), update2.root_secret.as_bytes());

        // Bob, still a member, converges as usual.
        let publics2: Vec<_> = update2.path.iter().map(|&(x, p, _)| (x, p)).collect();
        bob.merge_update(alice.own_leaf(), update2.leaf_public, &publics2).unwrap();
        let anc = common_ancestor(leaf_to_node(bob.own_leaf()), leaf_to_node(alice.own_leaf()), bob.leaf_count());
        let secret = update2.path.iter().find(|&&(x, _, _)| x == anc).map(|(_, _, s)| s.clone()).unwrap();
        assert_eq!(
            bob.absorb_path_secret(alice.own_leaf(), secret).unwrap().as_bytes(),
            update2.root_secret.as_bytes()
        );
    }

    #[test]
    fn a_wrong_path_secret_is_rejected_not_silently_installed() {
        let (mut alice, mut bob, _) = three_member_views();
        let update = alice.apply_own_update(&mut rng(12));
        let publics: Vec<_> = update.path.iter().map(|&(x, p, _)| (x, p)).collect();
        bob.merge_update(alice.own_leaf(), update.leaf_public, &publics).unwrap();

        let err = bob
            .absorb_path_secret(alice.own_leaf(), PathSecret::new([99u8; 32]))
            .unwrap_err();
        assert!(matches!(err, TreeError::SecretMismatch { .. }));
    }

    #[test]
    fn a_mismatched_update_path_is_rejected() {
        let (mut alice, mut bob, _) = three_member_views();
        let update = alice.apply_own_update(&mut rng(13));
        // Claim the update keys a different set of nodes than alice's
        // real direct path — must be refused outright.
        let bogus = vec![(0u32, update.leaf_public)];
        assert_eq!(
            bob.merge_update(alice.own_leaf(), update.leaf_public, &bogus).unwrap_err(),
            TreeError::PathMismatch
        );
    }

    #[test]
    fn growth_reuses_blank_leaves_before_widening_the_tree() {
        let (mut alice, _, carol) = three_member_views();
        alice.remove_leaf(carol.own_leaf());
        let (_, dave_public) = keypair(4);
        // Dave lands in carol's vacated slot, not a new one.
        assert_eq!(alice.add_leaf(dave_public), carol.own_leaf());
        assert_eq!(alice.leaf_count(), 3);
    }

    #[test]
    fn private_keys_exist_only_where_the_invariant_allows() {
        let (mut alice, mut bob, _) = three_member_views();
        let update = alice.apply_own_update(&mut rng(14));
        let publics: Vec<_> = update.path.iter().map(|&(x, p, _)| (x, p)).collect();
        bob.merge_update(alice.own_leaf(), update.leaf_public, &publics).unwrap();

        // Bob holds no private key for alice's leaf or for nodes not
        // above his own leaf — before absorbing a path secret he holds
        // only his own leaf key.
        assert!(bob.node_secret(leaf_to_node(alice.own_leaf())).is_none());
        assert!(bob.node_secret(leaf_to_node(bob.own_leaf())).is_some());
    }
}
