import Foundation

/// A `@username` claim published to the public DHT (see
/// `p2p_core::username` and `FfiP2pNode.announceUsername`/
/// `resolveUsername`) — 32 bytes of Ed25519 public key followed by a
/// 64-byte signature over the normalized username, so nobody but that
/// key's owner can produce a valid claim for a given name. A DHT node
/// storing the record can't forge or alter one without the signature
/// failing to verify.
///
/// This is *not* the same as reserving the name against a determined
/// second claimant — a plain DHT has no global ordering/consensus, so
/// there's no way to prove "who claimed it first" the way a blockchain or
/// a server could. It only rules out impersonation: whoever a claim
/// verifies against is definitely who published it, even if two different
/// identities have each published their own claim for the same name at
/// different times.
enum UsernameClaim {
  /// Case-insensitive, matching `p2p_core::username::normalize` — the DHT
  /// key is already normalized on the Rust side, but the *signed message*
  /// has to be normalized the same way here too, or a claim signed for
  /// "Alice" wouldn't verify when looked up as "alice".
  static func normalize(_ username: String) -> String {
    username.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
  }

  static func build(for username: String, identity: FfiIdentity) -> Data {
    let message = Data(normalize(username).utf8)
    let signature = identity.sign(message: message)
    return identity.publicKeyBytes() + signature
  }

  /// The claim's public key if `claim` verifies for `username`, `nil`
  /// otherwise (malformed bytes, or a signature that doesn't check out —
  /// both mean "don't trust this claim", not two different outcomes worth
  /// telling apart).
  static func verify(username: String, claim: Data) -> Data? {
    guard claim.count == 32 + 64 else { return nil }
    let publicKey = Data(claim.prefix(32))
    let signature = Data(claim.suffix(64))
    let message = Data(normalize(username).utf8)
    guard identityVerify(publicKey: publicKey, message: message, signature: signature) else {
      return nil
    }
    return publicKey
  }
}
