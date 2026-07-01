//! End-to-end proof that two `P2pNode`s — the actual public API, not
//! internal swarm plumbing — can find each other and exchange an envelope
//! over a real dialed connection, with no relay/bootstrap infrastructure
//! involved (this test never reaches the public internet: it dials a
//! directly-known loopback address, the same as a same-network mDNS-
//! discovered peer would be dialed).

use spiritchat_p2p_core::{Command, P2pEvent, P2pNode};

#[tokio::test]
async fn alice_dials_bob_directly_and_they_exchange_an_envelope() {
    let mut alice = P2pNode::spawn_with_bootstrap([1u8; 32], vec![]).unwrap();
    let mut bob = P2pNode::spawn_with_bootstrap([2u8; 32], vec![]).unwrap();
    let bob_peer_id = bob.local_peer_id();

    // Wait for bob to report a concrete listen address (skip the 0.0.0.0
    // wildcard address the OS also reports).
    let bob_addr = loop {
        match bob.next_event().await.expect("bob's event loop is alive") {
            P2pEvent::ListeningOn(addr) if !addr.to_string().contains("0.0.0.0") => break addr,
            _ => continue,
        }
    };

    alice
        .command(Command::Dial { peer: bob_peer_id, known_addresses: vec![bob_addr] })
        .unwrap();

    let mut alice_connected = false;
    let mut sent = false;
    let mut delivered = false;
    let mut bob_received: Option<Vec<u8>> = None;

    loop {
        tokio::select! {
            Some(event) = alice.next_event() => {
                match event {
                    P2pEvent::PeerConnected(peer) if peer == bob_peer_id => {
                        alice_connected = true;
                        alice.command(Command::SendEnvelope {
                            to: bob_peer_id,
                            bytes: b"an already-encrypted envelope".to_vec(),
                        }).unwrap();
                        sent = true;
                    }
                    P2pEvent::EnvelopeDelivered { to } if to == bob_peer_id => {
                        delivered = true;
                    }
                    P2pEvent::EnvelopeDeliveryFailed { reason, .. } => {
                        panic!("delivery failed: {reason}");
                    }
                    _ => {}
                }
            }
            Some(event) = bob.next_event() => {
                if let P2pEvent::EnvelopeReceived { bytes, .. } = event {
                    bob_received = Some(bytes);
                }
            }
        }

        if delivered && bob_received.is_some() {
            break;
        }
    }

    assert!(alice_connected, "alice never saw a ConnectionEstablished event for bob");
    assert!(sent, "alice never sent the envelope");
    assert_eq!(bob_received.unwrap(), b"an already-encrypted envelope");
}

#[tokio::test]
async fn a_peer_can_announce_and_resolve_its_own_addresses_via_the_dht() {
    // Two directly-connected nodes act as each other's DHT peers for this
    // test — no public bootstrap network involved, but it exercises the
    // exact put_record/get_record path a real global lookup would use.
    let mut alice = P2pNode::spawn_with_bootstrap([3u8; 32], vec![]).unwrap();
    let mut bob = P2pNode::spawn_with_bootstrap([4u8; 32], vec![]).unwrap();
    let alice_peer_id = alice.local_peer_id();
    let bob_peer_id = bob.local_peer_id();

    let bob_addr = loop {
        match bob.next_event().await.unwrap() {
            P2pEvent::ListeningOn(addr) if !addr.to_string().contains("0.0.0.0") => break addr,
            _ => continue,
        }
    };

    alice
        .command(Command::Dial { peer: bob_peer_id, known_addresses: vec![bob_addr.clone()] })
        .unwrap();

    // Wait until bob has specifically completed identify with alice — that
    // (not just the connection being up) is what puts alice into bob's own
    // Kademlia routing table, which put_record needs someone in to
    // replicate to at all.
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

    bob.command(Command::AnnounceAddresses { addresses: vec![bob_addr.clone()] }).unwrap();

    // Wait for bob's own put_record to actually complete before alice
    // looks it up — otherwise this races the DHT write against the read.
    loop {
        match bob.next_event().await.unwrap() {
            P2pEvent::AddressesAnnounced => break,
            P2pEvent::AddressAnnouncementFailed { reason } => {
                panic!("bob failed to announce its own addresses: {reason}")
            }
            _ => continue,
        }
    }

    alice.command(Command::ResolvePeerAddresses { peer: bob_peer_id }).unwrap();

    loop {
        tokio::select! {
            Some(event) = alice.next_event() => {
                match event {
                    P2pEvent::PeerAddressesResolved { peer, addresses } if peer == bob_peer_id => {
                        assert!(addresses.contains(&bob_addr));
                        return;
                    }
                    P2pEvent::PeerAddressResolutionFailed { peer } if peer == bob_peer_id => {
                        panic!("address resolution failed for a peer we're directly connected to");
                    }
                    _ => {}
                }
            }
            Some(_) = bob.next_event() => {}
        }
    }
}
