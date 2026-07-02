import { create } from 'zustand'
import AsyncStorage from '@react-native-async-storage/async-storage'
import {
  fingerprint as cryptoCoreFingerprint,
  publicKeyBase64,
  p2pLocalPeerId,
} from '../modules/spiritchat-crypto-core'

const DISPLAY_NAME_KEY = 'profile.displayName'
const BIO_KEY          = 'profile.bio'

// There is no account server: this device's identity is the Keychain-backed
// key pair from IdentitySession (see the native module), not a row in a
// database. `fingerprint`/`publicKey`/`peerId` are derived from it and never
// change; `displayName`/`bio` are just local labels, persisted on-device so
// they survive restarts without needing anywhere to validate or look them up.
interface ProfileState {
  isReady:     boolean
  fingerprint: string
  publicKey:   string
  peerId:      string
  displayName: string
  bio:         string

  bootstrap:      () => Promise<void>
  setDisplayName: (name: string) => Promise<void>
  setBio:         (bio: string) => Promise<void>
}

export const useProfileStore = create<ProfileState>((set) => ({
  isReady:     false,
  fingerprint: '',
  publicKey:   '',
  peerId:      '',
  displayName: '',
  bio:         '',

  bootstrap: async () => {
    const [storedName, storedBio] = await Promise.all([
      AsyncStorage.getItem(DISPLAY_NAME_KEY),
      AsyncStorage.getItem(BIO_KEY),
    ])
    set({
      isReady:     true,
      fingerprint: cryptoCoreFingerprint(),
      publicKey:   publicKeyBase64(),
      peerId:      p2pLocalPeerId(),
      displayName: storedName ?? '',
      bio:         storedBio ?? '',
    })
  },

  setDisplayName: async (name) => {
    const trimmed = name.trim()
    await AsyncStorage.setItem(DISPLAY_NAME_KEY, trimmed)
    set({ displayName: trimmed })
  },

  setBio: async (bio) => {
    const trimmed = bio.trim()
    await AsyncStorage.setItem(BIO_KEY, trimmed)
    set({ bio: trimmed })
  },
}))
