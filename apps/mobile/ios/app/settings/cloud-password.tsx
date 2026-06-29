import { useState } from 'react'
import {
  View, Text, TextInput, Pressable,
  ScrollView, KeyboardAvoidingView, Platform, StyleSheet,
} from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { GlassView } from 'expo-glass-effect'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'

const BTN_H = 42

export default function CloudPasswordScreen() {
  const router = useRouter()
  const insets = useSafeAreaInsets()

  const [password,    setPassword]    = useState('')
  const [confirm,     setConfirm]     = useState('')
  const [hint,        setHint]        = useState('')
  const [showPass,    setShowPass]    = useState(false)
  const [showConfirm, setShowConfirm] = useState(false)
  const [error,       setError]       = useState<string | null>(null)

  const canSave = password.length >= 6 && confirm.length > 0

  function handleSave() {
    if (password !== confirm) {
      setError('Пароли не совпадают')
      return
    }
    setError(null)
    // TODO: connect to backend
  }

  return (
    <KeyboardAvoidingView
      style={s.root}
      behavior={Platform.OS === 'ios' ? 'padding' : 'height'}
    >
      {/* Nav */}
      <View style={[s.nav, { paddingTop: insets.top + 8 }]}>
        <Pressable onPress={() => router.back()} style={s.backWrap}>
          <GlassView style={s.backBtn} glassEffectStyle="regular" isInteractive colorScheme="dark">
            <Ionicons name="chevron-back" size={22} color="#fff" />
          </GlassView>
        </Pressable>
        <Text style={s.navTitle}>Облачный пароль</Text>
        <View style={s.navSpacer} />
      </View>

      <ScrollView
        contentContainerStyle={[s.scroll, { paddingBottom: insets.bottom + 40 }]}
        keyboardShouldPersistTaps="handled"
        showsVerticalScrollIndicator={false}
      >
        {/* Icon + description */}
        <View style={s.hero}>
          <View style={s.heroIcon}>
            <Ionicons name="lock-closed" size={36} color="#fff" />
          </View>
          <Text style={s.heroTitle}>Двухэтапная проверка</Text>
          <Text style={s.heroDesc}>
            Добавьте дополнительный пароль для входа в аккаунт.{'\n'}
            Он понадобится вместе с кодом из email.
          </Text>
        </View>

        {error ? (
          <View style={s.errorBox}>
            <Text style={s.errorText}>{error}</Text>
          </View>
        ) : null}

        {/* Password fields */}
        <View style={s.group}>
          <View style={[s.fieldRow, s.fieldBorder]}>
            <Text style={s.fieldLabel}>Новый пароль</Text>
            <View style={s.passwordRow}>
              <TextInput
                style={[s.fieldInput, { flex: 1 }]}
                value={password}
                onChangeText={setPassword}
                placeholder="Минимум 6 символов"
                placeholderTextColor="#3f3f46"
                secureTextEntry={!showPass}
                autoCapitalize="none"
                autoCorrect={false}
              />
              <Pressable onPress={() => setShowPass(v => !v)} hitSlop={8}>
                <Ionicons
                  name={showPass ? 'eye-off-outline' : 'eye-outline'}
                  size={20}
                  color="#52525b"
                />
              </Pressable>
            </View>
          </View>

          <View style={[s.fieldRow, s.fieldBorder]}>
            <Text style={s.fieldLabel}>Повторите пароль</Text>
            <View style={s.passwordRow}>
              <TextInput
                style={[s.fieldInput, { flex: 1 }]}
                value={confirm}
                onChangeText={t => { setConfirm(t); setError(null) }}
                placeholder="Введите пароль ещё раз"
                placeholderTextColor="#3f3f46"
                secureTextEntry={!showConfirm}
                autoCapitalize="none"
                autoCorrect={false}
              />
              <Pressable onPress={() => setShowConfirm(v => !v)} hitSlop={8}>
                <Ionicons
                  name={showConfirm ? 'eye-off-outline' : 'eye-outline'}
                  size={20}
                  color="#52525b"
                />
              </Pressable>
            </View>
          </View>

          <View style={s.fieldRow}>
            <Text style={s.fieldLabel}>Подсказка (необязательно)</Text>
            <TextInput
              style={s.fieldInput}
              value={hint}
              onChangeText={setHint}
              placeholder="Подсказка для пароля"
              placeholderTextColor="#3f3f46"
              autoCapitalize="none"
              autoCorrect={false}
              maxLength={128}
            />
          </View>
        </View>

        <Text style={s.hint}>
          Подсказка будет показана при запросе пароля. Не используйте сам пароль в качестве подсказки.
        </Text>

        {/* Save button */}
        <Pressable
          onPress={handleSave}
          disabled={!canSave}
          style={({ pressed }) => [s.saveBtn, !canSave && s.saveBtnDisabled, pressed && canSave && s.saveBtnPressed]}
        >
          <Text style={[s.saveBtnText, !canSave && s.saveBtnTextDisabled]}>
            Установить пароль
          </Text>
        </Pressable>
      </ScrollView>
    </KeyboardAvoidingView>
  )
}

const s = StyleSheet.create({
  root:   { flex: 1, backgroundColor: '#000' },

  nav: {
    flexDirection: 'row', alignItems: 'center',
    paddingHorizontal: 16, paddingBottom: 10,
  },
  backWrap:  { zIndex: 1 },
  backBtn: {
    width: BTN_H, height: BTN_H, borderRadius: BTN_H / 2,
    alignItems: 'center', justifyContent: 'center',
  },
  navTitle:  { flex: 1, color: '#fff', fontSize: 17, fontWeight: '600', textAlign: 'center' },
  navSpacer: { width: BTN_H },

  scroll: { paddingHorizontal: 16, paddingTop: 32 },

  hero: { alignItems: 'center', marginBottom: 32 },
  heroIcon: {
    width: 80, height: 80, borderRadius: 40,
    backgroundColor: '#1c1c1e',
    alignItems: 'center', justifyContent: 'center',
    marginBottom: 20,
  },
  heroTitle: { color: '#fff', fontSize: 22, fontWeight: '700', marginBottom: 10 },
  heroDesc:  { color: '#8e8e93', fontSize: 15, lineHeight: 22, textAlign: 'center' },

  errorBox:  { backgroundColor: 'rgba(127,29,29,0.4)', borderRadius: 12, borderWidth: 1, borderColor: '#b91c1c', padding: 12, marginBottom: 12 },
  errorText: { color: '#f87171', fontSize: 14 },

  group:       { backgroundColor: '#111114', borderRadius: 16, overflow: 'hidden' },
  fieldRow:    { paddingHorizontal: 16, paddingVertical: 12, gap: 4 },
  fieldBorder: { borderBottomWidth: StyleSheet.hairlineWidth, borderBottomColor: '#27272a' },
  fieldLabel:  { color: '#a1a1aa', fontSize: 12, fontWeight: '500' },
  fieldInput:  { color: '#fff', fontSize: 16, paddingVertical: 2 },
  passwordRow: { flexDirection: 'row', alignItems: 'center', gap: 8 },

  hint: { color: '#3f3f46', fontSize: 12, lineHeight: 18, marginTop: 10, marginHorizontal: 4, marginBottom: 28 },

  saveBtn:             { backgroundColor: '#2f7bff', borderRadius: 14, paddingVertical: 16, alignItems: 'center' },
  saveBtnDisabled:     { backgroundColor: '#1a3d7a' },
  saveBtnPressed:      { backgroundColor: '#1a6aee' },
  saveBtnText:         { color: '#fff', fontSize: 16, fontWeight: '700' },
  saveBtnTextDisabled: { color: 'rgba(255,255,255,0.4)' },
})
