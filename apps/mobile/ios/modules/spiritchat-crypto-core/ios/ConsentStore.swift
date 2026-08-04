import Foundation

/// Who this account has decided not to hear from, and how strongly.
///
/// Phase 1 of `docs/consent-and-moderation.md`: the purely local half, which
/// needs no protocol change and no agreement with anyone. Nothing here is
/// published, gossiped, or written to the ledger — a block is a statement
/// this device makes to itself about what it will accept, and it is nobody
/// else's business that it was made.
///
/// Deliberately **native-side and keyed by `PeerId`**, for one reason that
/// drives the whole design: enforcement has to happen in
/// `ChatManager.onEnvelopeReceived`, which runs before any decryption and
/// often before JavaScript is even running. A block list living in the JS
/// layer could only ever filter what had already been decrypted and
/// delivered — which is not blocking, it is hiding. A `PeerId` is derived
/// from the identity public key (see `p2p-core`'s `peer_id_from_public_key`),
/// so keying by it is the same as keying by identity, and it is the only
/// name available at that point in the pipeline.
///
/// Per account slot, since two accounts on one device must never see each
/// other's decisions — the same reasoning behind per-slot ledger files and
/// media directories.
final class ConsentStore {
  /// How much of a peer this account is willing to accept.
  enum Stance: String, Codable {
    /// Envelopes are dropped unopened. The peer is told nothing — there is
    /// no "you have been blocked" signal in this protocol, deliberately:
    /// telling someone they were blocked is a message, and a blocked person
    /// is exactly who you do not want to be able to make you send one.
    case blocked
    /// Accepted and decrypted as normal, but never surfaces a notification
    /// and never raises the conversation in the list. For the case where
    /// severing contact would cause more trouble than absorbing it.
    case restricted
  }

  private let slot: Int
  private let lock = NSLock()
  private var stances: [String: Stance]

  init(slot: Int) {
    self.slot = slot
    self.stances = Self.load(slot: slot)
  }

  // MARK: - Queries

  /// The hot path: called for every inbound envelope before anything is
  /// parsed or decrypted, so it must stay a dictionary lookup under a lock
  /// and nothing more.
  func isBlocked(_ peerId: String) -> Bool {
    lock.lock()
    defer { lock.unlock() }
    return stances[peerId] == .blocked
  }

  func isRestricted(_ peerId: String) -> Bool {
    lock.lock()
    defer { lock.unlock() }
    return stances[peerId] == .restricted
  }

  func stance(for peerId: String) -> Stance? {
    lock.lock()
    defer { lock.unlock() }
    return stances[peerId]
  }

  /// Every peer with a stance, for the settings screen. Sorted so the list
  /// doesn't reshuffle between reads.
  func all() -> [(peerId: String, stance: Stance)] {
    lock.lock()
    defer { lock.unlock() }
    return stances
      .map { (peerId: $0.key, stance: $0.value) }
      .sorted { $0.peerId < $1.peerId }
  }

  // MARK: - Mutations

  /// Sets (or replaces) a peer's stance. Persisted immediately rather than
  /// on a timer: a user who blocks someone and force-quits the app in the
  /// same second must not find them unblocked on relaunch.
  func set(_ stance: Stance, for peerId: String) {
    lock.lock()
    stances[peerId] = stance
    let snapshot = stances
    lock.unlock()
    Self.persist(snapshot, slot: slot)
  }

  /// Clears any stance — unblock, or lift a restriction.
  func clear(_ peerId: String) {
    lock.lock()
    stances.removeValue(forKey: peerId)
    let snapshot = stances
    lock.unlock()
    Self.persist(snapshot, slot: slot)
  }

  // MARK: - Persistence

  private static func fileURL(slot: Int) -> URL {
    let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
    let dir = base
      .appendingPathComponent("Chats", isDirectory: true)
      .appendingPathComponent("\(slot)", isDirectory: true)
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir.appendingPathComponent("consent.json")
  }

  private static func load(slot: Int) -> [String: Stance] {
    guard
      let data = try? Data(contentsOf: fileURL(slot: slot)),
      let decoded = try? JSONDecoder().decode([String: Stance].self, from: data)
    else {
      // Missing file is the normal first-run case. A corrupt one is treated
      // the same way — starting empty is the safe direction here, since the
      // alternative is refusing to start a session over a settings file.
      return [:]
    }
    return decoded
  }

  private static func persist(_ stances: [String: Stance], slot: Int) {
    guard let data = try? JSONEncoder().encode(stances) else { return }
    // `.completeFileProtection` matches how the rest of this account's data
    // is held: readable only while the device is unlocked.
    try? data.write(to: fileURL(slot: slot), options: [.atomic, .completeFileProtection])
  }

  /// Wipes this slot's decisions — for `IdentitySession.removeSlot`, where
  /// leaving a block list behind would let the next account created in the
  /// same slot inherit a stranger's choices.
  static func removeAll(slot: Int) {
    try? FileManager.default.removeItem(at: fileURL(slot: slot))
  }
}
