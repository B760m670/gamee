import ExpoModulesCore

enum BlobStoreError: Error {
  case unreadableFile(String)
  case malformedId(String)
}

public class SpiritchatCryptoCoreModule: Module {
  public func definition() -> ModuleDefinition {
    Name("SpiritchatCryptoCore")

    // This device's persistent identity (see IdentitySession.swift):
    // generated once and stored in the Keychain, not regenerated per call.
    Function("fingerprint") { () -> String in
      IdentitySession.shared.fingerprint
    }

    Function("publicKeyBase64") { () -> String in
      IdentitySession.shared.publicKeyBytes.base64EncodedString()
    }

    Events("onP2pEvent")

    // Starts the node and begins pumping its event loop as soon as the
    // module is created, independent of whether JS has attached a
    // listener yet — an incoming envelope must still be dialed/received
    // even if the chat screen isn't mounted.
    OnCreate {
      Task {
        while let event = await P2pSession.shared.node.nextEvent() {
          self.sendEvent("onP2pEvent", P2pSession.encode(event))
        }
      }
    }

    Function("p2pLocalPeerId") { () -> String in
      P2pSession.shared.node.localPeerId()
    }

    Function("p2pDial") { (peerId: String, knownAddresses: [String]) throws in
      try P2pSession.shared.node.dial(peerId: peerId, knownAddresses: knownAddresses)
    }

    Function("p2pResolvePeerAddresses") { (peerId: String) throws in
      try P2pSession.shared.node.resolvePeerAddresses(peerId: peerId)
    }

    Function("p2pAnnounceAddresses") { (addresses: [String]) throws in
      try P2pSession.shared.node.announceAddresses(addresses: addresses)
    }

    Function("p2pSendEnvelope") { (peerId: String, bytes: Data) throws in
      try P2pSession.shared.node.sendEnvelope(peerId: peerId, bytes: bytes)
    }

    Function("p2pReserveRelaySlot") { (relayAddress: String) throws in
      try P2pSession.shared.node.reserveRelaySlot(relayAddress: relayAddress)
    }

    // Reads `fileUri` (a local file already on disk, e.g. from
    // expo-image-manipulator's cropped output) straight off the
    // filesystem rather than taking bytes over the JS bridge — the image
    // is already a file, so there's no reason to round-trip it through JS
    // as a second copy. Hashes it, caches it under that hash in
    // BlobStore, and registers it with the P2P node so peers can fetch it
    // directly from this device. Returns the hex-encoded content id.
    Function("blobSaveFromFile") { (fileUri: String) throws -> String in
      guard let url = URL(string: fileUri), let bytes = try? Data(contentsOf: url) else {
        throw BlobStoreError.unreadableFile(fileUri)
      }
      let id = blobContentId(bytes: bytes)
      let idHex = id.hexEncoded
      try BlobStore.save(bytes, idHex: idHex)
      try P2pSession.shared.node.setLocalBlob(id: id, bytes: bytes)
      return idHex
    }

    // The local cache path for `idHex`, if this device has it (its own
    // blob, or one fetched from a peer and cached) — nil if not, meaning
    // it needs fetching via p2pFetchBlob first.
    Function("blobLocalPath") { (idHex: String) -> String? in
      BlobStore.exists(idHex: idHex) ? BlobStore.path(for: idHex).absoluteString : nil
    }

    // Stops serving `idHex` to peers and removes the on-disk cache entry.
    Function("blobClear") { (idHex: String) throws in
      guard let id = Data(hexEncoded: idHex) else { throw BlobStoreError.malformedId(idHex) }
      try P2pSession.shared.node.clearLocalBlob(id: id)
      BlobStore.remove(idHex: idHex)
    }

    // Re-registers an already-cached blob with the P2P node — needed once
    // per launch, since the Rust node's local_blobs map starts empty every
    // time (nothing is persisted below the app's own cache). Returns
    // false if `idHex` isn't cached on disk at all.
    Function("blobReserve") { (idHex: String) throws -> Bool in
      guard let bytes = BlobStore.load(idHex: idHex) else { return false }
      guard let id = Data(hexEncoded: idHex) else { throw BlobStoreError.malformedId(idHex) }
      try P2pSession.shared.node.setLocalBlob(id: id, bytes: bytes)
      return true
    }

    // Fetches blob `idHex` from `peerId` (dial first if not connected).
    // Answered by a `blobFetched`/`blobFetchFailed` event on `onP2pEvent`.
    Function("p2pFetchBlob") { (peerId: String, idHex: String) throws in
      guard let id = Data(hexEncoded: idHex) else { throw BlobStoreError.malformedId(idHex) }
      try P2pSession.shared.node.fetchBlob(peerId: peerId, id: id)
    }
  }
}
