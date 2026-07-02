import { create } from 'zustand'
import AsyncStorage from '@react-native-async-storage/async-storage'
import {
  hasIdentity,
  fingerprint as cryptoCoreFingerprint,
  publicKeyBase64,
  p2pLocalPeerId,
  p2pLastStartupErrorDescription,
  blobSaveFromFile,
  blobLocalPath,
  blobClear,
  blobReserve,
  queryLedgerUsernameOwner,
  submitLedgerUsernameClaim,
  signOut as nativeSignOut,
  accountSlots as nativeAccountSlots,
  activeAccountSlot,
  switchAccount as nativeSwitchAccount,
  removeAccount as nativeRemoveAccount,
  type AccountSlot,
} from '../modules/spiritchat-crypto-core'

// Namespaced by fingerprint rather than fixed keys: this device's Keychain
// identity is recoverable via its phrase, so signing out and restoring the
// *same* identity later should bring these local labels back with it, not
// wipe them the way a genuinely different identity's data must never leak
// into. Namespacing gets both for free — a different identity simply reads
// under a different, empty namespace — without `signOut` needing to
// actively delete anything (see `signOut`'s own doc comment below).
export const displayNameKey = (fingerprint: string) => `profile.${fingerprint}.displayName`
const bioKey         = (fingerprint: string) => `profile.${fingerprint}.bio`
const avatarIdKey    = (fingerprint: string) => `profile.${fingerprint}.avatarId`
const usernameKey    = (fingerprint: string) => `profile.${fingerprint}.username`

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
  /**
   * The underlying reason the P2P/ledger node hasn't started, if it
   * hasn't — surfaced from `p2pLastStartupErrorDescription()` (see its own
   * doc comment) so this is visible in the UI without a device console.
   * `null` once P2P comes up; identity/profile data is usable regardless,
   * since a P2P failure no longer blocks onboarding (see `bootstrap`).
   */
  p2pStartupError: string | null
  displayName:     string
  bio:             string
  avatarId:        string | null
  avatarLocalPath: string | null
  username:        string | null
  /** Every account registered on this device (up to 3) — for the account switcher. */
  accounts:        AccountSlot[]
  /** Which entry in `accounts` this store's other fields currently reflect. */
  activeSlot:      number

  bootstrap:          () => Promise<void>
  setDisplayName:     (name: string) => Promise<void>
  setBio:             (bio: string) => Promise<void>
  setAvatarFromFile:  (fileUri: string) => Promise<void>
  clearAvatar:        () => Promise<void>
  persistUsername:    (username: string) => Promise<void>
  signOut:            () => Promise<void>
  /** Instant — no recovery phrase needed for a slot already on this device. */
  switchAccount:      (slot: number) => Promise<void>
  /** Permanent — the only way back into `slot` afterward is its own recovery phrase. */
  removeAccount:      (slot: number) => Promise<void>
}

/** How often to retry reading `peerId` while P2P hasn't come up yet. */
const P2P_RETRY_INTERVAL_MS = 1000
/** Give up updating the store after this many retries — P2P keeps trying
 * to start in the background regardless (see P2pSession.swift); this just
 * bounds how long `bootstrap` itself keeps polling for a first success. */
const P2P_RETRY_ATTEMPTS = 30

export const useProfileStore = create<ProfileState>((set, get) => ({
  isReady:         false,
  fingerprint:     '',
  publicKey:       '',
  peerId:          '',
  p2pStartupError: null,
  displayName:     '',
  bio:             '',
  avatarId:        null,
  avatarLocalPath: null,
  username:        null,
  accounts:        [],
  activeSlot:      0,

  bootstrap: async () => {
    // Read first — the identity is already real the moment this runs
    // (bootstrap only ever follows `hasIdentity()`/`setIdentityFromWords`
    // succeeding), and every local key below is namespaced by it.
    const fingerprint = cryptoCoreFingerprint()
    const accounts = nativeAccountSlots()
    const activeSlot = activeAccountSlot()

    const [storedName, storedBio, storedAvatarId, storedUsername] = await Promise.all([
      AsyncStorage.getItem(displayNameKey(fingerprint)),
      AsyncStorage.getItem(bioKey(fingerprint)),
      AsyncStorage.getItem(avatarIdKey(fingerprint)),
      AsyncStorage.getItem(usernameKey(fingerprint)),
    ])

    const avatarId = storedAvatarId && blobReserve(storedAvatarId) ? storedAvatarId : null

    // A P2P/ledger startup failure must never block onboarding — the
    // identity itself (fingerprint/publicKey, both purely local) is
    // already real regardless of whether P2P ever comes up. `peerId` is
    // the only field genuinely dependent on it; degrade to an empty
    // string and surface the real reason instead of throwing.
    let peerId = ''
    let p2pStartupError: string | null = null
    try {
      peerId = p2pLocalPeerId()
    } catch {
      p2pStartupError = p2pLastStartupErrorDescription()
    }

    set({
      isReady:         true,
      fingerprint,
      publicKey:       publicKeyBase64(),
      peerId,
      p2pStartupError,
      displayName:     storedName ?? '',
      bio:             storedBio ?? '',
      avatarId,
      avatarLocalPath: avatarId ? blobLocalPath(avatarId) : null,
      username:        storedUsername,
      accounts,
      activeSlot,
    })

    // P2pSession.swift retries starting the node in the background on its
    // own (every 200ms) regardless of this store — poll for a first
    // success here just long enough to update `peerId`/clear the error
    // without the user having to relaunch once it does come up. Explicitly
    // NOT awaited: onboarding is waiting on `bootstrap()`'s own promise to
    // navigate into the app, and blocking that on up to
    // P2P_RETRY_ATTEMPTS * P2P_RETRY_INTERVAL_MS would defeat the entire
    // point of degrading gracefully instead of failing outright.
    if (!peerId) {
      void (async () => {
        for (let attempt = 0; attempt < P2P_RETRY_ATTEMPTS; attempt++) {
          await new Promise((resolve) => setTimeout(resolve, P2P_RETRY_INTERVAL_MS))
          try {
            set({ peerId: p2pLocalPeerId(), p2pStartupError: null })
            return
          } catch {
            // still not up — keep the latest error text current in case it changed
            set({ p2pStartupError: p2pLastStartupErrorDescription() })
          }
        }
      })()
    }

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
            AsyncStorage.removeItem(usernameKey(fingerprint)).catch(() => {})
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
    await AsyncStorage.setItem(displayNameKey(get().fingerprint), trimmed)
    set({ displayName: trimmed })
  },

  setBio: async (bio) => {
    const trimmed = bio.trim()
    await AsyncStorage.setItem(bioKey(get().fingerprint), trimmed)
    set({ bio: trimmed })
  },

  setAvatarFromFile: async (fileUri) => {
    const previousId = get().avatarId
    const id = blobSaveFromFile(fileUri)
    if (previousId && previousId !== id) blobClear(previousId)
    await AsyncStorage.setItem(avatarIdKey(get().fingerprint), id)
    set({ avatarId: id, avatarLocalPath: blobLocalPath(id) })
  },

  clearAvatar: async () => {
    const previousId = get().avatarId
    if (previousId) blobClear(previousId)
    await AsyncStorage.removeItem(avatarIdKey(get().fingerprint))
    set({ avatarId: null, avatarLocalPath: null })
  },

  // Purely local: writes whatever username the caller already confirmed
  // (or cleared) with the ledger — see app/settings/username.tsx, which
  // owns the actual submit/confirm flow, since it needs to show
  // fine-grained progress this store doesn't track.
  persistUsername: async (username) => {
    const key = usernameKey(get().fingerprint)
    if (username) {
      await AsyncStorage.setItem(key, username)
    } else {
      await AsyncStorage.removeItem(key)
    }
    set({ username: username || null })
  },

  // There is no server session to invalidate — this permanently forgets
  // the *active* Keychain identity (see IdentitySession.removeSlot). The
  // local labels above are namespaced by fingerprint, not wiped here: if
  // the *same* identity is ever registered on this device again (its own
  // recovery phrase is the only way back in), its display name/bio/
  // avatar/username come back with it, while a genuinely *different*
  // identity simply reads under its own, empty namespace and never sees
  // them. If another account is also registered on this device, it
  // becomes active automatically (see `afterAccountChange`) instead of
  // forcing a trip through onboarding.
  signOut: async () => {
    nativeSignOut()
    await afterAccountChange(set, get)
  },

  // Instant — every registered slot's full key material already lives in
  // this device's Keychain, so switching never needs a recovery phrase.
  switchAccount: async (slot) => {
    nativeSwitchAccount(slot)
    await afterAccountChange(set, get)
  },

  // Permanent removal of `slot`, regardless of whether it's active — see
  // `signOut`'s doc comment for the same "falls back to another account
  // if one exists" behavior when it is.
  removeAccount: async (slot) => {
    nativeRemoveAccount(slot)
    await afterAccountChange(set, get)
  },
}))

// Shared by signOut/switchAccount/removeAccount: all three can leave this
// device either still logged into *some* account (the new active one, or
// a fallback the native side already switched to) or logged into none —
// re-bootstrap in the first case, reset to the empty pre-onboarding state
// in the second, rather than duplicating this check in three places.
async function afterAccountChange(
  set: (partial: Partial<ProfileState>) => void,
  get: () => ProfileState
) {
  if (hasIdentity()) {
    await get().bootstrap()
  } else {
    set({
      isReady:         false,
      fingerprint:     '',
      publicKey:       '',
      peerId:          '',
      p2pStartupError: null,
      displayName:     '',
      bio:             '',
      avatarId:        null,
      avatarLocalPath: null,
      username:        null,
      accounts:        [],
      activeSlot:      0,
    })
  }
}
