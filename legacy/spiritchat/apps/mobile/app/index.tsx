import { useEffect } from 'react'
import { View, ActivityIndicator } from 'react-native'
import { useRouter } from 'expo-router'
import { useAuthStore } from '../store/auth'

export default function Index() {
  const router          = useRouter()
  const bootstrap       = useAuthStore(s => s.bootstrap)
  const isBootstrapping = useAuthStore(s => s.isBootstrapping)
  const token           = useAuthStore(s => s.token)

  useEffect(() => {
    bootstrap()
  }, [])

  useEffect(() => {
    if (isBootstrapping) return
    if (token) {
      router.replace('/(tabs)/messages')
    } else {
      router.replace('/(auth)/login')
    }
  }, [isBootstrapping, token])

  return (
    <View style={{ flex: 1, backgroundColor: '#000000', alignItems: 'center', justifyContent: 'center' }}>
      <ActivityIndicator color="#2f7bff" size="large" />
    </View>
  )
}
