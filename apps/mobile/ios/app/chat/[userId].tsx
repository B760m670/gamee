import { useEffect, useState } from 'react'
import {
  View, Text, FlatList, Pressable, Alert,
  KeyboardAvoidingView, Platform, StyleSheet,
} from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { GlassView } from 'expo-glass-effect'
import { useRouter, useLocalSearchParams } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { PeerAvatar } from '../../components/PeerAvatar'
import { MessageBubble } from '../../components/MessageBubble'
import { ChatInputBar } from '../../components/ChatInputBar'
import { useMediaAttach } from '../../hooks/useMediaAttach'
import { useChatStore, type ChatMessage, type PeerInfo } from '../../store/chat'

const BTN_H = 44

export default function ChatScreen() {
  const insets = useSafeAreaInsets()
  const router = useRouter()
  const params = useLocalSearchParams<{
    userId: string
    peerFingerprint?: string
    peerPublicKeyBase64?: string
    peerUsername?: string
  }>()
  const peerId = params.userId

  // A search result carries the peer's info in the route params (first
  // time this device has ever seen them); reopening an existing
  // conversation from the Chats list doesn't, so fall back to what's
  // already known locally — either is enough for `openConversation`.
  const known = useChatStore(s => s.conversations[peerId] ?? s.activePeers[peerId])
  const openConversation = useChatStore(s => s.openConversation)
  const sendMessage = useChatStore(s => s.sendMessage)
  const sendVoice = useChatStore(s => s.sendVoice)
  const sendMedia = useChatStore(s => s.sendMedia)
  const deleteMessage = useChatStore(s => s.deleteMessage)
  const messages = useChatStore(s => s.messages[peerId] ?? [])

  const [ready, setReady] = useState(false)

  useEffect(() => {
    const peer: PeerInfo = known ?? {
      peerId,
      peerFingerprint: params.peerFingerprint ?? '',
      peerPublicKeyBase64: params.peerPublicKeyBase64 ?? '',
      peerUsername: params.peerUsername ?? null,
    }
    openConversation(peer).finally(() => setReady(true))
  }, [peerId])

  const title = known?.peerUsername ? `@${known.peerUsername}` : (known?.peerFingerprint || params.peerFingerprint || 'Чат')

  function openProfile() {
    router.push({ pathname: '/profile/[id]', params: { id: peerId } })
  }

  function handleSend(text: string) {
    sendMessage(peerId, text)
  }

  function handleSendVoice(fileUri: string, durationMs: number) {
    sendVoice(peerId, fileUri, durationMs)
  }

  const handleAttach = useMediaAttach(m =>
    sendMedia(peerId, m.fileUri, m.mime, m.filename, m.durationMs),
  )

  function confirmDeleteMessage(msg: ChatMessage) {
    Alert.alert('Удалить сообщение?', 'Оно удалится только на этом устройстве.', [
      { text: 'Отмена', style: 'cancel' },
      { text: 'Удалить', style: 'destructive', onPress: () => deleteMessage(peerId, msg.localId) },
    ])
  }

  return (
    <KeyboardAvoidingView
      style={s.root}
      behavior={Platform.OS === 'ios' ? 'padding' : undefined}
      keyboardVerticalOffset={0}
    >
      {/* Header */}
      <View style={[s.header, { paddingTop: insets.top + 6 }]}>
        <Pressable onPress={() => router.back()} style={s.backOverlay}>
          <GlassView style={s.backBtn} glassEffectStyle="regular" isInteractive colorScheme="dark">
            <Ionicons name="chevron-back" size={22} color="#fff" />
          </GlassView>
        </Pressable>
        <Pressable style={s.headerCenter} onPress={openProfile}>
          <Text style={s.headerTitle} numberOfLines={1}>{title}</Text>
        </Pressable>
        <View style={s.headerRight}>
          <Pressable onPress={openProfile}>
            <PeerAvatar peerId={peerId} size={32} username={known?.peerUsername ?? undefined} />
          </Pressable>
        </View>
      </View>

      <FlatList
        data={[...messages].reverse()}
        inverted
        keyExtractor={m => m.localId}
        renderItem={({ item }: { item: ChatMessage }) => (
          <MessageBubble msg={item} onLongPress={confirmDeleteMessage} />
        )}
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
  root:   { flex: 1, backgroundColor: '#000' },

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
  headerCenter: { flex: 1, alignItems: 'center' },
  headerTitle:  { color: '#fff', fontSize: 17, fontWeight: '600' },
  headerRight:  { width: BTN_H, alignItems: 'flex-end' },

  emptyChat: { transform: [{ scaleY: -1 }], alignItems: 'center', paddingTop: 60 },
  emptyText: { color: '#52525b', fontSize: 15 },
})
