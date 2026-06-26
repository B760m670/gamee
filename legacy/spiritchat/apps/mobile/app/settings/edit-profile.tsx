import { useState } from 'react'
import {
  View, Text, TextInput, Pressable,
  KeyboardAvoidingView, Platform, ActivityIndicator, ScrollView, StyleSheet,
} from 'react-native'
import { GlassView } from 'expo-glass-effect'
import { Ionicons } from '@expo/vector-icons'
import { useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { useAuthStore, type AppUser } from '../../store/auth'
import { Avatar } from '../../components/Avatar'
import { api } from '../../lib/api'
import { useAvatarUpload } from '../../hooks/useAvatarUpload'
import { AvatarEditorModal } from '../../components/AvatarEditorModal'

const AVATAR_SIZE = 100
const BTN_H       = 44

export default function EditProfileScreen() {
  const insets = useSafeAreaInsets()
  const router  = useRouter()
  const { user } = useAuthStore(s => ({ user: s.user }))
  const { pickAndUpload, uploading: uploadingPhoto, error: uploadError, editorUri, handleEditorDone, handleEditorCancel } = useAvatarUpload()

  const BTN_TOP = insets.top + 10

  const [editName, setEditName] = useState(user?.display_name ?? '')
  const [editBio,  setEditBio]  = useState(user?.bio ?? '')
  const [saving,   setSaving]   = useState(false)
  const [error,    setError]    = useState<string | null>(null)

  const displayError = error ?? uploadError

  async function handleSave() {
    if (saving) return
    const hasChanges = editName.trim() !== (user?.display_name ?? '') ||
                       editBio.trim()  !== (user?.bio ?? '')
    if (!hasChanges) { router.back(); return }
    if (!editName.trim()) return
    setError(null)
    setSaving(true)
    try {
      const res = await api.put<{ data: AppUser }>('/api/v1/users/me', {
        display_name: editName.trim(),
        bio: editBio.trim() || null,
      })
      useAuthStore.setState({ user: res.data })
      router.back()
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : 'Ошибка сохранения'
      setError(msg)
      setSaving(false)
    }
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
        {/* Avatar — same position as Settings screen for seamless fade transition */}
        <View style={s.profileSection}>
          <Pressable onPress={pickAndUpload} disabled={uploadingPhoto}>
            <Avatar
              uri={user?.avatar_url ?? null}
              size={AVATAR_SIZE}
              username={user?.username ?? user?.display_name ?? '?'}
            />
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
          <Pressable
            style={({ pressed }) => [s.fieldRow, s.usernameBtn, pressed && s.usernameBtnPressed]}
            onPress={() => router.push('/settings/username')}
          >
            <View style={s.usernameBtnLeft}>
              <View style={s.usernameIcon}>
                <Ionicons name="at" size={16} color="#fff" />
              </View>
              <View>
                <Text style={s.fieldLabel}>Имя пользователя</Text>
                <Text style={s.usernameValue}>
                  {user?.username ? `@${user.username}` : 'Не задано'}
                </Text>
              </View>
            </View>
            <Ionicons name="chevron-forward" size={16} color="rgba(255,255,255,0.3)" />
          </Pressable>
        </View>

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

        <Pressable
          onPress={async () => {
            await useAuthStore.getState().signOut()
            router.replace('/(auth)/login')
          }}
          style={s.signOutBtn}
        >
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
  profileTextWrap: { alignItems: 'center', marginTop: 14, minHeight: 72 },
  choosePhoto: { color: '#2f7bff', fontSize: 15, fontWeight: '500', textAlign: 'center' },

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

  usernameBtn:        { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', gap: 0 },
  usernameBtnPressed: { backgroundColor: '#1a1a1e' },
  usernameBtnLeft:    { flexDirection: 'row', alignItems: 'center', gap: 12 },
  usernameIcon: {
    width: 30, height: 30, borderRadius: 8,
    backgroundColor: '#3b82f6',
    alignItems: 'center', justifyContent: 'center',
  },
  usernameValue: { color: '#fff', fontSize: 15, marginTop: 2 },
  bioCount:    { color: '#3f3f46', fontSize: 12, textAlign: 'right', marginTop: 4, marginRight: 4 },
  errorBox:    { backgroundColor: 'rgba(127,29,29,0.4)', borderRadius: 12, borderWidth: 1, borderColor: '#b91c1c', padding: 12, marginBottom: 12 },
  errorText:   { color: '#f87171', fontSize: 14 },
  signOutBtn:  { marginTop: 36, alignItems: 'center', paddingVertical: 14 },
  signOutText: { color: '#ef4444', fontSize: 16, fontWeight: '500', textAlign: 'center' },
})
