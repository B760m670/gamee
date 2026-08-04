import { useEffect } from 'react'
import { View, Text, FlatList, Pressable, Alert, StyleSheet } from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { GlassView } from 'expo-glass-effect'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { PeerAvatar } from '../../components/PeerAvatar'
import { useChatStore } from '../../store/chat'
import { useContactsStore } from '../../store/contacts'
import { useConsentStore, type ConsentStance } from '../../store/consent'

/**
 * Everyone this account has blocked or restricted, and the only place to
 * lift either without first finding the person again — which matters,
 * because a blocked peer no longer appears in chats or search, so their
 * profile can become unreachable by every other route.
 */
export default function BlockedScreen() {
  const insets = useSafeAreaInsets()
  const router = useRouter()

  const stances = useConsentStore(s => s.stances)
  const loaded = useConsentStore(s => s.loaded)
  const load = useConsentStore(s => s.load)
  const apply = useConsentStore(s => s.apply)

  // The native store is the source of truth, so this screen re-reads on
  // mount rather than trusting whatever the mirror happens to hold.
  useEffect(() => { load() }, [])

  const conversations = useChatStore(s => s.conversations)
  const activePeers = useChatStore(s => s.activePeers)
  const contacts = useContactsStore(s => s.contacts)

  const entries = Object.entries(stances)
    .filter((e): e is [string, Exclude<ConsentStance, 'none'>] => !!e[1])
    .map(([peerId, stance]) => {
      const peer = conversations[peerId] ?? activePeers[peerId] ?? contacts[peerId]
      return {
        peerId,
        stance,
        username: peer?.peerUsername ?? null,
        // Falls back to the peer id: someone blocked before any profile was
        // ever fetched has no name here, and showing a truncated id is more
        // honest than an empty row the user can't act on.
        subtitle: peer?.peerFingerprint || peerId,
      }
    })

  function lift(peerId: string, stance: Exclude<ConsentStance, 'none'>) {
    const title = stance === 'blocked' ? 'Разблокировать?' : 'Снять ограничение?'
    const body = stance === 'blocked'
      ? 'Его сообщения снова будут приходить. Ранее удалённые не восстановятся.'
      : 'Уведомления от этого чата снова включатся.'
    Alert.alert(title, body, [
      { text: 'Отмена', style: 'cancel' },
      {
        text: stance === 'blocked' ? 'Разблокировать' : 'Снять',
        onPress: () => {
          if (!apply(peerId, 'none')) {
            Alert.alert('Не удалось', 'Сессия ещё не готова. Попробуйте через мгновение.')
          }
        },
      },
    ])
  }

  return (
    <View style={s.root}>
      <View style={[s.nav, { paddingTop: insets.top + 8 }]}>
        <Pressable onPress={() => router.back()} style={s.backWrap}>
          <GlassView style={s.backBtn} glassEffectStyle="regular" isInteractive colorScheme="dark">
            <Ionicons name="chevron-back" size={22} color="#fff" />
          </GlassView>
        </Pressable>
        <Text style={s.navTitle}>Заблокированные</Text>
        <View style={s.navSpacer} />
      </View>

      <FlatList
        data={entries}
        keyExtractor={e => e.peerId}
        contentContainerStyle={{ paddingHorizontal: 16, paddingBottom: insets.bottom + 24 }}
        ListHeaderComponent={
          entries.length > 0 ? (
            <Text style={s.hint}>
              Сообщения заблокированных отклоняются, не расшифровываясь, — они об этом не узнают.
              Ограниченные приходят, но молча.
            </Text>
          ) : null
        }
        ListEmptyComponent={
          loaded ? (
            <View style={s.empty}>
              <Ionicons name="shield-checkmark-outline" size={40} color="#3f3f46" />
              <Text style={s.emptyText}>Никто не заблокирован</Text>
            </View>
          ) : null
        }
        renderItem={({ item }) => (
          <View style={s.row}>
            <PeerAvatar peerId={item.peerId} size={40} username={item.username ?? undefined} />
            <View style={s.rowText}>
              <Text style={s.rowTitle} numberOfLines={1}>
                {item.username ? `@${item.username}` : item.subtitle}
              </Text>
              <Text style={s.rowSub} numberOfLines={1}>
                {item.stance === 'blocked' ? 'Заблокирован' : 'Ограничен'}
              </Text>
            </View>
            <Pressable
              onPress={() => lift(item.peerId, item.stance)}
              hitSlop={8}
              style={({ pressed }) => [s.liftBtn, pressed && { opacity: 0.7 }]}
            >
              <Text style={s.liftText}>{item.stance === 'blocked' ? 'Разблокировать' : 'Снять'}</Text>
            </Pressable>
          </View>
        )}
      />
    </View>
  )
}

const s = StyleSheet.create({
  root: { flex: 1, backgroundColor: '#000' },

  nav: {
    flexDirection: 'row', alignItems: 'center',
    paddingHorizontal: 12, paddingBottom: 10,
  },
  backWrap: {},
  backBtn: { width: 40, height: 40, borderRadius: 20, alignItems: 'center', justifyContent: 'center' },
  navTitle: { flex: 1, textAlign: 'center', color: '#fff', fontSize: 17, fontWeight: '600' },
  navSpacer: { width: 40 },

  hint: { color: '#71717a', fontSize: 13, lineHeight: 18, paddingVertical: 12 },

  row: {
    flexDirection: 'row', alignItems: 'center', gap: 12,
    paddingVertical: 10,
    borderBottomWidth: StyleSheet.hairlineWidth, borderBottomColor: '#1c1c1e',
  },
  rowText: { flex: 1 },
  rowTitle: { color: '#fff', fontSize: 16 },
  rowSub: { color: '#71717a', fontSize: 13, marginTop: 2 },
  liftBtn: { paddingHorizontal: 12, paddingVertical: 6, borderRadius: 14, backgroundColor: '#1c1c1e' },
  liftText: { color: '#2f7bff', fontSize: 14, fontWeight: '600' },

  empty: { alignItems: 'center', paddingTop: 80, gap: 12 },
  emptyText: { color: '#52525b', fontSize: 15 },
})
