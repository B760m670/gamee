import { View, Text, Pressable, StyleSheet } from 'react-native'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'

export default function WelcomeScreen() {
  const router = useRouter()
  const insets = useSafeAreaInsets()

  return (
    <View style={[s.root, { paddingTop: insets.top + 40, paddingBottom: insets.bottom + 24 }]}>
      <View style={s.titleBlock}>
        <Text style={s.title}>SpiritChat</Text>
        <Text style={s.subtitle}>
          Децентрализованный мессенджер без серверов. Твой аккаунт — это криптографическая
          идентичность на этом устройстве, а не запись в чьей-то базе данных.
        </Text>
      </View>

      <View style={s.buttons}>
        <Pressable
          style={({ pressed }) => [s.btn, s.btnPrimary, pressed && s.btnPressed]}
          onPress={() => router.push('/(onboarding)/create')}
        >
          <Text style={s.btnPrimaryText}>Создать аккаунт</Text>
        </Pressable>
        <Pressable
          style={({ pressed }) => [s.btn, s.btnSecondary, pressed && s.btnPressed]}
          onPress={() => router.push('/(onboarding)/restore')}
        >
          <Text style={s.btnSecondaryText}>У меня есть фраза восстановления</Text>
        </Pressable>
      </View>
    </View>
  )
}

const s = StyleSheet.create({
  root:  { flex: 1, backgroundColor: '#000000', paddingHorizontal: 24, justifyContent: 'space-between' },

  titleBlock: { marginTop: 40 },
  title:      { color: '#ffffff', fontSize: 34, fontWeight: '700', textAlign: 'center', marginBottom: 16 },
  subtitle:   { color: '#71717a', fontSize: 15, lineHeight: 22, textAlign: 'center' },

  buttons: { gap: 12 },
  btn:     { borderRadius: 14, paddingVertical: 17, alignItems: 'center' },
  btnPressed: { opacity: 0.8 },

  btnPrimary:     { backgroundColor: '#2f7bff' },
  btnPrimaryText: { color: '#ffffff', fontWeight: '700', fontSize: 17 },

  btnSecondary:     { backgroundColor: '#111114', borderWidth: 1, borderColor: '#27272a' },
  btnSecondaryText: { color: '#2f7bff', fontWeight: '600', fontSize: 16 },
})
