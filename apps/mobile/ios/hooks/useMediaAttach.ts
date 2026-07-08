import { useCallback } from 'react'
import { Alert } from 'react-native'
import * as ImagePicker from 'expo-image-picker'
import { ImageManipulator, SaveFormat } from 'expo-image-manipulator'

/** What a picked attachment resolves to, ready for `sendMedia`. */
export interface PickedMedia {
  fileUri: string
  mime: string
  filename: string | null
  durationMs: number | null
}

/**
 * Opens the photo/video library picker and normalises the result for the
 * media send path — shared by the 1:1 and group chat screens so both encrypt
 * and send attachments identically. Photos are downscaled/re-encoded so a
 * single image doesn't explode into hundreds of 64 KiB encrypted blobs.
 */
export function useMediaAttach(onPicked: (media: PickedMedia) => void) {
  return useCallback(async () => {
    try {
      const perm = await ImagePicker.requestMediaLibraryPermissionsAsync()
      if (!perm.granted) return
      const res = await ImagePicker.launchImageLibraryAsync({
        mediaTypes: ['images', 'videos'],
        quality: 1,
        videoMaxDuration: 120,
      })
      if (res.canceled || res.assets.length === 0) return
      const asset = res.assets[0]

      if (asset.type === 'video') {
        onPicked({
          fileUri: asset.uri,
          mime: asset.mimeType ?? 'video/mp4',
          filename: asset.fileName ?? null,
          durationMs: asset.duration != null ? Math.round(asset.duration) : null,
        })
        return
      }

      // A long edge of 1600px at JPEG q0.8 is plenty for a chat and keeps the
      // encrypted chunk count modest.
      const ref = await ImageManipulator.manipulate(asset.uri)
        .resize({ width: Math.min(asset.width || 1600, 1600) })
        .renderAsync()
      const out = await ref.saveAsync({ compress: 0.8, format: SaveFormat.JPEG })
      onPicked({ fileUri: out.uri, mime: 'image/jpeg', filename: asset.fileName ?? null, durationMs: null })
    } catch (e) {
      Alert.alert('Не удалось прикрепить файл', String(e))
    }
  }, [onPicked])
}
