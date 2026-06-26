import { useCallback, useState } from 'react'
import {
  View, Text, FlatList, Pressable,
  ActivityIndicator, Keyboard, useWindowDimensions, StyleSheet,
} from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { useRouter, useFocusEffect } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { SearchBar } from '../../components/SearchBar'
import { UserListItem } from '../../components/UserListItem'
import { ConversationRow } from '../../components/ConversationRow'
import { useUserSearch, normalizeQuery, type PublicUser } from '../../hooks/useUserSearch'
import { useConversations } from '../../hooks/useConversations'
import { useAuthStore } from '../../store/auth'

const SEARCH_H = 54 // height of the collapsed search trigger, hidden above the fold

export default function ChatsScreen() {
  const insets = useSafeAreaInsets()
  const router = useRouter()
  const { height } = useWindowDimensions()
  const me = useAuthStore(s => s.user?.id) ?? ''

  const [active, setActive] = useState(false)
  const [query,  setQuery]  = useState('')

  const { results, loading: searching, error } = useUserSearch(active ? query : '')
  const { items, loading: loadingConvs, reload } = useConversations()
  const term = normalizeQuery(query)

  useFocusEffect(useCallback(() => { reload() }, [reload]))

  function closeSearch() {
    Keyboard.dismiss()
    setActive(false)
    setQuery('')
  }

  function openChat(userId: string) {
    Keyboard.dismiss()
    router.push({ pathname: '/chat/[userId]', params: { userId } })
  }

  return (
    <View style={[s.root, { paddingTop: insets.top }]}>
      <View style={s.header}>
        <Text style={s.title}>Chats</Text>
      </View>

      <FlatList
        data={items}
        keyExtractor={c => c.conversation_id}
        renderItem={({ item }) => <ConversationRow item={item} meId={me} onPress={openChat} />}
        contentOffset={{ x: 0, y: SEARCH_H }}
        showsVerticalScrollIndicator={false}
        ListHeaderComponent={
          <Pressable style={s.trigger} onPress={() => setActive(true)}>
            <Ionicons name="search" size={18} color="#52525b" />
            <Text style={s.triggerText}>Поиск</Text>
          </Pressable>
        }
        ListEmptyComponent={
          loadingConvs ? (
            <View style={[s.empty, { minHeight: height - insets.top - 160 }]}>
              <ActivityIndicator color="#52525b" />
            </View>
          ) : (
            <View style={[s.empty, { minHeight: height - insets.top - 160 }]}>
              <Ionicons name="chatbubbles-outline" size={56} color="#27272a" />
              <Text style={s.emptyText}>Здесь появятся ваши чаты</Text>
            </View>
          )
        }
      />

      {active ? (
        <View style={[s.overlay, { paddingTop: insets.top }]}>
          <SearchBar value={query} onChangeText={setQuery} onCancel={closeSearch} autoFocus />

          {term.length < 2 ? (
            <View style={s.hint}>
              <Ionicons name="at" size={40} color="#27272a" />
              <Text style={s.hintText}>Введите имя пользователя для поиска</Text>
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
              <Text style={s.hintText}>Ничего не найдено</Text>
            </View>
          ) : (
            <FlatList
              data={results}
              keyExtractor={u => u.id}
              renderItem={({ item }: { item: PublicUser }) => (
                <UserListItem user={item} onPress={(u) => openChat(u.id)} />
              )}
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

  hint:      { flex: 1, alignItems: 'center', justifyContent: 'center', gap: 12, paddingTop: 60 },
  hintText:  { color: '#52525b', fontSize: 15 },
  errorText: { color: '#f87171', fontSize: 15 },
})
