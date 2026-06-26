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
import { useAuthStore } from '../../store/auth'

const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/

export default function LoginScreen() {
  const [email, setEmail] = useState('')

  const router = useRouter()
  const insets = useSafeAreaInsets()
  const { sendOtp, isLoading, error, clearError, isAddingAccount, cancelAddAccount } = useAuthStore()

  const isValid = EMAIL_RE.test(email.trim())

  async function handleSend() {
    if (!isValid || isLoading) return
    clearError()
    try {
      await sendOtp(email.trim().toLowerCase())
      router.push('/(auth)/verify')
    } catch {}
  }

  function handleCancel() {
    cancelAddAccount()
    router.back()
  }

  return (
    <KeyboardAvoidingView
      style={s.root}
      behavior={Platform.OS === 'ios' ? 'padding' : 'height'}
    >
      <View style={[s.inner, { paddingTop: insets.top + 12, paddingBottom: insets.bottom + 20 }]}>

        {isAddingAccount && (
          <TouchableOpacity style={s.backBtn} onPress={handleCancel} hitSlop={8}>
            <Text style={s.cancelText}>Отмена</Text>
          </TouchableOpacity>
        )}

        <View style={[s.titleBlock, isAddingAccount && { marginTop: 20 }]}>
          <Text style={s.title}>Enter your email</Text>
          <Text style={s.subtitle}>
            We'll send you a 6-digit code to sign in or create an account.
          </Text>
        </View>

        {error ? (
          <View style={s.errorBox}>
            <Text style={s.errorText}>{error}</Text>
          </View>
        ) : null}

        <TextInput
          style={s.input}
          placeholder="you@example.com"
          placeholderTextColor="#3f3f46"
          keyboardType="email-address"
          textContentType="emailAddress"
          autoCapitalize="none"
          autoCorrect={false}
          value={email}
          onChangeText={setEmail}
          onSubmitEditing={handleSend}
          returnKeyType="done"
          autoFocus
        />

        <TouchableOpacity
          style={[s.btn, (!isValid || isLoading) && s.btnDisabled]}
          onPress={handleSend}
          disabled={!isValid || isLoading}
          activeOpacity={0.85}
        >
          {isLoading
            ? <ActivityIndicator color="#fff" />
            : <Text style={s.btnText}>Send Code</Text>
          }
        </TouchableOpacity>

      </View>
    </KeyboardAvoidingView>
  )
}

const s = StyleSheet.create({
  root:  { flex: 1, backgroundColor: '#000000' },
  inner: { flex: 1, paddingHorizontal: 24 },

  titleBlock: { marginBottom: 36, alignItems: 'center' },
  title:      { color: '#ffffff', fontSize: 28, fontWeight: '700', letterSpacing: -0.3, textAlign: 'center', marginBottom: 10 },
  subtitle:   { color: '#71717a', fontSize: 15, textAlign: 'center', lineHeight: 22 },

  errorBox:  { marginBottom: 14, borderRadius: 10, backgroundColor: 'rgba(127,29,29,0.5)', borderWidth: 1, borderColor: '#b91c1c', paddingHorizontal: 16, paddingVertical: 12 },
  errorText: { color: '#f87171', fontSize: 14 },

  input: {
    backgroundColor: '#111114', borderRadius: 14, borderWidth: 1, borderColor: '#27272a',
    paddingHorizontal: 16, height: 54, color: '#ffffff', fontSize: 17, marginBottom: 16,
  },

  backBtn:    { marginBottom: 16, alignSelf: 'flex-start' },
  cancelText: { color: '#2f7bff', fontSize: 17 },

  btn:         { borderRadius: 14, paddingVertical: 17, alignItems: 'center', backgroundColor: '#2f7bff' },
  btnDisabled: { backgroundColor: '#1a3d7a' },
  btnText:     { color: '#ffffff', fontWeight: '700', fontSize: 17 },
})
