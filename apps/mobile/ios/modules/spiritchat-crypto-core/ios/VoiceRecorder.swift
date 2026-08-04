import AVFoundation
import Foundation

/// Records a voice message to a local AAC/.m4a file — the native half of
/// voice notes. Deliberately a plain singleton like `MiningController`
/// and `MixRelayController`: it owns nothing but the transient recording
/// session, and the encrypt/send of the finished file is entirely the
/// `ChatManager` media path's job (this only produces the plaintext file
/// on disk that `chatSendMedia` then encrypts chunk by chunk).
///
/// AAC in an .m4a container is chosen over Opus for one reason: it's what
/// iOS records and plays natively with no extra codec, so both the
/// recorder here and the player in the chat UI are first-party APIs. The
/// media layer treats the bytes as opaque either way.
final class VoiceRecorder {
  static let shared = VoiceRecorder()

  private let lock = NSLock()
  private var recorder: AVAudioRecorder?
  private var currentURL: URL?
  private var startedAt: Date?

  private init() {}

  enum RecorderError: Error {
    case permissionDenied
    case alreadyRecording
    case sessionSetupFailed(String)
  }

  /// Whether the microphone permission has already been granted — the JS
  /// side calls `requestPermission` first if this is false.
  var hasPermission: Bool {
    AVAudioSession.sharedInstance().recordPermission == .granted
  }

  /// Asks for microphone permission, resolving `true`/`false`. Wraps
  /// `AVAudioSession`'s callback API so the module can expose it as an
  /// async function to JS.
  func requestPermission(_ completion: @escaping (Bool) -> Void) {
    AVAudioSession.sharedInstance().requestRecordPermission { granted in
      completion(granted)
    }
  }

  /// Begins recording to a fresh temp file, returning its `file://` URL.
  /// Throws if permission isn't granted or a recording is already active.
  @discardableResult
  func start() throws -> String {
    lock.lock()
    defer { lock.unlock() }
    guard recorder == nil else { throw RecorderError.alreadyRecording }
    guard hasPermission else { throw RecorderError.permissionDenied }

    let session = AVAudioSession.sharedInstance()
    do {
      try session.setCategory(.playAndRecord, mode: .default, options: [.defaultToSpeaker])
      try session.setActive(true)
    } catch {
      throw RecorderError.sessionSetupFailed("\(error)")
    }

    let dir = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
    let url = dir.appendingPathComponent("voice-\(UUID().uuidString).m4a")
    let settings: [String: Any] = [
      AVFormatIDKey: Int(kAudioFormatMPEG4AAC),
      AVSampleRateKey: 44100.0,
      AVNumberOfChannelsKey: 1,
      AVEncoderAudioQualityKey: AVAudioQuality.medium.rawValue,
    ]

    let rec = try AVAudioRecorder(url: url, settings: settings)
    rec.record()
    recorder = rec
    currentURL = url
    startedAt = Date()
    return url.absoluteString
  }

  /// Finishes the current recording, returning its file URL and duration
  /// in milliseconds. `nil` if nothing was recording.
  func stop() -> [String: Any]? {
    lock.lock()
    defer { lock.unlock() }
    guard let rec = recorder, let url = currentURL, let started = startedAt else { return nil }
    rec.stop()
    let durationMs = Int(Date().timeIntervalSince(started) * 1000)
    recorder = nil
    currentURL = nil
    startedAt = nil
    try? AVAudioSession.sharedInstance().setActive(false, options: [.notifyOthersOnDeactivation])
    return ["fileUri": url.absoluteString, "durationMs": durationMs]
  }

  /// Aborts the current recording and deletes its file — for a
  /// swipe-to-cancel gesture.
  func cancel() {
    lock.lock()
    defer { lock.unlock() }
    recorder?.stop()
    if let url = currentURL { try? FileManager.default.removeItem(at: url) }
    recorder = nil
    currentURL = nil
    startedAt = nil
    try? AVAudioSession.sharedInstance().setActive(false, options: [.notifyOthersOnDeactivation])
  }
}
