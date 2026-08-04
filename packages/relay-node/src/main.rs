//! A headless SpiritChat node for always-on machines — the first
//! *standing* peer this network can have. Runs the exact same
//! `spiritchat-p2p-core` node the phones run (same mailbox cache, same
//! passive mix forwarding, same ledger gossip), with only the policy
//! differences an always-on, mains-powered machine affords:
//!
//! - announces itself under the well-known standing-relay DHT key
//!   (`Command::AnnouncePublicRelay`) so fresh installs can find it;
//! - re-broadcasts its mix routing key on a short cadence
//!   (`Command::AnnounceMixRelay`) so every newly connected peer hears it;
//! - keeps Loopix cover traffic on by default (phones gate this behind
//!   foreground-and-charging; a plugged-in machine has no such concern);
//! - mines the `@username` ledger by default, which is what lets phones
//!   stop mining entirely (see `--no-mine`).
//!
//! That last point is load-bearing, not a nicety. A `@username` claim is
//! only ever *confirmed* by being mined into a block, so the ledger needs
//! someone grinding SHA-256 or every claim sits in the mempool forever.
//! Historically the phones were the only miners, which is both a poor use
//! of a battery-powered device and something the App Store does not allow
//! (guideline 3.1.5(b) permits mining only when "performed off device").
//! Moving the work here resolves both at once: the phones submit claims,
//! standing relays turn them into blocks. A network with no relay running
//! still delivers messages — mailboxes, mixnet and chat are independent of
//! the ledger — it simply cannot confirm new usernames until one appears.
//!
//! Deliberately **not** a server: it holds no accounts, sees no
//! plaintext, learns no sender/recipient identities (deposits are keyed
//! by unlinkable tags, routed through Sphinx layers), and nothing in the
//! protocol trusts it more than any phone. If it disappears, the network
//! degrades exactly as if one more phone went offline. Anyone can run
//! any number of these.

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use spiritchat_p2p_core::{
    public_dht_bootstrap_addresses, public_key_bytes_from_seed, Command, Multiaddr, P2pEvent,
    P2pNode,
};

/// How often the mix routing key is re-broadcast to the gossip directory.
/// Gossip only reaches peers connected *at broadcast time*, so a standing
/// relay has to keep repeating itself for the benefit of whoever connected
/// since the last round — 30s matches the app's own retrieval-sweep
/// cadence, bounding a fresh peer's time-to-usable-relay to about one tick.
const MIX_RELAY_ANNOUNCE_INTERVAL: Duration = Duration::from_secs(30);

/// How often the current listen addresses are re-published to the DHT
/// rendezvous record (records expire; same reason the app re-announces).
const ADDRESS_ANNOUNCE_INTERVAL: Duration = Duration::from_secs(10 * 60);

struct Config {
    data_dir: PathBuf,
    port: Option<u16>,
    bootstrap: Vec<Multiaddr>,
    cover_traffic: bool,
    mine: bool,
}

const USAGE: &str = "\
spiritchat-relay — a headless, always-on SpiritChat peer (mix relay + mailbox cache)

USAGE:
  spiritchat-relay [OPTIONS]

OPTIONS:
  --data-dir <path>       Where the identity seed and ledger/mailbox caches live
                          (default: ./spiritchat-relay-data). Keep it stable across
                          restarts — the node's identity is derived from it.
  --port <port>           Fixed TCP+QUIC listen port (default: OS-assigned). Set one
                          and open it in your firewall so the relay is directly
                          dialable; without inbound reachability it can still relay
                          for peers that dial out to it, but fresh installs can't
                          reach it first.
  --bootstrap <multiaddr> Bootstrap peer to join through, repeatable. Default: the
                          public IPFS DHT bootstrap set (what the phones use).
                          Passing at least one --bootstrap replaces that default —
                          useful for a private/test network.
  --no-cover-traffic      Disable Loopix cover/loop traffic (on by default here,
                          since a standing relay pays no battery cost).
  --no-mine               Don't mine the @username ledger. Mining is ON by default:
                          it is how claims get confirmed, and relays are the only
                          miners left now that the app no longer mines on phones.
                          Expect one core's worth of sustained CPU. Turning it off
                          is fine for a relay run purely for message delivery —
                          mailboxes, mixnet and chat don't involve the ledger — but
                          if no relay anywhere is mining, no new username can be
                          registered.
  --help                  This text.
";

fn parse_args() -> Result<Config, String> {
    let mut config = Config {
        data_dir: PathBuf::from("./spiritchat-relay-data"),
        port: None,
        bootstrap: Vec::new(),
        cover_traffic: true,
        mine: true,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => {
                let value = args.next().ok_or("--data-dir needs a path")?;
                config.data_dir = PathBuf::from(value);
            }
            "--port" => {
                let value = args.next().ok_or("--port needs a port number")?;
                config.port = Some(value.parse().map_err(|_| format!("not a valid port: {value}"))?);
            }
            "--bootstrap" => {
                let value = args.next().ok_or("--bootstrap needs a multiaddr")?;
                config.bootstrap.push(value.parse().map_err(|_| format!("not a valid multiaddr: {value}"))?);
            }
            "--no-cover-traffic" => config.cover_traffic = false,
            "--no-mine" => config.mine = false,
            "--help" | "-h" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}\n\n{USAGE}")),
        }
    }
    Ok(config)
}

/// Loads the 32-byte identity seed, generating (and persisting) one on
/// first run. The seed *is* this relay's identity — its PeerId, its mix
/// routing key, everything — so it lives in the data dir and never
/// changes across restarts; a relay whose identity churned on every start
/// would invalidate every provider/rendezvous record pointing at it.
fn load_or_create_seed(data_dir: &PathBuf) -> std::io::Result<[u8; 32]> {
    let path = data_dir.join("identity.seed");
    match std::fs::read(&path) {
        Ok(bytes) if bytes.len() == 32 => {
            let mut seed = [0u8; 32];
            seed.copy_from_slice(&bytes);
            Ok(seed)
        }
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{} exists but isn't exactly 32 bytes — refusing to guess; delete it to generate a fresh identity", path.display()),
        )),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let seed: [u8; 32] = rand::random();
            let mut file = std::fs::File::create(&path)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            }
            file.write_all(&seed)?;
            Ok(seed)
        }
        Err(err) => Err(err),
    }
}

fn log(message: &str) {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    println!("[{now}] {message}");
    // Piped/service stdout is block-buffered — flush so an operator
    // tailing journald/logs sees lines as they happen, not in bursts.
    let _ = std::io::stdout().flush();
}

#[tokio::main]
async fn main() {
    let config = match parse_args() {
        Ok(config) => config,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };

    if let Err(err) = std::fs::create_dir_all(&config.data_dir) {
        eprintln!("cannot create data dir {}: {err}", config.data_dir.display());
        std::process::exit(1);
    }
    let seed = match load_or_create_seed(&config.data_dir) {
        Ok(seed) => seed,
        Err(err) => {
            eprintln!("cannot load or create the identity seed: {err}");
            std::process::exit(1);
        }
    };

    let bootstrap = if config.bootstrap.is_empty() {
        public_dht_bootstrap_addresses()
    } else {
        config.bootstrap.clone()
    };

    let mut node = match P2pNode::spawn_with_config(seed, bootstrap, config.port, config.data_dir.join("ledger.redb")) {
        Ok(node) => node,
        Err(err) => {
            eprintln!("failed to start the P2P node: {err}");
            std::process::exit(1);
        }
    };

    log(&format!("standing relay up — peer id {}", node.local_peer_id()));
    log(&format!("data dir: {}", config.data_dir.display()));

    // Deferred internally until the first connection lands (see
    // `needs_dht_peer`), so it's safe to issue immediately.
    let _ = node.command(Command::AnnouncePublicRelay);
    if config.cover_traffic {
        let _ = node.command(Command::SetMixDummyTrafficActive { enabled: true });
    }
    if config.mine {
        // Blocks are attributed to the *public* half of this relay's
        // identity — never the seed, which is its private key and would be
        // published to the network in every block header we mined.
        match public_key_bytes_from_seed(&seed) {
            Ok(public_key) => {
                let _ = node.command(Command::StartMining { public_key });
                log("mining the @username ledger (one core; --no-mine to disable)");
            }
            Err(err) => {
                // Not fatal: a relay that can't mine is still a useful relay,
                // and refusing to start would take message delivery down over
                // a feature that is strictly additive.
                log(&format!("not mining — cannot derive this relay's public key: {err}"));
            }
        }
    } else {
        log("mining disabled (--no-mine) — this relay won't confirm @username claims");
    }

    let mut listen_addresses: Vec<Multiaddr> = Vec::new();
    let mut mix_announce_ticker = tokio::time::interval(MIX_RELAY_ANNOUNCE_INTERVAL);
    let mut address_announce_ticker = tokio::time::interval(ADDRESS_ANNOUNCE_INTERVAL);

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                log("shutting down");
                let _ = node.command(Command::Shutdown);
                // Give the event loop a moment to actually process the
                // shutdown (drop the ledger/mailbox stores cleanly) —
                // exiting the process mid-write would be safe (redb is
                // crash-consistent) but needlessly discards a committed
                // state other peers already saw.
                tokio::time::sleep(Duration::from_millis(300)).await;
                return;
            }
            _ = mix_announce_ticker.tick() => {
                let _ = node.command(Command::AnnounceMixRelay);
            }
            _ = address_announce_ticker.tick() => {
                if !listen_addresses.is_empty() {
                    let _ = node.command(Command::AnnounceAddresses { addresses: listen_addresses.clone() });
                }
            }
            event = node.next_event() => {
                let Some(event) = event else {
                    log("event loop stopped — exiting");
                    return;
                };
                match &event {
                    P2pEvent::ListeningOn(address) => {
                        if !listen_addresses.contains(address) {
                            listen_addresses.push(address.clone());
                            let _ = node.command(Command::AnnounceAddresses { addresses: listen_addresses.clone() });
                        }
                    }
                    // The steady-state hum (cover traffic, gossip) isn't
                    // worth a log line each; connections, deposits, and
                    // failures are what an operator actually watches for.
                    P2pEvent::PeerDiscoveredLocally(_) => {}
                    _ => {}
                }
                match event {
                    P2pEvent::PeerConnected(peer) => log(&format!("peer connected: {peer}")),
                    P2pEvent::PeerDisconnected(peer) => log(&format!("peer disconnected: {peer}")),
                    P2pEvent::ListeningOn(address) => log(&format!("listening on {address}")),
                    P2pEvent::PublicRelayAnnounced => log("announced as a standing relay in the DHT"),
                    P2pEvent::PublicRelayAnnouncementFailed { reason } => log(&format!("standing-relay announcement FAILED: {reason}")),
                    P2pEvent::MailboxDepositStored => log("mailbox deposit accepted into the cache"),
                    P2pEvent::MixForwardFailed { reason } => log(&format!("mix forward failed: {reason}")),
                    P2pEvent::AddressAnnouncementFailed { reason } => log(&format!("address announcement failed: {reason}")),
                    P2pEvent::ChainTipChanged { height, .. } => log(&format!("ledger tip now at height {height}")),
                    // Distinct from the tip moving, which also fires for blocks
                    // received from other peers: this one is a block *this*
                    // relay found, so it's the only line that tells an operator
                    // their mining is actually contributing.
                    P2pEvent::NewBlockMined { height } => log(&format!("mined block at height {height}")),
                    _ => {}
                }
            }
        }
    }
}
