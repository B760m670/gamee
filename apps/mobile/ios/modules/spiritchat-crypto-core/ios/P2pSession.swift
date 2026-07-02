import Foundation

/// This device's live network presence: one libp2p node, started once per
/// app run using the same 32-byte seed as `IdentitySession`'s identity key
/// (see `spiritchat_p2p_core::identity::keypair_from_seed`), so a peer's
/// `PeerId` on the wire always matches the identity in their contact card.
final class P2pSession {
  static let shared = P2pSession()

  let node: FfiP2pNode

  private init() {
    do {
      node = try FfiP2pNode.spawn(identitySeed: IdentitySession.shared.identity.secretBytes())
    } catch {
      // Mirrors IdentitySession's fatalError: a node that silently failed
      // to start would leave the app looking connected while never sending
      // or receiving anything.
      fatalError("Failed to start the P2P node: \(error)")
    }
  }

  /// Encodes an event as a plain dictionary for the JS bridge, tagged by
  /// `type` so the TS side can discriminate without a second binding layer.
  static func encode(_ event: FfiP2pEvent) -> [String: Any?] {
    switch event {
    case .listeningOn(let address):
      return ["type": "listeningOn", "address": address]
    case .peerDiscoveredLocally(let peerId):
      return ["type": "peerDiscoveredLocally", "peerId": peerId]
    case .peerConnected(let peerId):
      return ["type": "peerConnected", "peerId": peerId]
    case .peerIdentified(let peerId):
      return ["type": "peerIdentified", "peerId": peerId]
    case .peerDisconnected(let peerId):
      return ["type": "peerDisconnected", "peerId": peerId]
    case .dialFailed(let peerId, let reason):
      return ["type": "dialFailed", "peerId": peerId, "reason": reason]
    case .relayReservationFailed(let reason):
      return ["type": "relayReservationFailed", "reason": reason]
    case .envelopeReceived(let fromPeerId, let bytes):
      return ["type": "envelopeReceived", "fromPeerId": fromPeerId, "bytes": bytes]
    case .envelopeDelivered(let toPeerId):
      return ["type": "envelopeDelivered", "toPeerId": toPeerId]
    case .envelopeDeliveryFailed(let toPeerId, let reason):
      return ["type": "envelopeDeliveryFailed", "toPeerId": toPeerId, "reason": reason]
    case .peerAddressesResolved(let peerId, let addresses):
      return ["type": "peerAddressesResolved", "peerId": peerId, "addresses": addresses]
    case .peerAddressResolutionFailed(let peerId):
      return ["type": "peerAddressResolutionFailed", "peerId": peerId]
    case .addressesAnnounced:
      return ["type": "addressesAnnounced"]
    case .addressAnnouncementFailed(let reason):
      return ["type": "addressAnnouncementFailed", "reason": reason]
    case .blobFetched(let peerId, let id, let bytes):
      // Cache to disk here rather than shipping the raw bytes across the
      // JS bridge a second time — JS only ever needs a file path to hand
      // to <Image>, never the bytes themselves.
      let idHex = id.hexEncoded
      let url = (try? BlobStore.save(bytes, idHex: idHex)) ?? BlobStore.path(for: idHex)
      return ["type": "blobFetched", "peerId": peerId, "id": idHex, "localPath": url.absoluteString]
    case .blobFetchFailed(let peerId, let id, let reason):
      return ["type": "blobFetchFailed", "peerId": peerId, "id": id.hexEncoded, "reason": reason]
    }
  }
}
