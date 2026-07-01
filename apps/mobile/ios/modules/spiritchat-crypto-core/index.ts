import { requireNativeModule } from 'expo-modules-core'

const NativeCryptoCore = requireNativeModule('SpiritchatCryptoCore')

/**
 * Generates a throwaway identity through the native Rust crypto core
 * (packages/crypto-core-ffi) and returns its fingerprint. Proves the
 * XCFramework loaded and the UniFFI bridge works end to end on-device —
 * not part of the real identity/session lifecycle, which lands as its own
 * feature once the data layer moves off its current UI-shell stub.
 */
export function selfCheck(): string {
  return NativeCryptoCore.selfCheck()
}
