import { create } from 'zustand'
import AsyncStorage from '@react-native-async-storage/async-storage'
import { chatSendMessage, chatSendMedia, VOICE_MIME, type ChatEvent } from '../modules/spiritchat-crypto-core'

/** Attached media on a message (photo/video/voice), once downloaded. */
export interface ChatMedia {
  /** file:// path to the decrypted media on disk. */
  localPath: string
  mime: string
  filename: string | null
  durationMs: number | null
  totalSize: number
}

export interface ChatMessage {
  localId: string
  outgoing: boolean
  text: string
  /** Present when this message carries media instead of (or besides) text. */
  media?: ChatMedia
  /** Epoch milliseconds. */
  at: number
  /**
   * Only meaningful for `outgoing` messages — an incoming one is always
   * `sent`. `queued` means the transport couldn't hand this off on its
   * *last* attempt (peer unreachable right now) — not a permanent failure:
   * there is no relay/mailbox server in this project, so "the recipient
   * currently isn't reachable" is an expected, ordinary state, not an
   * error. It stays queued and keeps retrying automatically (see
   * ChatManager.swift) for as long as this device keeps running.
   */
  status: 'sending' | 'sent' | 'queued'
}

export interface PeerInfo {
  peerId: string
  peerFingerprint: string
  peerPublicKeyBase64: string
  peerUsername: string | null
}

export interface Conversation extends PeerInfo {
  lastMessageText: string
  /** Epoch milliseconds. */
  lastMessageAt: number
}

// Namespaced by *this device's own* active fingerprint, the same pattern
// store/profile.ts already uses for displayName/bio/etc — a different
// account on this device reads under its own, empty namespace and can
// never see another account's conversations.
/** A short human label + icon for a media message, used as its conversation preview. */
export function mediaLabel(mime: string): string {
  if (mime.startsWith('image/')) return '📷 Фото'
  if (mime.startsWith('video/')) return '🎥 Видео'
  if (mime.startsWith('audio/')) return '🎤 Голосовое'
  return '📎 Файл'
}

const conversationsKey = (myFingerprint: string) => `chat.${myFingerprint}.conversations`
const messagesKey = (myFingerprint: string, peerId: string) => `chat.${myFingerprint}.messages.${peerId}`

interface ChatState {
  myFingerprint: string
  /** Every conversation with at least one sent or received message — what the Chats list renders. Keyed by peerId. */
  conversations: Record<string, Conversation>
  /**
   * Peers a chat screen is open for but nothing has been sent/received
   * yet — `openConversation` populates this immediately (so the composer
   * has what it needs to send) without adding an empty conversation to the
   * list; `sendMessage`/`handleChatEvent` are what actually promote a peer
   * into `conversations`.
   */
  activePeers: Record<string, PeerInfo>
  /** Message history per conversation, keyed by peerId — loaded lazily by `openConversation`, not all at once. */
  messages: Record<string, ChatMessage[]>

  loadForFingerprint: (myFingerprint: string) => Promise<void>
  reset: () => void
  openConversation: (peer: PeerInfo) => Promise<ChatMessage[]>
  sendMessage: (peerId: string, text: string) => Promise<void>
  /** Sends a recorded voice note (from ChatInputBar) to `peerId`. */
  sendVoice: (peerId: string, fileUri: string, durationMs: number) => Promise<void>
  /**
   * Sends a photo or video (from the attach picker) to `peerId`. `fileUri`
   * points at the picked file on disk; `mime` is its content type
   * (e.g. image/jpeg, video/mp4) and drives how the bubble renders it.
   */
  sendMedia: (
    peerId: string,
    fileUri: string,
    mime: string,
    filename: string | null,
    durationMs: number | null,
  ) => Promise<void>
  /**
   * Removes one message from this device's own copy of the history.
   * Local-only by design (for now): the 1:1 wire format carries no shared
   * message id both sides could agree on, so a cryptographically honest
   * "delete for both" needs a framing change first — a local delete that
   * *pretended* to be mutual would be worse than none.
   */
  deleteMessage: (peerId: string, localId: string) => Promise<void>
  /** Removes the whole conversation (history + list entry) from this device only. */
  deleteChat: (peerId: string) => Promise<void>
  handleChatEvent: (event: ChatEvent) => void
}

async function persistConversations(myFingerprint: string, conversations: Record<string, Conversation>) {
  await AsyncStorage.setItem(conversationsKey(myFingerprint), JSON.stringify(Object.values(conversations)))
}

async function persistMessages(myFingerprint: string, peerId: string, messages: ChatMessage[]) {
  await AsyncStorage.setItem(messagesKey(myFingerprint, peerId), JSON.stringify(messages))
}

// --- Inactive-account routing --------------------------------------------
//
// Every registered account's node runs concurrently now (see
// P2pSession.swift), so chat events can arrive for an account that is NOT
// the one on screen. Those never touch this store's in-memory state (that
// is the active account's view) — they're persisted straight into the
// owning fingerprint's own AsyncStorage namespace, so switching to that
// account later loads them exactly as if it had been active all along.

/**
 * Serializes read-modify-write cycles per fingerprint — two events landing
 * back-to-back for the same inactive account must not interleave their
 * AsyncStorage round trips, or the first one's append gets lost.
 */
const backgroundWrites: Record<string, Promise<void>> = {}
function enqueueBackgroundWrite(fingerprint: string, op: () => Promise<void>) {
  const prev = backgroundWrites[fingerprint] ?? Promise.resolve()
  backgroundWrites[fingerprint] = prev.then(op).catch(() => {})
}

async function appendIncomingForInactive(
  fingerprint: string,
  event: Extract<ChatEvent, { type: 'messageReceived' }>,
) {
  const at = event.at * 1000
  const rawMessages = await AsyncStorage.getItem(messagesKey(fingerprint, event.peerId))
  const list: ChatMessage[] = rawMessages ? JSON.parse(rawMessages) : []
  list.push({ localId: `in-${event.peerId}-${event.at}`, outgoing: false, text: event.plaintext, at, status: 'sent' })
  await AsyncStorage.setItem(messagesKey(fingerprint, event.peerId), JSON.stringify(list))

  const rawConversations = await AsyncStorage.getItem(conversationsKey(fingerprint))
  const conversations: Conversation[] = rawConversations ? JSON.parse(rawConversations) : []
  const known = conversations.find(c => c.peerId === event.peerId)
  const updated: Conversation = {
    peerId: event.peerId,
    peerFingerprint: event.peerFingerprint,
    peerPublicKeyBase64: event.peerPublicKeyBase64,
    peerUsername: known?.peerUsername ?? null,
    lastMessageText: event.plaintext,
    lastMessageAt: at,
  }
  const next = [...conversations.filter(c => c.peerId !== event.peerId), updated]
  await AsyncStorage.setItem(conversationsKey(fingerprint), JSON.stringify(next))
}

async function updateStatusForInactive(
  fingerprint: string,
  event: Extract<ChatEvent, { type: 'messageSent' } | { type: 'messageFailed' }>,
) {
  const key = messagesKey(fingerprint, event.peerId)
  const raw = await AsyncStorage.getItem(key)
  if (!raw) return
  const list: ChatMessage[] = JSON.parse(raw)
  const status: ChatMessage['status'] = event.type === 'messageSent' ? 'sent' : 'queued'
  const next = list.map(m => (m.localId === event.localId ? { ...m, status } : m))
  await AsyncStorage.setItem(key, JSON.stringify(next))
}

export const useChatStore = create<ChatState>((set, get) => ({
  myFingerprint: '',
  conversations: {},
  activePeers: {},
  messages: {},

  // Loads this identity's own conversation list — call once bootstrap
  // knows which fingerprint is active, and again on every account switch
  // (see app/_layout.tsx), since a different fingerprint means an entirely
  // different, namespaced set of conversations.
  loadForFingerprint: async (myFingerprint) => {
    if (get().myFingerprint === myFingerprint) return
    const raw = await AsyncStorage.getItem(conversationsKey(myFingerprint))
    const list: Conversation[] = raw ? JSON.parse(raw) : []
    const byPeerId = Object.fromEntries(list.map(c => [c.peerId, c]))
    set({ myFingerprint, conversations: byPeerId, activePeers: {}, messages: {} })
  },

  // Clears in-memory state without touching AsyncStorage — for sign-out,
  // where there's no "next fingerprint" to load into yet (mirrors
  // `afterAccountChange` in store/profile.ts).
  reset: () => set({ myFingerprint: '', conversations: {}, activePeers: {}, messages: {} }),

  // Prepares to chat with `peer` and returns its message history so far —
  // deliberately does NOT add it to the conversation list yet: this is
  // reached by tapping a search result, before the user has necessarily
  // decided to send anything, and the list should only ever show
  // conversations that actually have a message in them (see `sendMessage`/
  // `handleChatEvent`, the two places that promote a peer into `conversations`).
  openConversation: async (peer) => {
    set(state => ({ activePeers: { ...state.activePeers, [peer.peerId]: peer } }))
    const existing = get().messages[peer.peerId]
    if (existing) return existing
    const raw = await AsyncStorage.getItem(messagesKey(get().myFingerprint, peer.peerId))
    const list: ChatMessage[] = raw ? JSON.parse(raw) : []
    set(state => ({ messages: { ...state.messages, [peer.peerId]: list } }))
    return list
  },

  // Optimistic send: the message renders immediately with `status:
  // 'sending'`, before `chatSendMessage` even returns — the native side
  // has already made it durable on disk by the time this call resolves
  // (see ChatManager/ChatStore.swift), so "sending" here just means "not
  // yet handed to the transport successfully," not "might be lost."
  sendMessage: async (peerId, text) => {
    const trimmed = text.trim()
    if (!trimmed) return
    const peer = get().activePeers[peerId] ?? get().conversations[peerId]
    if (!peer) return

    const localId = chatSendMessage(peerId, peer.peerPublicKeyBase64, trimmed)
    const at = Date.now()
    const message: ChatMessage = { localId, outgoing: true, text: trimmed, at, status: 'sending' }

    const myFingerprint = get().myFingerprint
    const nextMessages = [...(get().messages[peerId] ?? []), message]
    const nextConversations = {
      ...get().conversations,
      [peerId]: { ...peer, lastMessageText: trimmed, lastMessageAt: at },
    }
    set({
      messages: { ...get().messages, [peerId]: nextMessages },
      conversations: nextConversations,
    })
    await Promise.all([
      persistMessages(myFingerprint, peerId, nextMessages),
      persistConversations(myFingerprint, nextConversations),
    ])
  },

  sendVoice: async (peerId, fileUri, durationMs) => {
    await get().sendMedia(peerId, fileUri, VOICE_MIME, null, durationMs)
  },

  sendMedia: async (peerId, fileUri, mime, filename, durationMs) => {
    const peer = get().activePeers[peerId] ?? get().conversations[peerId]
    if (!peer) return

    const localId = chatSendMedia(peerId, peer.peerPublicKeyBase64, fileUri, mime, filename, durationMs)
    const at = Date.now()
    const label = mediaLabel(mime)
    const message: ChatMessage = {
      localId, outgoing: true, text: label,
      media: { localPath: fileUri, mime, filename, durationMs, totalSize: 0 },
      at, status: 'sending',
    }

    const myFingerprint = get().myFingerprint
    const nextMessages = [...(get().messages[peerId] ?? []), message]
    const nextConversations = { ...get().conversations, [peerId]: { ...peer, lastMessageText: label, lastMessageAt: at } }
    set({ messages: { ...get().messages, [peerId]: nextMessages }, conversations: nextConversations })
    await Promise.all([
      persistMessages(myFingerprint, peerId, nextMessages),
      persistConversations(myFingerprint, nextConversations),
    ])
  },

  deleteMessage: async (peerId, localId) => {
    const myFingerprint = get().myFingerprint
    if (!myFingerprint) return
    const list = get().messages[peerId]
    if (!list) return
    const next = list.filter(m => m.localId !== localId)
    set(state => ({ messages: { ...state.messages, [peerId]: next } }))
    await persistMessages(myFingerprint, peerId, next)

    // Keep the conversation preview honest if the deleted message was the
    // latest one.
    const conversation = get().conversations[peerId]
    if (conversation) {
      const last = next[next.length - 1]
      const updated = {
        ...conversation,
        lastMessageText: last?.text ?? '',
        lastMessageAt: last?.at ?? conversation.lastMessageAt,
      }
      const nextConversations = { ...get().conversations, [peerId]: updated }
      set({ conversations: nextConversations })
      await persistConversations(myFingerprint, nextConversations)
    }
  },

  deleteChat: async (peerId) => {
    const myFingerprint = get().myFingerprint
    if (!myFingerprint) return
    const nextConversations = { ...get().conversations }
    delete nextConversations[peerId]
    set(state => {
      const messages = { ...state.messages }
      delete messages[peerId]
      const activePeers = { ...state.activePeers }
      delete activePeers[peerId]
      return { conversations: nextConversations, messages, activePeers }
    })
    await Promise.all([
      persistConversations(myFingerprint, nextConversations),
      AsyncStorage.removeItem(messagesKey(myFingerprint, peerId)),
    ])
  },

  // Routes every `onChatEvent` from the native side (see app/_layout.tsx,
  // which subscribes once for the app's lifetime) into local state —
  // `messageReceived` can be for a conversation this device never
  // initiated, so it's the other place (besides `sendMessage`) a peer gets
  // promoted into `conversations`.
  handleChatEvent: (event) => {
    const myFingerprint = get().myFingerprint

    // Group events are handled entirely in store/groups.ts — nothing to
    // do with them here.
    if (
      event.type === 'groupInvited' ||
      event.type === 'groupMessageReceived' ||
      event.type === 'groupMemberAdded' ||
      event.type === 'groupMemberRemoved'
    ) return

    // An event from an account that isn't on screen right now (every
    // registered account's node runs concurrently) — persist it into that
    // account's own namespace and leave the active account's in-memory
    // state alone.
    if (event.selfFingerprint && event.selfFingerprint !== myFingerprint) {
      const origin = event.selfFingerprint
      if (event.type === 'messageReceived') {
        enqueueBackgroundWrite(origin, () => appendIncomingForInactive(origin, event))
      } else if (event.type === 'messageSent' || event.type === 'messageFailed') {
        enqueueBackgroundWrite(origin, () => updateStatusForInactive(origin, event))
      }
      // Media for an inactive account is re-fetched when it's next opened;
      // nothing to persist here in this first cut.
      return
    }

    if (!myFingerprint) return // no account active right now — nothing to attribute this to

    // A finished 1:1 media download. Group media (groupId set) is the
    // groups store's concern; skip it here.
    if (event.type === 'mediaReceived') {
      if (event.groupId) return
      const known = get().conversations[event.peerId] ?? get().activePeers[event.peerId]
      const peer: PeerInfo = {
        peerId: event.peerId,
        peerFingerprint: known?.peerFingerprint ?? '',
        peerPublicKeyBase64: known?.peerPublicKeyBase64 ?? '',
        peerUsername: known?.peerUsername ?? null,
      }
      const label = mediaLabel(event.mime)
      const message: ChatMessage = {
        localId: `media-${event.peerId}-${event.at}`,
        outgoing: false,
        text: label,
        media: { localPath: event.localPath, mime: event.mime, filename: event.filename, durationMs: event.durationMs, totalSize: event.totalSize },
        at: event.at,
        status: 'sent',
      }
      const nextMessages = [...(get().messages[event.peerId] ?? []), message]
      const nextConversations = { ...get().conversations, [event.peerId]: { ...peer, lastMessageText: label, lastMessageAt: event.at } }
      set(state => ({
        activePeers: { ...state.activePeers, [event.peerId]: peer },
        messages: { ...state.messages, [event.peerId]: nextMessages },
        conversations: nextConversations,
      }))
      persistMessages(myFingerprint, event.peerId, nextMessages).catch(() => {})
      persistConversations(myFingerprint, nextConversations).catch(() => {})
      return
    }

    if (event.type === 'messageReceived') {
      const known = get().conversations[event.peerId] ?? get().activePeers[event.peerId]
      const peer: PeerInfo = {
        peerId: event.peerId,
        peerFingerprint: event.peerFingerprint,
        peerPublicKeyBase64: event.peerPublicKeyBase64,
        peerUsername: known?.peerUsername ?? null,
      }
      const at = event.at * 1000
      const message: ChatMessage = { localId: `in-${event.peerId}-${event.at}`, outgoing: false, text: event.plaintext, at, status: 'sent' }

      const nextMessages = [...(get().messages[event.peerId] ?? []), message]
      const nextConversations = { ...get().conversations, [event.peerId]: { ...peer, lastMessageText: event.plaintext, lastMessageAt: at } }
      set(state => ({
        activePeers: { ...state.activePeers, [event.peerId]: peer },
        messages: { ...state.messages, [event.peerId]: nextMessages },
        conversations: nextConversations,
      }))
      persistMessages(myFingerprint, event.peerId, nextMessages).catch(() => {})
      persistConversations(myFingerprint, nextConversations).catch(() => {})
      return
    }

    // messageSent / messageFailed — both just update one already-rendered
    // outgoing message's status by localId. "Failed" only ever means "not
    // delivered on this attempt" (see the `queued` status doc above), so it
    // maps to the same UI state as "still sending," not an error.
    const list = get().messages[event.peerId]
    if (!list) return
    const status: ChatMessage['status'] = event.type === 'messageSent' ? 'sent' : 'queued'
    const next = list.map(m => (m.localId === event.localId ? { ...m, status } : m))
    set(state => ({ messages: { ...state.messages, [event.peerId]: next } }))
    persistMessages(myFingerprint, event.peerId, next).catch(() => {})
  },
}))
