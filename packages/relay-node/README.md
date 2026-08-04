# spiritchat-relay

A headless, always-on SpiritChat peer: the exact same node the phones run,
kept online so the network always has at least one reachable **mix relay**,
**mailbox cache** and **ledger miner**. This is what makes offline delivery
real in a young network — a sender's deposit needs *some* standing node to
hold it until the recipient comes online.

Since the App Store build of the app no longer mines (App Review guideline
3.1.5(b) permits mining only where "the processing is performed off
device"), relays are now also where `@username` claims get confirmed. See
[Mining](#mining) — it is on by default.

It is deliberately **not a server**: it holds no accounts, sees no
plaintext, and learns no sender/recipient identities (mailbox deposits are
keyed by unlinkable rotating tags and arrive through Sphinx mix routing).
The protocol trusts it exactly as much as any phone — anyone can run any
number of these, and the network uses however many exist.

## Build and run

```bash
cd packages/relay-node
cargo build --release
./target/release/spiritchat-relay --port 4001 --data-dir /var/lib/spiritchat-relay
```

- `--port` — fixed TCP+QUIC listen port. Set one and open it in your
  firewall (both TCP and UDP) so fresh installs can dial in directly.
- `--data-dir` — holds the identity seed (the relay's stable identity —
  back it up if you care about keeping the same PeerId) plus the ledger
  and mailbox caches. Defaults to `./spiritchat-relay-data`.
- `--no-cover-traffic` — disables Loopix cover/loop traffic (on by
  default; costs on the order of hundreds of KB/hour).
- `--no-mine` — stops this relay mining the `@username` ledger (on by
  default; see [Mining](#mining)).
- `--bootstrap <multiaddr>` — join through specific peers instead of the
  public IPFS DHT (repeatable; useful for a private/test network). The
  multiaddr must include the peer id, e.g.
  `/ip4/10.0.0.5/tcp/4001/p2p/12D3Koo...`.

On start it announces itself under the well-known standing-relay DHT key;
every phone automatically discovers and dials announced relays whenever
its own mix-relay directory is empty, so no phone-side configuration is
needed.

## Mining

On by default. The relay grinds SHA-256 on **one core**, continuously,
looking for blocks that extend the `@username` ledger. Expect that core to
stay busy; the node deliberately never mines on more than one, so a relay
sharing a machine with other work stays a good neighbour.

This matters more than it might look. A `@username` claim is not registered
when a phone submits it — it is registered when it lands in a *mined block*.
Phones used to do that mining themselves, which is a poor use of a battery
and is not something the App Store allows, so the app's App Store build does
no mining at all and relies on relays for it.

The practical consequence: **if no relay anywhere is mining, no new username
can be registered.** Everything else keeps working — messages, groups,
media, mailboxes and mix routing never touch the ledger — but claims sit in
the mempool unconfirmed until some relay picks them up.

Use `--no-mine` if you are running a relay purely to help with message
delivery and would rather not spend the CPU. Running several relays where
only some mine is perfectly fine.

Blocks this relay finds are attributed to the public half of its identity
key (derived from the seed in `--data-dir`; the seed itself never leaves the
machine). There is no reward attached — the ledger has no currency, only
names — so attribution is just provenance.

## Run as a systemd service

```ini
[Unit]
Description=SpiritChat standing relay
After=network-online.target

[Service]
ExecStart=/usr/local/bin/spiritchat-relay --port 4001 --data-dir /var/lib/spiritchat-relay
Restart=on-failure
StateDirectory=spiritchat-relay

[Install]
WantedBy=multi-user.target
```

## What it stores

- `identity.seed` — 32 bytes, `0600`. The relay's identity; delete it and
  the relay comes back as a brand new peer.
- `ledger.redb` — the public `@username` chain, same as every node.
- `mailbox.redb` — other people's encrypted, tag-addressed deposits
  (50 MB budget by default, 14-day retention, oldest evicted first).
