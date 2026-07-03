import { useState } from 'react'
import {
  View, Text, TextInput, Pressable, Alert,
  KeyboardAvoidingView, Platform, ActivityIndicator, ScrollView, StyleSheet,
} from 'react-native'
import { GlassView } from 'expo-glass-effect'
import { Ionicons } from '@expo/vector-icons'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { useProfileStore } from '../../store/profile'
import { Avatar } from '../../components/Avatar'
import { useAvatarUpload } from '../../hooks/useAvatarUpload'
import { AvatarEditorModal } from '../../components/AvatarEditorModal'
import { navigateAfterAccountChange } from '../../utils/navigation'

const AVATAR_SIZE = 100
const BTN_H       = 44

export default function EditProfileScreen() {
  const insets = useSafeAreaInsets()
  const router  = useRouter()
  const { displayName, bio, fingerprint, avatarLocalPath, username, accounts, setDisplayName, setBio, signOut } = useProfileStore(s => ({
    displayName:     s.displayName,
    bio:             s.bio,
    fingerprint:     s.fingerprint,
    avatarLocalPath: s.avatarLocalPath,
    username:        s.username,
    accounts:        s.accounts,
    setDisplayName:  s.setDisplayName,
    setBio:          s.setBio,
    signOut:         s.signOut,
  }))
  const { pickAndUpload, uploading: uploadingPhoto, error: uploadError, editorUri, handleEditorDone, handleEditorCancel } = useAvatarUpload()

  const BTN_TOP = insets.top + 10

  const [editName, setEditName] = useState(displayName)
  const [editBio,  setEditBio]  = useState(bio)
  const [saving,   setSaving]   = useState(false)

  const displayError = uploadError

  async function handleSave() {
    if (saving) return
    setSaving(true)
    await Promise.all([
      setDisplayName(editName),
      setBio(editBio),
    ])
    setSaving(false)
    router.back()
  }

  function handleSignOutPress() {
    const otherAccountRemains = accounts.length > 1
    Alert.alert(
      'Выйти из аккаунта?',
      otherAccountRemains
        ? 'Вернуться обратно можно только по фразе восстановления этого аккаунта. Другой зарегистрированный на устройстве аккаунт станет активным.'
        : 'Здесь нет сервера — вернуться обратно можно только по фразе восстановления. Убедись, что сохранил её (Настройки → Фраза восстановления), иначе аккаунт будет утерян навсегда.',
      [
        { text: 'Отмена', style: 'cancel' },
        {
          text: 'Выйти',
          style: 'destructive',
          onPress: async () => {
            await signOut()
            navigateAfterAccountChange(router)
          },
        },
      ]
    )
  }

  return (
    <KeyboardAvoidingView
      style={s.root}
      behavior={Platform.OS === 'ios' ? 'padding' : 'height'}
    >
      {/* Corner buttons — mounted with screen, GlassView initialises correctly */}
      <View style={[s.btnOverlay, { top: BTN_TOP, left: 16 }]}>
        <Pressable onPress={() => router.back()}>
          <GlassView style={s.pillShape} glassEffectStyle="regular" isInteractive colorScheme="dark">
            <Text style={s.btnLabel}>Отмена</Text>
          </GlassView>
        </Pressable>
      </View>

      <View style={[s.btnOverlay, { top: BTN_TOP, right: 16 }]}>
        <Pressable onPress={handleSave}>
          <GlassView style={s.pillShape} glassEffectStyle="regular" isInteractive colorScheme="dark">
            {saving
              ? <ActivityIndicator color="#fff" size="small" />
              : <Text style={s.btnLabelDone}>Готово</Text>
            }
          </GlassView>
        </Pressable>
      </View>

      <ScrollView
        keyboardShouldPersistTaps="handled"
        contentContainerStyle={{
          paddingTop:        insets.top + 14,
          paddingHorizontal: 16,
          paddingBottom:     insets.bottom + 40,
        }}
        showsVerticalScrollIndicator={false}
      >
        <View style={s.profileSection}>
          <Pressable onPress={pickAndUpload} disabled={uploadingPhoto}>
            <Avatar uri={avatarLocalPath} size={AVATAR_SIZE} username={editName || '?'} />
            <View style={s.avatarOverlay}>
              {uploadingPhoto
                ? <ActivityIndicator color="#fff" size="large" />
                : <Ionicons name="camera" size={30} color="#fff" />
              }
            </View>
          </Pressable>

          <View style={s.profileTextWrap}>
            <Pressable onPress={pickAndUpload}>
              <Text style={s.choosePhoto}>Выбрать фотографию</Text>
            </Pressable>
          </View>
        </View>

        {displayError ? (
          <View style={s.errorBox}>
            <Text style={s.errorText}>{displayError}</Text>
          </View>
        ) : null}

        <View style={s.group}>
          <View style={[s.fieldRow, s.fieldBorder]}>
            <Text style={s.fieldLabel}>Имя</Text>
            <TextInput
              style={s.fieldInput}
              value={editName}
              onChangeText={setEditName}
              placeholder="Ваше имя"
              placeholderTextColor="#3f3f46"
              maxLength={50}
              returnKeyType="next"
            />
          </View>
          <View style={s.fieldRow}>
            <Text style={s.fieldLabel}>Идентификатор</Text>
            <Text style={s.fingerprintValue}>{fingerprint}</Text>
          </View>
        </View>
        <Text style={s.fingerprintHint}>
          Это криптографический отпечаток твоего устройства — сравни его с собеседником лично или по другому каналу, чтобы убедиться, что переписка не подменена. Он не редактируется и не зависит от имени.
        </Text>

        <Pressable
          style={({ pressed }) => [s.group, { marginTop: 12 }, s.recoveryRow, pressed && s.recoveryRowPressed]}
          onPress={() => router.push('/settings/username')}
        >
          <View style={s.fieldRow}>
            <Text style={s.fieldLabel}>Имя пользователя</Text>
            <Text style={s.usernameValue}>{username ? `@${username}` : 'Не задано'}</Text>
          </View>
        </Pressable>
        <Text style={s.fingerprintHint}>
          Необязательно — если задать, тебя можно будет найти по точному @имени. Публикуется в открытой P2P-сети без сервера, поэтому уникальность не гарантирована железно.
        </Text>

        <Pressable
          style={({ pressed }) => [s.group, { marginTop: 12 }, s.recoveryRow, pressed && s.recoveryRowPressed]}
          onPress={() => router.push('/settings/recovery-phrase')}
        >
          <View style={s.fieldRow}>
            <Text style={s.recoveryLabel}>Фраза восстановления</Text>
          </View>
        </Pressable>

        <Pressable
          style={({ pressed }) => [s.group, { marginTop: 12 }, s.recoveryRow, pressed && s.recoveryRowPressed]}
          onPress={() => router.push('/settings/accounts')}
        >
          <View style={s.fieldRow}>
            <Text style={s.fieldLabel}>Аккаунты</Text>
            <Text style={s.usernameValue}>{`${accounts.length} из 3`}</Text>
          </View>
        </Pressable>

        <View style={[s.group, { marginTop: 12 }]}>
          <View style={s.fieldRow}>
            <Text style={s.fieldLabel}>О себе</Text>
            <TextInput
              style={[s.fieldInput, { minHeight: 60, textAlignVertical: 'top' }]}
              value={editBio}
              onChangeText={setEditBio}
              placeholder="Несколько слов о себе..."
              placeholderTextColor="#3f3f46"
              multiline
              maxLength={200}
              scrollEnabled={false}
            />
          </View>
        </View>
        <Text style={s.bioCount}>{editBio.length}/200</Text>

        <Pressable onPress={handleSignOutPress} style={s.signOutBtn}>
          <Text style={s.signOutText}>Выйти из аккаунта</Text>
        </Pressable>
      </ScrollView>

      <AvatarEditorModal
        uri={editorUri}
        onDone={handleEditorDone}
        onCancel={handleEditorCancel}
      />
    </KeyboardAvoidingView>
  )
}

const s = StyleSheet.create({
  root: { flex: 1, backgroundColor: '#000' },

  profileSection: { alignItems: 'center', paddingTop: 8, paddingBottom: 8 },
  avatarOverlay: {
    position: 'absolute',
    width: AVATAR_SIZE, height: AVATAR_SIZE,
    borderRadius: AVATAR_SIZE / 2,
    backgroundColor: 'rgba(0,0,0,0.45)',
    alignItems: 'center', justifyContent: 'center',
  },
  profileTextWrap: { alignItems: 'center', marginTop: 14, minHeight: 20 },
  choosePhoto: { color: '#2f7bff', fontSize: 15, fontWeight: '500', textAlign: 'center' },

  errorBox:  { backgroundColor: 'rgba(127,29,29,0.4)', borderRadius: 12, borderWidth: 1, borderColor: '#b91c1c', padding: 12, marginBottom: 12 },
  errorText: { color: '#f87171', fontSize: 14 },

  btnOverlay: { position: 'absolute', zIndex: 10 },
  pillShape: {
    height: BTN_H, paddingHorizontal: 12, borderRadius: BTN_H / 2,
    alignItems: 'center', justifyContent: 'center',
  },
  btnLabel:     { color: '#fff', fontSize: 17, fontWeight: '500' },
  btnLabelDone: { color: '#fff', fontSize: 17, fontWeight: '600' },

  group:       { backgroundColor: '#111114', borderRadius: 16, overflow: 'hidden' },
  fieldRow:    { paddingHorizontal: 16, paddingVertical: 12, gap: 4 },
  fieldBorder: { borderBottomWidth: StyleSheet.hairlineWidth, borderBottomColor: '#27272a' },
  fieldLabel:  { color: '#a1a1aa', fontSize: 12, fontWeight: '500' },
  fieldInput:  { color: '#fff', fontSize: 16, paddingVertical: 2 },

  fingerprintValue: { color: '#fff', fontSize: 15, fontVariant: ['tabular-nums'], marginTop: 2 },
  fingerprintHint:  { color: '#52525b', fontSize: 12, lineHeight: 17, marginTop: 8, marginHorizontal: 4 },

  recoveryRow:        {},
  recoveryRowPressed: { backgroundColor: '#1a1a1e' },
  recoveryLabel:       { color: '#2f7bff', fontSize: 16, fontWeight: '500' },
  usernameValue:       { color: '#fff', fontSize: 15, marginTop: 2 },

  bioCount: { color: '#3f3f46', fontSize: 12, textAlign: 'right', marginTop: 4, marginRight: 4 },

  signOutBtn:  { marginTop: 36, alignItems: 'center', paddingVertical: 14 },
  signOutText: { color: '#ef4444', fontSize: 16, fontWeight: '500', textAlign: 'center' },
})
