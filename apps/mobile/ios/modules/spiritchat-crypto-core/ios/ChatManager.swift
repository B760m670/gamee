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
  private var retrievalTimer: Timer?
  private var contactCardAnnounceTimer: Timer?

  private let slot: Int
  private let node: FfiP2pNode
  private let identity: FfiIdentity
  private let agreement: FfiAgreementKey
  private let prekeys: FfiPrekeyStore

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
    // fetch/send path a brand new `sendMessage` call would go through.
    for peerId in Set(ChatStore.loadOutbox(slot: slot).map(\.peerId)) {
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

  /// Advances `peerId`'s outbox by exactly one step: dial if not
  /// connected, fetch a contact card if connected but no session exists
  /// yet, or encrypt-and-send if a session already exists. A no-op if
  /// nothing is queued for `peerId`, or a step is already outstanding
  /// (`peerStates`) — the event handlers below drive it forward from here.
  private func attemptSend(peerId: String) {
    guard let item = firstOutboxItem(peerId) else { return }

    guard isConnected(peerId) else {
      guard beginState(.dialing, for: peerId) else { return }
      try? node.dial(peerId: peerId, knownAddresses: [])
      return
    }

    if let session = ChatStore.loadSession(slot: slot, peerId: peerId) {
      guard beginState(.sendingEnvelope, for: peerId) else { return }
      sendContinuing(item: item, session: session)
    } else {
      guard beginState(.fetchingCard, for: peerId) else { return }
      do {
        try node.fetchBlob(peerId: peerId, id: Self.contactCardBlobId)
      } catch {
        endState(for: peerId)
        emit(failedEvent(item: item, reason: "\(error)"))
      }
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
      emit(failedEvent(item: item, reason: "queued for offline delivery"))
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
  // Deliberately v1-scoped: a group is created with a fixed initial
  // member list (no add/remove afterward yet), and every member named at
  // creation must already be an existing 1:1 contact (a group control
  // message piggybacks on an *existing* pairwise ratchet session — it
  // never triggers first-contact/X3DH establishment the way an ordinary
  // `sendMessage` does). Both control-message delivery and group content
  // fan-out are best-effort (direct send, falling back to a single
  // mailbox deposit attempt) rather than the persistent retry queue
  // 1:1 messages get — a member who's unreachable through both avenues
  // simply won't have working group crypto until a later interaction
  // gives another chance to catch up.

  /// Creates a new group named `name` with `memberPeerIds` as its initial
  /// (non-self) members — every one of them must already have an
  /// established pairwise session (see this section's own doc comment).
  /// Returns the new group's id, or `nil` if this device's own identity
  /// isn't available yet or the group couldn't be persisted.
  @discardableResult
  func createGroup(name: String, memberPeerIds: [String]) -> String? {
    guard let myPeerId = try? node.localPeerId() else { return nil }
    let groupIdBytes = Data((0..<16).map { _ in UInt8.random(in: 0...255) })
    let groupId = Self.hexString(groupIdBytes)

    let ownState = FfiSenderKeyState.generate()
    let session = GroupStore.Session(
      groupId: groupId, name: name, members: memberPeerIds,
      ownSenderKeyStateBytes: ownState.toBytes(), receiverStates: [:]
    )
    guard (try? GroupStore.saveSession(session, slot: slot)) != nil else { return nil }

    // The invite's own member list includes this device (the creator) —
    // every recipient needs to know to also send *it* their distribution,
    // not just each other.
    let allMembers = memberPeerIds + [myPeerId]
    let distribution = ownState.toDistributionBytes()
    for member in memberPeerIds {
      sendGroupControlMessageBestEffort(
        peerId: member,
        payload: Self.frameGroupInvite(groupId: groupIdBytes, name: name, members: allMembers, distribution: distribution)
      )
    }
    return groupId
  }

  /// Encrypts `plaintext` once under this device's own chain for `groupId`
  /// and fans the same ciphertext out to every other member — returns a
  /// local id (nothing currently tracks it against a delivery event, since
  /// fan-out is best-effort; kept for symmetry with `sendMessage` and any
  /// future UI that wants one). `nil` if this device isn't (or is no
  /// longer) a member of `groupId`.
  @discardableResult
  func sendGroupMessage(groupId: String, plaintext: Data) -> String? {
    guard var session = GroupStore.loadSession(slot: slot, groupId: groupId),
          let groupIdBytes = Self.data(fromHex: groupId),
          let ownState = try? FfiSenderKeyState.fromBytes(bytes: session.ownSenderKeyStateBytes)
    else { return nil }

    guard let signedEnvelope = try? ownState.encrypt(plaintext: plaintext, associatedData: groupIdBytes) else {
      return nil
    }
    session.ownSenderKeyStateBytes = ownState.toBytes()
    try? GroupStore.saveSession(session, slot: slot)

    let wireEnvelope = Self.frameGroupMessage(groupId: groupIdBytes, signedEnvelope: signedEnvelope)
    for member in session.members {
      sendGroupContentBestEffort(peerId: member, wireEnvelope: wireEnvelope)
    }
    return UUID().uuidString
  }

  /// Delivers `wireEnvelope` to `peerId` right now if reachable, otherwise
  /// falls back to a single mailbox deposit attempt using the *pairwise*
  /// shared material already established with them (see
  /// `mailboxSharedMaterial`) — reusing that same per-pair mailbox queue
  /// for group content too, distinguished on retrieval purely by this
  /// envelope's own leading tag byte, the same way every other kind of
  /// envelope already is. Requires an existing pairwise session (to learn
  /// `peerId`'s identity public key for the mailbox tag) — a group member
  /// this device has literally never established one with can't be
  /// reached this way, consistent with this section's own v1 scope.
  private func sendGroupContentBestEffort(peerId: String, wireEnvelope: Data) {
    if isConnected(peerId) {
      do {
        try node.sendEnvelope(peerId: peerId, bytes: wireEnvelope)
        return
      } catch {
        // Fall through to the mailbox below.
      }
    }
    guard let pairwiseSession = ChatStore.loadSession(slot: slot, peerId: peerId) else {
      NSLog("[ChatManager] no pairwise session with group member \(peerId) — cannot deliver a group message to them right now")
      return
    }
    try? node.depositToMailbox(sharedMaterial: mailboxSharedMaterial(peerPublicKey: pairwiseSession.peerPublicKey), envelope: wireEnvelope)
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
    session.receiverStates[fromPeerId] = receiverState.toBytes()
    try? GroupStore.saveSession(session, slot: slot)
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

    for (memberPeerId, receiverStateBytes) in session.receiverStates {
      guard let receiverState = try? FfiSenderKeyReceiverState.fromBytes(bytes: receiverStateBytes) else { continue }
      guard let plaintext = try? receiverState.decrypt(message: signedEnvelope, associatedData: groupIdBytes) else { continue }
      var updated = session
      updated.receiverStates[memberPeerId] = receiverState.toBytes()
      try? GroupStore.saveSession(updated, slot: slot)
      emit(groupMessageReceivedEvent(groupId: groupId, senderPeerId: memberPeerId, plaintext: plaintext))
      return
    }
    NSLog("[ChatManager] a group message for \(groupId) matched no known member's chain — dropped")
  }

  // MARK: - Receiving

  private func onEnvelopeReceived(fromPeerId: String, bytes: Data) {
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
      emit(receivedEvent(peerId: fromPeerId, peerFingerprint: fingerprint, peerPublicKey: response.initiatorIdentityBytes, plaintext: plaintext))
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
      emit(receivedEvent(peerId: fromPeerId, peerFingerprint: fingerprint, peerPublicKey: session.peerPublicKey, plaintext: plaintext))
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
      emit(receivedEvent(peerId: peerId, peerFingerprint: fingerprint, peerPublicKey: response.initiatorIdentityBytes, plaintext: plaintext))
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
      emit(receivedEvent(peerId: session.peerId, peerFingerprint: fingerprint, peerPublicKey: session.peerPublicKey, plaintext: plaintext))
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

    case .dialFailed(let maybePeerId, let reason):
      guard let peerId = maybePeerId, state(for: peerId) == .dialing else { return }
      guard let item = firstOutboxItem(peerId) else { endState(for: peerId); return }
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
      guard id == Self.contactCardBlobId, state(for: peerId) == .fetchingCard else { return }
      handleCardFetched(peerId: peerId, cardBytes: bytes, delivery: .sendDirectly)

    case .blobFetchFailed(let peerId, let id, let reason):
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
      guard state(for: toPeerId) == .sendingEnvelope else { return }
      endState(for: toPeerId)
      if let item = firstOutboxItem(toPeerId) {
        removeOutboxItem(localId: item.localId)
        emit(sentEvent(item: item))
      }
      attemptSend(peerId: toPeerId)

    case .envelopeDeliveryFailed(let toPeerId, let reason):
      guard state(for: toPeerId) == .sendingEnvelope else { return }
      if let item = firstOutboxItem(toPeerId), let session = ChatStore.loadSession(slot: slot, peerId: toPeerId) {
        depositContinuing(item: item, session: session)
      } else {
        endState(for: toPeerId)
        if let item = firstOutboxItem(toPeerId) { emit(failedEvent(item: item, reason: reason)) }
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
}
