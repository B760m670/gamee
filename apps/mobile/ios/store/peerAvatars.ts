import { create } from 'zustand'
import { File } from 'expo-file-system'
import {
  p2pAnnounceAvatarPointer,
  p2pFetchBlob,
  p2pResolveAvatarPointer,
  p2pSetLocalBlobRaw,
  type P2pEvent,
} from '../modules/spiritchat-crypto-core'

function asciiToHex(text: string): string {
  let out = ''
  for (let i = 0; i < text.length; i++) out += text.charCodeAt(i).toString(16).padStart(2, '0')
  return out
}

function asciiToBytes(text: string): Uint8Array {
  const bytes = new Uint8Array(text.length)
  for (let i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i)
  return bytes
}

function bytesToAscii(bytes: Uint8Array): string {
  let out = ''
  for (let i = 0; i < bytes.length; i++) out += String.fromCharCode(bytes[i])
  return out
}

/**
 * The fixed, non-content-addressed blob id every SpiritChat node serves its
 * own current avatar's content id under, as plain UTF-8 text (or empty
 * bytes if it has no avatar right now) — a peer fetches this first, then
 * fetches the real, content-addressed avatar blob under whatever id it
 * names. Two hops instead of one because the *pointer* has to live at a
 * fixed, guessable id (there's nothing else to look it up by, unlike a real
 * avatar image which is addressed by its own hash) — this keeps the actual
 * image bytes properly content-addressed, so two different peers' avatars
 * can never collide in this device's local blob cache.
 */
const AVATAR_POINTER_BLOB_ID = asciiToHex('spiritchat-avatar-pointer-v1')

interface PeerAvatarState {
  /** peerId -> resolved local file path, once the real avatar blob (not just its pointer) has been fetched. */
  localPaths: Record<string, string>
}

export const usePeerAvatarStore = create<PeerAvatarState>(() => ({ localPaths: {} }))

// Pure bookkeeping for the two-hop fetch, not rendered by anything — kept
// out of the zustand store so it doesn't need immutable updates.
const pendingPointer = new Set<string>()
const pendingPointerViaDht = new Set<string>()
const pendingContent: Record<string, string> = {}

/**
 * Publishes (or clears) this device's own avatar pointer so peers can find
 * its current avatar — call at bootstrap and every time the avatar
 * changes (see store/profile.ts). Publishes both ways: the existing
 * fixed-id local blob (fetchable only while this device is online) and,
 * since the DHT-published contact-card fix, also a DHT record under this
 * device's own peer id, so the pointer itself (not the avatar bytes,
 * which still need a live connection either way) is discoverable even
 * while this device is currently offline.
 */
export function publishOwnAvatarPointer(avatarId: string | null) {
  const bytes = asciiToBytes(avatarId ?? '')
  try {
    p2pSetLocalBlobRaw(AVATAR_POINTER_BLOB_ID, bytes)
  } catch {
    // P2P isn't up yet — profile.ts's own P2P-ready retry loop re-bootstraps
    // and calls this again once it is.
  }
  try {
    p2pAnnounceAvatarPointer(bytes)
  } catch {
    // Same as above — profile.ts's periodic re-announce sweep retries.
  }
}

/** Kicks off fetching `peerId`'s current avatar — a no-op if already resolved or already in flight. */
export function requestPeerAvatar(peerId: string) {
  if (usePeerAvatarStore.getState().localPaths[peerId]) return
  if (pendingPointer.has(peerId) || pendingPointerViaDht.has(peerId) || pendingContent[peerId]) return
  pendingPointer.add(peerId)
  try {
    p2pFetchBlob(peerId, AVATAR_POINTER_BLOB_ID)
  } catch {
    pendingPointer.delete(peerId)
  }
}

async function readPointerText(localPath: string): Promise<string> {
  const file = new File(localPath)
  const text = await file.text()
  return text.trim()
}

/** Shared by both the direct-fetch and DHT-resolved pointer paths once a content id is known. */
function fetchAvatarContent(peerId: string, contentId: string) {
  if (!contentId) return // this peer has no avatar set right now
  pendingContent[peerId] = contentId
  try {
    p2pFetchBlob(peerId, contentId)
  } catch {
    delete pendingContent[peerId]
  }
}

/**
 * Feed every `P2pEvent` into this (see app/_layout.tsx, wired once
 * alongside the existing `addP2pEventListener` subscription) — reacts only
 * to the events this two-hop fetch (now with a DHT fallback for the
 * pointer half) produces and ignores everything else.
 */
export function handlePeerAvatarEvent(event: P2pEvent) {
  if (event.type === 'blobFetched') {
    if (event.id === AVATAR_POINTER_BLOB_ID) {
      pendingPointer.delete(event.peerId)
      readPointerText(event.localPath).then((contentId) => fetchAvatarContent(event.peerId, contentId)).catch(() => {})
      return
    }
    if (pendingContent[event.peerId] === event.id) {
      delete pendingContent[event.peerId]
      usePeerAvatarStore.setState(state => ({ localPaths: { ...state.localPaths, [event.peerId]: event.localPath } }))
    }
    return
  }

  if (event.type === 'blobFetchFailed') {
    if (event.id === AVATAR_POINTER_BLOB_ID) {
      // The peer wasn't reachable directly — fall back to whatever
      // pointer it may have published into the DHT while offline.
      pendingPointer.delete(event.peerId)
      pendingPointerViaDht.add(event.peerId)
      try {
        p2pResolveAvatarPointer(event.peerId)
      } catch {
        pendingPointerViaDht.delete(event.peerId)
      }
    } else if (pendingContent[event.peerId] === event.id) {
      delete pendingContent[event.peerId]
    }
    return
  }

  if (event.type === 'avatarPointerResolved') {
    if (!pendingPointerViaDht.delete(event.peerId)) return
    fetchAvatarContent(event.peerId, bytesToAscii(event.avatarContentId))
    return
  }

  if (event.type === 'avatarPointerResolutionFailed') {
    pendingPointerViaDht.delete(event.peerId)
  }
}
