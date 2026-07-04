import Foundation

/// On-disk persistence for a group's Sender Key state — mirrors
/// `ChatStore`'s own per-peer session files exactly, just keyed by group
/// id instead of peer id. See `ChatManager`'s own doc comment for the
/// scheme this backs (Sender Keys, distributed over existing pairwise
/// sessions) and its current, deliberately v1 scope: creation with an
/// initial member list only, no add/remove-after-creation yet.
enum GroupStore {
  /// `groupId` is 16 random bytes, hex-encoded — generated once by
  /// whoever creates the group and never changed after. `members` is
  /// every *other* participant (never this device's own peer id) —
  /// what `ChatManager` fans a group message out to, and what a new
  /// member's own distribution needs to reach. `ownSenderKeyStateBytes`
  /// is this device's own outgoing chain for this group — as secret as
  /// any other signing key, since it's what proves this device's own
  /// authorship of every message it sends here.
  struct Session: Codable {
    let groupId: String
    let name: String
    let members: [String]
    var ownSenderKeyStateBytes: Data
    /// memberPeerId -> that member's `FfiSenderKeyReceiverState` bytes,
    /// as currently known — populated as each member's distribution
    /// arrives (see `ChatManager.handleGroupMemberDistribution`), so a
    /// brand new group starts with this empty even though `members`
    /// already lists everyone.
    var receiverStates: [String: Data]
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
}
