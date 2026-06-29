import { View, Text, TouchableOpacity, StyleSheet } from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'

export default function QRScreen() {
  const router = useRouter()
  const insets = useSafeAreaInsets()

  return (
    <View style={[s.root, { paddingTop: insets.top }]}>
      <View style={s.nav}>
        <TouchableOpacity onPress={() => router.back()} style={s.navBack} activeOpacity={0.7}>
          <Ionicons name="chevron-back" size={24} color="#2f7bff" />
          <Text style={s.navBackText}>Назад</Text>
        </TouchableOpacity>
        <Text style={s.navTitle}>QR-код</Text>
        <View style={{ flex: 1 }} />
      </View>
    </View>
  )
}

const s = StyleSheet.create({
  root: { flex: 1, backgroundColor: '#000' },
  nav: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingHorizontal: 8,
    paddingBottom: 12,
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderBottomColor: '#27272a',
  },
  navBack:     { flexDirection: 'row', alignItems: 'center', flex: 1 },
  navBackText: { color: '#2f7bff', fontSize: 16 },
  navTitle:    { color: '#fff', fontSize: 17, fontWeight: '600', flex: 2, textAlign: 'center' },
})
