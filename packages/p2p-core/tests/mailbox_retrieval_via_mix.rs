//! End-to-end proof of the full offline-delivery round trip: Alice
//! deposits an envelope for a (fictional) recipient through Bob, then —
//! as if she were that recipient, sharing the same `shared_material` —
//! issues `Command::RetrieveFromMailbox` and gets the envelope back,
//! routed anonymously through a SURB Bob never had to be told anything
//! about beyond "reply to whoever gave you this."

use std::time::Duration;

use spiritchat_p2p_core::{Command, P2pEvent, P2pNode};

const WAIT_TIMEOUT: Duration = Duration::from_secs(15);
const SHARED_MATERIAL: &[u8] = b"alice-and-bobs-recipient-shared-material";

#[tokio::test]
async fn alice_deposits_then_retrieves_her_own_message_back_through_bob() {
    let alice_dir = tempfile::tempdir().unwrap();
    let bob_dir = tempfile::tempdir().unwrap();
    let mut alice = P2pNode::spawn_with_bootstrap([61u8; 32], vec![], alice_dir.path().join("ledger.redb")).unwrap();
    let mut bob = P2pNode::spawn_with_bootstrap([62u8; 32], vec![], bob_dir.path().join("ledger.redb")).unwrap();
    let bob_peer_id = bob.local_peer_id();

    let bob_addr = tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            match bob.next_event().await.expect("bob's event loop is alive") {
                P2pEvent::ListeningOn(addr) if !addr.to_string().contains("0.0.0.0") => return addr,
                _ => continue,
            }
        }
    })
    .await
    .expect("bob never reported a concrete listen address");

    alice.command(Command::Dial { peer: bob_peer_id, known_addresses: vec![bob_addr] }).unwrap();

    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(event) = alice.next_event() => {
                    if matches!(event, P2pEvent::PeerConnected(p) if p == bob_peer_id) {
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

    bob.command(Command::AnnounceMixRelay).unwrap();

    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(event) = alice.next_event() => {
                    if matches!(event, P2pEvent::MixRelayDiscovered { peer } if peer == bob_peer_id) {
                        return;
                    }
                }
                Some(_) = bob.next_event() => {}
            }
        }
    })
    .await
    .expect("alice never discovered bob's mix relay announcement");

    let original_envelope = b"an already-encrypted envelope, round-tripped through Bob's mailbox".to_vec();
    alice
        .command(Command::DepositToMailbox { shared_material: SHARED_MATERIAL.to_vec(), envelope: original_envelope.clone() })
        .unwrap();

    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(event) = alice.next_event() => {
                    if let P2pEvent::MixForwardFailed { reason } = event {
                        panic!("alice's deposit failed to route: {reason}");
                    }
                }
                Some(event) = bob.next_event() => {
                    if matches!(event, P2pEvent::MailboxDepositStored) {
                        return;
                    }
                }
            }
        }
    })
    .await
    .expect("bob never stored alice's mailbox deposit");

    // Now Alice, acting as though she were the recipient (same
    // shared_material, which is exactly what makes this whole scheme
    // work without either side needing to reveal a real identity to
    // whoever stores the deposit), retrieves it back.
    alice.command(Command::RetrieveFromMailbox { shared_material: SHARED_MATERIAL.to_vec() }).unwrap();

    let retrieved = tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(event) = alice.next_event() => {
                    match event {
                        P2pEvent::MailboxEnvelopeRetrieved { envelope } => return envelope,
                        P2pEvent::MixForwardFailed { reason } => panic!("alice's retrieval query failed to route: {reason}"),
                        _ => {}
                    }
                }
                Some(_) = bob.next_event() => {}
            }
        }
    })
    .await
    .expect("alice never received her own deposit back");

    assert_eq!(retrieved, original_envelope);
}
