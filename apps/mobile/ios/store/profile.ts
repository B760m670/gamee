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
  queryLedgerUsernameOwner,
  submitLedgerUsernameClaim,
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
// *is* published somewhere else — the phone-run `@username` ledger (see
// spiritchat-ledger-core), a small purpose-built proof-of-work chain, not a
// server — so unlike a display name, a stranger with no prior connection
// can look someone up by it, and (once a claim is mined and confirmed) it's
// guaranteed to be unique, not just advisory the way the old DHT-based
// claims were. Optional, since nothing else in this app depends on having
// one. This store only ever persists an *already-confirmed-or-submitted*
// username locally (`persistUsername`) — the multi-stage submit/confirm
// flow itself lives in app/settings/username.tsx, since it needs to show
// fine-grained progress a single store action couldn't expose.
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
  persistUsername:    (username: string) => Promise<void>
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

    // Unlike the old DHT claims, a confirmed ledger claim never expires or
    // needs re-publishing — it's permanent chain state. But this node's
    // *local* mempool is never persisted (see p2p-core's node.rs), so a
    // claim submitted just before the app last closed could easily have
    // been lost before any miner included it. Reconcile on every launch,
    // without blocking startup on it: if the chain already shows this
    // device as the owner, there's nothing to do; if someone else's claim
    // won a race that happened while this device was offline, drop the
    // now-invalid local name instead of continuing to show one that isn't
    // actually held; otherwise (not found yet) resubmit — safe either way,
    // since it's the same username/owner claim regardless of how many
    // times it's (re)signed.
    if (storedUsername) {
      queryLedgerUsernameOwner(storedUsername)
        .then((owner) => {
          if (owner.status === 'found' && owner.ownerPublicKeyBase64 === get().publicKey) return
          if (owner.status === 'found') {
            AsyncStorage.removeItem(USERNAME_KEY).catch(() => {})
            set({ username: null })
            return
          }
          submitLedgerUsernameClaim(storedUsername).catch(() => {})
        })
        .catch(() => {})
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

  // Purely local: writes whatever username the caller already confirmed
  // (or cleared) with the ledger — see app/settings/username.tsx, which
  // owns the actual submit/confirm flow, since it needs to show
  // fine-grained progress this store doesn't track.
  persistUsername: async (username) => {
    if (username) {
      await AsyncStorage.setItem(USERNAME_KEY, username)
    } else {
      await AsyncStorage.removeItem(USERNAME_KEY)
    }
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
