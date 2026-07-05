//! End-to-end proof that a peer's avatar *pointer* (its current avatar
//! content id, not the image bytes themselves) published into the DHT
//! (`Command::AnnounceAvatarPointer`) can be found by another node
//! (`Command::ResolveAvatarPointer`) that never fetched it directly — the
//! mechanism that lets a peer discover which content id to `FetchBlob` for
//! even while the avatar's owner is currently offline. Fetching the actual
//! image bytes still needs a live connection either way; this only proves
//! the pointer half.

use std::time::Duration;

use spiritchat_p2p_core::{Command, P2pEvent, P2pNode};

const WAIT_TIMEOUT: Duration = Duration::from_secs(20);

#[tokio::test]
async fn bob_resolves_alices_avatar_pointer_from_the_dht() {
    let alice_dir = tempfile::tempdir().unwrap();
    let bob_dir = tempfile::tempdir().unwrap();
    let mut alice = P2pNode::spawn_with_bootstrap([93u8; 32], vec![], alice_dir.path().join("ledger.redb")).unwrap();
    let mut bob = P2pNode::spawn_with_bootstrap([94u8; 32], vec![], bob_dir.path().join("ledger.redb")).unwrap();
    let alice_peer_id = alice.local_peer_id();
    let bob_peer_id = bob.local_peer_id();

    let bob_addr = loop {
        match bob.next_event().await.expect("bob's event loop is alive") {
            P2pEvent::ListeningOn(addr) if !addr.to_string().contains("0.0.0.0") => break addr,
            _ => continue,
        }
    };

    alice.command(Command::Dial { peer: bob_peer_id, known_addresses: vec![bob_addr] }).unwrap();

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

    let avatar_content_id = b"deadbeef00112233".to_vec();
    alice.command(Command::AnnounceAvatarPointer { avatar_content_id: avatar_content_id.clone() }).unwrap();

    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            match alice.next_event().await.expect("alice's event loop is alive") {
                P2pEvent::AvatarPointerAnnounced => return,
                P2pEvent::AvatarPointerAnnouncementFailed { reason } => {
                    panic!("alice failed to announce her avatar pointer: {reason}")
                }
                _ => continue,
            }
        }
    })
    .await
    .expect("alice's avatar pointer announcement never completed");

    // Bob never fetched this directly from Alice — only the DHT record.
    bob.command(Command::ResolveAvatarPointer { owner: alice_peer_id }).unwrap();

    let resolved = tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(_) = alice.next_event() => {}
                Some(event) = bob.next_event() => {
                    match event {
                        P2pEvent::AvatarPointerResolved { owner, avatar_content_id } if owner == alice_peer_id => {
                            return avatar_content_id;
                        }
                        P2pEvent::AvatarPointerResolutionFailed { owner } if owner == alice_peer_id => {
                            panic!("bob's avatar pointer resolution failed");
                        }
                        _ => {}
                    }
                }
            }
        }
    })
    .await
    .expect("bob never resolved alice's avatar pointer from the DHT");

    assert_eq!(resolved, avatar_content_id);
}
