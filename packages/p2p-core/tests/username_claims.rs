//! End-to-end proof that a username claim published by one node can be
//! resolved by another over the DHT — the same mechanism a real @username
//! lookup would use (with signature verification of the opaque `claim`
//! bytes happening at the app layer, not here; this crate only moves them).

use spiritchat_p2p_core::{Command, P2pEvent, P2pNode};

#[tokio::test]
async fn bob_resolves_a_username_alice_announced() {
    let mut alice = P2pNode::spawn_with_bootstrap([9u8; 32], vec![]).unwrap();
    let mut bob = P2pNode::spawn_with_bootstrap([10u8; 32], vec![]).unwrap();
    let alice_peer_id = alice.local_peer_id();

    let alice_addr = loop {
        match alice.next_event().await.expect("alice's event loop is alive") {
            P2pEvent::ListeningOn(addr) if !addr.to_string().contains("0.0.0.0") => break addr,
            _ => continue,
        }
    };

    bob.command(Command::Dial { peer: alice_peer_id, known_addresses: vec![alice_addr] })
        .unwrap();

    // Wait until bob has identified alice (puts her in bob's routing table
    // — needed for bob's own get_record to have someone to ask) *and*
    // alice's own announce has completed, before bob resolves.
    let mut alice_identified = false;
    let claim = b"pretend-this-is-pubkey-and-signature".to_vec();
    loop {
        tokio::select! {
            Some(event) = alice.next_event() => {
                if matches!(event, P2pEvent::PeerConnected(peer) if peer == bob.local_peer_id()) {
                    alice.command(Command::AnnounceUsername {
                        username: "Alice".to_string(),
                        claim: claim.clone(),
                    }).unwrap();
                }
            }
            Some(event) = bob.next_event() => {
                if matches!(event, P2pEvent::PeerIdentified(p) if p == alice_peer_id) {
                    alice_identified = true;
                }
            }
        }
        if alice_identified {
            break;
        }
    }

    // Wait for alice's own announce to actually land before bob looks it up
    // — otherwise this races the DHT write against the read.
    loop {
        tokio::select! {
            Some(event) = alice.next_event() => {
                match event {
                    P2pEvent::UsernameAnnounced { .. } => break,
                    P2pEvent::UsernameAnnouncementFailed { reason, .. } => {
                        panic!("alice failed to announce her username: {reason}")
                    }
                    _ => {}
                }
            }
            Some(_) = bob.next_event() => {}
        }
    }

    // Resolving by a different case must still find it (case-insensitive).
    bob.command(Command::ResolveUsername { username: "alice".to_string() }).unwrap();

    loop {
        tokio::select! {
            Some(event) = bob.next_event() => {
                match event {
                    P2pEvent::UsernameResolved { username, claim: resolved } => {
                        assert_eq!(username, "alice");
                        assert_eq!(resolved, claim);
                        return;
                    }
                    P2pEvent::UsernameResolutionFailed { .. } => {
                        panic!("resolution failed for a username alice just announced");
                    }
                    _ => {}
                }
            }
            Some(_) = alice.next_event() => {}
        }
    }
}

/// Reproduces the real-world "the quorum failed; needed 1 peers" failure:
/// a DHT command issued the instant a node spawns, before it has connected
/// to anyone, used to fail immediately (Kademlia has nobody to even ask).
/// It should instead be held and replayed once the first connection lands,
/// so the caller sees it succeed rather than an error that was really just
/// "you asked before bootstrapping finished."
#[tokio::test]
async fn a_username_announced_before_any_connection_is_deferred_until_one_exists() {
    let mut alice = P2pNode::spawn_with_bootstrap([13u8; 32], vec![]).unwrap();
    let mut bob = P2pNode::spawn_with_bootstrap([14u8; 32], vec![]).unwrap();
    let bob_peer_id = bob.local_peer_id();

    // Announce before alice has connected to anyone at all — this is the
    // exact race a real app hits announcing a username moments after
    // launch/onboarding, before the public DHT bootstrap connection lands.
    let claim = b"pretend-this-is-pubkey-and-signature".to_vec();
    alice
        .command(Command::AnnounceUsername { username: "Alice".to_string(), claim: claim.clone() })
        .unwrap();

    let bob_addr = loop {
        match bob.next_event().await.expect("bob's event loop is alive") {
            P2pEvent::ListeningOn(addr) if !addr.to_string().contains("0.0.0.0") => break addr,
            _ => continue,
        }
    };

    alice
        .command(Command::Dial { peer: bob_peer_id, known_addresses: vec![bob_addr] })
        .unwrap();

    loop {
        tokio::select! {
            Some(event) = alice.next_event() => {
                match event {
                    P2pEvent::UsernameAnnounced { username } => {
                        assert_eq!(username, "Alice");
                        return;
                    }
                    P2pEvent::UsernameAnnouncementFailed { reason, .. } => {
                        panic!("the deferred announce still failed: {reason}")
                    }
                    _ => {}
                }
            }
            Some(_) = bob.next_event() => {}
        }
    }
}

#[tokio::test]
async fn resolving_an_unclaimed_username_fails_cleanly() {
    let mut alice = P2pNode::spawn_with_bootstrap([11u8; 32], vec![]).unwrap();
    let mut bob = P2pNode::spawn_with_bootstrap([12u8; 32], vec![]).unwrap();
    let alice_peer_id = alice.local_peer_id();

    let alice_addr = loop {
        match alice.next_event().await.expect("alice's event loop is alive") {
            P2pEvent::ListeningOn(addr) if !addr.to_string().contains("0.0.0.0") => break addr,
            _ => continue,
        }
    };

    bob.command(Command::Dial { peer: alice_peer_id, known_addresses: vec![alice_addr] })
        .unwrap();

    loop {
        tokio::select! {
            Some(_) = alice.next_event() => {}
            Some(event) = bob.next_event() => {
                if matches!(event, P2pEvent::PeerIdentified(p) if p == alice_peer_id) {
                    break;
                }
            }
        }
    }

    bob.command(Command::ResolveUsername { username: "nobody-claimed-this".to_string() })
        .unwrap();

    loop {
        tokio::select! {
            Some(_) = alice.next_event() => {}
            Some(event) = bob.next_event() => {
                match event {
                    P2pEvent::UsernameResolutionFailed { username } => {
                        assert_eq!(username, "nobody-claimed-this");
                        return;
                    }
                    P2pEvent::UsernameResolved { .. } => {
                        panic!("resolved a username nobody ever announced");
                    }
                    _ => {}
                }
            }
        }
    }
}
