import ExpoModulesCore

enum BlobStoreError: Error {
  case unreadableFile(String)
  case malformedId(String)
}

private func requireIdentity() throws -> IdentitySession {
  guard let session = IdentitySession.shared else { throw IdentitySessionError.notYetInitialized }
  return session
}

private func requireP2pSession() throws -> P2pSession {
  guard let session = P2pSession.shared else { throw IdentitySessionError.notYetInitialized }
  return session
}

public class SpiritchatCryptoCoreModule: Module {
  public func definition() -> ModuleDefinition {
    Name("SpiritchatCryptoCore")

    // Whether an identity already exists on this device — the JS layer
    // checks this on launch to decide between onboarding (create/restore)
    // and going straight to the app. None of the functions below this
    // point work until it's true.
    Function("hasIdentity") { () -> Bool in
      IdentitySession.hasStoredIdentity()
    }

    // Generates a brand-new 12-word BIP39 recovery phrase. Pure — nothing
    // is persisted yet; the words only become the device's identity once
    // handed back to `setIdentityFromWords` after the user has confirmed
    // they wrote them down.
    Function("generateRecoveryPhrase") { () -> String in
      FfiRecoveryPhrase.generate().words()
    }

    // Creates (or restores) the identity/agreement/prekeys from `words` and
    // persists all of it to the Keychain. Used for both onboarding paths —
    // a phrase this device just generated, or one the user typed back in on
    // a fresh install — since deriving an identity from a phrase works
    // identically either way. Throws if `words` fails BIP39 checksum
    // validation (e.g. a typo when restoring). Returns the resulting
    // fingerprint.
    Function("setIdentityFromWords") { (words: String) throws -> String in
      let phrase = try FfiRecoveryPhrase.fromWords(words: words)
      let session = try IdentitySession.begin(withPhrase: phrase)
      return session.fingerprint
    }

    // The current identity's recovery phrase, if this device still has it
    // cached (see IdentitySession.storedRecoveryPhraseWords) — for
    // Settings → "Show recovery phrase". `nil` isn't expected in practice
    // (it's written at the same time as the identity itself) but isn't
    // treated as an error, since there's nothing actionable to do about it
    // beyond telling the user it's unavailable.
    Function("recoveryPhraseWords") { () -> String? in
      IdentitySession.storedRecoveryPhraseWords()
    }

    // This device's persistent identity (see IdentitySession.swift):
    // created once and stored in the Keychain, not regenerated per call.
    Function("fingerprint") { () throws -> String in
      try requireIdentity().fingerprint
    }

    Function("publicKeyBase64") { () throws -> String in
      try requireIdentity().publicKeyBytes.base64EncodedString()
    }

    Events("onP2pEvent")

    // Starts pumping the P2P node's event loop as soon as an identity
    // exists — immediately on launch if one was already on this device,
    // or the moment onboarding finishes creating/restoring one. Polls
    // rather than being notified because this is a one-time state
    // transition (nil -> non-nil, never back), not a recurring condition;
    // a callback/notification mechanism for something that happens at most
    // once per app launch would be more machinery than the problem needs.
    OnCreate {
      Task {
        while P2pSession.shared == nil {
          try? await Task.sleep(nanoseconds: 200_000_000)
        }
        guard let session = P2pSession.shared else { return }
        while let event = await session.node.nextEvent() {
          self.sendEvent("onP2pEvent", P2pSession.encode(event))
        }
      }
    }

    Function("p2pLocalPeerId") { () throws -> String in
      try requireP2pSession().node.localPeerId()
    }

    Function("p2pDial") { (peerId: String, knownAddresses: [String]) throws in
      try requireP2pSession().node.dial(peerId: peerId, knownAddresses: knownAddresses)
    }

    Function("p2pResolvePeerAddresses") { (peerId: String) throws in
      try requireP2pSession().node.resolvePeerAddresses(peerId: peerId)
    }

    Function("p2pAnnounceAddresses") { (addresses: [String]) throws in
      try requireP2pSession().node.announceAddresses(addresses: addresses)
    }

    Function("p2pSendEnvelope") { (peerId: String, bytes: Data) throws in
      try requireP2pSession().node.sendEnvelope(peerId: peerId, bytes: bytes)
    }

    Function("p2pReserveRelaySlot") { (relayAddress: String) throws in
      try requireP2pSession().node.reserveRelaySlot(relayAddress: relayAddress)
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
      try requireP2pSession().node.setLocalBlob(id: id, bytes: bytes)
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
      try requireP2pSession().node.clearLocalBlob(id: id)
      BlobStore.remove(idHex: idHex)
    }

    // Re-registers an already-cached blob with the P2P node — needed once
    // per launch, since the Rust node's local_blobs map starts empty every
    // time (nothing is persisted below the app's own cache). Returns
    // false if `idHex` isn't cached on disk at all.
    Function("blobReserve") { (idHex: String) throws -> Bool in
      guard let bytes = BlobStore.load(idHex: idHex) else { return false }
      guard let id = Data(hexEncoded: idHex) else { throw BlobStoreError.malformedId(idHex) }
      try requireP2pSession().node.setLocalBlob(id: id, bytes: bytes)
      return true
    }

    // Fetches blob `idHex` from `peerId` (dial first if not connected).
    // Answered by a `blobFetched`/`blobFetchFailed` event on `onP2pEvent`.
    Function("p2pFetchBlob") { (peerId: String, idHex: String) throws in
      guard let id = Data(hexEncoded: idHex) else { throw BlobStoreError.malformedId(idHex) }
      try requireP2pSession().node.fetchBlob(peerId: peerId, id: id)
    }
  }
}
