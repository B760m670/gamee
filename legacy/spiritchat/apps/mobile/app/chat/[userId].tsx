import {
  View, Text, FlatList, Pressable, ActivityIndicator,
  KeyboardAvoidingView, Platform, StyleSheet,
} from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { useRouter, useLocalSearchParams } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { Avatar } from '../../components/Avatar'
import { MessageBubble } from '../../components/MessageBubble'
import { ChatInputBar } from '../../components/ChatInputBar'
import { useChat } from '../../hooks/useChat'

export default function ChatScreen() {
  const insets = useSafeAreaInsets()
  const router = useRouter()
  const { userId } = useLocalSearchParams<{ userId: string }>()

  const { other, messages, loading, error, otherLastReadAt, me, send, loadOlder } = useChat(userId)

  const title = other?.display_name?.trim() || (other?.username ? `@${other.username}` : 'Чат')

  function openProfile() {
    if (!other) return
    router.push({
      pathname: '/profile/[id]',
      params: { id: other.id, preload: JSON.stringify(other) },
    })
  }

  return (
    <KeyboardAvoidingView
      style={s.root}
      behavior={Platform.OS === 'ios' ? 'padding' : undefined}
      keyboardVerticalOffset={0}
    >
      {/* Header */}
      <View style={[s.header, { paddingTop: insets.top + 6 }]}>
        <Pressable onPress={() => router.back()} hitSlop={8} style={s.back}>
          <Ionicons name="chevron-back" size={28} color="#2f7bff" />
        </Pressable>
        <Pressable style={s.headerCenter} onPress={openProfile}>
          <Avatar uri={other?.avatar_url} size={36} username={other?.username ?? other?.display_name} />
          <Text style={s.headerTitle} numberOfLines={1}>{title}</Text>
        </Pressable>
      </View>

      {loading ? (
        <View style={s.center}><ActivityIndicator color="#52525b" /></View>
      ) : error ? (
        <View style={s.center}><Text style={s.errorText}>{error}</Text></View>
      ) : (
        <FlatList
          data={messages}
          inverted
          keyExtractor={m => m.client_id ?? m.id}
          renderItem={({ item }) => {
            const isMine = item.sender_id === me
            const readByOther = isMine && !!otherLastReadAt &&
              otherLastReadAt.localeCompare(item.created_at) >= 0
            return <MessageBubble msg={item} isMine={isMine} readByOther={readByOther} />
          }}
          onEndReached={loadOlder}
          onEndReachedThreshold={0.4}
          keyboardShouldPersistTaps="handled"
          keyboardDismissMode="interactive"
          contentContainerStyle={{ paddingVertical: 10 }}
          ListEmptyComponent={
            <View style={s.emptyChat}>
              <Text style={s.emptyText}>Нет сообщений. Напишите первым!</Text>
            </View>
          }
        />
      )}

      <View style={{ paddingBottom: insets.bottom + 6 }}>
        <ChatInputBar onSend={send} />
      </View>
    </KeyboardAvoidingView>
  )
}

const s = StyleSheet.create({
  root:   { flex: 1, backgroundColor: '#000' },
  center: { flex: 1, alignItems: 'center', justifyContent: 'center' },

  header: {
    flexDirection: 'row', alignItems: 'center', gap: 4,
    paddingHorizontal: 8, paddingBottom: 8,
    borderBottomWidth: StyleSheet.hairlineWidth, borderBottomColor: '#1c1c1e',
  },
  back:         { padding: 2 },
  headerCenter: { flex: 1, flexDirection: 'row', alignItems: 'center', gap: 10 },
  headerTitle:  { color: '#fff', fontSize: 17, fontWeight: '600', flexShrink: 1 },

  emptyChat: { transform: [{ scaleY: -1 }], alignItems: 'center', paddingTop: 60 },
  emptyText: { color: '#52525b', fontSize: 15 },
  errorText: { color: '#f87171', fontSize: 15 },
})
