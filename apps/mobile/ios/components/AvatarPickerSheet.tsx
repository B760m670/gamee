import { ActionSheetIOS } from 'react-native'
import * as ImagePicker from 'expo-image-picker'

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
