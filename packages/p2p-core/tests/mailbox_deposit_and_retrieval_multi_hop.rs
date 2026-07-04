//! Phase 8 hardening: proves the mixnet's real, `pick_mix_path`-driven
//! multi-hop routing (as opposed to the earlier single-relay shortcut every
//! other mailbox test here happens to exercise, since they only ever give
//! Alice exactly one relay to choose from) actually works end to end
//! through the public `Command::DepositToMailbox`/`RetrieveFromMailbox`
//! API, over a fully-meshed set of relays where any hop can reach any
//! other. It also demonstrates, observably rather than by construction
//! alone, the property this whole design exists for: of the three relays
//! Alice could have routed through, only the one that actually ends up as
//! the path's final hop ever reports storing anything — whichever
//! relay(s) merely forwarded her packet along the way never see, and never
//! report, the plaintext deposit landing.

use std::collections::HashSet;
use std::time::Duration;

use spiritchat_p2p_core::{Command, Multiaddr, P2pEvent, P2pNode, PeerId};

// Generous relative to the 2-node mailbox tests — this one has to stand
// up a full 4-node, 6-connection mesh plus 3 gossip discoveries before
// anything else can happen.
const WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const SHARED_MATERIAL: &[u8] = b"alice-and-bobs-recipient-shared-material-multi-hop";

async fn listen_addr(node: &mut P2pNode) -> Multiaddr {
    loop {
        match node.next_event().await.expect("node's event loop is alive") {
            P2pEvent::ListeningOn(addr) if !addr.to_string().contains("0.0.0.0") => return addr,
            _ => continue,
        }
    }
}

fn normalized_pair(a: PeerId, b: PeerId) -> (PeerId, PeerId) {
    if a.to_bytes() < b.to_bytes() {
        (a, b)
    } else {
        (b, a)
    }
}

#[tokio::test]
async fn alice_deposits_and_retrieves_through_a_real_multi_hop_mesh() {
    let alice_dir = tempfile::tempdir().unwrap();
    let bob_dir = tempfile::tempdir().unwrap();
    let carol_dir = tempfile::tempdir().unwrap();
    let dave_dir = tempfile::tempdir().unwrap();
    let mut alice = P2pNode::spawn_with_bootstrap([81u8; 32], vec![], alice_dir.path().join("ledger.redb")).unwrap();
    let mut bob = P2pNode::spawn_with_bootstrap([82u8; 32], vec![], bob_dir.path().join("ledger.redb")).unwrap();
    let mut carol = P2pNode::spawn_with_bootstrap([83u8; 32], vec![], carol_dir.path().join("ledger.redb")).unwrap();
    let mut dave = P2pNode::spawn_with_bootstrap([84u8; 32], vec![], dave_dir.path().join("ledger.redb")).unwrap();

    let alice_peer = alice.local_peer_id();
    let bob_peer = bob.local_peer_id();
    let carol_peer = carol.local_peer_id();
    let dave_peer = dave.local_peer_id();

    let bob_addr = listen_addr(&mut bob).await;
    let carol_addr = listen_addr(&mut carol).await;
    let dave_addr = listen_addr(&mut dave).await;

    // A full mesh — every one of the 4 nodes directly connected to every
    // other one — is what makes *any* random 3-hop path `pick_mix_path`
    // might choose among {bob, carol, dave} actually forwardable end to
    // end: each hop needs a live connection to the *next* hop, not just
    // Alice needing one to the first.
    alice.command(Command::Dial { peer: bob_peer, known_addresses: vec![bob_addr.clone()] }).unwrap();
    alice.command(Command::Dial { peer: carol_peer, known_addresses: vec![carol_addr.clone()] }).unwrap();
    alice.command(Command::Dial { peer: dave_peer, known_addresses: vec![dave_addr.clone()] }).unwrap();
    bob.command(Command::Dial { peer: carol_peer, known_addresses: vec![carol_addr.clone()] }).unwrap();
    bob.command(Command::Dial { peer: dave_peer, known_addresses: vec![dave_addr.clone()] }).unwrap();
    carol.command(Command::Dial { peer: dave_peer, known_addresses: vec![dave_addr] }).unwrap();

    let expected_pairs: HashSet<(PeerId, PeerId)> = [
        normalized_pair(alice_peer, bob_peer),
        normalized_pair(alice_peer, carol_peer),
        normalized_pair(alice_peer, dave_peer),
        normalized_pair(bob_peer, carol_peer),
        normalized_pair(bob_peer, dave_peer),
        normalized_pair(carol_peer, dave_peer),
    ]
    .into_iter()
    .collect();
    let mut connected_pairs: HashSet<(PeerId, PeerId)> = HashSet::new();

    tokio::time::timeout(WAIT_TIMEOUT, async {
        while connected_pairs.len() < expected_pairs.len() {
            tokio::select! {
                Some(event) = alice.next_event() => {
                    if let P2pEvent::PeerConnected(peer) = event { connected_pairs.insert(normalized_pair(alice_peer, peer)); }
                }
                Some(event) = bob.next_event() => {
                    if let P2pEvent::PeerConnected(peer) = event { connected_pairs.insert(normalized_pair(bob_peer, peer)); }
                }
                Some(event) = carol.next_event() => {
                    if let P2pEvent::PeerConnected(peer) = event { connected_pairs.insert(normalized_pair(carol_peer, peer)); }
                }
                Some(event) = dave.next_event() => {
                    if let P2pEvent::PeerConnected(peer) = event { connected_pairs.insert(normalized_pair(dave_peer, peer)); }
                }
            }
        }
    })
    .await
    .expect("the 4-node mesh never fully connected");
    // Let gossipsub's mesh actually form before anyone announces as a
    // relay, same reasoning as every other gossip-propagation test here.
    tokio::time::sleep(Duration::from_millis(1500)).await;

    bob.command(Command::AnnounceMixRelay).unwrap();
    carol.command(Command::AnnounceMixRelay).unwrap();
    dave.command(Command::AnnounceMixRelay).unwrap();

    let expected_relays: HashSet<PeerId> = [bob_peer, carol_peer, dave_peer].into_iter().collect();
    let mut discovered_relays: HashSet<PeerId> = HashSet::new();
    tokio::time::timeout(WAIT_TIMEOUT, async {
        while discovered_relays.len() < expected_relays.len() {
            tokio::select! {
                Some(event) = alice.next_event() => {
                    if let P2pEvent::MixRelayDiscovered { peer } = event {
                        discovered_relays.insert(peer);
                    }
                }
                Some(_) = bob.next_event() => {}
                Some(_) = carol.next_event() => {}
                Some(_) = dave.next_event() => {}
            }
        }
    })
    .await
    .expect("alice never discovered all three relays");
    assert_eq!(discovered_relays, expected_relays);

    let original_envelope = b"an already-encrypted envelope, routed through a real multi-hop path".to_vec();
    alice
        .command(Command::DepositToMailbox { shared_material: SHARED_MATERIAL.to_vec(), envelope: original_envelope.clone() })
        .unwrap();

    // Collect every `MailboxDepositStored` any of the three relays reports
    // — not just the first — for a short grace window after the first one
    // arrives, so a bug that stored (or reported storing) the same
    // deposit on more than one relay would actually be caught here rather
    // than the test just stopping at the first success.
    let mut storers: Vec<PeerId> = Vec::new();
    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(event) = alice.next_event() => {
                    if let P2pEvent::MixForwardFailed { reason } = event {
                        panic!("alice's deposit failed to route: {reason}");
                    }
                }
                Some(event) = bob.next_event() => {
                    if matches!(event, P2pEvent::MailboxDepositStored) { storers.push(bob_peer); }
                }
                Some(event) = carol.next_event() => {
                    if matches!(event, P2pEvent::MailboxDepositStored) { storers.push(carol_peer); }
                }
                Some(event) = dave.next_event() => {
                    if matches!(event, P2pEvent::MailboxDepositStored) { storers.push(dave_peer); }
                }
            }
            if !storers.is_empty() {
                break;
            }
        }
    })
    .await
    .expect("no relay ever reported storing alice's deposit");

    // A short grace period to catch any stray duplicate storage report —
    // draining, not asserting a specific count of *events* observed here,
    // since a well-behaved run simply won't produce any more.
    let _ = tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            tokio::select! {
                Some(event) = bob.next_event() => { if matches!(event, P2pEvent::MailboxDepositStored) { storers.push(bob_peer); } }
                Some(event) = carol.next_event() => { if matches!(event, P2pEvent::MailboxDepositStored) { storers.push(carol_peer); } }
                Some(event) = dave.next_event() => { if matches!(event, P2pEvent::MailboxDepositStored) { storers.push(dave_peer); } }
            }
        }
    })
    .await;

    assert_eq!(
        storers.len(),
        1,
        "exactly one relay (the path's true final hop) should ever report storing the deposit — \
         any intermediate hop reporting it too would mean the mailbox tag leaked past the layer meant to hide it"
    );

    // Now Alice, acting as though she were the recipient (same
    // shared_material), retrieves it back through an independently
    // multi-hop outbound query and SURB return leg. The query's own
    // outbound path picks its final hop deterministically (closest to the
    // tag, same as the deposit above did) — so with all 3 relays already
    // known to Alice, this should succeed on the very first attempt, no
    // retry needed; a regression here would mean the deterministic
    // routing fix stopped actually converging.
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
                Some(_) = carol.next_event() => {}
                Some(_) = dave.next_event() => {}
            }
        }
    })
    .await
    .expect("alice never received her own deposit back through the multi-hop mesh, even after retrying");

    assert_eq!(retrieved, original_envelope);
}
