import { View, Text, StyleSheet } from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { useSafeAreaInsets } from 'react-native-safe-area-context'

// @username search + starting a conversation moved to the Chats tab (see
// app/(tabs)/messages.tsx) — a chat now opens directly from a search
// result instead of going through a separate "connect" step here. This
// tab is reserved for a future *saved* contacts list (people explicitly
// added, not every past search) — not built yet.
export default function ContactsScreen() {
  const insets = useSafeAreaInsets()

  return (
    <View style={[s.root, { paddingTop: insets.top }]}>
      <View style={s.header}>
        <Text style={s.title}>Контакты</Text>
      </View>
      <View style={s.empty}>
        <Ionicons name="person-add-outline" size={56} color="#27272a" />
        <Text style={s.emptyText}>Сохранённые контакты появятся здесь</Text>
        <Text style={s.emptyHint}>
          Эта функция ещё не реализована. Чтобы написать кому-то, найдите его по @имени пользователя во вкладке «Чаты».
        </Text>
      </View>
    </View>
  )
}

const s = StyleSheet.create({
  root:   { flex: 1, backgroundColor: '#000000' },
  header: { paddingHorizontal: 20, paddingVertical: 16 },
  title:  { color: '#ffffff', fontSize: 28, fontWeight: '800' },

  empty:     { flex: 1, alignItems: 'center', justifyContent: 'center', gap: 12, paddingHorizontal: 32, paddingTop: 40 },
  emptyText: { color: '#a1a1aa', fontSize: 16, textAlign: 'center' },
  emptyHint: { color: '#52525b', fontSize: 13, textAlign: 'center', lineHeight: 18 },
})
