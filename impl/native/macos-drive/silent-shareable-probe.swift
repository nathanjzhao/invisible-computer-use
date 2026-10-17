// Read-only discovery fixture for an explicitly owned disposable Mac.
// Does not capture pixels, request permission, activate an app, or send input.
// A normal enumerator is opt-in because that API can display consent UI.
import Foundation
import ScreenCaptureKit
import CoreGraphics
import Darwin

func report(_ object: [String: Any], exitCode: Int32 = 0) -> Never {
    let data = try! JSONSerialization.data(withJSONObject: object, options: [.sortedKeys, .prettyPrinted])
    print(String(data: data, encoding: .utf8)!)
    exit(exitCode)
}

let env = ProcessInfo.processInfo.environment
guard env["ICU_DISPOSABLE_HOST"] == "1",
      env["ICU_SILENT_CAPTURE_PROBE"] == "1",
      !FileManager.default.fileExists(atPath: "/Users/nathanzhao"),
      !(env["HOME"] ?? "").hasPrefix("/Users/nathanzhao") else {
    report(["status": "refused", "reason": "explicit disposable host required"], exitCode: 2)
}
let console = try FileManager.default.attributesOfItem(atPath: "/dev/console")
guard (console[.ownerAccountID] as? NSNumber)?.uint32Value == geteuid() else {
    report(["status": "refused", "reason": "run as disposable Mac console user"], exitCode: 2)
}
let baseline = CommandLine.arguments.contains("--prompting-baseline")
guard !baseline || env["ICU_ALLOW_PROMPTING_ENUMERATOR"] == "1" else {
    report(["status": "refused", "reason": "normal enumerator requires separate explicit opt-in"], exitCode: 2)
}
guard #available(macOS 14.4, *) else {
    report(["status": "unsupported", "reason": "macOS 14.4+ exact API required; no fallback"])
}

let targetWindow = env["ICU_PROBE_WINDOW_ID"].flatMap(UInt32.init)
let targetPid = env["ICU_PROBE_PID"].flatMap(Int32.init)
let lock = NSLock()
var answer: [String: Any]? = nil
let started = ProcessInfo.processInfo.systemUptime
let preflight = CGPreflightScreenCaptureAccess()
let completion: (SCShareableContent?, Error?) -> Void = { content, error in
    var result: [String: Any] = [
        "status": content == nil ? "unavailable" : "ok",
        "source": baseline ? "getShareableContent" : "getCurrentProcessShareableContent",
        "prompting_baseline": baseline,
        "os_version": ProcessInfo.processInfo.operatingSystemVersionString,
        "observer_pid": getpid(),
        "screen_capture_preflight": preflight,
        "elapsed_ms": (ProcessInfo.processInfo.systemUptime - started) * 1000,
        "capture_attempted": false,
        "fallback_attempted": false,
    ]
    if let content = content {
        // Keep output to routing facts, not document/window titles.
        result["windows"] = content.windows.map { window in
            ["window_id": window.windowID,
             "owner_pid": window.owningApplication?.processID as Any? ?? NSNull(),
             "title_present": window.title?.isEmpty == false,
             "on_screen": window.isOnScreen,
             "width": window.frame.width,
             "height": window.frame.height] as [String: Any]
        }
        result["display_ids"] = content.displays.map(\.displayID)
        if let window = targetWindow, let pid = targetPid {
            result["target"] = ["window_id": window, "pid": pid]
            result["foreign_target_present"] = content.windows.contains {
                $0.windowID == window && $0.owningApplication?.processID == pid && pid != getpid()
            }
        }
    }
    if let error = error { result["error"] = error.localizedDescription }
    lock.lock(); answer = result; lock.unlock()
}

// Exact API only. Unlike screencapturekit 6's compatibility wrapper, this
// branch cannot fall back to excludingDesktopWindows on an old runtime/SDK.
if baseline {
    SCShareableContent.getWithCompletionHandler(completion)
} else {
    SCShareableContent.getCurrentProcessShareableContent(completionHandler: completion)
}
while ProcessInfo.processInfo.systemUptime - started < 5 {
    lock.lock(); let result = answer; lock.unlock()
    if let result = result { report(result) }
    RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.01))
}
report(["status": "timed_out", "capture_attempted": false, "fallback_attempted": false], exitCode: 1)
