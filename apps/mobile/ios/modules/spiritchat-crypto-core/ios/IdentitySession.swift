import Foundation

enum IdentitySessionError: Error {
  /// Thrown by anything that needs an identity (fingerprint, the P2P node,
  /// blob storage, ...) before one has been created or restored yet. The JS
  /// onboarding flow is responsible for calling `hasIdentity`/
  /// `setIdentityFromWords` before touching any of those — this is a
  /// programmer-error guard, not something a user should ever trigger.
  case notYetInitialized
}

/// The device's local account: a long-term signing identity, a separate
/// X3DH agreement key, and a prekey store, persisted in the Keychain once
/// created so a contact card scanned today still resolves to the same
/// account after every future app restart.
///
/// Unlike the identity/agreement/prekey trio it wraps, the identity's own
/// seed is never generated from raw randomness this class doesn't retain —
/// it's always derived from a BIP39 recovery phrase (see
/// `FfiRecoveryPhrase`), whether that phrase was just generated for a new
/// account or typed back in to restore one. There is no server anywhere in
/// this project that could hold a "reset password" link, so the phrase
/// *is* the only account-recovery mechanism there will ever be — losing it
/// after failing to write it down means losing the account permanently, the
/// same as any cryptocurrency wallet.
final class IdentitySession {
  private static var cached: IdentitySession?

  private static let identityAccount = "identity-secret"
  private static let agreementAccount = "agreement-secret"
  private static let prekeysAccount = "prekey-store"
  private static let recoveryPhraseAccount = "recovery-phrase"

  /// One-time prekeys generated for a brand-new account. Replenishing them
  /// as they're consumed depends on the transport (there is no server or
  /// relay to fetch a fresh bundle from yet), so this is only enough to
  /// make X3DH work until that's designed.
  private static let initialOneTimePrekeyCount: UInt32 = 20

  let identity: FfiIdentity
  let agreement: FfiAgreementKey
  let prekeys: FfiPrekeyStore

  private init(identity: FfiIdentity, agreement: FfiAgreementKey, prekeys: FfiPrekeyStore) {
    self.identity = identity
    self.agreement = agreement
    self.prekeys = prekeys
  }

  /// `nil` until an identity exists on this device — either loaded from a
  /// previous launch, or created this launch via `begin(withWords:)`.
  /// Everything else in this module (the P2P node, blob storage, the
  /// fingerprint the app displays) requires this to be non-nil first.
  static var shared: IdentitySession? {
    if let cached { return cached }
    guard let loaded = try? loadExisting() else { return nil }
    cached = loaded
    return loaded
  }

  /// Whether an identity is already stored on this device — check this on
  /// launch to decide whether to show onboarding (create/restore) or go
  /// straight to the app. Does not materialize `shared`.
  static func hasStoredIdentity() -> Bool {
    (try? KeychainStore.load(account: identityAccount)) != nil
  }

  /// The words of the currently active identity's recovery phrase, if this
  /// device still has them cached (see the doc comment on
  /// `recoveryPhraseAccount` below for why this can legitimately be `nil`).
  /// For re-displaying under Settings → "Show recovery phrase", not for
  /// anything this class itself needs — the identity is already derived
  /// and persisted separately.
  static func storedRecoveryPhraseWords() -> String? {
    guard let data = try? KeychainStore.load(account: recoveryPhraseAccount) else { return nil }
    return String(data: data, encoding: .utf8)
  }

  /// Creates the identity/agreement/prekeys from `phrase` and persists all
  /// of them, including the phrase's own words (Keychain already holds the
  /// identity's raw secret at the same protection level, so caching the
  /// phrase too — purely so Settings can re-display it later for a user who
  /// didn't get to write it down properly the first time — doesn't create
  /// any new exposure beyond what the raw key already has). Used
  /// identically whether `phrase` was just generated (new account) or typed
  /// back in (recovery) — from this point on there's no difference between
  /// the two paths.
  ///
  /// The agreement key and prekeys are deliberately *not* derived from the
  /// phrase — they're freshly generated every time this runs. Deriving them
  /// too would mean anyone who ever recorded a past agreement key could
  /// reconstruct it again from a recovered phrase, defeating the forward
  /// secrecy the Double Ratchet exists to provide. Recovering an account
  /// restores the same fingerprint and PeerId, not the same session state —
  /// existing contacts will need to re-handshake, the same way losing a
  /// Signal-linked device does.
  @discardableResult
  static func begin(withPhrase phrase: FfiRecoveryPhrase) throws -> IdentitySession {
    let identity = try FfiIdentity.fromSecretBytes(bytes: phrase.deriveIdentitySeed())
    let agreement = FfiAgreementKey.generate()
    let prekeys = FfiPrekeyStore.generate(
      identity: identity,
      oneTimeCount: initialOneTimePrekeyCount
    )

    try KeychainStore.save(identity.secretBytes(), account: identityAccount)
    try KeychainStore.save(agreement.secretBytes(), account: agreementAccount)
    try KeychainStore.save(prekeys.toBytes(), account: prekeysAccount)
    if let wordsData = phrase.words().data(using: .utf8) {
      try KeychainStore.save(wordsData, account: recoveryPhraseAccount)
    }

    let session = IdentitySession(identity: identity, agreement: agreement, prekeys: prekeys)
    cached = session
    return session
  }

  private static func loadExisting() throws -> IdentitySession? {
    guard
      let identityBytes = try KeychainStore.load(account: identityAccount),
      let agreementBytes = try KeychainStore.load(account: agreementAccount),
      let prekeyBytes = try KeychainStore.load(account: prekeysAccount)
    else {
      return nil
    }
    return IdentitySession(
      identity: try FfiIdentity.fromSecretBytes(bytes: identityBytes),
      agreement: try FfiAgreementKey.fromSecretBytes(bytes: agreementBytes),
      prekeys: try FfiPrekeyStore.fromBytes(bytes: prekeyBytes)
    )
  }

  var fingerprint: String {
    identity.fingerprint()
  }

  var publicKeyBytes: Data {
    identity.publicKeyBytes()
  }
}
