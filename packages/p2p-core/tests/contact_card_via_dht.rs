//! End-to-end proof that a contact card published into the DHT
//! (`Command::AnnounceContactCard`) can be found by another node
//! (`Command::ResolveContactCard`) that has never fetched it directly —
//! the mechanism that makes a *first* message to someone who's offline
//! right now possible at all, since it needs no live connection to the
//! card's owner, only to whichever nodes Kademlia replicated the record to.

use std::time::Duration;

use spiritchat_p2p_core::{Command, P2pEvent, P2pNode};

const WAIT_TIMEOUT: Duration = Duration::from_secs(20);

#[tokio::test]
async fn bob_resolves_alices_contact_card_from_the_dht() {
    let alice_dir = tempfile::tempdir().unwrap();
    let bob_dir = tempfile::tempdir().unwrap();
    let mut alice = P2pNode::spawn_with_bootstrap([91u8; 32], vec![], alice_dir.path().join("ledger.redb")).unwrap();
    let mut bob = P2pNode::spawn_with_bootstrap([92u8; 32], vec![], bob_dir.path().join("ledger.redb")).unwrap();
    let alice_peer_id = alice.local_peer_id();
    let bob_peer_id = bob.local_peer_id();

    let bob_addr = loop {
        match bob.next_event().await.expect("bob's event loop is alive") {
            P2pEvent::ListeningOn(addr) if !addr.to_string().contains("0.0.0.0") => break addr,
            _ => continue,
        }
    };

    alice.command(Command::Dial { peer: bob_peer_id, known_addresses: vec![bob_addr] }).unwrap();

    // Wait until bob has specifically completed identify with alice —
    // that's what puts alice into bob's own Kademlia routing table, which
    // put_record needs someone in to replicate to at all (mirrors the
    // same wait in two_nodes_exchange_an_envelope's own DHT test).
    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(_) = alice.next_event() => {}
                Some(event) = bob.next_event() => {
                    if matches!(event, P2pEvent::PeerIdentified(p) if p == alice_peer_id) {
                        return;
                    }
                }
            }
        }
    })
    .await
    .expect("bob never identified alice");

    let owner_identity_public_key = vec![7u8; 32];
    let card_bytes = b"alice's prekey bundle bytes, opaque to this crate".to_vec();
    alice
        .command(Command::AnnounceContactCard {
            owner_identity_public_key: owner_identity_public_key.clone(),
            card: card_bytes.clone(),
        })
        .unwrap();

    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            match alice.next_event().await.expect("alice's event loop is alive") {
                P2pEvent::ContactCardAnnounced => return,
                P2pEvent::ContactCardAnnouncementFailed { reason } => {
                    panic!("alice failed to announce her contact card: {reason}")
                }
                _ => continue,
            }
        }
    })
    .await
    .expect("alice's contact card announcement never completed");

    // Bob never fetched this directly from Alice (no SetLocalBlob/FetchBlob
    // involved at all) — only the DHT record she just published.
    bob.command(Command::ResolveContactCard { owner_identity_public_key: owner_identity_public_key.clone() }).unwrap();

    let resolved = tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(_) = alice.next_event() => {}
                Some(event) = bob.next_event() => {
                    match event {
                        P2pEvent::ContactCardResolved { owner_identity_public_key: owner, card } if owner == owner_identity_public_key => {
                            return card;
                        }
                        P2pEvent::ContactCardResolutionFailed { owner_identity_public_key: owner } if owner == owner_identity_public_key => {
                            panic!("bob's contact card resolution failed");
                        }
                        _ => {}
                    }
                }
            }
        }
    })
    .await
    .expect("bob never resolved alice's contact card from the DHT");

    assert_eq!(resolved, card_bytes);
}
