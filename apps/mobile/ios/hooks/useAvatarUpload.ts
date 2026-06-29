import { useState } from 'react'
import { useAuthStore, type AppUser } from '../store/auth'
import { api } from '../lib/api'
import { showAvatarPickerSheet, uploadAvatar } from '../components/AvatarPickerSheet'

export function useAvatarUpload() {
  const { token } = useAuthStore(s => ({ token: s.token }))
  const user = useAuthStore(s => s.user)

  const [uploading,  setUploading]  = useState(false)
  const [error,      setError]      = useState<string | null>(null)
  const [editorUri,  setEditorUri]  = useState<string | null>(null)

  async function pickAndUpload() {
    const result = await showAvatarPickerSheet(!!(user?.avatar_url))

    if (result.type === 'image') {
      setEditorUri(result.uri)

    } else if (result.type === 'remove') {
      if (!token) return
      setUploading(true)
      setError(null)
      try {
        const res = await api.put<{ data: AppUser }>('/api/v1/users/me', { avatar_url: null })
        useAuthStore.setState({ user: res.data })
      } catch {
        setError('Не удалось удалить фото')
      } finally {
        setUploading(false)
      }
    }
  }

  async function handleEditorDone(processedUri: string) {
    if (!token) return
    setEditorUri(null)
    setUploading(true)
    setError(null)
    try {
      const publicUrl = await uploadAvatar(token, processedUri)
      const res = await api.put<{ data: AppUser }>('/api/v1/users/me', { avatar_url: publicUrl })
      useAuthStore.setState({ user: res.data })
    } catch {
      setError('Не удалось загрузить фото')
    } finally {
      setUploading(false)
    }
  }

  function handleEditorCancel() {
    setEditorUri(null)
  }

  return {
    pickAndUpload,
    uploading,
    error,
    editorUri,
    handleEditorDone,
    handleEditorCancel,
    clearError: () => setError(null),
  }
}
