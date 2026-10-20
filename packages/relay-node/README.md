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

A prebuilt static binary is published to the `relay` release tag by the
`relay-node-build` workflow — prefer it over compiling on the machine you
intend to run this on, since a release build of this crate pulls in the
whole libp2p tree and peaks well above what a small VPS has spare:

```bash
curl -L -o spiritchat-relay \
  https://github.com/B760m670/gamee/releases/download/relay/spiritchat-relay
chmod +x spiritchat-relay && sudo mv spiritchat-relay /usr/local/bin/
```

To build it yourself instead:

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

# Keep the relay strictly subordinate to whatever else the machine is
# for. Mining saturates a core by design, so on a box that is also doing
# real work these are not optional:
Nice=19               # mining yields the moment anything else wants the CPU
CPUWeight=20          # and loses the tie when it doesn't (normal is 100)
MemoryMax=512M        # a hard ceiling, so a leak or a bad estimate kills
MemorySwapMax=0       # the relay rather than the machine's real workload

[Install]
WantedBy=multi-user.target
```

## What it costs

Measured on an idle node (one core, release build), so treat these as a
**floor**: a relay with a populated DHT routing table, a gossipsub mesh
and real connections will sit above them.

| | idle | mining |
|---|---|---|
| Resident memory | 17 MB | 20 MB |
| CPU | ~0% | ~96% of one core |
| Threads | 7 | 7 |

Memory is small enough not to be the deciding factor on any realistic
host; **CPU is**. Mining is a continuously saturated core, so on a
single-core machine running anything else, use `--no-mine` — `Nice=19`
cannot conjure a second core.

Disk is roughly 1.6 KB per block, so the chain grows on the order of
100-200 MB/year at the 5-minute target block time, plus the mailbox's own
50 MB budget. On a fresh network expect a burst of blocks at first: the
difficulty starts trivially low and ratchets up over successive retargets
until it reaches that target.

## What it stores

- `identity.seed` — 32 bytes, `0600`. The relay's identity; delete it and
  the relay comes back as a brand new peer.
- `ledger.redb` — the public `@username` chain, same as every node.
- `mailbox.redb` — other people's encrypted, tag-addressed deposits
  (50 MB budget by default, 14-day retention, oldest evicted first).
