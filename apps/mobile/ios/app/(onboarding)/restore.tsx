import { useState } from 'react'
import {
  View, Text, TextInput, Pressable, KeyboardAvoidingView, Platform,
  ActivityIndicator, ScrollView, StyleSheet,
} from 'react-native'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { setIdentityFromWords } from '../../modules/spiritchat-crypto-core'
import { useProfileStore } from '../../store/profile'

export default function RestoreAccountScreen() {
  const router    = useRouter()
  const insets    = useSafeAreaInsets()
  const bootstrap = useProfileStore(s => s.bootstrap)

  const [input, setInput]         = useState('')
  const [restoring, setRestoring] = useState(false)
  const [error, setError]         = useState<string | null>(null)

  const normalized = input.trim().split(/\s+/).filter(Boolean).join(' ')
  const wordCount  = normalized ? normalized.split(' ').length : 0
  const canSubmit  = wordCount === 12 && !restoring

  async function handleRestore() {
    if (!canSubmit) return
    setRestoring(true)
    setError(null)
    try {
      setIdentityFromWords(normalized)
      await bootstrap()
      // See create.tsx's identical comment: this screen is also reachable
      // via "add account" from deep in Settings → Accounts, not just
      // fresh onboarding.
      router.dismissAll()
      router.replace('/(tabs)/messages')
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Неверная фраза восстановления')
      setRestoring(false)
    }
  }

  return (
    <KeyboardAvoidingView style={s.root} behavior={Platform.OS === 'ios' ? 'padding' : 'height'}>
      <ScrollView
        contentContainerStyle={{ paddingTop: insets.top + 24, paddingBottom: insets.bottom + 24, paddingHorizontal: 24 }}
        keyboardShouldPersistTaps="handled"
        showsVerticalScrollIndicator={false}
      >
        <Text style={s.title}>Восстановление аккаунта</Text>
        <Text style={s.subtitle}>
          Введи свою фразу из 12 слов, по порядку, через пробел.
        </Text>

        <TextInput
          style={s.input}
          value={input}
          onChangeText={setInput}
          placeholder="слово1 слово2 слово3 ..."
          placeholderTextColor="#3f3f46"
          autoCapitalize="none"
          autoCorrect={false}
          multiline
          textAlignVertical="top"
        />

        <Text style={s.count}>{wordCount} / 12 слов</Text>

        {error ? (
          <View style={s.errorBox}>
            <Text style={s.errorText}>{error}</Text>
          </View>
        ) : null}

        <Pressable
          style={({ pressed }) => [s.btn, !canSubmit && s.btnDisabled, pressed && canSubmit && s.btnPressed]}
          onPress={handleRestore}
          disabled={!canSubmit}
        >
          {restoring
            ? <ActivityIndicator color="#fff" />
            : <Text style={s.btnText}>Восстановить</Text>
          }
        </Pressable>
      </ScrollView>
    </KeyboardAvoidingView>
  )
}

const s = StyleSheet.create({
  root: { flex: 1, backgroundColor: '#000000' },

  title:    { color: '#ffffff', fontSize: 26, fontWeight: '700', marginBottom: 10 },
  subtitle: { color: '#71717a', fontSize: 14, lineHeight: 20, marginBottom: 20 },

  input: {
    backgroundColor: '#111114', borderRadius: 14, borderWidth: 1, borderColor: '#27272a',
    paddingHorizontal: 16, paddingVertical: 14, color: '#ffffff', fontSize: 16,
    minHeight: 120, lineHeight: 22,
  },
  count: { color: '#52525b', fontSize: 12, textAlign: 'right', marginTop: 6, marginBottom: 20 },

  errorBox:  { backgroundColor: 'rgba(127,29,29,0.4)', borderRadius: 12, borderWidth: 1, borderColor: '#b91c1c', padding: 12, marginBottom: 16 },
  errorText: { color: '#f87171', fontSize: 14 },

  btn:         { borderRadius: 14, paddingVertical: 17, alignItems: 'center', backgroundColor: '#2f7bff' },
  btnDisabled: { backgroundColor: '#1a3d7a' },
  btnPressed:  { opacity: 0.85 },
  btnText:     { color: '#ffffff', fontWeight: '700', fontSize: 17 },
})
