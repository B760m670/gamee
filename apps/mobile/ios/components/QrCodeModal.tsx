import { useState, useRef, useEffect } from 'react'
import {
  View, Modal, Pressable, Text, Share, StyleSheet, Dimensions, Alert,
} from 'react-native'
import QRCode from 'react-native-qrcode-svg'
import { CameraView, useCameraPermissions, type BarcodeScanningResult } from 'expo-camera'
import { GlassView } from 'expo-glass-effect'
import { Ionicons } from '@expo/vector-icons'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import Svg, { Path } from 'react-native-svg'
import { Avatar } from './Avatar'
import type { AppUser } from '../store/auth'

const { width: SCREEN_W, height: SCREEN_H } = Dimensions.get('window')
const QR_SIZE  = Math.round(SCREEN_W * 0.58)
const BTN_H    = 44

// Square scan frame — vertically centred, shifted up slightly for the cancel button
const FRAME_SIZE = Math.round(SCREEN_W * 0.65)
const FRAME_X    = (SCREEN_W - FRAME_SIZE) / 2
const FRAME_Y    = (SCREEN_H - FRAME_SIZE) / 2 - 50

// Dark overlay with a square transparent hole (evenodd fill rule)
const scanPath = [
  `M 0 0 H ${SCREEN_W} V ${SCREEN_H} H 0 Z`,
  `M ${FRAME_X} ${FRAME_Y} H ${FRAME_X + FRAME_SIZE} V ${FRAME_Y + FRAME_SIZE} H ${FRAME_X} Z`,
].join(' ')

// Deep link for the user's profile; replace base with a real domain once available
function profileLink(user: AppUser) {
  const handle = user.username ?? user.id
  return `mysocialapp://u/${handle}`
}

type Props = {
  visible: boolean
  onClose: () => void
  user: AppUser | null
}

export function QrCodeModal({ visible, onClose, user }: Props) {
  const insets = useSafeAreaInsets()
  const [permission, requestPermission] = useCameraPermissions()
  const [scanning, setScanning] = useState(false)
  const didScan = useRef(false)

  useEffect(() => {
    if (!visible) setScanning(false)
  }, [visible])

  async function handleShare() {
    if (!user) return
    try { await Share.share({ message: profileLink(user) }) } catch {}
  }

  async function handleScan() {
    if (!permission?.granted) {
      const { granted } = await requestPermission()
      if (!granted) return
    }
    didScan.current = false
    setScanning(true)
  }

  function handleBarcodeScanned({ data }: BarcodeScanningResult) {
    if (didScan.current) return
    didScan.current = true
    setScanning(false)
    // TODO: parse mysocialapp://u/<username> and navigate to that profile
    Alert.alert('QR отсканирован', data)
  }

  const link = user ? profileLink(user) : 'mysocialapp://'

  return (
    <Modal visible={visible} animationType="slide" onRequestClose={onClose}>
      <View style={[s.root, { paddingTop: insets.top + 10, paddingBottom: insets.bottom + 16 }]}>

        {scanning ? (
          <>
            <CameraView
              style={StyleSheet.absoluteFill}
              barcodeScannerSettings={{ barcodeTypes: ['qr'] }}
              onBarcodeScanned={handleBarcodeScanned}
            />
            <Svg
              width={SCREEN_W}
              height={SCREEN_H}
              style={StyleSheet.absoluteFill}
              pointerEvents="none"
            >
              <Path d={scanPath} fill="rgba(0,0,0,0.6)" fillRule="evenodd" />
            </Svg>
            <View style={s.topBar}>
              <Pressable onPress={() => setScanning(false)}>
                <GlassView style={s.pill} glassEffectStyle="regular" isInteractive colorScheme="dark">
                  <Text style={s.pillLabel}>Отмена</Text>
                </GlassView>
              </Pressable>
            </View>
            <Text style={[s.scanHint, { top: FRAME_Y + FRAME_SIZE + 20 }]}>
              Наведите на QR‑код
            </Text>
          </>
        ) : (
          <>
            <View style={s.topBar}>
              <Pressable onPress={onClose}>
                <GlassView style={s.pill} glassEffectStyle="regular" isInteractive colorScheme="dark">
                  <Text style={s.pillLabel}>Закрыть</Text>
                </GlassView>
              </Pressable>
            </View>

            <View style={s.content}>
              <Avatar
                uri={user?.avatar_url ?? null}
                size={80}
                username={user?.username ?? user?.display_name ?? '?'}
              />
              <Text style={s.name}>{user?.display_name ?? ''}</Text>
              {user?.username ? (
                <Text style={s.handle}>@{user.username}</Text>
              ) : null}

              <View style={s.qrCard}>
                <QRCode
                  value={link}
                  size={QR_SIZE}
                  backgroundColor="white"
                  color="black"
                />
              </View>
            </View>

            <View style={s.toolbar}>
              <Pressable style={s.toolItem} onPress={handleScan}>
                <GlassView style={s.toolBtn} glassEffectStyle="regular" isInteractive colorScheme="dark">
                  <Ionicons name="scan-outline" size={26} color="#fff" />
                </GlassView>
                <Text style={s.toolLabel}>Сканировать</Text>
              </Pressable>
              <Pressable style={s.toolItem} onPress={handleShare}>
                <GlassView style={s.toolBtn} glassEffectStyle="regular" isInteractive colorScheme="dark">
                  <Ionicons name="share-outline" size={26} color="#fff" />
                </GlassView>
                <Text style={s.toolLabel}>Поделиться</Text>
              </Pressable>
            </View>
          </>
        )}

      </View>
    </Modal>
  )
}

const s = StyleSheet.create({
  root:    { flex: 1, backgroundColor: '#111114' },
  topBar:  { paddingHorizontal: 16 },
  pill:    {
    height: BTN_H, paddingHorizontal: 16, borderRadius: BTN_H / 2,
    alignItems: 'center', justifyContent: 'center', alignSelf: 'flex-start',
  },
  pillLabel: { color: '#fff', fontSize: 17, fontWeight: '500' },

  content:  { flex: 1, alignItems: 'center', justifyContent: 'center', gap: 8 },
  name:     { color: '#fff', fontSize: 22, fontWeight: '600', marginTop: 12 },
  handle:   { color: 'rgba(255,255,255,0.5)', fontSize: 15 },

  qrCard:   { marginTop: 24, backgroundColor: '#fff', borderRadius: 20, padding: 16 },

  toolbar:   { flexDirection: 'row', justifyContent: 'center', gap: 56, paddingTop: 16, paddingBottom: 8 },
  toolItem:  { alignItems: 'center', gap: 8 },
  toolBtn:   { width: 56, height: 56, borderRadius: 28, alignItems: 'center', justifyContent: 'center' },
  toolLabel: { color: 'rgba(255,255,255,0.6)', fontSize: 12 },

  scanHint:  { position: 'absolute', left: 0, right: 0, textAlign: 'center', color: '#fff', fontSize: 15, fontWeight: '500' },
})
