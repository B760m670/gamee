//! End-to-end proof that `Command::StartMining`/`StopMining` drive a real
//! `spawn_blocking` nonce search that produces valid blocks, picks up
//! mempool claims from a connected peer, stops when asked, and — the
//! scenario the whole ledger exists for — resolves a genuine double-claim
//! race between two independently mining nodes to the same winner on both
//! sides.

use std::time::Duration;

use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha20Rng;
use spiritchat_crypto_core::identity::IdentityKeyPair;
use spiritchat_ledger_core::{Block, Transaction};
use spiritchat_p2p_core::{Command, Multiaddr, P2pEvent, P2pNode, PeerId};

/// See `tests/ledger_sync.rs`'s doc comment on the same constant — this
/// keeps a genuine bug (an expected event that never arrives) a fast test
/// failure instead of a silent hang.
const WAIT_TIMEOUT: Duration = Duration::from_secs(30);

fn identity(seed: u64) -> IdentityKeyPair {
    IdentityKeyPair::generate(&mut ChaCha20Rng::seed_from_u64(seed))
}

async fn wait_for_concrete_listen_addr(node: &mut P2pNode) -> Multiaddr {
    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            match node.next_event().await.expect("node's event loop is alive") {
                P2pEvent::ListeningOn(addr) if !addr.to_string().contains("0.0.0.0") => return addr,
                _ => continue,
            }
        }
    })
    .await
    .expect("node never reported a concrete listen address")
}

/// Dials `dialer` to `target` and waits until `dialer` sees the connection
/// — draining `target`'s events in the background so its own channel
/// doesn't fill up while we wait. Used instead of the two-way `select!`
/// dance in `ledger_sync.rs` where only one side's confirmation matters.
async fn connect(dialer: &mut P2pNode, dialer_addr_of_target: Multiaddr, target_peer: PeerId) {
    dialer.command(Command::Dial { peer: target_peer, known_addresses: vec![dialer_addr_of_target] }).unwrap();
    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            if let P2pEvent::PeerConnected(peer) = dialer.next_event().await.expect("dialer's event loop is alive") {
                if peer == target_peer {
                    return;
                }
            }
        }
    })
    .await
    .expect("dialer never connected to target");
}

#[tokio::test]
async fn starting_mining_produces_a_real_block_that_extends_the_tip() {
    let dir = tempfile::tempdir().unwrap();
    let mut node = P2pNode::spawn_with_bootstrap([41u8; 32], vec![], dir.path().join("ledger.redb")).unwrap();

    node.command(Command::StartMining { public_key: [7u8; 32] }).unwrap();

    let height = tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            match node.next_event().await.expect("node's event loop is alive") {
                P2pEvent::NewBlockMined { height } => return height,
                P2pEvent::LedgerSubmissionRejected { reason } => panic!("mined block was rejected: {reason}"),
                _ => continue,
            }
        }
    })
    .await
    .expect("mining never produced a block");

    assert!(height >= 1);
    node.command(Command::StopMining).unwrap();
}

#[tokio::test]
async fn stop_mining_halts_further_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let mut node = P2pNode::spawn_with_bootstrap([42u8; 32], vec![], dir.path().join("ledger.redb")).unwrap();

    node.command(Command::StartMining { public_key: [7u8; 32] }).unwrap();
    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            if let P2pEvent::NewBlockMined { .. } = node.next_event().await.expect("node's event loop is alive") {
                return;
            }
        }
    })
    .await
    .expect("mining never produced a first block");

    node.command(Command::StopMining).unwrap();

    // A result already mid-flight when StopMining was issued may still
    // land — but `Mining::stop` clears the active generation immediately,
    // so `run_event_loop`'s generation check discards it rather than
    // applying it. Give any such straggler time to arrive and be dropped,
    // then assert a real quiet period follows.
    tokio::time::sleep(Duration::from_millis(300)).await;
    while (tokio::time::timeout(Duration::from_millis(20), node.next_event()).await).is_ok() {
        // draining whatever was already queued
    }

    let more = tokio::time::timeout(Duration::from_millis(1500), async {
        loop {
            if let P2pEvent::NewBlockMined { .. } = node.next_event().await.expect("node's event loop is alive") {
                return;
            }
        }
    })
    .await;
    assert!(more.is_err(), "mining kept producing blocks after StopMining");
}

#[tokio::test]
async fn a_submitted_claim_gets_mined_into_a_block_by_a_connected_peer() {
    let alice_dir = tempfile::tempdir().unwrap();
    let bob_dir = tempfile::tempdir().unwrap();
    let mut alice =
        P2pNode::spawn_with_bootstrap([43u8; 32], vec![], alice_dir.path().join("ledger.redb")).unwrap();
    let mut bob = P2pNode::spawn_with_bootstrap([44u8; 32], vec![], bob_dir.path().join("ledger.redb")).unwrap();
    let alice_peer_id = alice.local_peer_id();

    let alice_addr = wait_for_concrete_listen_addr(&mut alice).await;
    bob.command(Command::Dial { peer: alice_peer_id, known_addresses: vec![alice_addr] }).unwrap();
    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(event) = alice.next_event() => {
                    if matches!(event, P2pEvent::PeerConnected(p) if p == bob.local_peer_id()) {
                        return;
                    }
                }
                Some(_) = bob.next_event() => {}
            }
        }
    })
    .await
    .expect("alice and bob never connected");
    // Let gossipsub's mesh form before submitting anything.
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let claimant = identity(5);
    let claim = Transaction::new_claim(&claimant, "carol", 1, Block::genesis().hash(), [9; 8]).unwrap();
    alice.command(Command::SubmitUsernameClaim { transaction: claim }).unwrap();
    // Let the claim reach bob's mempool over gossip before bob starts
    // mining, so the very first candidate bob assembles already includes
    // it (not strictly required for correctness — a later restart would
    // pick it up too — but keeps this test's expected height at 1).
    tokio::time::sleep(Duration::from_millis(500)).await;

    bob.command(Command::StartMining { public_key: [8u8; 32] }).unwrap();

    let mut alice_confirmed = false;
    let mut bob_confirmed = false;
    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(event) = alice.next_event() => match event {
                    P2pEvent::ChainTipChanged { .. } => {
                        alice.command(Command::QueryUsernameOwner { username: "carol".to_string() }).unwrap();
                    }
                    P2pEvent::UsernameOwnerResolved { username, owner_public_key, .. } => {
                        assert_eq!(username, "carol");
                        assert_eq!(owner_public_key, claimant.public_key().to_bytes().to_vec());
                        alice_confirmed = true;
                    }
                    P2pEvent::LedgerSubmissionRejected { reason } => {
                        panic!("alice rejected bob's mined block: {reason}");
                    }
                    _ => {}
                },
                Some(event) = bob.next_event() => match event {
                    P2pEvent::NewBlockMined { .. } => {
                        bob.command(Command::QueryUsernameOwner { username: "carol".to_string() }).unwrap();
                    }
                    P2pEvent::UsernameOwnerResolved { username, owner_public_key, .. } => {
                        assert_eq!(username, "carol");
                        assert_eq!(owner_public_key, claimant.public_key().to_bytes().to_vec());
                        bob_confirmed = true;
                    }
                    P2pEvent::LedgerSubmissionRejected { reason } => {
                        panic!("bob's own mined block was rejected: {reason}");
                    }
                    _ => {}
                },
            }
            if alice_confirmed && bob_confirmed {
                return;
            }
        }
    })
    .await
    .expect("carol's claim was never mined and resolved by both peers");

    bob.command(Command::StopMining).unwrap();
}

/// The scenario the whole ledger exists for: two peers each try to claim
/// the same username at roughly the same time. Both mine continuously and
/// race live over gossip — whichever block a node's own chain accepts
/// first may later be reorged out by a heavier competing chain, exactly
/// like `spiritchat_ledger_core`'s own `a_heavier_fork_reorgs_the_canonical_tip`
/// unit test, except here it happens for real, over the wire, between two
/// live mining nodes. The only thing this test can assert deterministically
/// is the property that actually matters: both nodes converge on *the same*
/// winner, not which of the two claimants wins.
#[tokio::test]
async fn a_double_claim_race_between_two_live_miners_converges_to_the_same_winner_on_both_sides() {
    let alice_dir = tempfile::tempdir().unwrap();
    let bob_dir = tempfile::tempdir().unwrap();
    let mut alice =
        P2pNode::spawn_with_bootstrap([45u8; 32], vec![], alice_dir.path().join("ledger.redb")).unwrap();
    let mut bob = P2pNode::spawn_with_bootstrap([46u8; 32], vec![], bob_dir.path().join("ledger.redb")).unwrap();
    let alice_peer_id = alice.local_peer_id();
    let bob_peer_id = bob.local_peer_id();

    let alice_addr = wait_for_concrete_listen_addr(&mut alice).await;
    connect(&mut bob, alice_addr, alice_peer_id).await;
    // Drain alice's own view of the connection (connect() only watches the
    // dialer's side) and let gossipsub's mesh form.
    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            if let P2pEvent::PeerConnected(peer) = alice.next_event().await.expect("alice's event loop is alive") {
                if peer == bob_peer_id {
                    return;
                }
            }
        }
    })
    .await
    .expect("alice never saw bob connect");
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let claimant_alice = identity(6);
    let claimant_bob = identity(7);
    let genesis_hash = Block::genesis().hash();
    let claim_alice = Transaction::new_claim(&claimant_alice, "prize", 1, genesis_hash, [1; 8]).unwrap();
    let claim_bob = Transaction::new_claim(&claimant_bob, "prize", 1, genesis_hash, [2; 8]).unwrap();

    // Submitted at (almost) the same moment, from opposite ends, so
    // whichever side's miner happens to solve a block first has no
    // structural head start — both mempools end up holding both
    // competing claims, and each side's candidate-assembly picks
    // deterministically (tx-id-ascending) between them, so a solo miner
    // never even builds a block containing both.
    alice.command(Command::SubmitUsernameClaim { transaction: claim_alice }).unwrap();
    bob.command(Command::SubmitUsernameClaim { transaction: claim_bob }).unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    alice.command(Command::StartMining { public_key: [10u8; 32] }).unwrap();
    bob.command(Command::StartMining { public_key: [11u8; 32] }).unwrap();

    let mut alice_owner: Option<Vec<u8>> = None;
    let mut bob_owner: Option<Vec<u8>> = None;
    // Keep polling both sides' view of "prize" until they've each settled
    // (i.e. stayed the same across two consecutive query results) and
    // agree with each other — a single snapshot right after the first
    // block lands could still be reorged out from under it a moment later.
    let mut alice_stable_count = 0u32;
    let mut bob_stable_count = 0u32;
    const REQUIRED_STABLE: u32 = 3;

    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(event) = alice.next_event() => match event {
                    P2pEvent::ChainTipChanged { .. } => {
                        alice.command(Command::QueryUsernameOwner { username: "prize".to_string() }).unwrap();
                    }
                    P2pEvent::UsernameOwnerResolved { owner_public_key, .. } => {
                        if alice_owner.as_ref() == Some(&owner_public_key) {
                            alice_stable_count += 1;
                        } else {
                            alice_stable_count = 0;
                        }
                        alice_owner = Some(owner_public_key);
                    }
                    P2pEvent::UsernameOwnerNotFound { .. } => {
                        alice_owner = None;
                        alice_stable_count = 0;
                    }
                    _ => {}
                },
                Some(event) = bob.next_event() => match event {
                    P2pEvent::ChainTipChanged { .. } => {
                        bob.command(Command::QueryUsernameOwner { username: "prize".to_string() }).unwrap();
                    }
                    P2pEvent::UsernameOwnerResolved { owner_public_key, .. } => {
                        if bob_owner.as_ref() == Some(&owner_public_key) {
                            bob_stable_count += 1;
                        } else {
                            bob_stable_count = 0;
                        }
                        bob_owner = Some(owner_public_key);
                    }
                    P2pEvent::UsernameOwnerNotFound { .. } => {
                        bob_owner = None;
                        bob_stable_count = 0;
                    }
                    _ => {}
                },
            }
            if alice_stable_count >= REQUIRED_STABLE
                && bob_stable_count >= REQUIRED_STABLE
                && alice_owner.is_some()
            {
                return;
            }
        }
    })
    .await
    .expect("alice and bob never converged on a single stable owner for 'prize'");

    alice.command(Command::StopMining).unwrap();
    bob.command(Command::StopMining).unwrap();

    assert!(
        alice_owner == Some(claimant_alice.public_key().to_bytes().to_vec())
            || alice_owner == Some(claimant_bob.public_key().to_bytes().to_vec()),
        "the winner must be one of the two real claimants"
    );
    assert_eq!(alice_owner, bob_owner, "both nodes must converge on the same winner");
}
