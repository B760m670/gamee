import Foundation

/// On-disk persistence for `ChatManager`: each contact's Double Ratchet
/// session state, and the outbox of not-yet-delivered plaintext messages.
/// Unlike `BlobStore` (content-addressed, safe to share across every
/// account on the device) or the `@username` ledger (deliberately one
/// shared file — see `P2pSession.ledgerDatabasePath`), a conversation is
/// private per-account data, so everything here lives under a
/// slot-namespaced subdirectory — switching accounts can never mix one
/// account's sessions/outbox into another's.
enum ChatStore {
  /// A contact's live ratchet state, keyed by their PeerId (not
  /// fingerprint — `ChatManager` only ever has a PeerId on hand when an
  /// envelope arrives). `peerPublicKey` rides alongside it because only the
  /// *first* incoming envelope of a conversation carries the sender's
  /// identity key (inside the X3DH `InitialMessage`) — every later one is
  /// just ratchet ciphertext, so it has to be remembered from here on to
  /// keep answering "whose conversation is this" (fingerprint, display).
  struct Session: Codable {
    let peerId: String
    let peerPublicKey: Data
    let ratchetBytes: Data
  }

  /// A plaintext message not yet handed to the transport successfully —
  /// created the instant `ChatManager.sendMessage` is called (so sending
  /// works immediately even while offline/disconnected) and removed only
  /// once `P2pEvent.EnvelopeDelivered` confirms it. `peerPublicKey` is
  /// carried here (not just `peerId`) because establishing a *first*
  /// session with this peer needs it for X3DH, and nothing else on this
  /// device may have it cached yet.
  struct OutboxItem: Codable {
    let localId: String
    let peerId: String
    let peerPublicKey: Data
    let plaintext: Data
    let createdAt: Double
  }

  private static func chatsDirectory(slot: Int) -> URL {
    let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
    let dir = base
      .appendingPathComponent("Chats", isDirectory: true)
      .appendingPathComponent("\(slot)", isDirectory: true)
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir
  }

  private static func sessionsDirectory(slot: Int) -> URL {
    let dir = chatsDirectory(slot: slot).appendingPathComponent("Sessions", isDirectory: true)
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir
  }

  private static func sessionPath(slot: Int, peerId: String) -> URL {
    sessionsDirectory(slot: slot).appendingPathComponent("\(peerId).json")
  }

  static func loadSession(slot: Int, peerId: String) -> Session? {
    guard let data = try? Data(contentsOf: sessionPath(slot: slot, peerId: peerId)) else { return nil }
    return try? JSONDecoder().decode(Session.self, from: data)
  }

  static func saveSession(_ session: Session, slot: Int) throws {
    let data = try JSONEncoder().encode(session)
    try data.write(to: sessionPath(slot: slot, peerId: session.peerId), options: .atomic)
  }

  /// Every contact this device currently has a live ratchet session with —
  /// what a periodic mailbox-retrieval sweep iterates (there's no separate
  /// "conversations" list on this side; a session file *is* the record of
  /// a conversation existing) and what a mailbox-retrieved envelope with no
  /// attached sender identity gets matched against by trying each one's
  /// ratchet in turn.
  static func loadAllSessions(slot: Int) -> [Session] {
    let dir = sessionsDirectory(slot: slot)
    guard let files = try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil) else {
      return []
    }
    return files.compactMap { url in
      guard let data = try? Data(contentsOf: url) else { return nil }
      return try? JSONDecoder().decode(Session.self, from: data)
    }
  }

  private static func outboxPath(slot: Int) -> URL {
    chatsDirectory(slot: slot).appendingPathComponent("outbox.json")
  }

  /// The whole outbox, across every contact — small in practice (each
  /// entry is one not-yet-delivered message), so reading/writing it whole
  /// on every mutation is simpler than a real per-message store and avoids
  /// building index machinery this doesn't need yet.
  static func loadOutbox(slot: Int) -> [OutboxItem] {
    guard let data = try? Data(contentsOf: outboxPath(slot: slot)) else { return [] }
    return (try? JSONDecoder().decode([OutboxItem].self, from: data)) ?? []
  }

  static func saveOutbox(_ items: [OutboxItem], slot: Int) throws {
    let data = try JSONEncoder().encode(items)
    try data.write(to: outboxPath(slot: slot), options: .atomic)
  }
}
