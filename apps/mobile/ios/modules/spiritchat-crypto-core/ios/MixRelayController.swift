import Foundation
import UIKit

/// Gates this device's *active* participation in the Sphinx/Loopix mixnet
/// — announcing itself as a usable relay and generating sustained dummy
/// (cover/loop) traffic — on the same two conditions `MiningController`
/// already uses: the app must be in the foreground, and the device must
/// be charging. Passive forwarding (relaying an already-built packet for
/// whoever routes traffic through this node) needs no gate at all — it's
/// cheap, event-driven, and only ever happens when asked — so this class
/// only concerns itself with the parts of mixnet participation that are
/// an ongoing cost this device pays on its own initiative.
///
/// Layered on top of the foreground+charging gate is `participationEnabled`,
/// a user-facing Settings toggle (default on) persisted in `UserDefaults` —
/// unlike mining, which has no opt-out beyond simply not charging the
/// device, relaying other people's traffic is a privacy favor to the
/// network this device's owner should be able to decline outright.
final class MixRelayController {
  static let shared = MixRelayController()

  private static let participationEnabledKey = "spiritchat.mixRelayParticipationEnabled"

  private var isObserving = false
  private var isActive = false

  private init() {}

  /// Whether this device offers to relay/announce for the mixnet at all —
  /// defaults to on (absent key reads as `true`), matching the project's
  /// existing "mix-relay participation is opt-in, but the default favors
  /// the network" stance from `Command::AnnounceMixRelay`'s own doc
  /// comment. Setting this re-evaluates immediately, so toggling it in
  /// Settings takes effect without waiting for a foreground/charging
  /// transition.
  var participationEnabled: Bool {
    get {
      if UserDefaults.standard.object(forKey: Self.participationEnabledKey) == nil { return true }
      return UserDefaults.standard.bool(forKey: Self.participationEnabledKey)
    }
    set {
      UserDefaults.standard.set(newValue, forKey: Self.participationEnabledKey)
      onMainThread { self.reevaluate() }
    }
  }

  /// Runs `work` on the main thread — see `MiningController.onMainThread`
  /// for why this is synchronous rather than `.async`: callers include the
  /// same off-main-actor `OnCreate` polling loop and the `signOut` path,
  /// where a queued async hop could race a session teardown.
  private func onMainThread(_ work: @escaping () -> Void) {
    if Thread.isMainThread {
      work()
    } else {
      DispatchQueue.main.sync(execute: work)
    }
  }

  /// Starts observing app/battery state and immediately applies the
  /// current one. Call once a `P2pSession` exists — mirrors
  /// `MiningController.start()` exactly. Safe to call more than once.
  func start() {
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

  /// Deactivates (if active) and forgets observation state — for sign out
  /// / account switch, where the `P2pSession` this was gating is about to
  /// shut down. `start()` re-attaches everything for the next session.
  func stop() {
    onMainThread { self.stopOnMainThread() }
  }

  private func stopOnMainThread() {
    guard isObserving else { return }
    NotificationCenter.default.removeObserver(self)
    isObserving = false
    if isActive {
      try? P2pSession.shared?.node.setMixDummyTrafficActive(enabled: false)
      isActive = false
    }
  }

  @objc private func reevaluate() {
    guard let session = P2pSession.shared else { return }

    let isForeground = UIApplication.shared.applicationState == .active
    let isCharging = UIDevice.current.batteryState == .charging || UIDevice.current.batteryState == .full
    let shouldParticipate = participationEnabled && isForeground && isCharging

    if shouldParticipate && !isActive {
      // Re-announcing on every activation (rather than once ever) is
      // deliberate: it's a cheap gossip publish, and it means a relay
      // that's been offline rejoins the discoverable directory the next
      // time conditions allow it to actually pull its weight, without
      // needing any separate "did I already announce" bookkeeping here.
      try? session.node.announceMixRelay()
      try? session.node.setMixDummyTrafficActive(enabled: true)
      isActive = true
    } else if !shouldParticipate && isActive {
      try? session.node.setMixDummyTrafficActive(enabled: false)
      isActive = false
    }
  }
}
