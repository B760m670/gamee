import Foundation
import Security

enum KeychainError: Error {
  case saveFailed(OSStatus)
  case loadFailed(OSStatus)
}

/// Thin wrapper around the iOS Keychain for storing small secret byte
/// blobs (identity/agreement keys, the prekey store). Items are scoped to
/// this device only and unreadable until the device has been unlocked at
/// least once since boot — appropriate for private key material the app
/// needs available in the background, but that should never leave the
/// device or be readable before the user has unlocked it even once.
enum KeychainStore {
  private static let service = "com.spiritchat.cryptocore.identity"

  static func save(_ data: Data, account: String) throws {
    let baseQuery: [String: Any] = [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: service,
      kSecAttrAccount as String: account,
    ]
    // Overwriting via SecItemUpdate requires the item to already exist
    // with matching attributes; deleting first and re-adding is simpler
    // and this data is never large enough for that to matter.
    SecItemDelete(baseQuery as CFDictionary)

    var attributes = baseQuery
    attributes[kSecValueData as String] = data
    attributes[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
    let status = SecItemAdd(attributes as CFDictionary, nil)
    guard status == errSecSuccess else {
      throw KeychainError.saveFailed(status)
    }
  }

  /// Returns `nil` if there is no item for `account` yet; throws for any
  /// other failure.
  static func load(account: String) throws -> Data? {
    let query: [String: Any] = [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: service,
      kSecAttrAccount as String: account,
      kSecReturnData as String: true,
      kSecMatchLimit as String: kSecMatchLimitOne,
    ]
    var result: AnyObject?
    let status = SecItemCopyMatching(query as CFDictionary, &result)
    if status == errSecItemNotFound {
      return nil
    }
    guard status == errSecSuccess else {
      throw KeychainError.loadFailed(status)
    }
    return result as? Data
  }
}
