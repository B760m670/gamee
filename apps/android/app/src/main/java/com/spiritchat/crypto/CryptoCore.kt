package com.spiritchat.crypto

import uniffi.spiritchat_crypto_core_ffi.FfiIdentity

/**
 * Entry point into the shared Rust crypto core (packages/crypto-core-ffi),
 * loaded through the UniFFI/JNA bindings generated at build time into
 * `uniffi.spiritchat_crypto_core_ffi`. [selfCheck] proves the native
 * library actually loaded and a full identity/fingerprint round trip works
 * on-device — the real identity/session lifecycle (persisted keys,
 * handshake, ratchet) lands as its own feature once the data layer moves
 * off its current UI-shell stub.
 */
object CryptoCore {
    fun selfCheck(): String {
        val identity = FfiIdentity.generate()
        return identity.fingerprint()
    }
}
