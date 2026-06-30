import { Pressable, StyleSheet, type ViewStyle } from 'react-native'

interface Props {
  onPress: () => void
  style?: ViewStyle | ViewStyle[]
  children: React.ReactNode
}

export function SettingsRow({ onPress, style, children }: Props) {
  return (
    <Pressable
      onPress={onPress}
      style={({ pressed }) => [s.row, style, pressed && s.rowPressed]}
    >
      {children}
    </Pressable>
  )
}

const s = StyleSheet.create({
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingHorizontal: 14,
    paddingVertical: 10,
    gap: 12,
    minHeight: 44,
    backgroundColor: '#1c1c1e',
    borderRadius: 999,
    overflow: 'hidden',
  },
  rowPressed: {
    backgroundColor: '#3a3a3c',
  },
})
