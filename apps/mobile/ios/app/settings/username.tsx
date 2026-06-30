import { useState } from 'react'
import {
  View, Text, TextInput, Pressable,
  KeyboardAvoidingView, Platform, ScrollView, StyleSheet,
} from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { GlassView } from 'expo-glass-effect'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { useAuthStore, type AppUser } from '../../store/auth'
import { api } from '../../lib/api'
import { validateUsername } from '../../utils/username'

const BTN_H = 42

export default function UsernameScreen() {
  const router = useRouter()
  const insets = useSafeAreaInsets()
  const { user } = useAuthStore(s => ({ user: s.user }))

  const BTN_TOP = insets.top + 12

  const [username, setUsername] = useState(user?.username ?? '')
  const [saving,   setSaving]   = useState(false)
  const [error,    setError]    = useState<string | null>(null)

  const trimmed      = username.trim().toLowerCase()
  const validationErr = trimmed.length > 0 ? validateUsername(trimmed) : null
  const isValid      = trimmed.length === 0 || validationErr === null
  const hasChanges   = trimmed !== (user?.username ?? '')

  function handleChange(t: string) {
    setUsername(t.replace(/[^a-zA-Z0-9_]/g, '').toLowerCase())
    setError(null)
  }

  async function handleSave() {
    if (saving) return
    if (!hasChanges) { router.back(); return }
    if (validationErr) return
    setError(null)
    setSaving(true)
    try {
      const body: Record<string, unknown> = {
        display_name: user?.display_name ?? '',
        username:     trimmed || null,
      }
      const res = await api.put<{ data: AppUser }>('/api/v1/users/me', body)
      useAuthStore.setState({ user: res.data })
      router.back()
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : 'Ошибка сохранения'
      setError(/duplicate|unique|already/i.test(msg) ? 'Это имя пользователя уже занято' : msg)
      setSaving(false)
    }
  }

  const statusColor = trimmed.length === 0
    ? 'transparent'
    : isValid ? '#34d399' : '#f87171'

  return (
    <KeyboardAvoidingView
      style={s.root}
      behavior={Platform.OS === 'ios' ? 'padding' : 'height'}
    >
      {/* Отмена */}
      <View style={[s.btnOverlay, { top: BTN_TOP, left: 16 }]}>
        <Pressable onPress={() => router.back()}>
          <GlassView style={s.backBtn} glassEffectStyle="regular" isInteractive colorScheme="dark">
            <Ionicons name="chevron-back" size={22} color="#fff" />
          </GlassView>
        </Pressable>
      </View>

      {/* Готово */}
      <View style={[s.btnOverlay, { top: BTN_TOP, right: 16 }]}>
        <Pressable onPress={handleSave}>
          <GlassView style={s.pillShape} glassEffectStyle="regular" isInteractive colorScheme="dark">
            <Text style={s.btnLabel}>Готово</Text>
          </GlassView>
        </Pressable>
      </View>

      <ScrollView
        keyboardShouldPersistTaps="handled"
        contentContainerStyle={[s.scroll, { paddingTop: insets.top + 70, paddingBottom: insets.bottom + 40 }]}
        showsVerticalScrollIndicator={false}
      >
        <Text style={s.title}>Имя пользователя</Text>

        {/* Input */}
        <View style={s.inputWrap}>
          <Text style={s.at}>@</Text>
          <TextInput
            style={s.input}
            value={username}
            onChangeText={handleChange}
            placeholder="username"
            placeholderTextColor="#3f3f46"
            autoCapitalize="none"
            autoCorrect={false}
            autoFocus
            maxLength={32}
            returnKeyType="done"
            onSubmitEditing={handleSave}
          />
          {trimmed.length > 0 && (
            <Ionicons
              name={isValid ? 'checkmark-circle' : 'close-circle'}
              size={20}
              color={statusColor}
            />
          )}
        </View>

        {/* Validation / error */}
        {(validationErr || error) ? (
          <Text style={s.errorText}>{error ?? validationErr}</Text>
        ) : null}

        {/* Description */}
        <Text style={s.desc}>
          Установите имя пользователя, чтобы другие люди могли легко находить вас и писать вам.{'\n\n'}
          Допустимы только латинские буквы, цифры и «_». Минимум 5 символов.{'\n\n'}
          Чтобы убрать имя пользователя, оставьте поле пустым.
        </Text>
      </ScrollView>
    </KeyboardAvoidingView>
  )
}

const s = StyleSheet.create({
  root:  { flex: 1, backgroundColor: '#000' },
  scroll: { paddingHorizontal: 16 },

  title: { color: '#fff', fontSize: 28, fontWeight: '700', marginBottom: 24 },

  inputWrap: {
    flexDirection: 'row', alignItems: 'center',
    backgroundColor: '#111114', borderRadius: 14,
    borderWidth: 1, borderColor: '#27272a',
    paddingHorizontal: 16, height: 54, gap: 4,
    marginBottom: 8,
  },
  at:    { color: '#52525b', fontSize: 17 },
  input: { flex: 1, color: '#fff', fontSize: 17, height: '100%' },

  errorText: { color: '#f87171', fontSize: 13, marginLeft: 4, marginBottom: 4 },

  desc: { color: '#52525b', fontSize: 14, lineHeight: 20, marginTop: 16 },

  btnOverlay: { position: 'absolute', zIndex: 10 },
  backBtn: {
    width: BTN_H, height: BTN_H, borderRadius: BTN_H / 2,
    alignItems: 'center', justifyContent: 'center',
  },
  pillShape: {
    height: BTN_H, paddingHorizontal: 18, borderRadius: BTN_H / 2,
    alignItems: 'center', justifyContent: 'center',
  },
  btnLabel: { color: '#fff', fontSize: 15, fontWeight: '500' },
})
