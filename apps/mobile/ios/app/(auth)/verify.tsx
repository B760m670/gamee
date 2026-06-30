import { useState, useRef, useEffect } from 'react'
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
import { Ionicons } from '@expo/vector-icons'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { useAuthStore } from '../../store/auth'

const CODE_LENGTH = 6
const RESEND_SECONDS = 60

// Mask email: "username@gmail.com" → "us****@gmail.com"
function maskEmail(email: string): string {
  const at = email.indexOf('@')
  if (at < 0) return email
  const local = email.slice(0, at)
  const domain = email.slice(at)
  const visible = local.slice(0, Math.min(2, local.length))
  const stars = '*'.repeat(Math.max(1, Math.min(local.length - visible.length, 4)))
  return visible + stars + domain
}

export default function VerifyScreen() {
  const [code, setCode]   = useState('')
  const [timer, setTimer] = useState(RESEND_SECONDS)
  const inputRef          = useRef<TextInput>(null)
  const intervalRef       = useRef<ReturnType<typeof setInterval> | null>(null)

  const router = useRouter()
  const insets = useSafeAreaInsets()

  const { sendOtp, verifyOtp, email, devCode, isLoading, error, clearError } = useAuthStore()

  useEffect(() => {
    startTimer()
    return () => clearInterval(intervalRef.current ?? undefined)
  }, [])

  function startTimer() {
    setTimer(RESEND_SECONDS)
    clearInterval(intervalRef.current ?? undefined)
    intervalRef.current = setInterval(() => {
      setTimer(t => {
        if (t <= 1) { clearInterval(intervalRef.current ?? undefined); return 0 }
        return t - 1
      })
    }, 1000)
  }

  async function handleResend() {
    if (!email || timer > 0) return
    clearError()
    try {
      await sendOtp(email)
      startTimer()
    } catch {}
  }

  async function handleVerify(value?: string) {
    const digits = (value ?? code).replace(/\D/g, '')
    if (digits.length < CODE_LENGTH || !email) return
    clearError()
    const wasAddingAccount = useAuthStore.getState().isAddingAccount
    try {
      await verifyOtp(email, digits)
      if (wasAddingAccount) {
        router.replace('/(tabs)/settings')
      } else {
        router.replace(useAuthStore.getState().isNew ? '/(auth)/setup' : '/(tabs)/messages')
      }
    } catch {}
  }

  function handleCodeChange(v: string) {
    const digits = v.replace(/\D/g, '').slice(0, CODE_LENGTH)
    setCode(digits)
    if (digits.length === CODE_LENGTH) handleVerify(digits)
  }

  const masked = email ? maskEmail(email) : ''

  return (
    <KeyboardAvoidingView
      style={s.root}
      behavior={Platform.OS === 'ios' ? 'padding' : 'height'}
    >
      <View style={[s.inner, { paddingTop: insets.top + 12, paddingBottom: insets.bottom + 20 }]}>

        {/* Back button */}
        <TouchableOpacity style={s.backBtn} onPress={() => router.back()} hitSlop={8}>
          <Ionicons name="arrow-back" size={22} color="#a1a1aa" />
        </TouchableOpacity>

        {/* Title */}
        <View style={s.titleBlock}>
          <Text style={s.title}>Enter the code</Text>
          {masked ? (
            <Text style={s.subtitle}>
              We sent a 6-digit code to{'\n'}
              <Text style={s.highlight}>{masked}</Text>
            </Text>
          ) : (
            <Text style={s.subtitle}>Enter the code we sent to your email.</Text>
          )}
        </View>

        {/* Error */}
        {error ? (
          <View style={s.errorBox}>
            <Text style={s.errorText}>{error}</Text>
          </View>
        ) : null}

        {/* Dev hint */}
        {devCode ? (
          <TouchableOpacity style={s.devBox} onPress={() => handleCodeChange(devCode)} activeOpacity={0.7}>
            <Ionicons name="construct-outline" size={14} color="#a1a1aa" />
            <Text style={s.devText}>Dev code: <Text style={s.devCode}>{devCode}</Text> · tap to fill</Text>
          </TouchableOpacity>
        ) : null}

        {/* Code boxes */}
        <TouchableOpacity activeOpacity={1} onPress={() => inputRef.current?.focus()} style={s.boxes}>
          {Array.from({ length: CODE_LENGTH }).map((_, i) => {
            const char  = code[i] ?? ''
            const isCur = code.length === i && !isLoading
            return (
              <View key={i} style={[s.box, char && s.boxFilled, isCur && s.boxActive]}>
                <Text style={s.boxChar}>{char}</Text>
                {isCur ? <View style={s.cursor} /> : null}
              </View>
            )
          })}
        </TouchableOpacity>

        {/* Hidden input */}
        <TextInput
          ref={inputRef}
          value={code}
          onChangeText={handleCodeChange}
          keyboardType="number-pad"
          maxLength={CODE_LENGTH}
          autoFocus
          style={s.hiddenInput}
          caretHidden
        />

        {/* Confirm button */}
        <TouchableOpacity
          style={[s.btn, (isLoading || code.length < CODE_LENGTH) && s.btnDisabled]}
          onPress={() => handleVerify()}
          disabled={isLoading || code.length < CODE_LENGTH}
          activeOpacity={0.85}
        >
          {isLoading
            ? <ActivityIndicator color="#fff" />
            : <Text style={s.btnText}>Confirm</Text>
          }
        </TouchableOpacity>

        {/* Resend */}
        <View style={s.resendRow}>
          {timer > 0 ? (
            <Text style={s.timerText}>
              Resend code in <Text style={s.timerNum}>{timer}s</Text>
            </Text>
          ) : (
            <TouchableOpacity onPress={handleResend} activeOpacity={0.7}>
              <Text style={s.resendText}>Resend code</Text>
            </TouchableOpacity>
          )}
        </View>

      </View>
    </KeyboardAvoidingView>
  )
}

const s = StyleSheet.create({
  root:  { flex: 1, backgroundColor: '#000000' },
  inner: { flex: 1, paddingHorizontal: 24 },

  backBtn: { marginBottom: 24, alignSelf: 'flex-start' },

  titleBlock: { marginBottom: 32, alignItems: 'center' },
  title:      { color: '#ffffff', fontSize: 28, fontWeight: '700', letterSpacing: -0.3, textAlign: 'center', marginBottom: 10 },
  subtitle:   { color: '#71717a', fontSize: 15, textAlign: 'center', lineHeight: 22 },
  highlight:  { color: '#a1a1aa', fontWeight: '600' },

  errorBox:  { marginBottom: 16, borderRadius: 10, backgroundColor: 'rgba(127,29,29,0.5)', borderWidth: 1, borderColor: '#b91c1c', paddingHorizontal: 16, paddingVertical: 12 },
  errorText: { color: '#f87171', fontSize: 14, textAlign: 'center' },

  devBox:  { flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 6, marginBottom: 16, borderRadius: 10, backgroundColor: '#111114', borderWidth: 1, borderColor: '#27272a', paddingHorizontal: 16, paddingVertical: 10 },
  devText: { color: '#a1a1aa', fontSize: 13 },
  devCode: { color: '#2f7bff', fontWeight: '700' },

  boxes: { flexDirection: 'row', gap: 10, justifyContent: 'center', marginBottom: 28 },
  box:   {
    width: 46, height: 56, borderRadius: 12,
    backgroundColor: '#111114', borderWidth: 1.5, borderColor: '#27272a',
    alignItems: 'center', justifyContent: 'center',
  },
  boxFilled: { borderColor: '#3f3f46' },
  boxActive: { borderColor: '#2f7bff' },
  boxChar:   { color: '#ffffff', fontSize: 24, fontWeight: '600' },
  cursor:    { position: 'absolute', bottom: 10, width: 2, height: 22, backgroundColor: '#2f7bff', borderRadius: 1 },

  hiddenInput: { position: 'absolute', width: 0, height: 0, opacity: 0 },

  btn:         { borderRadius: 14, paddingVertical: 17, alignItems: 'center', backgroundColor: '#2f7bff', marginBottom: 20 },
  btnDisabled: { backgroundColor: '#1a3d7a' },
  btnText:     { color: '#ffffff', fontWeight: '700', fontSize: 17 },

  resendRow:  { alignItems: 'center' },
  timerText:  { color: '#52525b', fontSize: 14 },
  timerNum:   { color: '#71717a', fontWeight: '600' },
  resendText: { color: '#2f7bff', fontSize: 15, fontWeight: '600' },
})
