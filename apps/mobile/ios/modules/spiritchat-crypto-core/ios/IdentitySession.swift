import Foundation

enum IdentitySessionError: Error {
  /// Thrown by anything that needs an identity (fingerprint, the P2P node,
  /// blob storage, ...) before one has been created or restored yet. The JS
  /// onboarding flow is responsible for calling `hasIdentity`/
  /// `setIdentityFromWords` before touching any of those — this is a
  /// programmer-error guard, not something a user should ever trigger.
  case notYetInitialized
  /// Thrown by account creation/restore once every slot already holds an
  /// identity — the user-facing message is "remove an account first."
  case noFreeSlot
  /// Thrown by `switchTo`/`removeSlot` for a slot index with nothing
  /// stored in it.
  case slotEmpty(Int)
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
///
/// Up to `maxSlots` accounts can live on this device side by side, each in
/// its own Keychain namespace — see `slots()`/`activeSlot`/`switchTo`.
/// Switching is instant and needs no re-authentication (all slots are
/// simultaneously present in the Keychain, exactly like a multi-account
/// wallet app); the recovery phrase is only ever needed again to *create*
/// an account in a fresh device/slot, never to move between slots that
/// already exist on this one.
final class IdentitySession {
  static let maxSlots = 3

  private static var cached: IdentitySession?

  private static func identityAccount(_ slot: Int) -> String { "identity-secret-\(slot)" }
  private static func agreementAccount(_ slot: Int) -> String { "agreement-secret-\(slot)" }
  private static func prekeysAccount(_ slot: Int) -> String { "prekey-store-\(slot)" }
  private static func recoveryPhraseAccount(_ slot: Int) -> String { "recovery-phrase-\(slot)" }
  /// Which slot `shared` currently loads from — itself just a small
  /// non-secret index, but kept in the Keychain rather than UserDefaults to
  /// avoid a second persistence mechanism for one integer.
  private static let activeSlotAccount = "active-slot"

  /// One-time prekeys generated for a brand-new account. Replenishing them
  /// as they're consumed depends on the transport (there is no server or
  /// relay to fetch a fresh bundle from yet), so this is only enough to
  /// make X3DH work until that's designed.
  private static let initialOneTimePrekeyCount: UInt32 = 20

  let slot: Int
  let identity: FfiIdentity
  let agreement: FfiAgreementKey
  let prekeys: FfiPrekeyStore

  private init(slot: Int, identity: FfiIdentity, agreement: FfiAgreementKey, prekeys: FfiPrekeyStore) {
    self.slot = slot
    self.identity = identity
    self.agreement = agreement
    self.prekeys = prekeys
  }

  /// `nil` until an identity exists in the active slot on this device —
  /// either loaded from a previous launch, or created/restored/switched to
  /// this launch. Everything else in this module (the P2P node, blob
  /// storage, the fingerprint the app displays) requires this to be
  /// non-nil first.
  static var shared: IdentitySession? {
    if let cached { return cached }
    guard let loaded = try? loadExisting(slot: activeSlot) else { return nil }
    cached = loaded
    return loaded
  }

  /// Which slot is currently active — persisted so it survives a relaunch.
  /// Defaults to 0 (never explicitly set yet, e.g. a fresh install).
  private(set) static var activeSlot: Int = {
    guard
      let data = try? KeychainStore.load(account: activeSlotAccount),
      let text = String(data: data, encoding: .utf8),
      let value = Int(text)
    else {
      return 0
    }
    return value
  }()

  private static func setActiveSlot(_ slot: Int) {
    activeSlot = slot
    if let data = "\(slot)".data(using: .utf8) {
      try? KeychainStore.save(data, account: activeSlotAccount)
    }
  }

  /// Whether an identity is stored in `slot` — check this to find a free
  /// slot for a new account, or to render an occupied one in an account
  /// switcher. Does not materialize `shared` or touch `activeSlot`.
  static func hasStoredIdentity(slot: Int) -> Bool {
    (try? KeychainStore.load(account: identityAccount(slot))) != nil
  }

  /// Whether the *active* slot has a stored identity — check this on
  /// launch to decide whether to show onboarding (create/restore) or go
  /// straight to the app. Does not materialize `shared`.
  static func hasStoredIdentity() -> Bool {
    hasStoredIdentity(slot: activeSlot)
  }

  /// Every currently-occupied slot, in slot order — what an account
  /// switcher UI lists. Each entry is peeked independently of `shared`/
  /// `activeSlot`, so listing accounts never disturbs which one is active.
  static func occupiedSlots() -> [Int] {
    (0..<maxSlots).filter { hasStoredIdentity(slot: $0) }
  }

  /// The fingerprint stored in `slot`, without activating it — only reads
  /// the identity secret (not the agreement key or prekey store, which an
  /// account switcher listing doesn't need), and never touches `cached`/
  /// `activeSlot`. `nil` if `slot` is empty.
  static func peekFingerprint(slot: Int) -> String? {
    guard
      let bytes = try? KeychainStore.load(account: identityAccount(slot)),
      let identity = try? FfiIdentity.fromSecretBytes(bytes: bytes)
    else {
      return nil
    }
    return identity.fingerprint()
  }

  /// The words of the currently active identity's recovery phrase, if this
  /// device still has them cached (see the doc comment on
  /// `recoveryPhraseAccount` below for why this can legitimately be `nil`).
  /// For re-displaying under Settings → "Show recovery phrase". `nil` isn't
  /// expected in practice (it's written at the same time as the identity
  /// itself) but isn't treated as an error, since there's nothing
  /// actionable to do about it beyond telling the user it's unavailable.
  static func storedRecoveryPhraseWords() -> String? {
    guard let data = try? KeychainStore.load(account: recoveryPhraseAccount(activeSlot)) else { return nil }
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
  /// If `phrase` derives to an identity already occupying a slot on this
  /// device, switches to that slot instead of creating a duplicate (typing
  /// back in a phrase for an account you already have registered here
  /// should just take you to it, not fork a second copy). Otherwise picks
  /// the first free slot; throws `.noFreeSlot` if all `maxSlots` are
  /// already occupied — the caller-facing message is "remove an account
  /// before adding another."
  ///
  /// The agreement key and prekeys are deliberately *not* derived from the
  /// phrase — they're freshly generated every time a *new* slot is created.
  /// Deriving them too would mean anyone who ever recorded a past agreement
  /// key could reconstruct it again from a recovered phrase, defeating the
  /// forward secrecy the Double Ratchet exists to provide. Recovering an
  /// account restores the same fingerprint and PeerId, not the same session
  /// state — existing contacts will need to re-handshake, the same way
  /// losing a Signal-linked device does.
  @discardableResult
  static func begin(withPhrase phrase: FfiRecoveryPhrase) throws -> IdentitySession {
    let identity = try FfiIdentity.fromSecretBytes(bytes: phrase.deriveIdentitySeed())
    let targetFingerprint = identity.fingerprint()

    for slot in 0..<maxSlots where hasStoredIdentity(slot: slot) {
      if peekFingerprint(slot: slot) == targetFingerprint {
        return try switchTo(slot: slot)
      }
    }

    guard let freeSlot = (0..<maxSlots).first(where: { !hasStoredIdentity(slot: $0) }) else {
      throw IdentitySessionError.noFreeSlot
    }

    let agreement = FfiAgreementKey.generate()
    let prekeys = FfiPrekeyStore.generate(
      identity: identity,
      oneTimeCount: initialOneTimePrekeyCount
    )

    try KeychainStore.save(identity.secretBytes(), account: identityAccount(freeSlot))
    try KeychainStore.save(agreement.secretBytes(), account: agreementAccount(freeSlot))
    try KeychainStore.save(prekeys.toBytes(), account: prekeysAccount(freeSlot))
    if let wordsData = phrase.words().data(using: .utf8) {
      try KeychainStore.save(wordsData, account: recoveryPhraseAccount(freeSlot))
    }

    setActiveSlot(freeSlot)
    let session = IdentitySession(slot: freeSlot, identity: identity, agreement: agreement, prekeys: prekeys)
    cached = session
    return session
  }

  /// Switches `activeSlot` to `slot` and reloads `shared` from it —
  /// instant, no phrase required, since every occupied slot's full
  /// key material already lives in this device's Keychain. Throws
  /// `.slotEmpty` if nothing is stored there.
  @discardableResult
  static func switchTo(slot: Int) throws -> IdentitySession {
    let session = try loadExisting(slot: slot)
    setActiveSlot(slot)
    cached = session
    return session
  }

  private static func loadExisting(slot: Int) throws -> IdentitySession {
    guard
      let identityBytes = try KeychainStore.load(account: identityAccount(slot)),
      let agreementBytes = try KeychainStore.load(account: agreementAccount(slot)),
      let prekeyBytes = try KeychainStore.load(account: prekeysAccount(slot))
    else {
      throw IdentitySessionError.slotEmpty(slot)
    }
    return IdentitySession(
      slot: slot,
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

  /// Permanently wipes the identity/agreement/prekeys/recovery-phrase
  /// stored in `slot` from the Keychain — distinct from `switchTo`, which
  /// only moves `activeSlot` and never deletes anything. The only way back
  /// into a *removed* slot is its recovery phrase; if it wasn't saved, this
  /// is permanent. If `slot` was active, forgets the cached session too —
  /// the caller is responsible for routing to onboarding or another slot
  /// afterward, the same as the pre-multi-account `signOut`.
  static func removeSlot(_ slot: Int) {
    KeychainStore.delete(account: identityAccount(slot))
    KeychainStore.delete(account: agreementAccount(slot))
    KeychainStore.delete(account: prekeysAccount(slot))
    KeychainStore.delete(account: recoveryPhraseAccount(slot))
    if slot == activeSlot {
      cached = nil
    }
  }
}
