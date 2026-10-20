package expo.modules.spiritchatcryptocore

import uniffi.spiritchat_crypto_core_ffi.FfiAgreementKey
import uniffi.spiritchat_crypto_core_ffi.FfiIdentity
import uniffi.spiritchat_crypto_core_ffi.FfiPrekeyStore
import uniffi.spiritchat_crypto_core_ffi.FfiRecoveryPhrase

internal class IdentitySessionException(message: String) : Exception(message)

/**
 * The account this device is signed into — a port of
 * `IdentitySession.swift`, keeping its slot names and its rules, because
 * the two platforms describe the same accounts to the same network and a
 * divergence here is a divergence in who someone *is*.
 *
 * Three slots, so one device can hold three accounts and switch between
 * them without re-entering a phrase. A slot holds four items: the
 * identity secret, an agreement key, a prekey store, and the recovery
 * phrase's own words.
 *
 * The one deliberate difference from iOS: there is no prekey-format
 * migration path here. The iOS side carries one because it has stored
 * prekeys in an older shape on real devices; this code has never run
 * anywhere, so there is nothing to migrate from and inventing a migration
 * would be inventing a format that never existed.
 */
internal class IdentitySession private constructor(
  val slot: Int,
  val identity: FfiIdentity,
  val agreement: FfiAgreementKey,
  val prekeys: FfiPrekeyStore,
) {

  companion object {
    const val MAX_SLOTS = 3

    /**
     * How many one-time prekeys a new account publishes. Each one lets
     * exactly one stranger open a conversation without this device being
     * online, so the number is how many such first contacts can be
     * absorbed while offline — matched to iOS so an account behaves the
     * same whichever platform created it.
     */
    private const val INITIAL_ONE_TIME_PREKEY_COUNT: UInt = 20u

    private fun identityAccount(slot: Int) = "identity-secret-$slot"
    private fun agreementAccount(slot: Int) = "agreement-secret-$slot"
    private fun prekeysAccount(slot: Int) = "prekey-store-$slot"
    private fun recoveryPhraseAccount(slot: Int) = "recovery-phrase-$slot"
    private const val ACTIVE_SLOT_ACCOUNT = "active-slot"

    private lateinit var store: SecretStore
    private var cached: IdentitySession? = null

    /** Called once, from the module, before anything else here is used. */
    fun attach(store: SecretStore) {
      this.store = store
    }

    var activeSlot: Int = 0
      private set
      get() {
        // Read through on each access rather than cached in a lazy
        // initialiser: the stored value changes under us whenever another
        // slot is activated, and a stale 0 here would silently operate on
        // the wrong account.
        val stored = store.load(ACTIVE_SLOT_ACCOUNT)?.toString(Charsets.UTF_8)?.trim()
        return stored?.toIntOrNull()?.takeIf { it in 0 until MAX_SLOTS } ?: 0
      }

    private fun setActiveSlot(slot: Int) {
      store.save(ACTIVE_SLOT_ACCOUNT, slot.toString().toByteArray(Charsets.UTF_8))
    }

    fun shared(): IdentitySession? {
      cached?.let { if (it.slot == activeSlot) return it }
      val loaded = runCatching { loadExisting(activeSlot) }.getOrNull() ?: return null
      cached = loaded
      return loaded
    }

    fun require(): IdentitySession =
      shared() ?: throw IdentitySessionException(
        "No account on this device yet — create one or restore from a recovery phrase first."
      )

    private fun hasStoredIdentity(slot: Int) = store.contains(identityAccount(slot))

    /**
     * Whether `slot` holds all four items, which is exactly what
     * [loadExisting] needs. Checked separately from "has an identity"
     * because the four are written one after another with nothing
     * wrapping them in a transaction: a slot killed mid-write can hold an
     * identity and nothing else, and offering that as switchable gives
     * someone an account that can never load.
     */
    private fun isSlotComplete(slot: Int) =
      hasStoredIdentity(slot) &&
        store.contains(agreementAccount(slot)) &&
        store.contains(prekeysAccount(slot))

    /** Whether the active slot is usable — what launch checks to decide
     *  between onboarding and the app. */
    fun hasStoredIdentity(): Boolean = isSlotComplete(activeSlot)

    fun occupiedSlots(): List<Int> = (0 until MAX_SLOTS).filter { isSlotComplete(it) }

    /** The fingerprint in `slot` without activating it — what an account
     *  switcher lists. Never disturbs which account is active. */
    fun peekFingerprint(slot: Int): String? = runCatching {
      val bytes = store.load(identityAccount(slot)) ?: return null
      FfiIdentity.fromSecretBytes(bytes).fingerprint()
    }.getOrNull()

    /**
     * The active account's recovery phrase, for re-displaying under
     * settings. `null` is not an error: there is nothing actionable to do
     * about it beyond saying it is unavailable.
     */
    fun storedRecoveryPhraseWords(): String? =
      runCatching { store.load(recoveryPhraseAccount(activeSlot))?.toString(Charsets.UTF_8) }
        .getOrNull()

    /**
     * Creates and persists everything derived from `phrase`. Used
     * identically whether the phrase was just generated or typed back in
     * — past this point the two are the same operation.
     *
     * Restoring returns the same fingerprint and peer id, not the same
     * conversations: existing contacts re-handshake, as they do when any
     * linked device is lost.
     */
    fun begin(phrase: FfiRecoveryPhrase): IdentitySession {
      val identity = FfiIdentity.fromSecretBytes(phrase.deriveIdentitySeed())
      val targetFingerprint = identity.fingerprint()

      for (slot in 0 until MAX_SLOTS) {
        if (!hasStoredIdentity(slot)) continue
        if (peekFingerprint(slot) != targetFingerprint) continue

        if (isSlotComplete(slot)) return switchTo(slot)

        // Same account, interrupted mid-write. Typing the phrase back in
        // is the only repair a user has, so repair it rather than forking
        // a second slot for one identity or leaving this one stuck.
        val repaired = writeSlot(slot, identity, phrase)
        setActiveSlot(slot)
        cached = repaired
        return repaired
      }

      val freeSlot = (0 until MAX_SLOTS).firstOrNull { !hasStoredIdentity(it) }
        ?: throw IdentitySessionException(
          "All $MAX_SLOTS account slots on this device are in use — remove one first."
        )

      val session = writeSlot(freeSlot, identity, phrase)
      setActiveSlot(freeSlot)
      cached = session
      return session
    }

    /**
     * Writes the full four-item record. Not transactional — there is no
     * cross-entry transaction to use — but idempotent: running it again
     * regenerates the agreement key and prekeys, and [isSlotComplete]
     * keeps reporting the slot unusable until every item has landed.
     */
    private fun writeSlot(
      slot: Int,
      identity: FfiIdentity,
      phrase: FfiRecoveryPhrase,
    ): IdentitySession {
      val agreement = FfiAgreementKey.generate()
      val prekeys = FfiPrekeyStore.generate(identity, INITIAL_ONE_TIME_PREKEY_COUNT)

      store.save(identityAccount(slot), identity.secretBytes())
      store.save(agreementAccount(slot), agreement.secretBytes())
      store.save(prekeysAccount(slot), prekeys.toBytes())
      store.save(recoveryPhraseAccount(slot), phrase.words().toByteArray(Charsets.UTF_8))

      return IdentitySession(slot, identity, agreement, prekeys)
    }

    /** Instant, no phrase needed — every occupied slot's key material is
     *  already on this device. */
    fun switchTo(slot: Int): IdentitySession {
      val session = loadExisting(slot)
      setActiveSlot(slot)
      cached = session
      return session
    }

    /** Loads a slot *without* activating it — what running every
     *  account's node at once needs. `null` if empty or incomplete. */
    fun loadFor(slot: Int): IdentitySession? = runCatching { loadExisting(slot) }.getOrNull()

    private fun loadExisting(slot: Int): IdentitySession {
      val identityBytes = store.load(identityAccount(slot))
      val agreementBytes = store.load(agreementAccount(slot))
      val prekeyBytes = store.load(prekeysAccount(slot))
      if (identityBytes == null || agreementBytes == null || prekeyBytes == null) {
        throw IdentitySessionException("Account slot $slot is empty or was never finished writing.")
      }
      return IdentitySession(
        slot,
        FfiIdentity.fromSecretBytes(identityBytes),
        FfiAgreementKey.fromSecretBytes(agreementBytes),
        FfiPrekeyStore.fromBytes(prekeyBytes),
      )
    }

    /** Erases `slot` entirely. Deleting what is not there is not an error. */
    fun removeSlot(slot: Int) {
      store.delete(identityAccount(slot))
      store.delete(agreementAccount(slot))
      store.delete(prekeysAccount(slot))
      store.delete(recoveryPhraseAccount(slot))
      if (cached?.slot == slot) cached = null
    }
  }
}
