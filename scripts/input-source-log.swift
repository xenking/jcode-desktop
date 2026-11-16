import AppKit
import Carbon
import Foundation
import Darwin

// Never record typed text. O_APPEND makes each short line atomic across the
// agent, Desktop, and the user's marker command.
let home = FileManager.default.homeDirectoryForCurrentUser
let logDir = home.appendingPathComponent(".jcode/logs", isDirectory: true)
try FileManager.default.createDirectory(at: logDir, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
let fd = open(logDir.appendingPathComponent("input-source.log").path, O_WRONLY | O_CREAT | O_APPEND, 0o600)
guard fd >= 0 else { fatalError("cannot open input-source.log") }
let formatter = ISO8601DateFormatter()
formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
func log(_ event: String) {
    let line = "\(formatter.string(from: Date())) pid=\(getpid()) \(event)\n"
    _ = line.withCString { write(fd, $0, strlen($0)) }
}
if CommandLine.arguments.dropFirst().first == "--mark" {
    log("marker=repro")
    exit(0)
}
func sourceID() -> String {
    guard let source = TISCopyCurrentKeyboardInputSource()?.takeRetainedValue(),
          let property = TISGetInputSourceProperty(source, kTISPropertyInputSourceID) else { return "unknown" }
    return Unmanaged<CFString>.fromOpaque(property).takeUnretainedValue() as String
}
func recordSource(_ event: String) {
    let app = NSWorkspace.shared.frontmostApplication
    let name = (app?.bundleIdentifier ?? "unknown").replacingOccurrences(of: " ", with: "_")
    log("\(event) source=\(sourceID()) frontmost=\(name) frontmost_pid=\(app?.processIdentifier ?? 0)")
}
let callback: CFNotificationCallback = { _, _, _, _, _ in recordSource("input-source-changed") }
CFNotificationCenterAddObserver(CFNotificationCenterGetDistributedCenter(), nil, callback,
    kTISNotifySelectedKeyboardInputSourceChanged, nil, .deliverImmediately)
recordSource("recorder-started")
RunLoop.main.run()
