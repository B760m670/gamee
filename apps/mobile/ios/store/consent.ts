import { create } from 'zustand'
import { setConsent, consentList, type ConsentStance } from '../modules/spiritchat-crypto-core'

export type { ConsentStance }

/**
 * Mirror of the native `ConsentStore` — who this account refuses to hear
 * from, and how strongly. See `docs/consent-and-moderation.md`.
 *
 * Deliberately a *mirror* and not the source of truth. The list that decides
 * whether an envelope is dropped lives on the native side, because dropping
 * has to happen before decryption and often before JS is even running (see
 * `ConsentStore.swift`). What lives here exists only so screens can render
 * without a synchronous bridge call per row, and every mutation goes to
 * native first — if that throws, this store is left untouched rather than
 * showing a block that isn't real.
 *
 * Unlike the other stores in this app there is no AsyncStorage persistence
 * and no per-fingerprint namespacing: the native side already persists per
 * account slot, and duplicating that here would create a second copy that
 * could disagree with the one doing the enforcing.
 */
interface ConsentState {
  /**
   * Only peers with an actual stance. `Partial` rather than a plain `Record`
   * on purpose: a missing key is the common case and means `'none'`, and the
   * type should say so instead of promising a value for every peer id.
   */
  stances: Partial<Record<string, Exclude<ConsentStance, 'none'>>>
  loaded: boolean

  /** Reads the native list. Safe to call repeatedly; cheap. */
  load: () => void
  /** Forgets everything in memory — for account switch/sign-out. */
  reset: () => void
  /**
   * Applies a stance, native first. Returns whether it took effect, so a
   * screen can avoid claiming success when the bridge refused (no session
   * yet, unknown stance).
   */
  apply: (peerId: string, stance: ConsentStance) => boolean

  stanceFor: (peerId: string) => ConsentStance
  isBlocked: (peerId: string) => boolean
  isRestricted: (peerId: string) => boolean
}

export const useConsentStore = create<ConsentState>((set, get) => ({
  stances: {},
  loaded: false,

  load: () => {
    try {
      const list = consentList()
      const stances: Partial<Record<string, Exclude<ConsentStance, 'none'>>> = {}
      for (const entry of list) stances[entry.peerId] = entry.stance
      set({ stances, loaded: true })
    } catch {
      // No live session yet (app still starting, or signed out). Leaving
      // `loaded` false means a screen can tell "nothing blocked" apart from
      // "not read yet" and retry rather than render an empty list as truth.
    }
  },

  reset: () => set({ stances: {}, loaded: false }),

  apply: (peerId, stance) => {
    try {
      setConsent(peerId, stance)
    } catch {
      return false
    }
    set(state => {
      const stances = { ...state.stances }
      if (stance === 'none') delete stances[peerId]
      else stances[peerId] = stance
      return { stances }
    })
    return true
  },

  stanceFor: (peerId) => get().stances[peerId] ?? 'none',
  isBlocked: (peerId) => get().stances[peerId] === 'blocked',
  isRestricted: (peerId) => get().stances[peerId] === 'restricted',
}))
