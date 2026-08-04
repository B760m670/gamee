# Consent and moderation

Design for abuse handling in a serverless, end-to-end-encrypted, peer-to-peer
messenger. Status: **proposal, not implemented.** Written before any code
because a mistake in this layer is a protocol mistake, and protocol mistakes
are expensive to take back.

## 1. What this can and cannot do

Three properties of the system rule out most of what "moderation" usually
means, and no amount of engineering removes them:

**Identity is free.** An identity is a keypair. Banning one costs the banned
party two seconds. Any sanction attached to a raw key is a suggestion.

**Content is unreadable.** Nobody — not the network, not a relay, not the
project — can read a message. So nobody can verify what a report claims.

**Messages are deliberately repudiable.** In Double Ratchet and in MLS the
recipient can authenticate the sender but *cannot prove that authentication to
a third party*. This is a designed property: it means your messages can never
be shown to anyone else as your provable statements. It could be removed by
having senders sign plaintext, which would turn every message ever sent into
permanent evidence against its author. **This design does not do that**, and
no future version should without a very deliberate decision.

Together these mean a global tribunal is impossible. Worse, it is dangerous:
a system that converts unverifiable accusations into automatic sanctions is
not moderation, it is a brigading weapon — a handful of throwaway identities
sink anyone. The more automatic such a system is, the more effective the
weapon.

### Non-goals

- **Content moderation inside a conversation both parties consented to.** If
  you accepted someone and they say something vile, the remedy is to block and
  leave. This is equally true of Signal and WhatsApp; claiming otherwise here
  would be a lie told to users.
- **Global bans.** See above.
- **Any human reviewer, any hosted classifier, any paid service.**

### Goal

Handle the abuse that actually scales in a messenger — **unwanted contact**:
spam, mass solicitation, harassment by strangers, ban evasion by fresh
identities. Handle it with numbers only, adjudicated by each participant's own
device, with no authority anywhere.

## 2. The primitive: contact tickets

The insight this design rests on is that repudiability is **divisible**. What
must stay deniable is *what was said*. What need not be hidden is *that a
deliberate approach was made*.

Before a first message to a non-contact, the sender constructs:

```
base   = SHA-256( TICKET_DOMAIN ‖ sender_pubkey ‖ recipient_pubkey ‖ epoch )
work   = SHA-256( WORK_DOMAIN ‖ base ‖ nonce )      -- must have >= D leading zero bits
ticket = base(32) ‖ nonce(8) ‖ epoch(8) ‖ Ed25519_sender(SIG_DOMAIN ‖ base)(64)
```

- `epoch` is the same 24-hour period the mailbox tags already rotate on
  (`TAG_EPOCH_SECS`), big-endian.
- `D` is a difficulty in leading zero bits, chosen **by the recipient's
  device**, not globally agreed (§5).
- Domain-separation constants keep a ticket from ever being mistaken for a
  mailbox stamp, a prekey signature, or anything else this project signs.

What the object proves, and what it deliberately does not:

| | |
|---|---|
| Proves | key `A` spent work and *deliberately addressed* key `B` during epoch `E` |
| Does not reveal | a single byte of any message — the signature covers a hash of identities, never content |
| Unforgeable | only `A` holds `A`'s signing key |
| Non-transferable | `base` binds the recipient; a ticket for `B` is worthless against `C` |
| Expiring | bound to an epoch, so evidence ages out by construction |
| Canonical | exactly one `base` per (sender, recipient, epoch) — a recipient cannot manufacture several tickets from one approach |

Message deniability is untouched: nothing signed here says anything about what
was written.

## 3. Why this defeats the brigading problem

**To accuse `A`, you must exhibit a ticket signed by `A` and addressed to
you.** If `A` never approached you, there is nothing to exhibit, and you
cannot make one — it needs `A`'s key.

A farm of ten thousand fabricated identities cannot produce a single
accusation against someone who never contacted them. The right to accuse is
issued by the accused, through their own action. An attacker who wants to
frame `A` must first induce `A` to message many attacker-controlled
identities, and `A` controls that.

Note carefully what is being measured. Not "`A` is abusive" — unprovable, and
this design never claims it. What is measured is:

> `A` initiated contact with N distinct keys during epoch E, and M of them
> marked the approach unwanted.

That is the signature of spam and mass harassment, obtained without reading
anything.

## 4. Receipts

A recipient who blocks or reports a first contact may publish a **receipt**:

```
receipt = base(32) ‖ nonce(8) ‖ epoch(8) ‖ sender_pubkey(32) ‖ signature(64)
```

Any third party can check the signature against `sender_pubkey`, check the
proof of work from `base` and `nonce`, and check that the epoch is recent —
**without learning who the recipient was**, since `base` is a hash they cannot
invert. Distinct recipients yield distinct `base` values, so counting distinct
`base` per (sender, epoch) counts distinct complainants.

Publishing a receipt is always the recipient's explicit choice. Blocking
alone, the common case, stays entirely local and silent.

## 5. Adjudication

### 5.1 Subjective (the default, and the recommended whole of v1)

Each device computes its own view. There is no global truth and none is
sought: "unwanted **by you**" is a different claim from "unwanted", and only
the first is decidable.

Inputs available to a device: its own block list; receipts reaching it through
its own contact graph; whether the sender shares contacts with it; whether the
sender holds a `@username` (which costs proof-of-work to obtain, §7).

Output: the difficulty `D` this device demands of a stranger's next ticket,
and whether that stranger's first message lands in the inbox or in Requests.

Sybil resistance here is structural rather than assumed. A cluster of fake
identities influences a device only through edges it managed to establish into
the honest graph, and those edges require honest users to have added them.
The fakes can talk to each other all they like; it changes nothing.

### 5.2 Objective (a ledger rule — deferred, see §9)

Because tickets are unforgeable, aggregate counts *could* safely live on the
existing proof-of-work ledger, which ordinary reports never could. A rule of
the shape "if ≥N distinct receipts name `A` in epoch `E`, the baseline
difficulty for `A`'s first contacts rises network-wide, decaying over
subsequent epochs" is sound with respect to forgery.

It is **not** sound with respect to privacy, and §9.4 explains why. This layer
is deliberately not part of the first implementation.

## 6. The sanction ladder

Every step is automatic, proportionate, and reversible; weights decay by
epoch, so standing recovers on its own once approaches stop being unwanted.

1. Normal: ticket difficulty is trivial, message goes to the inbox.
2. Unknown sender, no signal: slightly higher difficulty, message goes to
   Requests rather than the inbox.
3. Negative signal in your graph: markedly higher difficulty; Requests only,
   with the reason shown.
4. Strong negative signal: first contact requires an introduction from a
   mutual contact.
5. You blocked them: nothing gets through, decided locally and absolutely.

There is no step that disables an account. That is both impossible (§1) and
wrong: sanctions here are a *price*, and prices are paid, not served.

The sanctioned party is told plainly what is happening — "messages to new
people are taking longer to send" — with the reason and the fact that it
recovers. A penalty nobody can see or understand is indistinguishable from a
bug.

## 7. Fit with what already exists

- **Hashcash stamps** (`p2p-core/src/mailbox.rs`) already implement exactly
  this shape of locally-checkable, consensus-free proof of work, at 20 bits.
  Tickets follow that precedent and should share its difficulty vocabulary.
- **`SendEnvelope`** currently carries no stamp and no sender binding; direct
  first contact is where tickets must be enforced. This is new protocol work.
- **MLS groups** do have a shared context, so member removal is real
  moderation and is worth building — separate from this document.
- **The `@username` ledger** gives an identity that cost real work. A key
  without a name is inherently cheap and can reasonably be treated with more
  caution; this is an input to §5.1, not a sanction of its own.

## 8. Parameters

| Parameter | Proposed | Reasoning |
|---|---|---|
| Epoch | 24 h (reuse `TAG_EPOCH_SECS`) | already the rotation period for mailbox tags |
| Base difficulty | 20 bits | matches the existing stamp; sub-second for one message |
| Decay half-life | 7 epochs | a bad week stops mattering within a month |
| Full expiry | 28 epochs | no permanent record, ever |

## 9. Threat model

**9.1 Sybil accusation.** Fabricate identities, accuse an honest user.
*Blocked by construction* — receipts require a signature from the accused
(§3).

**9.2 Sybil evasion.** Discard a sanctioned identity, make a fresh one.
Partially mitigated: a fresh key has no shared contacts and no `@username`, so
it starts at the cautious tier and pays a higher price per approach. The
cost is per-recipient and unavoidable, which is the point — evasion is
permitted but not free.

**9.3 Retaliatory receipts.** `A` messages `B` legitimately; `B` publishes a
receipt out of spite. One receipt is one data point, weighted by whether `B`
is in the observer's graph at all. Isolated receipts from strangers should
carry near-zero weight; this must be true in the scoring or the mechanism
becomes the abuse.

**9.4 Pair disclosure — the unsolved one.** A receipt names the sender in the
clear (verification needs the key). An observer who already knows `B`'s public
key can recompute `base` for any candidate sender and test it, learning that
`A` contacted `B`. Within a contact graph this is contained. Published
globally on the ledger it is a real leak of the social graph, and it is why
§5.2 is deferred. The principled fix is a zero-knowledge proof of possession
of a valid signature, publishing only `H(base)` — heavy, and future work. **We
should not ship the global layer before this is answered.**

**9.5 Ticket replay.** Non-issue: `base` binds the recipient, and the epoch
bounds the window.

**9.6 Difficulty as a denial-of-service.** A recipient could demand absurd
difficulty of everyone. This only silences their own inbox, so it is
self-limiting, but the UI must never let it happen by accident.

**9.7 Cost to honest low-power devices.** 20 bits is sub-second, and only on
*first* contact with someone new. Established conversations never pay.

## 10. Implementation phases

1. **Local consent.** Block and restrict, enforced in `ChatManager` before
   decryption; Requests as a real destination; the screens for both. No
   protocol change, and enough on its own to satisfy App Review 1.2.
2. **Tickets.** Format, mining, verification in `p2p-core`, with the tests
   the mailbox stamp already models. Enforced on first contact.
3. **Recipient-set difficulty.** §5.1 scoring, the sanction ladder, and the
   screen where a user can see their own standing.
4. **Receipts within the graph.** Blinded circulation, weighting, the "N of
   your contacts blocked this person" signal.
5. **Global layer.** Only if §9.4 gets a real answer.

## 11. Open decisions

1. **Global ledger layer.** Recommendation: **defer**, per §9.4. Subjective
   adjudication delivers nearly all of the practical protection with none of
   the privacy cost.
2. **Tickets for `@username`-initiated contact.** Recommendation: **yes,
   required**. Exempting it would make the name registry the obvious spam
   vector, and at 20 bits a human sending one message never notices.
3. **Decay.** Recommendation: half-life 7 epochs, full expiry 28 (§8).
