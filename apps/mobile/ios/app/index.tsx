import { useEffect, useState } from 'react'
import { View, ActivityIndicator } from 'react-native'
import { useRouter } from 'expo-router'
import { useProfileStore } from '../store/profile'
import { hasIdentity } from '../modules/spiritchat-crypto-core'

export default function Index() {
  const router    = useRouter()
  const bootstrap = useProfileStore(s => s.bootstrap)
  const isReady   = useProfileStore(s => s.isReady)
  const [checkedIdentity, setCheckedIdentity] = useState(false)

  useEffect(() => {
    // There's no account to log into — this device's Keychain identity,
    // once it exists, is always available with no session/token to expire.
    // The only real fork is whether one has ever been created here yet.
    if (hasIdentity()) {
      bootstrap()
    } else {
      router.replace('/(onboarding)/welcome')
    }
    setCheckedIdentity(true)
  }, [])

  useEffect(() => {
    if (checkedIdentity && isReady) router.replace('/(tabs)/messages')
  }, [checkedIdentity, isReady])

  return (
    <View style={{ flex: 1, backgroundColor: '#000000', alignItems: 'center', justifyContent: 'center' }}>
      <ActivityIndicator color="#2f7bff" size="large" />
    </View>
  )
}
