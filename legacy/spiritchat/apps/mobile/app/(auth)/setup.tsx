import { useState } from 'react'
import {
  View,
  Text,
  TextInput,
  TouchableOpacity,
  KeyboardAvoidingView,
  Platform,
  ActivityIndicator,
  StyleSheet,
} from 'react-native'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { api } from '../../lib/api'
import { useAuthStore, type AppUser } from '../../store/auth'
import { validateUsername } from '../../utils/username'

export default function SetupScreen() {
  const [displayName, setDisplayName] = useState('')
  const [username, setUsername]       = useState('')
  const [isSaving, setIsSaving]       = useState(false)
  const [error, setError]             = useState<string | null>(null)

  const router = useRouter()
  const insets = useSafeAreaInsets()

  const trimmedUsername = username.trim().toLowerCase()
  const usernameError   = trimmedUsername.length > 0 ? validateUsername(trimmedUsername) : null
  const canSave = displayName.trim().length > 0 && usernameError === null && !isSaving

  function handleUsernameChange(text: string) {
    setUsername(text.replace(/[^a-zA-Z0-9_]/g, ''))
  }

  async function handleSave() {
    if (!canSave) return
    setError(null)
    setIsSaving(true)
    try {
      const body: Record<string, unknown> = {
        display_name: displayName.trim(),
        onboarded: true,
      }
      if (trimmedUsername) body.username = trimmedUsername

      const res = await api.put<{ data: AppUser }>('/api/v1/users/me', body)
      useAuthStore.setState({ user: res.data, isNew: false })
      router.replace('/(tabs)/messages')
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : 'Failed to save profile'
      // Surface a friendlier message for the most common conflict.
      setError(/duplicate|unique|already/i.test(message) ? 'That username is already taken' : message)
      setIsSaving(false)
    }
  }

  return (
    <KeyboardAvoidingView
      style={s.root}
      behavior={Platform.OS === 'ios' ? 'padding' : 'height'}
    >
      <View style={[s.inner, { paddingTop: insets.top + 40, paddingBottom: insets.bottom + 20 }]}>

        <View style={s.titleBlock}>
          <Text style={s.title}>Set up your profile</Text>
          <Text style={s.subtitle}>Tell us your name. You can change it later.</Text>
        </View>

        {error ? (
          <View style={s.errorBox}>
            <Text style={s.errorText}>{error}</Text>
          </View>
        ) : null}

        {/* Display name */}
        <Text style={s.label}>Name</Text>
        <TextInput
          style={s.input}
          placeholder="Your name"
          placeholderTextColor="#3f3f46"
          value={displayName}
          onChangeText={setDisplayName}
          autoFocus
          maxLength={50}
          returnKeyType="next"
        />

        {/* Username (optional) */}
        <View style={s.labelRow}>
          <Text style={s.label}>Username</Text>
          <Text style={s.optional}>Optional</Text>
        </View>
        <View style={[s.input, s.usernameWrap, usernameError ? s.inputError : null]}>
          <Text style={s.at}>@</Text>
          <TextInput
            style={s.usernameInput}
            placeholder="username"
            placeholderTextColor="#3f3f46"
            value={username}
            onChangeText={handleUsernameChange}
            autoCapitalize="none"
            autoCorrect={false}
            maxLength={32}
            onSubmitEditing={handleSave}
            returnKeyType="done"
          />
        </View>
        {usernameError ? <Text style={s.hint}>{usernameError}</Text> : null}

        <TouchableOpacity
          style={[s.btn, !canSave && s.btnDisabled]}
          onPress={handleSave}
          disabled={!canSave}
          activeOpacity={0.85}
        >
          {isSaving
            ? <ActivityIndicator color="#fff" />
            : <Text style={s.btnText}>Continue</Text>
          }
        </TouchableOpacity>

      </View>
    </KeyboardAvoidingView>
  )
}

const s = StyleSheet.create({
  root:  { flex: 1, backgroundColor: '#000000' },
  inner: { flex: 1, paddingHorizontal: 24 },

  titleBlock: { marginBottom: 32 },
  title:      { color: '#ffffff', fontSize: 28, fontWeight: '700', letterSpacing: -0.3, marginBottom: 8 },
  subtitle:   { color: '#71717a', fontSize: 15, lineHeight: 22 },

  errorBox:  { marginBottom: 16, borderRadius: 10, backgroundColor: 'rgba(127,29,29,0.5)', borderWidth: 1, borderColor: '#b91c1c', paddingHorizontal: 16, paddingVertical: 12 },
  errorText: { color: '#f87171', fontSize: 14 },

  label:    { color: '#a1a1aa', fontSize: 13, marginBottom: 6, marginLeft: 4 },
  labelRow: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', marginTop: 18, marginRight: 4 },
  optional: { color: '#52525b', fontSize: 12 },

  input: {
    backgroundColor: '#111114', borderRadius: 14, borderWidth: 1, borderColor: '#27272a',
    paddingHorizontal: 16, height: 54, color: '#ffffff', fontSize: 17, justifyContent: 'center',
  },
  inputError: { borderColor: '#b91c1c' },

  usernameWrap:  { flexDirection: 'row', alignItems: 'center', paddingHorizontal: 16 },
  at:            { color: '#52525b', fontSize: 17 },
  usernameInput: { flex: 1, color: '#ffffff', fontSize: 17, marginLeft: 2, height: '100%' },

  hint: { color: '#f87171', fontSize: 12, marginTop: 6, marginLeft: 4 },

  btn:         { borderRadius: 14, paddingVertical: 17, alignItems: 'center', backgroundColor: '#2f7bff', marginTop: 32 },
  btnDisabled: { backgroundColor: '#1a3d7a' },
  btnText:     { color: '#ffffff', fontWeight: '700', fontSize: 17 },
})
