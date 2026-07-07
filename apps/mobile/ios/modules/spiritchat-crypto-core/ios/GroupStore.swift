import Foundation

/// On-disk persistence for a group's key state — mirrors `ChatStore`'s
/// own per-peer session files exactly, just keyed by group id instead of
/// peer id. Backs both group schemes: legacy **Sender Keys** groups
/// (`ownSenderKeyStateBytes`/`receiverStates`) and **MLS/TreeKEM** groups
/// (`mlsStateBytes`). A session is MLS iff `mlsStateBytes != nil`; new
/// groups are always MLS, existing Sender Keys groups keep working
/// unchanged (their JSON simply has no `mlsStateBytes` key). See
/// `ChatManager`'s own doc comments for both schemes.
enum GroupStore {
  /// `groupId` is 16 random bytes, hex-encoded — generated once at
  /// creation and never changed after. `members` is every *other*
  /// current participant (never this device's own peer id) — what
  /// `ChatManager` fans a group message out to; mutable since membership
  /// can change after creation.
  ///
  /// The Sender Keys fields are optional so an MLS group's JSON needn't
  /// carry them (and vice versa — an old Sender Keys file decodes with a
  /// nil `mlsStateBytes`). Exactly one scheme's fields are populated per
  /// session.
  struct Session: Codable {
    let groupId: String
    let name: String
    var members: [String]

    // --- MLS (new groups) ---
    /// This member's serialized `FfiMlsGroup` state — as secret as a 1:1
    /// ratchet session (it holds this device's signing key and every
    /// private tree node it's entitled to). Present iff this is an MLS
    /// group.
    var mlsStateBytes: Data?

    // --- Sender Keys (legacy groups) ---
    var ownSenderKeyStateBytes: Data?
    /// memberPeerId -> that member's `FfiSenderKeyReceiverState` bytes.
    var receiverStates: [String: Data]?

    var isMls: Bool { mlsStateBytes != nil }
  }

  /// A group this device has been invited into (MLS) but not yet joined:
  /// it generated a leaf keypair and replied with a key package, and is
  /// waiting for the committer's Welcome. `leafSecret` is the private half
  /// of that leaf key (needed to open the Welcome); `name` is stashed from
  /// the invite request since the Welcome itself carries no app-level name.
  struct PendingJoin: Codable {
    let groupId: String
    let name: String
    let leafSecret: Data
  }

  /// A group content envelope not yet confirmed delivered to one
  /// specific member — mirrors `ChatStore.OutboxItem`'s own reasoning
  /// exactly, just fanned out per-member instead of per-conversation
  /// (one group message becomes one `GroupOutboxItem` per member, since
  /// each is delivered — and can fail/retry — independently). Durable
  /// the instant `sendGroupMessage` returns, same as the 1:1 outbox.
  struct OutboxItem: Codable {
    let localId: String
    let groupId: String
    let memberPeerId: String
    let wireEnvelope: Data
    let createdAt: Double
  }

  private static func groupsDirectory(slot: Int) -> URL {
    let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
    let dir = base
      .appendingPathComponent("Chats", isDirectory: true)
      .appendingPathComponent("\(slot)", isDirectory: true)
      .appendingPathComponent("Groups", isDirectory: true)
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir
  }

  private static func sessionPath(slot: Int, groupId: String) -> URL {
    groupsDirectory(slot: slot).appendingPathComponent("\(groupId).json")
  }

  static func loadSession(slot: Int, groupId: String) -> Session? {
    guard let data = try? Data(contentsOf: sessionPath(slot: slot, groupId: groupId)) else { return nil }
    return try? JSONDecoder().decode(Session.self, from: data)
  }

  static func saveSession(_ session: Session, slot: Int) throws {
    let data = try JSONEncoder().encode(session)
    try data.write(to: sessionPath(slot: slot, groupId: session.groupId), options: .atomic)
  }

  /// Every group this device currently participates in — what a future
  /// "your groups" list would read, and what `ChatManager` could use for
  /// its own bookkeeping if that's ever needed.
  static func loadAllSessions(slot: Int) -> [Session] {
    let dir = groupsDirectory(slot: slot)
    guard let files = try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil) else {
      return []
    }
    return files.compactMap { url in
      guard let data = try? Data(contentsOf: url) else { return nil }
      return try? JSONDecoder().decode(Session.self, from: data)
    }
  }

  private static func outboxPath(slot: Int) -> URL {
    // Deliberately alongside (not inside `Groups/`) ChatStore's own
    // `outbox.json` — a flat, whole-file store mirrors its exact
    // reasoning: small in practice, so reading/writing it whole on every
    // mutation is simpler than per-item files.
    let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
    let dir = base.appendingPathComponent("Chats", isDirectory: true).appendingPathComponent("\(slot)", isDirectory: true)
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir.appendingPathComponent("groupOutbox.json")
  }

  static func loadOutbox(slot: Int) -> [OutboxItem] {
    guard let data = try? Data(contentsOf: outboxPath(slot: slot)) else { return [] }
    return (try? JSONDecoder().decode([OutboxItem].self, from: data)) ?? []
  }

  static func saveOutbox(_ items: [OutboxItem], slot: Int) throws {
    let data = try JSONEncoder().encode(items)
    try data.write(to: outboxPath(slot: slot), options: .atomic)
  }

  // --- Pending MLS joins ------------------------------------------------

  private static func pendingDirectory(slot: Int) -> URL {
    let dir = groupsDirectory(slot: slot).appendingPathComponent("PendingJoins", isDirectory: true)
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir
  }

  private static func pendingPath(slot: Int, groupId: String) -> URL {
    pendingDirectory(slot: slot).appendingPathComponent("\(groupId).json")
  }

  static func loadPendingJoin(slot: Int, groupId: String) -> PendingJoin? {
    guard let data = try? Data(contentsOf: pendingPath(slot: slot, groupId: groupId)) else { return nil }
    return try? JSONDecoder().decode(PendingJoin.self, from: data)
  }

  static func savePendingJoin(_ pending: PendingJoin, slot: Int) throws {
    let data = try JSONEncoder().encode(pending)
    try data.write(to: pendingPath(slot: slot, groupId: pending.groupId), options: .atomic)
  }

  static func deletePendingJoin(slot: Int, groupId: String) {
    try? FileManager.default.removeItem(at: pendingPath(slot: slot, groupId: groupId))
  }
}
