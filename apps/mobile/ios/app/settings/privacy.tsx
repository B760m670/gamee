import { useState } from 'react'
import { View, Text, Pressable, Switch, StyleSheet } from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { GlassView } from 'expo-glass-effect'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { SettingsRow } from '../../components/SettingsRow'
import {
  mixRelayParticipationEnabled,
  setMixRelayParticipationEnabled,
  mixDummyTrafficBytesPerHourEstimate,
  allowsDirectDelivery,
  setAllowsDirectDelivery,
} from '../../modules/spiritchat-crypto-core'

const BTN_H = 42

function formatBytesPerHour(bytes: number): string {
  if (bytes < 1024) return `${bytes} Б/ч`
  return `${(bytes / 1024).toFixed(0)} КБ/ч`
}

export default function PrivacyScreen() {
  const router = useRouter()
  const insets = useSafeAreaInsets()
  const [mixRelayEnabled, setMixRelayEnabled] = useState(() => mixRelayParticipationEnabled())
  const [trafficEstimate] = useState(() => mixDummyTrafficBytesPerHourEstimate())

  const [directAllowed, setDirectAllowed] = useState(() => allowsDirectDelivery())

  const toggleMixRelay = (value: boolean) => {
    setMixRelayParticipationEnabled(value)
    setMixRelayEnabled(value)
  }

  const toggleDirect = (value: boolean) => {
    setAllowsDirectDelivery(value)
    setDirectAllowed(value)
  }

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

        <View style={s.rowGap} />

        <SettingsRow onPress={() => router.push('/settings/blocked')}>
          <View style={[s.iconWrap, { backgroundColor: '#ef4444' }]}>
            <Ionicons name="ban" size={15} color="#fff" />
          </View>
          <Text style={s.rowLabel}>Заблокированные</Text>
          <Ionicons name="chevron-forward" size={16} color="rgba(255,255,255,0.3)" />
        </SettingsRow>

        <View style={s.rowGap} />

        <Pressable onPress={() => toggleMixRelay(!mixRelayEnabled)} style={s.toggleRow}>
          <View style={[s.iconWrap, { backgroundColor: '#06b6d4' }]}>
            <Ionicons name="git-network" size={15} color="#fff" />
          </View>
          <View style={s.toggleTextWrap}>
            <Text style={s.toggleTitle}>Ретрансляция сообщений</Text>
            <Text style={s.rowSubLabel}>
              Помогает пересылать чужие зашифрованные сообщения через анонимную сеть,
              когда телефон заряжается и на экране. Взамен ваши сообщения тоже смогут
              доставляться, пока собеседник офлайн — без единого сервера.
              {'\n'}Фоновый трафик: ~{formatBytesPerHour(trafficEstimate)} (без учёта переписки).
            </Text>
          </View>
          <Switch
            value={mixRelayEnabled}
            onValueChange={toggleMixRelay}
            trackColor={{ false: '#3a3a3c', true: '#06b6d4' }}
          />
        </Pressable>

        <View style={s.rowGap} />

        <Pressable onPress={() => toggleDirect(!directAllowed)} style={s.toggleRow}>
          <View style={[s.iconWrap, { backgroundColor: '#f59e0b' }]}>
            <Ionicons name="flash" size={15} color="#fff" />
          </View>
          <View style={s.toggleTextWrap}>
            <Text style={s.toggleTitle}>Прямая доставка при недоступной сети</Text>
            <Text style={s.rowSubLabel}>
              Обычно сообщения идут через анонимную сеть, и наблюдатель видит связь
              с промежуточным узлом, а не с человеком. Шифрование скрывает, что вы
              написали; только анонимная сеть скрывает, кому.
              {'\n'}Если включить, то когда анонимный путь недоступен, сообщение уйдёт
              напрямую собеседнику — быстрее и надёжнее, но ваш оператор связи увидит,
              с кем именно вы общаетесь. Выключено — сообщение подождёт.
            </Text>
          </View>
          <Switch
            value={directAllowed}
            onValueChange={toggleDirect}
            trackColor={{ false: '#3a3a3c', true: '#f59e0b' }}
          />
        </Pressable>
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
  rowGap:   { height: 10 },

  toggleRow: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingHorizontal: 14,
    paddingVertical: 12,
    gap: 12,
    backgroundColor: '#1c1c1e',
    borderRadius: 20,
  },
  toggleTextWrap: { flex: 1 },
  toggleTitle:    { color: '#fff', fontSize: 17, marginBottom: 4 },
  rowSubLabel:    { color: '#71717a', fontSize: 13, lineHeight: 18 },
})
