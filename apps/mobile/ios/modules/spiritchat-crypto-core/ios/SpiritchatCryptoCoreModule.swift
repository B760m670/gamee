import ExpoModulesCore

public class SpiritchatCryptoCoreModule: Module {
  public func definition() -> ModuleDefinition {
    Name("SpiritchatCryptoCore")

    // This device's persistent identity (see IdentitySession.swift):
    // generated once and stored in the Keychain, not regenerated per call.
    Function("fingerprint") { () -> String in
      IdentitySession.shared.fingerprint
    }

    Function("publicKeyBase64") { () -> String in
      IdentitySession.shared.publicKeyBytes.base64EncodedString()
    }
  }
}
