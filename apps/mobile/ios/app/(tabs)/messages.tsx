import { useMemo, useState } from 'react'
import {
  View, Text, TextInput, FlatList, Pressable,
  ActivityIndicator, Keyboard, StyleSheet,
} from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { UserListItem } from '../../components/UserListItem'
import { ConversationRow } from '../../components/ConversationRow'
import { useUsernameSearch, normalizeUsernameQuery, type FoundUser } from '../../hooks/useUsernameSearch'
import { useChatStore } from '../../store/chat'
import { useProfileStore } from '../../store/profile'

// Deliberately a plain, always-in-flow search field — not a hidden,
// pull-to-reveal trigger that opens a separate absolutely-positioned
// overlay screen (the previous version of this screen did that, and a
// broken position style on the overlay made it render as a normal in-flow
// block shoved to the bottom of the screen, under the tab bar, instead of
// covering it — see the bug report this replaced). A search field that's
// simply always there has no equivalent failure mode.
export default function ChatsScreen() {
  const insets = useSafeAreaInsets()
  const router = useRouter()
  const conversations = useChatStore(s => s.conversations)
  const myPeerId = useProfileStore(s => s.peerId)

  const [query, setQuery] = useState('')
  const active = query.length > 0

  const { results: rawResults, loading: searching, error } = useUsernameSearch(query)
  // Never let someone find and message their own account — a self-lookup
  // always resolves (this device published its own claim), but there is
  // no legitimate reason to open a "chat" with yourself.
  const results = useMemo(() => rawResults.filter(u => u.peerId !== myPeerId), [rawResults, myPeerId])
  const items = useMemo(() => Object.values(conversations).sort((a, b) => b.lastMessageAt - a.lastMessageAt), [conversations])
  const term = normalizeUsernameQuery(query)

  function openChat(peerId: string) {
    Keyboard.dismiss()
    router.push({ pathname: '/chat/[userId]', params: { userId: peerId } })
  }

  function openFoundUser(user: FoundUser) {
    Keyboard.dismiss()
    setQuery('')
    router.push({
      pathname: '/chat/[userId]',
      params: {
        userId: user.peerId,
        peerFingerprint: user.fingerprint,
        peerPublicKeyBase64: user.publicKeyBase64,
        peerUsername: user.username,
      },
    })
  }

  return (
    <View style={[s.root, { paddingTop: insets.top }]}>
      <View style={s.header}>
        <Text style={s.title}>Chats</Text>
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

      {active ? (
        term.length < 5 ? (
          <View style={s.hint}>
            <Ionicons name="at" size={40} color="#27272a" />
            <Text style={s.hintText}>Введите имя пользователя для поиска</Text>
            <Text style={s.hintSubtext}>
              Поиск работает только по точному имени — это открытая P2P-сеть без сервера, а не каталог всех пользователей.
            </Text>
          </View>
        ) : searching ? (
          <View style={s.hint}>
            <ActivityIndicator color="#52525b" />
          </View>
        ) : error ? (
          <View style={s.hint}>
            <Text style={s.errorText}>{error}</Text>
          </View>
        ) : results.length === 0 ? (
          <View style={s.hint}>
            <Ionicons name="search" size={40} color="#27272a" />
            <Text style={s.hintText}>Никто не публикует такое имя прямо сейчас</Text>
          </View>
        ) : (
          <FlatList
            data={results}
            keyExtractor={u => u.peerId}
            renderItem={({ item }) => <UserListItem user={item} onPress={openFoundUser} />}
            keyboardShouldPersistTaps="handled"
            keyboardDismissMode="on-drag"
            ListHeaderComponent={<Text style={s.sectionHeader}>Пользователи</Text>}
            contentContainerStyle={{ paddingBottom: insets.bottom + 20 }}
          />
        )
      ) : (
        <FlatList
          style={s.list}
          data={items}
          keyExtractor={c => c.peerId}
          renderItem={({ item }) => <ConversationRow item={item} onPress={openChat} />}
          showsVerticalScrollIndicator={false}
          contentContainerStyle={{ paddingBottom: insets.bottom + 20 }}
          ListEmptyComponent={
            <View style={s.empty}>
              <Ionicons name="chatbubbles-outline" size={56} color="#27272a" />
              <Text style={s.emptyText}>Здесь появятся ваши чаты</Text>
            </View>
          }
        />
      )}
    </View>
  )
}

const s = StyleSheet.create({
  root:   { flex: 1, backgroundColor: '#000' },
  header: { paddingHorizontal: 16, paddingVertical: 14 },
  title:  { color: '#fff', fontSize: 22, fontWeight: '700' },
  list:   { flex: 1 },

  searchRow: {
    flexDirection: 'row', alignItems: 'center', gap: 8,
    marginHorizontal: 16, marginBottom: 8, paddingHorizontal: 12, height: 44,
    backgroundColor: '#1c1c1e', borderRadius: 12,
  },
  searchInput: { flex: 1, color: '#fff', fontSize: 16, height: '100%', padding: 0 },

  empty:     { alignItems: 'center', justifyContent: 'center', gap: 12, paddingTop: 80 },
  emptyText: { color: '#52525b', fontSize: 16 },

  sectionHeader: {
    color: '#52525b', fontSize: 13, fontWeight: '600',
    paddingHorizontal: 16, paddingTop: 4, paddingBottom: 6,
    textTransform: 'uppercase',
  },

  hint:         { flex: 1, alignItems: 'center', justifyContent: 'center', gap: 12, paddingTop: 60, paddingHorizontal: 32 },
  hintText:     { color: '#52525b', fontSize: 15, textAlign: 'center' },
  hintSubtext:  { color: '#3f3f46', fontSize: 12, textAlign: 'center', lineHeight: 17 },
  errorText:    { color: '#f87171', fontSize: 15 },
})
