import { create } from 'zustand'
import AsyncStorage from '@react-native-async-storage/async-storage'
import type { PeerInfo } from './chat'
// Deferred-at-call-time import — profile.ts also imports this store, and
// the cycle is safe precisely because neither touches the other during
// module initialization.
import { scheduleRecoveryBackupPublish } from './profile'

/**
 * A saved contact — everything needed to open a chat with them later
 * (`peerPublicKeyBase64` is what a first-message X3DH handshake needs),
 * plus when they were added, for a stable sort. Purely local, namespaced
 * by this device's own account fingerprint like every other per-account
 * store: there is no server-side address book, and no contact ever learns
 * they were added — by design.
 */
export interface Contact extends PeerInfo {
  addedAt: number
}

const contactsKey = (myFingerprint: string) => `contacts.${myFingerprint}.list`

interface ContactsState {
  myFingerprint: string
  /** Keyed by peerId — what the Contacts tab renders. */
  contacts: Record<string, Contact>

  loadForFingerprint: (myFingerprint: string) => Promise<void>
  reset: () => void
  addContact: (peer: PeerInfo) => void
  removeContact: (peerId: string) => void
}

async function persistContacts(myFingerprint: string, contacts: Record<string, Contact>) {
  await AsyncStorage.setItem(contactsKey(myFingerprint), JSON.stringify(Object.values(contacts)))
}

export const useContactsStore = create<ContactsState>((set, get) => ({
  myFingerprint: '',
  contacts: {},

  // Loads this identity's own contact list — call once bootstrap knows
  // which fingerprint is active, and again on every account switch
  // (see app/_layout.tsx), mirroring store/chat.ts's loadForFingerprint.
  loadForFingerprint: async (myFingerprint) => {
    if (get().myFingerprint === myFingerprint) return
    const raw = await AsyncStorage.getItem(contactsKey(myFingerprint))
    const list: Contact[] = raw ? JSON.parse(raw) : []
    set({ myFingerprint, contacts: Object.fromEntries(list.map(c => [c.peerId, c])) })
  },

  reset: () => set({ myFingerprint: '', contacts: {} }),

  addContact: (peer) => {
    const myFingerprint = get().myFingerprint
    if (!myFingerprint || !peer.peerId) return
    const next = {
      ...get().contacts,
      [peer.peerId]: { ...peer, addedAt: Date.now() },
    }
    set({ contacts: next })
    persistContacts(myFingerprint, next).catch(() => {})
    scheduleRecoveryBackupPublish()
  },

  removeContact: (peerId) => {
    const myFingerprint = get().myFingerprint
    if (!myFingerprint) return
    const next = { ...get().contacts }
    delete next[peerId]
    set({ contacts: next })
    persistContacts(myFingerprint, next).catch(() => {})
    scheduleRecoveryBackupPublish()
  },
}))
