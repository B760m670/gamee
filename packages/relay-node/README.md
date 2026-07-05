# spiritchat-relay

A headless, always-on SpiritChat peer: the exact same node the phones run,
kept online so the network always has at least one reachable **mix relay**
and **mailbox cache**. This is what makes offline delivery real in a young
network — a sender's deposit needs *some* standing node to hold it until
the recipient comes online.

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
- `--bootstrap <multiaddr>` — join through specific peers instead of the
  public IPFS DHT (repeatable; useful for a private/test network). The
  multiaddr must include the peer id, e.g.
  `/ip4/10.0.0.5/tcp/4001/p2p/12D3Koo...`.

On start it announces itself under the well-known standing-relay DHT key;
every phone automatically discovers and dials announced relays whenever
its own mix-relay directory is empty, so no phone-side configuration is
needed.

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
