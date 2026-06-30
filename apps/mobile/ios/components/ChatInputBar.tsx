import { useState } from 'react'
import { View, TextInput, Pressable, StyleSheet } from 'react-native'
import { Ionicons } from '@expo/vector-icons'

interface Props {
  onSend: (text: string) => void
  onAttach?: () => void
}

export function ChatInputBar({ onSend, onAttach }: Props) {
  const [text, setText] = useState('')
  const canSend = text.trim().length > 0

  function handleSend() {
    if (!canSend) return
    onSend(text)
    setText('')
  }

  return (
    <View style={s.row}>
      <Pressable onPress={onAttach} hitSlop={8} style={s.attach}>
        <Ionicons name="add" size={26} color="#71717a" />
      </Pressable>
      <View style={s.field}>
        <TextInput
          style={s.input}
          value={text}
          onChangeText={setText}
          placeholder="Сообщение"
          placeholderTextColor="#52525b"
          multiline
        />
      </View>
      <Pressable onPress={handleSend} disabled={!canSend} style={[s.send, !canSend && s.sendOff]}>
        <Ionicons name="arrow-up" size={22} color="#fff" />
      </Pressable>
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
  input:   { color: '#fff', fontSize: 16, maxHeight: 120, padding: 0 },
  send: {
    width: 38, height: 38, borderRadius: 19, backgroundColor: '#2f7bff',
    alignItems: 'center', justifyContent: 'center',
  },
  sendOff: { backgroundColor: '#27272a' },
})
