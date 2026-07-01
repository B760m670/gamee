import { Component, useEffect } from 'react'
import { View, Text, ScrollView } from 'react-native'
import { Stack } from 'expo-router'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { GestureHandlerRootView } from 'react-native-gesture-handler'
import { SafeAreaProvider } from 'react-native-safe-area-context'
import { StatusBar } from 'expo-status-bar'
import { useFonts, Nunito_700Bold, Nunito_800ExtraBold } from '@expo-google-fonts/nunito'
import { fingerprint as cryptoCoreFingerprint } from '../modules/spiritchat-crypto-core'

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
    // Loads (or, on first launch, creates) this device's persistent
    // identity from the Keychain. A stable fingerprint across restarts is
    // the proof that persistence actually works, not just that the
    // native library loaded.
    try {
      console.log('[CryptoCore] identity fingerprint: ' + cryptoCoreFingerprint())
    } catch (err) {
      console.error('[CryptoCore] failed to load/create identity', err)
    }
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
