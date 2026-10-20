package expo.modules.spiritchatcryptocore

import expo.modules.kotlin.exception.CodedException
import expo.modules.kotlin.exception.Exceptions
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import uniffi.spiritchat_crypto_core_ffi.FfiRecoveryPhrase

/**
 * Raised by everything this module does not do yet.
 *
 * The whole surface is declared from the start, rather than added a
 * function at a time, because `requireNativeModule` hands JavaScript one
 * object: a name missing from it fails as `undefined is not a function`,
 * somewhere in a screen, with nothing saying whether it is a typo, a
 * build problem or simply unported. Declared and throwing, it says which
 * — by name, at the call, in a sentence a person can act on.
 *
 * Every one of these is a promise to be deleted.
 */
internal class NotPortedYetException(what: String, wave: String) : CodedException(
  "`$what` is not on Android yet — it lands with $wave. " +
    "This is a gap in the port, not a failure; iOS has it."
)

/**
 * The Android half of the native module behind `index.ts`.
 *
 * Its job is exactly its iOS counterpart's: expose the shared Rust core
 * and the orchestration around it to the React Native app above, which is
 * the same application code on both platforms. Where this file and
 * `SpiritchatCryptoCoreModule.swift` disagree, one of the two is wrong.
 *
 * Function names, argument order and return shapes are fixed by the
 * `requireNativeModule` block in `index.ts` and are not ours to improve
 * here — a nicer name on this side is just a name JavaScript will not
 * find.
 */
class SpiritchatCryptoCoreModule : Module() {

  /** How every module in this SDK reaches a Context, including the
   *  exception it raises when there is none. */
  private val context
    get() = appContext.reactContext ?: throw Exceptions.AppContextLost()

  private val identityStore by lazy { SecretStore(context) }

  override fun definition() = ModuleDefinition {
    Name("SpiritchatCryptoCore")

    // Both event names the JS side subscribes to. Declared together
    // because `addListener` is typed over this pair; emitting is wave 3
    // and wave 4 work.
    Events("onP2pEvent", "onChatEvent")

    OnCreate {
      IdentitySession.attach(identityStore)
    }

    // ---- Identity and accounts -------------------------------------
    // Ported. This is what onboarding needs, and until it works the app
    // cannot get past its first screen to show anything else.

    Function("hasIdentity") {
      IdentitySession.hasStoredIdentity()
    }

    Function("generateRecoveryPhrase") {
      FfiRecoveryPhrase.generate().words()
    }

    /**
     * Takes the twelve words — freshly generated or typed back in — and
     * returns the resulting fingerprint. One function for both paths
     * because past this point they are the same operation.
     */
    Function("setIdentityFromWords") { words: String ->
      val phrase = FfiRecoveryPhrase.fromWords(words)
      IdentitySession.begin(phrase).identity.fingerprint()
    }

    Function("recoveryPhraseWords") {
      IdentitySession.storedRecoveryPhraseWords()
    }

    Function("fingerprint") {
      IdentitySession.require().identity.fingerprint()
    }

    Function("publicKeyBase64") {
      android.util.Base64.encodeToString(
        IdentitySession.require().identity.publicKeyBytes(),
        android.util.Base64.NO_WRAP,
      )
    }

    Function("accountSlots") {
      IdentitySession.occupiedSlots().mapNotNull { slot ->
        IdentitySession.peekFingerprint(slot)?.let {
          mapOf("slot" to slot, "fingerprint" to it)
        }
      }
    }

    Function("activeAccountSlot") {
      IdentitySession.activeSlot
    }

    Function("switchAccount") { slot: Int ->
      IdentitySession.switchTo(slot)
      Unit
      // Nodes and controllers follow the active account; wiring that
      // follow-through is wave 3, where there is a node to follow.
    }

    Function("signOut") {
      IdentitySession.removeSlot(IdentitySession.activeSlot)
      IdentitySession.occupiedSlots().firstOrNull()?.let { IdentitySession.switchTo(it) }
      Unit
    }

    Function("removeAccount") { slot: Int ->
      val wasActive = slot == IdentitySession.activeSlot
      IdentitySession.removeSlot(slot)
      if (wasActive) {
        IdentitySession.occupiedSlots().firstOrNull()?.let { IdentitySession.switchTo(it) }
      }
      Unit
    }

    // ---- The P2P node ----------------------------------------------
    // Wave 3. Thin over the Rust node, which already does the work; what
    // is missing is the session that owns one per account and the pump
    // that turns its events into `onP2pEvent`.

    Function("p2pLocalPeerId") { notPortedYet("p2pLocalPeerId", "the P2P wave") }
    Function("p2pIsReady") {
      // Answered rather than thrown: this is the question the UI asks to
      // decide whether to show a connected state, and a throw here would
      // surface as an error banner rather than an honest "not connected".
      false
    }
    Function("p2pLastStartupErrorDescription") {
      "The peer-to-peer node is not part of the Android build yet."
    }
    Function("p2pListenAddresses") { emptyList<String>() }
    Function("p2pDial") { _: String, _: List<String> ->
      notPortedYet("p2pDial", "the P2P wave")
    }
    Function("p2pResolvePeerAddresses") { _: String ->
      notPortedYet("p2pResolvePeerAddresses", "the P2P wave")
    }
    Function("p2pAnnounceAddresses") { _: List<String> ->
      notPortedYet("p2pAnnounceAddresses", "the P2P wave")
    }
    Function("p2pSendEnvelope") { _: String, _: ByteArray ->
      notPortedYet("p2pSendEnvelope", "the P2P wave")
    }
    Function("p2pReserveRelaySlot") { _: String ->
      notPortedYet("p2pReserveRelaySlot", "the P2P wave")
    }
    Function("p2pAnnounceUsername") { _: String ->
      notPortedYet("p2pAnnounceUsername", "the P2P wave")
    }
    Function("p2pResolveUsername") { _: String ->
      notPortedYet("p2pResolveUsername", "the P2P wave")
    }
    Function("p2pFetchBlob") { _: String, _: String ->
      notPortedYet("p2pFetchBlob", "the P2P wave")
    }
    Function("p2pSetLocalBlobRaw") { _: String, _: ByteArray ->
      notPortedYet("p2pSetLocalBlobRaw", "the P2P wave")
    }
    Function("p2pAnnounceAvatarPointer") { _: ByteArray ->
      notPortedYet("p2pAnnounceAvatarPointer", "the P2P wave")
    }
    Function("p2pResolveAvatarPointer") { _: String ->
      notPortedYet("p2pResolveAvatarPointer", "the P2P wave")
    }
    Function("p2pRequestChainSync") { _: String ->
      notPortedYet("p2pRequestChainSync", "the P2P wave")
    }
    Function("p2pQueryChainTip") {
      notPortedYet("p2pQueryChainTip", "the P2P wave")
    }
    Function("p2pQueryUsernameOwner") { _: String ->
      notPortedYet("p2pQueryUsernameOwner", "the P2P wave")
    }
    Function("p2pSubmitUsernameClaim") { _: ByteArray ->
      notPortedYet("p2pSubmitUsernameClaim", "the P2P wave")
    }
    Function("p2pSubmitMinedBlock") { _: ByteArray ->
      notPortedYet("p2pSubmitMinedBlock", "the P2P wave")
    }
    Function("ledgerBuildUsernameClaim") { _: String, _: Int, _: ByteArray, _: ByteArray ->
      notPortedYet("ledgerBuildUsernameClaim", "the P2P wave")
    }
    Function("p2pStartMining") { _: String ->
      notPortedYet("p2pStartMining", "the P2P wave")
    }
    Function("p2pStopMining") {
      // Stopping what was never started is not an error anywhere else in
      // this codebase, and throwing here would make teardown paths fail
      // on their way out.
    }

    // ---- Delivery policy and the mix -------------------------------
    // Wave 3. These read and write a preference each; they are answered
    // honestly rather than thrown so the privacy screens can render and
    // show their real state.

    Function("allowsDirectDelivery") { deliveryPreferences().getBoolean(ALLOWS_DIRECT, false) }
    Function("setAllowsDirectDelivery") { allowed: Boolean ->
      deliveryPreferences().edit().putBoolean(ALLOWS_DIRECT, allowed).apply()
    }
    Function("mixRelayParticipationEnabled") {
      deliveryPreferences().getBoolean(MIX_RELAY, false)
    }
    Function("setMixRelayParticipationEnabled") { enabled: Boolean ->
      deliveryPreferences().edit().putBoolean(MIX_RELAY, enabled).apply()
    }
    Function("mixDummyTrafficBytesPerHourEstimate") { 0.0 }

    // ---- Blobs ------------------------------------------------------
    // Wave 2, with the rest of storage.

    Function("blobSaveFromFile") { _: String ->
      notPortedYet("blobSaveFromFile", "the storage wave")
    }
    // The type is spelled out because a lambda returning only `null`
    // infers `Nothing?`, which cannot be reified any more than
    // `Nothing` can. It is also what index.ts declares: `string | null`.
    Function<String?, String>("blobLocalPath") { _ -> null }
    Function("blobClear") { _: String -> }
    Function("blobReserve") { _: String -> false }

    // ---- Chat -------------------------------------------------------
    // Wave 4, and the largest piece of the port: delivery policy, the
    // outbox, groups and consent are 2,090 lines of ChatManager on iOS.

    Function("chatSendMessage") { _: String, _: String, _: String ->
      notPortedYet("chatSendMessage", "the chat wave")
    }
    Function("chatSendMedia") { _: String, _: String, _: String, _: String, _: String?, _: Int? ->
      notPortedYet("chatSendMedia", "the chat wave")
    }
    Function("chatCreateGroup") { _: String, _: List<String> ->
      notPortedYet("chatCreateGroup", "the chat wave")
    }
    Function("chatSendGroupMessage") { _: String, _: String ->
      notPortedYet("chatSendGroupMessage", "the chat wave")
    }
    Function("chatSendGroupMedia") { _: String, _: String, _: String, _: String?, _: Int? ->
      notPortedYet("chatSendGroupMedia", "the chat wave")
    }
    Function("chatAddGroupMember") { _: String, _: String ->
      notPortedYet("chatAddGroupMember", "the chat wave")
    }
    Function("chatRemoveGroupMember") { _: String, _: String ->
      notPortedYet("chatRemoveGroupMember", "the chat wave")
    }
    Function("chatSetConsent") { _: String, _: String ->
      notPortedYet("chatSetConsent", "the chat wave")
    }
    Function("chatConsentFor") { _: String -> "none" }
    Function("chatConsentList") { emptyList<Map<String, String>>() }

    // ---- Recovery backup --------------------------------------------
    // Wave 3: it is published to and read from the DHT.

    Function("recoveryBackupPublish") { _: String ->
      notPortedYet("recoveryBackupPublish", "the P2P wave")
    }
    Function("recoveryBackupRequest") {
      notPortedYet("recoveryBackupRequest", "the P2P wave")
    }

    // ---- Voice notes -------------------------------------------------
    // Wave 5. Android records through MediaRecorder, which has nothing in
    // common with AVAudioRecorder beyond the shape of this interface.

    Function("hasMicrophonePermission") {
      context.checkSelfPermission(android.Manifest.permission.RECORD_AUDIO) ==
        android.content.pm.PackageManager.PERMISSION_GRANTED
    }
    AsyncFunction("requestMicrophonePermission") {
      notPortedYet("requestMicrophonePermission", "the platform wave")
    }
    Function("voiceRecordingStart") {
      notPortedYet("voiceRecordingStart", "the platform wave")
    }
    Function<Map<String, Any>?>("voiceRecordingStop") { null }
    Function("voiceRecordingCancel") { }
  }

  /**
   * Raises [NotPortedYetException], and — the point of existing — is
   * declared to return Unit while doing it.
   *
   * `Function` is `inline fun <reified R> Function(name, body: () -> R)`,
   * and a lambda whose whole body is a `throw` has type `Nothing`, which
   * cannot be a reified type argument: "Cannot use 'Nothing' as reified
   * type parameter", twenty-seven times over. Going through a Unit
   * function gives the inference something to land on.
   */
  private fun notPortedYet(what: String, wave: String) {
    throw NotPortedYetException(what, wave)
  }

  private fun deliveryPreferences() = context.getSharedPreferences(
    "com.spiritchat.cryptocore.delivery",
    android.content.Context.MODE_PRIVATE,
  )

  private companion object {
    const val ALLOWS_DIRECT = "allows-direct-delivery"
    const val MIX_RELAY = "mix-relay-participation"
  }
}
