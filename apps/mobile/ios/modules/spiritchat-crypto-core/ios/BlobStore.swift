import Foundation

/// On-disk cache for content-addressed blobs (avatars today; any future
/// P2P-fetched content later) — keyed by the same hash `blobContentId`
/// produces, so a file already on disk never needs re-downloading, and
/// `P2pSession` can re-register this device's own blobs with the P2P node
/// after every relaunch (the Rust node itself keeps nothing on disk).
enum BlobStore {
  private static var directory: URL {
    let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
    let dir = base.appendingPathComponent("Blobs", isDirectory: true)
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir
  }

  static func path(for idHex: String) -> URL {
    directory.appendingPathComponent(idHex)
  }

  @discardableResult
  static func save(_ bytes: Data, idHex: String) throws -> URL {
    let url = path(for: idHex)
    try bytes.write(to: url, options: .atomic)
    return url
  }

  static func load(idHex: String) -> Data? {
    try? Data(contentsOf: path(for: idHex))
  }

  static func exists(idHex: String) -> Bool {
    FileManager.default.fileExists(atPath: path(for: idHex).path)
  }

  static func remove(idHex: String) {
    try? FileManager.default.removeItem(at: path(for: idHex))
  }
}
