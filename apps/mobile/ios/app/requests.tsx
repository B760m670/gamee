import { useMemo } from 'react'
import { View, Text, FlatList, Pressable, Alert, StyleSheet } from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { GlassView } from 'expo-glass-effect'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { PeerAvatar } from '../components/PeerAvatar'
import { useChatStore } from '../store/chat'
import { useContactsStore } from '../store/contacts'
import { useConsentStore } from '../store/consent'

/**
 * Conversations strangers started that this device has not consented to yet
 * — phase 1 of `docs/consent-and-moderation.md`.
 *
 * The messages are already here and already decrypted; what is withheld is
 * *attention*, not delivery. Nothing on this screen is visible to the sender:
 * they cannot tell a request that is sitting unread from one that was
 * accepted, which is deliberate, since a "your request was ignored" signal
 * would be a message a stranger could extract from you.
 */
export default function RequestsScreen() {
  const insets = useSafeAreaInsets()
  const router = useRouter()

  const conversations = useChatStore(s => s.conversations)
  const acceptRequest = useChatStore(s => s.acceptRequest)
  const deleteChat = useChatStore(s => s.deleteChat)
  const contacts = useContactsStore(s => s.contacts)
  const applyConsent = useConsentStore(s => s.apply)

  const requests = useMemo(
    () => Object.values(conversations)
      .filter(c => c.accepted !== true && !contacts[c.peerId])
      .sort((a, b) => b.lastMessageAt - a.lastMessageAt),
    [conversations, contacts],
  )

  function accept(peerId: string) {
    acceptRequest(peerId)
    router.push({ pathname: '/chat/[userId]', params: { userId: peerId } })
  }

  function decline(peerId: string, name: string) {
    // Two genuinely different outcomes, so the sheet offers both rather than
    // collapsing them: deleting is "not now", blocking is "never", and only
    // the second stops them writing again.
    Alert.alert(
      name,
      'Удалить запрос или заблокировать отправителя? Он не узнает ни о том, ни о другом.',
      [
        { text: 'Отмена', style: 'cancel' },
        { text: 'Удалить запрос', onPress: () => deleteChat(peerId) },
        {
          text: 'Заблокировать',
          style: 'destructive',
          onPress: () => {
            applyConsent(peerId, 'blocked')
            deleteChat(peerId)
          },
        },
      ],
    )
  }

  return (
    <View style={s.root}>
      <View style={[s.nav, { paddingTop: insets.top + 8 }]}>
        <Pressable onPress={() => router.back()} style={s.backWrap}>
          <GlassView style={s.backBtn} glassEffectStyle="regular" isInteractive colorScheme="dark">
            <Ionicons name="chevron-back" size={22} color="#fff" />
          </GlassView>
        </Pressable>
        <Text style={s.navTitle}>Запросы</Text>
        <View style={s.navSpacer} />
      </View>

      <FlatList
        data={requests}
        keyExtractor={c => c.peerId}
        contentContainerStyle={{ paddingHorizontal: 16, paddingBottom: insets.bottom + 24 }}
        ListHeaderComponent={
          requests.length > 0 ? (
            <Text style={s.hint}>
              Люди не из ваших контактов. Сообщения уже пришли — они просто не попадают в чаты,
              пока вы не согласитесь.
            </Text>
          ) : null
        }
        ListEmptyComponent={
          <View style={s.empty}>
            <Ionicons name="mail-outline" size={40} color="#3f3f46" />
            <Text style={s.emptyText}>Нет запросов</Text>
          </View>
        }
        renderItem={({ item }) => {
          const name = item.peerUsername ? `@${item.peerUsername}` : item.peerFingerprint
          return (
            <View style={s.row}>
              <Pressable style={s.rowMain} onPress={() => accept(item.peerId)}>
                <PeerAvatar peerId={item.peerId} size={44} username={item.peerUsername ?? undefined} />
                <View style={s.rowText}>
                  <Text style={s.rowTitle} numberOfLines={1}>{name}</Text>
                  <Text style={s.rowSub} numberOfLines={1}>{item.lastMessageText}</Text>
                </View>
              </Pressable>
              <View style={s.rowActions}>
                <Pressable
                  onPress={() => accept(item.peerId)}
                  hitSlop={6}
                  style={({ pressed }) => [s.acceptBtn, pressed && { opacity: 0.7 }]}
                >
                  <Text style={s.acceptText}>Принять</Text>
                </Pressable>
                <Pressable onPress={() => decline(item.peerId, name)} hitSlop={6}>
                  <Ionicons name="ellipsis-horizontal" size={20} color="#71717a" />
                </Pressable>
              </View>
            </View>
          )
        }}
      />
    </View>
  )
}

const s = StyleSheet.create({
  root: { flex: 1, backgroundColor: '#000' },

  nav: { flexDirection: 'row', alignItems: 'center', paddingHorizontal: 12, paddingBottom: 10 },
  backWrap: {},
  backBtn: { width: 40, height: 40, borderRadius: 20, alignItems: 'center', justifyContent: 'center' },
  navTitle: { flex: 1, textAlign: 'center', color: '#fff', fontSize: 17, fontWeight: '600' },
  navSpacer: { width: 40 },

  hint: { color: '#71717a', fontSize: 13, lineHeight: 18, paddingVertical: 12 },

  row: {
    flexDirection: 'row', alignItems: 'center', gap: 10,
    paddingVertical: 10,
    borderBottomWidth: StyleSheet.hairlineWidth, borderBottomColor: '#1c1c1e',
  },
  rowMain: { flex: 1, flexDirection: 'row', alignItems: 'center', gap: 12 },
  rowText: { flex: 1 },
  rowTitle: { color: '#fff', fontSize: 16 },
  rowSub: { color: '#71717a', fontSize: 13, marginTop: 2 },
  rowActions: { flexDirection: 'row', alignItems: 'center', gap: 12 },
  acceptBtn: { paddingHorizontal: 12, paddingVertical: 6, borderRadius: 14, backgroundColor: '#2f7bff' },
  acceptText: { color: '#fff', fontSize: 14, fontWeight: '600' },

  empty: { alignItems: 'center', paddingTop: 80, gap: 12 },
  emptyText: { color: '#52525b', fontSize: 15 },
})
