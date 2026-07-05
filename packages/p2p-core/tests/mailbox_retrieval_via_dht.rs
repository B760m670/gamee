//! End-to-end proof of the real DHT-based replication added on top of the
//! existing mix-routed mailbox (see `mailbox_dht.rs`): once a relay
//! accepts a validated deposit as the mix path's final hop, it also
//! replicates the envelope into the public DHT under one of a small,
//! fixed number of per-tag slots. This test shows a third node — who
//! never learns the relay's Sphinx routing key at all, and so could never
//! build a real mix path to it — can still recover the deposit purely by
//! querying those DHT slots directly, the availability gap this feature
//! closes.

use std::time::Duration;

use spiritchat_p2p_core::{Command, P2pEvent, P2pNode};

const WAIT_TIMEOUT: Duration = Duration::from_secs(20);
const SHARED_MATERIAL: &[u8] = b"alice-and-daves-recipient-shared-material-dht";

async fn listen_addr(node: &mut P2pNode) -> spiritchat_p2p_core::Multiaddr {
    loop {
        match node.next_event().await.expect("node's event loop is alive") {
            P2pEvent::ListeningOn(addr) if !addr.to_string().contains("0.0.0.0") => return addr,
            _ => continue,
        }
    }
}

#[tokio::test]
async fn dave_recovers_alices_deposit_from_the_dht_with_no_mix_path_to_the_relay_at_all() {
    let alice_dir = tempfile::tempdir().unwrap();
    let relay_dir = tempfile::tempdir().unwrap();
    let dave_dir = tempfile::tempdir().unwrap();
    let mut alice = P2pNode::spawn_with_bootstrap([71u8; 32], vec![], alice_dir.path().join("ledger.redb")).unwrap();
    let mut relay = P2pNode::spawn_with_bootstrap([72u8; 32], vec![], relay_dir.path().join("ledger.redb")).unwrap();
    let relay_peer_id = relay.local_peer_id();

    let relay_addr = listen_addr(&mut relay).await;
    alice.command(Command::Dial { peer: relay_peer_id, known_addresses: vec![relay_addr.clone()] }).unwrap();

    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(event) = alice.next_event() => {
                    if matches!(event, P2pEvent::PeerConnected(p) if p == relay_peer_id) { return; }
                }
                Some(_) = relay.next_event() => {}
            }
        }
    })
    .await
    .expect("alice and the relay never connected");
    // Let gossipsub's mesh actually form before the relay announces
    // itself, same reasoning as every other gossip-propagation test here.
    tokio::time::sleep(Duration::from_millis(1500)).await;

    relay.command(Command::AnnounceMixRelay).unwrap();

    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(event) = alice.next_event() => {
                    if matches!(event, P2pEvent::MixRelayDiscovered { peer } if peer == relay_peer_id) { return; }
                }
                Some(_) = relay.next_event() => {}
            }
        }
    })
    .await
    .expect("alice never discovered the relay's mix announcement");

    alice
        .command(Command::DepositToMailbox {
            shared_material: SHARED_MATERIAL.to_vec(),
            envelope: b"an already-encrypted envelope, recovered purely via the DHT".to_vec(),
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
                Some(event) = relay.next_event() => {
                    if matches!(event, P2pEvent::MailboxDepositStored) { return; }
                }
            }
        }
    })
    .await
    .expect("the relay never stored alice's mailbox deposit");

    // Only now does Dave even exist — he was never around for the
    // gossip broadcast that taught alice the relay's Sphinx routing key,
    // so he can never learn it either (gossipsub doesn't replay old
    // messages to a peer that joins the mesh later). Dave dials the
    // relay directly (the same single hop `contact_card_via_dht.rs` uses)
    // purely so its own DHT record lands in reach — never to exchange
    // mix traffic.
    let mut dave = P2pNode::spawn_with_bootstrap([73u8; 32], vec![], dave_dir.path().join("ledger.redb")).unwrap();
    dave.command(Command::Dial { peer: relay_peer_id, known_addresses: vec![relay_addr] }).unwrap();

    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(event) = dave.next_event() => {
                    if matches!(event, P2pEvent::PeerIdentified(p) if p == relay_peer_id) { return; }
                }
                Some(_) = relay.next_event() => {}
            }
        }
    })
    .await
    .expect("dave never identified the relay");

    dave.command(Command::RetrieveFromMailbox { shared_material: SHARED_MATERIAL.to_vec() }).unwrap();

    let retrieved = tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                // Dave has no known mix relay routing key at all, so his
                // own mix-routed retrieval attempt is *expected* to fail
                // (a `MixForwardFailed` here is not a bug in this test —
                // it's the whole point being proven). Only a genuine
                // `MailboxEnvelopeRetrieved` — which can only have come
                // from the DHT slot lookups — counts as success.
                Some(event) = dave.next_event() => {
                    if let P2pEvent::MailboxEnvelopeRetrieved { envelope } = event {
                        return envelope;
                    }
                }
                Some(_) = alice.next_event() => {}
                Some(_) = relay.next_event() => {}
            }
        }
    })
    .await
    .expect("dave never recovered alice's deposit from the DHT");

    assert_eq!(retrieved, b"an already-encrypted envelope, recovered purely via the DHT".to_vec());
}
