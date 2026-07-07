//! TreeKEM-based continuous group key agreement — the group-messaging
//! core that will replace `spiritchat_crypto_core::sender_key`. Follows
//! **RFC 9420 (Messaging Layer Security)**'s constructions — the ratchet
//! tree, its key schedule, proposals/commits — with this project's usual
//! adaptations: one fixed ciphersuite (X25519 / HKDF-SHA256 /
//! ChaCha20-Poly1305 / Ed25519, all already in this workspace), no X.509
//! credential machinery, no cross-vendor interop requirement, and the
//! role RFC 9420 assigns to a "Delivery Service" (totally ordering
//! commits) filled by a deterministic peer-to-peer arbitration rule
//! instead of a server — this network doesn't have one, by design.
//!
//! What TreeKEM buys over Sender Keys, concretely:
//! - **O(log N) membership changes** instead of every remaining member
//!   re-keying with every other member (O(N²) messages) on each removal;
//! - **post-compromise security**: a routine Update commit rotates the
//!   path from a member's leaf to the root, locking a past compromise
//!   back out — Sender Keys only heals via that full O(N²) reset;
//! - **cryptographic membership agreement**: the tree (and the transcript
//!   hash over its history) *is* the membership — two members cannot
//!   silently disagree about who is in the group.
//!
//! Built in phases, each a working, tested unit (the same shape
//! `ledger-core` and the mixnet were built in):
//! 1. `tree_math` — the array representation of a left-balanced binary
//!    tree: indexing, parents/children/siblings, direct paths, copaths.
//! 2. The ratchet tree itself: nodes holding HPKE keypairs, blank/merged
//!    state, path secret derivation to the root.
//! 3. The epoch key schedule + encrypting path updates to copath nodes.
//! 4. Proposals / Commits / Welcome, transcript hashing.
//! 5. Deterministic P2P commit arbitration + transport wiring.
//! 6. FFI/Swift/TS + migration of existing Sender Keys groups.

pub mod hpke;
pub mod key_schedule;
pub mod ratchet_tree;
pub mod secrets;
pub mod tree_math;
pub mod update_path;
