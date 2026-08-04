import {
  View, Text, Pressable, ScrollView, Alert, StyleSheet,
} from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { GlassView } from 'expo-glass-effect'
import { useRouter, useLocalSearchParams } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { PeerAvatar } from '../../components/PeerAvatar'
import { useChatStore } from '../../store/chat'
import { useContactsStore } from '../../store/contacts'
import { useConsentStore } from '../../store/consent'

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
  const knownPeer = useChatStore(s => s.conversations[id] ?? s.activePeers[id])
  const savedContact = useContactsStore(s => s.contacts[id])
  const addContact = useContactsStore(s => s.addContact)
  const removeContact = useContactsStore(s => s.removeContact)
  const stance = useConsentStore(s => s.stances[id] ?? 'none')
  const applyConsent = useConsentStore(s => s.apply)
  // A profile can be reached from a conversation (chat store knows the
  // peer) or straight from the Contacts tab (only the contacts store
  // does) — either source has the full PeerInfo.
  const peer = knownPeer ?? savedContact

  const title = peer?.peerUsername ? `@${peer.peerUsername}` : (peer?.peerFingerprint || 'Профиль')

  function handleAction(key: string) {
    if (key === 'message') {
      router.push({ pathname: '/chat/[userId]', params: { userId: id } })
      return
    }
    Alert.alert('Скоро', 'Эта функция появится позже.')
  }

  function failed() {
    Alert.alert('Не удалось', 'Сессия ещё не готова. Попробуйте через мгновение.')
  }

  function toggleRestrict() {
    if (!applyConsent(id, stance === 'restricted' ? 'none' : 'restricted')) failed()
  }

  function confirmBlockToggle() {
    if (stance === 'blocked') {
      if (!applyConsent(id, 'none')) failed()
      return
    }
    // Destructive and not fully reversible in its effects — anything queued
    // for this peer is discarded, not held — so it asks first, and says what
    // actually happens rather than a generic "are you sure?".
    Alert.alert(
      'Заблокировать?',
      'Его сообщения будут отклоняться, не расшифровываясь, а неотправленные вами — удалены. Он не узнает о блокировке.',
      [
        { text: 'Отмена', style: 'cancel' },
        {
          text: 'Заблокировать',
          style: 'destructive',
          onPress: () => {
            if (!applyConsent(id, 'blocked')) failed()
          },
        },
      ],
    )
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
          <PeerAvatar peerId={id} size={AVATAR_SIZE} username={peer?.peerUsername ?? undefined} />
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

        {peer ? (
          <View style={[s.group, { marginTop: 16 }]}>
            <Pressable
              style={({ pressed }) => [s.contactBtn, pressed && { opacity: 0.7 }]}
              onPress={() => (savedContact ? removeContact(id) : addContact(peer))}
            >
              <Ionicons
                name={savedContact ? 'person-remove-outline' : 'person-add-outline'}
                size={20}
                color={savedContact ? '#f87171' : '#2f7bff'}
              />
              <Text style={[s.contactBtnText, savedContact && { color: '#f87171' }]}>
                {savedContact ? 'Убрать из контактов' : 'В контакты'}
              </Text>
            </Pressable>
          </View>
        ) : null}

        <View style={[s.group, { marginTop: 16 }]}>
          {stance !== 'blocked' ? (
            <Pressable
              style={({ pressed }) => [s.contactBtn, pressed && { opacity: 0.7 }]}
              onPress={toggleRestrict}
            >
              <Ionicons
                name={stance === 'restricted' ? 'volume-medium-outline' : 'volume-mute-outline'}
                size={20}
                color={stance === 'restricted' ? '#2f7bff' : '#a1a1aa'}
              />
              <Text style={[s.contactBtnText, { color: stance === 'restricted' ? '#2f7bff' : '#a1a1aa' }]}>
                {stance === 'restricted' ? 'Снять ограничение' : 'Ограничить'}
              </Text>
            </Pressable>
          ) : null}

          <Pressable
            style={({ pressed }) => [s.contactBtn, pressed && { opacity: 0.7 }]}
            onPress={confirmBlockToggle}
          >
            <Ionicons
              name={stance === 'blocked' ? 'lock-open-outline' : 'ban-outline'}
              size={20}
              color={stance === 'blocked' ? '#2f7bff' : '#f87171'}
            />
            <Text style={[s.contactBtnText, { color: stance === 'blocked' ? '#2f7bff' : '#f87171' }]}>
              {stance === 'blocked' ? 'Разблокировать' : 'Заблокировать'}
            </Text>
          </Pressable>
        </View>

        {stance !== 'none' ? (
          <Text style={s.stanceNote}>
            {stance === 'blocked'
              ? 'Сообщения от этого человека отклоняются, не расшифровываясь. Он об этом не узнает.'
              : 'Сообщения приходят, но без уведомлений, и чат не поднимается в списке.'}
          </Text>
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

  contactBtn: {
    flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 8,
    paddingVertical: 14,
  },
  contactBtnText: { color: '#2f7bff', fontSize: 16, fontWeight: '600' },
  stanceNote: {
    color: '#71717a', fontSize: 13, lineHeight: 18,
    paddingHorizontal: 32, paddingTop: 10,
  },
})
