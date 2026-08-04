import { View, Text, Pressable, Alert, StyleSheet } from 'react-native'
import { FlashList } from '@shopify/flash-list'
import { Ionicons } from '@expo/vector-icons'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { PeerAvatar } from '../../components/PeerAvatar'
import { useContactsStore, type Contact } from '../../store/contacts'

// Saved contacts only — people explicitly added from their profile, not
// every past search or conversation. Everything here is local to this
// account (there is no server-side address book, and nobody learns they
// were added); tapping a contact opens the chat directly, carrying the
// saved key material the same way a search result does.
export default function ContactsScreen() {
  const insets = useSafeAreaInsets()
  const router = useRouter()
  const contacts = useContactsStore(s => s.contacts)
  const removeContact = useContactsStore(s => s.removeContact)

  const list = Object.values(contacts).sort((a, b) => {
    const nameA = a.peerUsername ?? a.peerFingerprint
    const nameB = b.peerUsername ?? b.peerFingerprint
    return nameA.localeCompare(nameB)
  })

  function openContact(contact: Contact) {
    router.push({
      pathname: '/chat/[userId]',
      params: {
        userId: contact.peerId,
        peerFingerprint: contact.peerFingerprint,
        peerPublicKeyBase64: contact.peerPublicKeyBase64,
        peerUsername: contact.peerUsername ?? undefined,
      },
    })
  }

  function confirmRemove(contact: Contact) {
    const name = contact.peerUsername ? `@${contact.peerUsername}` : contact.peerFingerprint
    Alert.alert('Удалить контакт', `${name} исчезнет из списка. Переписка не удалится.`, [
      { text: 'Отмена', style: 'cancel' },
      { text: 'Удалить', style: 'destructive', onPress: () => removeContact(contact.peerId) },
    ])
  }

  return (
    <View style={[s.root, { paddingTop: insets.top }]}>
      <View style={s.header}>
        <Text style={s.title}>Контакты</Text>
      </View>

      {list.length === 0 ? (
        <View style={s.empty}>
          <Ionicons name="person-add-outline" size={56} color="#27272a" />
          <Text style={s.emptyText}>Сохранённые контакты появятся здесь</Text>
          <Text style={s.emptyHint}>
            Найдите пользователя по @имени во вкладке «Чаты», откройте его профиль и нажмите «В контакты».
          </Text>
        </View>
      ) : (
        <FlashList
          data={list}
          keyExtractor={c => c.peerId}
          renderItem={({ item }) => (
            <Pressable
              style={({ pressed }) => [s.row, pressed && s.rowPressed]}
              onPress={() => openContact(item)}
              onLongPress={() => confirmRemove(item)}
            >
              <PeerAvatar peerId={item.peerId} size={46} username={item.peerUsername ?? undefined} />
              <View style={s.rowText}>
                <Text style={s.rowTitle} numberOfLines={1}>
                  {item.peerUsername ? `@${item.peerUsername}` : item.peerFingerprint}
                </Text>
                <Text style={s.rowSubtitle} numberOfLines={1}>{item.peerFingerprint}</Text>
              </View>
              <Ionicons name="chatbubble-outline" size={20} color="#2f7bff" />
            </Pressable>
          )}
          contentContainerStyle={{ paddingBottom: insets.bottom + 90 }}
        />
      )}
    </View>
  )
}

const s = StyleSheet.create({
  root:   { flex: 1, backgroundColor: '#000000' },
  header: { paddingHorizontal: 20, paddingVertical: 16 },
  title:  { color: '#ffffff', fontSize: 28, fontWeight: '800' },

  row: {
    flexDirection: 'row', alignItems: 'center', gap: 12,
    paddingHorizontal: 16, paddingVertical: 10,
  },
  rowPressed:  { backgroundColor: '#1a1a1e' },
  rowText:     { flex: 1, gap: 2 },
  rowTitle:    { color: '#fff', fontSize: 16, fontWeight: '600' },
  rowSubtitle: { color: '#52525b', fontSize: 13, fontVariant: ['tabular-nums'] },

  empty:     { flex: 1, alignItems: 'center', justifyContent: 'center', gap: 12, paddingHorizontal: 32, paddingTop: 40 },
  emptyText: { color: '#a1a1aa', fontSize: 16, textAlign: 'center' },
  emptyHint: { color: '#52525b', fontSize: 13, textAlign: 'center', lineHeight: 18 },
})
