import { View, Text } from 'react-native'
import { Image } from 'expo-image'

interface AvatarProps {
  uri: string | null | undefined
  size?: number
  username?: string
}

const blurhash =
  '|rF?hV%2WCj[ayj[a|j[az_NaeWBj@ayfRayfQfQM{M|azj[azf6fQfQfQIpWXofj[ayj[j[fQayWCoeoeaya}j[ayfQa{oLj?j[WVj[ayayj[fQoff7azayj[ayj[j[ayofayayayj[fQj[ayayj[ayfjj[j[ayjuayj['

export function Avatar({ uri, size = 40, username }: AvatarProps) {
  const initials = username
    ? username.slice(0, 2).toUpperCase()
    : '?'

  if (!uri) {
    return (
      <View
        style={{
          width: size,
          height: size,
          borderRadius: size / 2,
          backgroundColor: '#27272a',
          alignItems: 'center',
          justifyContent: 'center',
        }}
      >
        <Text
          style={{
            color: '#a1a1aa',
            fontSize: size * 0.35,
            fontWeight: '600',
          }}
        >
          {initials}
        </Text>
      </View>
    )
  }

  return (
    <Image
      source={{ uri }}
      style={{
        width: size,
        height: size,
        borderRadius: size / 2,
        backgroundColor: '#27272a',
      }}
      placeholder={blurhash}
      contentFit="cover"
      transition={200}
    />
  )
}
