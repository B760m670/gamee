import { requireNativeModule } from 'expo-modules-core'

/**
 * Mirrors `spiritchat_crypto_core_ffi::FfiP2pEvent` — see p2p_event.rs.
 * Discriminated by `type` so the native side can stay a plain dictionary
 * instead of needing a second binding layer.
 */
export type P2pEvent =
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

type NativeEvents = {
  onP2pEvent(event: P2pEvent): void
}

const NativeCryptoCore = requireNativeModule<
  {
    fingerprint(): string
    publicKeyBase64(): string
    p2pLocalPeerId(): string
    p2pDial(peerId: string, knownAddresses: string[]): void
    p2pResolvePeerAddresses(peerId: string): void
    p2pAnnounceAddresses(addresses: string[]): void
    p2pSendEnvelope(peerId: string, bytes: Uint8Array): void
    p2pReserveRelaySlot(relayAddress: string): void
    blobSaveFromFile(fileUri: string): string
    blobLocalPath(idHex: string): string | null
    blobClear(idHex: string): void
    blobReserve(idHex: string): boolean
    p2pFetchBlob(peerId: string, idHex: string): void
    addListener<EventName extends keyof NativeEvents>(
      eventName: EventName,
      listener: NativeEvents[EventName]
    ): { remove(): void }
  } & object
>('SpiritchatCryptoCore')

/**
 * This device's persistent identity fingerprint ("1234 5678 9012"). The
 * underlying identity/agreement keys and prekey store are generated once
 * and stored in the iOS Keychain (see IdentitySession.swift) — this value
 * is stable across app restarts, not regenerated on every call.
 */
export function fingerprint(): string {
  return NativeCryptoCore.fingerprint()
}

/** This device's public identity key, base64-encoded. */
export function publicKeyBase64(): string {
  return NativeCryptoCore.publicKeyBase64()
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

/** Subscribes to the node's event stream (connections, envelopes, DHT results). Returns an unsubscribe function. */
export function addP2pEventListener(listener: (event: P2pEvent) => void): () => void {
  const subscription = NativeCryptoCore.addListener('onP2pEvent', listener)
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
