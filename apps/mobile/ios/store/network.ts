import { create } from 'zustand'
import type { P2pEvent } from '../modules/spiritchat-crypto-core'

/**
 * What this device's network layer is actually doing right now.
 *
 * Exists because none of it was observable. Every question that matters
 * when a message doesn't arrive — is this node connected to anyone at all,
 * did it ever reach the DHT, does it have an address a distant peer could
 * dial — had no answer anywhere in the UI, so a failed send was
 * indistinguishable from a peer being offline, from the node never having
 * started, from the two devices simply never having found each other.
 *
 * Purely observational: it is fed from the one global `addP2pEventListener`
 * in `app/_layout.tsx` and never issues a command of its own. Nothing in
 * the messaging path reads it, so it cannot influence delivery — it only
 * reports.
 *
 * Not persisted, deliberately. Every value here describes the current
 * process's live connections; restoring yesterday's peer list from disk
 * would produce a screen that looks informative and is lying.
 */
interface NetworkState {
  /** Peers with at least one live connection, newest first. */
  connectedPeers: string[]
  /** Peers discovered on the local network via mDNS, connected or not. */
  locallyDiscovered: string[]
  /**
   * Whether any peer has ever connected in this process. Distinguishes
   * "connected to nobody right now" from "never got off the ground",
   * which are very different problems with the same empty peer list.
   */
  everConnected: boolean
  /** Ledger chain tip height, as last reported by `chainTipChanged`. */
  chainHeight: number | null
  /** The most recent dial failure, kept for the diagnostics screen. */
  lastDialFailure: { peerId: string | null; reason: string; at: number } | null

  handleEvent: (event: P2pEvent) => void
  /** Forgets everything — for account switch/sign-out. */
  reset: () => void
}

export const useNetworkStore = create<NetworkState>((set) => ({
  connectedPeers: [],
  locallyDiscovered: [],
  everConnected: false,
  chainHeight: null,
  lastDialFailure: null,

  handleEvent: (event) => {
    switch (event.type) {
      case 'peerConnected':
        set(state => ({
          // `PeerConnected` now fires only for a peer's first connection
          // (see node.rs), but this stays idempotent regardless: a
          // duplicate must not produce a duplicate row.
          connectedPeers: state.connectedPeers.includes(event.peerId)
            ? state.connectedPeers
            : [event.peerId, ...state.connectedPeers],
          everConnected: true,
        }))
        break

      case 'peerDisconnected':
        set(state => ({
          connectedPeers: state.connectedPeers.filter(id => id !== event.peerId),
        }))
        break

      case 'peerDiscoveredLocally':
        set(state => ({
          locallyDiscovered: state.locallyDiscovered.includes(event.peerId)
            ? state.locallyDiscovered
            : [event.peerId, ...state.locallyDiscovered],
        }))
        break

      case 'chainTipChanged':
        set({ chainHeight: event.height })
        break

      case 'dialFailed':
        set({
          lastDialFailure: {
            peerId: event.peerId ?? null,
            reason: event.reason,
            at: Date.now(),
          },
        })
        break
    }
  },

  reset: () => set({
    connectedPeers: [],
    locallyDiscovered: [],
    everConnected: false,
    chainHeight: null,
    lastDialFailure: null,
  }),
}))
