import { useEffect } from 'react'
import { Avatar } from './Avatar'
import { usePeerAvatarStore, requestPeerAvatar } from '../store/peerAvatars'

interface Props {
  peerId: string
  size?: number
  username?: string
}

// Kicks off fetching `peerId`'s current avatar (see store/peerAvatars.ts)
// the first time it's rendered, and re-renders itself once the fetch
// resolves — every place a peer's avatar is shown (conversation list,
// search results, chat header, profile) can just drop this in instead of
// separately wiring up the fetch.
export function PeerAvatar({ peerId, size = 40, username }: Props) {
  const localPath = usePeerAvatarStore(s => s.localPaths[peerId])

  useEffect(() => {
    requestPeerAvatar(peerId)
  }, [peerId])

  return <Avatar uri={localPath ?? null} size={size} username={username} />
}
