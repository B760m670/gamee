import Foundation

/// Which channel this binary was built for. Decided at **compile time**, from
/// the `SPIRITCHAT_APP_STORE` Swift condition the podspec sets when
/// `SPIRITCHAT_DISTRIBUTION=app-store` is exported for `pod install` (see
/// `SpiritchatCryptoCore.podspec` and the iOS workflow).
///
/// Compile time, not a runtime setting, and that distinction is the whole
/// point: a build destined for review must be *incapable* of the behaviour it
/// promises not to perform, so that the promise cannot be undone by a remote
/// config, a debug menu, a JS bundle update, or a bug. The code that would do
/// it is not in the binary.
enum DistributionChannel {
  /// Built for App Store / TestFlight review.
  case appStore
  /// Sideloaded (SideStore, LiveContainer) or a local dev build — the channel
  /// this project has shipped through so far.
  case direct
}

/// The behavioural differences between distribution channels, gathered in one
/// place so what a reviewed build can and cannot do is auditable by reading a
/// single file rather than by grepping for `#if` scattered across the module.
enum DistributionPolicy {
  static let channel: DistributionChannel = {
    #if SPIRITCHAT_APP_STORE
      return .appStore
    #else
      return .direct
    #endif
  }()

  /// Whether this build may grind proof-of-work for the `@username` ledger on
  /// the user's device.
  ///
  /// `false` for App Store builds. App Review guideline 2.4.2 forbids apps
  /// running unrelated background processes such as mining, and 3.1.5(b)
  /// allows mining only where "the processing is performed off device". The
  /// ledger still needs blocks, so the work moved rather than disappeared:
  /// standing relays (`packages/relay-node`, mining on by default) mine, and
  /// phones only submit claims and wait for confirmation. A user on an App
  /// Store build can still register a `@username`; the block that confirms it
  /// is simply produced by a machine that is plugged into a wall.
  ///
  /// Direct builds keep mining under the existing foreground-and-charging
  /// gate, so an installed base distributing through SideStore continues to
  /// contribute hash power to the same ledger.
  static var allowsOnDeviceMining: Bool {
    switch channel {
    case .appStore: return false
    case .direct: return true
    }
  }

  /// One line for the diagnostics screen / log, so which policy a given build
  /// is running under is answerable without a debugger.
  static var summary: String {
    switch channel {
    case .appStore: return "app-store (on-device mining disabled)"
    case .direct: return "direct (on-device mining enabled)"
    }
  }
}
