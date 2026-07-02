import { useEffect, useState } from 'react'
import {
  View, Text, Pressable, ScrollView, StyleSheet, Alert, ActivityIndicator,
} from 'react-native'
import AsyncStorage from '@react-native-async-storage/async-storage'
import { Ionicons } from '@expo/vector-icons'
import { GlassView } from 'expo-glass-effect'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { useProfileStore, displayNameKey } from '../../store/profile'
import { navigateAfterAccountChange } from '../../utils/navigation'

const BTN_H = 44
const MAX_ACCOUNTS = 3

export default function AccountsScreen() {
  const router = useRouter()
  const insets = useSafeAreaInsets()
  const { accounts, activeSlot, switchAccount, removeAccount } = useProfileStore(s => ({
    accounts:      s.accounts,
    activeSlot:    s.activeSlot,
    switchAccount: s.switchAccount,
    removeAccount: s.removeAccount,
  }))

  // Each account's own display name, if it set one — peeked directly from
  // AsyncStorage (not through the store, which only ever holds the
  // *active* account's fields) purely so the list reads as more than a
  // wall of fingerprints.
  const [names, setNames] = useState<Record<number, string>>({})
  useEffect(() => {
    let cancelled = false
    Promise.all(
      accounts.map(async (a) => [a.slot, (await AsyncStorage.getItem(displayNameKey(a.fingerprint))) ?? ''] as const)
    ).then((pairs) => {
      if (!cancelled) setNames(Object.fromEntries(pairs))
    })
    return () => { cancelled = true }
  }, [accounts])

  const [busySlot, setBusySlot] = useState<number | null>(null)

  async function handleSwitch(slot: number) {
    if (slot === activeSlot || busySlot !== null) return
    setBusySlot(slot)
    try {
      await switchAccount(slot)
      navigateAfterAccountChange(router)
    } catch (e) {
      Alert.alert('Не удалось переключиться', e instanceof Error ? e.message : 'Неизвестная ошибка')
      setBusySlot(null)
    }
  }

  function handleRemove(slot: number) {
    const isActive = slot === activeSlot
    Alert.alert(
      'Удалить аккаунт?',
      isActive
        ? 'Здесь нет сервера — вернуться обратно можно только по фразе восстановления. Убедись, что сохранил её, иначе аккаунт будет утерян навсегда.'
        : 'Вернуться обратно можно только по фразе восстановления этого аккаунта.',
      [
        { text: 'Отмена', style: 'cancel' },
        {
          text: 'Удалить',
          style: 'destructive',
          onPress: async () => {
            setBusySlot(slot)
            try {
              await removeAccount(slot)
              navigateAfterAccountChange(router)
            } catch (e) {
              Alert.alert('Не удалось удалить', e instanceof Error ? e.message : 'Неизвестная ошибка')
              setBusySlot(null)
            }
          },
        },
      ]
    )
  }

  function handleAddAccount() {
    if (accounts.length >= MAX_ACCOUNTS) {
      Alert.alert('Достигнут лимит', `Можно зарегистрировать не более ${MAX_ACCOUNTS} аккаунтов на этом устройстве — удали один, чтобы добавить другой.`)
      return
    }
    router.push('/(onboarding)/welcome')
  }

  return (
    <View style={s.root}>
      <View style={[s.btnOverlay, { top: insets.top + 10, left: 16 }]}>
        <Pressable onPress={() => router.back()}>
          <GlassView style={s.backBtn} glassEffectStyle="regular" isInteractive colorScheme="dark">
            <Ionicons name="chevron-back" size={22} color="#fff" />
          </GlassView>
        </Pressable>
      </View>

      <ScrollView
        contentContainerStyle={{ paddingTop: insets.top + 70, paddingBottom: insets.bottom + 40, paddingHorizontal: 24 }}
        showsVerticalScrollIndicator={false}
      >
        <Text style={s.title}>Аккаунты</Text>
        <Text style={s.subtitle}>
          До {MAX_ACCOUNTS} аккаунтов на этом устройстве одновременно — переключение мгновенное, фраза восстановления не нужна. Она нужна только чтобы добавить аккаунт, которого ещё нет на этом устройстве.
        </Text>

        <View style={s.group}>
          {accounts.map((a, i) => {
            const isActive = a.slot === activeSlot
            const busy = busySlot === a.slot
            return (
              <View key={a.slot} style={[s.row, i > 0 && s.rowBorder]}>
                <Pressable style={s.rowMain} onPress={() => handleSwitch(a.slot)} disabled={busySlot !== null}>
                  <View style={s.rowText}>
                    <Text style={s.rowName}>{names[a.slot] || 'Без имени'}</Text>
                    <Text style={s.rowFingerprint}>{a.fingerprint}</Text>
                  </View>
                  {busy ? (
                    <ActivityIndicator color="#71717a" size="small" />
                  ) : isActive ? (
                    <Ionicons name="checkmark-circle" size={22} color="#4ade80" />
                  ) : null}
                </Pressable>
                <Pressable onPress={() => handleRemove(a.slot)} disabled={busySlot !== null} hitSlop={8} style={s.removeBtn}>
                  <Ionicons name="trash-outline" size={18} color="#ef4444" />
                </Pressable>
              </View>
            )
          })}
        </View>

        <Pressable
          style={({ pressed }) => [s.addBtn, pressed && s.addBtnPressed]}
          onPress={handleAddAccount}
          disabled={busySlot !== null}
        >
          <Ionicons name="add-circle-outline" size={18} color="#2f7bff" />
          <Text style={s.addText}>Добавить аккаунт</Text>
        </Pressable>
      </ScrollView>
    </View>
  )
}

const s = StyleSheet.create({
  root: { flex: 1, backgroundColor: '#000' },

  title:    { color: '#fff', fontSize: 26, fontWeight: '700', marginBottom: 10 },
  subtitle: { color: '#71717a', fontSize: 14, lineHeight: 20, marginBottom: 24 },

  group: { backgroundColor: '#111114', borderRadius: 16, overflow: 'hidden' },
  row:   { flexDirection: 'row', alignItems: 'center' },
  rowBorder: { borderTopWidth: StyleSheet.hairlineWidth, borderTopColor: '#27272a' },
  rowMain: { flex: 1, flexDirection: 'row', alignItems: 'center', paddingHorizontal: 16, paddingVertical: 14, gap: 12 },
  rowText: { flex: 1 },
  rowName:        { color: '#fff', fontSize: 16, fontWeight: '600', marginBottom: 2 },
  rowFingerprint: { color: '#71717a', fontSize: 12, fontVariant: ['tabular-nums'] },
  removeBtn: { paddingHorizontal: 16, paddingVertical: 14 },

  addBtn: {
    flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 8,
    marginTop: 20, paddingVertical: 14,
    backgroundColor: '#111114', borderWidth: 1, borderColor: '#27272a', borderRadius: 14,
  },
  addBtnPressed: { opacity: 0.7 },
  addText: { color: '#2f7bff', fontSize: 15, fontWeight: '600' },

  btnOverlay: { position: 'absolute', zIndex: 10 },
  backBtn: {
    width: BTN_H, height: BTN_H, borderRadius: BTN_H / 2,
    alignItems: 'center', justifyContent: 'center',
  },
})
