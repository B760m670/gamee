//! End-to-end proof that `Command::DepositToMailbox` actually reaches a
//! connected mix relay, gets mined, wrapped in a real Sphinx packet,
//! routed, peeled, validated, and stored — the offline-delivery path this
//! whole mixnet exists for. Alice deposits for a (fictional, this test
//! doesn't need a real recipient) contact through Bob, who discovers her
//! as a mix relay isn't even needed here — Bob is Alice's own chosen
//! relay, discovered via gossip the same way `mix_relay_directory.rs`
//! already proves works.

use std::time::Duration;

use spiritchat_p2p_core::{Command, P2pEvent, P2pNode};

const WAIT_TIMEOUT: Duration = Duration::from_secs(15);

#[tokio::test]
async fn alice_deposits_a_message_and_bob_stores_it() {
    let alice_dir = tempfile::tempdir().unwrap();
    let bob_dir = tempfile::tempdir().unwrap();
    let mut alice = P2pNode::spawn_with_bootstrap([51u8; 32], vec![], alice_dir.path().join("ledger.redb")).unwrap();
    let mut bob = P2pNode::spawn_with_bootstrap([52u8; 32], vec![], bob_dir.path().join("ledger.redb")).unwrap();
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
    // Let gossipsub's mesh actually form before bob announces himself as
    // a relay, same reasoning as every other gossip-propagation test here.
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

    alice
        .command(Command::DepositToMailbox {
            shared_material: b"alice-and-bobs-recipient-shared-material".to_vec(),
            envelope: b"an already-encrypted envelope, offline delivery".to_vec(),
        })
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
}
