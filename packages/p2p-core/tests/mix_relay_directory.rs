//! End-to-end proof that `Command::AnnounceMixRelay` actually reaches a
//! connected peer over gossip and is recorded as
//! `P2pEvent::MixRelayDiscovered` — the mechanism a node uses to learn
//! another peer's Sphinx routing public key without ever having exchanged
//! mix traffic with them directly. `node.rs`'s own unit tests already
//! cover the pure parsing/bookkeeping logic in isolation; this proves the
//! actual wiring (topic subscription, gossip publish, event dispatch)
//! works over a real dialed connection.

use std::time::Duration;

use spiritchat_p2p_core::{Command, P2pEvent, P2pNode};

const WAIT_TIMEOUT: Duration = Duration::from_secs(15);

#[tokio::test]
async fn bob_discovers_alices_mix_relay_announcement_over_gossip() {
    let alice_dir = tempfile::tempdir().unwrap();
    let bob_dir = tempfile::tempdir().unwrap();
    let mut alice = P2pNode::spawn_with_bootstrap([31u8; 32], vec![], alice_dir.path().join("ledger.redb")).unwrap();
    let mut bob = P2pNode::spawn_with_bootstrap([32u8; 32], vec![], bob_dir.path().join("ledger.redb")).unwrap();
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

    // Same reasoning as the ledger's own gossip test: let the connection
    // land and gossipsub's mesh actually form before publishing, so this
    // exercises real propagation rather than racing mesh formation.
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

    alice.command(Command::AnnounceMixRelay).unwrap();

    let discovered_peer = tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(event) = bob.next_event() => {
                    if let P2pEvent::MixRelayDiscovered { peer } = event {
                        return peer;
                    }
                }
                Some(_) = alice.next_event() => {}
            }
        }
    })
    .await
    .expect("bob never discovered alice's mix relay announcement");

    assert_eq!(discovered_peer, alice_peer_id);
}
