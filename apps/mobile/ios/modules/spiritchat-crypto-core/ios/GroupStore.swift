import Foundation

/// On-disk persistence for a group's Sender Key state — mirrors
/// `ChatStore`'s own per-peer session files exactly, just keyed by group
/// id instead of peer id. See `ChatManager`'s own doc comment for the
/// scheme this backs (Sender Keys, distributed over existing pairwise
/// sessions).
enum GroupStore {
  /// `groupId` is 16 random bytes, hex-encoded — generated once at
  /// creation and never changed after. `members` is every *other*
  /// current participant (never this device's own peer id) — what
  /// `ChatManager` fans a group message out to, and what a new member's
  /// own distribution needs to reach; mutable since membership can
  /// change after creation (see `ChatManager.addGroupMember`/
  /// `removeGroupMember`). `ownSenderKeyStateBytes` is this device's own
  /// outgoing chain for this group — as secret as any other signing key,
  /// since it's what proves this device's own authorship of every
  /// message it sends here; replaced outright (not just advanced) when
  /// a member is removed, for forward secrecy against them.
  struct Session: Codable {
    let groupId: String
    let name: String
    var members: [String]
    var ownSenderKeyStateBytes: Data
    /// memberPeerId -> that member's `FfiSenderKeyReceiverState` bytes,
    /// as currently known — populated as each member's distribution
    /// arrives (see `ChatManager.handleGroupMemberDistribution`), so a
    /// brand new group starts with this empty even though `members`
    /// already lists everyone.
    var receiverStates: [String: Data]
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
}
