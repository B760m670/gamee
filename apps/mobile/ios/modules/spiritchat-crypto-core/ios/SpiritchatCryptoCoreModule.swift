import ExpoModulesCore

public class SpiritchatCryptoCoreModule: Module {
  public func definition() -> ModuleDefinition {
    Name("SpiritchatCryptoCore")

    // Proves the native crypto core (packages/crypto-core-ffi) actually
    // loaded through the XCFramework and a full identity/fingerprint round
    // trip works on-device. Remove once real identity/session code depends
    // on this module directly.
    Function("selfCheck") { () -> String in
      let identity = FfiIdentity.generate()
      return identity.fingerprint()
    }
  }
}
