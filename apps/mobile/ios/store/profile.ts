import { create } from 'zustand'
import AsyncStorage from '@react-native-async-storage/async-storage'
import {
  fingerprint as cryptoCoreFingerprint,
  publicKeyBase64,
  p2pLocalPeerId,
  blobSaveFromFile,
  blobLocalPath,
  blobClear,
  blobReserve,
} from '../modules/spiritchat-crypto-core'

const DISPLAY_NAME_KEY = 'profile.displayName'
const BIO_KEY          = 'profile.bio'
const AVATAR_ID_KEY    = 'profile.avatarId'

// There is no account server: this device's identity is the Keychain-backed
// key pair from IdentitySession (see the native module), not a row in a
// database. `fingerprint`/`publicKey`/`peerId` are derived from it and never
// change; `displayName`/`bio` are just local labels, persisted on-device so
// they survive restarts without needing anywhere to validate or look them up.
//
// The avatar works the same way, just with a bigger payload: `avatarId` is
// the image's content hash (see blob.rs), persisted so this device knows
// which blob is "current"; the bytes themselves live in BlobStore.swift and
// are (re-)registered with the P2P node via `blobReserve` on every launch,
// since the Rust node keeps nothing on disk between runs. A contact fetches
// it directly from this device the same way `blobSaveFromFile` registered
// it in the first place — there is no upload step.
interface ProfileState {
  isReady:         boolean
  fingerprint:     string
  publicKey:       string
  peerId:          string
  displayName:     string
  bio:             string
  avatarId:        string | null
  avatarLocalPath: string | null

  bootstrap:          () => Promise<void>
  setDisplayName:     (name: string) => Promise<void>
  setBio:             (bio: string) => Promise<void>
  setAvatarFromFile:  (fileUri: string) => Promise<void>
  clearAvatar:        () => Promise<void>
}

export const useProfileStore = create<ProfileState>((set, get) => ({
  isReady:         false,
  fingerprint:     '',
  publicKey:       '',
  peerId:          '',
  displayName:     '',
  bio:             '',
  avatarId:        null,
  avatarLocalPath: null,

  bootstrap: async () => {
    const [storedName, storedBio, storedAvatarId] = await Promise.all([
      AsyncStorage.getItem(DISPLAY_NAME_KEY),
      AsyncStorage.getItem(BIO_KEY),
      AsyncStorage.getItem(AVATAR_ID_KEY),
    ])

    const avatarId = storedAvatarId && blobReserve(storedAvatarId) ? storedAvatarId : null

    set({
      isReady:         true,
      fingerprint:     cryptoCoreFingerprint(),
      publicKey:       publicKeyBase64(),
      peerId:          p2pLocalPeerId(),
      displayName:     storedName ?? '',
      bio:             storedBio ?? '',
      avatarId,
      avatarLocalPath: avatarId ? blobLocalPath(avatarId) : null,
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

  setAvatarFromFile: async (fileUri) => {
    const previousId = get().avatarId
    const id = blobSaveFromFile(fileUri)
    if (previousId && previousId !== id) blobClear(previousId)
    await AsyncStorage.setItem(AVATAR_ID_KEY, id)
    set({ avatarId: id, avatarLocalPath: blobLocalPath(id) })
  },

  clearAvatar: async () => {
    const previousId = get().avatarId
    if (previousId) blobClear(previousId)
    await AsyncStorage.removeItem(AVATAR_ID_KEY)
    set({ avatarId: null, avatarLocalPath: null })
  },
}))
