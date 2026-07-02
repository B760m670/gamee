import { useEffect, useRef, useState } from 'react'
import {
  View, Text, TextInput, Pressable, ActivityIndicator, StyleSheet, KeyboardAvoidingView, Platform,
} from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { Avatar } from '../../components/Avatar'
import { lookupUsername, p2pDial, addP2pEventListener, type UsernameLookup } from '../../modules/spiritchat-crypto-core'

type Result =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'invalid'; message: string }
  | { kind: 'notFound' }
  | { kind: 'found'; username: string; lookup: Extract<UsernameLookup, { status: 'resolved' }> }
  | { kind: 'error'; message: string }

type DialState = 'idle' | 'dialing' | 'connected' | 'failed'

// @username search is an exact DHT lookup, not a directory listing — there
// is no server anywhere in this project that could hold a searchable index
// of every handle, so unlike a typical "people search" this only ever
// answers "does this exact handle currently resolve to someone verifiable".
function normalizeQuery(raw: string): string {
  return raw.trim().toLowerCase().replace(/^@/, '')
}

export default function ContactsScreen() {
  const insets = useSafeAreaInsets()
  const [query, setQuery]   = useState('')
  const [result, setResult] = useState<Result>({ kind: 'idle' })
  const [dial, setDial]     = useState<DialState>('idle')
  const token = useRef(0)

  useEffect(() => {
    const normalized = normalizeQuery(query)
    setDial('idle')

    if (normalized.length === 0) {
      setResult({ kind: 'idle' })
      return
    }
    if (normalized.length < 5) {
      setResult({ kind: 'invalid', message: 'Минимум 5 символов' })
      return
    }
    if (!/^[a-z0-9_]+$/.test(normalized)) {
      setResult({ kind: 'invalid', message: 'Только латинские буквы, цифры и «_»' })
      return
    }

    const myToken = ++token.current
    setResult({ kind: 'checking' })
    const timer = setTimeout(async () => {
      try {
        const lookup = await lookupUsername(normalized)
        if (token.current !== myToken) return
        if (lookup.status === 'resolved') {
          setResult({ kind: 'found', username: normalized, lookup })
        } else {
          setResult({ kind: 'notFound' })
        }
      } catch {
        if (token.current !== myToken) return
        setResult({ kind: 'error', message: 'Не удалось выполнить поиск — проверь соединение' })
      }
    }, 500)

    return () => clearTimeout(timer)
  }, [query])

  useEffect(() => {
    if (result.kind !== 'found') return
    const peerId = result.lookup.peerId
    return addP2pEventListener((event) => {
      if (event.type === 'peerConnected' && event.peerId === peerId) {
        setDial('connected')
      } else if (event.type === 'dialFailed' && event.peerId === peerId) {
        setDial('failed')
      }
    })
  }, [result])

  function handleConnect() {
    if (result.kind !== 'found' || dial === 'dialing' || dial === 'connected') return
    setDial('dialing')
    try {
      p2pDial(result.lookup.peerId)
    } catch {
      setDial('failed')
    }
  }

  return (
    <KeyboardAvoidingView style={s.root} behavior={Platform.OS === 'ios' ? 'padding' : 'height'}>
      <View style={[s.header, { paddingTop: insets.top }]}>
        <Text style={s.title}>Contacts</Text>
      </View>

      <View style={s.searchRow}>
        <Ionicons name="at" size={18} color="#52525b" />
        <TextInput
          style={s.searchInput}
          value={query}
          onChangeText={setQuery}
          placeholder="Найти по имени пользователя"
          placeholderTextColor="#52525b"
          autoCapitalize="none"
          autoCorrect={false}
          returnKeyType="search"
        />
        {query.length > 0 ? (
          <Pressable onPress={() => setQuery('')} hitSlop={8}>
            <Ionicons name="close-circle" size={18} color="#52525b" />
          </Pressable>
        ) : null}
      </View>

      {result.kind === 'idle' ? (
        <View style={s.empty}>
          <Ionicons name="people-outline" size={56} color="#27272a" />
          <Text style={s.emptyText}>Найдите человека по его @имени пользователя</Text>
          <Text style={s.emptyHint}>
            Поиск работает только по точному имени — это открытая P2P-сеть без сервера, а не каталог всех пользователей.
          </Text>
        </View>
      ) : result.kind === 'invalid' ? (
        <View style={s.empty}>
          <Text style={s.errorText}>{result.message}</Text>
        </View>
      ) : result.kind === 'error' ? (
        <View style={s.empty}>
          <Text style={s.errorText}>{result.message}</Text>
        </View>
      ) : result.kind === 'checking' ? (
        <View style={s.empty}>
          <ActivityIndicator color="#52525b" />
          <Text style={s.emptyHint}>Ищём в открытой сети — может занять до 30 секунд</Text>
        </View>
      ) : result.kind === 'notFound' ? (
        <View style={s.empty}>
          <Ionicons name="search" size={40} color="#27272a" />
          <Text style={s.emptyText}>Никто не публикует такое имя прямо сейчас</Text>
        </View>
      ) : result.kind === 'found' ? (
        <View style={s.card}>
          <Avatar size={64} uri={null} username={result.username} />
          <Text style={s.cardName}>{`@${result.username}`}</Text>
          <Text style={s.cardFingerprint}>{result.lookup.fingerprint}</Text>

          <Pressable
            style={({ pressed }) => [s.connectBtn, pressed && s.connectBtnPressed, dial === 'connected' && s.connectBtnDone]}
            onPress={handleConnect}
            disabled={dial === 'dialing' || dial === 'connected'}
          >
            {dial === 'dialing' ? (
              <ActivityIndicator size="small" color="#fff" />
            ) : (
              <Text style={s.connectText}>
                {dial === 'connected' ? 'Подключено' : dial === 'failed' ? 'Повторить подключение' : 'Подключиться'}
              </Text>
            )}
          </Pressable>
          {dial === 'failed' ? (
            <Text style={s.errorText}>Не удалось установить соединение — собеседник может быть офлайн</Text>
          ) : null}
        </View>
      ) : null}
    </KeyboardAvoidingView>
  )
}

const s = StyleSheet.create({
  root:   { flex: 1, backgroundColor: '#000000' },
  header: { paddingHorizontal: 20, paddingVertical: 16 },
  title:  { color: '#ffffff', fontSize: 28, fontWeight: '800' },

  searchRow: {
    flexDirection: 'row', alignItems: 'center', gap: 8,
    marginHorizontal: 16, marginBottom: 12, paddingHorizontal: 12, height: 44,
    backgroundColor: '#1c1c1e', borderRadius: 12,
  },
  searchInput: { flex: 1, color: '#fff', fontSize: 16, height: '100%', padding: 0 },

  empty:     { flex: 1, alignItems: 'center', justifyContent: 'center', gap: 12, paddingHorizontal: 32, paddingTop: 40 },
  emptyText: { color: '#a1a1aa', fontSize: 16, textAlign: 'center' },
  emptyHint: { color: '#52525b', fontSize: 13, textAlign: 'center', lineHeight: 18 },
  errorText: { color: '#f87171', fontSize: 14, textAlign: 'center' },

  card: { alignItems: 'center', gap: 6, paddingTop: 40, paddingHorizontal: 32 },
  cardName:        { color: '#fff', fontSize: 20, fontWeight: '700', marginTop: 10 },
  cardFingerprint: { color: '#52525b', fontSize: 13, fontVariant: ['tabular-nums'], marginBottom: 16 },

  connectBtn: {
    minWidth: 180, alignItems: 'center', justifyContent: 'center',
    backgroundColor: '#2f7bff', borderRadius: 12, paddingVertical: 12, paddingHorizontal: 20,
  },
  connectBtnPressed: { opacity: 0.8 },
  connectBtnDone:    { backgroundColor: '#16a34a' },
  connectText:       { color: '#fff', fontSize: 15, fontWeight: '600' },
})
