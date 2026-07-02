import { useEffect, useRef, useState } from 'react'
import {
  View, Text, TextInput, Pressable, ScrollView, StyleSheet, ActivityIndicator, KeyboardAvoidingView, Platform,
} from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { GlassView } from 'expo-glass-effect'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { useProfileStore } from '../../store/profile'
import { validateUsername } from '../../utils/username'
import { lookupUsername } from '../../modules/spiritchat-crypto-core'

const BTN_H = 44

type Status =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'available' }
  | { kind: 'mine' }
  | { kind: 'taken' }
  | { kind: 'invalid'; message: string }
  | { kind: 'error'; message: string }

export default function UsernameScreen() {
  const router = useRouter()
  const insets = useSafeAreaInsets()
  const { username: savedUsername, fingerprint, setUsername } = useProfileStore(s => ({
    username:    s.username,
    fingerprint: s.fingerprint,
    setUsername: s.setUsername,
  }))

  const [text, setText]     = useState(savedUsername ?? '')
  const [status, setStatus] = useState<Status>({ kind: 'idle' })
  const [saving, setSaving] = useState(false)
  const checkToken = useRef(0)

  useEffect(() => {
    const normalized = text.trim().toLowerCase()

    if (normalized === (savedUsername ?? '')) {
      setStatus({ kind: 'idle' })
      return
    }
    if (normalized.length === 0) {
      setStatus({ kind: 'idle' })
      return
    }
    const validationError = validateUsername(normalized)
    if (validationError) {
      setStatus({ kind: 'invalid', message: validationError })
      return
    }

    const token = ++checkToken.current
    setStatus({ kind: 'checking' })
    const timer = setTimeout(async () => {
      try {
        const lookup = await lookupUsername(normalized)
        if (checkToken.current !== token) return
        if (lookup.status === 'available') {
          setStatus({ kind: 'available' })
        } else if (lookup.status === 'resolved' && lookup.fingerprint === fingerprint) {
          setStatus({ kind: 'mine' })
        } else {
          setStatus({ kind: 'taken' })
        }
      } catch {
        if (checkToken.current !== token) return
        setStatus({ kind: 'error', message: 'Не удалось проверить — проверь соединение и попробуй ещё раз' })
      }
    }, 500)

    return () => clearTimeout(timer)
  }, [text, savedUsername, fingerprint])

  const normalized = text.trim().toLowerCase()
  const canSave =
    !saving &&
    normalized !== (savedUsername ?? '') &&
    (normalized.length === 0 || status.kind === 'available' || status.kind === 'mine')

  async function handleSave() {
    if (!canSave) return
    setSaving(true)
    try {
      await setUsername(normalized)
      router.back()
    } catch (e) {
      setStatus({ kind: 'error', message: e instanceof Error ? e.message : 'Не удалось сохранить' })
    } finally {
      setSaving(false)
    }
  }

  return (
    <KeyboardAvoidingView style={s.root} behavior={Platform.OS === 'ios' ? 'padding' : 'height'}>
      <View style={[s.btnOverlay, { top: insets.top + 10, left: 16 }]}>
        <Pressable onPress={() => router.back()}>
          <GlassView style={s.backBtn} glassEffectStyle="regular" isInteractive colorScheme="dark">
            <Ionicons name="chevron-back" size={22} color="#fff" />
          </GlassView>
        </Pressable>
      </View>

      <View style={[s.btnOverlay, { top: insets.top + 10, right: 16 }]}>
        <Pressable onPress={handleSave} disabled={!canSave}>
          <GlassView style={[s.saveBtn, !canSave && s.saveBtnDisabled]} glassEffectStyle="regular" isInteractive colorScheme="dark">
            {saving
              ? <ActivityIndicator color="#fff" size="small" />
              : <Text style={[s.saveText, !canSave && s.saveTextDisabled]}>Готово</Text>
            }
          </GlassView>
        </Pressable>
      </View>

      <ScrollView
        keyboardShouldPersistTaps="handled"
        contentContainerStyle={{ paddingTop: insets.top + 70, paddingBottom: insets.bottom + 40, paddingHorizontal: 24 }}
        showsVerticalScrollIndicator={false}
      >
        <Text style={s.title}>Имя пользователя</Text>
        <Text style={s.subtitle}>
          Необязательно. Если задать, тебя можно будет найти по точному @имени. Оно публикуется в открытой P2P-сети (DHT) без сервера — уникальность не гарантирована железно, только тем, что имя подписано твоим ключом.
        </Text>

        <View style={s.inputRow}>
          <Text style={s.at}>@</Text>
          <TextInput
            style={s.input}
            value={text}
            onChangeText={(t) => setText(t.replace(/\s/g, ''))}
            placeholder="username"
            placeholderTextColor="#3f3f46"
            autoCapitalize="none"
            autoCorrect={false}
            maxLength={32}
            returnKeyType="done"
            onSubmitEditing={handleSave}
          />
          {status.kind === 'checking' && <ActivityIndicator size="small" color="#52525b" />}
        </View>

        <StatusLine status={status} />

        {savedUsername ? (
          <Pressable
            style={({ pressed }) => [s.clearBtn, pressed && s.clearBtnPressed]}
            onPress={() => setText('')}
          >
            <Text style={s.clearText}>Убрать имя пользователя</Text>
          </Pressable>
        ) : null}
      </ScrollView>
    </KeyboardAvoidingView>
  )
}

function StatusLine({ status }: { status: Status }) {
  switch (status.kind) {
    case 'checking':
      // A DHT lookup walks the network to answer, so this can take a
      // while — say so, rather than leave a bare spinner that looks stuck.
      return <Text style={s.statusHint}>Ищём в сети — может занять до 30 секунд</Text>
    case 'available':
      return <Text style={s.statusOk}>Свободно</Text>
    case 'mine':
      return <Text style={s.statusOk}>Уже закреплено за тобой</Text>
    case 'taken':
      return <Text style={s.statusErr}>Занято другим аккаунтом</Text>
    case 'invalid':
      return <Text style={s.statusErr}>{status.message}</Text>
    case 'error':
      return <Text style={s.statusErr}>{status.message}</Text>
    default:
      return null
  }
}

const s = StyleSheet.create({
  root: { flex: 1, backgroundColor: '#000' },

  title:    { color: '#fff', fontSize: 26, fontWeight: '700', marginBottom: 10 },
  subtitle: { color: '#71717a', fontSize: 14, lineHeight: 20, marginBottom: 24 },

  inputRow: {
    flexDirection: 'row', alignItems: 'center', gap: 4,
    backgroundColor: '#111114', borderWidth: 1, borderColor: '#27272a',
    borderRadius: 14, paddingHorizontal: 16, paddingVertical: 14,
  },
  at:    { color: '#71717a', fontSize: 17, fontWeight: '600' },
  input: { flex: 1, color: '#fff', fontSize: 17, paddingVertical: 0 },

  statusOk:   { color: '#4ade80', fontSize: 13, marginTop: 10, marginHorizontal: 4 },
  statusHint: { color: '#71717a', fontSize: 13, marginTop: 10, marginHorizontal: 4 },
  statusErr: { color: '#f87171', fontSize: 13, marginTop: 10, marginHorizontal: 4 },

  clearBtn:        { marginTop: 28, alignItems: 'center', paddingVertical: 12 },
  clearBtnPressed: { opacity: 0.7 },
  clearText:       { color: '#ef4444', fontSize: 15, fontWeight: '500' },

  btnOverlay: { position: 'absolute', zIndex: 10 },
  backBtn: {
    width: BTN_H, height: BTN_H, borderRadius: BTN_H / 2,
    alignItems: 'center', justifyContent: 'center',
  },
  saveBtn: {
    height: BTN_H, paddingHorizontal: 14, borderRadius: BTN_H / 2,
    alignItems: 'center', justifyContent: 'center',
  },
  saveBtnDisabled: { opacity: 0.4 },
  saveText:        { color: '#fff', fontSize: 17, fontWeight: '600' },
  saveTextDisabled: { color: '#a1a1aa' },
})
