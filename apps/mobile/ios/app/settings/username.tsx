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
import {
  queryLedgerUsernameOwner,
  submitLedgerUsernameClaim,
  queryLedgerChainTip,
  addP2pEventListener,
  LEDGER_CONFIRMATION_DEPTH,
} from '../../modules/spiritchat-crypto-core'

const BTN_H = 44

// Whether this exact @handle currently resolves to someone on the ledger —
// separate from `SubmitStatus` below, which only exists once the user has
// actually pressed "Готово" for a *new* name.
type CheckStatus =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'available' }
  | { kind: 'mine' }
  | { kind: 'taken' }
  | { kind: 'invalid'; message: string }
  | { kind: 'error'; message: string }

// The claim's own lifecycle after submission — a block's worth of
// probabilistic finality (see the project's ledger design) means this can't
// resolve instantly, so the screen stays open and shows real progress
// instead of an optimistic instant `router.back()`.
type SubmitStatus =
  | { kind: 'idle' }
  | { kind: 'submitting' }
  | { kind: 'pending' }
  | { kind: 'confirming'; depth: number }
  | { kind: 'confirmed' }
  | { kind: 'rejected'; reason: string }

export default function UsernameScreen() {
  const router = useRouter()
  const insets = useSafeAreaInsets()
  const { username: savedUsername, publicKey, persistUsername } = useProfileStore(s => ({
    username:        s.username,
    publicKey:        s.publicKey,
    persistUsername: s.persistUsername,
  }))

  const [text, setText]     = useState(savedUsername ?? '')
  const [check, setCheck]   = useState<CheckStatus>({ kind: 'idle' })
  const [submit, setSubmit] = useState<SubmitStatus>({ kind: 'idle' })
  const checkToken = useRef(0)

  const normalized = text.trim().toLowerCase()

  // Checks whether the typed name is available — paused once a submission
  // is in flight, since editing the input is hidden then anyway.
  useEffect(() => {
    if (submit.kind !== 'idle') return

    if (normalized === (savedUsername ?? '')) {
      setCheck({ kind: 'idle' })
      return
    }
    if (normalized.length === 0) {
      setCheck({ kind: 'idle' })
      return
    }
    const validationError = validateUsername(normalized)
    if (validationError) {
      setCheck({ kind: 'invalid', message: validationError })
      return
    }

    const token = ++checkToken.current
    setCheck({ kind: 'checking' })
    const timer = setTimeout(async () => {
      try {
        const owner = await queryLedgerUsernameOwner(normalized)
        if (checkToken.current !== token) return
        if (owner.status === 'not_found') {
          setCheck({ kind: 'available' })
        } else if (owner.ownerPublicKeyBase64 === publicKey) {
          setCheck({ kind: 'mine' })
        } else {
          setCheck({ kind: 'taken' })
        }
      } catch {
        if (checkToken.current !== token) return
        setCheck({ kind: 'error', message: 'Не удалось проверить — проверь соединение и попробуй ещё раз' })
      }
    }, 500)

    return () => clearTimeout(timer)
  }, [text, savedUsername, publicKey, submit.kind])

  // Once a claim is submitted, tracks it toward confirmation: checks
  // immediately, then again on every chain tip change, until it's either
  // confirmed, lost to a competing claim, or this screen unmounts (the
  // claim itself keeps going either way — this is only local UI state).
  useEffect(() => {
    if (submit.kind !== 'pending' && submit.kind !== 'confirming') return
    let cancelled = false

    const check = async () => {
      try {
        const owner = await queryLedgerUsernameOwner(normalized)
        if (cancelled || owner.status !== 'found') return
        if (owner.ownerPublicKeyBase64 !== publicKey) {
          setSubmit({ kind: 'rejected', reason: 'Кто-то другой закрепил это имя раньше' })
          return
        }
        const tip = await queryLedgerChainTip()
        if (cancelled) return
        const depth = tip.height - owner.claimedAtHeight + 1
        setSubmit(depth >= LEDGER_CONFIRMATION_DEPTH ? { kind: 'confirmed' } : { kind: 'confirming', depth })
      } catch {
        // A transient query failure isn't a rejection — just wait for the
        // next tip change to try again.
      }
    }

    check()
    const unsubscribe = addP2pEventListener((event) => {
      if (event.type === 'chainTipChanged') check()
    })
    return () => {
      cancelled = true
      unsubscribe()
    }
  }, [submit.kind, normalized, publicKey])

  const editing = submit.kind === 'idle'
  const canSave =
    editing &&
    normalized !== (savedUsername ?? '') &&
    (normalized.length === 0 || check.kind === 'available' || check.kind === 'mine')

  async function handleSave() {
    if (!canSave) return

    if (normalized.length === 0) {
      await persistUsername('')
      router.back()
      return
    }
    if (check.kind === 'mine') {
      // Already confirmed and owned by this device — nothing to
      // (re)submit, just make sure the locally saved label matches.
      await persistUsername(normalized)
      router.back()
      return
    }

    setSubmit({ kind: 'submitting' })
    try {
      await submitLedgerUsernameClaim(normalized)
      await persistUsername(normalized)
      setSubmit({ kind: 'pending' })
    } catch (e) {
      setSubmit({ kind: 'rejected', reason: e instanceof Error ? e.message : 'Не удалось отправить заявку' })
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

      {editing ? (
        <View style={[s.btnOverlay, { top: insets.top + 10, right: 16 }]}>
          <Pressable onPress={handleSave} disabled={!canSave}>
            <GlassView style={[s.saveBtn, !canSave && s.saveBtnDisabled]} glassEffectStyle="regular" isInteractive colorScheme="dark">
              <Text style={[s.saveText, !canSave && s.saveTextDisabled]}>Готово</Text>
            </GlassView>
          </Pressable>
        </View>
      ) : null}

      <ScrollView
        keyboardShouldPersistTaps="handled"
        contentContainerStyle={{ paddingTop: insets.top + 70, paddingBottom: insets.bottom + 40, paddingHorizontal: 24 }}
        showsVerticalScrollIndicator={false}
      >
        <Text style={s.title}>Имя пользователя</Text>

        {editing ? (
          <>
            <Text style={s.subtitle}>
              Необязательно. Имя закрепляется в собственном децентрализованном реестре — небольшом
              proof-of-work блокчейне, который поддерживают постоянно работающие узлы сети, без единого
              сервера. Твоё устройство только отправляет заявку — вычислениями оно не занимается. После
              подтверждения (~30 минут, {LEDGER_CONFIRMATION_DEPTH} блоков) имя гарантированно закреплено только за
              тобой. Пока сеть небольшая, у более мощного участника есть теоретическая возможность ненадолго
              переписать последние блоки — этот риск снижается по мере роста сети, а имя всегда можно сменить.
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
              {check.kind === 'checking' && <ActivityIndicator size="small" color="#52525b" />}
            </View>

            <CheckStatusLine status={check} />

            {savedUsername ? (
              <Pressable
                style={({ pressed }) => [s.clearBtn, pressed && s.clearBtnPressed]}
                onPress={() => setText('')}
              >
                <Text style={s.clearText}>Убрать имя пользователя</Text>
              </Pressable>
            ) : null}
          </>
        ) : (
          <SubmitProgress username={normalized} status={submit} onRetry={() => setSubmit({ kind: 'idle' })} onDone={() => router.back()} />
        )}
      </ScrollView>
    </KeyboardAvoidingView>
  )
}

function CheckStatusLine({ status }: { status: CheckStatus }) {
  switch (status.kind) {
    case 'checking':
      return <Text style={s.statusHint}>Проверяем в локальном состоянии сети…</Text>
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

function SubmitProgress({
  username, status, onRetry, onDone,
}: {
  username: string
  status: Exclude<SubmitStatus, { kind: 'idle' }>
  onRetry: () => void
  onDone: () => void
}) {
  return (
    <View style={s.progress}>
      <Text style={s.progressHandle}>{`@${username}`}</Text>

      {status.kind === 'submitting' ? (
        <>
          <ActivityIndicator color="#71717a" style={s.progressSpinner} />
          <Text style={s.progressHint}>Отправляем заявку в сеть…</Text>
        </>
      ) : status.kind === 'pending' ? (
        <>
          <ActivityIndicator color="#71717a" style={s.progressSpinner} />
          <Text style={s.progressHint}>Заявка в сети — ждём, пока кто-то из участников её замайнит</Text>
        </>
      ) : status.kind === 'confirming' ? (
        <>
          <ActivityIndicator color="#71717a" style={s.progressSpinner} />
          <Text style={s.progressHint}>{`Подтверждений: ${status.depth} из ${LEDGER_CONFIRMATION_DEPTH}`}</Text>
        </>
      ) : status.kind === 'confirmed' ? (
        <>
          <Ionicons name="checkmark-circle" size={40} color="#4ade80" style={s.progressSpinner} />
          <Text style={s.progressOk}>Имя подтверждено и закреплено за тобой</Text>
          <Pressable style={({ pressed }) => [s.doneBtn, pressed && s.doneBtnPressed]} onPress={onDone}>
            <Text style={s.doneText}>Готово</Text>
          </Pressable>
        </>
      ) : (
        <>
          <Ionicons name="close-circle" size={40} color="#f87171" style={s.progressSpinner} />
          <Text style={s.progressErr}>{status.reason}</Text>
          <Pressable style={({ pressed }) => [s.doneBtn, pressed && s.doneBtnPressed]} onPress={onRetry}>
            <Text style={s.doneText}>Попробовать снова</Text>
          </Pressable>
        </>
      )}
    </View>
  )
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

  progress: { alignItems: 'center', paddingTop: 40, gap: 4 },
  progressHandle: { color: '#fff', fontSize: 22, fontWeight: '700', marginBottom: 16 },
  progressSpinner: { marginBottom: 12 },
  progressHint: { color: '#a1a1aa', fontSize: 14, textAlign: 'center', lineHeight: 20 },
  progressOk:   { color: '#4ade80', fontSize: 15, textAlign: 'center', lineHeight: 20 },
  progressErr:  { color: '#f87171', fontSize: 15, textAlign: 'center', lineHeight: 20 },

  doneBtn: {
    marginTop: 24, backgroundColor: '#2f7bff', borderRadius: 12,
    paddingVertical: 12, paddingHorizontal: 24,
  },
  doneBtnPressed: { opacity: 0.8 },
  doneText:       { color: '#fff', fontSize: 15, fontWeight: '600' },

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
