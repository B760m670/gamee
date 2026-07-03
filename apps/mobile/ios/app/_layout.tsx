import { Component, useEffect } from 'react'
import { View, Text, ScrollView } from 'react-native'
import { Stack } from 'expo-router'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { GestureHandlerRootView } from 'react-native-gesture-handler'
import { SafeAreaProvider } from 'react-native-safe-area-context'
import { StatusBar } from 'expo-status-bar'
import { useFonts, Nunito_700Bold, Nunito_800ExtraBold } from '@expo-google-fonts/nunito'
import {
  hasIdentity,
  fingerprint as cryptoCoreFingerprint,
  p2pLocalPeerId,
  addP2pEventListener,
  addChatEventListener,
  requestLedgerChainSync,
} from '../modules/spiritchat-crypto-core'
import { useProfileStore } from '../store/profile'
import { useChatStore } from '../store/chat'

const queryClient = new QueryClient({
  defaultOptions: { queries: { staleTime: 30_000, retry: 2 } },
})

class ErrorBoundary extends Component<{ children: React.ReactNode }, { error: Error | null }> {
  state = { error: null }
  static getDerivedStateFromError(error: Error) { return { error } }
  render() {
    if (this.state.error) {
      return (
        <ScrollView style={{ flex: 1, backgroundColor: '#000', padding: 20 }}>
          <Text style={{ color: '#ef4444', fontSize: 16, fontWeight: '700', marginBottom: 8 }}>
            App Error
          </Text>
          <Text style={{ color: '#fca5a5', fontSize: 13 }}>{String(this.state.error)}</Text>
        </ScrollView>
      )
    }
    return this.props.children
  }
}

/**
 * Global, always-visible surface for `p2pStartupError` (see profile.ts's
 * doc comment on the field) — onboarding no longer blocks on a P2P
 * startup failure, so this is the only place left where the real
 * underlying reason is still shown, in an environment (sideloaded via
 * LiveContainer) where standard OS crash/diagnostic logs aren't reliably
 * reachable. Not dismissible: it reflects live state (see profile.ts's
 * background retry loop in `bootstrap`) and disappears on its own the
 * moment P2P actually comes up.
 */
function P2pStartupErrorBanner() {
  const error = useProfileStore(s => s.p2pStartupError)
  if (!error) return null
  return (
    <View style={{
      position: 'absolute', top: 50, left: 12, right: 12, zIndex: 999,
      backgroundColor: 'rgba(127,29,29,0.92)', borderRadius: 12, borderWidth: 1, borderColor: '#b91c1c',
      padding: 12,
    }}>
      <Text style={{ color: '#fecaca', fontSize: 11, fontWeight: '700', marginBottom: 4 }}>
        P2P не запустился
      </Text>
      <Text style={{ color: '#fecaca', fontSize: 11 }}>{error}</Text>
    </View>
  )
}

export default function RootLayout() {
  const [fontsLoaded] = useFonts({ Nunito_700Bold, Nunito_800ExtraBold })
  const fingerprint = useProfileStore(s => s.fingerprint)

  // Chat events are handled entirely in the store (see store/chat.ts) —
  // this just wires the native event stream to it, once for the app's
  // lifetime; ChatManager.swift already only ever emits for whichever
  // account is currently active, so nothing here needs to change on an
  // account switch.
  useEffect(() => {
    return addChatEventListener((event) => {
      useChatStore.getState().handleChatEvent(event)
    })
  }, [])

  // Loads the active account's own conversations whenever `fingerprint`
  // changes — on first bootstrap, and again on every account switch, since
  // a different fingerprint means an entirely different, namespaced set of
  // conversations (see store/chat.ts's AsyncStorage key helpers). Resets
  // to empty when it goes blank (signed out of every account).
  useEffect(() => {
    if (fingerprint) {
      useChatStore.getState().loadForFingerprint(fingerprint)
    } else {
      useChatStore.getState().reset()
    }
  }, [fingerprint])

  useEffect(() => {
    // On a fresh install there's no identity yet — onboarding (create or
    // restore, see app/(onboarding)) hasn't run, so the calls below would
    // throw. index.tsx handles that routing; this effect only logs the
    // already-loaded identity as a smoke test, so skip it rather than log
    // a misleading "failed" error for what's actually the expected state.
    if (!hasIdentity()) return

    // A stable fingerprint across restarts is the proof that Keychain
    // persistence actually works, not just that the native library loaded.
    try {
      console.log('[CryptoCore] identity fingerprint: ' + cryptoCoreFingerprint())
    } catch (err) {
      console.error('[CryptoCore] failed to load identity', err)
    }

    // The P2P node is started natively as soon as the module loads (see
    // P2pSession.swift); this just confirms it's alive and starts
    // forwarding its events. There is no server — `p2pLocalPeerId()` is
    // this device's address on the public IPFS DHT it joined directly.
    try {
      console.log('[P2P] local peer id: ' + p2pLocalPeerId())
    } catch (err) {
      console.error('[P2P] failed to read local peer id', err)
    }
    const unsubscribe = addP2pEventListener((event) => {
      console.log('[P2P] event: ' + JSON.stringify(event))

      // Opportunistically catches this node's @username ledger up to
      // whatever a newly connected peer knows. Without this, a freshly
      // launched (or long-idle) app only sees blocks minted *after* it
      // happened to be listening, and a username availability check or
      // contact search could give a wrong answer purely because this
      // device hasn't caught up yet — a no-op once already at least as
      // heavy as the peer, so this is safe to fire on every connection.
      if (event.type === 'peerConnected') {
        requestLedgerChainSync(event.peerId).catch(() => {})
      }
    })
    return unsubscribe
  }, [])

  return (
    <ErrorBoundary>
      <GestureHandlerRootView style={{ flex: 1, backgroundColor: '#000000' }}>
        <SafeAreaProvider style={{ backgroundColor: '#000000' }}>
          <QueryClientProvider client={queryClient}>
            <StatusBar style="light" />
            <P2pStartupErrorBanner />
            <Stack
              screenOptions={{
                headerShown: false,
                contentStyle: { backgroundColor: '#000000' },
                cardStyle:    { backgroundColor: '#000000' },
              }}
            >
              <Stack.Screen name="settings/edit-profile" options={{ animation: 'fade', gestureEnabled: false }} />
            </Stack>
          </QueryClientProvider>
        </SafeAreaProvider>
      </GestureHandlerRootView>
    </ErrorBoundary>
  )
}
