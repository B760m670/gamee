import { useState } from 'react'
import {
  View, Text, FlatList, Pressable,
  ActivityIndicator, Keyboard, useWindowDimensions, StyleSheet,
} from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { SearchBar } from '../../components/SearchBar'
import { UserListItem } from '../../components/UserListItem'
import { ConversationRow } from '../../components/ConversationRow'
import { useUsernameSearch, normalizeUsernameQuery, type FoundUser } from '../../hooks/useUsernameSearch'
import { useChatStore } from '../../store/chat'

const SEARCH_H = 54 // height of the collapsed search trigger, hidden above the fold

export default function ChatsScreen() {
  const insets = useSafeAreaInsets()
  const router = useRouter()
  const { height } = useWindowDimensions()
  const conversations = useChatStore(s => s.conversations)

  const [active, setActive] = useState(false)
  const [query,  setQuery]  = useState('')

  const { results, loading: searching, error } = useUsernameSearch(active ? query : '')
  const items = Object.values(conversations).sort((a, b) => b.lastMessageAt - a.lastMessageAt)
  const term = normalizeUsernameQuery(query)

  function closeSearch() {
    Keyboard.dismiss()
    setActive(false)
    setQuery('')
  }

  function openChat(peerId: string) {
    Keyboard.dismiss()
    router.push({ pathname: '/chat/[userId]', params: { userId: peerId } })
  }

  function openFoundUser(user: FoundUser) {
    Keyboard.dismiss()
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

      <FlatList
        data={items}
        keyExtractor={c => c.peerId}
        renderItem={({ item }) => <ConversationRow item={item} onPress={openChat} />}
        contentOffset={{ x: 0, y: SEARCH_H }}
        showsVerticalScrollIndicator={false}
        ListHeaderComponent={
          <Pressable style={s.trigger} onPress={() => setActive(true)}>
            <Ionicons name="at" size={18} color="#52525b" />
            <Text style={s.triggerText}>Найти по имени пользователя</Text>
          </Pressable>
        }
        ListEmptyComponent={
          <View style={[s.empty, { minHeight: height - insets.top - 160 }]}>
            <Ionicons name="chatbubbles-outline" size={56} color="#27272a" />
            <Text style={s.emptyText}>Здесь появятся ваши чаты</Text>
          </View>
        }
      />

      {active ? (
        <View style={[s.overlay, { paddingTop: insets.top }]}>
          <SearchBar value={query} onChangeText={setQuery} onCancel={closeSearch} autoFocus />

          {term.length < 5 ? (
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
          )}
        </View>
      ) : null}
    </View>
  )
}

const s = StyleSheet.create({
  root:   { flex: 1, backgroundColor: '#000' },
  header: { paddingHorizontal: 16, paddingVertical: 14 },
  title:  { color: '#fff', fontSize: 22, fontWeight: '700' },

  trigger: {
    height: SEARCH_H, flexDirection: 'row', alignItems: 'center', gap: 8,
    marginHorizontal: 16, marginBottom: 4, paddingHorizontal: 12,
    backgroundColor: '#1c1c1e', borderRadius: 12,
  },
  triggerText: { color: '#52525b', fontSize: 16 },

  empty:     { alignItems: 'center', justifyContent: 'center', gap: 12 },
  emptyText: { color: '#52525b', fontSize: 16 },

  overlay: { ...StyleSheet.absoluteFillObject, backgroundColor: '#000' },

  sectionHeader: {
    color: '#52525b', fontSize: 13, fontWeight: '600',
    paddingHorizontal: 16, paddingTop: 12, paddingBottom: 6,
    textTransform: 'uppercase',
  },

  hint:         { flex: 1, alignItems: 'center', justifyContent: 'center', gap: 12, paddingTop: 60, paddingHorizontal: 32 },
  hintText:     { color: '#52525b', fontSize: 15, textAlign: 'center' },
  hintSubtext:  { color: '#3f3f46', fontSize: 12, textAlign: 'center', lineHeight: 17 },
  errorText:    { color: '#f87171', fontSize: 15 },
})
