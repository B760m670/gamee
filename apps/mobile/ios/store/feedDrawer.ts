import { Animated, Dimensions } from 'react-native'

export const DRAWER_WIDTH = 265
export const SCREEN_WIDTH = Dimensions.get('window').width

export const drawerTranslate     = new Animated.Value(0)
export const shortVideoTranslate = new Animated.Value(0)  // 0=Posts, SCREEN_WIDTH=Shorts visible
export const searchTranslate     = new Animated.Value(0)  // 0=hidden, SCREEN_WIDTH=visible

// ─── Home panel + tab bar ─────────────────────────────────────────────────────
// Only moves for the Drawer. Shorts and Search are content-level / overlay panels.
export const homeTranslateX = drawerTranslate

// Feed drawer background zoom
export const drawerScale = drawerTranslate.interpolate({
  inputRange: [0, DRAWER_WIDTH],
  outputRange: [0.92, 1],
  extrapolate: 'clamp',
})

// ─── Content pager (Posts ↔ Short Videos) ────────────────────────────────────
// Posts slides left as Short Videos comes in from right — shared header stays fixed above.
export const postsContentX = shortVideoTranslate.interpolate({
  inputRange: [0, SCREEN_WIDTH],
  outputRange: [0, -SCREEN_WIDTH],
  extrapolate: 'clamp',
})
export const shortsContentX = shortVideoTranslate.interpolate({
  inputRange: [0, SCREEN_WIDTH],
  outputRange: [SCREEN_WIDTH, 0],
  extrapolate: 'clamp',
})

// ─── Search panel (full-screen overlay in _layout.tsx) ────────────────────────
export const searchPanelX = searchTranslate.interpolate({
  inputRange: [0, SCREEN_WIDTH],
  outputRange: [SCREEN_WIDTH, 0],
  extrapolate: 'clamp',
})

// ─── Shared state refs ────────────────────────────────────────────────────────
export const isSearchOpenRef     = { current: false }
export const isShortVideoOpenRef = { current: false }

// ─── Animation helpers ────────────────────────────────────────────────────────
const SPRING = { damping: 28, stiffness: 190, useNativeDriver: true } as const

export function openDrawerAnim(onDone?: () => void) {
  Animated.spring(drawerTranslate, { toValue: DRAWER_WIDTH, ...SPRING }).start(onDone as any)
}
export function closeDrawerAnim(onDone?: () => void) {
  Animated.spring(drawerTranslate, { toValue: 0, ...SPRING }).start(onDone as any)
}

export function openShortVideoAnim(onDone?: () => void) {
  Animated.spring(shortVideoTranslate, { toValue: SCREEN_WIDTH, ...SPRING }).start(onDone as any)
}
export function closeShortVideoAnim(onDone?: () => void) {
  Animated.spring(shortVideoTranslate, { toValue: 0, ...SPRING }).start(onDone as any)
}

export function openSearchAnim(onDone?: () => void) {
  Animated.spring(searchTranslate, { toValue: SCREEN_WIDTH, ...SPRING }).start(onDone as any)
}
export function closeSearchAnim(onDone?: () => void) {
  Animated.spring(searchTranslate, { toValue: 0, ...SPRING }).start(onDone as any)
}
