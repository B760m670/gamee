import { memo, useState } from 'react'
import { StyleSheet } from 'react-native'
import { Image } from 'expo-image'
import { useVideoPlayer, VideoView } from 'expo-video'
import type { ChatMedia } from '../store/chat'

interface Props {
  media: ChatMedia
}

const MAX_W = 240
const MAX_H = 320

/** Inline photo bubble, sized to the image's own aspect ratio once loaded. */
function ImageThumb({ media }: { media: ChatMedia }) {
  const [ratio, setRatio] = useState(1)
  const width = MAX_W
  const height = Math.min(MAX_H, Math.round(MAX_W / ratio))
  return (
    <Image
      source={{ uri: media.localPath }}
      style={[s.media, { width, height }]}
      contentFit="cover"
      transition={150}
      onLoad={e => {
        const { width: w, height: h } = e.source
        if (w > 0 && h > 0) setRatio(w / h)
      }}
    />
  )
}

/** Inline video bubble with native playback controls. */
function VideoThumb({ media }: { media: ChatMedia }) {
  const player = useVideoPlayer(media.localPath, p => {
    p.loop = false
  })
  return (
    <VideoView
      player={player}
      style={[s.media, { width: MAX_W, height: Math.round(MAX_W * 0.66) }]}
      contentFit="cover"
      nativeControls
    />
  )
}

/**
 * Renders a downloaded photo or video attachment inside a message bubble.
 * The bytes arrived E2E-encrypted over content-addressed blobs and were
 * decrypted to `localPath` on disk — here they're just a local file.
 */
function MediaThumbBase({ media }: Props) {
  if (media.mime.startsWith('video/')) return <VideoThumb media={media} />
  return <ImageThumb media={media} />
}

export const MediaThumb = memo(MediaThumbBase)

const s = StyleSheet.create({
  media: { borderRadius: 12, backgroundColor: '#000' },
})
