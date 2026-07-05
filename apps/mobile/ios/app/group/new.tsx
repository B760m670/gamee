import { useMemo, useState } from 'react'
import { View, Text, TextInput, FlatList, Pressable, StyleSheet } from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { PeerAvatar } from '../../components/PeerAvatar'
import { useChatStore, type Conversation } from '../../store/chat'
import { useGroupStore } from '../../store/groups'

/**
 * Members are picked from existing 1:1 conversations only — a group
 * control message piggybacks on an *existing* pairwise session (see
 * `chatCreateGroup`'s own doc comment), so anyone not already a contact
 * can't be added here yet.
 */
export default function NewGroupScreen() {
  const insets = useSafeAreaInsets()
  const router = useRouter()
  const conversations = useChatStore(s => s.conversations)
  const createGroup = useGroupStore(s => s.createGroup)

  const [name, setName] = useState('')
  const [selected, setSelected] = useState<Set<string>>(new Set())

  const contacts = useMemo(() => Object.values(conversations), [conversations])
  const canCreate = name.trim().length > 0 && selected.size > 0

  function toggle(peerId: string) {
    setSelected(prev => {
      const next = new Set(prev)
      if (next.has(peerId)) next.delete(peerId)
      else next.add(peerId)
      return next
    })
  }

  function handleCreate() {
    if (!canCreate) return
    const groupId = createGroup(name, Array.from(selected))
    if (groupId) router.replace({ pathname: '/group/[groupId]', params: { groupId } })
    else router.back()
  }

  return (
    <View style={[s.root, { paddingTop: insets.top }]}>
      <View style={s.header}>
        <Pressable onPress={() => router.back()} hitSlop={8}>
          <Text style={s.cancel}>Отмена</Text>
        </Pressable>
        <Text style={s.title}>Новая группа</Text>
        <Pressable onPress={handleCreate} disabled={!canCreate} hitSlop={8}>
          <Text style={[s.create, !canCreate && s.createOff]}>Создать</Text>
        </Pressable>
      </View>

      <View style={s.nameRow}>
        <TextInput
          style={s.nameInput}
          value={name}
          onChangeText={setName}
          placeholder="Название группы"
          placeholderTextColor="#52525b"
        />
      </View>

      <Text style={s.sectionHeader}>Участники</Text>
      <FlatList
        data={contacts}
        keyExtractor={(c: Conversation) => c.peerId}
        contentContainerStyle={{ paddingBottom: insets.bottom + 20 }}
        renderItem={({ item }) => {
          const isSelected = selected.has(item.peerId)
          const title = item.peerUsername ? `@${item.peerUsername}` : item.peerFingerprint
          return (
            <Pressable style={s.row} onPress={() => toggle(item.peerId)}>
              <PeerAvatar peerId={item.peerId} size={44} username={item.peerUsername ?? undefined} />
              <Text style={s.rowTitle} numberOfLines={1}>{title}</Text>
              <Ionicons
                name={isSelected ? 'checkmark-circle' : 'ellipse-outline'}
                size={22}
                color={isSelected ? '#2f7bff' : '#3f3f46'}
              />
            </Pressable>
          )
        }}
        ListEmptyComponent={
          <View style={s.empty}>
            <Text style={s.emptyText}>Сначала напишите кому-нибудь — группу можно создать только с существующими собеседниками.</Text>
          </View>
        }
      />
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
  title:     { color: '#fff', fontSize: 16, fontWeight: '600' },
  cancel:    { color: '#71717a', fontSize: 16 },
  create:    { color: '#2f7bff', fontSize: 16, fontWeight: '600' },
  createOff: { color: '#3f3f46' },

  nameRow: { paddingHorizontal: 16, paddingVertical: 12 },
  nameInput: {
    color: '#fff', fontSize: 17, backgroundColor: '#1c1c1e',
    borderRadius: 12, paddingHorizontal: 14, height: 46,
  },

  sectionHeader: {
    color: '#52525b', fontSize: 13, fontWeight: '600',
    paddingHorizontal: 16, paddingTop: 8, paddingBottom: 6,
    textTransform: 'uppercase',
  },

  row: {
    flexDirection: 'row', alignItems: 'center', gap: 12,
    paddingHorizontal: 16, paddingVertical: 8,
  },
  rowTitle: { flex: 1, color: '#fff', fontSize: 16 },

  empty:     { paddingHorizontal: 32, paddingTop: 40 },
  emptyText: { color: '#52525b', fontSize: 14, textAlign: 'center', lineHeight: 20 },
})
