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

  /// Runs `work` on the main thread — inline if already there, otherwise
  /// synchronously dispatched. `UIApplication`/`UIDevice` are main-thread-
  /// only APIs, and callers of `start()`/`stop()` (a plain `Task { ... }`
  /// in `SpiritchatCryptoCoreModule`'s `OnCreate`, which runs on a
  /// background executor, not the main actor; the `signOut` `Function`
  /// closure, whose thread this module makes no assumption about) can't be
  /// trusted to already be on it. Synchronous rather than `.async`: `stop()`
  /// is called right before `P2pSession.signOut()`/`IdentitySession.removeSlot`
  /// wipe the very session it needs to send `stopMining` to — an async hop
  /// would race that wipe and could silently no-op, leaving a mining
  /// attempt to keep grinding after sign-out instead of actually stopping.
  private func onMainThread(_ work: @escaping () -> Void) {
    if Thread.isMainThread {
      work()
    } else {
      DispatchQueue.main.sync(execute: work)
    }
  }

  /// Starts observing app/battery state and immediately applies the
  /// current one. Call once a `P2pSession` (and therefore a node to gate)
  /// exists — there's nothing to start/stop before that. Safe to call more
  /// than once; only the first call attaches observers.
  func start() {
    // An App Store build never mines on device (see `DistributionPolicy`);
    // bail before touching battery monitoring or notifications, so such a
    // build doesn't even observe the state it would have mined on.
    guard DistributionPolicy.allowsOnDeviceMining else { return }
    onMainThread { self.startOnMainThread() }
  }

  private func startOnMainThread() {
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
  /// exists. Safe to call from any thread — see `onMainThread`'s doc
  /// comment. Returns only after mining has actually been asked to stop,
  /// so a caller about to tear down the session it gates (see `signOut`)
  /// can rely on this having happened first.
  func stop() {
    onMainThread { self.stopOnMainThread() }
  }

  private func stopOnMainThread() {
    guard isObserving else { return }
    NotificationCenter.default.removeObserver(self)
    isObserving = false
    if isMining {
      try? P2pSession.shared?.node.stopMining()
      isMining = false
    }
  }

  @objc private func reevaluate() {
    // Defence in depth: `start()` already refuses to attach the observers
    // that call this, so on an App Store build nothing should reach here —
    // but this is the single place that actually issues `startMining`, and
    // it is worth the policy being re-checked at the point of the act rather
    // than only at the point of subscription.
    guard DistributionPolicy.allowsOnDeviceMining else { return }
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
