import { useMemo } from 'react'
import { View, Text, FlatList, Pressable, StyleSheet } from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { useRouter, useLocalSearchParams } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { PeerAvatar } from '../../../components/PeerAvatar'
import { useChatStore } from '../../../store/chat'
import { useGroupStore } from '../../../store/groups'
import { useProfileStore } from '../../../store/profile'

/** Add/remove members — see `chatAddGroupMember`/`chatRemoveGroupMember`'s own doc comments: a new member must already be an existing 1:1 contact. */
export default function GroupMembersScreen() {
  const insets = useSafeAreaInsets()
  const router = useRouter()
  const { groupId } = useLocalSearchParams<{ groupId: string }>()

  const group = useGroupStore(s => s.groups[groupId])
  const addMember = useGroupStore(s => s.addMember)
  const removeMember = useGroupStore(s => s.removeMember)
  const conversations = useChatStore(s => s.conversations)
  const myPeerId = useProfileStore(s => s.peerId)

  const members = group?.members ?? []
  const addable = useMemo(
    () => Object.values(conversations).filter(c => c.peerId !== myPeerId && !members.includes(c.peerId)),
    [conversations, members]
  )

  return (
    <View style={[s.root, { paddingTop: insets.top }]}>
      <View style={s.header}>
        <Pressable onPress={() => router.back()} hitSlop={8}>
          <Ionicons name="chevron-back" size={24} color="#fff" />
        </Pressable>
        <Text style={s.title}>Участники</Text>
        <View style={{ width: 24 }} />
      </View>

      <Text style={s.sectionHeader}>В группе</Text>
      <FlatList
        data={members}
        keyExtractor={m => m}
        renderItem={({ item: peerId }) => {
          const known = conversations[peerId]
          const title = known?.peerUsername ? `@${known.peerUsername}` : (known?.peerFingerprint ?? peerId)
          return (
            <View style={s.row}>
              <PeerAvatar peerId={peerId} size={44} username={known?.peerUsername ?? undefined} />
              <Text style={s.rowTitle} numberOfLines={1}>{title}</Text>
              <Pressable onPress={() => removeMember(groupId, peerId)} hitSlop={8}>
                <Ionicons name="remove-circle-outline" size={22} color="#f87171" />
              </Pressable>
            </View>
          )
        }}
      />

      {addable.length > 0 ? (
        <>
          <Text style={s.sectionHeader}>Добавить</Text>
          <FlatList
            data={addable}
            keyExtractor={c => c.peerId}
            contentContainerStyle={{ paddingBottom: insets.bottom + 20 }}
            renderItem={({ item }) => {
              const title = item.peerUsername ? `@${item.peerUsername}` : item.peerFingerprint
              return (
                <Pressable style={s.row} onPress={() => addMember(groupId, item.peerId)}>
                  <PeerAvatar peerId={item.peerId} size={44} username={item.peerUsername ?? undefined} />
                  <Text style={s.rowTitle} numberOfLines={1}>{title}</Text>
                  <Ionicons name="add-circle-outline" size={22} color="#2f7bff" />
                </Pressable>
              )
            }}
          />
        </>
      ) : null}
    </View>
  )
}

const s = StyleSheet.create({
  root: { flex: 1, backgroundColor: '#000' },

  header: {
    flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between',
    paddingHorizontal: 16, paddingVertical: 12,
    borderBottomWidth: StyleSheet.hairlineWidth, borderBottomColor: '#1c1c1e',
  },
  title: { color: '#fff', fontSize: 16, fontWeight: '600' },

  sectionHeader: {
    color: '#52525b', fontSize: 13, fontWeight: '600',
    paddingHorizontal: 16, paddingTop: 14, paddingBottom: 6,
    textTransform: 'uppercase',
  },

  row: {
    flexDirection: 'row', alignItems: 'center', gap: 12,
    paddingHorizontal: 16, paddingVertical: 8,
  },
  rowTitle: { flex: 1, color: '#fff', fontSize: 16 },
})
