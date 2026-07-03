import Foundation

/// This device's live network presence: one libp2p node, started once per
/// app run using the same 32-byte seed as `IdentitySession`'s identity key
/// (see `spiritchat_p2p_core::identity::keypair_from_seed`), so a peer's
/// `PeerId` on the wire always matches the identity in their contact card.
final class P2pSession {
  // Guards `cached` against a real race: the native event-pump loop (see
  // SpiritchatCryptoCoreModule's OnCreate) polls `shared` from a background
  // Task every 200ms, while JS calls functions like `p2pLocalPeerId()` —
  // routed through Expo's own (not-necessarily-main) dispatch queue —
  // moments after `setIdentityFromWords` resolves during onboarding. Both
  // can see `cached == nil` at once. Before this class opened a real file
  // on disk (see `ledgerDatabasePath`, added alongside the `@username`
  // ledger), the worst outcome of that race was a wasted, harmless second
  // `FfiP2pNode` — now it means two `ChainStore::open()` calls racing for
  // the same `redb` file, which can fail with a lock conflict. Only ever
  // held for the duration of the check-then-create (or check-then-clear)
  // below, never across a call into `node` itself.
  private static let lock = NSLock()
  private static var cached: P2pSession?

  /// The error from the most recent failed start attempt, if any — not
  /// acted on by this class itself, just somewhere a debug screen (or a
  /// developer attached to the console) can find *some* signal for why P2P
  /// isn't up, given this environment doesn't reliably surface standard
  /// crash reports (see `shared`'s doc comment on why this class no longer
  /// treats a start failure as fatal).
  private(set) static var lastStartupError: Error?

  let node: FfiP2pNode
  /// Owns end-to-end-encrypted messaging for this session — created fresh
  /// alongside `node` so it's always torn down and rebuilt together with
  /// it (sign-out, account switch), never left pointing at a stale node or
  /// identity. See `ChatManager`'s own doc comment for what it does.
  let chatManager: ChatManager

  private init(node: FfiP2pNode, chatManager: ChatManager) {
    self.node = node
    self.chatManager = chatManager
  }

  /// Where the `@username` ledger's on-disk `redb` database file lives —
  /// mirrors `BlobStore.swift`'s own `applicationSupportDirectory`
  /// convention, but points at a single file, not a directory (`redb`
  /// creates/opens one file, the same as any other embedded database).
  /// One shared file (not per-identity): the ledger is a single public
  /// chain everyone eventually converges on, not per-account state the
  /// way Keychain-stored secrets are.
  private static var ledgerDatabasePath: URL {
    let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
    let dir = base.appendingPathComponent("Ledger", isDirectory: true)
    do {
      try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    } catch {
      // Not treated as fatal here either — `ChainStore::open` (Rust) will
      // itself fail loudly a moment later if the directory genuinely isn't
      // usable, and `shared` below is what actually decides how to react
      // to that. Logged so the underlying reason isn't silently lost.
      NSLog("[P2pSession] failed to create the ledger directory at \(dir.path): \(error)")
    }
    return dir.appendingPathComponent("ledger.redb")
  }

  /// `nil` until `IdentitySession.shared` exists (there's nothing to start
  /// before onboarding produces one) **or** if the most recent attempt to
  /// spawn the node failed. A failure is deliberately not fatal — bringing
  /// down the entire app over the P2P/ledger subsystem specifically (out
  /// of every subsystem this app has) would mean a device or environment
  /// this one piece doesn't like (a locked-out ledger file, a networking
  /// permission the OS won't grant, anything else in
  /// `spiritchat_p2p_core::P2pNode::spawn`) makes the whole app unusable
  /// instead of just leaving P2P/username features degraded. Every caller
  /// of `shared` already treats `nil` as "not ready yet" (see
  /// `requireP2pSession()` in SpiritchatCryptoCoreModule), which reads as a
  /// normal, catchable error to JS rather than a crash. Not cached as a
  /// permanent failure — retried on the next access (the OnCreate polling
  /// loop tries again every 200ms) since a cause like a transient file
  /// lock isn't necessarily permanent.
  static var shared: P2pSession? {
    lock.lock()
    defer { lock.unlock() }
    if let cached { return cached }
    guard let identitySession = IdentitySession.shared else { return nil }
    do {
      let node = try FfiP2pNode.spawn(
        identitySeed: identitySession.identity.secretBytes(),
        ledgerDataDir: Self.ledgerDatabasePath.path
      )
      let chatManager = ChatManager(
        slot: identitySession.slot,
        node: node,
        identity: identitySession.identity,
        agreement: identitySession.agreement,
        prekeys: identitySession.prekeys
      )
      let session = P2pSession(node: node, chatManager: chatManager)
      cached = session
      lastStartupError = nil
      return session
    } catch {
      lastStartupError = error
      NSLog("[P2pSession] failed to start the P2P node: \(error)")
      return nil
    }
  }

  /// Stops the node and forgets it — for "sign out", where the identity
  /// this node was built from is going away. A subsequent `shared` access
  /// (once a new identity exists) then starts a genuinely fresh node
  /// instead of reusing this one's now-meaningless connections/DHT state.
  /// Safe to call even if no node was ever started.
  static func signOut() {
    lock.lock()
    defer { lock.unlock() }
    try? cached?.node.shutdown()
    cached = nil
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
    case .mailboxDepositStored:
      return ["type": "mailboxDepositStored"]
    }
  }
}
