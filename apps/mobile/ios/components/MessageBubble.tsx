import { memo } from 'react'
import { View, Text, StyleSheet } from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import type { ChatMessage } from '../lib/chat'

interface Props {
  msg: ChatMessage
  isMine: boolean
  readByOther: boolean
}

function formatTime(iso: string): string {
  const d = new Date(iso)
  const hh = d.getHours().toString().padStart(2, '0')
  const mm = d.getMinutes().toString().padStart(2, '0')
  return `${hh}:${mm}`
}

function MessageBubbleBase({ msg, isMine, readByOther }: Props) {
  return (
    <View style={[s.wrap, isMine ? s.wrapMine : s.wrapOther]}>
      <View style={[s.bubble, isMine ? s.bubbleMine : s.bubbleOther]}>
        {msg.content ? <Text style={s.text}>{msg.content}</Text> : null}
        <View style={s.meta}>
          <Text style={s.time}>{formatTime(msg.created_at)}</Text>
          {isMine ? (
            msg.status === 'sending' ? (
              <Ionicons name="time-outline" size={13} color="rgba(255,255,255,0.7)" />
            ) : msg.status === 'failed' ? (
              <Ionicons name="alert-circle" size={13} color="#fecaca" />
            ) : readByOther ? (
              <Ionicons name="checkmark-done" size={14} color="#9fd1ff" />
            ) : (
              <Ionicons name="checkmark" size={14} color="rgba(255,255,255,0.7)" />
            )
          ) : null}
        </View>
      </View>
    </View>
  )
}

export const MessageBubble = memo(MessageBubbleBase)

const s = StyleSheet.create({
  wrap:      { paddingHorizontal: 10, marginVertical: 2, maxWidth: '100%' },
  wrapMine:  { alignItems: 'flex-end' },
  wrapOther: { alignItems: 'flex-start' },
  bubble: {
    maxWidth: '78%', borderRadius: 18, paddingHorizontal: 12, paddingVertical: 7,
    flexDirection: 'row', flexWrap: 'wrap', alignItems: 'flex-end', gap: 6,
  },
  bubbleMine:  { backgroundColor: '#2f7bff', borderBottomRightRadius: 5 },
  bubbleOther: { backgroundColor: '#1c1c1e', borderBottomLeftRadius: 5 },
  text: { color: '#fff', fontSize: 16, lineHeight: 21 },
  meta: { flexDirection: 'row', alignItems: 'center', gap: 3, marginLeft: 'auto' },
  time: { color: 'rgba(255,255,255,0.6)', fontSize: 11 },
})
