import Foundation
import UIKit

/// Gates this device's participation in mining the `@username` ledger on
/// two conditions, both required: the app must be in the foreground, and
/// the device must be charging. Mining is real, sustained CPU work with no
/// built-in throttle beyond periodically checking a stop flag (see
/// `spiritchat_p2p_core::node`'s mining loop) — running it unconditionally
/// in the background or on battery would silently drain the device for a
/// feature most users never explicitly opted into, and iOS would kill a
/// background CPU-bound task anyway. Replaces the earlier debug-triggered
/// `p2pStartMining`/`p2pStopMining` calls with this automatic, device-state-
/// driven gate — the "real" mining policy the project's phased ledger plan
/// calls for.
///
/// Sent immediately on every relevant transition, deliberately with no
/// debounce: a user unplugging mid-mine or backgrounding the app should
/// stop consuming CPU/battery right away, not after some grace window.
final class MiningController {
  static let shared = MiningController()

  private var isObserving = false
  private var isMining = false

  private init() {}

  /// Starts observing app/battery state and immediately applies the
  /// current one. Call once a `P2pSession` (and therefore a node to gate)
  /// exists — there's nothing to start/stop before that. Safe to call more
  /// than once; only the first call attaches observers.
  func start() {
    guard !isObserving else { return }
    isObserving = true

    UIDevice.current.isBatteryMonitoringEnabled = true

    let center = NotificationCenter.default
    center.addObserver(self, selector: #selector(reevaluate), name: UIApplication.willEnterForegroundNotification, object: nil)
    center.addObserver(self, selector: #selector(reevaluate), name: UIApplication.didEnterBackgroundNotification, object: nil)
    center.addObserver(self, selector: #selector(reevaluate), name: UIDevice.batteryStateDidChangeNotification, object: nil)

    reevaluate()
  }

  /// Stops mining (if active) and forgets observation state — for sign
  /// out, where the `P2pSession` this was gating is about to shut down and
  /// be replaced (or not replaced at all, if the user lands back on
  /// onboarding). `start()` re-attaches everything once the next session
  /// exists.
  func stop() {
    guard isObserving else { return }
    NotificationCenter.default.removeObserver(self)
    isObserving = false
    if isMining {
      try? P2pSession.shared?.node.stopMining()
      isMining = false
    }
  }

  @objc private func reevaluate() {
    guard let session = P2pSession.shared, let identity = IdentitySession.shared else { return }

    let isForeground = UIApplication.shared.applicationState == .active
    let isCharging = UIDevice.current.batteryState == .charging || UIDevice.current.batteryState == .full
    let shouldMine = isForeground && isCharging

    if shouldMine && !isMining {
      try? session.node.startMining(publicKey: identity.publicKeyBytes)
      isMining = true
    } else if !shouldMine && isMining {
      try? session.node.stopMining()
      isMining = false
    }
  }
}
