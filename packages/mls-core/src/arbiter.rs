//! Deterministic peer-to-peer commit arbitration — the piece RFC 9420
//! leaves to a "Delivery Service" and this project, having no server,
//! must supply itself. MLS requires that at most one commit advance any
//! given epoch: if two members commit concurrently against epoch N, every
//! member must independently agree on which one becomes epoch N+1, or the
//! group forks.
//!
//! The rule is the same shape the rest of this project already uses to
//! break ties without a coordinator (the ledger's heaviest-chain-then-
//! lowest-hash, the mixnet's closest-relay-by-XOR): **the commit with the
//! numerically smallest `commit_id` wins its epoch.** `commit_id` is a
//! hash of fully public commit content, so every member computes the same
//! ordering over the same set of observed commits and converges without
//! ever exchanging a vote. A member whose own commit loses hasn't applied
//! it (that's why `build_commit` doesn't mutate) — they simply adopt the
//! winner and, if their intent still matters, re-commit at the new epoch.
//!
//! This resolves *contention*, not *delivery*: it assumes members
//! eventually see the same set of commits for an epoch (the transport's
//! job — gossip, mailbox). Two members who resolved on different observed
//! sets would diverge, but the transcript's confirmation tag (phase 4)
//! makes that divergence immediately detectable rather than silent, and
//! the losing side re-syncs. Buffering commits until an epoch is
//! considered settled is a transport-layer policy (phase 6), not this
//! module's concern; here we resolve one already-collected batch.

use crate::group::{commit_id, Commit, CommitOutput, GroupError, GroupState};

/// A commit this member built locally (via `GroupState::build_commit`),
/// held un-applied until arbitration decides whether it won its epoch.
pub struct OwnCommit {
    pub output: CommitOutput,
    /// The post-commit state to adopt if this commit wins.
    pub next: GroupState,
}

/// What arbitration decided.
pub enum Resolution {
    /// Our own commit had the smallest id and won — we keep its staged
    /// state. `losers` are the ids of concurrent commits that must be
    /// dropped (and, by their committers, re-based).
    OwnWon { winner_id: [u8; 32] },
    /// Another member's commit won; we advanced by processing it. If we
    /// also had a commit in this batch, its id is included in `losers` —
    /// our intent may need re-committing at the new epoch.
    AdoptedOther { winner_id: [u8; 32] },
}

pub struct ResolveOutcome {
    pub state: GroupState,
    pub resolution: Resolution,
    /// Commit ids that lost this epoch — informational, so a caller can
    /// tell whether its own proposal still needs redoing.
    pub losers: Vec<[u8; 32]>,
}

/// Resolves one epoch's contention. `current` is this member's state at
/// the contended epoch; `own` is their own un-applied commit for it (if
/// any); `others` are every other member's commit observed for the same
/// epoch. Deterministic: any member calling this with the same `others`
/// (and their own, if any) reaches the same winning epoch.
pub fn resolve(
    current: &GroupState,
    own: Option<OwnCommit>,
    others: &[Commit],
) -> Result<ResolveOutcome, GroupError> {
    let group_id = current.group_id_bytes();
    let epoch = current.epoch();

    // Everyone contends only for the epoch we're actually sitting at; a
    // commit built against a different epoch isn't in this race.
    for c in others {
        if c.from_epoch != epoch {
            return Err(GroupError::WrongEpoch);
        }
    }
    if let Some(o) = &own {
        if o.output.commit.from_epoch != epoch {
            return Err(GroupError::WrongEpoch);
        }
    }

    // Collect (id, source) for every candidate and find the smallest id.
    let own_id = own.as_ref().map(|o| commit_id(group_id, &o.output.commit));
    let mut best: Option<[u8; 32]> = own_id;
    for c in others {
        let id = commit_id(group_id, c);
        if best.is_none_or(|b| id < b) {
            best = Some(id);
        }
    }
    let winner_id = best.ok_or(GroupError::NoCommitToResolve)?;

    let mut losers: Vec<[u8; 32]> = Vec::new();
    if let Some(oid) = own_id {
        if oid != winner_id {
            losers.push(oid);
        }
    }
    for c in others {
        let id = commit_id(group_id, c);
        if id != winner_id {
            losers.push(id);
        }
    }

    if Some(winner_id) == own_id {
        let own = own.expect("own_id is Some only when own is Some");
        Ok(ResolveOutcome { state: own.next, resolution: Resolution::OwnWon { winner_id }, losers })
    } else {
        let winner = others
            .iter()
            .find(|c| commit_id(group_id, c) == winner_id)
            .expect("winner_id came from own or others; not-own means it's in others");
        let mut state = current.clone();
        state.process_commit(winner)?;
        Ok(ResolveOutcome { state, resolution: Resolution::AdoptedOther { winner_id }, losers })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::Proposal;
    use crate::member::KeyPackage;
    use ed25519_dalek::SigningKey;
    use rand_chacha::rand_core::SeedableRng;
    use rand_chacha::ChaCha20Rng;
    use x25519_dalek::{PublicKey, StaticSecret};

    fn rng(seed: u64) -> ChaCha20Rng {
        ChaCha20Rng::seed_from_u64(seed)
    }

    fn sk(seed: u64) -> SigningKey {
        SigningKey::generate(&mut rng(seed))
    }

    fn candidate(seed: u64) -> (SigningKey, StaticSecret, KeyPackage) {
        let s = sk(seed);
        let ls = StaticSecret::random_from_rng(rng(seed + 500));
        let kp = KeyPackage::create(&s, PublicKey::from(&ls));
        (s, ls, kp)
    }

    /// A 3-member group where all three sit at the same epoch, ready to
    /// commit concurrently.
    fn three_member_group() -> (GroupState, GroupState, GroupState) {
        let mut alice = GroupState::create(b"g".to_vec(), sk(1), &mut rng(10));
        let (bsk, bls, bkp) = candidate(2);
        let out = alice.commit(vec![Proposal::Add(bkp)], &mut rng(11)).unwrap();
        let mut bob = GroupState::join(&out.welcomes[0].1, bsk, bls).unwrap();

        let (csk, cls, ckp) = candidate(3);
        let out = alice.commit(vec![Proposal::Add(ckp)], &mut rng(12)).unwrap();
        bob.process_commit(&out.commit).unwrap();
        let carol = GroupState::join(&out.welcomes[0].1, csk, cls).unwrap();
        (alice, bob, carol)
    }

    #[test]
    fn two_concurrent_committers_and_a_bystander_all_converge_on_one_winner() {
        let (alice, bob, carol) = three_member_group();
        let start_epoch = alice.epoch();

        // Alice and Bob both commit a self-update against the same epoch,
        // concurrently — neither has seen the other's.
        let (alice_out, alice_next) = alice.build_commit(vec![], &mut rng(100)).unwrap();
        let (bob_out, bob_next) = bob.build_commit(vec![], &mut rng(200)).unwrap();
        let alice_commit = alice_out.commit;
        let bob_commit = bob_out.commit;

        // Each member resolves over the same observed set.
        let alice_res = resolve(
            &alice,
            Some(OwnCommit { output: CommitOutput { commit: clone_commit(&alice_commit), welcomes: vec![] }, next: alice_next }),
            std::slice::from_ref(&bob_commit),
        )
        .unwrap();
        let bob_res = resolve(
            &bob,
            Some(OwnCommit { output: CommitOutput { commit: clone_commit(&bob_commit), welcomes: vec![] }, next: bob_next }),
            std::slice::from_ref(&alice_commit),
        )
        .unwrap();
        // Carol committed nothing; she just adopts the winner.
        let carol_res = resolve(&carol, None, &[clone_commit(&alice_commit), clone_commit(&bob_commit)]).unwrap();

        // All three landed on the next epoch...
        assert_eq!(alice_res.state.epoch(), start_epoch + 1);
        assert_eq!(bob_res.state.epoch(), start_epoch + 1);
        assert_eq!(carol_res.state.epoch(), start_epoch + 1);
        // ...and the *same* one.
        let auth = alice_res.state.epoch_authenticator();
        assert_eq!(bob_res.state.epoch_authenticator(), auth);
        assert_eq!(carol_res.state.epoch_authenticator(), auth);

        // Exactly one of Alice/Bob sees themselves as the winner.
        let alice_won = matches!(alice_res.resolution, Resolution::OwnWon { .. });
        let bob_won = matches!(bob_res.resolution, Resolution::OwnWon { .. });
        assert!(alice_won ^ bob_won, "exactly one committer wins its epoch");
    }

    #[test]
    fn the_loser_can_rebase_and_still_land_its_change() {
        let (alice, bob, mut carol) = three_member_group();

        // Alice self-updates; Bob concurrently tries to remove Carol.
        let (alice_out, alice_next) = alice.build_commit(vec![], &mut rng(100)).unwrap();
        let (bob_out, bob_next) = bob.build_commit(vec![Proposal::Remove(carol.own_leaf())], &mut rng(200)).unwrap();

        let winner_is_alice = {
            let a = crate::group::commit_id(b"g", &alice_out.commit);
            let b = crate::group::commit_id(b"g", &bob_out.commit);
            a < b
        };

        // Resolve on every side.
        let alice_res = resolve(&alice, Some(OwnCommit { output: CommitOutput { commit: clone_commit(&alice_out.commit), welcomes: vec![] }, next: alice_next }), std::slice::from_ref(&bob_out.commit)).unwrap();
        let mut bob_after = resolve(&bob, Some(OwnCommit { output: CommitOutput { commit: clone_commit(&bob_out.commit), welcomes: vec![] }, next: bob_next }), std::slice::from_ref(&alice_out.commit)).unwrap().state;
        carol.process_commit(if winner_is_alice { &alice_out.commit } else { &bob_out.commit }).unwrap();

        if winner_is_alice {
            // Bob's removal lost. He re-issues it at the new epoch; it
            // now lands, and Carol can no longer follow.
            let re = bob_after.commit(vec![Proposal::Remove(carol.own_leaf())], &mut rng(300)).unwrap();
            // Alice (also at the new epoch) applies Bob's re-based removal.
            let mut alice_after = alice_res.state;
            alice_after.process_commit(&re.commit).unwrap();
            assert_eq!(alice_after.epoch_authenticator(), bob_after.epoch_authenticator());
            assert!(carol.process_commit(&re.commit).is_err());
        } else {
            // Bob's removal won outright; Carol was removed already.
            assert_eq!(bob_after.epoch_authenticator(), alice_res.state.epoch_authenticator());
        }
    }

    #[test]
    fn resolution_is_independent_of_the_order_commits_were_observed() {
        // A bystander must reach the same winner whichever order the two
        // concurrent commits happened to arrive in.
        let (alice, bob, carol) = three_member_group();
        let (alice_out, _) = alice.build_commit(vec![], &mut rng(100)).unwrap();
        let (bob_out, _) = bob.build_commit(vec![], &mut rng(200)).unwrap();

        let forward = resolve(&carol, None, &[clone_commit(&alice_out.commit), clone_commit(&bob_out.commit)]).unwrap();
        let reversed = resolve(&carol, None, &[clone_commit(&bob_out.commit), clone_commit(&alice_out.commit)]).unwrap();
        assert_eq!(forward.state.epoch_authenticator(), reversed.state.epoch_authenticator());
    }

    // Commit isn't Clone (it owns an UpdatePath of sealed secrets); for
    // these tests we only need to feed the same commit to two resolvers,
    // so rebuild a shallow copy field-by-field.
    fn clone_commit(c: &Commit) -> Commit {
        Commit {
            from_epoch: c.from_epoch,
            committer: c.committer,
            proposals: c.proposals.clone(),
            update_path: clone_update_path(&c.update_path),
            confirmation_tag: c.confirmation_tag,
            signature: c.signature,
        }
    }

    fn clone_update_path(u: &crate::update_path::UpdatePath) -> crate::update_path::UpdatePath {
        crate::update_path::UpdatePath {
            updater: u.updater,
            leaf_public: u.leaf_public,
            nodes: u
                .nodes
                .iter()
                .map(|n| crate::update_path::PathNode {
                    node: n.node,
                    public: n.public,
                    sealed: n.sealed.clone(),
                })
                .collect(),
        }
    }
}
