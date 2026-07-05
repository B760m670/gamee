import Foundation

/// This device's live network presence: one libp2p node **per registered
/// account slot**, each using that identity's own 32-byte seed (see
/// `spiritchat_p2p_core::identity::keypair_from_seed`), so a peer's
/// `PeerId` on the wire always matches the identity in their contact
/// card. Every occupied slot's node runs concurrently — the device being
/// online means *every* account on it is online, not just the one the UI
/// currently shows; switching accounts re-points the UI, it no longer
/// tears a node down. Only the mining/mix-relay/dummy-traffic policies
/// (`MiningController`/`MixRelayController`) follow the active slot, so
/// the background cost of the extra nodes stays receive-and-retry only.
final class P2pSession {
  // Guards `sessions` against a real race: the native supervisor loop
  // (see SpiritchatCryptoCoreModule's OnCreate) polls `session(forSlot:)`
  // from a background Task every 200ms, while JS calls functions like
  // `p2pLocalPeerId()` — routed through Expo's own (not-necessarily-main)
  // dispatch queue — moments after `setIdentityFromWords` resolves during
  // onboarding. Both can see an empty slot at once; unguarded, that means
  // two `ChainStore::open()` calls racing for the same `redb` file, which
  // can fail with a lock conflict. Only ever held for the duration of the
  // check-then-create (or check-then-remove) below, never across a call
  // into `node` itself.
  private static let lock = NSLock()
  private static var sessions: [Int: P2pSession] = [:]

  /// The error from the most recent failed start attempt, if any — not
  /// acted on by this class itself, just somewhere a debug screen (or a
  /// developer attached to the console) can find *some* signal for why P2P
  /// isn't up, given this environment doesn't reliably surface standard
  /// crash reports (see `shared`'s doc comment on why this class no longer
  /// treats a start failure as fatal).
  private(set) static var lastStartupError: Error?

  let slot: Int
  /// This session's own identity fingerprint — stamped onto every event
  /// this session emits toward JS (see SpiritchatCryptoCoreModule's pump),
  /// so the stores can route an inactive account's traffic into that
  /// account's own namespaced storage instead of the active one's.
  let selfFingerprint: String
  /// This node's own PeerId — cached at spawn so `interconnect()` never
  /// needs a throwing FFI call mid-walk.
  let peerId: String
  /// This account's 32-byte identity seed — what decrypts a recovery
  /// backup fetched from the DHT (see the pump in
  /// SpiritchatCryptoCoreModule). Exactly as sensitive as the Keychain
  /// item it was read from; never crosses the JS bridge.
  let identitySeed: Data
  let node: FfiP2pNode
  /// Owns end-to-end-encrypted messaging for this session — created fresh
  /// alongside `node` so it's always torn down and rebuilt together with
  /// it (sign-out, account removal), never left pointing at a stale node
  /// or identity. See `ChatManager`'s own doc comment for what it does.
  let chatManager: ChatManager

  // Guarded by `stateLock`: appended from this session's event pump task,
  // read from the supervisor loop's `interconnect()` — two different
  // tasks.
  private let stateLock = NSLock()
  private var pumpClaimed = false
  private var listenAddresses: [String] = []

  private init(slot: Int, selfFingerprint: String, peerId: String, identitySeed: Data, node: FfiP2pNode, chatManager: ChatManager) {
    self.slot = slot
    self.selfFingerprint = selfFingerprint
    self.peerId = peerId
    self.identitySeed = identitySeed
    self.node = node
    self.chatManager = chatManager
  }

  /// One-shot claim of this session's event pump — the supervisor loop
  /// polls every 200ms and must start exactly one pump per session object,
  /// without racing itself across iterations.
  func claimPump() -> Bool {
    stateLock.lock()
    defer { stateLock.unlock() }
    if pumpClaimed { return false }
    pumpClaimed = true
    return true
  }

  func recordListenAddress(_ address: String) {
    stateLock.lock()
    defer { stateLock.unlock() }
    if !listenAddresses.contains(address) {
      listenAddresses.append(address)
    }
  }

  func currentListenAddresses() -> [String] {
    stateLock.lock()
    defer { stateLock.unlock() }
    return listenAddresses
  }

  /// Where a given slot's `@username` ledger `redb` file lives. Per-slot
  /// (unlike the single shared file this used to be) because every running
  /// node owns its store file exclusively — two concurrent nodes sharing
  /// one `redb` is exactly the "Database already open" lock conflict the
  /// account switcher used to hit. The chain *contents* still converge to
  /// the same public chain either way; only the copies are per-slot, a few
  /// MB each. The pre-multi-node shared file is migrated into the active
  /// slot's directory on first touch (see `migrateSharedLedgerIfNeeded`).
  private static func ledgerDatabasePath(slot: Int) -> URL {
    let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
    let dir = base
      .appendingPathComponent("Ledger", isDirectory: true)
      .appendingPathComponent("\(slot)", isDirectory: true)
    do {
      try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    } catch {
      // Not treated as fatal here either — `ChainStore::open` (Rust) will
      // itself fail loudly a moment later if the directory genuinely isn't
      // usable, and `session(forSlot:)` below is what actually decides how
      // to react to that. Logged so the underlying reason isn't silently
      // lost.
      NSLog("[P2pSession] failed to create the ledger directory at \(dir.path): \(error)")
    }
    return dir.appendingPathComponent("ledger.redb")
  }

  /// Moves the pre-multi-node shared `Ledger/ledger.redb` (and its sibling
  /// `mailbox.redb`) into `slot`'s own directory, so the account that was
  /// active before this update keeps its synced chain and cached deposits
  /// instead of re-syncing from zero. A no-op once migrated (or on a fresh
  /// install that never had the shared file).
  private static func migrateSharedLedgerIfNeeded(into slot: Int) {
    let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
    let legacyDir = base.appendingPathComponent("Ledger", isDirectory: true)
    let files = ["ledger.redb", "mailbox.redb"]
    let target = ledgerDatabasePath(slot: slot).deletingLastPathComponent()
    for file in files {
      let old = legacyDir.appendingPathComponent(file)
      let new = target.appendingPathComponent(file)
      guard FileManager.default.fileExists(atPath: old.path),
            !FileManager.default.fileExists(atPath: new.path) else { continue }
      do {
        try FileManager.default.moveItem(at: old, to: new)
      } catch {
        NSLog("[P2pSession] failed to migrate \(file) into slot \(slot): \(error)")
      }
    }
  }

  /// The *active* account's session — what every JS-facing module function
  /// operates on. Same contract as always: `nil` until an identity exists
  /// or if the most recent spawn attempt failed (deliberately non-fatal;
  /// see `session(forSlot:)`).
  static var shared: P2pSession? {
    session(forSlot: IdentitySession.activeSlot)
  }

  /// The running session for `slot`, spawning it on first access — every
  /// occupied slot gets its own live node now, not just the active one:
  /// the device is online, so every account registered on it can be too
  /// (receiving messages, acking, retrying its outbox) regardless of which
  /// one the UI currently shows. `nil` if `slot` holds no complete
  /// identity **or** the most recent spawn attempt failed. A failure is
  /// deliberately not fatal — bringing down the entire app over the
  /// P2P/ledger subsystem specifically would make a device this one piece
  /// doesn't like unusable instead of degraded; callers treat `nil` as
  /// "not ready yet" and the supervisor loop retries on its next pass, so
  /// a transient cause (e.g. a file lock still being released) heals
  /// itself.
  static func session(forSlot slot: Int) -> P2pSession? {
    lock.lock()
    defer { lock.unlock() }
    if let existing = sessions[slot] { return existing }
    guard let identitySession = IdentitySession.loadFor(slot: slot) else { return nil }
    do {
      // Only the active slot inherits the pre-multi-node shared chain —
      // the supervisor spawns slots in index order, and without this
      // check slot 0 would grab the migration even when another slot was
      // the account that actually synced it.
      if slot == IdentitySession.activeSlot {
        migrateSharedLedgerIfNeeded(into: slot)
      }
      let node = try FfiP2pNode.spawn(
        identitySeed: identitySession.identity.secretBytes(),
        ledgerDataDir: Self.ledgerDatabasePath(slot: slot).path
      )
      let chatManager = ChatManager(
        slot: slot,
        node: node,
        identity: identitySession.identity,
        agreement: identitySession.agreement,
        prekeys: identitySession.prekeys
      )
      let session = P2pSession(
        slot: slot,
        selfFingerprint: identitySession.fingerprint,
        peerId: node.localPeerId(),
        identitySeed: identitySession.identity.secretBytes(),
        node: node,
        chatManager: chatManager
      )
      sessions[slot] = session
      lastStartupError = nil
      return session
    } catch {
      lastStartupError = error
      NSLog("[P2pSession] failed to start the P2P node for slot \(slot): \(error)")
      return nil
    }
  }

  /// Every currently-running session, in slot order.
  static func runningSessions() -> [P2pSession] {
    lock.lock()
    defer { lock.unlock() }
    return sessions.keys.sorted().compactMap { sessions[$0] }
  }

  /// Dials every running session from every other, over their own local
  /// listen addresses. Two accounts on one device are two full peers; on a
  /// device where mDNS is unavailable (iOS multicast needs an entitlement
  /// sideloaded builds don't reliably have) nothing else would ever
  /// connect them — and this loopback connection is exactly what makes a
  /// message from one of this device's accounts to another arrive
  /// instantly instead of waiting on DHT/relay round trips. Deduplicated
  /// by address-set fingerprint so the supervisor can call this every poll
  /// tick without spamming redundant dials.
  private static var lastInterconnectKey = ""
  static func interconnect() {
    let running = runningSessions()
    guard running.count >= 2 else { return }
    let key = running
      .map { "\($0.slot):\($0.currentListenAddresses().count)" }
      .joined(separator: ",")
    guard key != lastInterconnectKey else { return }
    lastInterconnectKey = key

    for target in running {
      let addresses = target.currentListenAddresses().filter { !$0.contains("/p2p-circuit") }
      guard !addresses.isEmpty, !target.peerId.isEmpty else { continue }
      for dialer in running where dialer !== target {
        try? dialer.node.dial(peerId: target.peerId, knownAddresses: addresses)
      }
    }
  }

  /// Stops `slot`'s node and forgets its session — for sign-out/account
  /// removal, where that identity is going away. A subsequent
  /// `session(forSlot:)` for a re-created slot starts a genuinely fresh
  /// node instead of reusing this one's now-meaningless connections/DHT
  /// state. Safe to call even if no node was ever started for `slot`.
  static func stop(slot: Int) {
    lock.lock()
    defer { lock.unlock() }
    if let session = sessions.removeValue(forKey: slot) {
      try? session.node.shutdown()
    }
    lastInterconnectKey = ""
  }

  /// Kept for the one caller (`removeAccountSlot`) that historically
  /// meant "stop the active account's node".
  static func signOut() {
    stop(slot: IdentitySession.activeSlot)
  }

  /// Encodes an event as a plain dictionary for the JS bridge, tagged by
  /// `type` so the TS side can discriminate without a second binding layer.
  static func encode(_ event: FfiP2pEvent) -> [String: Any?] {
    switch event {
    case .listeningOn(let address):
      return ["type": "listeningOn", "address": address]
    case .peerDiscoveredLocally(let peerId):
      return ["type": "peerDiscoveredLocally", "peerId": peerId]
    case .peerConnected(let peerId):
      return ["type": "peerConnected", "peerId": peerId]
    case .peerIdentified(let peerId):
      return ["type": "peerIdentified", "peerId": peerId]
    case .peerDisconnected(let peerId):
      return ["type": "peerDisconnected", "peerId": peerId]
    case .dialFailed(let peerId, let reason):
      return ["type": "dialFailed", "peerId": peerId, "reason": reason]
    case .relayReservationFailed(let reason):
      return ["type": "relayReservationFailed", "reason": reason]
    case .envelopeReceived(let fromPeerId, let bytes):
      return ["type": "envelopeReceived", "fromPeerId": fromPeerId, "bytes": bytes]
    case .envelopeDelivered(let toPeerId):
      return ["type": "envelopeDelivered", "toPeerId": toPeerId]
    case .envelopeDeliveryFailed(let toPeerId, let reason):
      return ["type": "envelopeDeliveryFailed", "toPeerId": toPeerId, "reason": reason]
    case .peerAddressesResolved(let peerId, let addresses):
      return ["type": "peerAddressesResolved", "peerId": peerId, "addresses": addresses]
    case .peerAddressResolutionFailed(let peerId):
      return ["type": "peerAddressResolutionFailed", "peerId": peerId]
    case .addressesAnnounced:
      return ["type": "addressesAnnounced"]
    case .addressAnnouncementFailed(let reason):
      return ["type": "addressAnnouncementFailed", "reason": reason]
    case .blobFetched(let peerId, let id, let bytes):
      // Cache to disk here rather than shipping the raw bytes across the
      // JS bridge a second time — JS only ever needs a file path to hand
      // to <Image>, never the bytes themselves.
      let idHex = id.hexEncoded
      let url = (try? BlobStore.save(bytes, idHex: idHex)) ?? BlobStore.path(for: idHex)
      return ["type": "blobFetched", "peerId": peerId, "id": idHex, "localPath": url.absoluteString]
    case .blobFetchFailed(let peerId, let id, let reason):
      return ["type": "blobFetchFailed", "peerId": peerId, "id": id.hexEncoded, "reason": reason]
    case .contactCardAnnounced:
      return ["type": "contactCardAnnounced"]
    case .contactCardAnnouncementFailed(let reason):
      return ["type": "contactCardAnnouncementFailed", "reason": reason]
    case .contactCardResolved(let ownerIdentityPublicKey, let card):
      return [
        "type": "contactCardResolved",
        "ownerIdentityPublicKeyBase64": ownerIdentityPublicKey.base64EncodedString(),
        "card": card,
      ]
    case .contactCardResolutionFailed(let ownerIdentityPublicKey):
      return ["type": "contactCardResolutionFailed", "ownerIdentityPublicKeyBase64": ownerIdentityPublicKey.base64EncodedString()]
    case .avatarPointerAnnounced:
      return ["type": "avatarPointerAnnounced"]
    case .avatarPointerAnnouncementFailed(let reason):
      return ["type": "avatarPointerAnnouncementFailed", "reason": reason]
    case .avatarPointerResolved(let peerId, let avatarContentId):
      return ["type": "avatarPointerResolved", "peerId": peerId, "avatarContentId": avatarContentId]
    case .avatarPointerResolutionFailed(let peerId):
      return ["type": "avatarPointerResolutionFailed", "peerId": peerId]
    case .recoveryBackupAnnounced:
      return ["type": "recoveryBackupAnnounced"]
    case .recoveryBackupAnnouncementFailed(let reason):
      return ["type": "recoveryBackupAnnouncementFailed", "reason": reason]
    case .recoveryBackupResolved(let ownerIdentityPublicKey, let backup):
      // Normally intercepted (and decrypted) by the module's event pump
      // before it ever reaches this generic encoder — kept here so the
      // switch stays exhaustive and a stray event still crosses safely.
      return [
        "type": "recoveryBackupResolved",
        "ownerIdentityPublicKeyBase64": ownerIdentityPublicKey.base64EncodedString(),
        "backup": backup,
      ]
    case .recoveryBackupResolutionFailed(let ownerIdentityPublicKey):
      return ["type": "recoveryBackupResolutionFailed", "ownerIdentityPublicKeyBase64": ownerIdentityPublicKey.base64EncodedString()]
    case .usernameResolved(let username, let claim):
      // Verify here, not in JS — the DHT is a public, untrusted store, so
      // an unverified claim must never reach the app as if it were
      // trustworthy. A claim that doesn't check out (wrong signature,
      // malformed bytes) is reported the same as "nothing published",
      // since neither is safe to act on.
      guard
        let publicKey = UsernameClaim.verify(username: username, claim: claim),
        let fingerprint = try? identityFingerprintOfPublicKey(publicKey: publicKey),
        let peerId = try? p2pPeerIdFromPublicKey(publicKey: publicKey)
      else {
        return ["type": "usernameClaimInvalid", "username": username]
      }
      return [
        "type": "usernameResolved",
        "username": username,
        "publicKeyBase64": publicKey.base64EncodedString(),
        "fingerprint": fingerprint,
        "peerId": peerId,
      ]
    case .usernameResolutionFailed(let username):
      return ["type": "usernameResolutionFailed", "username": username]
    case .usernameAnnounced(let username):
      return ["type": "usernameAnnounced", "username": username]
    case .usernameAnnouncementFailed(let username, let reason):
      return ["type": "usernameAnnouncementFailed", "username": username, "reason": reason]
    case .chainTipChanged(let height, let hash):
      return ["type": "chainTipChanged", "height": height, "hash": hash]
    case .ledgerSubmissionRejected(let reason):
      return ["type": "ledgerSubmissionRejected", "reason": reason]
    case .usernameOwnerResolved(let username, let ownerPublicKey, let claimedAtHeight):
      // The owner key came from a block already validated by consensus
      // (its claim signature was checked before ever being mined), unlike
      // a raw DHT record — so deriving fingerprint/peerId here can't
      // meaningfully fail in practice; `try?` only guards against a
      // genuinely corrupt key length, not an untrusted one.
      return [
        "type": "usernameOwnerResolved",
        "username": username,
        "ownerPublicKeyBase64": ownerPublicKey.base64EncodedString(),
        "claimedAtHeight": claimedAtHeight,
        "fingerprint": (try? identityFingerprintOfPublicKey(publicKey: ownerPublicKey)) ?? "",
        "peerId": (try? p2pPeerIdFromPublicKey(publicKey: ownerPublicKey)) ?? "",
      ]
    case .usernameOwnerNotFound(let username):
      return ["type": "usernameOwnerNotFound", "username": username]
    case .chainSyncCompleted(let height):
      return ["type": "chainSyncCompleted", "height": height]
    case .chainSyncFailed(let peerId, let reason):
      return ["type": "chainSyncFailed", "peerId": peerId, "reason": reason]
    case .newBlockMined(let height):
      return ["type": "newBlockMined", "height": height]
    case .mixPacketArrived(let payload):
      return ["type": "mixPacketArrived", "payload": payload]
    case .mixForwardFailed(let reason):
      return ["type": "mixForwardFailed", "reason": reason]
    case .mixRelayDiscovered(let peerId):
      return ["type": "mixRelayDiscovered", "peerId": peerId]
    case .publicRelayAnnounced:
      return ["type": "publicRelayAnnounced"]
    case .publicRelayAnnouncementFailed(let reason):
      return ["type": "publicRelayAnnouncementFailed", "reason": reason]
    case .publicRelayDiscovered(let peerId):
      return ["type": "publicRelayDiscovered", "peerId": peerId]
    case .mailboxDepositStored:
      return ["type": "mailboxDepositStored"]
    case .mailboxEnvelopeRetrieved(let envelope):
      return ["type": "mailboxEnvelopeRetrieved", "envelope": envelope]
    }
  }
}
