import { useEffect, useState } from 'react'
import {
  View, Text, Pressable, ScrollView, ActivityIndicator, StyleSheet,
} from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { generateRecoveryPhrase, setIdentityFromWords } from '../../modules/spiritchat-crypto-core'
import { useProfileStore } from '../../store/profile'

export default function CreateAccountScreen() {
  const router    = useRouter()
  const insets    = useSafeAreaInsets()
  const bootstrap = useProfileStore(s => s.bootstrap)

  const [words, setWords]         = useState<string[]>([])
  const [confirmed, setConfirmed] = useState(false)
  const [creating, setCreating]   = useState(false)
  const [error, setError]         = useState<string | null>(null)

  useEffect(() => {
    setWords(generateRecoveryPhrase().split(' '))
  }, [])

  async function handleContinue() {
    if (!confirmed || creating || words.length === 0) return
    setCreating(true)
    setError(null)
    try {
      setIdentityFromWords(words.join(' '))
      await bootstrap()
      // This screen is reachable two ways: fresh onboarding (nothing else
      // in the stack, dismissAll is a no-op) and "add account" from deep
      // in Settings → Accounts (where it isn't) — dismissAll before
      // replacing keeps a later back-navigation from popping into a
      // stale, pre-switch settings screen either way.
      router.dismissAll()
      router.replace('/(tabs)/messages')
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Не удалось создать аккаунт')
      setCreating(false)
    }
  }

  return (
    <ScrollView
      style={s.root}
      contentContainerStyle={{ paddingTop: insets.top + 24, paddingBottom: insets.bottom + 24, paddingHorizontal: 24 }}
      showsVerticalScrollIndicator={false}
    >
      <Text style={s.title}>Фраза восстановления</Text>
      <Text style={s.subtitle}>
        Это единственный способ восстановить аккаунт. Здесь нет сервера — никто не сможет
        сбросить пароль или прислать код. Запиши эти 12 слов на бумаге, по порядку, и храни
        в надёжном месте.
      </Text>

      <View style={s.warningBox}>
        <Ionicons name="warning" size={18} color="#f59e0b" />
        <Text style={s.warningText}>
          Если ты потеряешь и фразу, и это устройство — аккаунт нельзя будет восстановить.
        </Text>
      </View>

      <View style={s.grid}>
        {words.map((word, i) => (
          <View key={i} style={s.wordCell}>
            <Text style={s.wordIndex}>{i + 1}</Text>
            <Text style={s.wordText}>{word}</Text>
          </View>
        ))}
      </View>

      {error ? (
        <View style={s.errorBox}>
          <Text style={s.errorText}>{error}</Text>
        </View>
      ) : null}

      <Pressable style={s.confirmRow} onPress={() => setConfirmed(c => !c)}>
        <View style={[s.checkbox, confirmed && s.checkboxChecked]}>
          {confirmed ? <Ionicons name="checkmark" size={14} color="#fff" /> : null}
        </View>
        <Text style={s.confirmText}>Я записал фразу в надёжном месте</Text>
      </Pressable>

      <Pressable
        style={({ pressed }) => [s.btn, (!confirmed || creating) && s.btnDisabled, pressed && confirmed && s.btnPressed]}
        onPress={handleContinue}
        disabled={!confirmed || creating}
      >
        {creating
          ? <ActivityIndicator color="#fff" />
          : <Text style={s.btnText}>Продолжить</Text>
        }
      </Pressable>
    </ScrollView>
  )
}

const s = StyleSheet.create({
  root: { flex: 1, backgroundColor: '#000000' },

  title:    { color: '#ffffff', fontSize: 26, fontWeight: '700', marginBottom: 10 },
  subtitle: { color: '#71717a', fontSize: 14, lineHeight: 20, marginBottom: 16 },

  warningBox: {
    flexDirection: 'row', gap: 10, alignItems: 'flex-start',
    backgroundColor: 'rgba(245,158,11,0.12)', borderWidth: 1, borderColor: 'rgba(245,158,11,0.4)',
    borderRadius: 12, padding: 12, marginBottom: 20,
  },
  warningText: { flex: 1, color: '#f59e0b', fontSize: 13, lineHeight: 18 },

  grid: { flexDirection: 'row', flexWrap: 'wrap', gap: 10, marginBottom: 20 },
  wordCell: {
    width: '47%', flexDirection: 'row', alignItems: 'center', gap: 8,
    backgroundColor: '#111114', borderWidth: 1, borderColor: '#27272a',
    borderRadius: 10, paddingHorizontal: 12, paddingVertical: 12,
  },
  wordIndex: { color: '#52525b', fontSize: 13, width: 18 },
  wordText:  { color: '#ffffff', fontSize: 15, fontWeight: '600' },

  errorBox:  { backgroundColor: 'rgba(127,29,29,0.4)', borderRadius: 12, borderWidth: 1, borderColor: '#b91c1c', padding: 12, marginBottom: 16 },
  errorText: { color: '#f87171', fontSize: 14 },

  confirmRow: { flexDirection: 'row', alignItems: 'center', gap: 10, marginBottom: 24 },
  checkbox: {
    width: 22, height: 22, borderRadius: 6, borderWidth: 1.5, borderColor: '#3f3f46',
    alignItems: 'center', justifyContent: 'center',
  },
  checkboxChecked: { backgroundColor: '#2f7bff', borderColor: '#2f7bff' },
  confirmText:     { color: '#d4d4d8', fontSize: 14, flex: 1 },

  btn:         { borderRadius: 14, paddingVertical: 17, alignItems: 'center', backgroundColor: '#2f7bff' },
  btnDisabled: { backgroundColor: '#1a3d7a' },
  btnPressed:  { opacity: 0.85 },
  btnText:     { color: '#ffffff', fontWeight: '700', fontSize: 17 },
})
