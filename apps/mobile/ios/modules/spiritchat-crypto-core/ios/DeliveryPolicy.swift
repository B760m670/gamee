import Foundation

/// How a message is allowed to reach its recipient — the one place that
/// decides whether this app trades metadata for speed.
///
/// The distinction it exists to enforce: **end-to-end encryption protects
/// what was said; it does nothing about who said it to whom.** A direct
/// connection to a contact puts that contact's IP address in front of
/// every network this device's traffic crosses — the carrier first among
/// them. Content stays unreadable, and the social graph is handed over in
/// the clear. For most people, most of the time, the graph is the more
/// sensitive of the two.
///
/// Routing through the Sphinx/Loopix mix instead (see `p2p-core`'s `mix.rs`
/// and `mailbox.rs`) means an observer sees this device talking to a mix
/// node, not to a person. Deposits are addressed by unlinkable rotating
/// tags rather than by recipient identity, and no single hop learns both
/// ends of a path.
///
/// **This costs nothing in reachability, and that is not a coincidence.**
/// Both halves of the mailbox path — depositing into the mix and sweeping
/// for one's own tags — are *outbound* operations. Neither needs this
/// device to be dialable from outside, so the mode that protects metadata
/// is also the mode that works from behind carrier-grade NAT, where a
/// direct connection to a contact is usually impossible anyway.
enum DeliveryRoute {
  /// Deposit into the mix. Nothing on the wire links this device to the
  /// recipient.
  case mix
  /// Open a connection straight to the recipient. Fast and reliable, and
  /// it discloses to the network who is being talked to.
  case direct
}

enum DeliveryPolicy {
  private static let allowDirectKey = "spiritchat.allowDirectDelivery"

  /// Whether this device may fall back to a direct connection when no mix
  /// path is available.
  ///
  /// **Off by default, and the default is the point.** A privacy property
  /// that silently degrades under load is not a privacy property — if an
  /// unavailable mix quietly turned into a direct connection, the graph
  /// would leak exactly when the network is at its weakest, and the user
  /// would never know it happened. With this off, a message that cannot be
  /// sent unlinkably stays queued instead, which this project already
  /// treats as an ordinary state rather than an error (there is no server
  /// here; "not deliverable right now" is normal).
  ///
  /// Turning it on is a legitimate choice — a user who values delivery
  /// speed over graph privacy, or who is talking to someone already
  /// publicly associated with them — but it has to be a choice, made once,
  /// knowingly, rather than a behaviour the transport picks on its own.
  static var allowsDirectDelivery: Bool {
    get { UserDefaults.standard.bool(forKey: allowDirectKey) }
    set { UserDefaults.standard.set(newValue, forKey: allowDirectKey) }
  }

  /// The route to use for a message right now.
  ///
  /// `mixPathAvailable` is what the caller currently knows about the mix's
  /// usability. When it is false and direct delivery is not allowed, the
  /// answer is `nil` — meaning "do not send yet", never "send it the fast
  /// way just this once".
  static func route(mixPathAvailable: Bool) -> DeliveryRoute? {
    if mixPathAvailable { return .mix }
    return allowsDirectDelivery ? .direct : nil
  }
}
