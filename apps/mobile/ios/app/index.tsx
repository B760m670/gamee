import { useEffect } from 'react'
import { View, ActivityIndicator } from 'react-native'
import { useRouter } from 'expo-router'
import { useProfileStore } from '../store/profile'

export default function Index() {
  const router    = useRouter()
  const bootstrap = useProfileStore(s => s.bootstrap)
  const isReady   = useProfileStore(s => s.isReady)

  useEffect(() => {
    bootstrap()
  }, [])

  useEffect(() => {
    // There's no account to log into — the device's Keychain identity
    // (loaded above) is always available once bootstrap resolves, so the
    // only gate is "has it loaded yet", not "is there a session token".
    if (isReady) router.replace('/(tabs)/messages')
  }, [isReady])

  return (
    <View style={{ flex: 1, backgroundColor: '#000000', alignItems: 'center', justifyContent: 'center' }}>
      <ActivityIndicator color="#2f7bff" size="large" />
    </View>
  )
}
