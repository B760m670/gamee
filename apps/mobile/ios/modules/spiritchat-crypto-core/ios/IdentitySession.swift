import Foundation

/// The device's local account: a long-term signing identity, a separate
/// X3DH agreement key, and a prekey store — generated once and persisted
/// in the Keychain from then on, so a contact card scanned today still
/// resolves to the same account after every future app restart.
final class IdentitySession {
  static let shared = IdentitySession()

  private static let identityAccount = "identity-secret"
  private static let agreementAccount = "agreement-secret"
  private static let prekeysAccount = "prekey-store"

  /// One-time prekeys generated for a brand-new account. Replenishing
  /// them as they're consumed depends on the transport (there is no
  /// server or relay to fetch a fresh bundle from yet), so this is only
  /// enough to make X3DH work until that's designed.
  private static let initialOneTimePrekeyCount: UInt32 = 20

  let identity: FfiIdentity
  let agreement: FfiAgreementKey
  let prekeys: FfiPrekeyStore

  private init() {
    if let loaded = Self.loadExisting() {
      (identity, agreement, prekeys) = loaded
      return
    }

    let newIdentity = FfiIdentity.generate()
    let newAgreement = FfiAgreementKey.generate()
    let newPrekeys = FfiPrekeyStore.generate(
      identity: newIdentity,
      oneTimeCount: Self.initialOneTimePrekeyCount
    )

    do {
      try KeychainStore.save(newIdentity.secretBytes(), account: Self.identityAccount)
      try KeychainStore.save(newAgreement.secretBytes(), account: Self.agreementAccount)
      try KeychainStore.save(newPrekeys.toBytes(), account: Self.prekeysAccount)
    } catch {
      // A freshly generated identity that fails to persist would silently
      // vanish on the next launch — that's worse than crashing loudly now.
      fatalError("Failed to persist a new identity to the Keychain: \(error)")
    }

    identity = newIdentity
    agreement = newAgreement
    prekeys = newPrekeys
  }

  private static func loadExisting() -> (FfiIdentity, FfiAgreementKey, FfiPrekeyStore)? {
    do {
      guard
        let identityBytes = try KeychainStore.load(account: identityAccount),
        let agreementBytes = try KeychainStore.load(account: agreementAccount),
        let prekeyBytes = try KeychainStore.load(account: prekeysAccount)
      else {
        return nil
      }
      return (
        try FfiIdentity.fromSecretBytes(bytes: identityBytes),
        try FfiAgreementKey.fromSecretBytes(bytes: agreementBytes),
        try FfiPrekeyStore.fromBytes(bytes: prekeyBytes)
      )
    } catch {
      // A corrupt or partially-written Keychain entry must not leave the
      // app unable to start — fall back to generating a fresh identity,
      // the same situation as a first install.
      return nil
    }
  }

  var fingerprint: String {
    identity.fingerprint()
  }

  var publicKeyBytes: Data {
    identity.publicKeyBytes()
  }
}
