//! End-to-end proof that a blob registered with `Command::SetLocalBlob` on
//! one node can be fetched by another over a real dialed connection — the
//! same mechanism an avatar image would use, with no server, CDN, or
//! pinning service anywhere in the path.

use spiritchat_p2p_core::{Command, P2pEvent, P2pNode};

#[tokio::test]
async fn bob_fetches_a_blob_alice_is_serving() {
    let mut alice = P2pNode::spawn_with_bootstrap([5u8; 32], vec![]).unwrap();
    let mut bob = P2pNode::spawn_with_bootstrap([6u8; 32], vec![]).unwrap();
    let alice_peer_id = alice.local_peer_id();

    let alice_addr = loop {
        match alice.next_event().await.expect("alice's event loop is alive") {
            P2pEvent::ListeningOn(addr) if !addr.to_string().contains("0.0.0.0") => break addr,
            _ => continue,
        }
    };

    let blob_id = b"avatar-hash-placeholder".to_vec();
    let blob_bytes = b"pretend this is a compressed avatar image".to_vec();
    alice
        .command(Command::SetLocalBlob { id: blob_id.clone(), bytes: blob_bytes.clone() })
        .unwrap();

    bob.command(Command::Dial { peer: alice_peer_id, known_addresses: vec![alice_addr] })
        .unwrap();

    let mut fetched: Option<Vec<u8>> = None;
    loop {
        tokio::select! {
            Some(_) = alice.next_event() => {}
            Some(event) = bob.next_event() => {
                match event {
                    P2pEvent::PeerConnected(peer) if peer == alice_peer_id => {
                        bob.command(Command::FetchBlob { peer: alice_peer_id, id: blob_id.clone() }).unwrap();
                    }
                    P2pEvent::BlobFetched { peer, id, bytes } if peer == alice_peer_id && id == blob_id => {
                        fetched = Some(bytes);
                    }
                    P2pEvent::BlobFetchFailed { reason, .. } => {
                        panic!("blob fetch failed: {reason}");
                    }
                    _ => {}
                }
            }
        }

        if fetched.is_some() {
            break;
        }
    }

    assert_eq!(fetched.unwrap(), blob_bytes);
}

#[tokio::test]
async fn fetching_a_blob_nobody_registered_fails_cleanly() {
    let mut alice = P2pNode::spawn_with_bootstrap([7u8; 32], vec![]).unwrap();
    let mut bob = P2pNode::spawn_with_bootstrap([8u8; 32], vec![]).unwrap();
    let alice_peer_id = alice.local_peer_id();

    let alice_addr = loop {
        match alice.next_event().await.expect("alice's event loop is alive") {
            P2pEvent::ListeningOn(addr) if !addr.to_string().contains("0.0.0.0") => break addr,
            _ => continue,
        }
    };

    bob.command(Command::Dial { peer: alice_peer_id, known_addresses: vec![alice_addr] })
        .unwrap();

    let missing_id = b"nobody-registered-this".to_vec();
    let mut failed = false;
    loop {
        tokio::select! {
            Some(_) = alice.next_event() => {}
            Some(event) = bob.next_event() => {
                match event {
                    P2pEvent::PeerConnected(peer) if peer == alice_peer_id => {
                        bob.command(Command::FetchBlob { peer: alice_peer_id, id: missing_id.clone() }).unwrap();
                    }
                    P2pEvent::BlobFetchFailed { peer, id, .. } if peer == alice_peer_id && id == missing_id => {
                        failed = true;
                    }
                    P2pEvent::BlobFetched { .. } => {
                        panic!("fetched a blob that was never registered");
                    }
                    _ => {}
                }
            }
        }

        if failed {
            break;
        }
    }
}
