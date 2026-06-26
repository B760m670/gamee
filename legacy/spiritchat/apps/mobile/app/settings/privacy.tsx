import { View, Text, Pressable, StyleSheet } from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { GlassView } from 'expo-glass-effect'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { SettingsRow } from '../../components/SettingsRow'

const BTN_H = 42

export default function PrivacyScreen() {
  const router = useRouter()
  const insets = useSafeAreaInsets()

  return (
    <View style={s.root}>
      {/* Nav */}
      <View style={[s.nav, { paddingTop: insets.top + 8 }]}>
        <Pressable onPress={() => router.back()} style={s.backWrap}>
          <GlassView style={s.backBtn} glassEffectStyle="regular" isInteractive colorScheme="dark">
            <Ionicons name="chevron-back" size={22} color="#fff" />
          </GlassView>
        </Pressable>
        <Text style={s.navTitle}>Конфиденциальность</Text>
        <View style={s.navSpacer} />
      </View>

      {/* Rows */}
      <View style={s.content}>
        <SettingsRow onPress={() => router.push('/settings/cloud-password')}>
          <View style={[s.iconWrap, { backgroundColor: '#8b5cf6' }]}>
            <Ionicons name="shield-checkmark" size={15} color="#fff" />
          </View>
          <Text style={s.rowLabel}>Облачный пароль</Text>
          <Ionicons name="chevron-forward" size={16} color="rgba(255,255,255,0.3)" />
        </SettingsRow>
      </View>
    </View>
  )
}

const s = StyleSheet.create({
  root: { flex: 1, backgroundColor: '#000' },

  nav: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingHorizontal: 16,
    paddingBottom: 10,
  },
  backWrap:  { zIndex: 1 },
  backBtn: {
    width: BTN_H, height: BTN_H, borderRadius: BTN_H / 2,
    alignItems: 'center', justifyContent: 'center',
  },
  navTitle:  { flex: 1, color: '#fff', fontSize: 17, fontWeight: '600', textAlign: 'center' },
  navSpacer: { width: BTN_H },

  content: { paddingHorizontal: 16, paddingTop: 20 },

  iconWrap: {
    width: 30, height: 30, borderRadius: 8,
    alignItems: 'center', justifyContent: 'center',
  },
  rowLabel: { flex: 1, color: '#fff', fontSize: 17 },
})
