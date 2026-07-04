//! End-to-end proof that a Sphinx packet built by one node is actually
//! peeled and relayed, hop by hop, over real dialed `P2pNode` connections
//! — not just the pure in-process logic `mix.rs`'s own unit tests already
//! cover. Alice sends through Bob (a relay that only ever sees "forward
//! this to Carol", never the original message) to Carol (the final hop,
//! who alone recovers the payload).

use std::time::Duration;

use sphinx_packet::route::DestinationAddressBytes;
use spiritchat_p2p_core::{build_packet, node_address_for, routing_keypair_from_seed, Command, MixHop, P2pEvent, P2pNode};

#[tokio::test]
async fn alice_relays_a_mix_packet_through_bob_to_carol() {
    let alice_seed = [10u8; 32];
    let bob_seed = [11u8; 32];
    let carol_seed = [12u8; 32];

    let alice_dir = tempfile::tempdir().unwrap();
    let bob_dir = tempfile::tempdir().unwrap();
    let carol_dir = tempfile::tempdir().unwrap();
    let mut alice = P2pNode::spawn_with_bootstrap(alice_seed, vec![], alice_dir.path().join("ledger.redb")).unwrap();
    let mut bob = P2pNode::spawn_with_bootstrap(bob_seed, vec![], bob_dir.path().join("ledger.redb")).unwrap();
    let mut carol = P2pNode::spawn_with_bootstrap(carol_seed, vec![], carol_dir.path().join("ledger.redb")).unwrap();
    let bob_peer_id = bob.local_peer_id();
    let carol_peer_id = carol.local_peer_id();

    let bob_addr = loop {
        match bob.next_event().await.expect("bob's event loop is alive") {
            P2pEvent::ListeningOn(addr) if !addr.to_string().contains("0.0.0.0") => break addr,
            _ => continue,
        }
    };
    let carol_addr = loop {
        match carol.next_event().await.expect("carol's event loop is alive") {
            P2pEvent::ListeningOn(addr) if !addr.to_string().contains("0.0.0.0") => break addr,
            _ => continue,
        }
    };

    alice.command(Command::Dial { peer: bob_peer_id, known_addresses: vec![bob_addr] }).unwrap();
    bob.command(Command::Dial { peer: carol_peer_id, known_addresses: vec![carol_addr] }).unwrap();

    // Both hops of the relay chain (alice<->bob, bob<->carol) need to be
    // up before alice sends anything, or bob won't yet know how to reach
    // carol when it peels a Forward outcome.
    let mut alice_bob_connected = false;
    let mut bob_carol_connected = false;
    while !(alice_bob_connected && bob_carol_connected) {
        tokio::select! {
            Some(event) = alice.next_event() => {
                if matches!(event, P2pEvent::PeerConnected(p) if p == bob_peer_id) {
                    alice_bob_connected = true;
                }
            }
            Some(event) = bob.next_event() => {
                if matches!(event, P2pEvent::PeerConnected(p) if p == carol_peer_id) {
                    bob_carol_connected = true;
                }
            }
            Some(_) = carol.next_event() => {}
        }
    }

    // Alice doesn't need a live directory lookup to learn Bob's and
    // Carol's mix routing public keys — they're deterministically derived
    // from the same identity seed their libp2p PeerId already comes from
    // (`routing_keypair_from_seed`), the same way a real sender would
    // derive them from a peer's already-known identity public key without
    // a network round trip.
    let (_bob_secret, bob_routing_public) = routing_keypair_from_seed(&bob_seed);
    let (_carol_secret, carol_routing_public) = routing_keypair_from_seed(&carol_seed);

    let path = vec![
        MixHop { address: node_address_for(&bob_peer_id.to_bytes()), public_key: bob_routing_public },
        MixHop { address: node_address_for(&carol_peer_id.to_bytes()), public_key: carol_routing_public },
    ];
    let message = b"a message no single relay should be able to read in full";
    let destination_address = DestinationAddressBytes::from_bytes([42u8; 32]);
    let identifier = [7u8; 16];

    let packet = build_packet(message, &path, destination_address, identifier, Duration::from_millis(20)).unwrap();

    alice
        .command(Command::SendMixPacket { first_hop: bob_peer_id, packet_bytes: packet.to_bytes() })
        .unwrap();

    let arrived = loop {
        tokio::select! {
            Some(event) = alice.next_event() => {
                if let P2pEvent::MixForwardFailed { reason } = event {
                    panic!("alice reported a mix forward failure: {reason}");
                }
            }
            Some(event) = bob.next_event() => {
                if let P2pEvent::MixForwardFailed { reason } = event {
                    panic!("bob (the relay hop) failed to forward: {reason}");
                }
            }
            Some(event) = carol.next_event() => {
                match event {
                    P2pEvent::MixPacketArrived { payload } => break payload,
                    P2pEvent::MixForwardFailed { reason } => {
                        panic!("carol (the final hop) failed to peel: {reason}");
                    }
                    _ => {}
                }
            }
        }
    };

    assert_eq!(&arrived[..message.len()], message);
}
