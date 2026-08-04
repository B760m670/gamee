//! The array representation of a left-balanced binary tree, exactly as
//! RFC 9420's tree-math appendix defines it — every later phase (the
//! ratchet tree, path encryption) is built on these index computations,
//! so they live alone here where they can be tested to exhaustion with
//! no cryptography in sight.
//!
//! Layout: a tree over `n` leaves occupies `2n - 1` array slots. Leaves
//! sit at even indices (leaf `i` is node `2i`), intermediate nodes at odd
//! indices, and a node's *level* — its height above the leaves — is the
//! number of trailing one bits in its index. "Left-balanced" means: the
//! left subtree of any node is always a complete (full) subtree, and
//! extra leaves overflow rightward; this is what keeps every existing
//! node's index stable as the group grows, which matters because a
//! member's leaf index is baked into ciphertexts addressed to them.

/// A node's position in the array representation.
pub type NodeIndex = u32;
/// A leaf's ordinal (leaf `i` lives at node index `2i`).
pub type LeafIndex = u32;

/// How many array slots a tree over `n` leaves occupies.
pub fn node_width(n: u32) -> u32 {
    if n == 0 {
        0
    } else {
        2 * (n - 1) + 1
    }
}

pub fn leaf_to_node(leaf: LeafIndex) -> NodeIndex {
    2 * leaf
}

/// `Some(leaf)` if `x` is a leaf node, `None` for intermediate nodes.
pub fn node_to_leaf(x: NodeIndex) -> Option<LeafIndex> {
    if x % 2 == 0 {
        Some(x / 2)
    } else {
        None
    }
}

fn log2(x: u32) -> u32 {
    if x == 0 {
        0
    } else {
        31 - x.leading_zeros()
    }
}

/// A node's height above the leaf level — the number of trailing ones in
/// its index. Leaves (even indices) are level 0.
pub fn level(x: NodeIndex) -> u32 {
    x.trailing_ones()
}

/// The root of a tree over `n` leaves (`n >= 1`).
pub fn root(n: u32) -> NodeIndex {
    debug_assert!(n >= 1);
    (1 << log2(node_width(n))) - 1
}

/// The left child of intermediate node `x`. Never depends on tree size:
/// a left subtree is always complete.
pub fn left(x: NodeIndex) -> NodeIndex {
    let k = level(x);
    debug_assert!(k > 0, "a leaf has no children");
    x ^ (0x01 << (k - 1))
}

/// The right child of intermediate node `x` in a tree over `n` leaves.
/// In a left-balanced tree the *nominal* right child can lie beyond the
/// tree's width when the right subtree isn't full — step down its left
/// spine until inside.
pub fn right(x: NodeIndex, n: u32) -> NodeIndex {
    let k = level(x);
    debug_assert!(k > 0, "a leaf has no children");
    let mut r = x ^ (0x03 << (k - 1));
    while r >= node_width(n) {
        r = left(r);
    }
    r
}

/// One step up in the *complete* tree containing `x` — may land outside
/// a smaller tree's width; `parent` below corrects for that.
fn parent_step(x: NodeIndex) -> NodeIndex {
    let k = level(x);
    let b = (x >> (k + 1)) & 0x01;
    (x | (1 << k)) ^ (b << (k + 1))
}

/// The parent of `x` in a tree over `n` leaves. `x` must not be the root.
pub fn parent(x: NodeIndex, n: u32) -> NodeIndex {
    debug_assert!(x != root(n), "the root has no parent");
    let mut p = parent_step(x);
    while p >= node_width(n) {
        p = parent_step(p);
    }
    p
}

/// The other child of `x`'s parent.
pub fn sibling(x: NodeIndex, n: u32) -> NodeIndex {
    let p = parent(x, n);
    if x < p {
        right(p, n)
    } else {
        left(p)
    }
}

/// The nodes from `x`'s parent up to and including the root — the set a
/// member re-keys when they update (their leaf's path to the root is
/// exactly what a TreeKEM update replaces).
pub fn direct_path(x: NodeIndex, n: u32) -> Vec<NodeIndex> {
    let r = root(n);
    let mut path = Vec::new();
    let mut current = x;
    while current != r {
        current = parent(current, n);
        path.push(current);
    }
    path
}

/// The sibling of `x` and of each node on `x`'s direct path (except the
/// root, which has none) — the set of subtrees a path update must be
/// encrypted *to*: every member not on the path sits under exactly one
/// copath node, which is what makes an update cost O(log N) ciphertexts
/// instead of O(N).
pub fn copath(x: NodeIndex, n: u32) -> Vec<NodeIndex> {
    if x == root(n) {
        return Vec::new();
    }
    let mut nodes = vec![x];
    let mut path = direct_path(x, n);
    path.pop(); // the root has no sibling
    nodes.extend(path);
    nodes.into_iter().map(|node| sibling(node, n)).collect()
}

/// The lowest node that is an ancestor of both `a` and `b` — where two
/// members' paths to the root first meet, and therefore the node whose
/// secret is the freshest one both already share.
pub fn common_ancestor(a: NodeIndex, b: NodeIndex, n: u32) -> NodeIndex {
    let mut ancestors_of_a = vec![a];
    ancestors_of_a.extend(direct_path(a, n));
    let mut b_line = vec![b];
    b_line.extend(direct_path(b, n));
    for node in b_line {
        if ancestors_of_a.contains(&node) {
            return node;
        }
    }
    unreachable!("every pair of nodes shares at least the root")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hand-computed vectors for n = 5 leaves (width 9, nodes 0..=8):
    //
    //             7
    //           /   \
    //          3     \
    //        /   \    \
    //       1     5    \
    //      / \   / \    \
    //     0   2 4   6    8
    #[test]
    fn the_five_leaf_tree_matches_hand_computed_structure() {
        let n = 5;
        assert_eq!(node_width(n), 9);
        assert_eq!(root(n), 7);
        assert_eq!(left(7), 3);
        assert_eq!(right(7, n), 8);
        assert_eq!(left(3), 1);
        assert_eq!(right(3, n), 5);
        assert_eq!(parent(8, n), 7);
        assert_eq!(parent(0, n), 1);
        assert_eq!(parent(5, n), 3);
        assert_eq!(sibling(0, n), 2);
        assert_eq!(sibling(8, n), 3);
        assert_eq!(direct_path(0, n), vec![1, 3, 7]);
        assert_eq!(copath(0, n), vec![2, 5, 8]);
        assert_eq!(direct_path(8, n), vec![7]);
        assert_eq!(copath(8, n), vec![3]);
        assert_eq!(common_ancestor(0, 4, n), 3);
        assert_eq!(common_ancestor(0, 8, n), 7);
    }

    #[test]
    fn a_single_member_group_is_just_its_own_root() {
        assert_eq!(node_width(1), 1);
        assert_eq!(root(1), 0);
        assert!(direct_path(0, 1).is_empty());
        assert!(copath(0, 1).is_empty());
    }

    #[test]
    fn levels_count_trailing_ones() {
        assert_eq!(level(0), 0);
        assert_eq!(level(1), 1);
        assert_eq!(level(3), 2);
        assert_eq!(level(7), 3);
        assert_eq!(level(5), 1);
        assert_eq!(level(8), 0);
    }

    /// Every structural invariant, exhaustively, for every group size a
    /// real chat plausibly reaches — this is what stands in for interop
    /// test vectors, since these properties uniquely pin the structure.
    #[test]
    fn structural_invariants_hold_for_every_tree_up_to_256_leaves() {
        for n in 2u32..=256 {
            let width = node_width(n);
            let r = root(n);
            assert!(r < width);

            for x in 0..width {
                // Parent/child are inverses.
                if x != r {
                    let p = parent(x, n);
                    assert!(p < width, "parent inside the tree (n={n}, x={x})");
                    assert!(level(p) > level(x));
                    assert!(
                        left(p) == x || right(p, n) == x,
                        "x is one of its parent's children (n={n}, x={x}, p={p})"
                    );
                    // Sibling is the parent's other child, and mutual.
                    let s = sibling(x, n);
                    assert_ne!(s, x);
                    assert_eq!(parent(s, n), p);
                    assert_eq!(sibling(s, n), x);
                }

                // Direct path walks strictly upward to the root; the
                // copath is exactly the siblings alongside it.
                let path = direct_path(x, n);
                if x == r {
                    assert!(path.is_empty());
                } else {
                    assert_eq!(*path.last().unwrap(), r);
                    let cop = copath(x, n);
                    assert_eq!(cop.len(), path.len());
                    for (i, &c) in cop.iter().enumerate() {
                        let on_path = if i == 0 { x } else { path[i - 1] };
                        assert_eq!(sibling(on_path, n), c);
                    }
                }
            }

            // Every leaf is reachable from the root, and the union of the
            // subtrees under any leaf's copath plus the leaf itself covers
            // all n leaves exactly once — the property path encryption
            // relies on to reach every other member exactly once.
            let leaves_under = |top: NodeIndex| -> Vec<LeafIndex> {
                let mut stack = vec![top];
                let mut out = Vec::new();
                while let Some(node) = stack.pop() {
                    if let Some(leaf) = node_to_leaf(node) {
                        out.push(leaf);
                    } else {
                        stack.push(left(node));
                        stack.push(right(node, n));
                    }
                }
                out
            };
            assert_eq!({ let mut l = leaves_under(r); l.sort(); l }, (0..n).collect::<Vec<_>>());

            for leaf in 0..n {
                let x = leaf_to_node(leaf);
                let mut covered: Vec<LeafIndex> = vec![leaf];
                for c in copath(x, n) {
                    covered.extend(leaves_under(c));
                }
                covered.sort();
                assert_eq!(covered, (0..n).collect::<Vec<_>>(), "copath covers every leaf exactly once (n={n}, leaf={leaf})");
            }
        }
    }

    #[test]
    fn common_ancestor_is_symmetric_and_on_both_paths() {
        for n in 2u32..=64 {
            for a in 0..n {
                for b in 0..n {
                    let (xa, xb) = (leaf_to_node(a), leaf_to_node(b));
                    let anc = common_ancestor(xa, xb, n);
                    assert_eq!(anc, common_ancestor(xb, xa, n));
                    if a == b {
                        assert_eq!(anc, xa);
                    } else {
                        let mut line_a = vec![xa];
                        line_a.extend(direct_path(xa, n));
                        let mut line_b = vec![xb];
                        line_b.extend(direct_path(xb, n));
                        assert!(line_a.contains(&anc) && line_b.contains(&anc));
                    }
                }
            }
        }
    }
}
