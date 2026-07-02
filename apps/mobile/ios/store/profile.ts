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
  lookupUsername,
  announceUsername,
  signOut as nativeSignOut,
} from '../modules/spiritchat-crypto-core'

const DISPLAY_NAME_KEY = 'profile.displayName'
const BIO_KEY          = 'profile.bio'
const AVATAR_ID_KEY    = 'profile.avatarId'
const USERNAME_KEY     = 'profile.username'

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
//
// `username` is the odd one out: it's the one piece of profile data that
// *is* published somewhere else (the public DHT — see UsernameClaim.swift),
// so unlike a display name, a stranger with no prior connection can look
// someone up by it. Optional, since nothing else in this app depends on
// having one.
interface ProfileState {
  isReady:         boolean
  fingerprint:     string
  publicKey:       string
  peerId:          string
  displayName:     string
  bio:             string
  avatarId:        string | null
  avatarLocalPath: string | null
  username:        string | null

  bootstrap:          () => Promise<void>
  setDisplayName:     (name: string) => Promise<void>
  setBio:             (bio: string) => Promise<void>
  setAvatarFromFile:  (fileUri: string) => Promise<void>
  clearAvatar:        () => Promise<void>
  setUsername:        (username: string) => Promise<void>
  signOut:            () => Promise<void>
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
  username:        null,

  bootstrap: async () => {
    const [storedName, storedBio, storedAvatarId, storedUsername] = await Promise.all([
      AsyncStorage.getItem(DISPLAY_NAME_KEY),
      AsyncStorage.getItem(BIO_KEY),
      AsyncStorage.getItem(AVATAR_ID_KEY),
      AsyncStorage.getItem(USERNAME_KEY),
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
      username:        storedUsername,
    })

    // DHT records expire and need periodic re-publishing to stay resolvable
    // — re-announce on every launch, same reasoning as blobReserve above
    // for the avatar. Doesn't block startup on a DHT round-trip; if it
    // fails (e.g. offline), the claim just lapses until the next launch or
    // a manual re-save, not fatal to anything else.
    if (storedUsername) {
      announceUsername(storedUsername).catch(() => {})
    }
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

  // Assumes `username` is already validated (see utils/username.ts) and
  // lowercased/trimmed by the caller. Checking availability and publishing
  // are two separate DHT round-trips (not one atomic operation — nothing
  // in a plain DHT could make it atomic), so there's an inherent, small
  // race window between them; this is the same "advisory, not a
  // reservation" limitation documented on lookupUsername/announceUsername.
  setUsername: async (username) => {
    if (username) {
      const lookup = await lookupUsername(username)
      if (lookup.status === 'resolved' && lookup.fingerprint !== get().fingerprint) {
        throw new Error('Это имя пользователя уже занято')
      }
      await announceUsername(username)
    }
    await AsyncStorage.setItem(USERNAME_KEY, username)
    set({ username: username || null })
  },

  // There is no server session to invalidate — this wipes the local
  // identity (see IdentitySession.signOut) and every local label attached
  // to it, so a *different* identity later created/restored on this same
  // device doesn't inherit a stranger's old display name, avatar, or
  // username. The only way back into *this* account afterward is its
  // recovery phrase.
  signOut: async () => {
    nativeSignOut()
    await AsyncStorage.multiRemove([DISPLAY_NAME_KEY, BIO_KEY, AVATAR_ID_KEY, USERNAME_KEY])
    set({
      isReady:         false,
      fingerprint:     '',
      publicKey:       '',
      peerId:          '',
      displayName:     '',
      bio:             '',
      avatarId:        null,
      avatarLocalPath: null,
      username:        null,
    })
  },
}))
