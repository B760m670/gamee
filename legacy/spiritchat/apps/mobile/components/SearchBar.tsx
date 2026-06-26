import { View, TextInput, Pressable, Text, StyleSheet } from 'react-native'
import { Ionicons } from '@expo/vector-icons'

interface Props {
  value: string
  onChangeText: (t: string) => void
  onCancel: () => void
  autoFocus?: boolean
}

export function SearchBar({ value, onChangeText, onCancel, autoFocus }: Props) {
  return (
    <View style={s.row}>
      <View style={s.field}>
        <Ionicons name="search" size={18} color="#52525b" />
        <TextInput
          style={s.input}
          value={value}
          onChangeText={onChangeText}
          placeholder="Поиск"
          placeholderTextColor="#52525b"
          autoCapitalize="none"
          autoCorrect={false}
          autoFocus={autoFocus}
          returnKeyType="search"
          accessibilityRole="search"
        />
        {value.length > 0 ? (
          <Pressable onPress={() => onChangeText('')} hitSlop={8}>
            <Ionicons name="close-circle" size={18} color="#52525b" />
          </Pressable>
        ) : null}
      </View>
      <Pressable onPress={onCancel} hitSlop={8}>
        <Text style={s.cancel}>Отмена</Text>
      </Pressable>
    </View>
  )
}

const s = StyleSheet.create({
  row: {
    flexDirection: 'row', alignItems: 'center', gap: 10,
    paddingHorizontal: 16, paddingVertical: 8,
  },
  field: {
    flex: 1, flexDirection: 'row', alignItems: 'center', gap: 8,
    backgroundColor: '#1c1c1e', borderRadius: 12,
    paddingHorizontal: 12, height: 38,
  },
  input:  { flex: 1, color: '#fff', fontSize: 16, height: '100%', padding: 0 },
  cancel: { color: '#2f7bff', fontSize: 16 },
})
