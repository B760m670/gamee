import { create } from 'zustand'
import AsyncStorage from '@react-native-async-storage/async-storage'
import {
  chatCreateGroup,
  chatSendGroupMessage,
  chatAddGroupMember,
  chatRemoveGroupMember,
  type ChatEvent,
} from '../modules/spiritchat-crypto-core'
import type { ChatMessage } from './chat'
import { mediaLabel } from './chat'

/**
 * A group message reuses `ChatMessage`'s exact shape (so `MessageBubble`
 * needs no changes to render one) plus `senderPeerId`, since — unlike a
 * 1:1 conversation, which only ever has one "other" party — an incoming
 * group message needs to say *which* member sent it.
 */
export interface GroupMessage extends ChatMessage {
  senderPeerId: string
}

export interface GroupInfo {
  groupId: string
  name: string
  /** Every *other* member — never this device's own peer id, mirroring `GroupStore.Session.members` on the native side. */
  members: string[]
}

// Namespaced by this device's own active fingerprint, the same pattern
// store/chat.ts already uses.
const groupsKey = (myFingerprint: string) => `groups.${myFingerprint}.list`
const groupMessagesKey = (myFingerprint: string, groupId: string) => `groups.${myFingerprint}.messages.${groupId}`

interface GroupState {
  myFingerprint: string
  myPeerId: string
  /** Every group this device currently participates in — what a groups list renders. Keyed by groupId. */
  groups: Record<string, GroupInfo>
  /** Message history per group, keyed by groupId — loaded lazily by `openGroup`, not all at once. */
  messages: Record<string, GroupMessage[]>

  loadForFingerprint: (myFingerprint: string, myPeerId: string) => Promise<void>
  reset: () => void
  openGroup: (groupId: string) => Promise<GroupMessage[]>
  /** Every named member must already be an existing 1:1 contact — see `chatCreateGroup`'s own doc comment. Returns the new group's id, or `null` if `name`/`memberPeerIds` were empty. */
  createGroup: (name: string, memberPeerIds: string[]) => string | null
  sendMessage: (groupId: string, text: string) => void
  /** `newMemberPeerId` must already be an existing 1:1 contact — see `chatAddGroupMember`'s own doc comment. */
  addMember: (groupId: string, newMemberPeerId: string) => void
  removeMember: (groupId: string, memberToRemove: string) => void
  handleChatEvent: (event: ChatEvent) => void
}

async function persistGroups(myFingerprint: string, groups: Record<string, GroupInfo>) {
  await AsyncStorage.setItem(groupsKey(myFingerprint), JSON.stringify(Object.values(groups)))
}

async function persistGroupMessages(myFingerprint: string, groupId: string, messages: GroupMessage[]) {
  await AsyncStorage.setItem(groupMessagesKey(myFingerprint, groupId), JSON.stringify(messages))
}

// Serializes read-modify-write cycles per fingerprint for events arriving
// on an account that isn't on screen — mirrors store/chat.ts's own
// background routing exactly (see the comment there for why this exists).
const backgroundWrites: Record<string, Promise<void>> = {}
function enqueueBackgroundWrite(fingerprint: string, op: () => Promise<void>) {
  const prev = backgroundWrites[fingerprint] ?? Promise.resolve()
  backgroundWrites[fingerprint] = prev.then(op).catch(() => {})
}

async function applyGroupEventForInactive(fingerprint: string, event: ChatEvent) {
  const rawGroups = await AsyncStorage.getItem(groupsKey(fingerprint))
  const groups: GroupInfo[] = rawGroups ? JSON.parse(rawGroups) : []

  if (event.type === 'groupInvited') {
    const info: GroupInfo = { groupId: event.groupId, name: event.name, members: event.members }
    const next = [...groups.filter(g => g.groupId !== event.groupId), info]
    await AsyncStorage.setItem(groupsKey(fingerprint), JSON.stringify(next))
    return
  }

  if (event.type === 'groupMessageReceived') {
    const key = groupMessagesKey(fingerprint, event.groupId)
    const raw = await AsyncStorage.getItem(key)
    const list: GroupMessage[] = raw ? JSON.parse(raw) : []
    const at = event.at * 1000
    list.push({
      localId: `in-${event.groupId}-${event.senderPeerId}-${event.at}`,
      senderPeerId: event.senderPeerId,
      outgoing: false,
      text: event.plaintext,
      at,
      status: 'sent',
    })
    await AsyncStorage.setItem(key, JSON.stringify(list))
    return
  }

  if (event.type === 'groupMemberAdded' || event.type === 'groupMemberRemoved') {
    const group = groups.find(g => g.groupId === event.groupId)
    if (!group) return
    const members = event.type === 'groupMemberAdded'
      ? (group.members.includes(event.memberPeerId) ? group.members : [...group.members, event.memberPeerId])
      : group.members.filter(m => m !== event.memberPeerId)
    const next = groups.map(g => (g.groupId === event.groupId ? { ...g, members } : g))
    await AsyncStorage.setItem(groupsKey(fingerprint), JSON.stringify(next))
  }
}

export const useGroupStore = create<GroupState>((set, get) => ({
  myFingerprint: '',
  myPeerId: '',
  groups: {},
  messages: {},

  // Loads this identity's own group list — call once bootstrap knows
  // which fingerprint/peerId is active, and again on every account
  // switch, since a different fingerprint means an entirely different,
  // namespaced set of groups (mirrors store/chat.ts's own loadForFingerprint).
  loadForFingerprint: async (myFingerprint, myPeerId) => {
    if (get().myFingerprint === myFingerprint) return
    const raw = await AsyncStorage.getItem(groupsKey(myFingerprint))
    const list: GroupInfo[] = raw ? JSON.parse(raw) : []
    const byId = Object.fromEntries(list.map(g => [g.groupId, g]))
    set({ myFingerprint, myPeerId, groups: byId, messages: {} })
  },

  reset: () => set({ myFingerprint: '', myPeerId: '', groups: {}, messages: {} }),

  openGroup: async (groupId) => {
    const existing = get().messages[groupId]
    if (existing) return existing
    const raw = await AsyncStorage.getItem(groupMessagesKey(get().myFingerprint, groupId))
    const list: GroupMessage[] = raw ? JSON.parse(raw) : []
    set(state => ({ messages: { ...state.messages, [groupId]: list } }))
    return list
  },

  createGroup: (name, memberPeerIds) => {
    const trimmed = name.trim()
    if (!trimmed || memberPeerIds.length === 0) return null
    const groupId = chatCreateGroup(trimmed, memberPeerIds)
    const info: GroupInfo = { groupId, name: trimmed, members: memberPeerIds }
    const myFingerprint = get().myFingerprint
    const nextGroups = { ...get().groups, [groupId]: info }
    set({ groups: nextGroups })
    persistGroups(myFingerprint, nextGroups).catch(() => {})
    return groupId
  },

  // Renders immediately with `status: 'sent'` — delivery is durable and
  // retried on reconnect on the native side (see chatSendGroupMessage's
  // own doc comment) the same way 1:1 messages are, but there's no
  // single per-member delivery event to reflect back into one message's
  // status the way store/chat.ts's `sendMessage` does, since one group
  // message fans out to several members independently.
  sendMessage: (groupId, text) => {
    const trimmed = text.trim()
    if (!trimmed) return
    const localId = chatSendGroupMessage(groupId, trimmed)
    const at = Date.now()
    const message: GroupMessage = { localId, senderPeerId: get().myPeerId, outgoing: true, text: trimmed, at, status: 'sent' }
    const myFingerprint = get().myFingerprint
    const nextMessages = [...(get().messages[groupId] ?? []), message]
    set(state => ({ messages: { ...state.messages, [groupId]: nextMessages } }))
    persistGroupMessages(myFingerprint, groupId, nextMessages).catch(() => {})
  },

  addMember: (groupId, newMemberPeerId) => {
    chatAddGroupMember(groupId, newMemberPeerId)
  },

  removeMember: (groupId, memberToRemove) => {
    chatRemoveGroupMember(groupId, memberToRemove)
  },

  // Routes every `onChatEvent` from the native side (see app/_layout.tsx,
  // which feeds the same event stream to both this store and
  // store/chat.ts) — ignores anything that isn't a group event, the same
  // way store/chat.ts ignores these two.
  handleChatEvent: (event) => {
    const myFingerprint = get().myFingerprint

    const isGroupMedia = event.type === 'mediaReceived' && !!event.groupId
    if (
      event.type !== 'groupInvited' &&
      event.type !== 'groupMessageReceived' &&
      event.type !== 'groupMemberAdded' &&
      event.type !== 'groupMemberRemoved' &&
      !isGroupMedia
    ) return

    // A group event for an account that isn't on screen (every registered
    // account's node runs concurrently) — persist into that account's own
    // namespace, leave the active account's in-memory state alone. Media
    // is re-fetched when that account is next opened, not persisted here.
    if (event.selfFingerprint && event.selfFingerprint !== myFingerprint) {
      if (event.type !== 'mediaReceived') {
        const origin = event.selfFingerprint
        enqueueBackgroundWrite(origin, () => applyGroupEventForInactive(origin, event))
      }
      return
    }

    if (!myFingerprint) return

    if (event.type === 'mediaReceived' && event.groupId) {
      const at = event.at
      const message: GroupMessage = {
        localId: `media-${event.groupId}-${event.peerId}-${event.at}`,
        senderPeerId: event.peerId,
        outgoing: false,
        text: mediaLabel(event.mime),
        media: { localPath: event.localPath, mime: event.mime, filename: event.filename, durationMs: event.durationMs, totalSize: event.totalSize },
        at,
        status: 'sent',
      }
      const nextMessages = [...(get().messages[event.groupId] ?? []), message]
      set(state => ({ messages: { ...state.messages, [event.groupId as string]: nextMessages } }))
      persistGroupMessages(myFingerprint, event.groupId, nextMessages).catch(() => {})
      return
    }

    if (event.type === 'groupInvited') {
      const info: GroupInfo = { groupId: event.groupId, name: event.name, members: event.members }
      const nextGroups = { ...get().groups, [event.groupId]: info }
      set({ groups: nextGroups })
      persistGroups(myFingerprint, nextGroups).catch(() => {})
      return
    }

    if (event.type === 'groupMessageReceived') {
      const at = event.at * 1000
      const message: GroupMessage = {
        localId: `in-${event.groupId}-${event.senderPeerId}-${event.at}`,
        senderPeerId: event.senderPeerId,
        outgoing: false,
        text: event.plaintext,
        at,
        status: 'sent',
      }
      const nextMessages = [...(get().messages[event.groupId] ?? []), message]
      set(state => ({ messages: { ...state.messages, [event.groupId]: nextMessages } }))
      persistGroupMessages(myFingerprint, event.groupId, nextMessages).catch(() => {})
      return
    }

    if (event.type === 'groupMemberAdded') {
      const group = get().groups[event.groupId]
      if (!group || group.members.includes(event.memberPeerId)) return
      const nextGroups = { ...get().groups, [event.groupId]: { ...group, members: [...group.members, event.memberPeerId] } }
      set({ groups: nextGroups })
      persistGroups(myFingerprint, nextGroups).catch(() => {})
      return
    }

    if (event.type === 'groupMemberRemoved') {
      const group = get().groups[event.groupId]
      if (!group) return
      const nextGroups = { ...get().groups, [event.groupId]: { ...group, members: group.members.filter(m => m !== event.memberPeerId) } }
      set({ groups: nextGroups })
      persistGroups(myFingerprint, nextGroups).catch(() => {})
    }
  },
}))
