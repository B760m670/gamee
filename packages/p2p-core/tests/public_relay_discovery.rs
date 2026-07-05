//! End-to-end proof of the standing-relay bootstrap path (`public_relay.rs`):
//! a relay that announced itself under the well-known DHT provider key is
//! found by a node that has never exchanged any SpiritChat traffic with it
//! (they share only a common DHT peer), gets auto-dialed, hands over its
//! mix routing key via ordinary gossip once connected — and at that point
//! a real mailbox deposit from the discovering node lands in the relay's
//! cache. This is exactly the "fresh install, empty relay directory"
//! situation that used to dead-end in `MixForwardFailed`.

use std::time::Duration;

use spiritchat_p2p_core::{Command, P2pEvent, P2pNode};

const WAIT_TIMEOUT: Duration = Duration::from_secs(30);

async fn listen_addr(node: &mut P2pNode) -> libp2p::Multiaddr {
    loop {
        match node.next_event().await.expect("event loop is alive") {
            P2pEvent::ListeningOn(addr) if !addr.to_string().contains("0.0.0.0") => return addr,
            _ => continue,
        }
    }
}

#[tokio::test]
async fn a_fresh_node_discovers_a_standing_relay_and_deposits_through_it() {
    let hub_dir = tempfile::tempdir().unwrap();
    let relay_dir = tempfile::tempdir().unwrap();
    let phone_dir = tempfile::tempdir().unwrap();

    // `hub` plays the role the public IPFS DHT's own nodes play in the
    // real network: a Kademlia peer both sides can reach that is *not* a
    // SpiritChat relay itself and holds no SpiritChat state beyond DHT
    // records.
    let mut hub = P2pNode::spawn_with_bootstrap([101u8; 32], vec![], hub_dir.path().join("ledger.redb")).unwrap();
    let mut relay = P2pNode::spawn_with_bootstrap([102u8; 32], vec![], relay_dir.path().join("ledger.redb")).unwrap();
    let mut phone = P2pNode::spawn_with_bootstrap([103u8; 32], vec![], phone_dir.path().join("ledger.redb")).unwrap();

    let hub_peer_id = hub.local_peer_id();
    let relay_peer_id = relay.local_peer_id();
    let hub_addr = listen_addr(&mut hub).await;

    relay.command(Command::Dial { peer: hub_peer_id, known_addresses: vec![hub_addr.clone()] }).unwrap();
    phone.command(Command::Dial { peer: hub_peer_id, known_addresses: vec![hub_addr] }).unwrap();

    // Both sides need identify to have completed with the hub before DHT
    // operations can replicate/walk through it (same wait every *_via_dht
    // test in this suite does).
    tokio::time::timeout(WAIT_TIMEOUT, async {
        let (mut relay_ready, mut phone_ready) = (false, false);
        loop {
            tokio::select! {
                Some(_) = hub.next_event() => {}
                Some(event) = relay.next_event() => {
                    if matches!(event, P2pEvent::PeerIdentified(p) if p == hub_peer_id) {
                        relay_ready = true;
                    }
                }
                Some(event) = phone.next_event() => {
                    if matches!(event, P2pEvent::PeerIdentified(p) if p == hub_peer_id) {
                        phone_ready = true;
                    }
                }
            }
            if relay_ready && phone_ready {
                return;
            }
        }
    })
    .await
    .expect("relay/phone never finished identify with the hub");

    relay.command(Command::AnnouncePublicRelay).unwrap();
    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(_) = hub.next_event() => {}
                Some(event) = relay.next_event() => {
                    if matches!(event, P2pEvent::PublicRelayAnnounced) {
                        return;
                    }
                    if let P2pEvent::PublicRelayAnnouncementFailed { reason } = event {
                        panic!("the relay's announcement failed: {reason}");
                    }
                }
            }
        }
    })
    .await
    .expect("the relay's provider record never reached the DHT");

    // The phone discovers and auto-dials the relay; the connection then
    // lets gossip deliver the relay's mix routing key. The relay
    // re-announces on a short cadence, exactly as the standing-relay
    // binary does, since a gossipsub broadcast sent before the mesh
    // includes the phone would otherwise be missed forever.
    phone.command(Command::DiscoverPublicRelays).unwrap();
    let mut announce_ticker = tokio::time::interval(Duration::from_secs(1));
    tokio::time::timeout(WAIT_TIMEOUT, async {
        let (mut discovered, mut connected) = (false, false);
        loop {
            tokio::select! {
                Some(_) = hub.next_event() => {}
                Some(_) = relay.next_event() => {}
                Some(event) = phone.next_event() => {
                    match event {
                        P2pEvent::PublicRelayDiscovered { peer } if peer == relay_peer_id => discovered = true,
                        P2pEvent::PeerConnected(peer) if peer == relay_peer_id => connected = true,
                        P2pEvent::MixRelayDiscovered { peer } if peer == relay_peer_id => return,
                        _ => {}
                    }
                }
                _ = announce_ticker.tick() => {
                    let _ = relay.command(Command::AnnounceMixRelay);
                }
            }
            // Not strictly required for the final assertion, but if this
            // times out, knowing which stage stalled matters.
            let _ = (discovered, connected);
        }
    })
    .await
    .expect("the phone never learned the relay's mix routing key");

    // The actual point of all of the above: a deposit now has somewhere
    // to go. Single known relay -> it is deterministically the path's
    // final hop, so the deposit must land in *its* mailbox cache.
    phone
        .command(Command::DepositToMailbox {
            shared_material: b"shared between two peers who have never been online together".to_vec(),
            envelope: b"an already-encrypted envelope, opaque to every layer below the app".to_vec(),
        })
        .unwrap();

    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(_) = hub.next_event() => {}
                Some(event) = relay.next_event() => {
                    if matches!(event, P2pEvent::MailboxDepositStored) {
                        return;
                    }
                }
                Some(event) = phone.next_event() => {
                    if let P2pEvent::MixForwardFailed { reason } = event {
                        panic!("the deposit failed to route: {reason}");
                    }
                }
            }
        }
    })
    .await
    .expect("the deposit never landed in the relay's mailbox cache");
}
