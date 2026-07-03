import {
  View, Text, Pressable, ScrollView, Alert, StyleSheet,
} from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { GlassView } from 'expo-glass-effect'
import { useRouter, useLocalSearchParams } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { Avatar } from '../../components/Avatar'
import { useChatStore } from '../../store/chat'

const AVATAR_SIZE = 100
const BTN_H = 44

interface ActionButton {
  key: string
  icon: keyof typeof Ionicons.glyphMap
  label: string
}

const ACTIONS: ActionButton[] = [
  { key: 'message', icon: 'chatbubble',  label: 'Написать' },
  { key: 'call',     icon: 'call',                label: 'Звонок' },
  { key: 'video',    icon: 'videocam',            label: 'Видео' },
  { key: 'more',     icon: 'ellipsis-horizontal', label: 'Ещё' },
]

// There is no remote "fetch a stranger's profile" endpoint, and there
// never will be by design — everything shown here is whatever this device
// already knows locally about `id` (a PeerId), from a ledger username
// lookup or from having exchanged messages with them.
export default function ProfileScreen() {
  const insets = useSafeAreaInsets()
  const router = useRouter()
  const { id } = useLocalSearchParams<{ id: string }>()
  const peer = useChatStore(s => s.conversations[id] ?? s.activePeers[id])

  const title = peer?.peerUsername ? `@${peer.peerUsername}` : (peer?.peerFingerprint || 'Профиль')

  function handleAction(key: string) {
    if (key === 'message') {
      router.push({ pathname: '/chat/[userId]', params: { userId: id } })
      return
    }
    Alert.alert('Скоро', 'Эта функция появится позже.')
  }

  return (
    <View style={s.root}>
      <View style={[s.backOverlay, { top: insets.top + 10, left: 16 }]}>
        <Pressable onPress={() => router.back()}>
          <GlassView style={s.backBtn} glassEffectStyle="regular" isInteractive colorScheme="dark">
            <Ionicons name="chevron-back" size={22} color="#fff" />
          </GlassView>
        </Pressable>
      </View>

      <ScrollView
        contentContainerStyle={{ paddingTop: insets.top + 64, paddingBottom: insets.bottom + 40 }}
        showsVerticalScrollIndicator={false}
      >
        <View style={s.headerSection}>
          <Avatar uri={null} size={AVATAR_SIZE} username={peer?.peerUsername ?? undefined} />
          <Text style={s.name} numberOfLines={1}>{title}</Text>
          {peer?.peerFingerprint ? <Text style={s.status}>{peer.peerFingerprint}</Text> : null}
        </View>

        <View style={s.actions}>
          {ACTIONS.map(a => (
            <Pressable key={a.key} style={s.action} onPress={() => handleAction(a.key)}>
              <View style={s.actionIcon}>
                <Ionicons name={a.icon} size={22} color="#2f7bff" />
              </View>
              <Text style={s.actionLabel}>{a.label}</Text>
            </Pressable>
          ))}
        </View>

        {peer?.peerUsername ? (
          <View style={s.group}>
            <View style={s.infoRow}>
              <Text style={s.infoLabel}>Имя пользователя</Text>
              <Text style={s.infoValue}>@{peer.peerUsername}</Text>
            </View>
          </View>
        ) : null}
      </ScrollView>
    </View>
  )
}

const s = StyleSheet.create({
  root:   { flex: 1, backgroundColor: '#000' },
  center: { flex: 1, alignItems: 'center', justifyContent: 'center' },

  backOverlay: { position: 'absolute', zIndex: 10 },
  backBtn: {
    width: BTN_H, height: BTN_H, borderRadius: BTN_H / 2,
    alignItems: 'center', justifyContent: 'center',
  },

  headerSection: { alignItems: 'center', gap: 8, paddingBottom: 20 },
  name:   { color: '#fff', fontSize: 28, fontWeight: '700', marginTop: 14, paddingHorizontal: 24 },
  status: { color: '#52525b', fontSize: 16, fontVariant: ['tabular-nums'] },

  actions: {
    flexDirection: 'row', justifyContent: 'center', gap: 24,
    paddingVertical: 8, marginBottom: 24,
  },
  action:     { alignItems: 'center', gap: 6, width: 64 },
  actionIcon: {
    width: 52, height: 52, borderRadius: 26,
    backgroundColor: '#1c1c1e', alignItems: 'center', justifyContent: 'center',
  },
  actionLabel: { color: '#a1a1aa', fontSize: 12 },

  group:     { marginHorizontal: 16, backgroundColor: '#111114', borderRadius: 16, overflow: 'hidden' },
  infoRow:   { paddingHorizontal: 16, paddingVertical: 12, gap: 4 },
  infoLabel: { color: '#a1a1aa', fontSize: 12, fontWeight: '500' },
  infoValue: { color: '#fff', fontSize: 16, lineHeight: 22 },

  errorText: { color: '#f87171', fontSize: 15 },
})
