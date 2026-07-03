import Foundation

/// Content ids (blob hashes) cross the JS bridge as hex strings rather than
/// raw bytes — cheap to use as AsyncStorage keys and log/display, unlike a
/// `Uint8Array`.
extension Data {
  var hexEncoded: String {
    map { String(format: "%02x", $0) }.joined()
  }

  init?(hexEncoded hex: String) {
    guard hex.count % 2 == 0 else { return nil }
    var bytes = [UInt8]()
    bytes.reserveCapacity(hex.count / 2)
    var index = hex.startIndex
    while index < hex.endIndex {
      let next = hex.index(index, offsetBy: 2)
      guard let byte = UInt8(hex[index..<next], radix: 16) else { return nil }
      bytes.append(byte)
      index = next
    }
    self = Data(bytes)
  }
}
