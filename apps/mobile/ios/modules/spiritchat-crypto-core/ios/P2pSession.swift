import Foundation

/// This device's live network presence: one libp2p node, started once per
/// app run using the same 32-byte seed as `IdentitySession`'s identity key
/// (see `spiritchat_p2p_core::identity::keypair_from_seed`), so a peer's
/// `PeerId` on the wire always matches the identity in their contact card.
final class P2pSession {
  private static var cached: P2pSession?

  let node: FfiP2pNode

  private init(identitySeed: Data) {
    do {
      node = try FfiP2pNode.spawn(identitySeed: identitySeed, ledgerDataDir: Self.ledgerDatabasePath.path)
    } catch {
      // Mirrors IdentitySession's fatalError: a node that silently failed
      // to start would leave the app looking connected while never sending
      // or receiving anything.
      fatalError("Failed to start the P2P node: \(error)")
    }
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
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir.appendingPathComponent("ledger.redb")
  }

  /// `nil` until `IdentitySession.shared` exists — the node's identity
  /// comes from the exact same seed, so there is nothing to start until
  /// onboarding (create or restore) has produced one.
  static var shared: P2pSession? {
    if let cached { return cached }
    guard let identitySession = IdentitySession.shared else { return nil }
    let session = P2pSession(identitySeed: identitySession.identity.secretBytes())
    cached = session
    return session
  }

  /// Stops the node and forgets it — for "sign out", where the identity
  /// this node was built from is going away. A subsequent `shared` access
  /// (once a new identity exists) then starts a genuinely fresh node
  /// instead of reusing this one's now-meaningless connections/DHT state.
  /// Safe to call even if no node was ever started.
  static func signOut() {
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
      return [
        "type": "usernameOwnerResolved",
        "username": username,
        "ownerPublicKeyBase64": ownerPublicKey.base64EncodedString(),
        "claimedAtHeight": claimedAtHeight,
      ]
    case .usernameOwnerNotFound(let username):
      return ["type": "usernameOwnerNotFound", "username": username]
    case .chainSyncCompleted(let height):
      return ["type": "chainSyncCompleted", "height": height]
    case .chainSyncFailed(let peerId, let reason):
      return ["type": "chainSyncFailed", "peerId": peerId, "reason": reason]
    }
  }
}
