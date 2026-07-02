//! Proves `Command::Shutdown` actually stops the event loop — this is what
//! an app-level "sign out" depends on to fully release a node built from a
//! now-discarded identity instead of leaking it running forever.

use spiritchat_p2p_core::{Command, P2pNode};

#[tokio::test]
async fn shutdown_makes_next_event_return_none() {
    let mut node = P2pNode::spawn_with_bootstrap([13u8; 32], vec![]).unwrap();

    node.command(Command::Shutdown).unwrap();

    // Whatever events were already queued (at least one ListeningOn) may
    // still arrive first — the loop breaks *after* processing the Shutdown
    // command, not instantly — but the stream must end in `None`, not hang
    // or keep producing events forever.
    let mut saw_none = false;
    for _ in 0..50 {
        if node.next_event().await.is_none() {
            saw_none = true;
            break;
        }
    }
    assert!(saw_none, "next_event never returned None after Shutdown");
}
