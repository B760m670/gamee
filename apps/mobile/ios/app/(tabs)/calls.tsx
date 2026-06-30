import { View, Text, StyleSheet } from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { useSafeAreaInsets } from 'react-native-safe-area-context'

export default function CallsScreen() {
  const insets = useSafeAreaInsets()
  return (
    <View style={[styles.root, { paddingTop: insets.top }]}>
      <View style={styles.header}>
        <Text style={styles.title}>Calls</Text>
      </View>
      <View style={styles.empty}>
        <Ionicons name="call-outline" size={56} color="#27272a" />
        <Text style={styles.emptyText}>No recent calls</Text>
      </View>
    </View>
  )
}

const styles = StyleSheet.create({
  root:      { flex: 1, backgroundColor: '#000000' },
  header:    { paddingHorizontal: 20, paddingVertical: 16 },
  title:     { color: '#ffffff', fontSize: 28, fontWeight: '800' },
  empty:     { flex: 1, alignItems: 'center', justifyContent: 'center', gap: 12 },
  emptyText: { color: '#52525b', fontSize: 16 },
})
