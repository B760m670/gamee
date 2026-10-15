# Running a relay on Android (Termux / Ubuntu container)

What a `spiritchat-relay` process needs from the system it runs on, written
for whoever is setting that system up. Nothing here is Android-specific
engineering on our side — the binary is an ordinary headless Linux daemon —
but Android's process and power management make a few of these requirements
sharper than they would be on a server.

## What the program is

A single statically linked ARM64 binary. It is a peer in a serverless
peer-to-peer messenger: it carries other people's already-encrypted traffic
and never sees plaintext, holds no accounts, and has no API or web UI. It
runs continuously and talks only to the network.

```
https://github.com/B760m670/gamee/releases/download/relay/spiritchat-relay-aarch64
```

Built for `aarch64-unknown-linux-musl` and **statically linked**: it has no
`NEEDED` entries and loads no shared libraries, so it does not care which
distribution, libc or package set the container has. `chmod +x` and run it.

## What it needs

**Architecture.** `aarch64`. It will not run on an x86_64 emulator layer
without one; there is a separate x86_64 build if that is ever relevant.

**Outbound network, TCP and UDP, to arbitrary ports.** It dials peers
wherever they listen — 4001 is conventional but far from universal. UDP is
not optional: QUIC is a first-class transport here and often the only one
that works between two NAT-ed peers.

**Outbound UDP port 53 to 1.1.1.1 and 1.0.0.1, with TXT records intact.**
This one is easy to get wrong. The binary does **not** use the system
resolver or `/etc/resolv.conf`; it has Cloudflare's addresses compiled in,
because the platforms it targets do not reliably have a readable resolver
config. It also depends specifically on **TXT** lookups — the public
bootstrap addresses are `/dnsaddr/...`, which resolve through a three-level
chain of TXT records. An environment that answers A records but strips TXT
will leave the node unable to join the network at all, with no obvious
symptom beyond dial timeouts. Worth testing directly:

```sh
dig +short TXT _dnsaddr.bootstrap.libp2p.io @1.1.1.1
```

Four `dnsaddr=/dnsaddr/...` lines means this is fine. Empty output means it
is not, and nothing else will work until it is.

**A listen port it can bind on TCP and UDP.** Pick anything above 1024;
Android does not allow binding lower ports without root. 4001 is the
convention. Binding succeeds whether or not the port is reachable from
outside — see "Reachability" below for what inbound access changes.

**A writable data directory** that survives restarts, given with
`--data-dir`. It holds:

| File | Contents |
|---|---|
| `identity.seed` | 32 bytes, mode `0600` — this node's identity. Delete it and the node returns as a stranger. |
| `ledger.redb` | The public `@username` chain. Grows ~1.6 KB per block. |
| `mailbox.redb` | Other people's encrypted deposits. 50 MB budget, 14-day retention. |

Anywhere persistent is fine. Inside a proot container, the container's own
filesystem works.

**Roughly accurate wall-clock time.** Deposits carry timestamps and are
refused beyond two hours of drift. Android's clock is normally fine; this
only matters if the container somehow has its own, wrong one.

**A raised file-descriptor limit.** The default `ulimit -n` of 1024 is low
for a node holding many connections. 65536 is comfortable. If the
supervisor cannot set it, `ulimit -n 65536` before exec works.

## What it does not need

No root. No systemd — any supervisor, or a plain background process, is
fine. No inbound port forwarding to be *useful* (see below). No X, no GUI,
no package installs of any kind: it is one static file.

## The Android-specific part: keeping it alive

This is the requirement most likely to cause confusion, because the failure
looks like a bug in the program rather than what it is.

Android suspends and kills background processes aggressively. Without
intervention the relay will run normally while the screen is on and die
silently minutes after it goes off. Whatever is needed to prevent that —
`termux-wake-lock`, exempting the app from battery optimisation, a
foreground service, whatever the chosen container provides — has to be in
place, and it has to be arranged at the **Android** level. Nothing inside
the Linux container can prevent Android from killing the container.

A supervisor that restarts the process is worth having, but it is not a
substitute: a restarted process that is killed again in two minutes is not
running.

## Reachability: what inbound access changes

Worth stating because it decides how useful the node is, and it is not
all-or-nothing.

Connections in this network are **bidirectional once established**. A node
that dials out can carry traffic for every peer it connected to, in both
directions. So even with no inbound access at all, the node is a working
mix relay and mailbox cache for the peers it reached itself.

What inbound access adds is the ability to be *found and dialled first*,
which is what makes a node useful to someone who has just installed the app
and knows nobody.

- **Home Wi-Fi with a port forward** — full reachability. Most reliable.
- **Mobile data over IPv6** — depends entirely on the carrier. Some route
  inbound IPv6 to the handset, some firewall it. Testable in minutes, and
  worth testing, because it is the best outcome available without a router.
- **Mobile data over IPv4** — carrier-grade NAT, no inbound, ever.

Mining is unaffected by any of this: it only publishes outward, so it works
identically behind the strictest NAT.

## Resource cost, measured

On one core, with no peers (so treat these as a floor — a node with a full
routing table sits above them):

| | Idle | Mining at 100% | Mining at 25% |
|---|---|---|---|
| Resident memory | 17 MB | 20 MB | 20 MB |
| CPU | ~0% | ~94% of one core | ~32% of one core |

Memory is negligible on any phone. CPU is entirely a function of
`--mine-intensity`, which the nonce search enforces itself.

**On a phone, use a low intensity — 10 is sensible.** It costs the network
nothing while miners are few: difficulty retargets toward a fixed block
time, so a slower miner confirms claims just as promptly against a
proportionally lower difficulty. What it buys is a cooler handset and a
battery that lasts.

Network traffic is the cost worth watching on a metered connection: cover
traffic alone is on the order of hundreds of KB/hour, plus whatever is
relayed for others. On Wi-Fi this is irrelevant; on a mobile plan it adds
up. `--no-cover-traffic` reduces it at a cost to everyone's traffic
analysis resistance.

## Running it

```sh
curl -L -o spiritchat-relay \
  https://github.com/B760m670/gamee/releases/download/relay/spiritchat-relay-aarch64
chmod +x spiritchat-relay
ulimit -n 65536
./spiritchat-relay --port 4001 --mine-intensity 10 --data-dir ~/spiritchat-data
```

## Verifying it actually works

The log is the only interface. Within a minute of starting, look for:

```
standing relay up — peer id 12D3Koo...
listening on /ip4/.../tcp/4001
peer connected: ...
announced as a standing relay in the DHT
```

`peer connected` is the one that matters: it means the node reached the
public DHT. Without it the node is running but alone, and the `dial FAILED`
lines above it say why — they are printed in full for the first twenty
failures precisely so that this case is diagnosable.

`announced as a standing relay in the DHT` means other nodes can now
discover it. It does not appear on a node with no peers.

If mining is on, `mined block at height N` follows. Expect a burst of these
at first on a young chain and then a settling to roughly one per five
minutes — difficulty starts low and ratchets up to meet whatever hash rate
it finds.
