import { memo } from 'react'
import { Pressable, View, Text, StyleSheet } from 'react-native'
import { Avatar } from './Avatar'
import type { PublicUser } from '../hooks/useUserSearch'

interface Props {
  user: PublicUser
  onPress: (user: PublicUser) => void
}

function UserListItemBase({ user, onPress }: Props) {
  const title = user.display_name?.trim() || (user.username ? `@${user.username}` : 'Без имени')
  const subtitle = user.username ? `@${user.username}` : null

  return (
    <Pressable
      style={({ pressed }) => [s.row, pressed && s.rowPressed]}
      onPress={() => onPress(user)}
    >
      <Avatar uri={user.avatar_url} size={46} username={user.username ?? user.display_name} />
      <View style={s.text}>
        <Text style={s.title} numberOfLines={1}>{title}</Text>
        {subtitle && title !== subtitle ? (
          <Text style={s.subtitle} numberOfLines={1}>{subtitle}</Text>
        ) : null}
      </View>
    </Pressable>
  )
}

export const UserListItem = memo(UserListItemBase)

const s = StyleSheet.create({
  row: {
    flexDirection: 'row', alignItems: 'center', gap: 12,
    paddingHorizontal: 16, paddingVertical: 8,
  },
  rowPressed: { backgroundColor: '#1a1a1e' },
  text:     { flex: 1, gap: 2 },
  title:    { color: '#fff', fontSize: 16, fontWeight: '600' },
  subtitle: { color: '#52525b', fontSize: 14 },
})
