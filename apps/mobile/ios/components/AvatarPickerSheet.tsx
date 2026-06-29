import { ActionSheetIOS } from 'react-native'
import * as ImagePicker from 'expo-image-picker'
import * as ImageManipulator from 'expo-image-manipulator'

const WORKER_URL = 'https://mysocialapp-upload.metrvsapke.workers.dev'

// Telegram uses 640×640 JPEG at 60% quality for avatar uploads
const AVATAR_SIZE    = 640
const AVATAR_QUALITY = 0.6

export type AvatarPickerResult =
  | { type: 'image'; uri: string; mimeType: string }
  | { type: 'remove' }
  | { type: 'cancel' }

export function showAvatarPickerSheet(hasPhoto: boolean): Promise<AvatarPickerResult> {
  return new Promise((resolve) => {
    const actions: Array<{ label: string; key: 'library' | 'remove' | 'cancel' }> = [
      { label: 'Выбрать из галереи', key: 'library' },
      ...(hasPhoto ? [{ label: 'Удалить фото', key: 'remove' as const }] : []),
      { label: 'Отмена',             key: 'cancel'  },
    ]

    ActionSheetIOS.showActionSheetWithOptions(
      {
        options:              actions.map(a => a.label),
        cancelButtonIndex:    actions.length - 1,
        destructiveButtonIndex: hasPhoto ? 1 : undefined,
      },
      async (idx) => {
        const { key } = actions[idx]

        if (key === 'cancel') { resolve({ type: 'cancel' }); return }
        if (key === 'remove') { resolve({ type: 'remove' }); return }

        const result = await ImagePicker.launchImageLibraryAsync({
          mediaTypes: 'images',
          quality: 1,
        })

        if (result.canceled) {
          resolve({ type: 'cancel' })
        } else {
          const asset = result.assets[0]
          resolve({ type: 'image', uri: asset.uri, mimeType: asset.mimeType ?? 'image/jpeg' })
        }
      },
    )
  })
}

// Uploads image to Cloudflare R2 via Worker. Returns the public URL.
export async function uploadAvatar(token: string, uri: string): Promise<string> {
  const processed = await ImageManipulator.manipulateAsync(
    uri,
    [{ resize: { width: AVATAR_SIZE } }],
    { compress: AVATAR_QUALITY, format: ImageManipulator.SaveFormat.JPEG },
  )

  const body = new FormData()
  body.append('file', { uri: processed.uri, type: 'image/jpeg', name: 'avatar.jpg' } as unknown as Blob)

  const res = await fetch(WORKER_URL, {
    method:  'POST',
    headers: { Authorization: `Bearer ${token}` },
    body,
  })

  if (!res.ok) {
    const msg = await res.text()
    throw new Error(msg || 'Upload failed')
  }

  const { url } = await res.json() as { url: string }
  return url
}
