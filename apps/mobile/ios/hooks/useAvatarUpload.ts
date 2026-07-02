import { useState } from 'react'
import * as ImageManipulator from 'expo-image-manipulator'
import { useProfileStore } from '../store/profile'
import { showAvatarPickerSheet } from '../components/AvatarPickerSheet'

// Telegram uses 640×640 JPEG at 60% quality for avatar uploads — same
// target here, just written straight to BlobStore instead of a server.
const AVATAR_SIZE    = 640
const AVATAR_QUALITY = 0.6

export function useAvatarUpload() {
  const hasPhoto        = useProfileStore(s => s.avatarLocalPath !== null)
  const setAvatarFromFile = useProfileStore(s => s.setAvatarFromFile)
  const clearAvatar       = useProfileStore(s => s.clearAvatar)

  const [uploading,  setUploading]  = useState(false)
  const [error,      setError]      = useState<string | null>(null)
  const [editorUri,  setEditorUri]  = useState<string | null>(null)

  async function pickAndUpload() {
    const result = await showAvatarPickerSheet(hasPhoto)

    if (result.type === 'image') {
      setEditorUri(result.uri)

    } else if (result.type === 'remove') {
      setUploading(true)
      setError(null)
      try {
        await clearAvatar()
      } catch {
        setError('Не удалось удалить фото')
      } finally {
        setUploading(false)
      }
    }
  }

  async function handleEditorDone(croppedUri: string) {
    setEditorUri(null)
    setUploading(true)
    setError(null)
    try {
      const processed = await ImageManipulator.manipulateAsync(
        croppedUri,
        [{ resize: { width: AVATAR_SIZE } }],
        { compress: AVATAR_QUALITY, format: ImageManipulator.SaveFormat.JPEG },
      )
      await setAvatarFromFile(processed.uri)
    } catch {
      setError('Не удалось сохранить фото')
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
