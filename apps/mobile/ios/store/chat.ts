import { create } from 'zustand'
import AsyncStorage from '@react-native-async-storage/async-storage'
import { chatSendMessage, type ChatEvent } from '../modules/spiritchat-crypto-core'

export interface ChatMessage {
  localId: string
  outgoing: boolean
  text: string
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
  handleChatEvent: (event: ChatEvent) => void
}

async function persistConversations(myFingerprint: string, conversations: Record<string, Conversation>) {
  await AsyncStorage.setItem(conversationsKey(myFingerprint), JSON.stringify(Object.values(conversations)))
}

async function persistMessages(myFingerprint: string, peerId: string, messages: ChatMessage[]) {
  await AsyncStorage.setItem(messagesKey(myFingerprint, peerId), JSON.stringify(messages))
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

  // Routes every `onChatEvent` from the native side (see app/_layout.tsx,
  // which subscribes once for the app's lifetime) into local state —
  // `messageReceived` can be for a conversation this device never
  // initiated, so it's the other place (besides `sendMessage`) a peer gets
  // promoted into `conversations`.
  handleChatEvent: (event) => {
    const myFingerprint = get().myFingerprint
    if (!myFingerprint) return // no account active right now — nothing to attribute this to

    // Group events are handled entirely in store/groups.ts — nothing to
    // do with them here.
    if (
      event.type === 'groupInvited' ||
      event.type === 'groupMessageReceived' ||
      event.type === 'groupMemberAdded' ||
      event.type === 'groupMemberRemoved'
    ) return

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
