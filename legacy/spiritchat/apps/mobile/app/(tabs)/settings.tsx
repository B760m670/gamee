import { useState } from 'react'
import {
  View, Text, Pressable, ScrollView, StyleSheet,
} from 'react-native'
import { GlassView } from 'expo-glass-effect'
import { Ionicons } from '@expo/vector-icons'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { useAuthStore, MAX_ACCOUNTS } from '../../store/auth'
import { Avatar } from '../../components/Avatar'
import { SettingsRow } from '../../components/SettingsRow'
import { useAvatarUpload } from '../../hooks/useAvatarUpload'
import { AvatarEditorModal } from '../../components/AvatarEditorModal'
import { QrCodeModal } from '../../components/QrCodeModal'

const AVATAR_SIZE = 100
const BTN_H       = 44

export default function SettingsScreen() {
  const insets = useSafeAreaInsets()
  const router  = useRouter()
  const { user, accounts, activeId, switchAccount, beginAddAccount } = useAuthStore(s => ({
    user:           s.user,
    accounts:       s.accounts,
    activeId:       s.activeId,
    switchAccount:  s.switchAccount,
    beginAddAccount: s.beginAddAccount,
  }))
  const otherAccounts = accounts.filter(a => a.id !== activeId)
  const canAddAccount = accounts.length < MAX_ACCOUNTS

  const { pickAndUpload, uploading, editorUri, handleEditorDone, handleEditorCancel } = useAvatarUpload()
  const [qrVisible, setQrVisible] = useState(false)

  const BTN_TOP = insets.top + 10

  return (
    <View style={s.root}>
      <ScrollView
        contentContainerStyle={{
          paddingTop:        insets.top + 14,
          paddingHorizontal: 16,
          paddingBottom:     insets.bottom + 40,
        }}
        showsVerticalScrollIndicator={false}
      >
        {/* Profile section */}
        <View style={s.profileSection}>
          <Avatar
            uri={user?.avatar_url ?? null}
            size={AVATAR_SIZE}
            username={user?.username ?? user?.display_name ?? '?'}
          />
          <View style={s.profileTextWrap}>
            <Text style={s.name}>{user?.display_name ?? ''}</Text>
            <Text style={s.subInfo}>
              {user?.username
                ? `${user.email}  •  @${user.username}`
                : (user?.email ?? '')}
            </Text>
          </View>
        </View>

        {/* Photo button — blue action row, same pattern as Telegram Settings */}
        <SettingsRow onPress={pickAndUpload} style={s.photoRow}>
          <View style={[s.iconWrap, { backgroundColor: '#2f7bff' }]}>
            <Ionicons name="camera" size={15} color="#fff" />
          </View>
          <Text style={s.photoLabel}>
            {uploading
              ? 'Загрузка...'
              : user?.avatar_url ? 'Изменить фото' : 'Выбрать фотографию'}
          </Text>
        </SettingsRow>

        {/* Accounts section */}
        {(otherAccounts.length > 0 || canAddAccount) && (
          <>
            <View style={s.rowGap} />
            <View style={s.accountGroup}>
              {otherAccounts.map((account, idx) => (
                <Pressable
                  key={account.id}
                  style={({ pressed }) => [
                    s.accountRow,
                    idx < otherAccounts.length - 1 || canAddAccount ? s.accountRowBorder : null,
                    pressed && s.accountRowPressed,
                  ]}
                  onPress={() => switchAccount(account.id)}
                >
                  <Avatar
                    uri={account.user.avatar_url ?? null}
                    size={34}
                    username={account.user.username ?? account.user.display_name ?? '?'}
                  />
                  <Text style={s.accountName} numberOfLines={1}>
                    {account.user.display_name}
                  </Text>
                  <Ionicons name="chevron-forward" size={16} color="rgba(255,255,255,0.3)" />
                </Pressable>
              ))}
              {canAddAccount && (
                <Pressable
                  style={({ pressed }) => [s.accountRow, pressed && s.accountRowPressed]}
                  onPress={() => { beginAddAccount(); router.push('/(auth)/login') }}
                >
                  <View style={s.addIcon}>
                    <Ionicons name="add" size={18} color="#fff" />
                  </View>
                  <Text style={s.addLabel}>Добавить аккаунт</Text>
                </Pressable>
              )}
            </View>
          </>
        )}

        <View style={s.rowGap} />

        {/* Rows */}
        <SettingsRow onPress={() => router.push('/settings/privacy')}>
          <View style={[s.iconWrap, { backgroundColor: '#8b5cf6' }]}>
            <Ionicons name="lock-closed" size={15} color="#fff" />
          </View>
          <Text style={s.rowLabel}>Конфиденциальность</Text>
          <Ionicons name="chevron-forward" size={16} color="rgba(255,255,255,0.3)" />
        </SettingsRow>
      </ScrollView>

      {/* QR button (left) */}
      <View style={[s.btnOverlay, { top: BTN_TOP, left: 16 }]}>
        <Pressable onPress={() => setQrVisible(true)}>
          <GlassView style={s.qrShape} glassEffectStyle="regular" isInteractive colorScheme="dark">
            <Ionicons name="qr-code" size={26} color="#fff" />
          </GlassView>
        </Pressable>
      </View>

      {/* Изм. button (right) */}
      <View style={[s.btnOverlay, { top: BTN_TOP, right: 16 }]}>
        <Pressable onPress={() => router.push('/settings/edit-profile')}>
          <GlassView style={s.pillShape} glassEffectStyle="regular" isInteractive colorScheme="dark">
            <Text style={s.btnLabel}>Изм.</Text>
          </GlassView>
        </Pressable>
      </View>

      <AvatarEditorModal
        uri={editorUri}
        onDone={handleEditorDone}
        onCancel={handleEditorCancel}
      />
      <QrCodeModal
        visible={qrVisible}
        onClose={() => setQrVisible(false)}
        user={user ?? null}
      />
    </View>
  )
}

const s = StyleSheet.create({
  root: { flex: 1, backgroundColor: '#000' },

  profileSection: { alignItems: 'center', paddingTop: 8, paddingBottom: 8 },
  profileTextWrap: { alignItems: 'center', marginTop: 14, minHeight: 72 },
  name: {
    color: '#fff', fontSize: 28,
    fontWeight: '500',
    textAlign: 'center',
  },
  subInfo: {
    color: '#8e8e93', fontSize: 15,
    marginTop: 4, textAlign: 'center', marginBottom: 28,
  },

  iconWrap: {
    width: 30, height: 30, borderRadius: 8,
    alignItems: 'center', justifyContent: 'center',
  },
  rowLabel:  { flex: 1, color: '#fff',     fontSize: 17 },
  photoRow:  { marginBottom: 0 },
  photoLabel: { flex: 1, color: '#2f7bff', fontSize: 17 },
  rowGap:    { height: 10 },

  accountGroup:       { backgroundColor: '#1c1c1e', borderRadius: 16, overflow: 'hidden' },
  accountRow:         { flexDirection: 'row', alignItems: 'center', paddingHorizontal: 14, paddingVertical: 10, gap: 12, minHeight: 50 },
  accountRowBorder:   { borderBottomWidth: StyleSheet.hairlineWidth, borderBottomColor: '#38383a' },
  accountRowPressed:  { backgroundColor: '#3a3a3c' },
  accountName:        { flex: 1, color: '#fff', fontSize: 17 },
  addIcon:            { width: 34, height: 34, borderRadius: 17, backgroundColor: '#2f7bff', alignItems: 'center', justifyContent: 'center' },
  addLabel:           { flex: 1, color: '#2f7bff', fontSize: 17 },

  btnOverlay: { position: 'absolute', zIndex: 10 },
  qrShape: {
    width: BTN_H, height: BTN_H, borderRadius: BTN_H / 2,
    alignItems: 'center', justifyContent: 'center',
  },
  pillShape: {
    height: BTN_H, paddingHorizontal: 12, borderRadius: BTN_H / 2,
    alignItems: 'center', justifyContent: 'center',
  },
  btnLabel: { color: '#fff', fontSize: 17, fontWeight: '500' },
})
