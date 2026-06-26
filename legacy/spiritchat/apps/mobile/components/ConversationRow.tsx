import { memo } from 'react'
import { Pressable, View, Text, StyleSheet } from 'react-native'
import { Avatar } from './Avatar'
import type { ConversationSummary } from '../lib/chat'

interface Props {
  item: ConversationSummary
  meId: string
  onPress: (otherUserId: string) => void
}

function formatWhen(iso: string | null): string {
  if (!iso) return ''
  const d = new Date(iso)
  const now = new Date()
  const sameDay = d.toDateString() === now.toDateString()
  if (sameDay) {
    return `${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}`
  }
  const yesterday = new Date(now); yesterday.setDate(now.getDate() - 1)
  if (d.toDateString() === yesterday.toDateString()) return 'Вчера'
  return `${d.getDate().toString().padStart(2, '0')}.${(d.getMonth() + 1).toString().padStart(2, '0')}`
}

function ConversationRowBase({ item, meId, onPress }: Props) {
  const other = item.other
  const title = other?.display_name?.trim() || (other?.username ? `@${other.username}` : 'Без имени')

  const lm = item.last_message
  let preview = ''
  if (lm) {
    const body = lm.content?.trim() || (lm.media_url ? '📎 Вложение' : '')
    preview = lm.sender_id === meId ? `Вы: ${body}` : body
  }

  return (
    <Pressable
      style={({ pressed }) => [s.row, pressed && s.rowPressed]}
      onPress={() => other && onPress(other.id)}
    >
      <Avatar uri={other?.avatar_url} size={54} username={other?.username ?? other?.display_name} />
      <View style={s.center}>
        <Text style={s.title} numberOfLines={1}>{title}</Text>
        <Text style={s.preview} numberOfLines={1}>{preview}</Text>
      </View>
      <View style={s.right}>
        <Text style={s.time}>{formatWhen(lm?.created_at ?? null)}</Text>
        {item.unread > 0 ? (
          <View style={s.badge}><Text style={s.badgeText}>{item.unread}</Text></View>
        ) : null}
      </View>
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
  right:   { alignItems: 'flex-end', gap: 6 },
  time:    { color: '#52525b', fontSize: 12 },
  badge: {
    minWidth: 20, height: 20, borderRadius: 10, paddingHorizontal: 6,
    backgroundColor: '#2f7bff', alignItems: 'center', justifyContent: 'center',
  },
  badgeText: { color: '#fff', fontSize: 12, fontWeight: '700' },
})
