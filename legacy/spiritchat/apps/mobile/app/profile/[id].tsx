import { useEffect, useState } from 'react'
import {
  View, Text, Pressable, ScrollView, ActivityIndicator, Alert, StyleSheet,
} from 'react-native'
import { Ionicons } from '@expo/vector-icons'
import { GlassView } from 'expo-glass-effect'
import { useRouter, useLocalSearchParams } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { Avatar } from '../../components/Avatar'
import { api } from '../../lib/api'
import type { PublicUser } from '../../hooks/useUserSearch'

const AVATAR_SIZE = 100
const BTN_H = 44

interface ActionButton {
  key: string
  icon: keyof typeof Ionicons.glyphMap
  label: string
}

const ACTIONS: ActionButton[] = [
  { key: 'call',  icon: 'call',                label: 'Звонок' },
  { key: 'video', icon: 'videocam',            label: 'Видео' },
  { key: 'mute',  icon: 'notifications-off',   label: 'Уведомл.' },
  { key: 'more',  icon: 'ellipsis-horizontal', label: 'Ещё' },
]

export default function ProfileScreen() {
  const insets = useSafeAreaInsets()
  const router = useRouter()
  const params = useLocalSearchParams<{ id: string; preload?: string }>()

  const preloaded: PublicUser | null = (() => {
    try { return params.preload ? JSON.parse(params.preload) as PublicUser : null }
    catch { return null }
  })()

  const [user, setUser]       = useState<PublicUser | null>(preloaded)
  const [loading, setLoading] = useState(!preloaded)
  const [error, setError]     = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    ;(async () => {
      try {
        const res = await api.get<{ data: PublicUser }>(`/api/v1/users/${params.id}`)
        if (!cancelled) { setUser(res.data); setError(null) }
      } catch (err: unknown) {
        if (!cancelled && !preloaded) {
          setError(err instanceof Error ? err.message : 'Не удалось загрузить профиль')
        }
      } finally {
        if (!cancelled) setLoading(false)
      }
    })()
    return () => { cancelled = true }
  }, [params.id])

  function handleAction() {
    Alert.alert('Скоро', 'Эта функция появится позже.')
  }

  const title = user?.display_name?.trim() || (user?.username ? `@${user.username}` : 'Профиль')

  return (
    <View style={s.root}>
      <View style={[s.backOverlay, { top: insets.top + 10, left: 16 }]}>
        <Pressable onPress={() => router.back()}>
          <GlassView style={s.backBtn} glassEffectStyle="regular" isInteractive colorScheme="dark">
            <Ionicons name="chevron-back" size={22} color="#fff" />
          </GlassView>
        </Pressable>
      </View>

      {loading && !user ? (
        <View style={s.center}><ActivityIndicator color="#52525b" /></View>
      ) : error && !user ? (
        <View style={s.center}><Text style={s.errorText}>{error}</Text></View>
      ) : user ? (
        <ScrollView
          contentContainerStyle={{ paddingTop: insets.top + 64, paddingBottom: insets.bottom + 40 }}
          showsVerticalScrollIndicator={false}
        >
          <View style={s.headerSection}>
            <Avatar uri={user.avatar_url} size={AVATAR_SIZE} username={user.username ?? user.display_name} />
            <Text style={s.name} numberOfLines={1}>{title}</Text>
            {user.username ? <Text style={s.status}>@{user.username}</Text> : null}
          </View>

          <View style={s.actions}>
            {ACTIONS.map(a => (
              <Pressable key={a.key} style={s.action} onPress={handleAction}>
                <View style={s.actionIcon}>
                  <Ionicons name={a.icon} size={22} color="#2f7bff" />
                </View>
                <Text style={s.actionLabel}>{a.label}</Text>
              </Pressable>
            ))}
          </View>

          {user.bio ? (
            <View style={s.group}>
              <View style={s.infoRow}>
                <Text style={s.infoLabel}>О себе</Text>
                <Text style={s.infoValue}>{user.bio}</Text>
              </View>
            </View>
          ) : null}

          {user.username ? (
            <View style={[s.group, { marginTop: 12 }]}>
              <View style={s.infoRow}>
                <Text style={s.infoLabel}>Имя пользователя</Text>
                <Text style={s.infoValue}>@{user.username}</Text>
              </View>
            </View>
          ) : null}
        </ScrollView>
      ) : null}
    </View>
  )
}

const s = StyleSheet.create({
  root:   { flex: 1, backgroundColor: '#000' },
  center: { flex: 1, alignItems: 'center', justifyContent: 'center' },

  backOverlay: { position: 'absolute', zIndex: 10 },
  backBtn: {
    width: BTN_H, height: BTN_H, borderRadius: BTN_H / 2,
    alignItems: 'center', justifyContent: 'center',
  },

  headerSection: { alignItems: 'center', gap: 8, paddingBottom: 20 },
  name:   { color: '#fff', fontSize: 28, fontWeight: '700', marginTop: 14, paddingHorizontal: 24 },
  status: { color: '#52525b', fontSize: 16 },

  actions: {
    flexDirection: 'row', justifyContent: 'center', gap: 24,
    paddingVertical: 8, marginBottom: 24,
  },
  action:     { alignItems: 'center', gap: 6, width: 64 },
  actionIcon: {
    width: 52, height: 52, borderRadius: 26,
    backgroundColor: '#1c1c1e', alignItems: 'center', justifyContent: 'center',
  },
  actionLabel: { color: '#a1a1aa', fontSize: 12 },

  group:     { marginHorizontal: 16, backgroundColor: '#111114', borderRadius: 16, overflow: 'hidden' },
  infoRow:   { paddingHorizontal: 16, paddingVertical: 12, gap: 4 },
  infoLabel: { color: '#a1a1aa', fontSize: 12, fontWeight: '500' },
  infoValue: { color: '#fff', fontSize: 16, lineHeight: 22 },

  errorText: { color: '#f87171', fontSize: 15 },
})
