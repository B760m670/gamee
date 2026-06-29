import { useState, useEffect } from 'react'
import {
  View, Modal, Pressable, Text, ActivityIndicator,
  StyleSheet, Dimensions, Image as RNImage,
} from 'react-native'
import { GlassView } from 'expo-glass-effect'
import { Ionicons } from '@expo/vector-icons'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import Animated, {
  useSharedValue, useAnimatedStyle, withSpring,
} from 'react-native-reanimated'
import { GestureDetector, Gesture } from 'react-native-gesture-handler'
import Svg, { Path } from 'react-native-svg'
import * as ImageManipulator from 'expo-image-manipulator'

const { width: SCREEN_W } = Dimensions.get('window')
const CIRCLE_PAD    = 24
const CIRCLE_SIZE   = SCREEN_W - CIRCLE_PAD * 2
const CIRCLE_RADIUS = CIRCLE_SIZE / 2
const DISPLAY_W     = SCREEN_W
const BTN_H         = 44
const SPRING        = { damping: 30, stiffness: 300 }

type Props = {
  uri: string | null
  onDone: (uri: string) => void
  onCancel: () => void
}

export function AvatarEditorModal({ uri, onDone, onCancel }: Props) {
  const insets = useSafeAreaInsets()

  const [sourceUri, setSourceUri] = useState<string | null>(null)
  const [natSize,   setNatSize]   = useState({ w: 1, h: 1 })
  const [dispH,     setDispH]     = useState(SCREEN_W)
  const [cropH,     setCropH]     = useState(SCREEN_W)
  const [busy,      setBusy]      = useState(false)

  // Shared values — accessed in gesture worklets
  const tx      = useSharedValue(0);  const savedTx = useSharedValue(0)
  const ty      = useSharedValue(0);  const savedTy = useSharedValue(0)
  const sc      = useSharedValue(1);  const savedSc = useSharedValue(1)
  const minSc   = useSharedValue(1)
  const dispHSV = useSharedValue(SCREEN_W)  // mirror of dispH for worklets
  const isFlipped = useSharedValue(0)

  function loadUri(newUri: string) {
    RNImage.getSize(newUri, (w, h) => {
      const dH     = (h / w) * DISPLAY_W
      const initSc = Math.max(CIRCLE_SIZE / DISPLAY_W, CIRCLE_SIZE / dH)
      setNatSize({ w, h })
      setDispH(dH)
      dispHSV.value   = dH
      minSc.value     = initSc
      sc.value        = initSc;  savedSc.value = initSc
      tx.value        = 0;       savedTx.value = 0
      ty.value        = 0;       savedTy.value = 0
      setSourceUri(newUri)
    })
  }

  useEffect(() => {
    if (uri) { isFlipped.value = 0; setBusy(false); loadUri(uri) }
  }, [uri])

  // ── Gestures ──────────────────────────────────────────────────────────────
  const pan = Gesture.Pan()
    .onUpdate(e => {
      // Clamp so image always covers the circle
      const halfX = Math.max(0, (DISPLAY_W   * sc.value - CIRCLE_SIZE) / 2)
      const halfY = Math.max(0, (dispHSV.value * sc.value - CIRCLE_SIZE) / 2)
      tx.value = Math.max(-halfX, Math.min(halfX, savedTx.value + e.translationX))
      ty.value = Math.max(-halfY, Math.min(halfY, savedTy.value + e.translationY))
    })
    .onEnd(() => {
      savedTx.value = tx.value
      savedTy.value = ty.value
    })

  const pinch = Gesture.Pinch()
    .onUpdate(e => {
      sc.value = Math.max(savedSc.value * e.scale, minSc.value)
    })
    .onEnd(() => {
      // Snap back if somehow image no longer covers circle
      const halfX = Math.max(0, (DISPLAY_W    * sc.value - CIRCLE_SIZE) / 2)
      const halfY = Math.max(0, (dispHSV.value * sc.value - CIRCLE_SIZE) / 2)
      const clampedTx = Math.max(-halfX, Math.min(halfX, tx.value))
      const clampedTy = Math.max(-halfY, Math.min(halfY, ty.value))
      tx.value = withSpring(clampedTx, SPRING)
      ty.value = withSpring(clampedTy, SPRING)
      savedSc.value = sc.value
      savedTx.value = clampedTx
      savedTy.value = clampedTy
    })

  const gesture = Gesture.Simultaneous(pan, pinch)

  const animStyle = useAnimatedStyle(() => ({
    transform: [
      { translateX: tx.value },
      { translateY: ty.value },
      { scale: sc.value },
      { scaleX: isFlipped.value ? -1 : 1 },
    ],
  }))

  // ── Actions ───────────────────────────────────────────────────────────────
  async function handleRotate() {
    if (!sourceUri || busy) return
    setBusy(true)
    try {
      const r = await ImageManipulator.manipulateAsync(
        sourceUri, [{ rotate: 90 }],
        { format: ImageManipulator.SaveFormat.JPEG, compress: 1 },
      )
      loadUri(r.uri)
    } finally { setBusy(false) }
  }

  function handleFlip() { isFlipped.value = isFlipped.value ? 0 : 1 }

  async function handleDone() {
    if (!sourceUri || busy) return
    setBusy(true)
    try {
      const s  = savedSc.value
      const ox = savedTx.value
      const oy = savedTy.value

      const totalW = DISPLAY_W    * s
      const totalH = dispHSV.value * s

      const imgLeft  = (SCREEN_W - totalW) / 2 + ox
      const imgTop   = (cropH    - totalH) / 2 + oy
      const circLeft = (SCREEN_W - CIRCLE_SIZE) / 2
      const circTop  = (cropH    - CIRCLE_SIZE) / 2

      const cropDispX = (circLeft - imgLeft) / s
      const cropDispY = (circTop  - imgTop)  / s
      const cropDispS = CIRCLE_SIZE / s

      const scaleToNat = natSize.w / DISPLAY_W
      const cropX = Math.max(0, Math.round(cropDispX * scaleToNat))
      const cropY = Math.max(0, Math.round(cropDispY * scaleToNat))
      const cropW = Math.round(Math.min(cropDispS * scaleToNat, natSize.w - cropX))
      const cropH_ = Math.round(Math.min(cropDispS * scaleToNat, natSize.h - cropY))

      const actions: ImageManipulator.Action[] = []
      if (isFlipped.value) actions.push({ flip: ImageManipulator.FlipType.Horizontal })
      actions.push({ crop: { originX: cropX, originY: cropY, width: cropW, height: cropH_ } })

      const result = await ImageManipulator.manipulateAsync(
        sourceUri, actions,
        { format: ImageManipulator.SaveFormat.JPEG, compress: 1 },
      )
      onDone(result.uri)
    } catch { setBusy(false) }
  }

  // SVG overlay: rect with circular hole (evenodd fill rule)
  const cx = SCREEN_W / 2
  const cy = cropH / 2
  const r  = CIRCLE_RADIUS
  const overlayPath =
    `M 0 0 H ${SCREEN_W} V ${cropH} H 0 Z ` +
    `M ${cx - r} ${cy} a ${r} ${r} 0 1 0 ${2 * r} 0 a ${r} ${r} 0 1 0 ${-2 * r} 0`

  return (
    <Modal visible={!!uri} animationType="slide" presentationStyle="fullScreen">
      <View style={[s.root, { paddingTop: insets.top + 10, paddingBottom: insets.bottom + 16 }]}>

        {/* Cancel / Done */}
        <View style={s.topBar}>
          <Pressable onPress={onCancel}>
            <GlassView style={s.pill} glassEffectStyle="regular" isInteractive colorScheme="dark">
              <Text style={s.cancelLabel}>Отмена</Text>
            </GlassView>
          </Pressable>
          <Pressable onPress={handleDone} disabled={busy}>
            <GlassView style={s.pill} glassEffectStyle="regular" isInteractive colorScheme="dark">
              {busy
                ? <ActivityIndicator color="#fff" size="small" />
                : <Text style={s.doneLabel}>Готово</Text>}
            </GlassView>
          </Pressable>
        </View>

        {/* Crop area */}
        <View
          style={s.cropArea}
          onLayout={e => setCropH(e.nativeEvent.layout.height)}
        >
          {/* GestureDetector needs an explicit-sized child — wrap in absoluteFill View */}
          <GestureDetector gesture={gesture}>
            <View style={StyleSheet.absoluteFill}>
              <Animated.View style={[StyleSheet.absoluteFill, s.imgContainer, animStyle]}>
                {sourceUri ? (
                  <RNImage
                    source={{ uri: sourceUri }}
                    style={{ width: DISPLAY_W, height: dispH }}
                    resizeMode="cover"
                  />
                ) : null}
              </Animated.View>
            </View>
          </GestureDetector>

          {/* Dark overlay with circular hole */}
          <Svg
            width={SCREEN_W}
            height={cropH}
            style={StyleSheet.absoluteFill}
            pointerEvents="none"
          >
            <Path d={overlayPath} fill="rgba(0,0,0,0.62)" fillRule="evenodd" />
          </Svg>
        </View>

        {/* Toolbar */}
        <View style={s.toolbar}>
          <Pressable style={s.toolItem} onPress={handleRotate} disabled={busy}>
            <Ionicons name="refresh-outline" size={28} color={busy ? 'rgba(255,255,255,0.3)' : '#fff'} />
            <Text style={s.toolLabel}>Повернуть</Text>
          </Pressable>
          <Pressable style={s.toolItem} onPress={handleFlip}>
            <Ionicons name="swap-horizontal-outline" size={28} color="#fff" />
            <Text style={s.toolLabel}>Отразить</Text>
          </Pressable>
        </View>

      </View>
    </Modal>
  )
}

const s = StyleSheet.create({
  root:        { flex: 1, backgroundColor: '#000' },
  topBar:      { flexDirection: 'row', justifyContent: 'space-between', paddingHorizontal: 16 },
  pill:        { height: BTN_H, paddingHorizontal: 12, borderRadius: BTN_H / 2, alignItems: 'center', justifyContent: 'center', minWidth: 80 },
  cancelLabel: { color: '#fff', fontSize: 17, fontWeight: '500' },
  doneLabel:   { color: '#fff', fontSize: 17, fontWeight: '600' },
  cropArea:    { flex: 1, overflow: 'hidden' },
  imgContainer: { alignItems: 'center', justifyContent: 'center' },
  toolbar:     { flexDirection: 'row', justifyContent: 'center', gap: 56, paddingTop: 20, paddingBottom: 8 },
  toolItem:    { alignItems: 'center', gap: 8 },
  toolLabel:   { color: 'rgba(255,255,255,0.6)', fontSize: 12 },
})
