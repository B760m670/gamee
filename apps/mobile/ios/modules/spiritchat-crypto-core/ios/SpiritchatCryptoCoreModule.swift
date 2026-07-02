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

    Events("onP2pEvent")

    // Starts the node and begins pumping its event loop as soon as the
    // module is created, independent of whether JS has attached a
    // listener yet — an incoming envelope must still be dialed/received
    // even if the chat screen isn't mounted.
    OnCreate {
      Task {
        while let event = await P2pSession.shared.node.nextEvent() {
          self.sendEvent("onP2pEvent", P2pSession.encode(event))
        }
      }
    }

    Function("p2pLocalPeerId") { () -> String in
      P2pSession.shared.node.localPeerId()
    }

    Function("p2pDial") { (peerId: String, knownAddresses: [String]) throws in
      try P2pSession.shared.node.dial(peerId: peerId, knownAddresses: knownAddresses)
    }

    Function("p2pResolvePeerAddresses") { (peerId: String) throws in
      try P2pSession.shared.node.resolvePeerAddresses(peerId: peerId)
    }

    Function("p2pAnnounceAddresses") { (addresses: [String]) throws in
      try P2pSession.shared.node.announceAddresses(addresses: addresses)
    }

    Function("p2pSendEnvelope") { (peerId: String, bytes: Data) throws in
      try P2pSession.shared.node.sendEnvelope(peerId: peerId, bytes: bytes)
    }

    Function("p2pReserveRelaySlot") { (relayAddress: String) throws in
      try P2pSession.shared.node.reserveRelaySlot(relayAddress: relayAddress)
    }
  }
}
