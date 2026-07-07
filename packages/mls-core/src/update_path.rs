//! Transporting a path update — the bridge between the ratchet tree
//! (phase 2) and HPKE (this phase). When a member re-keys their
//! leaf-to-root path, every *other* member needs exactly one of the new
//! path secrets: the one keying the node where their own path first joins
//! the updater's. RFC 9420's `UpdatePath` carries, per direct-path node,
//! its new public key plus that node's path secret **sealed once to each
//! node in the copath child's resolution** — so every member opens exactly
//! one sealed secret (the one addressed to the subtree they sit in) and
//! derives the root from there, while learning nothing about path secrets
//! below their join point.
//!
//! Cost is the whole point: a group of N gets O(log N) sealed secrets per
//! blank-free path, not one per member. Blank nodes only widen a single
//! node's resolution; they never force a member off this path.

use rand_core::CryptoRngCore;
use x25519_dalek::PublicKey;

use crate::hpke::{self, SealedSecret};
use crate::ratchet_tree::{OwnUpdate, RatchetTree, TreeError};
use crate::secrets::PathSecret;
use crate::tree_math::{self, common_ancestor, leaf_to_node, LeafIndex, NodeIndex};

/// One direct-path node's contribution: its fresh public key, and its
/// path secret sealed to each recipient node in the copath resolution.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct PathNode {
    pub node: NodeIndex,
    pub public: PublicKey,
    /// `(recipient node index, sealed path secret)` — one per node in the
    /// copath child's resolution. A recipient opens the entry whose node
    /// they hold a private key for.
    pub sealed: Vec<(NodeIndex, SealedSecret)>,
}

/// The published form of an update — everything a non-updating member
/// needs to converge, and nothing they shouldn't have (no plaintext path
/// secrets).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct UpdatePath {
    pub updater: LeafIndex,
    pub leaf_public: PublicKey,
    /// Bottom-up, one per direct-path node.
    pub nodes: Vec<PathNode>,
}

/// Binds a sealed secret to its position so it can't be replayed into
/// another group/epoch/node (the HPKE `aad`). `context` is the group's
/// per-epoch context bytes; the target node index pins each seal.
fn seal_aad(context: &[u8], target_node: NodeIndex) -> Vec<u8> {
    let mut aad = Vec::with_capacity(context.len() + 8);
    aad.extend_from_slice(context);
    aad.extend_from_slice(b":node:");
    aad.extend_from_slice(&target_node.to_le_bytes());
    aad
}

/// Seals `own_update` (from `tree.apply_own_update`) into a publishable
/// `UpdatePath`. Must be called on the tree *after* `apply_own_update`, so
/// the copath resolutions and their public keys reflect current state
/// (the copath side is untouched by the update, so this is well-defined).
pub fn build_update_path(
    tree: &RatchetTree,
    own_update: &OwnUpdate,
    context: &[u8],
    rng: &mut impl CryptoRngCore,
) -> UpdatePath {
    let own_leaf_node = leaf_to_node(tree.own_leaf());
    let mut nodes = Vec::with_capacity(own_update.path.len());

    for (i, (node, public, secret)) in own_update.path.iter().enumerate() {
        // The child of `node` that lies on the updater's own path — the
        // node below it we came up from. The *other* child is the copath
        // node whose subtree must receive this path secret.
        let lower = if i == 0 { own_leaf_node } else { own_update.path[i - 1].0 };
        let copath_child = tree_math::sibling(lower, tree.leaf_count());

        let mut sealed = Vec::new();
        for recipient in tree.resolution(copath_child) {
            if let Some(recipient_public) = tree.node_public(recipient) {
                let blob = hpke::seal(rng, &recipient_public, secret.as_bytes(), &seal_aad(context, *node));
                sealed.push((recipient, blob));
            }
        }
        nodes.push(PathNode { node: *node, public: *public, sealed });
    }

    UpdatePath { updater: tree.own_leaf(), leaf_public: own_update.leaf_public, nodes }
}

/// Applies a received `UpdatePath` to our own tree view: merges the public
/// path, finds the single sealed secret addressed to our subtree, opens
/// it, and ratchets it up to the root. Returns the new root path secret —
/// the commit secret feeding the epoch key schedule (phase 3's
/// `EpochSecrets::derive`). Every failure mode is an explicit error, never
/// a silently diverged view.
pub fn process_update_path(
    tree: &mut RatchetTree,
    update: &UpdatePath,
    context: &[u8],
) -> Result<PathSecret, TreeError> {
    let publics: Vec<(NodeIndex, PublicKey)> = update.nodes.iter().map(|n| (n.node, n.public)).collect();
    tree.merge_update(update.updater, update.leaf_public, &publics)?;

    // The node where our path meets the updater's is the one we must
    // recover a secret for.
    let own_node = leaf_to_node(tree.own_leaf());
    let updater_node = leaf_to_node(update.updater);
    let join = common_ancestor(own_node, updater_node, tree.leaf_count());

    let path_node = update
        .nodes
        .iter()
        .find(|n| n.node == join)
        .ok_or(TreeError::PathMismatch)?;

    // Among the seals for the join node, exactly one targets a node whose
    // private key we hold (the resolution node above our own leaf).
    for (recipient, sealed) in &path_node.sealed {
        if let Some(secret) = tree.node_secret(*recipient) {
            if let Some(opened) = hpke::open(secret, sealed, &seal_aad(context, join)) {
                let bytes: [u8; 32] = opened.as_slice().try_into().map_err(|_| TreeError::PathMismatch)?;
                return tree.absorb_path_secret(update.updater, PathSecret::new(bytes));
            }
        }
    }
    Err(TreeError::SecretMismatch { node: join })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key_schedule::EpochSecrets;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;
    use x25519_dalek::StaticSecret;

    fn rng(seed: u64) -> ChaCha20Rng {
        ChaCha20Rng::seed_from_u64(seed)
    }

    fn keypair(seed: u64) -> (StaticSecret, PublicKey) {
        let secret = StaticSecret::random_from_rng(rng(seed));
        let public = PublicKey::from(&secret);
        (secret, public)
    }

    /// Builds n members' views of one group (creator adds the rest), the
    /// way phase 4's Welcome eventually will.
    fn group_of(n: u64) -> Vec<RatchetTree> {
        let (creator_secret, _) = keypair(1000);
        let mut creator = RatchetTree::new(creator_secret);
        let mut secrets = Vec::new();
        for i in 1..n {
            let (s, p) = keypair(1000 + i);
            creator.add_leaf(p);
            secrets.push(s);
        }
        let mut views = vec![creator.clone()];
        for (i, s) in secrets.into_iter().enumerate() {
            let leaf = (i + 1) as LeafIndex;
            views.push(RatchetTree::from_public_nodes(creator.public_nodes(), creator.leaf_count(), leaf, s).unwrap());
        }
        views
    }

    #[test]
    fn every_member_converges_on_the_updaters_root_secret() {
        for n in 2u64..=12 {
            let mut views = group_of(n);
            let context = format!("group:epoch:{n}").into_bytes();

            // Member 0 updates.
            let own_update = views[0].apply_own_update(&mut rng(1));
            let update_path = build_update_path(&views[0], &own_update, &context, &mut rng(2));

            for m in 1..views.len() {
                let root = process_update_path(&mut views[m], &update_path, &context).unwrap();
                assert_eq!(
                    root.as_bytes(),
                    own_update.root_secret.as_bytes(),
                    "member {m} of {n} must converge on the updater's root secret"
                );
                // And therefore on the same epoch.
                let updater_epoch = EpochSecrets::derive(&[0u8; 32], own_update.root_secret.as_bytes(), &context);
                let member_epoch = EpochSecrets::derive(&[0u8; 32], root.as_bytes(), &context);
                assert_eq!(updater_epoch.epoch_authenticator, member_epoch.epoch_authenticator);
            }
        }
    }

    #[test]
    fn the_seal_count_is_logarithmic_not_linear() {
        // The whole reason TreeKEM exists: a blank-free path update seals
        // O(log N) times, not O(N). For a full 8-member tree after every
        // member has updated once (so no blanks remain), an update from a
        // leaf touches 3 direct-path nodes with one seal each.
        let mut views = group_of(8);
        let context = b"ctx".to_vec();
        // Heal all blanks: every member updates once.
        for m in 0..8 {
            let own = views[m].apply_own_update(&mut rng(50 + m as u64));
            let up = build_update_path(&views[m], &own, &context, &mut rng(70 + m as u64));
            for other in 0..8 {
                if other != m {
                    process_update_path(&mut views[other], &up, &context).unwrap();
                }
            }
        }
        let own = views[0].apply_own_update(&mut rng(99));
        let up = build_update_path(&views[0], &own, &context, &mut rng(100));
        let total_seals: usize = up.nodes.iter().map(|n| n.sealed.len()).sum();
        assert_eq!(up.nodes.len(), 3, "log2(8) direct-path nodes");
        assert_eq!(total_seals, 3, "one seal per node when no blanks remain");
    }

    #[test]
    fn a_removed_member_cannot_process_the_healing_update() {
        let mut views = group_of(4);
        let context = b"ctx".to_vec();

        // Member 3 is removed; member 0 commits the removal by blanking
        // 3's leaf and re-keying its own path.
        let removed = 3usize;
        for (i, v) in views.iter_mut().enumerate() {
            if i != removed {
                v.remove_leaf(removed as LeafIndex);
            }
        }
        let own = views[0].apply_own_update(&mut rng(7));
        let up = build_update_path(&views[0], &own, &context, &mut rng(8));

        // Remaining members converge.
        let root1 = process_update_path(&mut views[1], &up, &context).unwrap();
        assert_eq!(root1.as_bytes(), own.root_secret.as_bytes());

        // The removed member's stale view holds no key in any resolution
        // this update sealed to — it cannot recover the root secret.
        let removed_result = process_update_path(&mut views[removed], &up, &context);
        assert!(removed_result.is_err());
    }

    #[test]
    fn a_seal_replayed_under_the_wrong_context_is_rejected() {
        let mut views = group_of(3);
        let own = views[0].apply_own_update(&mut rng(1));
        let up = build_update_path(&views[0], &own, b"epoch-5", &mut rng(2));
        // Same update bytes, wrong epoch context — the HPKE aad binding
        // makes every seal unopenable.
        assert!(process_update_path(&mut views[1], &up, b"epoch-6").is_err());
    }
}
