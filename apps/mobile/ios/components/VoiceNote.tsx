import { memo, useEffect } from 'react'
import { Pressable, View, Text, StyleSheet } from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { useVideoPlayer } from 'expo-video'
import { useEvent } from 'expo'
import type { ChatMedia } from '../store/chat'

interface Props {
  media: ChatMedia
  /** Tints the controls for outgoing (on-blue) vs incoming (on-grey) bubbles. */
  outgoing: boolean
}

function formatDuration(ms: number | null): string {
  const total = Math.max(0, Math.round((ms ?? 0) / 1000))
  const mm = Math.floor(total / 60)
  const ss = total % 60
  return `${mm}:${ss.toString().padStart(2, '0')}`
}

/**
 * A play/pause voice-note bubble. `expo-video`'s player handles the AAC/.m4a
 * the native `VoiceRecorder` produces natively — we never mount a `VideoView`,
 * it's audio-only, so the player is purely a headless audio engine here.
 */
function VoiceNoteBase({ media, outgoing }: Props) {
  const player = useVideoPlayer(media.localPath, p => {
    p.loop = false
  })
  const { isPlaying } = useEvent(player, 'playingChange', { isPlaying: player.playing })

  // When playback reaches the end, rewind so the next tap starts over.
  useEffect(() => {
    const sub = player.addListener('playToEnd', () => {
      player.currentTime = 0
      player.pause()
    })
    return () => sub.remove()
  }, [player])

  const tint = outgoing ? '#fff' : '#e5e5e7'

  function toggle() {
    if (isPlaying) player.pause()
    else {
      if (player.currentTime >= player.duration && player.duration > 0) player.currentTime = 0
      player.play()
    }
  }

  return (
    <Pressable style={s.row} onPress={toggle} hitSlop={6}>
      <Ionicons name={isPlaying ? 'pause' : 'play'} size={22} color={tint} />
      <View style={s.waveform}>
        <View style={[s.bar, { height: 8, backgroundColor: tint }]} />
        <View style={[s.bar, { height: 16, backgroundColor: tint }]} />
        <View style={[s.bar, { height: 22, backgroundColor: tint }]} />
        <View style={[s.bar, { height: 12, backgroundColor: tint }]} />
        <View style={[s.bar, { height: 18, backgroundColor: tint }]} />
        <View style={[s.bar, { height: 9, backgroundColor: tint }]} />
      </View>
      <Text style={[s.dur, { color: tint }]}>{formatDuration(media.durationMs)}</Text>
    </Pressable>
  )
}

export const VoiceNote = memo(VoiceNoteBase)

const s = StyleSheet.create({
  row: { flexDirection: 'row', alignItems: 'center', gap: 8, paddingVertical: 2, minWidth: 150 },
  waveform: { flexDirection: 'row', alignItems: 'center', gap: 3, height: 24 },
  bar: { width: 3, borderRadius: 1.5, opacity: 0.85 },
  dur: { fontSize: 12, marginLeft: 2 },
})
