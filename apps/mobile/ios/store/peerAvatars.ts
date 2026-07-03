import { create } from 'zustand'
import { File } from 'expo-file-system'
import { p2pFetchBlob, p2pSetLocalBlobRaw, type P2pEvent } from '../modules/spiritchat-crypto-core'

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
const pendingContent: Record<string, string> = {}

/**
 * Publishes (or clears) this device's own avatar pointer so peers can find
 * its current avatar — call at bootstrap and every time the avatar
 * changes (see store/profile.ts).
 */
export function publishOwnAvatarPointer(avatarId: string | null) {
  try {
    p2pSetLocalBlobRaw(AVATAR_POINTER_BLOB_ID, asciiToBytes(avatarId ?? ''))
  } catch {
    // P2P isn't up yet — profile.ts's own P2P-ready retry loop re-bootstraps
    // and calls this again once it is.
  }
}

/** Kicks off fetching `peerId`'s current avatar — a no-op if already resolved or already in flight. */
export function requestPeerAvatar(peerId: string) {
  if (usePeerAvatarStore.getState().localPaths[peerId]) return
  if (pendingPointer.has(peerId) || pendingContent[peerId]) return
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

/**
 * Feed every `P2pEvent` into this (see app/_layout.tsx, wired once
 * alongside the existing `addP2pEventListener` subscription) — reacts only
 * to the `blobFetched`/`blobFetchFailed` pairs this two-hop fetch produces
 * and ignores everything else.
 */
export function handlePeerAvatarEvent(event: P2pEvent) {
  if (event.type === 'blobFetched') {
    if (event.id === AVATAR_POINTER_BLOB_ID) {
      pendingPointer.delete(event.peerId)
      readPointerText(event.localPath)
        .then((contentId) => {
          if (!contentId) return // this peer has no avatar set right now
          pendingContent[event.peerId] = contentId
          try {
            p2pFetchBlob(event.peerId, contentId)
          } catch {
            delete pendingContent[event.peerId]
          }
        })
        .catch(() => {})
      return
    }
    if (pendingContent[event.peerId] === event.id) {
      delete pendingContent[event.peerId]
      usePeerAvatarStore.setState(state => ({ localPaths: { ...state.localPaths, [event.peerId]: event.localPath } }))
    }
    return
  }

  if (event.type === 'blobFetchFailed') {
    if (event.id === AVATAR_POINTER_BLOB_ID) pendingPointer.delete(event.peerId)
    else if (pendingContent[event.peerId] === event.id) delete pendingContent[event.peerId]
  }
}
