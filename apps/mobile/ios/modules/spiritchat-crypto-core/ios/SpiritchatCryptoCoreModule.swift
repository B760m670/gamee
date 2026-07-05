import ExpoModulesCore

enum BlobStoreError: Error {
  case unreadableFile(String)
  case malformedId(String)
}

enum MiningError: Error {
  case malformedPublicKey(String)
}

enum ChatError: Error {
  case malformedPublicKey(String)
  case malformedPlaintext
}

private func requireIdentity() throws -> IdentitySession {
  guard let session = IdentitySession.shared else { throw IdentitySessionError.notYetInitialized }
  return session
}

private func requireP2pSession() throws -> P2pSession {
  guard let session = P2pSession.shared else { throw IdentitySessionError.notYetInitialized }
  return session
}

/// Shared by `signOut` (always the active slot) and `removeAccount` (an
/// explicit slot, which may or may not be active) — permanently deletes
/// `slot`'s Keychain data, tearing down the running P2P node/mining first
/// if `slot` was the one they belonged to, then falls back to whichever
/// other account slot comes first (if any) so removing one account you're
/// still signed into others doesn't force a trip through onboarding.
private func removeAccountSlot(_ slot: Int) throws {
  let wasActive = slot == IdentitySession.activeSlot
  if wasActive {
    // Controllers gate the *active* node — stop them while that node is
    // still the one `P2pSession.shared` resolves to, so stopMining/dummy
    // traffic land on the right session.
    MiningController.shared.stop()
    MixRelayController.shared.stop()
  }
  P2pSession.stop(slot: slot)
  IdentitySession.removeSlot(slot)
  if wasActive, let fallback = IdentitySession.occupiedSlots().first {
    try IdentitySession.switchTo(slot: fallback)
    // The fallback account's node is already running (every slot's is);
    // the supervisor loop re-starts the controllers against it on its
    // next pass.
  }
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

    // Permanently removes the *active* account — the only way back in
    // afterward is its recovery phrase. If another account is also
    // registered on this device (see `accountSlots`), it becomes active
    // automatically instead of dropping into onboarding; `hasIdentity()`
    // only goes false if this was the last one. There is no server
    // session to invalidate; this local wipe is the entire effect.
    Function("signOut") { () throws in
      try removeAccountSlot(IdentitySession.activeSlot)
    }

    // Every account currently registered on this device (up to
    // `IdentitySession.maxSlots`), each in its own Keychain namespace —
    // for an account-switcher UI. Switching (`switchAccount`) is instant
    // and needs no re-authentication, since every slot's full key material
    // already lives in this device's Keychain; the recovery phrase is only
    // needed again to add a slot this device has never seen before.
    Function("accountSlots") { () -> [[String: Any]] in
      IdentitySession.occupiedSlots().map { slot in
        ["slot": slot, "fingerprint": IdentitySession.peekFingerprint(slot: slot) ?? ""]
      }
    }

    Function("activeAccountSlot") { () -> Int in
      IdentitySession.activeSlot
    }

    // Switches to an already-registered `slot` — throws if it's empty.
    // Every slot's node keeps running through a switch (that's the whole
    // point of concurrent multi-account sessions — the outgoing account
    // keeps receiving/retrying in the background); only the mining/mix
    // policy controllers move: stopped here while the old node is still
    // what `P2pSession.shared` resolves to, restarted by the supervisor
    // loop against the new one.
    Function("switchAccount") { (slot: Int) throws in
      MiningController.shared.stop()
      MixRelayController.shared.stop()
      try IdentitySession.switchTo(slot: slot)
    }

    // Permanently removes `slot` regardless of whether it's the active
    // account — see `removeAccountSlot`'s doc comment for the fallback
    // behavior when it is.
    Function("removeAccount") { (slot: Int) throws in
      try removeAccountSlot(slot)
    }

    Events("onP2pEvent", "onChatEvent")

    // The session supervisor: keeps one live node + event pump running
    // per *occupied account slot* (not just the active one — the device
    // being online means every account on it is online), starting new
    // ones as slots appear (launch, onboarding, "add account") and
    // re-starting any that stopped (sign-out then re-restore in the same
    // run). Also keeps the policy controllers attached to whichever slot
    // is currently active, and keeps this device's own nodes dialed into
    // each other (see P2pSession.interconnect). Polls rather than being
    // notified since "slots changed" is a simple, low-frequency state
    // change; a callback mechanism for it would be more machinery than
    // the problem needs.
    OnCreate {
      Task {
        while true {
          for slot in IdentitySession.occupiedSlots() {
            guard let session = P2pSession.session(forSlot: slot), session.claimPump() else { continue }
            self.startEventPump(for: session)
          }
          if P2pSession.shared != nil {
            // Safe to call every pass — both controllers no-op once
            // observing; after an account switch they were stopped, so
            // this is what re-attaches them to the new active node.
            MiningController.shared.start()
            MixRelayController.shared.start()
          }
          P2pSession.interconnect()
          try? await Task.sleep(nanoseconds: 200_000_000)
        }
      }
    }

    Function("p2pLocalPeerId") { () throws -> String in
      try requireP2pSession().node.localPeerId()
    }

    // Whether the P2P/ledger subsystem is currently up — checked (not
    // assumed) rather than inferred from some other call succeeding, since
    // a startup failure there is deliberately non-fatal now (see
    // P2pSession.shared's doc comment) and callers need a real way to tell
    // "not ready yet" from "actually broken" without treating either as
    // an app-breaking error.
    Function("p2pIsReady") { () -> Bool in
      P2pSession.shared != nil
    }

    // The underlying reason the most recent P2P/ledger startup attempt
    // failed, if any — `nil` once it succeeds. This is this app's only
    // window into *why* P2P isn't up in an environment (e.g. sideloaded
    // via LiveContainer) where standard OS crash/diagnostic logs aren't
    // reliably available; surfaced directly in onboarding's error banner
    // rather than requiring a connected Mac or Console access.
    Function("p2pLastStartupErrorDescription") { () -> String? in
      P2pSession.lastStartupError.map { "\($0)" }
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

    // Signs `username` with this identity's key and publishes the claim to
    // the public DHT — nobody else can produce a valid claim for it
    // without this key. This does *not* reserve the name against a
    // determined second claimant (a plain DHT has no way to arbitrate who
    // claimed a name first, the way a blockchain or a server could) —
    // `p2pResolveUsername` it first and treat an existing claim from a
    // *different* key as taken. Re-run periodically (DHT records expire,
    // same as address/blob announcements) and whenever the username
    // changes. Answered by `usernameAnnounced`/`usernameAnnouncementFailed`
    // on `onP2pEvent`.
    Function("p2pAnnounceUsername") { (username: String) throws in
      let session = try requireIdentity()
      let claim = UsernameClaim.build(for: username, identity: session.identity)
      try requireP2pSession().node.announceUsername(username: username, claim: claim)
    }

    // Looks up whatever is currently published for `username`. The
    // resulting event is already verified before it reaches JS (see
    // P2pSession.encode) — `usernameResolved` only fires for a claim whose
    // signature actually checks out; a present-but-invalid claim (tampered
    // or malformed) surfaces as `usernameClaimInvalid`, and nothing
    // published at all as `usernameResolutionFailed`. Search by @username
    // only ever works as an exact lookup — a DHT has no notion of
    // "starts with", so there's no live-search-as-you-type here, only
    // "resolve this exact handle".
    Function("p2pResolveUsername") { (username: String) throws in
      try requireP2pSession().node.resolveUsername(username: username)
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

    // Registers `bytes` under a caller-chosen `idHex` without going through
    // BlobStore's own content-addressing (unlike `blobSaveFromFile`/
    // `blobReserve`, which always derive the id from a hash of the bytes).
    // For small, non-secret, frequently-changing pointers this device wants
    // every peer to be able to fetch under one fixed, well-known id — e.g.
    // "what's my avatar's current content id right now" (see
    // store/peerAvatars.ts) — where content-addressing the pointer itself
    // would be pointless (its whole purpose is to be look-up-able without
    // already knowing what it contains). Not persisted by this crate any
    // more than any other locally-served blob is; the caller is
    // responsible for re-registering it (e.g. on every launch and every
    // time the underlying value changes).
    Function("p2pSetLocalBlobRaw") { (idHex: String, bytes: Data) throws in
      guard let id = Data(hexEncoded: idHex) else { throw BlobStoreError.malformedId(idHex) }
      try requireP2pSession().node.setLocalBlob(id: id, bytes: bytes)
    }

    // Encrypts `json` (the active account's profile/contacts snapshot,
    // assembled in JS — see store/profile.ts) under a key only this
    // account's recovery-phrase holder can derive, and publishes the
    // ciphertext into the public DHT keyed by the identity public key.
    // What makes "restore from phrase" bring the account's *data* back,
    // not just its keys — with no server holding anything readable.
    // Re-run periodically and on every profile/contacts change. Answered
    // by `recoveryBackupAnnounced`/`recoveryBackupAnnouncementFailed` on
    // `onP2pEvent`.
    Function("recoveryBackupPublish") { (json: String) throws in
      let identity = try requireIdentity()
      let ciphertext = try recoveryBackupEncrypt(
        identitySeed: identity.identity.secretBytes(),
        plaintext: Data(json.utf8)
      )
      try requireP2pSession().node.announceRecoveryBackup(
        ownerIdentityPublicKey: identity.publicKeyBytes,
        backup: ciphertext
      )
    }

    // Asks the DHT for this account's own published recovery backup —
    // what a fresh install runs right after restoring from a phrase. The
    // event pump decrypts a found record natively and delivers it to JS
    // as `recoveryBackupRestored` (already-decrypted JSON); a record that
    // doesn't authenticate under this account's key surfaces as
    // `recoveryBackupResolutionFailed`, same as none existing.
    Function("recoveryBackupRequest") { () throws in
      let identity = try requireIdentity()
      try requireP2pSession().node.resolveRecoveryBackup(ownerIdentityPublicKey: identity.publicKeyBytes)
    }

    // Publishes this device's own current avatar content id into the
    // public DHT, keyed by its own peer id — the pointer only, not the
    // avatar bytes (those still need `p2pFetchBlob` over a live
    // connection). `avatarContentId` is opaque bytes to this layer (see
    // store/peerAvatars.ts for what it encodes). Answered by
    // `avatarPointerAnnounced`/`avatarPointerAnnouncementFailed` on
    // `onP2pEvent`.
    Function("p2pAnnounceAvatarPointer") { (avatarContentId: Data) throws in
      try requireP2pSession().node.announceAvatarPointer(avatarContentId: avatarContentId)
    }

    // Looks up whatever avatar content id `peerId` currently has published
    // in the DHT — lets a caller learn which id to `p2pFetchBlob` even
    // while `peerId` is offline right now. Answered by
    // `avatarPointerResolved`/`avatarPointerResolutionFailed`.
    Function("p2pResolveAvatarPointer") { (peerId: String) throws in
      try requireP2pSession().node.resolveAvatarPointer(peerId: peerId)
    }

    // Builds and signs a new @username ledger claim — pure (nothing sent
    // anywhere yet); pass the result to `p2pSubmitUsernameClaim`. Claim-
    // building lives in Rust (not hand-encoded here, unlike the old DHT
    // claim's simple concat) since the ledger's signed preimage is more
    // structured. `nonce` should be 8 fresh random bytes per call.
    Function("ledgerBuildUsernameClaim") { (
      username: String, anchorHeight: UInt64, anchorBlockHash: Data, nonce: Data
    ) throws -> Data in
      let identity = try requireIdentity().identity
      return try ledgerBuildUsernameClaim(
        identity: identity,
        username: username,
        anchorHeight: anchorHeight,
        anchorBlockHash: anchorBlockHash,
        nonce: nonce
      )
    }

    // Broadcasts an already-built claim (see `ledgerBuildUsernameClaim`)
    // to the ledger's mempool topic. Does not by itself confirm the name
    // — watch `chainTipChanged` and re-check via `p2pQueryUsernameOwner`.
    Function("p2pSubmitUsernameClaim") { (transactionBytes: Data) throws in
      try requireP2pSession().node.submitUsernameClaim(transactionBytes: transactionBytes)
    }

    // Submits an already-mined ledger block — for a future mining loop
    // and for testing; validates and applies it locally like a gossiped
    // block, then gossips it onward.
    Function("p2pSubmitMinedBlock") { (blockBytes: Data) throws in
      try requireP2pSession().node.submitMinedBlock(blockBytes: blockBytes)
    }

    // Answers from this node's own local materialized ledger state only
    // — no network round trip. Answered by `usernameOwnerResolved`/
    // `usernameOwnerNotFound` on `onP2pEvent`.
    Function("p2pQueryUsernameOwner") { (username: String) throws in
      try requireP2pSession().node.queryUsernameOwner(username: username)
    }

    // Catches this node up to `peerId`'s ledger chain tip if it's heavier
    // than this node's own. Dial first if not already connected. Answered
    // by `chainSyncCompleted`/`chainSyncFailed` on `onP2pEvent`.
    Function("p2pRequestChainSync") { (peerId: String) throws in
      try requireP2pSession().node.requestChainSync(peerId: peerId)
    }

    // This node's own current ledger chain tip — needed as the anchor for
    // a new claim (see `ledgerBuildUsernameClaim`). Answered synchronously
    // by a `chainTipChanged` event on `onP2pEvent`.
    Function("p2pQueryChainTip") { () throws in
      try requireP2pSession().node.queryChainTip()
    }

    // Starts (or restarts) this node's mining loop, attributing any block
    // it mines to `publicKeyBase64` (an attribution key, not necessarily
    // this device's own identity key — base64 to match every other public
    // key that crosses this bridge, e.g. `publicKeyBase64()`). Runs
    // continuously until `p2pStopMining`. Kept exposed for tests/tooling
    // even though normal operation never needs it directly — see
    // `MiningController`, which is what actually calls this (and
    // `p2pStopMining`), gated on foreground+charging, without any JS
    // involvement. Successful blocks surface as `newBlockMined` on
    // `onP2pEvent`.
    Function("p2pStartMining") { (publicKeyBase64: String) throws in
      guard let publicKey = Data(base64Encoded: publicKeyBase64) else {
        throw MiningError.malformedPublicKey(publicKeyBase64)
      }
      try requireP2pSession().node.startMining(publicKey: publicKey)
    }

    // Stops mining started by `p2pStartMining`. A no-op if not currently
    // mining.
    Function("p2pStopMining") { () throws in
      try requireP2pSession().node.stopMining()
    }

    // Whether this device offers to relay/announce for the Sphinx/Loopix
    // mixnet at all — the Settings toggle backing `MixRelayController`.
    // Defaults to on; setting it re-evaluates immediately (see
    // `MixRelayController.participationEnabled`'s own doc comment), so a
    // user turning this off in Settings stops sustained dummy-traffic
    // generation right away rather than waiting for the next foreground/
    // charging transition.
    Function("mixRelayParticipationEnabled") { () -> Bool in
      MixRelayController.shared.participationEnabled
    }

    Function("setMixRelayParticipationEnabled") { (enabled: Bool) in
      MixRelayController.shared.participationEnabled = enabled
    }

    // An honest, lower-bound estimate of this device's own background
    // data cost from dummy (cover/loop) mix traffic alone, at the
    // interval the native side actually uses — for the same Settings row
    // as the toggle above, so a user deciding whether to opt out sees a
    // real number, not just a description.
    Function("mixDummyTrafficBytesPerHourEstimate") { () -> UInt64 in
      p2pEstimatedMixDummyTrafficBytesPerHour()
    }

    // Queues `plaintext` for `peerId` and starts delivering it immediately
    // — durable on disk before this returns, so it's never lost even if
    // the peer is offline or this device restarts before it goes out (see
    // ChatManager/ChatStore). `peerPublicKeyBase64` is required the first
    // time this device messages `peerId` (X3DH needs it to fetch/verify
    // their contact card); once a session exists it's only used to keep
    // the local record consistent. Returns a local id — match it against
    // `messageSent`/`messageFailed` on `onChatEvent` to update that
    // message's status; `messageReceived` on the same event answers
    // incoming messages, sender-initiated or not.
    Function("chatSendMessage") { (peerId: String, peerPublicKeyBase64: String, plaintext: String) throws -> String in
      guard let publicKey = Data(base64Encoded: peerPublicKeyBase64) else {
        throw ChatError.malformedPublicKey(peerPublicKeyBase64)
      }
      guard let plaintextBytes = plaintext.data(using: .utf8) else {
        throw ChatError.malformedPlaintext
      }
      return try requireP2pSession().chatManager.sendMessage(peerId: peerId, peerPublicKey: publicKey, plaintext: plaintextBytes)
    }

    // Creates a group named `name` with `memberPeerIds` as its initial
    // members — every one of them must already be an existing 1:1 contact
    // (see ChatManager's own "MARK: - Groups" doc comment for this v1's
    // scope). Returns the new group's id, or throws if this device's own
    // identity isn't ready yet. `groupInvited`/`groupMessageReceived` on
    // `onChatEvent` answer what happens next for everyone else.
    Function("chatCreateGroup") { (name: String, memberPeerIds: [String]) throws -> String in
      guard let groupId = try requireP2pSession().chatManager.createGroup(name: name, memberPeerIds: memberPeerIds) else {
        throw IdentitySessionError.notYetInitialized
      }
      return groupId
    }

    // Encrypts `plaintext` once (Sender Keys) and queues it for delivery
    // to every other member of `groupId` — durable and retried on
    // reconnect the same way `chatSendMessage` already is. Returns a
    // local id; throws if this device isn't a member of `groupId` or its
    // plaintext isn't valid UTF-8.
    Function("chatSendGroupMessage") { (groupId: String, plaintext: String) throws -> String in
      guard let plaintextBytes = plaintext.data(using: .utf8) else {
        throw ChatError.malformedPlaintext
      }
      guard let localId = try requireP2pSession().chatManager.sendGroupMessage(groupId: groupId, plaintext: plaintextBytes) else {
        throw IdentitySessionError.notYetInitialized
      }
      return localId
    }

    // Adds `newMemberPeerId` (who must already be an existing 1:1
    // contact) to `groupId`. `groupMemberAdded` on `onChatEvent` confirms
    // it locally; the new member gets a `groupInvited` once their own
    // invite arrives.
    Function("chatAddGroupMember") { (groupId: String, newMemberPeerId: String) throws in
      try requireP2pSession().chatManager.addGroupMember(groupId: groupId, newMemberPeerId: newMemberPeerId)
    }

    // Removes `memberToRemove` from `groupId` and rotates this device's
    // own Sender Key chain (every remaining member does the same
    // independently) so the removed member can't decrypt anything sent
    // afterward. `groupMemberRemoved` on `onChatEvent` confirms it locally.
    Function("chatRemoveGroupMember") { (groupId: String, memberToRemove: String) throws in
      try requireP2pSession().chatManager.removeGroupMember(groupId: groupId, memberToRemove: memberToRemove)
    }
  }

  /// One per `P2pSession`: drains that node's events until it shuts down.
  /// Every event crossing to JS is tagged with the session's slot and
  /// fingerprint, so the stores can route an inactive account's traffic
  /// into that account's own namespaced storage (see store/chat.ts) and
  /// the UI can ignore P2P events that aren't the active account's.
  private func startEventPump(for session: P2pSession) {
    session.chatManager.emit = { [weak self] event in
      var tagged = event
      tagged["slot"] = session.slot
      tagged["selfFingerprint"] = session.selfFingerprint
      self?.sendEvent("onChatEvent", tagged)
    }
    Task {
      while let event = await session.node.nextEvent() {
        if case .listeningOn(let address) = event {
          session.recordListenAddress(address)
        }
        var encoded = P2pSession.encode(event)
        if case .recoveryBackupResolved(_, let backup) = event {
          // Decrypt natively — JS should only ever see the plaintext
          // snapshot (or a clean failure), never ciphertext + key
          // material. A record that doesn't authenticate under this
          // account's key is someone else's or tampered: exactly
          // equivalent to no backup existing.
          if let plaintext = try? recoveryBackupDecrypt(identitySeed: session.identitySeed, backupBytes: backup),
             let json = String(data: plaintext, encoding: .utf8) {
            encoded = ["type": "recoveryBackupRestored", "json": json]
          } else {
            encoded = ["type": "recoveryBackupResolutionFailed"]
          }
        }
        encoded["slot"] = session.slot
        encoded["selfFingerprint"] = session.selfFingerprint
        self.sendEvent("onP2pEvent", encoded)
        session.chatManager.handleP2pEvent(event)
      }
      // The node shut down (sign out / account removal) — this pump ends
      // with it; a re-created slot gets a fresh session object and a
      // fresh pump from the supervisor loop.
    }
  }
}
