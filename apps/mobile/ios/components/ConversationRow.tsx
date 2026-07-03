import { memo } from 'react'
import { Pressable, View, Text, StyleSheet } from 'react-native'
import { Avatar } from './Avatar'
import type { Conversation } from '../store/chat'

interface Props {
  item: Conversation
  onPress: (peerId: string) => void
}

function formatWhen(epochMs: number): string {
  if (!epochMs) return ''
  const d = new Date(epochMs)
  const now = new Date()
  const sameDay = d.toDateString() === now.toDateString()
  if (sameDay) {
    return `${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}`
  }
  const yesterday = new Date(now); yesterday.setDate(now.getDate() - 1)
  if (d.toDateString() === yesterday.toDateString()) return 'Вчера'
  return `${d.getDate().toString().padStart(2, '0')}.${(d.getMonth() + 1).toString().padStart(2, '0')}`
}

function ConversationRowBase({ item, onPress }: Props) {
  const title = item.peerUsername ? `@${item.peerUsername}` : item.peerFingerprint

  return (
    <Pressable
      style={({ pressed }) => [s.row, pressed && s.rowPressed]}
      onPress={() => onPress(item.peerId)}
    >
      <Avatar uri={null} size={54} username={item.peerUsername ?? undefined} />
      <View style={s.center}>
        <Text style={s.title} numberOfLines={1}>{title}</Text>
        <Text style={s.preview} numberOfLines={1}>{item.lastMessageText}</Text>
      </View>
      <Text style={s.time}>{formatWhen(item.lastMessageAt)}</Text>
    </Pressable>
  )
}

export const ConversationRow = memo(ConversationRowBase)

const s = StyleSheet.create({
  row: {
    flexDirection: 'row', alignItems: 'center', gap: 12,
    paddingHorizontal: 16, paddingVertical: 8,
  },
  rowPressed: { backgroundColor: '#1a1a1e' },
  center:  { flex: 1, gap: 3 },
  title:   { color: '#fff', fontSize: 16, fontWeight: '600' },
  preview: { color: '#71717a', fontSize: 14 },
  time:    { color: '#52525b', fontSize: 12 },
})
