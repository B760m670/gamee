import { useEffect, useState } from 'react'
import { View, Text, FlatList, Pressable, KeyboardAvoidingView, Platform, StyleSheet } from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { GlassView } from 'expo-glass-effect'
import { useRouter, useLocalSearchParams } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { MessageBubble } from '../../components/MessageBubble'
import { ChatInputBar } from '../../components/ChatInputBar'
import { useMediaAttach } from '../../hooks/useMediaAttach'
import { useGroupStore, type GroupMessage } from '../../store/groups'

const BTN_H = 44

export default function GroupChatScreen() {
  const insets = useSafeAreaInsets()
  const router = useRouter()
  const { groupId } = useLocalSearchParams<{ groupId: string }>()

  const group = useGroupStore(s => s.groups[groupId])
  const openGroup = useGroupStore(s => s.openGroup)
  const sendMessage = useGroupStore(s => s.sendMessage)
  const sendVoice = useGroupStore(s => s.sendVoice)
  const sendMedia = useGroupStore(s => s.sendMedia)
  const messages = useGroupStore(s => s.messages[groupId] ?? [])

  const [ready, setReady] = useState(false)

  useEffect(() => {
    openGroup(groupId).finally(() => setReady(true))
  }, [groupId])

  function handleSend(text: string) {
    sendMessage(groupId, text)
  }

  function handleSendVoice(fileUri: string, durationMs: number) {
    sendVoice(groupId, fileUri, durationMs)
  }

  const handleAttach = useMediaAttach(m =>
    sendMedia(groupId, m.fileUri, m.mime, m.filename, m.durationMs),
  )

  return (
    <KeyboardAvoidingView style={s.root} behavior={Platform.OS === 'ios' ? 'padding' : undefined} keyboardVerticalOffset={0}>
      <View style={[s.header, { paddingTop: insets.top + 6 }]}>
        <Pressable onPress={() => router.back()} style={s.backOverlay}>
          <GlassView style={s.backBtn} glassEffectStyle="regular" isInteractive colorScheme="dark">
            <Ionicons name="chevron-back" size={22} color="#fff" />
          </GlassView>
        </Pressable>
        <Pressable style={s.headerCenter} onPress={() => router.push({ pathname: '/group/[groupId]/members', params: { groupId } })}>
          <Text style={s.headerTitle} numberOfLines={1}>{group?.name ?? 'Группа'}</Text>
          {group ? (
            <Text style={s.headerSubtitle} numberOfLines={1}>
              {group.members.length + 1} участник{group.members.length === 0 ? '' : 'ов'}
            </Text>
          ) : null}
        </Pressable>
        <View style={s.headerRight} />
      </View>

      <FlatList
        data={[...messages].reverse()}
        inverted
        keyExtractor={(m: GroupMessage) => m.localId}
        renderItem={({ item }: { item: GroupMessage }) => <MessageBubble msg={item} />}
        keyboardShouldPersistTaps="handled"
        keyboardDismissMode="interactive"
        contentContainerStyle={{ paddingVertical: 10 }}
        ListEmptyComponent={
          ready ? (
            <View style={s.emptyChat}>
              <Text style={s.emptyText}>Нет сообщений. Напишите первым!</Text>
            </View>
          ) : null
        }
      />

      <View style={{ paddingBottom: insets.bottom + 6 }}>
        <ChatInputBar onSend={handleSend} onSendVoice={handleSendVoice} onAttach={handleAttach} />
      </View>
    </KeyboardAvoidingView>
  )
}

const s = StyleSheet.create({
  root: { flex: 1, backgroundColor: '#000' },

  header: {
    flexDirection: 'row', alignItems: 'center', gap: 4,
    paddingHorizontal: 12, paddingBottom: 8,
    borderBottomWidth: StyleSheet.hairlineWidth, borderBottomColor: '#1c1c1e',
  },
  backOverlay: {},
  backBtn: {
    width: BTN_H, height: BTN_H, borderRadius: BTN_H / 2,
    alignItems: 'center', justifyContent: 'center',
  },
  headerCenter:   { flex: 1, alignItems: 'center' },
  headerTitle:    { color: '#fff', fontSize: 17, fontWeight: '600' },
  headerSubtitle: { color: '#71717a', fontSize: 12, marginTop: 1 },
  headerRight:    { width: BTN_H },

  emptyChat: { transform: [{ scaleY: -1 }], alignItems: 'center', paddingTop: 60 },
  emptyText: { color: '#52525b', fontSize: 15 },
})
