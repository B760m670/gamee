import { requireNativeModule } from 'expo-modules-core'

/**
 * Which account's node an event came from. Every registered account runs
 * its own live node now (not just the active one), so every event that
 * crosses the bridge is stamped with its session's slot + fingerprint by
 * the native pump — `addP2pEventListener` uses `slot` to drop inactive
 * accounts' node chatter before the active-account UI ever sees it, and
 * store/chat.ts uses `selfFingerprint` to route an inactive account's
 * messages into that account's own namespaced storage.
 */
export type EventOrigin = { slot?: number; selfFingerprint?: string }

/**
 * Mirrors `spiritchat_crypto_core_ffi::FfiP2pEvent` — see p2p_event.rs.
 * Discriminated by `type` so the native side can stay a plain dictionary
 * instead of needing a second binding layer.
 */
export type P2pEvent = EventOrigin & (
  | { type: 'listeningOn'; address: string }
  | { type: 'peerDiscoveredLocally'; peerId: string }
  | { type: 'peerConnected'; peerId: string }
  | { type: 'peerIdentified'; peerId: string }
  | { type: 'peerDisconnected'; peerId: string }
  | { type: 'dialFailed'; peerId: string | null; reason: string }
  | { type: 'relayReservationFailed'; reason: string }
  | { type: 'envelopeReceived'; fromPeerId: string; bytes: Uint8Array }
  | { type: 'envelopeDelivered'; toPeerId: string }
  | { type: 'envelopeDeliveryFailed'; toPeerId: string; reason: string }
  | { type: 'peerAddressesResolved'; peerId: string; addresses: string[] }
  | { type: 'peerAddressResolutionFailed'; peerId: string }
  | { type: 'addressesAnnounced' }
  | { type: 'addressAnnouncementFailed'; reason: string }
  | { type: 'blobFetched'; peerId: string; id: string; localPath: string }
  | { type: 'blobFetchFailed'; peerId: string; id: string; reason: string }
  | { type: 'contactCardAnnounced' }
  | { type: 'contactCardAnnouncementFailed'; reason: string }
  | { type: 'contactCardResolved'; ownerIdentityPublicKeyBase64: string; card: Uint8Array }
  | { type: 'contactCardResolutionFailed'; ownerIdentityPublicKeyBase64: string }
  | { type: 'avatarPointerAnnounced' }
  | { type: 'avatarPointerAnnouncementFailed'; reason: string }
  | { type: 'avatarPointerResolved'; peerId: string; avatarContentId: Uint8Array }
  | { type: 'avatarPointerResolutionFailed'; peerId: string }
  | { type: 'usernameResolved'; username: string; publicKeyBase64: string; fingerprint: string; peerId: string }
  | { type: 'usernameClaimInvalid'; username: string }
  | { type: 'usernameResolutionFailed'; username: string }
  | { type: 'usernameAnnounced'; username: string }
  | { type: 'usernameAnnouncementFailed'; username: string; reason: string }
  | { type: 'chainTipChanged'; height: number; hash: string }
  | { type: 'ledgerSubmissionRejected'; reason: string }
  | {
      type: 'usernameOwnerResolved'
      username: string
      ownerPublicKeyBase64: string
      claimedAtHeight: number
      fingerprint: string
      peerId: string
    }
  | { type: 'usernameOwnerNotFound'; username: string }
  | { type: 'chainSyncCompleted'; height: number }
  | { type: 'chainSyncFailed'; peerId: string; reason: string }
  | { type: 'newBlockMined'; height: number }
  | { type: 'mixPacketArrived'; payload: Uint8Array }
  | { type: 'mixForwardFailed'; reason: string }
  | { type: 'mixRelayDiscovered'; peerId: string }
  | { type: 'publicRelayAnnounced' }
  | { type: 'publicRelayAnnouncementFailed'; reason: string }
  | { type: 'publicRelayDiscovered'; peerId: string }
  | { type: 'mailboxDepositStored' }
  | { type: 'mailboxEnvelopeRetrieved'; envelope: Uint8Array }
  | { type: 'recoveryBackupAnnounced' }
  | { type: 'recoveryBackupAnnouncementFailed'; reason: string }
  // Raw form — normally never reaches JS: the native pump decrypts it
  // and delivers `recoveryBackupRestored` instead.
  | { type: 'recoveryBackupResolved'; ownerIdentityPublicKeyBase64: string; backup: Uint8Array }
  | { type: 'recoveryBackupResolutionFailed'; ownerIdentityPublicKeyBase64?: string }
  /** A found backup, already decrypted natively — `json` is the snapshot `recoveryBackupPublish` was given. */
  | { type: 'recoveryBackupRestored'; json: string }
)

/**
 * Synthesized by `ChatManager.swift` from decrypted/queued messages — not a
 * mirror of a Rust `P2pEvent` the way `P2pEvent` above is, since the
 * crypto/session logic behind these lives entirely on the Swift side (JS
 * never sees raw envelope bytes or handshake data, only these results).
 */
export type ChatEvent = EventOrigin & (
  | { type: 'messageReceived'; peerId: string; peerFingerprint: string; peerPublicKeyBase64: string; plaintext: string; at: number }
  | { type: 'messageSent'; peerId: string; localId: string }
  | { type: 'messageFailed'; peerId: string; localId: string; reason: string }
  | { type: 'groupInvited'; groupId: string; name: string; members: string[] }
  | { type: 'groupMessageReceived'; groupId: string; senderPeerId: string; plaintext: string; at: number }
  | { type: 'groupMemberAdded'; groupId: string; memberPeerId: string }
  | { type: 'groupMemberRemoved'; groupId: string; memberPeerId: string }
  // A media message (photo/video/voice) that finished downloading and
  // decrypting — `localPath` is a file:// URL to the decrypted media on
  // disk. `groupId` is set for group media, null for 1:1.
  | {
      type: 'mediaReceived'
      peerId: string
      groupId: string | null
      localPath: string
      mime: string
      filename: string | null
      durationMs: number | null
      totalSize: number
      at: number
    }
)

type NativeEvents = {
  onP2pEvent(event: P2pEvent): void
  onChatEvent(event: ChatEvent): void
}

const NativeCryptoCore = requireNativeModule<
  {
    hasIdentity(): boolean
    recoveryBackupPublish(json: string): void
    recoveryBackupRequest(): void
    generateRecoveryPhrase(): string
    setIdentityFromWords(words: string): string
    recoveryPhraseWords(): string | null
    fingerprint(): string
    publicKeyBase64(): string
    signOut(): void
    accountSlots(): { slot: number; fingerprint: string }[]
    activeAccountSlot(): number
    switchAccount(slot: number): void
    removeAccount(slot: number): void
    p2pLocalPeerId(): string
    p2pIsReady(): boolean
    p2pLastStartupErrorDescription(): string | null
    p2pDial(peerId: string, knownAddresses: string[]): void
    p2pResolvePeerAddresses(peerId: string): void
    p2pAnnounceAddresses(addresses: string[]): void
    p2pSendEnvelope(peerId: string, bytes: Uint8Array): void
    p2pReserveRelaySlot(relayAddress: string): void
    p2pAnnounceUsername(username: string): void
    p2pResolveUsername(username: string): void
    blobSaveFromFile(fileUri: string): string
    blobLocalPath(idHex: string): string | null
    blobClear(idHex: string): void
    blobReserve(idHex: string): boolean
    p2pFetchBlob(peerId: string, idHex: string): void
    p2pSetLocalBlobRaw(idHex: string, bytes: Uint8Array): void
    p2pAnnounceAvatarPointer(avatarContentId: Uint8Array): void
    p2pResolveAvatarPointer(peerId: string): void
    ledgerBuildUsernameClaim(username: string, anchorHeight: number, anchorBlockHash: Uint8Array, nonce: Uint8Array): Uint8Array
    p2pSubmitUsernameClaim(transactionBytes: Uint8Array): void
    p2pSubmitMinedBlock(blockBytes: Uint8Array): void
    p2pQueryUsernameOwner(username: string): void
    p2pRequestChainSync(peerId: string): void
    p2pQueryChainTip(): void
    p2pStartMining(publicKeyBase64: string): void
    p2pStopMining(): void
    mixRelayParticipationEnabled(): boolean
    setMixRelayParticipationEnabled(enabled: boolean): void
    mixDummyTrafficBytesPerHourEstimate(): number
    chatSendMessage(peerId: string, peerPublicKeyBase64: string, plaintext: string): string
    chatSendMedia(peerId: string, peerPublicKeyBase64: string, fileUri: string, mime: string, filename: string | null, durationMs: number | null): string
    chatSendGroupMedia(groupId: string, fileUri: string, mime: string, filename: string | null, durationMs: number | null): string
    hasMicrophonePermission(): boolean
    requestMicrophonePermission(): Promise<boolean>
    voiceRecordingStart(): string
    voiceRecordingStop(): { fileUri: string; durationMs: number } | null
    voiceRecordingCancel(): void
    chatCreateGroup(name: string, memberPeerIds: string[]): string
    chatSendGroupMessage(groupId: string, plaintext: string): string
    chatAddGroupMember(groupId: string, newMemberPeerId: string): void
    chatRemoveGroupMember(groupId: string, memberToRemove: string): void
    addListener<EventName extends keyof NativeEvents>(
      eventName: EventName,
      listener: NativeEvents[EventName]
    ): { remove(): void }
  } & object
>('SpiritchatCryptoCore')

/**
 * Whether an identity already exists on this device. Check this on launch
 * to decide between onboarding (create a new account / restore from a
 * recovery phrase) and going straight into the app — every other function
 * here throws until this is true.
 */
export function hasIdentity(): boolean {
  return NativeCryptoCore.hasIdentity()
}

/**
 * Generates a brand-new 12-word BIP39 recovery phrase. Pure — nothing is
 * persisted yet. Show it to the user, have them confirm they've written it
 * down, then call `setIdentityFromWords` with the same words to actually
 * create the account.
 */
export function generateRecoveryPhrase(): string {
  return NativeCryptoCore.generateRecoveryPhrase()
}

/**
 * Creates (or restores) this device's identity from `words` and persists
 * it — there is no server anywhere in this project that could offer a
 * "reset password" link, so this phrase is the only account-recovery
 * mechanism there will ever be, the same as a cryptocurrency wallet's seed
 * phrase. Works identically whether `words` came from
 * `generateRecoveryPhrase` moments ago (new account) or was typed back in
 * on a fresh install (recovery). Throws if the words fail BIP39 checksum
 * validation (e.g. a typo). Returns the resulting fingerprint.
 *
 * Recovering an account restores the same fingerprint/PeerId, not the same
 * conversations — the agreement key and prekeys are freshly generated
 * every time this runs (deliberately not derived from the phrase, to
 * preserve forward secrecy), so existing contacts will need to
 * re-handshake, the same way losing a Signal-linked device does.
 */
export function setIdentityFromWords(words: string): string {
  return NativeCryptoCore.setIdentityFromWords(words)
}

/**
 * This device's recovery phrase, if still cached from when the identity
 * was created/restored — for a Settings screen letting the user view it
 * again. Null only if it somehow wasn't persisted alongside the identity
 * (not expected in normal operation).
 */
export function recoveryPhraseWords(): string | null {
  return NativeCryptoCore.recoveryPhraseWords()
}

/**
 * This device's persistent identity fingerprint ("1234 5678 9012"). The
 * underlying identity/agreement keys and prekey store are generated once
 * and stored in the iOS Keychain (see IdentitySession.swift) — this value
 * is stable across app restarts, not regenerated on every call. Throws if
 * `hasIdentity()` is false — call `setIdentityFromWords` first.
 */
export function fingerprint(): string {
  return NativeCryptoCore.fingerprint()
}

/** This device's public identity key, base64-encoded. */
export function publicKeyBase64(): string {
  return NativeCryptoCore.publicKeyBase64()
}

/**
 * Permanently wipes the *active* account's identity, agreement key,
 * prekeys, and cached recovery phrase, and stops the P2P node built from
 * them. There is no server session to invalidate — this local wipe is the
 * entire effect. The only way back in afterward is `setIdentityFromWords`
 * with its recovery phrase; if it wasn't saved, this is permanent.
 *
 * If another account is also registered on this device (see
 * `accountSlots`), it becomes active automatically — `hasIdentity()` only
 * goes false if this was the last one, so the caller should check it
 * afterward rather than assuming a trip back to onboarding is always
 * needed.
 */
export function signOut(): void {
  NativeCryptoCore.signOut()
}

export type AccountSlot = { slot: number; fingerprint: string }

/**
 * Every account currently registered on this device (up to 3), each with
 * its own Keychain-backed identity — for an account-switcher UI. Order is
 * by slot index, not recency.
 */
export function accountSlots(): AccountSlot[] {
  return NativeCryptoCore.accountSlots()
}

/** Which slot `fingerprint()`/`publicKeyBase64()`/the P2P node currently reflect. */
export function activeAccountSlot(): number {
  return NativeCryptoCore.activeAccountSlot()
}

/**
 * Switches to an already-registered `slot` — instant, no recovery phrase
 * needed, since every registered slot's full key material already lives
 * in this device's Keychain (the phrase is only needed again to add a
 * slot this device has never seen before, via `setIdentityFromWords`).
 * Throws if `slot` has nothing stored in it. Tears down and restarts the
 * P2P node for the new identity — callers should treat this like a fresh
 * `bootstrap()`, not an in-place update.
 */
export function switchAccount(slot: number): void {
  NativeCryptoCore.switchAccount(slot)
}

/**
 * Permanently removes `slot` regardless of whether it's currently active
 * — the only way back in afterward is that slot's own recovery phrase. If
 * it was active, another registered slot (if any) becomes active
 * automatically; see `signOut`'s doc comment for the same fallback
 * behavior.
 */
export function removeAccount(slot: number): void {
  NativeCryptoCore.removeAccount(slot)
}

/**
 * This device's libp2p PeerId, base58-encoded. The node behind it is
 * started once per app run (see P2pSession.swift) using the same identity
 * seed as `fingerprint()`/`publicKeyBase64()` above, and joins the public
 * IPFS DHT — there is no server or relay this project operates.
 */
export function p2pLocalPeerId(): string {
  return NativeCryptoCore.p2pLocalPeerId()
}

/**
 * Whether the P2P/ledger node is currently up. A P2P startup failure is
 * deliberately non-fatal to the rest of the app (see `P2pSession.shared`'s
 * doc comment) and is retried automatically in the background, so this is
 * "not yet" rather than "never" — check it instead of assuming any P2P
 * call will succeed just because `hasIdentity()` is true.
 */
export function p2pIsReady(): boolean {
  return NativeCryptoCore.p2pIsReady()
}

/**
 * The underlying reason the P2P/ledger node's most recent startup attempt
 * failed, if any — `null` once it succeeds. This is the only window into
 * *why* P2P isn't up in an environment (e.g. sideloaded via LiveContainer)
 * where standard OS crash/diagnostic logs aren't reliably reachable —
 * onboarding surfaces this directly in its error banner rather than
 * requiring a connected Mac or device console access to diagnose.
 */
export function p2pLastStartupErrorDescription(): string | null {
  return NativeCryptoCore.p2pLastStartupErrorDescription()
}

/** Dials a peer directly at `knownAddresses`, or via the DHT if empty. */
export function p2pDial(peerId: string, knownAddresses: string[] = []): void {
  NativeCryptoCore.p2pDial(peerId, knownAddresses)
}

/** Looks up `peerId`'s current addresses in the DHT; answered by a `peerAddressesResolved`/`peerAddressResolutionFailed` event. */
export function p2pResolvePeerAddresses(peerId: string): void {
  NativeCryptoCore.p2pResolvePeerAddresses(peerId)
}

/** Publishes this node's own addresses to the DHT so other peers can find it. */
export function p2pAnnounceAddresses(addresses: string[]): void {
  NativeCryptoCore.p2pAnnounceAddresses(addresses)
}

/** Sends an already end-to-end-encrypted envelope to a connected peer. Dial first if not already connected. */
export function p2pSendEnvelope(peerId: string, bytes: Uint8Array): void {
  NativeCryptoCore.p2pSendEnvelope(peerId, bytes)
}

/** Asks a relay-capable peer (just another opted-in participant, not project infrastructure) to reserve a slot for NAT traversal. */
export function p2pReserveRelaySlot(relayAddress: string): void {
  NativeCryptoCore.p2pReserveRelaySlot(relayAddress)
}

/**
 * Subscribes to the node's event stream (connections, envelopes, DHT
 * results). Returns an unsubscribe function. Only the *active* account's
 * node events are delivered: every subscriber of this stream drives
 * active-account UI state (profile, avatars, username lookups), and an
 * inactive account's node answering e.g. a username query would corrupt
 * it. Inactive accounts' *messages* still arrive — those flow through
 * `addChatEventListener`, which deliberately does not filter (see
 * store/chat.ts's fingerprint routing).
 */
export function addP2pEventListener(listener: (event: P2pEvent) => void): () => void {
  const subscription = NativeCryptoCore.addListener('onP2pEvent', (event: P2pEvent) => {
    if (event.slot !== undefined && event.slot !== NativeCryptoCore.activeAccountSlot()) return
    listener(event)
  })
  return () => subscription.remove()
}

/**
 * Content-addresses a local file (its bytes are hashed, not read into JS),
 * caches it on-device, and registers it with the P2P node so any connected
 * peer can fetch it directly — no server, CDN, or pinning service. Returns
 * the hex-encoded content id; persist it (e.g. as the avatar id) to look the
 * file back up later via `blobLocalPath`.
 */
export function blobSaveFromFile(fileUri: string): string {
  return NativeCryptoCore.blobSaveFromFile(fileUri)
}

/** The local cache path for `idHex`, or null if this device doesn't have it yet (fetch it first). */
export function blobLocalPath(idHex: string): string | null {
  return NativeCryptoCore.blobLocalPath(idHex)
}

/** Stops serving `idHex` to peers and deletes the on-disk cache entry. */
export function blobClear(idHex: string): void {
  NativeCryptoCore.blobClear(idHex)
}

/** Re-registers an already-cached blob with the P2P node — call once per launch for anything this device should keep serving (e.g. its own avatar). Returns false if not cached. */
export function blobReserve(idHex: string): boolean {
  return NativeCryptoCore.blobReserve(idHex)
}

/** Fetches blob `idHex` from `peerId` (dial first if not connected). Answered by a `blobFetched`/`blobFetchFailed` event. */
export function p2pFetchBlob(peerId: string, idHex: string): void {
  NativeCryptoCore.p2pFetchBlob(peerId, idHex)
}

/**
 * Registers `bytes` under a caller-chosen `idHex`, bypassing the content-
 * addressing `blobSaveFromFile`/`blobReserve` always apply — for a small,
 * non-secret value every peer should be able to look up at one fixed,
 * well-known id without already knowing what it contains (e.g. "what's
 * this device's avatar content id right now", see store/peerAvatars.ts).
 * Not persisted beyond the running P2P node — call again on every launch
 * and whenever the value changes.
 */
export function p2pSetLocalBlobRaw(idHex: string, bytes: Uint8Array): void {
  NativeCryptoCore.p2pSetLocalBlobRaw(idHex, bytes)
}

/**
 * Encrypts `json` (the active account's profile/contacts snapshot) under
 * a key only this account's recovery-phrase holder can derive, and
 * publishes the ciphertext into the public DHT. What makes restoring
 * from a phrase bring the account's data back — no server ever holds
 * anything readable. Re-run periodically and on every profile/contacts
 * change. Answered by `recoveryBackupAnnounced`/
 * `recoveryBackupAnnouncementFailed` on `onP2pEvent`.
 */
export function recoveryBackupPublish(json: string): void {
  NativeCryptoCore.recoveryBackupPublish(json)
}

/**
 * Asks the DHT for this account's own published recovery backup — run
 * after restoring an identity from its phrase. A found record is
 * decrypted natively and arrives as `recoveryBackupRestored` (plain
 * JSON); anything else surfaces as `recoveryBackupResolutionFailed`.
 */
export function recoveryBackupRequest(): void {
  NativeCryptoCore.recoveryBackupRequest()
}

/**
 * Publishes this device's own current avatar content id into the public
 * DHT, keyed by its own peer id — the pointer only, not the avatar bytes
 * (those still need `p2pFetchBlob` over a live connection). Re-run
 * periodically (DHT records expire) and whenever the avatar changes.
 * Answered by `avatarPointerAnnounced`/`avatarPointerAnnouncementFailed`.
 */
export function p2pAnnounceAvatarPointer(avatarContentId: Uint8Array): void {
  NativeCryptoCore.p2pAnnounceAvatarPointer(avatarContentId)
}

/**
 * Looks up whatever avatar content id `peerId` currently has published in
 * the DHT — lets a caller learn which id to `p2pFetchBlob` even while
 * `peerId` is offline right now. Answered by `avatarPointerResolved`/
 * `avatarPointerResolutionFailed`.
 */
export function p2pResolveAvatarPointer(peerId: string): void {
  NativeCryptoCore.p2pResolveAvatarPointer(peerId)
}

export type UsernameLookup =
  | { status: 'resolved'; publicKeyBase64: string; fingerprint: string; peerId: string }
  | { status: 'invalid' }
  | { status: 'available' }

/**
 * Looks up `username` on the public DHT and waits for the answer — unlike
 * the raw `p2pResolveUsername`/`onP2pEvent` pair everything else here
 * uses, this one is worth wrapping in a promise because both "check
 * before claiming" and "search for a contact by handle" want to await a
 * single result rather than manage a listener themselves.
 *
 * This can only ever be an *exact* lookup: a DHT has no notion of "starts
 * with", so there is no live-search-as-you-type or partial-match here —
 * only "does this exact @handle currently resolve to someone verifiable".
 *
 * `'available'` means nothing verifiable is currently published — either
 * genuinely nobody has claimed it, or whoever last claimed it let their
 * DHT record expire without re-publishing. A plain DHT can't tell those
 * apart, and can't stop a different identity from claiming the name out
 * from under an inactive holder — this is advisory, not a reservation.
 *
 * Unlike a server round-trip, a DHT lookup can genuinely take a while —
 * "not found" is the *slowest* outcome, since Kademlia has to walk to the
 * key's closest peers before it can conclude nobody published it. The
 * native side caps its own query at 25s (see p2p-core's `Config`); this
 * default sits comfortably above that so a real answer from the network
 * always wins the race instead of this timeout preempting it.
 */
export function lookupUsername(username: string, timeoutMs = 30_000): Promise<UsernameLookup> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      unsubscribe()
      reject(new Error('Username lookup timed out'))
    }, timeoutMs)

    const unsubscribe = addP2pEventListener((event) => {
      if (event.type === 'usernameResolved' && event.username === username) {
        clearTimeout(timer)
        unsubscribe()
        resolve({ status: 'resolved', publicKeyBase64: event.publicKeyBase64, fingerprint: event.fingerprint, peerId: event.peerId })
      } else if (event.type === 'usernameClaimInvalid' && event.username === username) {
        clearTimeout(timer)
        unsubscribe()
        resolve({ status: 'invalid' })
      } else if (event.type === 'usernameResolutionFailed' && event.username === username) {
        clearTimeout(timer)
        unsubscribe()
        resolve({ status: 'available' })
      }
    })

    NativeCryptoCore.p2pResolveUsername(username)
  })
}

/**
 * Signs `username` with this device's identity and publishes the claim,
 * waiting for confirmation. Callers should `lookupUsername` first and
 * treat a `resolved` result from a *different* fingerprint as taken —
 * this call itself doesn't check, and publishing here doesn't reserve the
 * name against a determined second claimant (see `lookupUsername`'s doc).
 *
 * See `lookupUsername`'s doc for why the default timeout is as high as it
 * is — the same DHT round-trip cost applies here.
 */
export function announceUsername(username: string, timeoutMs = 30_000): Promise<void> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      unsubscribe()
      reject(new Error('Username announcement timed out'))
    }, timeoutMs)

    const unsubscribe = addP2pEventListener((event) => {
      if (event.type === 'usernameAnnounced' && event.username === username) {
        clearTimeout(timer)
        unsubscribe()
        resolve()
      } else if (event.type === 'usernameAnnouncementFailed' && event.username === username) {
        clearTimeout(timer)
        unsubscribe()
        reject(new Error(event.reason))
      }
    })

    NativeCryptoCore.p2pAnnounceUsername(username)
  })
}

function hexToBytes(hex: string): Uint8Array {
  const bytes = new Uint8Array(hex.length / 2)
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = parseInt(hex.substr(i * 2, 2), 16)
  }
  return bytes
}

/**
 * Not cryptographically secure — deliberately fine here, since a claim's
 * `nonce` only needs to make two otherwise-identical-looking claims
 * distinguishable (e.g. a resubmission), not resist prediction. Avoids
 * adding a dependency (e.g. `expo-crypto`) for something this low-stakes.
 */
function randomNonce(length: number): Uint8Array {
  const bytes = new Uint8Array(length)
  for (let i = 0; i < length; i++) {
    bytes[i] = Math.floor(Math.random() * 256)
  }
  return bytes
}

/**
 * This node's own current `@username` ledger chain tip — needed as the
 * anchor for a new claim (see `submitLedgerUsernameClaim`). Answered
 * synchronously (no network round trip): this is purely a read of local
 * state, not a DHT lookup.
 */
export function queryLedgerChainTip(timeoutMs = 10_000): Promise<{ height: number; hash: string }> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      unsubscribe()
      reject(new Error('Chain tip query timed out'))
    }, timeoutMs)

    const unsubscribe = addP2pEventListener((event) => {
      if (event.type === 'chainTipChanged') {
        clearTimeout(timer)
        unsubscribe()
        resolve({ height: event.height, hash: event.hash })
      }
    })

    NativeCryptoCore.p2pQueryChainTip()
  })
}

/**
 * How many blocks deep a claim needs to be before treating it as settled
 * rather than still-reorgable — an app-level UI convention, not something
 * the chain itself enforces (`ledger-core`'s validation rules don't know
 * about "confirmations" at all). At the ~5-minute target block time this
 * is roughly half an hour; see the project plan for why that specific
 * number was chosen over faster-but-riskier alternatives.
 */
export const LEDGER_CONFIRMATION_DEPTH = 6

export type LedgerUsernameOwner =
  | { status: 'found'; ownerPublicKeyBase64: string; claimedAtHeight: number; fingerprint: string; peerId: string }
  | { status: 'not_found' }

/**
 * Looks up `username` in this node's own materialized ledger state —
 * unlike `lookupUsername` (the DHT-based, best-effort predecessor this is
 * replacing), this is a real first-claim-wins answer with no "advisory
 * only" caveat, and (once synced) never needs a network round trip: this
 * node's local chain view *is* the answer.
 *
 * That last part is also the catch: the answer is only as good as this
 * node's own sync state. `app/_layout.tsx` opportunistically requests a
 * chain sync from every peer it connects to so this stays accurate without
 * the caller having to think about it, but a node that hasn't connected to
 * anyone yet (e.g. moments after a fresh launch, offline) can report
 * `not_found` for a name someone else already holds.
 */
export function queryLedgerUsernameOwner(username: string, timeoutMs = 10_000): Promise<LedgerUsernameOwner> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      unsubscribe()
      reject(new Error('Username owner query timed out'))
    }, timeoutMs)

    const unsubscribe = addP2pEventListener((event) => {
      if (event.type === 'usernameOwnerResolved' && event.username === username) {
        clearTimeout(timer)
        unsubscribe()
        resolve({
          status: 'found',
          ownerPublicKeyBase64: event.ownerPublicKeyBase64,
          claimedAtHeight: event.claimedAtHeight,
          fingerprint: event.fingerprint,
          peerId: event.peerId,
        })
      } else if (event.type === 'usernameOwnerNotFound' && event.username === username) {
        clearTimeout(timer)
        unsubscribe()
        resolve({ status: 'not_found' })
      }
    })

    NativeCryptoCore.p2pQueryUsernameOwner(username)
  })
}

/**
 * Signs `username` with this device's identity, anchored to this node's
 * current chain tip, and broadcasts the claim to the ledger's mempool.
 *
 * Unlike `announceUsername`'s DHT-based predecessor, this does **not**
 * mean the name is confirmed yet — nobody has necessarily mined it into a
 * block. There is no "accepted into the mempool" event to wait for (only
 * an explicit `ledgerSubmissionRejected` on failure), so this resolves
 * once `timeoutMs` passes with no rejection — treat the resolved promise
 * as "submitted, not yet confirmed," and watch `chainTipChanged` /
 * `queryLedgerUsernameOwner` afterward to see whether/when it actually
 * lands. Callers should `queryLedgerUsernameOwner` first and treat a
 * `found` result from a different key as taken — this call itself
 * doesn't check.
 */
export function submitLedgerUsernameClaim(username: string, timeoutMs = 10_000): Promise<void> {
  return queryLedgerChainTip().then(
    (tip) =>
      new Promise<void>((resolve, reject) => {
        const transactionBytes = NativeCryptoCore.ledgerBuildUsernameClaim(
          username,
          tip.height,
          hexToBytes(tip.hash),
          randomNonce(8)
        )

        const timer = setTimeout(() => {
          unsubscribe()
          resolve()
        }, timeoutMs)

        const unsubscribe = addP2pEventListener((event) => {
          if (event.type === 'ledgerSubmissionRejected') {
            clearTimeout(timer)
            unsubscribe()
            reject(new Error(event.reason))
          }
        })

        NativeCryptoCore.p2pSubmitUsernameClaim(transactionBytes)
      })
  )
}

/**
 * Catches this node up to `peerId`'s ledger chain tip if it's heavier
 * than this node's own (a no-op, resolving immediately, if this node's
 * chain is already at least as heavy). Dial first if not already
 * connected. Resolves with the resulting local tip height.
 */
export function requestLedgerChainSync(peerId: string, timeoutMs = 30_000): Promise<number> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      unsubscribe()
      reject(new Error('Chain sync timed out'))
    }, timeoutMs)

    const unsubscribe = addP2pEventListener((event) => {
      if (event.type === 'chainSyncCompleted') {
        clearTimeout(timer)
        unsubscribe()
        resolve(event.height)
      } else if (event.type === 'chainSyncFailed' && event.peerId === peerId) {
        clearTimeout(timer)
        unsubscribe()
        reject(new Error(event.reason))
      }
    })

    NativeCryptoCore.p2pRequestChainSync(peerId)
  })
}

/**
 * Starts (or restarts) this node's mining loop, attributing any block it
 * mines to `publicKeyBase64Value` (defaults to this device's own identity
 * key via `publicKeyBase64()` — an attribution key, not necessarily the
 * same key as whoever's claim ends up in the mined block). Runs
 * continuously until `stopLedgerMining`.
 *
 * Normal operation never needs to call this: `MiningController.swift`
 * already starts/stops mining automatically based on real device state
 * (foreground + charging, both required — mining is real sustained CPU
 * work with no other throttle) as soon as a session exists, entirely on
 * the native side. This wrapper exists for tests/tooling — a manual debug
 * toggle, if one is ever needed — not for product UI to call directly.
 * Successful blocks surface as `newBlockMined` on the event stream,
 * alongside the `chainTipChanged` every new tip fires.
 */
export function startLedgerMining(publicKeyBase64Value: string = publicKeyBase64()): void {
  NativeCryptoCore.p2pStartMining(publicKeyBase64Value)
}

/** Stops mining started by `startLedgerMining`. A no-op if not currently mining. */
export function stopLedgerMining(): void {
  NativeCryptoCore.p2pStopMining()
}

/**
 * Whether this device offers to relay/announce for the Sphinx/Loopix
 * mixnet at all — the Settings toggle backing `MixRelayController.swift`.
 * Defaults to on. Actual participation additionally requires foreground +
 * charging (the same device-state gate `MiningController` uses for
 * mining) — this toggle only controls whether the device is *willing* to,
 * not whether it's doing so right this moment.
 */
export function mixRelayParticipationEnabled(): boolean {
  return NativeCryptoCore.mixRelayParticipationEnabled()
}

/**
 * Sets the Settings toggle `mixRelayParticipationEnabled` reads — takes
 * effect immediately (no need to background/foreground the app first).
 */
export function setMixRelayParticipationEnabled(enabled: boolean): void {
  NativeCryptoCore.setMixRelayParticipationEnabled(enabled)
}

/**
 * A rough, honest lower-bound estimate (bytes/hour) of this device's own
 * background data cost from Loopix dummy (cover/loop) mix traffic alone —
 * real messaging traffic on top of this isn't counted, since it varies
 * with actual usage rather than being a constant hum. For display next to
 * the mix-relay participation toggle, not a hard guarantee.
 */
export function mixDummyTrafficBytesPerHourEstimate(): number {
  return NativeCryptoCore.mixDummyTrafficBytesPerHourEstimate()
}

/**
 * Queues `plaintext` for `peerId` and starts delivering it immediately —
 * durable on this device before this call returns, so sending works the
 * same whether `peerId` is online right now or not. `peerPublicKeyBase64`
 * is needed the first time this device messages `peerId` (to fetch and
 * verify their contact card, then run X3DH); pass whatever this device
 * already knows about them (e.g. from a ledger username lookup) every time
 * — cheap to repeat once a session already exists.
 *
 * Returns a local id — watch `onChatEvent` for a `messageSent`/
 * `messageFailed` carrying the same `localId` to learn what happened to
 * this specific message (delivery is transport-level, like
 * `p2pSendEnvelope`'s own `envelopeDelivered` — not a read receipt).
 *
 * There is no relay/mailbox server anywhere in this project: if `peerId`
 * never comes online again, this message can never be delivered — but it
 * isn't lost either. It stays queued and is retried automatically the next
 * time this device sees that peer reconnect (including across an app
 * restart), for as long as this device keeps running.
 */
export function chatSendMessage(peerId: string, peerPublicKeyBase64: string, plaintext: string): string {
  return NativeCryptoCore.chatSendMessage(peerId, peerPublicKeyBase64, plaintext)
}

/**
 * Sends a media file (photo/video/voice) to a 1:1 peer. `fileUri` is a
 * local file on disk (a recorded note, a picked image); it's encrypted
 * chunk by chunk under a fresh per-file key, each chunk registered as a
 * content-addressed blob, and a small manifest sent as the message. The
 * recipient sees a `mediaReceived` event once fetched and decrypted.
 * Returns a local id to correlate with `messageSent`/`messageFailed`.
 */
export function chatSendMedia(
  peerId: string, peerPublicKeyBase64: string, fileUri: string,
  mime: string, filename: string | null = null, durationMs: number | null = null,
): string {
  return NativeCryptoCore.chatSendMedia(peerId, peerPublicKeyBase64, fileUri, mime, filename, durationMs)
}

/** Sends a media file to a group — see `chatSendMedia`. */
export function chatSendGroupMedia(
  groupId: string, fileUri: string, mime: string,
  filename: string | null = null, durationMs: number | null = null,
): string {
  return NativeCryptoCore.chatSendGroupMedia(groupId, fileUri, mime, filename, durationMs)
}

// --- Voice recording (see VoiceRecorder.swift) ---

/** MIME to pass to `chatSendMedia` for a recorded voice note. */
export const VOICE_MIME = 'audio/mp4'

export function hasMicrophonePermission(): boolean {
  return NativeCryptoCore.hasMicrophonePermission()
}

/** Asks for microphone access. Call before the first recording. */
export function requestMicrophonePermission(): Promise<boolean> {
  return NativeCryptoCore.requestMicrophonePermission()
}

/** Starts recording a voice note; returns the temp file's `file://` URL. */
export function voiceRecordingStart(): string {
  return NativeCryptoCore.voiceRecordingStart()
}

/** Finishes recording — `{ fileUri, durationMs }`, or null if none was active. */
export function voiceRecordingStop(): { fileUri: string; durationMs: number } | null {
  return NativeCryptoCore.voiceRecordingStop()
}

/** Discards the current recording (swipe-to-cancel). */
export function voiceRecordingCancel(): void {
  NativeCryptoCore.voiceRecordingCancel()
}

/**
 * Creates a group named `name` with `memberPeerIds` as its initial
 * members. Every member (here, or added later via `chatAddGroupMember`)
 * must already be an existing 1:1 contact (a group invite piggybacks on
 * an *existing* pairwise session — it never triggers first-contact/X3DH
 * establishment the way `chatSendMessage` does). Returns the new group's id.
 */
export function chatCreateGroup(name: string, memberPeerIds: string[]): string {
  return NativeCryptoCore.chatCreateGroup(name, memberPeerIds)
}

/**
 * Encrypts `plaintext` once (Sender Keys — see
 * `spiritchat_crypto_core::sender_key`) and queues it for delivery to
 * every other member of `groupId` — durable and retried on reconnect the
 * same way `chatSendMessage` already is. Returns a local id.
 */
export function chatSendGroupMessage(groupId: string, plaintext: string): string {
  return NativeCryptoCore.chatSendGroupMessage(groupId, plaintext)
}

/**
 * Adds `newMemberPeerId` (who must already be an existing 1:1 contact) to
 * `groupId`. `groupMemberAdded` on `onChatEvent` confirms it locally.
 */
export function chatAddGroupMember(groupId: string, newMemberPeerId: string): void {
  NativeCryptoCore.chatAddGroupMember(groupId, newMemberPeerId)
}

/**
 * Removes `memberToRemove` from `groupId` and rotates this device's own
 * Sender Key chain (every remaining member does the same independently)
 * so the removed member can't decrypt anything sent afterward.
 * `groupMemberRemoved` on `onChatEvent` confirms it locally.
 */
export function chatRemoveGroupMember(groupId: string, memberToRemove: string): void {
  NativeCryptoCore.chatRemoveGroupMember(groupId, memberToRemove)
}

/** Subscribes to decrypted/queued-message events. Returns an unsubscribe function. */
export function addChatEventListener(listener: (event: ChatEvent) => void): () => void {
  const subscription = NativeCryptoCore.addListener('onChatEvent', listener)
  return () => subscription.remove()
}
