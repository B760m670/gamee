import { useState } from 'react'
import {
  View, Text, Pressable, ScrollView, StyleSheet,
} from 'react-native'
import { GlassView } from 'expo-glass-effect'
import { Ionicons } from '@expo/vector-icons'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { useProfileStore } from '../../store/profile'
import { Avatar } from '../../components/Avatar'
import { SettingsRow } from '../../components/SettingsRow'
import { QrCodeModal } from '../../components/QrCodeModal'
import { useAvatarUpload } from '../../hooks/useAvatarUpload'
import { AvatarEditorModal } from '../../components/AvatarEditorModal'

const AVATAR_SIZE = 100
const BTN_H       = 44

export default function SettingsScreen() {
  const insets = useSafeAreaInsets()
  const router  = useRouter()
  const { displayName, fingerprint, avatarLocalPath } = useProfileStore(s => ({
    displayName:     s.displayName,
    fingerprint:     s.fingerprint,
    avatarLocalPath: s.avatarLocalPath,
  }))
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
          <Avatar uri={avatarLocalPath} size={AVATAR_SIZE} username={displayName || '?'} />
          <View style={s.profileTextWrap}>
            <Text style={s.name}>{displayName || 'Без имени'}</Text>
            <Text style={s.subInfo}>{fingerprint}</Text>
          </View>
        </View>

        {/* Photo button — blue action row, same pattern as Telegram Settings */}
        <SettingsRow onPress={pickAndUpload} style={s.photoRow}>
          <View style={[s.iconWrap, { backgroundColor: '#2f7bff' }]}>
            <Ionicons name="camera" size={15} color="#fff" />
          </View>
          <Text style={s.photoLabel}>
            {uploading
              ? 'Сохранение...'
              : avatarLocalPath ? 'Изменить фото' : 'Выбрать фотографию'}
          </Text>
        </SettingsRow>

        <View style={s.rowGap} />

        {/* Rows */}
        <SettingsRow onPress={() => router.push('/settings/privacy')}>
          <View style={[s.iconWrap, { backgroundColor: '#8b5cf6' }]}>
            <Ionicons name="lock-closed" size={15} color="#fff" />
          </View>
          <Text style={s.rowLabel}>Конфиденциальность</Text>
          <Ionicons name="chevron-forward" size={16} color="rgba(255,255,255,0.3)" />
        </SettingsRow>

        <SettingsRow onPress={() => router.push('/settings/network')}>
          <View style={[s.iconWrap, { backgroundColor: '#0ea5e9' }]}>
            <Ionicons name="globe-outline" size={15} color="#fff" />
          </View>
          <Text style={s.rowLabel}>Сеть</Text>
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

      <QrCodeModal
        visible={qrVisible}
        onClose={() => setQrVisible(false)}
      />
      <AvatarEditorModal
        uri={editorUri}
        onDone={handleEditorDone}
        onCancel={handleEditorCancel}
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
