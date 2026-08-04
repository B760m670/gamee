import { memo } from 'react'
import { Pressable, View, Text, StyleSheet } from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import type { ChatMessage } from '../store/chat'
import { VoiceNote } from './VoiceNote'
import { MediaThumb } from './MediaThumb'

interface Props {
  msg: ChatMessage
  /** Long-press on the bubble — e.g. the chat screen's delete menu. */
  onLongPress?: (msg: ChatMessage) => void
}

function formatTime(epochMs: number): string {
  const d = new Date(epochMs)
  const hh = d.getHours().toString().padStart(2, '0')
  const mm = d.getMinutes().toString().padStart(2, '0')
  return `${hh}:${mm}`
}

function MessageBubbleBase({ msg, onLongPress }: Props) {
  const media = msg.media
  const isVoice = media && media.mime.startsWith('audio/') && !!media.localPath
  const isThumb = media && (media.mime.startsWith('image/') || media.mime.startsWith('video/')) && !!media.localPath
  return (
    <View style={[s.wrap, msg.outgoing ? s.wrapMine : s.wrapOther]}>
      <Pressable
        style={[
          s.bubble, msg.outgoing ? s.bubbleMine : s.bubbleOther,
          isThumb && s.bubbleThumb,
        ]}
        onLongPress={onLongPress ? () => onLongPress(msg) : undefined}
        delayLongPress={350}
      >
        {isVoice ? (
          <VoiceNote media={media!} outgoing={msg.outgoing} />
        ) : isThumb ? (
          <MediaThumb media={media!} />
        ) : (
          <Text style={s.text}>{msg.text}</Text>
        )}
        <View style={[s.meta, isThumb && s.metaThumb]}>
          <Text style={s.time}>{formatTime(msg.at)}</Text>
          {msg.outgoing ? (
            msg.status === 'sending' || msg.status === 'queued' ? (
              <Ionicons name="time-outline" size={13} color="rgba(255,255,255,0.7)" />
            ) : (
              <Ionicons name="checkmark" size={14} color="rgba(255,255,255,0.7)" />
            )
          ) : null}
        </View>
      </Pressable>
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
  bubbleThumb: { padding: 3, gap: 0 },
  bubbleMine:  { backgroundColor: '#2f7bff', borderBottomRightRadius: 5 },
  bubbleOther: { backgroundColor: '#1c1c1e', borderBottomLeftRadius: 5 },
  text: { color: '#fff', fontSize: 16, lineHeight: 21 },
  meta: { flexDirection: 'row', alignItems: 'center', gap: 3, marginLeft: 'auto' },
  metaThumb: {
    position: 'absolute', right: 8, bottom: 8,
    backgroundColor: 'rgba(0,0,0,0.45)', borderRadius: 10,
    paddingHorizontal: 6, paddingVertical: 2,
  },
  time: { color: 'rgba(255,255,255,0.6)', fontSize: 11 },
})
