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
/// (`packages/p2p-core`'s envelope/blob protocols) are both already fully
/// built and tested; this class is only the glue between them:
/// - A contact's current prekey bundle ("contact card") is fetched the same
///   way an avatar is — reusing the existing blob protocol under a fixed,
///   well-known blob id — rather than adding a new libp2p protocol.
/// - A conversation's first envelope carries both the X3DH `InitialMessage`
///   and the first ratchet ciphertext, framed as a 1-byte type tag (and,
///   for the first message, a 2-byte length prefix ahead of the initial
///   message bytes); every later envelope is just tagged ratchet
///   ciphertext. See `frameHandshake`/`frameContinuing` below.
final class ChatManager {
  /// Which async step (if any) is outstanding for a given peer — guards
  /// against acting on a stray/duplicate event (e.g. an `EnvelopeDelivered`
  /// arriving when nothing was actually being sent to that peer) and
  /// against starting a second attempt while one is already in flight.
  private enum PeerSendState {
    case dialing
    case fetchingCard
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

  /// How often `sweepMailboxRetrieval` runs — same order of magnitude as
  /// the mix's own dummy-traffic interval (`MIX_DUMMY_TRAFFIC_INTERVAL` in
  /// `node.rs`), so a retrieval query blends into traffic that's already
  /// happening on this schedule rather than standing out as its own signal.
  private static let retrievalSweepInterval: TimeInterval = 30

  private let lock = NSLock()
  private var peerStates: [String: PeerSendState] = [:]
  private var connectedPeers: Set<String> = []
  private var retrievalTimer: Timer?

  private let slot: Int
  private let node: FfiP2pNode
  private let identity: FfiIdentity
  private let agreement: FfiAgreementKey
  private let prekeys: FfiPrekeyStore

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
    try? node.setLocalBlob(id: Self.contactCardBlobId, bytes: card)

    // Anything left over from a previous run (app killed mid-send, or the
    // recipient was offline) gets another chance now — the same dial/
    // fetch/send path a brand new `sendMessage` call would go through.
    for peerId in Set(ChatStore.loadOutbox(slot: slot).map(\.peerId)) {
      attemptSend(peerId: peerId)
    }

    startRetrievalSweep()
  }

  deinit {
    retrievalTimer?.invalidate()
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

  /// Runs once this device's `fetchBlob` for `peerId`'s contact card comes
  /// back: verifies it (`FfiContactCard.parse` already checks every
  /// signature), runs X3DH against it, bootstraps the initiator side of a
  /// Double Ratchet session, and sends the first (combined) envelope.
  private func handleCardFetched(peerId: String, cardBytes: Data) {
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
      transitionState(.sendingEnvelope, for: peerId)
      try node.sendEnvelope(peerId: peerId, bytes: envelope)
    } catch {
      endState(for: peerId)
      emit(failedEvent(item: item, reason: "\(error)"))
    }
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
      NSLog("[ChatManager] failed to decrypt an envelope from \(fromPeerId): \(error)")
    }
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
      endState(for: peerId)
      if let item = firstOutboxItem(peerId) { emit(failedEvent(item: item, reason: reason)) }

    case .blobFetched(let peerId, let id, let bytes):
      guard id == Self.contactCardBlobId, state(for: peerId) == .fetchingCard else { return }
      handleCardFetched(peerId: peerId, cardBytes: bytes)

    case .blobFetchFailed(let peerId, let id, let reason):
      guard id == Self.contactCardBlobId, state(for: peerId) == .fetchingCard else { return }
      endState(for: peerId)
      if let item = firstOutboxItem(peerId) { emit(failedEvent(item: item, reason: reason)) }

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
}
