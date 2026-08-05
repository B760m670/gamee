import Foundation

/// Owns end-to-end-encrypted messaging for one `P2pSession`: establishing a
/// Double Ratchet session with a contact (via X3DH, the first time),
/// encrypting/sending, decrypting incoming envelopes, and retrying queued
/// messages once a contact reconnects. Created fresh alongside each
/// `P2pSession` (so it never outlives the identity/node it belongs to) and
/// fed every `FfiP2pEvent` from the same event pump that already drives
/// `P2pSession.encode` — JS never sees raw envelope bytes, only the
/// decrypted results this class emits through `emit`.
///
/// The crypto itself (`packages/crypto-core`) and the transport
/// (`packages/p2p-core`'s envelope/blob/mailbox/DHT primitives) are both
/// already fully built and tested; this class is only the glue between
/// them:
/// - A contact's current prekey bundle ("contact card") is normally
///   fetched the same way an avatar is — reusing the existing blob
///   protocol under a fixed, well-known blob id — but that needs a live
///   connection to them; if that fails (or the initial dial does), this
///   falls back to `resolveContactCard`'s DHT lookup, which a contact who
///   published their card while last online can still answer even while
///   currently offline (see `announceContactCard` in `init`).
/// - A conversation's first envelope carries both the X3DH `InitialMessage`
///   and the first ratchet ciphertext, framed as a 1-byte type tag (and,
///   for the first message, a 2-byte length prefix ahead of the initial
///   message bytes); every later envelope is just tagged ratchet
///   ciphertext. See `frameHandshake`/`frameContinuing` below.
/// - When a contact isn't directly reachable, an encrypted envelope is
///   deposited into the serverless Sphinx-mix mailbox instead of the
///   message just failing outright — see `depositContinuing`/
///   `sweepMailboxRetrieval`.
final class ChatManager {
  /// Which async step (if any) is outstanding for a given peer — guards
  /// against acting on a stray/duplicate event (e.g. an `EnvelopeDelivered`
  /// arriving when nothing was actually being sent to that peer) and
  /// against starting a second attempt while one is already in flight.
  private enum PeerSendState {
    case dialing
    case fetchingCard
    case resolvingCardViaDht
    case sendingEnvelope
    /// A group content envelope (see `GroupStore.OutboxItem`) is in
    /// flight to this peer — shares this peer's single slot with
    /// `sendingEnvelope` (never both at once) specifically because
    /// `P2pEvent.EnvelopeDelivered`/`EnvelopeDeliveryFailed` only carry
    /// `to: peerId`, not which envelope: at most one send may ever be
    /// outstanding to a given peer at a time, group or 1:1, or a
    /// delivery event could be attributed to the wrong one.
    case sendingGroupEnvelope
  }

  /// The blob id this device's current contact card is registered under —
  /// fixed and well-known (every SpiritChat node looks for the same id),
  /// unlike a content-addressed blob like an avatar. Reusing
  /// `Command::SetLocalBlob`/`FetchBlob` this way needs no `p2p-core`
  /// changes: an id is just opaque bytes to that layer either way.
  private static let contactCardBlobId = Data("spiritchat-contact-card-v1".utf8)

  private static let handshakeTag: UInt8 = 0x00
  private static let continuingTag: UInt8 = 0x01
  /// A group message: `[groupMessageTag][groupId: 16 bytes][Sender Key
  /// envelope]` — sent directly (never wrapped in a pairwise ratchet
  /// layer, unlike `handshakeTag`/`continuingTag`), since the Sender Key
  /// scheme's own AEAD+signature already provide this envelope's
  /// confidentiality and per-sender authenticity within the group. See
  /// `sendGroupMessage`'s own doc comment for why this is what makes
  /// Sender Keys cheaper than re-encrypting per-recipient the way 1:1
  /// messaging does.
  private static let groupMessageTag: UInt8 = 0x02

  /// A group *control* message's own inner tag, distinct from (and never
  /// compared against) the envelope-level tags above — these travel
  /// nested one level deeper, inside a pairwise ratchet message's own
  /// plaintext (see `groupControlAssociatedData`).
  private static let controlInviteTag: UInt8 = 0x01
  private static let controlDistributionTag: UInt8 = 0x02
  private static let controlMemberAddedTag: UInt8 = 0x03
  private static let controlMemberRemovedTag: UInt8 = 0x04
  // MLS group control (new groups). Distinct tags in the same inner
  // control-message namespace as the Sender Keys ones above, so both
  // schemes share one transport (pairwise ratchet sessions) without
  // colliding. See the "Groups (MLS)" section.
  private static let controlMlsInviteRequestTag: UInt8 = 0x05
  private static let controlMlsKeyPackageTag: UInt8 = 0x06
  private static let controlMlsWelcomeTag: UInt8 = 0x07
  private static let controlMlsCommitTag: UInt8 = 0x08

  /// Group control messages (an invite, or a member handing out their
  /// Sender Key chain — see `GroupStore`) travel through an *existing*
  /// pairwise ratchet session, exactly like an ordinary chat message,
  /// just under this associated data instead of empty. A receiver tries
  /// an ordinary chat decrypt first (empty AAD, the common case, checked
  /// first for the exact behavior every existing 1:1 message already
  /// has) and only falls back to this AAD if that fails — safe because a
  /// failed `FfiRatchet.decrypt` attempt never mutates the ratchet (see
  /// its own doc comment), so retrying against the same session bytes a
  /// second time is never a double-spend of any ratchet state.
  private static let groupControlAssociatedData = Data("spiritchat-group-control-v1".utf8)

  /// How often `sweepMailboxRetrieval` runs — same order of magnitude as
  /// the mix's own dummy-traffic interval (`MIX_DUMMY_TRAFFIC_INTERVAL` in
  /// `node.rs`), so a retrieval query blends into traffic that's already
  /// happening on this schedule rather than standing out as its own signal.
  private static let retrievalSweepInterval: TimeInterval = 30

  /// How often this device re-publishes its own contact card into the
  /// DHT (`reannounceContactCard`) — a DHT record needs refreshing so it
  /// doesn't expire, but there's no reason to hammer the network with an
  /// unchanged card anywhere near as often as the mailbox retrieval sweep.
  private static let contactCardAnnounceInterval: TimeInterval = 30 * 60

  private let lock = NSLock()
  private var peerStates: [String: PeerSendState] = [:]
  private var connectedPeers: Set<String> = []
  /// Mix relays this node currently knows of, from `mixRelayDiscovered`.
  /// Guarded by `lock`.
  ///
  /// Consulted before every send: with none of these, a deposit into the
  /// mix has nowhere to go, and `DeliveryPolicy` decides whether that
  /// means "wait" or "go direct and accept the disclosure".
  private var knownMixRelays: Set<String> = []
  /// In-flight media downloads, keyed by an internal download id (see the
  /// "Media receive" section). Guarded by `lock`.
  private var mediaDownloads: [String: MediaDownload] = [:]
  /// chunk content-id (hex) -> the downloads waiting on that chunk, so a
  /// `blobFetched` event routes to whoever needs it. Guarded by `lock`.
  private var mediaChunkWaiters: [String: [(String, Int)]] = [:]
  private var retrievalTimer: Timer?
  private var contactCardAnnounceTimer: Timer?

  private let slot: Int
  private let node: FfiP2pNode
  private let identity: FfiIdentity
  private let agreement: FfiAgreementKey
  private let prekeys: FfiPrekeyStore

  /// This account's own decisions about who it will accept. Consulted on the
  /// inbound path before decryption and on the outbound path before sending —
  /// see `ConsentStore` for why it lives here rather than in JS. Exposed so
  /// the module's block/unblock functions and the settings screen can reach
  /// the same instance the message path uses; two copies could disagree.
  let consent: ConsentStore

  /// Computed exactly once, in `init` — `FfiPrekeyStore.contactCard`
  /// hands out a fresh one-time prekey on every call ("generate a new
  /// card per contact"), so `reannounceContactCard` must re-publish this
  /// same stored value rather than minting (and burning through the
  /// finite one-time-prekey pool for) a new one every 30 minutes just to
  /// refresh an unchanged DHT record.
  private let contactCard: Data

  /// Set by whoever creates this instance (`SpiritchatCryptoCoreModule`) to
  /// forward events to JS via `sendEvent("onChatEvent", ...)` — kept as a
  /// closure rather than a hard dependency on `ExpoModulesCore` so this
  /// class stays a plain Swift object, the same way `MiningController`
  /// doesn't know about the module that gates it either.
  var emit: ([String: Any?]) -> Void = { _ in }

  init(slot: Int, node: FfiP2pNode, identity: FfiIdentity, agreement: FfiAgreementKey, prekeys: FfiPrekeyStore) {
    self.slot = slot
    self.node = node
    self.identity = identity
    self.agreement = agreement
    self.prekeys = prekeys
    self.consent = ConsentStore(slot: slot)

    let card = prekeys.contactCard(identity: identity, agreement: agreement)
    self.contactCard = card
    try? node.setLocalBlob(id: Self.contactCardBlobId, bytes: card)
    // Also published into the DHT, not just served over a live
    // connection — see `announceContactCard`'s own doc comment for why
    // that's what makes a *first* message to this device possible even
    // while it's the one currently offline.
    try? node.announceContactCard(ownerIdentityPublicKey: identity.publicKeyBytes(), card: card)

    // Anything left over from a previous run (app killed mid-send, or the
    // recipient was offline) gets another chance now — the same dial/
    // fetch/send path a brand new `sendMessage`/`sendGroupMessage` call
    // would go through.
    let pendingPeers = Set(ChatStore.loadOutbox(slot: slot).map(\.peerId))
      .union(GroupStore.loadOutbox(slot: slot).map(\.memberPeerId))
    for peerId in pendingPeers {
      attemptSend(peerId: peerId)
    }

    startRetrievalSweep()
    startContactCardAnnounceSweep()
  }

  deinit {
    retrievalTimer?.invalidate()
    contactCardAnnounceTimer?.invalidate()
  }

  // MARK: - Sending

  /// Queues `plaintext` for `peerId` and starts trying to deliver it
  /// immediately — returns right away regardless of connectivity (the
  /// message is durable on disk the moment this returns), so sending never
  /// blocks on the network. Returns a local id the caller can use to match
  /// up the `messageSent`/`messageFailed` event this send eventually fires.
  @discardableResult
  func sendMessage(peerId: String, peerPublicKey: Data, plaintext: Data) -> String {
    let item = ChatStore.OutboxItem(
      localId: UUID().uuidString,
      peerId: peerId,
      peerPublicKey: peerPublicKey,
      plaintext: plaintext,
      createdAt: Date().timeIntervalSince1970
    )
    var outbox = ChatStore.loadOutbox(slot: slot)
    outbox.append(item)
    try? ChatStore.saveOutbox(outbox, slot: slot)

    attemptSend(peerId: peerId)
    return item.localId
  }

  private func beginState(_ state: PeerSendState, for peerId: String) -> Bool {
    lock.lock()
    defer { lock.unlock() }
    guard peerStates[peerId] == nil else { return false }
    peerStates[peerId] = state
    return true
  }

  private func transitionState(_ state: PeerSendState, for peerId: String) {
    lock.lock()
    peerStates[peerId] = state
    lock.unlock()
  }

  private func endState(for peerId: String) {
    lock.lock()
    peerStates.removeValue(forKey: peerId)
    lock.unlock()
  }

  private func state(for peerId: String) -> PeerSendState? {
    lock.lock()
    defer { lock.unlock() }
    return peerStates[peerId]
  }

  private func isConnected(_ peerId: String) -> Bool {
    lock.lock()
    defer { lock.unlock() }
    return connectedPeers.contains(peerId)
  }

  /// The oldest still-queued message for `peerId` — outbox items are
  /// processed one at a time per peer (never two envelopes in flight to
  /// the same peer at once), which is what makes correlating a bare
  /// `EnvelopeDelivered { to: peerId }` back to a specific message
  /// unambiguous without this crate needing its own per-message id.
  private func firstOutboxItem(_ peerId: String) -> ChatStore.OutboxItem? {
    ChatStore.loadOutbox(slot: slot).first(where: { $0.peerId == peerId })
  }

  private func removeOutboxItem(localId: String) {
    var outbox = ChatStore.loadOutbox(slot: slot)
    outbox.removeAll(where: { $0.localId == localId })
    try? ChatStore.saveOutbox(outbox, slot: slot)
  }

  /// Drops every queued 1:1 envelope for `peerId` — used when that peer is
  /// blocked, so nothing already in the durable outbox is delivered later.
  private func discardOutbox(for peerId: String) {
    var outbox = ChatStore.loadOutbox(slot: slot)
    let before = outbox.count
    outbox.removeAll(where: { $0.peerId == peerId })
    guard outbox.count != before else { return }
    try? ChatStore.saveOutbox(outbox, slot: slot)
  }

  /// The oldest still-queued group envelope for `peerId` — same "one at
  /// a time per peer" reasoning as `firstOutboxItem`, and sharing that
  /// same peer's `peerStates` slot (see `PeerSendState.sendingGroupEnvelope`).
  private func firstGroupOutboxItem(_ peerId: String) -> GroupStore.OutboxItem? {
    GroupStore.loadOutbox(slot: slot).first(where: { $0.memberPeerId == peerId })
  }

  private func removeGroupOutboxItem(localId: String) {
    var outbox = GroupStore.loadOutbox(slot: slot)
    outbox.removeAll(where: { $0.localId == localId })
    try? GroupStore.saveOutbox(outbox, slot: slot)
  }

  /// Advances `peerId`'s outbox by exactly one step: dial if not
  /// connected, fetch a contact card if connected but no session exists
  /// yet, or encrypt-and-send if a session already exists. A no-op if
  /// nothing is queued for `peerId`, or a step is already outstanding
  /// (`peerStates`) — the event handlers below drive it forward from here.
  /// Falls through to `attemptGroupDelivery` once the 1:1 outbox is empty
  /// — a 1:1 message queued for `peerId` is always tried first, mirroring
  /// how this method already prioritized itself over everything else
  /// before group messages existed.
  private func attemptSend(peerId: String) {
    // Blocking cuts both directions. Enforced here rather than only at the
    // send call, because the outbox is durable and retried: a message queued
    // before the block, or one already waiting for the peer to come online,
    // must not slip out afterwards. Anything still queued for a blocked peer
    // is dropped rather than held, since it will never be sendable while the
    // block stands and keeping it would silently deliver it on unblock.
    if consent.isBlocked(peerId) {
      discardOutbox(for: peerId)
      return
    }
    guard let item = firstOutboxItem(peerId) else {
      attemptGroupDelivery(peerId: peerId)
      return
    }

    // Route before reachability, deliberately. Dialing the recipient is
    // what discloses the pair to the network, so whether that is allowed
    // has to be settled before a connection is opened — not after a mix
    // attempt has already failed, by which point the dial has happened.
    let route = DeliveryPolicy.route(mixPathAvailable: hasMixPath())

    if let session = ChatStore.loadSession(slot: slot, peerId: peerId) {
      switch route {
      case .mix:
        guard beginState(.sendingEnvelope, for: peerId) else { return }
        depositContinuing(item: item, session: session)
        return
      case .direct:
        guard isConnected(peerId) else {
          guard beginState(.dialing, for: peerId) else { return }
          try? node.dial(peerId: peerId, knownAddresses: [])
          return
        }
        guard beginState(.sendingEnvelope, for: peerId) else { return }
        sendContinuing(item: item, session: session)
        return
      case nil:
        // No unlinkable route and no permission to use a linkable one.
        // The item stays in the durable outbox and is retried when a mix
        // relay turns up (see `mixRelayDiscovered`) — this is a wait, not
        // a failure, and it is reported as such.
        emit(failedEvent(item: item, reason: "queued — no private route available yet"))
        return
      }
    }

    // No ratchet session yet: the contact card has to come first, and the
    // only way to get one without dialing the contact is the DHT — the
    // `fetchBlob` path below serves the same card but needs a direct
    // connection to them, which is the disclosure being avoided.
    switch route {
    case nil:
      // Nothing to start: the card would arrive with no private way to
      // use it. Waits for a mix relay, same as the session case above.
      emit(failedEvent(item: item, reason: "queued — no private route available yet"))
      return

    case .mix:
      guard beginState(.resolvingCardViaDht, for: peerId) else { return }
      do {
        try node.resolveContactCard(ownerIdentityPublicKey: item.peerPublicKey)
      } catch {
        endState(for: peerId)
        emit(failedEvent(item: item, reason: "\(error)"))
      }
      return

    case .direct:
      // Falls through to the dial-and-fetch path below.
      break
    }

    guard isConnected(peerId) else {
      guard beginState(.dialing, for: peerId) else { return }
      try? node.dial(peerId: peerId, knownAddresses: [])
      return
    }

    guard beginState(.fetchingCard, for: peerId) else { return }
    do {
      try node.fetchBlob(peerId: peerId, id: Self.contactCardBlobId)
    } catch {
      endState(for: peerId)
      emit(failedEvent(item: item, reason: "\(error)"))
    }
  }

  /// Whether a deposit into the mix has anywhere to go right now.
  private func hasMixPath() -> Bool {
    lock.lock()
    defer { lock.unlock() }
    return !knownMixRelays.isEmpty
  }

  /// Re-runs delivery for every peer with something queued. Used when a
  /// mix relay appears, since items held back for want of a private route
  /// have no other trigger — no dial fails, no connection changes, nothing
  /// else would ever wake them.
  private func retryQueuedForAllPeers() {
    let pendingPeers = Set(ChatStore.loadOutbox(slot: slot).map(\.peerId))
      .union(GroupStore.loadOutbox(slot: slot).map(\.memberPeerId))
    for peerId in pendingPeers {
      attemptSend(peerId: peerId)
    }
  }

  /// The group-content counterpart of `attemptSend`'s dial/send steps —
  /// no contact-card/X3DH step exists here, since a group member is
  /// required to already have an established pairwise session (see the
  /// "MARK: - Groups" doc comment). A no-op if nothing is queued for
  /// `peerId`, or `peerId`'s single send slot is already taken (by a 1:1
  /// send or an earlier group envelope).
  private func attemptGroupDelivery(peerId: String) {
    guard firstGroupOutboxItem(peerId) != nil else { return }

    guard isConnected(peerId) else {
      guard beginState(.dialing, for: peerId) else { return }
      try? node.dial(peerId: peerId, knownAddresses: [])
      return
    }

    guard let item = firstGroupOutboxItem(peerId), beginState(.sendingGroupEnvelope, for: peerId) else { return }
    do {
      try node.sendEnvelope(peerId: peerId, bytes: item.wireEnvelope)
      // Stays `.sendingGroupEnvelope` until `envelopeDelivered`/`envelopeDeliveryFailed`.
    } catch {
      endState(for: peerId)
    }
  }

  private func sendContinuing(item: ChatStore.OutboxItem, session: ChatStore.Session) {
    do {
      let ratchet = try FfiRatchet.fromBytes(bytes: session.ratchetBytes)
      let ciphertext = try ratchet.encrypt(plaintext: item.plaintext, associatedData: Data())
      let envelope = Self.frameContinuing(ciphertext)
      try ChatStore.saveSession(
        ChatStore.Session(peerId: item.peerId, peerPublicKey: session.peerPublicKey, ratchetBytes: ratchet.toBytes()),
        slot: slot
      )
      try node.sendEnvelope(peerId: item.peerId, bytes: envelope)
      // Stays `.sendingEnvelope` until `envelopeDelivered`/`envelopeDeliveryFailed`.
    } catch {
      endState(for: item.peerId)
      emit(failedEvent(item: item, reason: "\(error)"))
    }
  }

  /// The value `depositToMailbox`/`retrieveFromMailbox` both key on for a
  /// given contact — derived from both identities' long-term public keys,
  /// so it's computable independently by either side without needing an
  /// established ratchet session first (unlike the ratchet itself, which
  /// only exists once a handshake has actually completed).
  private func mailboxSharedMaterial(peerPublicKey: Data) -> Data {
    p2pMailboxSharedMaterial(ownIdentityPublicKey: identity.publicKeyBytes(), peerIdentityPublicKey: peerPublicKey)
  }

  /// Falls back here when a direct send couldn't be delivered (the peer
  /// went offline mid-flight, or wasn't reachable this attempt) but a
  /// ratchet session already exists — deposits the same kind of encrypted
  /// envelope into the serverless mailbox mix instead of just giving up,
  /// so the recipient's own periodic sweep (`sweepMailboxRetrieval`) can
  /// pick it up whenever they're next online. Successful *dispatch* ends
  /// this attempt the same way a hard failure would: the mix gives no
  /// per-item delivery receipt, and the item stays queued either way, so a
  /// future reconnect still retries a direct send too.
  private func depositContinuing(item: ChatStore.OutboxItem, session: ChatStore.Session) {
    defer { endState(for: item.peerId) }
    do {
      let ratchet = try FfiRatchet.fromBytes(bytes: session.ratchetBytes)
      let ciphertext = try ratchet.encrypt(plaintext: item.plaintext, associatedData: Data())
      let envelope = Self.frameContinuing(ciphertext)
      try ChatStore.saveSession(
        ChatStore.Session(peerId: item.peerId, peerPublicKey: session.peerPublicKey, ratchetBytes: ratchet.toBytes()),
        slot: slot
      )
      let sharedMaterial = mailboxSharedMaterial(peerPublicKey: session.peerPublicKey)
      try node.depositToMailbox(sharedMaterial: sharedMaterial, envelope: envelope)
      // Handed to the mix, and the outbox entry goes with it.
      //
      // This used to report a failure and keep the item queued, which was
      // right while a deposit was the fallback after a direct send had
      // already failed: the item stayed so a later reconnect could retry
      // directly. Under `DeliveryPolicy` the mix is the *primary* route, so
      // both halves of that would now be wrong — every message would sit
      // permanently marked "queued" even though it was dispatched, and the
      // retained entry would be re-deposited on every reconnect, delivering
      // the same message repeatedly.
      //
      // "Sent" here means handed to the mix, not confirmed read: the mix
      // deliberately returns no per-item receipt (one would identify the
      // pair, which is the entire thing being hidden). The deposit remains
      // retrievable until the recipient's sweep collects it or the mailbox
      // retention window expires — ordinary store-and-forward semantics.
      removeOutboxItem(localId: item.localId)
      emit(sentEvent(item: item))
    } catch {
      emit(failedEvent(item: item, reason: "\(error)"))
    }
  }

  /// What `handleCardFetched` does with the freshly-framed first envelope
  /// once X3DH and the ratchet are set up: send it immediately (the card
  /// came from a direct `fetchBlob`, meaning the peer is reachable right
  /// now) or deposit it into the mailbox (the card came from
  /// `resolveContactCard`'s DHT fallback, meaning it very much isn't) —
  /// every other step is identical either way.
  private enum FirstEnvelopeDelivery {
    case sendDirectly
    case depositToMailbox
  }

  /// Runs once this device has `peerId`'s contact card, however it got
  /// it: verifies it (`FfiContactCard.parse` already checks every
  /// signature), runs X3DH against it, bootstraps the initiator side of a
  /// Double Ratchet session, and either sends or deposits the first
  /// (combined) envelope per `delivery`.
  private func handleCardFetched(peerId: String, cardBytes: Data, delivery: FirstEnvelopeDelivery) {
    guard let item = firstOutboxItem(peerId) else {
      endState(for: peerId)
      return
    }
    guard let card = try? FfiContactCard.parse(bytes: cardBytes) else {
      endState(for: peerId)
      emit(failedEvent(item: item, reason: "Не удалось прочитать карточку собеседника"))
      return
    }
    do {
      let handshake = try x3dhInitiate(identity: identity, agreement: agreement, card: card)
      let ratchet = try FfiRatchet.initInitiator(
        sharedSecretBytes: handshake.sharedSecret,
        remoteRatchetPublicBytes: card.signedPrekeyPublicBytes()
      )
      let ciphertext = try ratchet.encrypt(plaintext: item.plaintext, associatedData: Data())
      let envelope = Self.frameHandshake(initialMessage: handshake.initialMessage, ratchetMessage: ciphertext)
      try ChatStore.saveSession(
        ChatStore.Session(peerId: peerId, peerPublicKey: item.peerPublicKey, ratchetBytes: ratchet.toBytes()),
        slot: slot
      )
      switch delivery {
      case .sendDirectly:
        transitionState(.sendingEnvelope, for: peerId)
        try node.sendEnvelope(peerId: peerId, bytes: envelope)
      case .depositToMailbox:
        try node.depositToMailbox(sharedMaterial: mailboxSharedMaterial(peerPublicKey: item.peerPublicKey), envelope: envelope)
        endState(for: peerId)
        emit(failedEvent(item: item, reason: "queued for offline delivery"))
      }
    } catch {
      endState(for: peerId)
      emit(failedEvent(item: item, reason: "\(error)"))
    }
  }

  // MARK: - Groups (Sender Keys)
  //
  // Every group member encrypts once, under their own Sender Key chain,
  // for every other member to decrypt — cheaper than 1:1 messaging's
  // per-recipient re-encryption, at the cost of everyone in the group
  // sharing the AEAD key, which is why every message also carries a
  // signature (checked inside `spiritchat_crypto_core::sender_key`
  // itself, not here) proving *which* member actually sent it. See that
  // module's own doc comment for the scheme in full.
  //
  // Every member (named at creation, or added later via `addGroupMember`)
  // must already be an existing 1:1 contact — a group control message
  // piggybacks on an *existing* pairwise ratchet session, it never
  // triggers first-contact/X3DH establishment the way an ordinary
  // `sendMessage` does. Removing a member (`removeGroupMember`) rotates
  // this device's own chain and every remaining member independently
  // rotates its own too on hearing about it — forward secrecy against
  // the removed member never depends on trusting whoever initiated the
  // removal to have done it right.
  //
  // Group *content* gets the same durable, retried-on-reconnect delivery
  // 1:1 messages do (`GroupStore.OutboxItem`, `attemptGroupDelivery`).
  // Group *control* messages (invites, chain handouts, membership
  // changes) remain best-effort (direct send, falling back to a single
  // mailbox deposit attempt, no persistent queue of their own) — a member
  // unreachable through both avenues at the moment one of these is sent
  // simply misses it until a later interaction gives another chance to
  // catch up.

  /// Creates a new group named `name` with `memberPeerIds` as its initial
  /// (non-self) members — every one of them must already have an
  /// established pairwise session (see this section's own doc comment).
  /// Returns the new group's id, or `nil` if this device's own identity
  /// isn't available yet or the group couldn't be persisted.
  @discardableResult
  func createGroup(name: String, memberPeerIds: [String]) -> String? {
    // New groups are always MLS/TreeKEM (see the "Groups (MLS)" section);
    // existing Sender Keys groups keep working through the legacy paths,
    // which every method below routes to via `session.isMls`.
    return createGroupMls(name: name, memberPeerIds: memberPeerIds)
  }

  /// Encrypts `plaintext` once under this device's own chain for `groupId`
  /// and queues the same ciphertext for delivery to every other member —
  /// durable the instant this returns (one `GroupStore.OutboxItem` per
  /// member, see `attemptGroupDelivery`), the same "queued before
  /// anything is sent" guarantee `sendMessage` already gives 1:1 chats.
  /// Returns a local id shared by every member's queued item; `nil` if
  /// this device isn't (or is no longer) a member of `groupId`.
  @discardableResult
  func sendGroupMessage(groupId: String, plaintext: Data) -> String? {
    guard let session = GroupStore.loadSession(slot: slot, groupId: groupId) else { return nil }
    if session.isMls {
      return sendGroupMessageMls(groupId: groupId, plaintext: plaintext)
    }
    guard var session = GroupStore.loadSession(slot: slot, groupId: groupId),
          let groupIdBytes = Self.data(fromHex: groupId),
          let ownStateBytes = session.ownSenderKeyStateBytes,
          let ownState = try? FfiSenderKeyState.fromBytes(bytes: ownStateBytes)
    else { return nil }

    guard let signedEnvelope = try? ownState.encrypt(plaintext: plaintext, associatedData: groupIdBytes) else {
      return nil
    }
    session.ownSenderKeyStateBytes = ownState.toBytes()
    try? GroupStore.saveSession(session, slot: slot)

    let localId = UUID().uuidString
    let wireEnvelope = Self.frameGroupMessage(groupId: groupIdBytes, signedEnvelope: signedEnvelope)
    let createdAt = Date().timeIntervalSince1970
    var outbox = GroupStore.loadOutbox(slot: slot)
    for member in session.members {
      outbox.append(GroupStore.OutboxItem(localId: localId, groupId: groupId, memberPeerId: member, wireEnvelope: wireEnvelope, createdAt: createdAt))
    }
    try? GroupStore.saveOutbox(outbox, slot: slot)

    for member in session.members {
      attemptSend(peerId: member)
    }
    return localId
  }

  /// Falls back here when a direct send to a group member couldn't be
  /// delivered but a pairwise session with them exists — deposits the
  /// same envelope into their mailbox queue (the same per-pair queue
  /// `depositContinuing` already uses for 1:1 messages, distinguished on
  /// retrieval purely by this envelope's own leading tag byte) instead of
  /// giving up. The item stays queued either way (mirrors
  /// `depositContinuing`'s own reasoning exactly): a future reconnect to
  /// this member still retries a direct send too.
  private func depositGroupItemToMailbox(_ item: GroupStore.OutboxItem) {
    guard let pairwiseSession = ChatStore.loadSession(slot: slot, peerId: item.memberPeerId) else {
      NSLog("[ChatManager] no pairwise session with group member \(item.memberPeerId) — cannot deliver a group message to them right now")
      return
    }
    try? node.depositToMailbox(sharedMaterial: mailboxSharedMaterial(peerPublicKey: pairwiseSession.peerPublicKey), envelope: item.wireEnvelope)
  }

  /// Encrypts `payload` (an invite or a member distribution) under the
  /// *existing* pairwise ratchet session with `peerId`, using
  /// `groupControlAssociatedData` so the receiving end can tell it apart
  /// from an ordinary chat message. Returns `false` (and does nothing
  /// else) if no such session exists yet — see this section's own v1
  /// scope note on why this never triggers first-contact establishment.
  @discardableResult
  private func sendGroupControlMessageBestEffort(peerId: String, payload: Data) -> Bool {
    guard let session = ChatStore.loadSession(slot: slot, peerId: peerId) else {
      NSLog("[ChatManager] no pairwise session with \(peerId) yet — add them as a contact before adding them to a group")
      return false
    }
    do {
      let ratchet = try FfiRatchet.fromBytes(bytes: session.ratchetBytes)
      let ciphertext = try ratchet.encrypt(plaintext: payload, associatedData: Self.groupControlAssociatedData)
      let envelope = Self.frameContinuing(ciphertext)
      try ChatStore.saveSession(
        ChatStore.Session(peerId: peerId, peerPublicKey: session.peerPublicKey, ratchetBytes: ratchet.toBytes()),
        slot: slot
      )
      if isConnected(peerId) {
        try node.sendEnvelope(peerId: peerId, bytes: envelope)
      } else {
        try node.depositToMailbox(sharedMaterial: mailboxSharedMaterial(peerPublicKey: session.peerPublicKey), envelope: envelope)
      }
      return true
    } catch {
      NSLog("[ChatManager] failed to deliver a group control message to \(peerId): \(error)")
      return false
    }
  }

  /// A group invite (`Self.controlInviteTag`) or someone's chain handout
  /// (`Self.controlDistributionTag`) — dispatched here once
  /// `handleIncomingGroupControl`/`handleRetrievedContinuing` have already
  /// confirmed `payload` decrypted successfully under
  /// `groupControlAssociatedData`.
  private func dispatchGroupControlPayload(fromPeerId: String, payload: Data) {
    guard let tag = payload.first else { return }
    let body = Data(payload.dropFirst())
    switch tag {
    case Self.controlInviteTag:
      guard let (groupId, name, members, distribution) = Self.parseGroupInvite(body) else { return }
      handleGroupInvite(fromPeerId: fromPeerId, groupIdBytes: groupId, name: name, allMembers: members, distributionBytes: distribution)
    case Self.controlDistributionTag:
      guard let (groupId, distribution) = Self.parseGroupMemberDistribution(body) else { return }
      handleGroupMemberDistribution(fromPeerId: fromPeerId, groupIdBytes: groupId, distributionBytes: distribution)
    case Self.controlMemberAddedTag:
      guard let (groupId, newMemberPeerId) = Self.parseGroupMembershipChange(body) else { return }
      handleGroupMemberAdded(fromPeerId: fromPeerId, groupIdBytes: groupId, newMemberPeerId: newMemberPeerId)
    case Self.controlMemberRemovedTag:
      guard let (groupId, removedMemberPeerId) = Self.parseGroupMembershipChange(body) else { return }
      handleGroupMemberRemoved(fromPeerId: fromPeerId, groupIdBytes: groupId, removedMemberPeerId: removedMemberPeerId)
    case Self.controlMlsInviteRequestTag:
      guard let (groupId, name, members) = Self.parseMlsInviteRequest(body) else { return }
      handleMlsInviteRequest(fromPeerId: fromPeerId, groupIdBytes: groupId, name: name, memberPeerIds: members)
    case Self.controlMlsKeyPackageTag:
      guard let (groupId, keyPackage) = Self.parseMlsBlob(body) else { return }
      handleMlsKeyPackage(fromPeerId: fromPeerId, groupIdBytes: groupId, keyPackageBytes: keyPackage)
    case Self.controlMlsWelcomeTag:
      guard let (groupId, welcome) = Self.parseMlsBlob(body) else { return }
      handleMlsWelcome(fromPeerId: fromPeerId, groupIdBytes: groupId, welcomeBytes: welcome)
    case Self.controlMlsCommitTag:
      guard let (groupId, commit) = Self.parseMlsBlob(body) else { return }
      handleMlsCommit(fromPeerId: fromPeerId, groupIdBytes: groupId, commitBytes: commit)
    default:
      NSLog("[ChatManager] unrecognized group control message tag \(tag) from \(fromPeerId) — dropped")
    }
  }

  /// `fromPeerId` invited this device into a new group. If this device
  /// already knows `groupId` (a duplicate/retried invite), this is
  /// treated exactly like an ordinary member distribution instead of
  /// re-creating the group from scratch. Otherwise: records the
  /// inviter's own distribution, generates this device's own fresh chain
  /// for the group, and hands that chain out to every other named member
  /// (including the inviter) the same best-effort way `createGroup` does.
  private func handleGroupInvite(fromPeerId: String, groupIdBytes: Data, name: String, allMembers: [String], distributionBytes: Data) {
    let groupId = Self.hexString(groupIdBytes)
    guard GroupStore.loadSession(slot: slot, groupId: groupId) == nil else {
      handleGroupMemberDistribution(fromPeerId: fromPeerId, groupIdBytes: groupIdBytes, distributionBytes: distributionBytes)
      return
    }
    guard let myPeerId = try? node.localPeerId(),
          let creatorReceiverState = try? FfiSenderKeyReceiverState.fromDistributionBytes(bytes: distributionBytes)
    else { return }

    let otherMembers = allMembers.filter { $0 != myPeerId }
    let ownState = FfiSenderKeyState.generate()
    let session = GroupStore.Session(
      groupId: groupId, name: name, members: otherMembers,
      mlsStateBytes: nil,
      ownSenderKeyStateBytes: ownState.toBytes(),
      receiverStates: [fromPeerId: creatorReceiverState.toBytes()]
    )
    guard (try? GroupStore.saveSession(session, slot: slot)) != nil else { return }
    emit(groupInvitedEvent(groupId: groupId, name: name, members: otherMembers))

    let myDistribution = ownState.toDistributionBytes()
    for member in otherMembers {
      sendGroupControlMessageBestEffort(
        peerId: member,
        payload: Self.frameGroupMemberDistribution(groupId: groupIdBytes, distribution: myDistribution)
      )
    }
  }

  /// `fromPeerId` handed out their current Sender Key chain for a group
  /// this device already knows about — recorded so a later
  /// `handleIncomingGroupMessage` from them can actually decrypt. Silently
  /// dropped if this device doesn't recognize `groupId` at all (an invite
  /// must have been lost, or arrived out of order) — there's nothing
  /// meaningful to attach this distribution to yet.
  private func handleGroupMemberDistribution(fromPeerId: String, groupIdBytes: Data, distributionBytes: Data) {
    let groupId = Self.hexString(groupIdBytes)
    guard var session = GroupStore.loadSession(slot: slot, groupId: groupId) else {
      NSLog("[ChatManager] a member distribution for unknown group \(groupId) from \(fromPeerId) — dropped")
      return
    }
    guard let receiverState = try? FfiSenderKeyReceiverState.fromDistributionBytes(bytes: distributionBytes) else { return }
    var receiverStates = session.receiverStates ?? [:]
    receiverStates[fromPeerId] = receiverState.toBytes()
    session.receiverStates = receiverStates
    try? GroupStore.saveSession(session, slot: slot)
  }

  /// Adds `newMemberPeerId` (who must already be an existing 1:1 contact,
  /// same requirement as `createGroup`) to `groupId`. Sends them an
  /// ordinary group invite carrying the full, now-updated roster (so they
  /// know to reach every other member too — no different from being
  /// invited at creation time from their perspective), and tells every
  /// other current member to also welcome the new one
  /// (`handleGroupMemberAdded`). No chain rotation needed for an add: a
  /// new member only ever receives each existing chain's *current*
  /// position onward (see `SenderKeyState`'s own doc comment), so nothing
  /// about the past is exposed by adding someone new.
  func addGroupMember(groupId: String, newMemberPeerId: String) {
    if let session = GroupStore.loadSession(slot: slot, groupId: groupId), session.isMls {
      addGroupMemberMls(groupId: groupId, newMemberPeerId: newMemberPeerId)
      return
    }
    guard var session = GroupStore.loadSession(slot: slot, groupId: groupId),
          let groupIdBytes = Self.data(fromHex: groupId),
          let myPeerId = try? node.localPeerId(),
          let ownStateBytes = session.ownSenderKeyStateBytes,
          let ownState = try? FfiSenderKeyState.fromBytes(bytes: ownStateBytes),
          !session.members.contains(newMemberPeerId)
    else { return }

    let previousMembers = session.members
    session.members.append(newMemberPeerId)
    guard (try? GroupStore.saveSession(session, slot: slot)) != nil else { return }
    emit(groupMemberAddedEvent(groupId: groupId, memberPeerId: newMemberPeerId))

    let allMembers = previousMembers + [newMemberPeerId, myPeerId]
    sendGroupControlMessageBestEffort(
      peerId: newMemberPeerId,
      payload: Self.frameGroupInvite(groupId: groupIdBytes, name: session.name, members: allMembers, distribution: ownState.toDistributionBytes())
    )
    for member in previousMembers {
      sendGroupControlMessageBestEffort(peerId: member, payload: Self.frameMemberAdded(groupId: groupIdBytes, newMemberPeerId: newMemberPeerId))
    }
  }

  /// `fromPeerId` (an existing member) reports that `newMemberPeerId` has
  /// joined `groupId` — adds them locally and sends them this device's
  /// own current chain directly, the same way any other member's
  /// distribution already reaches a newly-invited member.
  private func handleGroupMemberAdded(fromPeerId: String, groupIdBytes: Data, newMemberPeerId: String) {
    let groupId = Self.hexString(groupIdBytes)
    guard var session = GroupStore.loadSession(slot: slot, groupId: groupId), !session.members.contains(newMemberPeerId),
          let ownStateBytes = session.ownSenderKeyStateBytes,
          let ownState = try? FfiSenderKeyState.fromBytes(bytes: ownStateBytes)
    else { return }
    session.members.append(newMemberPeerId)
    guard (try? GroupStore.saveSession(session, slot: slot)) != nil else { return }
    emit(groupMemberAddedEvent(groupId: groupId, memberPeerId: newMemberPeerId))
    sendGroupControlMessageBestEffort(
      peerId: newMemberPeerId,
      payload: Self.frameGroupMemberDistribution(groupId: groupIdBytes, distribution: ownState.toDistributionBytes())
    )
  }

  /// Removes `memberToRemove` from `groupId` and rotates this device's
  /// own chain (a fresh `FfiSenderKeyState`, replacing the old one
  /// outright) before redistributing it to whoever remains — the
  /// removed member still holds the *old* chain key, and without
  /// rotating, could keep ratcheting it forward on their own to decrypt
  /// every future message despite no longer being sent anything directly.
  /// Notifies every remaining member (`handleGroupMemberRemoved`), each of
  /// which independently rotates its own chain the same way on receipt —
  /// forward secrecy against the removed member must not depend on
  /// trusting whoever initiated the removal to have done it right.
  func removeGroupMember(groupId: String, memberToRemove: String) {
    if let session = GroupStore.loadSession(slot: slot, groupId: groupId), session.isMls {
      removeGroupMemberMls(groupId: groupId, memberToRemove: memberToRemove)
      return
    }
    guard var session = GroupStore.loadSession(slot: slot, groupId: groupId),
          let groupIdBytes = Self.data(fromHex: groupId),
          session.members.contains(memberToRemove)
    else { return }

    session.members.removeAll { $0 == memberToRemove }
    session.receiverStates?.removeValue(forKey: memberToRemove)
    let freshState = FfiSenderKeyState.generate()
    session.ownSenderKeyStateBytes = freshState.toBytes()
    guard (try? GroupStore.saveSession(session, slot: slot)) != nil else { return }
    emit(groupMemberRemovedEvent(groupId: groupId, memberPeerId: memberToRemove))

    let distribution = freshState.toDistributionBytes()
    for member in session.members {
      sendGroupControlMessageBestEffort(peerId: member, payload: Self.frameGroupMemberDistribution(groupId: groupIdBytes, distribution: distribution))
      sendGroupControlMessageBestEffort(peerId: member, payload: Self.frameMemberRemoved(groupId: groupIdBytes, removedMemberPeerId: memberToRemove))
    }
  }

  /// `fromPeerId` reports that `removedMemberPeerId` is no longer in
  /// `groupId` — removes them locally and rotates this device's own
  /// chain too, redistributing to whoever's left, mirroring
  /// `removeGroupMember`'s own reasoning exactly (this device's forward
  /// secrecy against the removed member doesn't depend on `fromPeerId`
  /// having rotated correctly, only on this device doing its own part).
  private func handleGroupMemberRemoved(fromPeerId: String, groupIdBytes: Data, removedMemberPeerId: String) {
    let groupId = Self.hexString(groupIdBytes)
    guard var session = GroupStore.loadSession(slot: slot, groupId: groupId), session.members.contains(removedMemberPeerId) else { return }

    session.members.removeAll { $0 == removedMemberPeerId }
    session.receiverStates?.removeValue(forKey: removedMemberPeerId)
    let freshState = FfiSenderKeyState.generate()
    session.ownSenderKeyStateBytes = freshState.toBytes()
    guard (try? GroupStore.saveSession(session, slot: slot)) != nil else { return }
    emit(groupMemberRemovedEvent(groupId: groupId, memberPeerId: removedMemberPeerId))

    let distribution = freshState.toDistributionBytes()
    for member in session.members {
      sendGroupControlMessageBestEffort(peerId: member, payload: Self.frameGroupMemberDistribution(groupId: groupIdBytes, distribution: distribution))
    }
  }

  /// A group content envelope (`Self.groupMessageTag`) — arrived either
  /// directly (`onEnvelopeReceived`) or via the mailbox
  /// (`handleMailboxEnvelopeRetrieved`); unlike the 1:1 case, both paths
  /// share one handler here, since neither ever needs the transport-level
  /// sender: the envelope itself never names one (that's the whole point
  /// of Sender Keys' signature living *inside* the encrypted content, not
  /// on the wire), so every known member's receiver state is tried in
  /// turn regardless of how this arrived — mirrors
  /// `handleRetrievedContinuing`'s own "try every candidate" shape.
  private func handleIncomingGroupMessage(_ bytes: Data) {
    guard bytes.count >= 16 else { return }
    let groupIdBytes = Data(bytes.prefix(16))
    let signedEnvelope = Data(bytes.dropFirst(16))
    let groupId = Self.hexString(groupIdBytes)
    guard let session = GroupStore.loadSession(slot: slot, groupId: groupId) else {
      NSLog("[ChatManager] a group message for unknown group \(groupId) — dropped")
      return
    }

    if session.isMls {
      handleIncomingGroupMessageMls(groupId: groupId, session: session, wire: signedEnvelope)
      return
    }

    for (memberPeerId, receiverStateBytes) in (session.receiverStates ?? [:]) {
      guard let receiverState = try? FfiSenderKeyReceiverState.fromBytes(bytes: receiverStateBytes) else { continue }
      guard let plaintext = try? receiverState.decrypt(message: signedEnvelope, associatedData: groupIdBytes) else { continue }
      var updated = session
      updated.receiverStates?[memberPeerId] = receiverState.toBytes()
      try? GroupStore.saveSession(updated, slot: slot)
      deliverIncomingGroup(groupId: groupId, senderPeerId: memberPeerId, plaintext: plaintext)
      return
    }
    NSLog("[ChatManager] a group message for \(groupId) matched no known member's chain — dropped")
  }

  // MARK: - Groups (MLS/TreeKEM)
  //
  // New groups use MLS/TreeKEM (`spiritchat_mls_core` via `FfiMlsGroup`)
  // instead of Sender Keys: O(log N) membership changes (a removal is one
  // logarithmic commit, not everyone re-keying with everyone), post-
  // compromise security (a self-update re-randomizes the epoch), and
  // cryptographic agreement on the roster (the transcript-bound
  // confirmation tag). Existing Sender Keys groups keep working through
  // the legacy methods above; `session.isMls` is what every entry point
  // routes on.
  //
  // MLS needs a joiner's *key package* (a signed leaf public key) before
  // they can be added — a step Sender Keys didn't have. This is carried
  // over the same pairwise ratchet sessions the Sender Keys control
  // messages already used, so no new transport: a committer sends an
  // invite request, the invitee replies with a key package, the committer
  // commits the Add and returns a Welcome (to the joiner) plus a Commit
  // (broadcast to existing members). Every member still has to be a 1:1
  // contact first, exactly as before.

  private var identitySeed: Data { identity.secretBytes() }

  /// A roster leaf's identity public key resolved to a peer id, or nil for
  /// a blank leaf (empty identity bytes).
  private func mlsPeerId(forIdentity identityBytes: Data) -> String? {
    guard !identityBytes.isEmpty else { return nil }
    return try? p2pPeerIdFromPublicKey(publicKey: identityBytes)
  }

  /// Every *other* current member's peer id, from an MLS group's roster —
  /// what content and commit fan-out address.
  private func mlsMemberPeerIds(_ group: FfiMlsGroup, myPeerId: String) -> [String] {
    group.roster().compactMap { mlsPeerId(forIdentity: $0) }.filter { $0 != myPeerId }
  }

  /// The leaf a given peer id occupies in a roster, or nil if absent.
  private func mlsLeaf(forPeerId peerId: String, roster: [Data]) -> Int? {
    for (leaf, identityBytes) in roster.enumerated() {
      if let candidate = mlsPeerId(forIdentity: identityBytes), candidate == peerId { return leaf }
    }
    return nil
  }

  @discardableResult
  private func createGroupMls(name: String, memberPeerIds: [String]) -> String? {
    guard let myPeerId = try? node.localPeerId() else { return nil }
    let groupIdBytes = Data((0..<16).map { _ in UInt8.random(in: 0...255) })
    let groupId = Self.hexString(groupIdBytes)

    guard let group = try? FfiMlsGroup.create(groupId: groupIdBytes, identitySeed: identitySeed) else { return nil }
    let session = GroupStore.Session(
      groupId: groupId, name: name, members: memberPeerIds,
      mlsStateBytes: group.toBytes(), ownSenderKeyStateBytes: nil, receiverStates: nil
    )
    guard (try? GroupStore.saveSession(session, slot: slot)) != nil else { return nil }

    // Ask each initial member for a key package; their reply drives an
    // Add commit (see handleMlsKeyPackage). The roster list includes this
    // device so a joiner knows to reach it too.
    let roster = memberPeerIds + [myPeerId]
    for member in memberPeerIds {
      sendGroupControlMessageBestEffort(peerId: member, payload: Self.frameMlsInviteRequest(groupId: groupIdBytes, name: name, members: roster))
    }
    return groupId
  }

  @discardableResult
  private func sendGroupMessageMls(groupId: String, plaintext: Data) -> String? {
    guard var session = GroupStore.loadSession(slot: slot, groupId: groupId),
          let groupIdBytes = Self.data(fromHex: groupId),
          let stateBytes = session.mlsStateBytes,
          let group = try? FfiMlsGroup.fromBytes(bytes: stateBytes)
    else { return nil }

    let wire = group.encryptMessage(plaintext: plaintext)
    session.mlsStateBytes = group.toBytes()
    try? GroupStore.saveSession(session, slot: slot)

    let localId = UUID().uuidString
    let wireEnvelope = Self.frameGroupMessage(groupId: groupIdBytes, signedEnvelope: wire)
    let createdAt = Date().timeIntervalSince1970
    var outbox = GroupStore.loadOutbox(slot: slot)
    for member in session.members {
      outbox.append(GroupStore.OutboxItem(localId: localId, groupId: groupId, memberPeerId: member, wireEnvelope: wireEnvelope, createdAt: createdAt))
    }
    try? GroupStore.saveOutbox(outbox, slot: slot)
    for member in session.members { attemptSend(peerId: member) }
    return localId
  }

  private func handleIncomingGroupMessageMls(groupId: String, session: GroupStore.Session, wire: Data) {
    guard let stateBytes = session.mlsStateBytes,
          let group = try? FfiMlsGroup.fromBytes(bytes: stateBytes)
    else { return }
    guard let message = try? group.decryptMessage(wire: wire) else {
      NSLog("[ChatManager] an MLS group message for \(groupId) failed to decrypt — dropped")
      return
    }
    // decrypt_message doesn't advance persisted state, so nothing to save.
    let roster = group.roster()
    let senderLeaf = Int(message.senderLeaf)
    guard senderLeaf < roster.count, let senderPeerId = mlsPeerId(forIdentity: roster[senderLeaf]) else { return }
    deliverIncomingGroup(groupId: groupId, senderPeerId: senderPeerId, plaintext: message.plaintext)
  }

  private func addGroupMemberMls(groupId: String, newMemberPeerId: String) {
    guard let session = GroupStore.loadSession(slot: slot, groupId: groupId),
          let groupIdBytes = Self.data(fromHex: groupId),
          let myPeerId = try? node.localPeerId(),
          !session.members.contains(newMemberPeerId)
    else { return }
    // Same shape as creation: ask for a key package; the reply drives the
    // Add commit + Welcome in handleMlsKeyPackage.
    let roster = session.members + [newMemberPeerId, myPeerId]
    sendGroupControlMessageBestEffort(peerId: newMemberPeerId, payload: Self.frameMlsInviteRequest(groupId: groupIdBytes, name: session.name, members: roster))
  }

  private func removeGroupMemberMls(groupId: String, memberToRemove: String) {
    guard var session = GroupStore.loadSession(slot: slot, groupId: groupId),
          let groupIdBytes = Self.data(fromHex: groupId),
          let stateBytes = session.mlsStateBytes,
          let group = try? FfiMlsGroup.fromBytes(bytes: stateBytes),
          let myPeerId = try? node.localPeerId()
    else { return }
    guard let leaf = mlsLeaf(forPeerId: memberToRemove, roster: group.roster()) else { return }
    guard let output = try? group.commit(addKeyPackages: [], removeLeaves: [UInt32(leaf)]) else { return }

    session.mlsStateBytes = group.toBytes()
    session.members = mlsMemberPeerIds(group, myPeerId: myPeerId)
    try? GroupStore.saveSession(session, slot: slot)

    // One logarithmic commit to everyone still in the group — the O(log N)
    // removal that motivated MLS. The removed member simply can't process
    // it (they hold no key the new epoch's path was sealed to).
    for member in session.members {
      sendGroupControlMessageBestEffort(peerId: member, payload: Self.frameMlsBlob(tag: Self.controlMlsCommitTag, groupId: groupIdBytes, blob: output.commitBytes))
    }
    emit(groupMemberRemovedEvent(groupId: groupId, memberPeerId: memberToRemove))
  }

  /// A member asked this device for a key package to add it to a group.
  /// Generates a leaf key, remembers its secret (needed to open the
  /// forthcoming Welcome), and replies. Ignored if this device is already
  /// in the group (a duplicate request).
  private func handleMlsInviteRequest(fromPeerId: String, groupIdBytes: Data, name: String, memberPeerIds: [String]) {
    let groupId = Self.hexString(groupIdBytes)
    guard GroupStore.loadSession(slot: slot, groupId: groupId) == nil else { return }

    let leaf = mlsGenerateLeafKey()
    guard let keyPackage = try? mlsKeyPackage(identitySeed: identitySeed, leafPublic: leaf.publicKey) else { return }
    let pending = GroupStore.PendingJoin(groupId: groupId, name: name, leafSecret: leaf.secret)
    try? GroupStore.savePendingJoin(pending, slot: slot)
    sendGroupControlMessageBestEffort(peerId: fromPeerId, payload: Self.frameMlsBlob(tag: Self.controlMlsKeyPackageTag, groupId: groupIdBytes, blob: keyPackage))
  }

  /// A member sent this device (the committer) their key package. Commits
  /// the Add, sends them the Welcome, and broadcasts the Commit to every
  /// existing member.
  private func handleMlsKeyPackage(fromPeerId: String, groupIdBytes: Data, keyPackageBytes: Data) {
    let groupId = Self.hexString(groupIdBytes)
    guard var session = GroupStore.loadSession(slot: slot, groupId: groupId), session.isMls,
          let stateBytes = session.mlsStateBytes,
          let group = try? FfiMlsGroup.fromBytes(bytes: stateBytes),
          let myPeerId = try? node.localPeerId()
    else { return }

    guard let output = try? group.commit(addKeyPackages: [keyPackageBytes], removeLeaves: []) else {
      NSLog("[ChatManager] failed to commit an MLS add for \(groupId) — dropped")
      return
    }
    session.mlsStateBytes = group.toBytes()
    session.members = mlsMemberPeerIds(group, myPeerId: myPeerId)
    try? GroupStore.saveSession(session, slot: slot)

    for welcome in output.welcomes {
      sendGroupControlMessageBestEffort(peerId: fromPeerId, payload: Self.frameMlsBlob(tag: Self.controlMlsWelcomeTag, groupId: groupIdBytes, blob: welcome.welcomeBytes))
    }
    // Every already-joined member (not the new one) processes the commit.
    for member in session.members where member != fromPeerId {
      sendGroupControlMessageBestEffort(peerId: member, payload: Self.frameMlsBlob(tag: Self.controlMlsCommitTag, groupId: groupIdBytes, blob: output.commitBytes))
    }
    emit(groupMemberAddedEvent(groupId: groupId, memberPeerId: fromPeerId))
  }

  /// This device was welcomed into an MLS group it earlier sent a key
  /// package for. Joins using the leaf secret stashed at invite time.
  private func handleMlsWelcome(fromPeerId: String, groupIdBytes: Data, welcomeBytes: Data) {
    let groupId = Self.hexString(groupIdBytes)
    guard GroupStore.loadSession(slot: slot, groupId: groupId) == nil else { return }
    guard let pending = GroupStore.loadPendingJoin(slot: slot, groupId: groupId),
          let myPeerId = try? node.localPeerId(),
          let group = try? FfiMlsGroup.join(welcomeBytes: welcomeBytes, identitySeed: identitySeed, leafSecret: pending.leafSecret)
    else { return }

    let members = mlsMemberPeerIds(group, myPeerId: myPeerId)
    let session = GroupStore.Session(
      groupId: groupId, name: pending.name, members: members,
      mlsStateBytes: group.toBytes(), ownSenderKeyStateBytes: nil, receiverStates: nil
    )
    guard (try? GroupStore.saveSession(session, slot: slot)) != nil else { return }
    GroupStore.deletePendingJoin(slot: slot, groupId: groupId)
    emit(groupInvitedEvent(groupId: groupId, name: pending.name, members: members))
  }

  /// An existing member applies a commit (an add or removal someone else
  /// committed). Emits roster diffs so the UI membership stays live.
  private func handleMlsCommit(fromPeerId: String, groupIdBytes: Data, commitBytes: Data) {
    let groupId = Self.hexString(groupIdBytes)
    guard var session = GroupStore.loadSession(slot: slot, groupId: groupId), session.isMls,
          let stateBytes = session.mlsStateBytes,
          let group = try? FfiMlsGroup.fromBytes(bytes: stateBytes),
          let myPeerId = try? node.localPeerId()
    else { return }

    let before = Set(session.members)
    guard (try? group.processCommit(commitBytes: commitBytes)) != nil else {
      NSLog("[ChatManager] failed to process an MLS commit for \(groupId) — dropped")
      return
    }
    session.mlsStateBytes = group.toBytes()
    let after = mlsMemberPeerIds(group, myPeerId: myPeerId)
    session.members = after
    try? GroupStore.saveSession(session, slot: slot)

    let afterSet = Set(after)
    for added in afterSet.subtracting(before) { emit(groupMemberAddedEvent(groupId: groupId, memberPeerId: added)) }
    for removed in before.subtracting(afterSet) { emit(groupMemberRemovedEvent(groupId: groupId, memberPeerId: removed)) }
  }

  // MARK: - Media messages (photo / video / voice)
  //
  // A media message is an ordinary end-to-end chat message whose plaintext
  // is not text but a `mediaFrameMagic`-tagged manifest (see
  // `spiritchat_crypto_core::media` + `FfiMediaManifest`): the small
  // manifest travels encrypted over the same ratchet/MLS channel as any
  // message, while the media bytes travel separately as content-addressed
  // blobs — one blob per encrypted chunk, so a large video reuses the
  // existing blob protocol unchanged and never loads whole into memory.
  // The magic prefix begins with a NUL byte, which valid UTF-8 chat text
  // never starts with, so text messages stay byte-for-byte unchanged and
  // the receive side (next) tells the two apart unambiguously.
  //
  // This section is the send half: encrypt the file chunk by chunk,
  // register each encrypted chunk as a local blob, and send the manifest.
  // The receive half (fetching the chunks and reassembling) and the native
  // recorder / UI land alongside it.

  /// Prefix marking a decrypted plaintext as a media manifest rather than
  /// text. Leading NUL guarantees it can never collide with real text.
  private static let mediaFrameMagic = Data([0x00]) + Data("SCMEDIA1".utf8)

  private static func frameMediaPlaintext(_ manifestBytes: Data) -> Data {
    mediaFrameMagic + manifestBytes
  }

  /// Encrypts `mediaData` under a fresh per-file key, registers each
  /// encrypted chunk as a content-addressed local blob so peers can fetch
  /// them, and returns the framed manifest plaintext to send as a message.
  /// `nil` only if blob registration fails outright.
  private func registerMediaAndFrame(
    mediaData: Data, mime: String, filename: String?, durationMs: UInt32?, thumbnail: Data?
  ) -> Data? {
    let keyBytes = mediaGenerateKey()
    let chunkSize = Int(mediaChunkSize())
    // At least one (possibly empty) chunk, so an empty file still has a
    // final chunk and can't be forged as "no chunks".
    let chunkCount = max(1, (mediaData.count + chunkSize - 1) / chunkSize)
    var chunkIds: [Data] = []
    chunkIds.reserveCapacity(chunkCount)

    for i in 0..<chunkCount {
      let start = i * chunkSize
      let end = min(start + chunkSize, mediaData.count)
      let plainChunk = start < end ? mediaData.subdata(in: start..<end) : Data()
      let isLast = i == chunkCount - 1
      guard let ciphertext = try? mediaEncryptChunk(key: keyBytes, chunkIndex: UInt32(i), isLast: isLast, plaintext: plainChunk) else {
        return nil
      }
      let id = blobContentId(bytes: ciphertext)
      do {
        try node.setLocalBlob(id: id, bytes: ciphertext)
      } catch {
        NSLog("[ChatManager] failed to register a media chunk blob: \(error)")
        return nil
      }
      chunkIds.append(id)
    }

    let manifest = FfiMediaManifest(
      key: keyBytes,
      mime: mime,
      totalSize: UInt64(mediaData.count),
      chunkIds: chunkIds,
      filename: filename,
      durationMs: durationMs,
      thumbnail: thumbnail
    )
    return Self.frameMediaPlaintext(mediaManifestEncode(manifest: manifest))
  }

  /// Sends a media file to a 1:1 peer — mirrors `sendMessage`, only the
  /// plaintext is a framed media manifest instead of text.
  @discardableResult
  func sendMediaMessage(
    peerId: String, peerPublicKey: Data, mediaData: Data,
    mime: String, filename: String?, durationMs: UInt32?, thumbnail: Data?
  ) -> String? {
    guard let framed = registerMediaAndFrame(mediaData: mediaData, mime: mime, filename: filename, durationMs: durationMs, thumbnail: thumbnail) else {
      return nil
    }
    return sendMessage(peerId: peerId, peerPublicKey: peerPublicKey, plaintext: framed)
  }

  /// Sends a media file to a group — registers the same content-addressed
  /// chunk blobs, then rides the existing group send path.
  @discardableResult
  func sendGroupMediaMessage(
    groupId: String, mediaData: Data, mime: String, filename: String?, durationMs: UInt32?, thumbnail: Data?
  ) -> String? {
    guard let framed = registerMediaAndFrame(mediaData: mediaData, mime: mime, filename: filename, durationMs: durationMs, thumbnail: thumbnail) else {
      return nil
    }
    return sendGroupMessage(groupId: groupId, plaintext: framed)
  }

  // --- Media receive ----------------------------------------------------

  /// An in-flight media download: the manifest plus the chunk plaintexts
  /// as they arrive, and where to attribute the finished media.
  private struct MediaDownload {
    let senderPeerId: String
    let groupId: String?
    let manifest: FfiMediaManifest
    var chunks: [Data?]
  }

  /// Whether a decrypted plaintext is a media manifest rather than text.
  private static func isMediaFrame(_ plaintext: Data) -> Bool {
    plaintext.starts(with: mediaFrameMagic)
  }

  /// The single point every decrypted 1:1 plaintext flows through — text
  /// emits as before, a media frame kicks off a chunk download instead.
  private func deliverIncoming(peerId: String, peerFingerprint: String, peerPublicKey: Data, plaintext: Data) {
    if Self.isMediaFrame(plaintext) {
      handleIncomingMedia(fetchFromPeerId: peerId, senderPeerId: peerId, groupId: nil, plaintext: plaintext)
    } else {
      emit(receivedEvent(peerId: peerId, peerFingerprint: peerFingerprint, peerPublicKey: peerPublicKey, plaintext: plaintext))
    }
  }

  /// The same, for group messages.
  private func deliverIncomingGroup(groupId: String, senderPeerId: String, plaintext: Data) {
    if Self.isMediaFrame(plaintext) {
      handleIncomingMedia(fetchFromPeerId: senderPeerId, senderPeerId: senderPeerId, groupId: groupId, plaintext: plaintext)
    } else {
      emit(groupMessageReceivedEvent(groupId: groupId, senderPeerId: senderPeerId, plaintext: plaintext))
    }
  }

  /// Parses a media manifest and starts fetching its chunk blobs from the
  /// sender. Each chunk arrives as a `blobFetched` event routed through
  /// `handleMediaBlobFetched`; once all are in, the file is reassembled
  /// and a `mediaReceived` event fires.
  private func handleIncomingMedia(fetchFromPeerId: String, senderPeerId: String, groupId: String?, plaintext: Data) {
    let manifestBytes = Data(plaintext.dropFirst(Self.mediaFrameMagic.count))
    guard let manifest = try? mediaManifestDecode(bytes: manifestBytes), !manifest.chunkIds.isEmpty else {
      NSLog("[ChatManager] a media message with an unreadable manifest — dropped")
      return
    }

    let downloadId = UUID().uuidString
    let download = MediaDownload(
      senderPeerId: senderPeerId, groupId: groupId, manifest: manifest,
      chunks: Array(repeating: nil, count: manifest.chunkIds.count)
    )
    lock.lock()
    mediaDownloads[downloadId] = download
    for (index, id) in manifest.chunkIds.enumerated() {
      let idHex = Self.hexString(id)
      mediaChunkWaiters[idHex, default: []].append((downloadId, index))
    }
    lock.unlock()

    // Fetch every chunk. A blob already cached locally (e.g. media this
    // device also holds) still resolves via the same blobFetched path.
    for id in manifest.chunkIds {
      do {
        try node.fetchBlob(peerId: fetchFromPeerId, id: id)
      } catch {
        NSLog("[ChatManager] failed to request a media chunk from \(fetchFromPeerId): \(error)")
      }
    }
  }

  /// Routes a fetched blob into any waiting media download. Returns true
  /// if the blob was a media chunk (so the generic blob handler skips it).
  private func handleMediaBlobFetched(id: Data, bytes: Data) -> Bool {
    let idHex = Self.hexString(id)
    lock.lock()
    guard let waiters = mediaChunkWaiters[idHex] else { lock.unlock(); return false }
    mediaChunkWaiters.removeValue(forKey: idHex)
    var completed: [MediaDownload] = []
    for (downloadId, index) in waiters {
      guard var download = mediaDownloads[downloadId] else { continue }
      let isLast = index == download.manifest.chunkIds.count - 1
      guard let plain = try? mediaDecryptChunk(key: download.manifest.key, chunkIndex: UInt32(index), isLast: isLast, ciphertext: bytes) else {
        // A chunk that won't decrypt means a corrupt or wrong blob — drop
        // the whole download rather than deliver a half-broken file.
        mediaDownloads.removeValue(forKey: downloadId)
        continue
      }
      download.chunks[index] = plain
      if download.chunks.allSatisfy({ $0 != nil }) {
        mediaDownloads.removeValue(forKey: downloadId)
        completed.append(download)
      } else {
        mediaDownloads[downloadId] = download
      }
    }
    lock.unlock()

    for download in completed { finishMediaDownload(download) }
    return true
  }

  /// A media chunk fetch failed outright — abandon its download (the
  /// message simply doesn't render its media until re-sent/re-tried).
  private func handleMediaBlobFailed(id: Data) -> Bool {
    let idHex = Self.hexString(id)
    lock.lock()
    guard let waiters = mediaChunkWaiters[idHex] else { lock.unlock(); return false }
    mediaChunkWaiters.removeValue(forKey: idHex)
    for (downloadId, _) in waiters { mediaDownloads.removeValue(forKey: downloadId) }
    lock.unlock()
    NSLog("[ChatManager] a media chunk fetch failed — media download abandoned")
    return true
  }

  /// Reassembles a completed download to a local file and emits it.
  private func finishMediaDownload(_ download: MediaDownload) {
    var data = Data()
    for chunk in download.chunks { data.append(chunk ?? Data()) }

    let ext = Self.fileExtension(forMime: download.manifest.mime)
    let dir = Self.mediaDirectory(slot: slot)
    let localName = "\(UUID().uuidString).\(ext)"
    let url = dir.appendingPathComponent(localName)
    do {
      try data.write(to: url, options: .atomic)
    } catch {
      NSLog("[ChatManager] failed to write received media: \(error)")
      return
    }
    emit(mediaReceivedEvent(download: download, localPath: url.absoluteString))
  }

  private static func mediaDirectory(slot: Int) -> URL {
    let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
    let dir = base
      .appendingPathComponent("Chats", isDirectory: true)
      .appendingPathComponent("\(slot)", isDirectory: true)
      .appendingPathComponent("Media", isDirectory: true)
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir
  }

  private static func fileExtension(forMime mime: String) -> String {
    switch mime {
    case "audio/opus", "audio/ogg": return "opus"
    case "audio/mp4", "audio/aac", "audio/m4a": return "m4a"
    case "image/jpeg": return "jpg"
    case "image/png": return "png"
    case "image/gif": return "gif"
    case "video/mp4": return "mp4"
    case "video/quicktime": return "mov"
    default: return "bin"
    }
  }

  private func mediaReceivedEvent(download: MediaDownload, localPath: String) -> [String: Any?] {
    [
      "type": "mediaReceived",
      "peerId": download.senderPeerId,
      "groupId": download.groupId,
      "localPath": localPath,
      "mime": download.manifest.mime,
      "filename": download.manifest.filename,
      "durationMs": download.manifest.durationMs.map { Int($0) },
      "totalSize": Int(download.manifest.totalSize),
      "at": Int(Date().timeIntervalSince1970 * 1000),
    ]
  }

  // MARK: - Receiving

  private func onEnvelopeReceived(fromPeerId: String, bytes: Data) {
    // Before the type tag, before any parsing, before any decryption: this
    // is the whole point of keeping consent native-side. A blocked peer's
    // envelope is never opened, so nothing it contains — not a message, not
    // a media manifest, not a group invite — can reach the UI or the
    // notification layer. Ratchet state is left untouched too, which matters:
    // decrypting would advance it, so dropping here also means a blocked
    // peer cannot make this device do cryptographic work on demand.
    if consent.isBlocked(fromPeerId) { return }
    guard let tag = bytes.first else { return }
    let body = bytes.dropFirst()
    switch tag {
    case Self.handshakeTag:
      handleIncomingHandshake(fromPeerId: fromPeerId, body: Data(body))
    case Self.continuingTag:
      handleIncomingContinuing(fromPeerId: fromPeerId, body: Data(body))
    case Self.groupMessageTag:
      handleIncomingGroupMessage(Data(body))
    default:
      NSLog("[ChatManager] envelope from \(fromPeerId) has an unrecognized type tag \(tag) — dropped")
    }
  }

  /// A conversation's first incoming envelope: an X3DH `InitialMessage`
  /// (length-prefixed) followed by the first ratchet ciphertext. This is
  /// also the only place this device learns a new contact's identity
  /// public key from *them* rather than having already had it (e.g. from a
  /// ledger username lookup) — `x3dh_respond` reads it out of the initial
  /// message for exactly this reason.
  private func handleIncomingHandshake(fromPeerId: String, body: Data) {
    guard body.count >= 2 else { return }
    let length = Int(body[body.startIndex]) << 8 | Int(body[body.startIndex + 1])
    let initialMessageStart = body.index(body.startIndex, offsetBy: 2)
    guard body.distance(from: initialMessageStart, to: body.endIndex) >= length else { return }
    let initialMessageEnd = body.index(initialMessageStart, offsetBy: length)
    let initialMessageBytes = Data(body[initialMessageStart..<initialMessageEnd])
    let ratchetMessageBytes = Data(body[initialMessageEnd...])

    do {
      let response = try x3dhRespond(agreement: agreement, prekeys: prekeys, initialMessageBytes: initialMessageBytes)
      let ratchet = try FfiRatchet.initResponder(
        sharedSecretBytes: response.sharedSecret,
        myRatchetSecretBytes: prekeys.signedPrekeySecretBytes()
      )
      let plaintext = try ratchet.decrypt(message: ratchetMessageBytes, associatedData: Data())
      try ChatStore.saveSession(
        ChatStore.Session(peerId: fromPeerId, peerPublicKey: response.initiatorIdentityBytes, ratchetBytes: ratchet.toBytes()),
        slot: slot
      )
      let fingerprint = (try? identityFingerprintOfPublicKey(publicKey: response.initiatorIdentityBytes)) ?? ""
      deliverIncoming(peerId: fromPeerId, peerFingerprint: fingerprint, peerPublicKey: response.initiatorIdentityBytes, plaintext: plaintext)
    } catch {
      NSLog("[ChatManager] failed to accept a handshake from \(fromPeerId): \(error)")
    }
  }

  private func handleIncomingContinuing(fromPeerId: String, body: Data) {
    guard let session = ChatStore.loadSession(slot: slot, peerId: fromPeerId) else {
      NSLog("[ChatManager] a continuing envelope arrived from \(fromPeerId) with no local session — dropped")
      return
    }
    do {
      let ratchet = try FfiRatchet.fromBytes(bytes: session.ratchetBytes)
      let plaintext = try ratchet.decrypt(message: body, associatedData: Data())
      try ChatStore.saveSession(
        ChatStore.Session(peerId: fromPeerId, peerPublicKey: session.peerPublicKey, ratchetBytes: ratchet.toBytes()),
        slot: slot
      )
      let fingerprint = (try? identityFingerprintOfPublicKey(publicKey: session.peerPublicKey)) ?? ""
      deliverIncoming(peerId: fromPeerId, peerFingerprint: fingerprint, peerPublicKey: session.peerPublicKey, plaintext: plaintext)
    } catch {
      // Might actually be a group control message sent through this same
      // session instead — see `groupControlAssociatedData`'s own doc
      // comment for why retrying here is always safe.
      handleIncomingGroupControl(fromPeerId: fromPeerId, session: session, body: body)
    }
  }

  /// A failed ordinary decrypt in `handleIncomingContinuing` falls back
  /// here — tries the same session once more under
  /// `groupControlAssociatedData` before giving up entirely.
  private func handleIncomingGroupControl(fromPeerId: String, session: ChatStore.Session, body: Data) {
    guard let ratchet = try? FfiRatchet.fromBytes(bytes: session.ratchetBytes),
          let payload = try? ratchet.decrypt(message: body, associatedData: Self.groupControlAssociatedData)
    else {
      NSLog("[ChatManager] failed to decrypt an envelope from \(fromPeerId) as either a chat message or a group control message")
      return
    }
    try? ChatStore.saveSession(
      ChatStore.Session(peerId: fromPeerId, peerPublicKey: session.peerPublicKey, ratchetBytes: ratchet.toBytes()),
      slot: slot
    )
    dispatchGroupControlPayload(fromPeerId: fromPeerId, payload: payload)
  }

  /// A mailbox-retrieved envelope carries no attached sender — Sphinx
  /// delivery hides the true origin from the P2P layer by design, the same
  /// unlinkability property that makes offline delivery through the mix
  /// safe in the first place. Dispatch mirrors `onEnvelopeReceived`'s own
  /// tag byte, but each sub-handler has to *discover or guess* the sender
  /// instead of already knowing it.
  private func handleMailboxEnvelopeRetrieved(_ envelope: Data) {
    guard let tag = envelope.first else { return }
    let body = Data(envelope.dropFirst())
    switch tag {
    case Self.handshakeTag:
      handleRetrievedHandshake(body: body)
    case Self.continuingTag:
      handleRetrievedContinuing(body: body)
    case Self.groupMessageTag:
      handleIncomingGroupMessage(body)
    default:
      NSLog("[ChatManager] a mailbox-retrieved envelope has an unrecognized type tag \(tag) — dropped")
    }
  }

  /// A handshake retrieved from the mailbox: same framing as
  /// `handleIncomingHandshake`, but the sender's PeerId has to be derived
  /// from the X3DH response's own `initiatorIdentityBytes` rather than
  /// being handed one directly by the transport.
  private func handleRetrievedHandshake(body: Data) {
    guard body.count >= 2 else { return }
    let length = Int(body[body.startIndex]) << 8 | Int(body[body.startIndex + 1])
    let initialMessageStart = body.index(body.startIndex, offsetBy: 2)
    guard body.distance(from: initialMessageStart, to: body.endIndex) >= length else { return }
    let initialMessageEnd = body.index(initialMessageStart, offsetBy: length)
    let initialMessageBytes = Data(body[initialMessageStart..<initialMessageEnd])
    let ratchetMessageBytes = Data(body[initialMessageEnd...])

    do {
      let response = try x3dhRespond(agreement: agreement, prekeys: prekeys, initialMessageBytes: initialMessageBytes)
      let peerId = try p2pPeerIdFromPublicKey(publicKey: response.initiatorIdentityBytes)
      let ratchet = try FfiRatchet.initResponder(
        sharedSecretBytes: response.sharedSecret,
        myRatchetSecretBytes: prekeys.signedPrekeySecretBytes()
      )
      let plaintext = try ratchet.decrypt(message: ratchetMessageBytes, associatedData: Data())
      try ChatStore.saveSession(
        ChatStore.Session(peerId: peerId, peerPublicKey: response.initiatorIdentityBytes, ratchetBytes: ratchet.toBytes()),
        slot: slot
      )
      let fingerprint = (try? identityFingerprintOfPublicKey(publicKey: response.initiatorIdentityBytes)) ?? ""
      deliverIncoming(peerId: peerId, peerFingerprint: fingerprint, peerPublicKey: response.initiatorIdentityBytes, plaintext: plaintext)
    } catch {
      NSLog("[ChatManager] failed to accept a mailbox-retrieved handshake: \(error)")
    }
  }

  /// A continuing-conversation envelope retrieved from the mailbox: no
  /// sender attached, so — unlike `handleIncomingContinuing`, which already
  /// knows which session to use — this tries every known session's ratchet
  /// in turn and keeps whichever one actually decrypts. `FfiRatchet.decrypt`
  /// only succeeds against the one ratchet chain that produced this
  /// ciphertext, so at most one candidate can ever match.
  private func handleRetrievedContinuing(body: Data) {
    for session in ChatStore.loadAllSessions(slot: slot) {
      guard let ratchet = try? FfiRatchet.fromBytes(bytes: session.ratchetBytes) else { continue }
      guard let plaintext = try? ratchet.decrypt(message: body, associatedData: Data()) else { continue }
      try? ChatStore.saveSession(
        ChatStore.Session(peerId: session.peerId, peerPublicKey: session.peerPublicKey, ratchetBytes: ratchet.toBytes()),
        slot: slot
      )
      let fingerprint = (try? identityFingerprintOfPublicKey(publicKey: session.peerPublicKey)) ?? ""
      deliverIncoming(peerId: session.peerId, peerFingerprint: fingerprint, peerPublicKey: session.peerPublicKey, plaintext: plaintext)
      return
    }
    for session in ChatStore.loadAllSessions(slot: slot) {
      guard let ratchet = try? FfiRatchet.fromBytes(bytes: session.ratchetBytes) else { continue }
      guard let payload = try? ratchet.decrypt(message: body, associatedData: Self.groupControlAssociatedData) else { continue }
      try? ChatStore.saveSession(
        ChatStore.Session(peerId: session.peerId, peerPublicKey: session.peerPublicKey, ratchetBytes: ratchet.toBytes()),
        slot: slot
      )
      dispatchGroupControlPayload(fromPeerId: session.peerId, payload: payload)
      return
    }
    NSLog("[ChatManager] a mailbox-retrieved continuing envelope matched no known session — dropped")
  }

  // MARK: - Mailbox retrieval sweep

  /// Periodically checks every contact this device has a session with for
  /// mail queued while the two of them were never online at the same
  /// time — the read side of `depositContinuing`'s write. Scheduled
  /// explicitly on the main run loop rather than via
  /// `Timer.scheduledTimer` because `ChatManager` isn't guaranteed to be
  /// constructed on the main thread (`P2pSession.shared`'s lazy
  /// initialization can run wherever it's first touched), and `.common`
  /// mode keeps the timer firing even while the run loop is busy tracking
  /// UI scrolling/gestures.
  private func startRetrievalSweep() {
    DispatchQueue.main.async { [weak self] in
      guard let self else { return }
      let timer = Timer(timeInterval: Self.retrievalSweepInterval, repeats: true) { [weak self] _ in
        self?.sweepMailboxRetrieval()
      }
      RunLoop.main.add(timer, forMode: .common)
      self.retrievalTimer = timer
    }
  }

  private func sweepMailboxRetrieval() {
    for session in ChatStore.loadAllSessions(slot: slot) {
      try? node.retrieveFromMailbox(sharedMaterial: mailboxSharedMaterial(peerPublicKey: session.peerPublicKey))
    }
  }

  // MARK: - Contact card DHT announcement

  /// Periodically re-publishes this device's own contact card into the
  /// DHT (`announceContactCard`) so the record doesn't expire — mirrors
  /// `startRetrievalSweep`'s own main-run-loop scheduling caution (this
  /// class isn't guaranteed to be constructed on the main thread).
  private func startContactCardAnnounceSweep() {
    DispatchQueue.main.async { [weak self] in
      guard let self else { return }
      let timer = Timer(timeInterval: Self.contactCardAnnounceInterval, repeats: true) { [weak self] _ in
        self?.reannounceContactCard()
      }
      RunLoop.main.add(timer, forMode: .common)
      self.contactCardAnnounceTimer = timer
    }
  }

  private func reannounceContactCard() {
    try? node.announceContactCard(ownerIdentityPublicKey: identity.publicKeyBytes(), card: contactCard)
  }

  // MARK: - P2P event feed

  /// Called from the same event pump that drives `P2pSession.encode` (see
  /// `SpiritchatCryptoCoreModule`'s `OnCreate`), on every raw `FfiP2pEvent`
  /// — this class reacts to the handful it cares about and ignores the
  /// rest. JS never sees this event stream directly.
  func handleP2pEvent(_ event: FfiP2pEvent) {
    switch event {
    case .peerConnected(let peerId):
      lock.lock()
      connectedPeers.insert(peerId)
      lock.unlock()
      if state(for: peerId) == .dialing { endState(for: peerId) }
      attemptSend(peerId: peerId)

    case .peerDisconnected(let peerId):
      lock.lock()
      connectedPeers.remove(peerId)
      lock.unlock()

    case .mixRelayDiscovered(let peerId):
      lock.lock()
      let wasEmpty = knownMixRelays.isEmpty
      knownMixRelays.insert(peerId)
      lock.unlock()
      // Going from no usable mix to some is exactly the moment anything
      // held back for want of a route becomes sendable, and nothing else
      // would retry it until the next reconnect or app launch.
      if wasEmpty { retryQueuedForAllPeers() }

    case .dialFailed(let maybePeerId, let reason):
      guard let peerId = maybePeerId, state(for: peerId) == .dialing else { return }
      guard let item = firstOutboxItem(peerId) else {
        // No 1:1 item — this dial might have been for a queued group
        // envelope instead (see `attemptGroupDelivery`).
        if let groupItem = firstGroupOutboxItem(peerId) {
          endState(for: peerId)
          depositGroupItemToMailbox(groupItem)
        } else {
          endState(for: peerId)
        }
        return
      }
      if let session = ChatStore.loadSession(slot: slot, peerId: peerId) {
        // An existing conversation — the peer just isn't reachable right
        // now. Deposit into the mailbox instead of giving up outright,
        // the same fallback an already-connected send's later failure
        // (`envelopeDeliveryFailed`) already uses.
        depositContinuing(item: item, session: session)
      } else {
        // First contact with someone not directly reachable right now —
        // try the DHT for their contact card instead of giving up; if
        // that also fails, there's truly nothing left to try.
        transitionState(.resolvingCardViaDht, for: peerId)
        do {
          try node.resolveContactCard(ownerIdentityPublicKey: item.peerPublicKey)
        } catch {
          endState(for: peerId)
          emit(failedEvent(item: item, reason: reason))
        }
      }

    case .blobFetched(let peerId, let id, let bytes):
      // A media chunk takes priority — it's addressed by its own content
      // id, never the fixed contact-card id, so the two never overlap.
      if handleMediaBlobFetched(id: id, bytes: bytes) { return }
      guard id == Self.contactCardBlobId, state(for: peerId) == .fetchingCard else { return }
      handleCardFetched(peerId: peerId, cardBytes: bytes, delivery: .sendDirectly)

    case .blobFetchFailed(let peerId, let id, let reason):
      if handleMediaBlobFailed(id: id) { return }
      guard id == Self.contactCardBlobId, state(for: peerId) == .fetchingCard else { return }
      guard let item = firstOutboxItem(peerId) else { endState(for: peerId); return }
      // `attemptSend` only ever calls `fetchBlob` when no session exists
      // yet, so this is always a first-contact case — try the DHT next.
      transitionState(.resolvingCardViaDht, for: peerId)
      do {
        try node.resolveContactCard(ownerIdentityPublicKey: item.peerPublicKey)
      } catch {
        endState(for: peerId)
        emit(failedEvent(item: item, reason: reason))
      }

    case .contactCardResolved(let ownerIdentityPublicKey, let card):
      guard let peerId = try? p2pPeerIdFromPublicKey(publicKey: ownerIdentityPublicKey),
            state(for: peerId) == .resolvingCardViaDht
      else { return }
      handleCardFetched(peerId: peerId, cardBytes: card, delivery: .depositToMailbox)

    case .contactCardResolutionFailed(let ownerIdentityPublicKey):
      guard let peerId = try? p2pPeerIdFromPublicKey(publicKey: ownerIdentityPublicKey),
            state(for: peerId) == .resolvingCardViaDht
      else { return }
      endState(for: peerId)
      if let item = firstOutboxItem(peerId) {
        emit(failedEvent(item: item, reason: "Собеседник офлайн, и его карточка ещё не найдена в сети"))
      }

    case .envelopeReceived(let fromPeerId, let bytes):
      onEnvelopeReceived(fromPeerId: fromPeerId, bytes: bytes)

    case .envelopeDelivered(let toPeerId):
      switch state(for: toPeerId) {
      case .sendingEnvelope:
        endState(for: toPeerId)
        if let item = firstOutboxItem(toPeerId) {
          removeOutboxItem(localId: item.localId)
          emit(sentEvent(item: item))
        }
        attemptSend(peerId: toPeerId)
      case .sendingGroupEnvelope:
        endState(for: toPeerId)
        if let item = firstGroupOutboxItem(toPeerId) {
          removeGroupOutboxItem(localId: item.localId)
        }
        attemptSend(peerId: toPeerId)
      default:
        return
      }

    case .envelopeDeliveryFailed(let toPeerId, let reason):
      switch state(for: toPeerId) {
      case .sendingEnvelope:
        if let item = firstOutboxItem(toPeerId), let session = ChatStore.loadSession(slot: slot, peerId: toPeerId) {
          depositContinuing(item: item, session: session)
        } else {
          endState(for: toPeerId)
          if let item = firstOutboxItem(toPeerId) { emit(failedEvent(item: item, reason: reason)) }
        }
      case .sendingGroupEnvelope:
        defer { endState(for: toPeerId) }
        if let item = firstGroupOutboxItem(toPeerId) { depositGroupItemToMailbox(item) }
      default:
        return
      }

    case .mailboxEnvelopeRetrieved(let envelope):
      handleMailboxEnvelopeRetrieved(envelope)

    default:
      break
    }
  }

  // MARK: - Wire framing

  /// `[handshakeTag][2-byte BE length][InitialMessage bytes][ratchet envelope]`.
  /// The length prefix is needed because `InitialMessage`'s own byte layout
  /// doesn't report how many bytes it consumed, unlike the ratchet envelope
  /// that follows it (which is already self-delimiting).
  private static func frameHandshake(initialMessage: Data, ratchetMessage: Data) -> Data {
    var out = Data([handshakeTag])
    let length = UInt16(initialMessage.count)
    out.append(UInt8(length >> 8))
    out.append(UInt8(length & 0xff))
    out.append(initialMessage)
    out.append(ratchetMessage)
    return out
  }

  /// `[continuingTag][ratchet envelope]`.
  private static func frameContinuing(_ ratchetMessage: Data) -> Data {
    var out = Data([continuingTag])
    out.append(ratchetMessage)
    return out
  }

  /// `[groupMessageTag][groupId: 16 bytes][Sender Key envelope]`.
  private static func frameGroupMessage(groupId: Data, signedEnvelope: Data) -> Data {
    var out = Data([groupMessageTag])
    out.append(groupId)
    out.append(signedEnvelope)
    return out
  }

  /// `[controlInviteTag][groupId: 16 bytes][1-byte name length][name][1-byte
  /// member count][for each: 1-byte peer id length, peer id][Sender Key
  /// distribution bytes]`. The distribution is last (and un-length-
  /// prefixed) since `SenderKeyDistribution::decode` requires its own
  /// input to be exactly its own length, not merely a prefix of a longer
  /// buffer — the same reason it has to come after every length-prefixed
  /// field here, not before.
  private static func frameGroupInvite(groupId: Data, name: String, members: [String], distribution: Data) -> Data {
    var out = Data([controlInviteTag])
    out.append(groupId)
    let nameBytes = Data(name.utf8.prefix(255))
    out.append(UInt8(nameBytes.count))
    out.append(nameBytes)
    let clampedMembers = members.prefix(255)
    out.append(UInt8(clampedMembers.count))
    for member in clampedMembers {
      let memberBytes = Data(member.utf8.prefix(255))
      out.append(UInt8(memberBytes.count))
      out.append(memberBytes)
    }
    out.append(distribution)
    return out
  }

  private static func parseGroupInvite(_ body: Data) -> (groupId: Data, name: String, members: [String], distribution: Data)? {
    var offset = body.startIndex
    guard body.distance(from: offset, to: body.endIndex) >= 16 else { return nil }
    let groupId = Data(body[offset..<body.index(offset, offsetBy: 16)])
    offset = body.index(offset, offsetBy: 16)

    guard offset < body.endIndex else { return nil }
    let nameLength = Int(body[offset])
    offset = body.index(after: offset)
    guard body.distance(from: offset, to: body.endIndex) >= nameLength else { return nil }
    let nameEnd = body.index(offset, offsetBy: nameLength)
    let name = String(data: Data(body[offset..<nameEnd]), encoding: .utf8) ?? ""
    offset = nameEnd

    guard offset < body.endIndex else { return nil }
    let memberCount = Int(body[offset])
    offset = body.index(after: offset)
    var members: [String] = []
    for _ in 0..<memberCount {
      guard offset < body.endIndex else { return nil }
      let length = Int(body[offset])
      offset = body.index(after: offset)
      guard body.distance(from: offset, to: body.endIndex) >= length else { return nil }
      let end = body.index(offset, offsetBy: length)
      guard let member = String(data: Data(body[offset..<end]), encoding: .utf8) else { return nil }
      members.append(member)
      offset = end
    }

    return (groupId, name, members, Data(body[offset...]))
  }

  /// `[controlDistributionTag][groupId: 16 bytes][Sender Key distribution
  /// bytes]`.
  private static func frameGroupMemberDistribution(groupId: Data, distribution: Data) -> Data {
    var out = Data([controlDistributionTag])
    out.append(groupId)
    out.append(distribution)
    return out
  }

  private static func parseGroupMemberDistribution(_ body: Data) -> (groupId: Data, distribution: Data)? {
    guard body.count >= 16 else { return nil }
    return (Data(body.prefix(16)), Data(body.dropFirst(16)))
  }

  /// `[controlMemberAddedTag/controlMemberRemovedTag][groupId: 16 bytes]
  /// [1-byte peer id length][peer id]` — shared framing for both "someone
  /// joined" and "someone left" notifications, since both only ever name
  /// one other member.
  private static func frameGroupMembershipChange(tag: UInt8, groupId: Data, memberPeerId: String) -> Data {
    var out = Data([tag])
    out.append(groupId)
    let memberBytes = Data(memberPeerId.utf8.prefix(255))
    out.append(UInt8(memberBytes.count))
    out.append(memberBytes)
    return out
  }

  private static func frameMemberAdded(groupId: Data, newMemberPeerId: String) -> Data {
    frameGroupMembershipChange(tag: controlMemberAddedTag, groupId: groupId, memberPeerId: newMemberPeerId)
  }

  private static func frameMemberRemoved(groupId: Data, removedMemberPeerId: String) -> Data {
    frameGroupMembershipChange(tag: controlMemberRemovedTag, groupId: groupId, memberPeerId: removedMemberPeerId)
  }

  private static func parseGroupMembershipChange(_ body: Data) -> (groupId: Data, memberPeerId: String)? {
    guard body.count >= 16 else { return nil }
    let groupId = Data(body.prefix(16))
    let rest = body.dropFirst(16)
    guard let length = rest.first else { return nil }
    let peerIdBytes = rest.dropFirst()
    guard peerIdBytes.count == Int(length), let memberPeerId = String(data: Data(peerIdBytes), encoding: .utf8) else { return nil }
    return (groupId, memberPeerId)
  }

  // --- MLS control framing ---
  //
  // `[controlMlsInviteRequestTag][groupId: 16][1-byte name len][name]
  //  [1-byte member count][for each: 1-byte peer id len, peer id]` — the
  // creator's ask for a key package. Carries the roster peer ids so the
  // invitee learns who else is (or will be) in the group, same as a
  // Sender Keys invite did.
  private static func frameMlsInviteRequest(groupId: Data, name: String, members: [String]) -> Data {
    var out = Data([controlMlsInviteRequestTag])
    out.append(groupId)
    let nameBytes = Data(name.utf8.prefix(255))
    out.append(UInt8(nameBytes.count))
    out.append(nameBytes)
    let clamped = members.prefix(255)
    out.append(UInt8(clamped.count))
    for member in clamped {
      let b = Data(member.utf8.prefix(255))
      out.append(UInt8(b.count))
      out.append(b)
    }
    return out
  }

  private static func parseMlsInviteRequest(_ body: Data) -> (groupId: Data, name: String, members: [String])? {
    var offset = body.startIndex
    guard body.distance(from: offset, to: body.endIndex) >= 16 else { return nil }
    let groupId = Data(body[offset..<body.index(offset, offsetBy: 16)])
    offset = body.index(offset, offsetBy: 16)

    guard offset < body.endIndex else { return nil }
    let nameLength = Int(body[offset]); offset = body.index(after: offset)
    guard body.distance(from: offset, to: body.endIndex) >= nameLength else { return nil }
    let nameEnd = body.index(offset, offsetBy: nameLength)
    let name = String(data: Data(body[offset..<nameEnd]), encoding: .utf8) ?? ""
    offset = nameEnd

    guard offset < body.endIndex else { return nil }
    let memberCount = Int(body[offset]); offset = body.index(after: offset)
    var members: [String] = []
    for _ in 0..<memberCount {
      guard offset < body.endIndex else { return nil }
      let length = Int(body[offset]); offset = body.index(after: offset)
      guard body.distance(from: offset, to: body.endIndex) >= length else { return nil }
      let end = body.index(offset, offsetBy: length)
      guard let member = String(data: Data(body[offset..<end]), encoding: .utf8) else { return nil }
      members.append(member)
      offset = end
    }
    return (groupId, name, members)
  }

  /// `[tag][groupId: 16][opaque blob]` — shared framing for the three MLS
  /// control payloads that carry one length-implicit byte blob (a key
  /// package, a Welcome, or a Commit); the blob runs to the end, so no
  /// length prefix is needed.
  private static func frameMlsBlob(tag: UInt8, groupId: Data, blob: Data) -> Data {
    var out = Data([tag])
    out.append(groupId)
    out.append(blob)
    return out
  }

  private static func parseMlsBlob(_ body: Data) -> (groupId: Data, blob: Data)? {
    guard body.count >= 16 else { return nil }
    return (Data(body.prefix(16)), Data(body.dropFirst(16)))
  }

  private static func hexString(_ data: Data) -> String {
    data.map { String(format: "%02x", $0) }.joined()
  }

  private static func data(fromHex hex: String) -> Data? {
    guard hex.count % 2 == 0 else { return nil }
    var out = Data(capacity: hex.count / 2)
    var index = hex.startIndex
    while index < hex.endIndex {
      let next = hex.index(index, offsetBy: 2)
      guard let byte = UInt8(hex[index..<next], radix: 16) else { return nil }
      out.append(byte)
      index = next
    }
    return out
  }

  // MARK: - Event encoding

  private func receivedEvent(peerId: String, peerFingerprint: String, peerPublicKey: Data, plaintext: Data) -> [String: Any?] {
    [
      "type": "messageReceived",
      "peerId": peerId,
      "peerFingerprint": peerFingerprint,
      "peerPublicKeyBase64": peerPublicKey.base64EncodedString(),
      "plaintext": String(data: plaintext, encoding: .utf8) ?? "",
      "at": Date().timeIntervalSince1970,
    ]
  }

  private func sentEvent(item: ChatStore.OutboxItem) -> [String: Any?] {
    ["type": "messageSent", "peerId": item.peerId, "localId": item.localId]
  }

  private func failedEvent(item: ChatStore.OutboxItem, reason: String) -> [String: Any?] {
    ["type": "messageFailed", "peerId": item.peerId, "localId": item.localId, "reason": reason]
  }

  private func groupInvitedEvent(groupId: String, name: String, members: [String]) -> [String: Any?] {
    ["type": "groupInvited", "groupId": groupId, "name": name, "members": members]
  }

  private func groupMessageReceivedEvent(groupId: String, senderPeerId: String, plaintext: Data) -> [String: Any?] {
    [
      "type": "groupMessageReceived",
      "groupId": groupId,
      "senderPeerId": senderPeerId,
      "plaintext": String(data: plaintext, encoding: .utf8) ?? "",
      "at": Date().timeIntervalSince1970,
    ]
  }

  private func groupMemberAddedEvent(groupId: String, memberPeerId: String) -> [String: Any?] {
    ["type": "groupMemberAdded", "groupId": groupId, "memberPeerId": memberPeerId]
  }

  private func groupMemberRemovedEvent(groupId: String, memberPeerId: String) -> [String: Any?] {
    ["type": "groupMemberRemoved", "groupId": groupId, "memberPeerId": memberPeerId]
  }
}
