import { requireNativeModule } from 'expo-modules-core'

const NativeCryptoCore = requireNativeModule('SpiritchatCryptoCore')

/**
 * This device's persistent identity fingerprint ("1234 5678 9012"). The
 * underlying identity/agreement keys and prekey store are generated once
 * and stored in the iOS Keychain (see IdentitySession.swift) — this value
 * is stable across app restarts, not regenerated on every call.
 */
export function fingerprint(): string {
  return NativeCryptoCore.fingerprint()
}

/** This device's public identity key, base64-encoded. */
export function publicKeyBase64(): string {
  return NativeCryptoCore.publicKeyBase64()
}
