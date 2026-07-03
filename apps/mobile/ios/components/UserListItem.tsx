import { memo } from 'react'
import { Pressable, View, Text, StyleSheet } from 'react-native'
import { PeerAvatar } from './PeerAvatar'
import type { FoundUser } from '../hooks/useUsernameSearch'

interface Props {
  user: FoundUser
  onPress: (user: FoundUser) => void
}

// A single compact row — this device has no way to know a stranger's
// display name until they choose to share it in conversation (there is no
// server-side profile to fetch), so @username plus their avatar (if this
// device has already fetched it — see store/peerAvatars.ts) is all a
// search result can show.
function UserListItemBase({ user, onPress }: Props) {
  return (
    <Pressable
      style={({ pressed }) => [s.row, pressed && s.rowPressed]}
      onPress={() => onPress(user)}
    >
      <PeerAvatar peerId={user.peerId} size={46} username={user.username} />
      <View style={s.text}>
        <Text style={s.title} numberOfLines={1}>{`@${user.username}`}</Text>
        <Text style={s.subtitle} numberOfLines={1}>{user.fingerprint}</Text>
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
  subtitle: { color: '#52525b', fontSize: 13, fontVariant: ['tabular-nums'] },
})
