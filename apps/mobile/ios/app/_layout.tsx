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
  requestLedgerChainSync,
} from '../modules/spiritchat-crypto-core'

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

export default function RootLayout() {
  const [fontsLoaded] = useFonts({ Nunito_700Bold, Nunito_800ExtraBold })

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
