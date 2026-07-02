//! End-to-end proof that the `@username` ledger actually propagates
//! between real `P2pNode`s — gossip for a block mined while both peers
//! are connected, `Command::RequestChainSync` for a peer that missed
//! blocks mined before it connected at all.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha20Rng;
use spiritchat_crypto_core::identity::IdentityKeyPair;
use spiritchat_ledger_core::difficulty::{expand_target, INITIAL_DIFFICULTY_BITS};
use spiritchat_ledger_core::{Block, BlockHeader, Transaction};
use spiritchat_p2p_core::{Command, P2pEvent, P2pNode};

/// How long any single "wait for a network event" loop below is allowed
/// to run before the test fails outright — without this, a real bug that
/// makes an expected event never arrive hangs the test process forever
/// instead of failing loudly (exactly what happened while writing this
/// file: a stale genesis timestamp made every submitted block look
/// implausibly far in the future, and the wait loops silently ignored the
/// resulting `LedgerSubmissionRejected` event since they only pattern-
/// matched the event they expected).
const WAIT_TIMEOUT: Duration = Duration::from_secs(30);

fn identity(seed: u64) -> IdentityKeyPair {
    IdentityKeyPair::generate(&mut ChaCha20Rng::seed_from_u64(seed))
}

/// `node.rs` checks a submitted/gossiped block's timestamp against the
/// *real* wall clock (correctly, for production use) — unlike
/// `ledger-core`'s own unit tests, which pass an explicit fictional `now`
/// alongside a fictional block timestamp and never touch the system
/// clock, a block mined here needs a genuinely current timestamp or it's
/// correctly rejected as implausibly far in the future.
fn real_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

/// Mines the very first block after genesis (height 1) containing
/// `transactions` — every test here starts from a fresh node, so genesis
/// plus its known, hardcoded `INITIAL_DIFFICULTY_BITS` is all that's
/// needed to build a valid next block without querying anything.
fn mine_block_one(transactions: Vec<Transaction>, timestamp: u64) -> Block {
    let genesis = Block::genesis();
    let target = expand_target(INITIAL_DIFFICULTY_BITS);
    let mut header = BlockHeader {
        version: 1,
        height: 1,
        prev_hash: genesis.hash(),
        timestamp,
        tx_commitment: Block::compute_tx_commitment(&transactions),
        difficulty_target: INITIAL_DIFFICULTY_BITS,
        nonce: 0,
        miner_public_key: [0u8; 32],
    };
    loop {
        if header.hash().meets_target(&target) {
            break;
        }
        header.nonce += 1;
    }
    Block { header, transactions }
}

/// Mines a second block extending an already-mined height-1 block.
fn mine_block_two(parent: &Block, transactions: Vec<Transaction>, timestamp: u64) -> Block {
    let target = expand_target(parent.header.difficulty_target);
    let mut header = BlockHeader {
        version: 1,
        height: 2,
        prev_hash: parent.hash(),
        timestamp,
        tx_commitment: Block::compute_tx_commitment(&transactions),
        difficulty_target: parent.header.difficulty_target,
        nonce: 0,
        miner_public_key: [0u8; 32],
    };
    loop {
        if header.hash().meets_target(&target) {
            break;
        }
        header.nonce += 1;
    }
    Block { header, transactions }
}

#[tokio::test]
async fn a_mined_block_propagates_over_gossip_to_a_connected_peer() {
    let alice_ledger_dir = tempfile::tempdir().unwrap();
    let bob_ledger_dir = tempfile::tempdir().unwrap();
    let mut alice =
        P2pNode::spawn_with_bootstrap([21u8; 32], vec![], alice_ledger_dir.path().join("ledger.redb")).unwrap();
    let mut bob = P2pNode::spawn_with_bootstrap([22u8; 32], vec![], bob_ledger_dir.path().join("ledger.redb")).unwrap();
    let alice_peer_id = alice.local_peer_id();

    let alice_addr = tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            match alice.next_event().await.expect("alice's event loop is alive") {
                P2pEvent::ListeningOn(addr) if !addr.to_string().contains("0.0.0.0") => return addr,
                _ => continue,
            }
        }
    })
    .await
    .expect("alice never reported a concrete listen address");

    bob.command(Command::Dial { peer: alice_peer_id, known_addresses: vec![alice_addr] }).unwrap();

    // Wait for a real connection (and let gossipsub's mesh form — its
    // heartbeat grafts newly-overlapping-subscription peers within about
    // a second of both sides subscribing, which happens automatically at
    // node construction) before alice mines and submits anything, so this
    // test exercises real propagation rather than racing mesh formation.
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
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let claimant = identity(1);
    let claim = Transaction::new_claim(&claimant, "alice", 1, Block::genesis().hash(), [1; 8]).unwrap();
    let block = mine_block_one(vec![claim], real_now());
    alice.command(Command::SubmitMinedBlock { block }).unwrap();

    // Alice sees her own submission land locally...
    let mut alice_saw_tip = false;
    // ...and bob must independently learn about it purely through gossip,
    // never having been told directly.
    let mut bob_saw_tip = false;
    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(event) = alice.next_event() => match event {
                    P2pEvent::ChainTipChanged { height, .. } => {
                        assert_eq!(height, 1);
                        alice_saw_tip = true;
                    }
                    P2pEvent::LedgerSubmissionRejected { reason } => {
                        panic!("alice's own block was rejected: {reason}");
                    }
                    _ => {}
                },
                Some(event) = bob.next_event() => match event {
                    P2pEvent::ChainTipChanged { height, .. } => {
                        assert_eq!(height, 1);
                        bob_saw_tip = true;
                    }
                    P2pEvent::LedgerSubmissionRejected { reason } => {
                        panic!("bob rejected the block gossiped from alice: {reason}");
                    }
                    _ => {}
                },
            }
            if alice_saw_tip && bob_saw_tip {
                return;
            }
        }
    })
    .await
    .expect("bob never learned about alice's block via gossip");

    bob.command(Command::QueryUsernameOwner { username: "alice".to_string() }).unwrap();
    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            match bob.next_event().await.expect("bob's event loop is alive") {
                P2pEvent::UsernameOwnerResolved { username, owner_public_key, .. } => {
                    assert_eq!(username, "alice");
                    assert_eq!(owner_public_key, claimant.public_key().to_bytes().to_vec());
                    return;
                }
                P2pEvent::UsernameOwnerNotFound { .. } => {
                    panic!("bob should have learned about alice's claim via gossip before this query")
                }
                _ => continue,
            }
        }
    })
    .await
    .expect("bob never answered QueryUsernameOwner");
}

#[tokio::test]
async fn a_peer_that_missed_blocks_catches_up_via_request_chain_sync() {
    let alice_ledger_dir = tempfile::tempdir().unwrap();
    let bob_ledger_dir = tempfile::tempdir().unwrap();
    let mut alice =
        P2pNode::spawn_with_bootstrap([23u8; 32], vec![], alice_ledger_dir.path().join("ledger.redb")).unwrap();
    let mut bob = P2pNode::spawn_with_bootstrap([24u8; 32], vec![], bob_ledger_dir.path().join("ledger.redb")).unwrap();
    let alice_peer_id = alice.local_peer_id();

    // Alice mines two blocks entirely on her own, before bob ever
    // connects — bob genuinely has no way to have heard about these via
    // gossip, only via an explicit sync.
    let claimant = identity(2);
    let claim_one = Transaction::new_claim(&claimant, "alice", 1, Block::genesis().hash(), [1; 8]).unwrap();
    let block_one = mine_block_one(vec![claim_one], real_now());
    let block_one_hash = block_one.hash();
    alice.command(Command::SubmitMinedBlock { block: block_one.clone() }).unwrap();
    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            match alice.next_event().await.unwrap() {
                P2pEvent::ChainTipChanged { height: 1, .. } => return,
                P2pEvent::LedgerSubmissionRejected { reason } => panic!("block one rejected: {reason}"),
                _ => continue,
            }
        }
    })
    .await
    .expect("alice never applied her own first block");

    let claim_two = Transaction::new_claim(&claimant, "bobby", 2, block_one_hash, [2; 8]).unwrap();
    // Must be strictly after block_one's own timestamp for the median-
    // time-past check — `real_now()` again could easily land in the same
    // wall-clock second as block_one's, which the strict `>` check
    // rejects (this raced and failed intermittently before this fix).
    let block_two = mine_block_two(&block_one, vec![claim_two], block_one.header.timestamp + 1);
    alice.command(Command::SubmitMinedBlock { block: block_two }).unwrap();
    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            match alice.next_event().await.unwrap() {
                P2pEvent::ChainTipChanged { height: 2, .. } => return,
                P2pEvent::LedgerSubmissionRejected { reason } => panic!("block two rejected: {reason}"),
                _ => continue,
            }
        }
    })
    .await
    .expect("alice never applied her own second block");

    // Only now does bob connect and ask to be caught up.
    let alice_addr = tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            match alice.next_event().await.expect("alice's event loop is alive") {
                P2pEvent::ListeningOn(addr) if !addr.to_string().contains("0.0.0.0") => return addr,
                _ => continue,
            }
        }
    })
    .await
    .expect("alice never reported a concrete listen address");
    bob.command(Command::Dial { peer: alice_peer_id, known_addresses: vec![alice_addr] }).unwrap();

    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(_) = alice.next_event() => {}
                Some(event) = bob.next_event() => {
                    if matches!(event, P2pEvent::PeerConnected(p) if p == alice_peer_id) {
                        return;
                    }
                }
            }
        }
    })
    .await
    .expect("bob never connected to alice");

    bob.command(Command::RequestChainSync { peer: alice_peer_id }).unwrap();

    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(_) = alice.next_event() => {}
                Some(event) = bob.next_event() => {
                    match event {
                        P2pEvent::ChainSyncCompleted { height } => {
                            assert_eq!(height, 2);
                            return;
                        }
                        P2pEvent::ChainSyncFailed { reason, .. } => {
                            panic!("chain sync failed: {reason}");
                        }
                        _ => {}
                    }
                }
            }
        }
    })
    .await
    .expect("bob's chain sync never completed");

    bob.command(Command::QueryUsernameOwner { username: "bobby".to_string() }).unwrap();
    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            match bob.next_event().await.expect("bob's event loop is alive") {
                P2pEvent::UsernameOwnerResolved { username, owner_public_key, claimed_at_height } => {
                    assert_eq!(username, "bobby");
                    assert_eq!(owner_public_key, claimant.public_key().to_bytes().to_vec());
                    assert_eq!(claimed_at_height, 2);
                    return;
                }
                P2pEvent::UsernameOwnerNotFound { .. } => panic!("bob should have caught up via sync"),
                _ => continue,
            }
        }
    })
    .await
    .expect("bob never answered QueryUsernameOwner after sync");
}
