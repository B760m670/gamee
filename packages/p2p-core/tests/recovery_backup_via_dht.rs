//! End-to-end proof of network-backed account recovery: an encrypted
//! backup published into the DHT by one node is retrieved by a *different*
//! node (a fresh install restoring from the same phrase), and only the
//! correct identity seed decrypts it. This is the mechanism that turns
//! "restore from phrase" into "same keys, same account" instead of
//! "same keys, empty account".

use std::time::Duration;

use spiritchat_crypto_core::backup::{decrypt_backup, encrypt_backup};
use spiritchat_p2p_core::{Command, P2pEvent, P2pNode};

const WAIT_TIMEOUT: Duration = Duration::from_secs(20);

#[tokio::test]
async fn a_fresh_install_recovers_the_published_backup_and_decrypts_it_with_the_phrase_seed() {
    let old_dir = tempfile::tempdir().unwrap();
    let new_dir = tempfile::tempdir().unwrap();
    let mut old_install = P2pNode::spawn_with_bootstrap([81u8; 32], vec![], old_dir.path().join("ledger.redb")).unwrap();
    let mut new_install = P2pNode::spawn_with_bootstrap([82u8; 32], vec![], new_dir.path().join("ledger.redb")).unwrap();
    let old_peer_id = old_install.local_peer_id();

    let old_addr = loop {
        match old_install.next_event().await.expect("old install's event loop is alive") {
            P2pEvent::ListeningOn(addr) if !addr.to_string().contains("0.0.0.0") => break addr,
            _ => continue,
        }
    };
    new_install.command(Command::Dial { peer: old_peer_id, known_addresses: vec![old_addr] }).unwrap();

    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(_) = new_install.next_event() => {}
                Some(event) = old_install.next_event() => {
                    if matches!(event, P2pEvent::PeerIdentified(_)) { return; }
                }
            }
        }
    })
    .await
    .expect("the two installs never identified each other");

    // The account's identity seed — in the app this comes from the BIP39
    // phrase, identically on both installs. The *network* identities of
    // the two nodes in this test differ on purpose: what ties the backup
    // to the account is the record key + the encryption, not who published
    // it from which PeerId.
    let identity_seed = [7u8; 32];
    let owner_identity_public_key = vec![9u8; 32];

    let plaintext = br#"{"displayName":"kutik","contacts":["ab","cd"]}"#;
    let backup = encrypt_backup(&mut rand_chacha::ChaCha20Rng::from_entropy(), &identity_seed, plaintext).unwrap();

    old_install
        .command(Command::AnnounceRecoveryBackup {
            owner_identity_public_key: owner_identity_public_key.clone(),
            backup,
        })
        .unwrap();

    tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(_) = new_install.next_event() => {}
                Some(event) = old_install.next_event() => {
                    match event {
                        P2pEvent::RecoveryBackupAnnounced => return,
                        P2pEvent::RecoveryBackupAnnouncementFailed { reason } => panic!("announcement failed: {reason}"),
                        _ => {}
                    }
                }
            }
        }
    })
    .await
    .expect("the backup never reached the DHT");

    new_install
        .command(Command::ResolveRecoveryBackup { owner_identity_public_key: owner_identity_public_key.clone() })
        .unwrap();

    let ciphertext = tokio::time::timeout(WAIT_TIMEOUT, async {
        loop {
            tokio::select! {
                Some(_) = old_install.next_event() => {}
                Some(event) = new_install.next_event() => {
                    match event {
                        P2pEvent::RecoveryBackupResolved { owner_identity_public_key: owner, backup } => {
                            assert_eq!(owner, owner_identity_public_key);
                            return backup;
                        }
                        P2pEvent::RecoveryBackupResolutionFailed { .. } => panic!("resolution failed"),
                        _ => {}
                    }
                }
            }
        }
    })
    .await
    .expect("the fresh install never found the backup");

    // The right seed reads it; a wrong seed (a different account's phrase)
    // gets a clean authentication failure, never someone else's data.
    assert_eq!(decrypt_backup(&identity_seed, &ciphertext).unwrap(), plaintext);
    assert!(decrypt_backup(&[8u8; 32], &ciphertext).is_err());
}

use rand_chacha::rand_core::SeedableRng;
