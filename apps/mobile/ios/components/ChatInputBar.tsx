import { useState } from 'react'
import { View, TextInput, Pressable, Text, StyleSheet } from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import {
  voiceRecordingStart, voiceRecordingStop, voiceRecordingCancel,
  hasMicrophonePermission, requestMicrophonePermission,
} from '../modules/spiritchat-crypto-core'

interface Props {
  onSend: (text: string) => void
  /** Called with a recorded voice note's file URL and duration once the
   *  mic button is released (unless the recording was too short). */
  onSendVoice?: (fileUri: string, durationMs: number) => void
  onAttach?: () => void
}

/** Recordings shorter than this are treated as an accidental tap. */
const MIN_VOICE_MS = 500

export function ChatInputBar({ onSend, onSendVoice, onAttach }: Props) {
  const [text, setText] = useState('')
  const [recording, setRecording] = useState(false)
  const canSend = text.trim().length > 0

  function handleSend() {
    if (!canSend) return
    onSend(text)
    setText('')
  }

  async function startRecording() {
    if (!onSendVoice) return
    try {
      if (!hasMicrophonePermission()) {
        const granted = await requestMicrophonePermission()
        if (!granted) return
      }
      voiceRecordingStart()
      setRecording(true)
    } catch {
      setRecording(false)
    }
  }

  function stopRecording() {
    if (!recording) return
    setRecording(false)
    const result = voiceRecordingStop()
    if (result && result.durationMs >= MIN_VOICE_MS) {
      onSendVoice?.(result.fileUri, result.durationMs)
    } else {
      voiceRecordingCancel()
    }
  }

  return (
    <View style={s.row}>
      <Pressable onPress={onAttach} hitSlop={8} style={s.attach}>
        <Ionicons name="add" size={26} color="#71717a" />
      </Pressable>
      <View style={[s.field, recording && s.fieldRecording]}>
        {recording ? (
          <View style={s.recordingRow}>
            <View style={s.recDot} />
            <Text style={s.recText}>Идёт запись… отпустите, чтобы отправить</Text>
          </View>
        ) : (
          <TextInput
            style={s.input}
            value={text}
            onChangeText={setText}
            placeholder="Сообщение"
            placeholderTextColor="#52525b"
            multiline
          />
        )}
      </View>
      {canSend ? (
        <Pressable onPress={handleSend} style={s.send}>
          <Ionicons name="arrow-up" size={22} color="#fff" />
        </Pressable>
      ) : (
        <Pressable
          onPressIn={startRecording}
          onPressOut={stopRecording}
          disabled={!onSendVoice}
          style={[s.send, recording ? s.sendRecording : s.sendMic]}
        >
          <Ionicons name={recording ? 'mic' : 'mic-outline'} size={22} color="#fff" />
        </Pressable>
      )}
    </View>
  )
}

const s = StyleSheet.create({
  row: {
    flexDirection: 'row', alignItems: 'flex-end', gap: 8,
    paddingHorizontal: 10, paddingTop: 8,
  },
  attach: { paddingBottom: 6 },
  field: {
    flex: 1, backgroundColor: '#1c1c1e', borderRadius: 20,
    paddingHorizontal: 14, paddingVertical: 8, minHeight: 38, justifyContent: 'center',
  },
  fieldRecording: { backgroundColor: '#2a1a1a' },
  input:   { color: '#fff', fontSize: 16, maxHeight: 120, padding: 0 },
  recordingRow: { flexDirection: 'row', alignItems: 'center', gap: 8 },
  recDot:  { width: 10, height: 10, borderRadius: 5, backgroundColor: '#ef4444' },
  recText: { color: '#f87171', fontSize: 14 },
  send: {
    width: 38, height: 38, borderRadius: 19, backgroundColor: '#2f7bff',
    alignItems: 'center', justifyContent: 'center',
  },
  sendMic: { backgroundColor: '#3f3f46' },
  sendRecording: { backgroundColor: '#ef4444' },
})
