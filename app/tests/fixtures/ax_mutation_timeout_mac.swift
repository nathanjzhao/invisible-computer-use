// Separate owned diagnostic. Never part of the measured model cohort.
// Compile locally; run only on an explicitly owned test desktop.
import Cocoa
import ApplicationServices

let arguments = CommandLine.arguments
func refuse(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8)); exit(2)
}
guard ProcessInfo.processInfo.environment["ICU_AX_TIMEOUT_DIAGNOSTIC"] == "1",
      ProcessInfo.processInfo.environment["ICU_DISPOSABLE_TEST_HOST"] == "1",
      arguments.count == 3 else { refuse("Set ICU_AX_TIMEOUT_DIAGNOSTIC=1 and ICU_DISPOSABLE_TEST_HOST=1; pass WORK_DIR receiver|probe") }
let work = URL(fileURLWithPath: arguments[1], isDirectory: true).standardizedFileURL
let consoleOwner = (try? FileManager.default.attributesOfItem(atPath: "/dev/console"))?[.ownerAccountID] as? NSNumber
guard NSUserName() != "nathanzhao", !NSHomeDirectory().hasPrefix("/Users/nathanzhao"),
      !work.path.hasPrefix("/Users/nathanzhao"), getuid() != 0,
      consoleOwner?.uint32Value == getuid() else {
    refuse("Diagnostic requires the disposable host's non-root console user and refuses Nathan's laptop paths")
}
try FileManager.default.createDirectory(at: work, withIntermediateDirectories: true)
func stamp() -> [String: Double] { ["unix": Date().timeIntervalSince1970, "monotonic": ProcessInfo.processInfo.systemUptime] }
func save(_ name: String, _ value: Any) {
    let data = try! JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    try! data.write(to: work.appendingPathComponent(name), options: .atomic)
}
func load(_ name: String) -> [String: Any]? {
    guard let data = try? Data(contentsOf: work.appendingPathComponent(name)) else { return nil }
    return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
}

var eventRows: [[String: Any]] = []
var counts = ["low": 0, "high": 0]
func record(_ row: [String: Any]) {
    eventRows.append(row)
    save("receiver-events.json", ["events": eventRows, "counts": counts])
}
final class SlowButton: NSButton {
    let trial: String
    init(_ trial: String, frame: NSRect) {
        self.trial = trial
        super.init(frame: frame)
        title = "Slow " + trial; bezelStyle = .rounded
        setAccessibilityIdentifier("icu-slow-" + trial)
        setAccessibilityLabel("Slow " + trial)
    }
    required init?(coder: NSCoder) { fatalError("unused") }
    override func accessibilityPerformPress() -> Bool {
        counts[trial, default: 0] += 1
        // Apply effect before the callback waits. A client timeout does not
        // undo it; the independent receiver count catches accidental replay.
        title = "Applied \(trial) \(counts[trial]!)"
        record(["trial": trial, "phase": "effect", "count": counts[trial]!,
                "active": NSApp.isActive, "key": window?.isKeyWindow ?? false, "time": stamp()])
        Thread.sleep(forTimeInterval: 0.35)
        record(["trial": trial, "phase": "return", "count": counts[trial]!,
                "active": NSApp.isActive, "key": window?.isKeyWindow ?? false, "time": stamp()])
        return true
    }
}

func attribute(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    var value: CFTypeRef?
    return AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success ? value : nil
}
func findButton(_ app: AXUIElement, _ identifier: String) -> AXUIElement? {
    guard let windows = attribute(app, kAXWindowsAttribute) as? [AXUIElement] else { return nil }
    var queue = windows.filter { attribute($0, kAXTitleAttribute) as? String == "ICU AX Timeout Diagnostic" }
    var visited = 0
    while !queue.isEmpty && visited < 100 {
        let element = queue.removeFirst(); visited += 1
        if attribute(element, kAXIdentifierAttribute) as? String == identifier { return element }
        queue.append(contentsOf: attribute(element, kAXChildrenAttribute) as? [AXUIElement] ?? [])
    }
    return nil
}

switch arguments[2] {
case "receiver":
    let app = NSApplication.shared
    app.setActivationPolicy(.accessory)
    let window = NSWindow(contentRect: NSRect(x: 500, y: 500, width: 340, height: 150),
                          styleMask: [.titled], backing: .buffered, defer: false)
    window.title = "ICU AX Timeout Diagnostic"
    window.isReleasedWhenClosed = false
    window.contentView!.addSubview(SlowButton("low", frame: NSRect(x: 20, y: 45, width: 140, height: 50)))
    window.contentView!.addSubview(SlowButton("high", frame: NSRect(x: 180, y: 45, width: 140, height: 50)))
    save("receiver-events.json", ["events": eventRows, "counts": counts])
    window.orderFrontRegardless()
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
        save("ready.json", ["pid": ProcessInfo.processInfo.processIdentifier, "window": window.windowNumber,
                            "callback_delay_seconds": 0.35, "time": stamp()])
    }
    DispatchQueue.main.asyncAfter(deadline: .now() + 90) { app.terminate(nil) }
    withExtendedLifetime(window) { app.run() }
case "probe":
    guard AXIsProcessTrusted() else { refuse("Diagnostic probe has no Accessibility grant; no permission prompt requested") }
    guard let ready = load("ready.json"), let pid = ready["pid"] as? Int32 else { refuse("Owned receiver ready.json missing") }
    var trials: [[String: Any]] = []
    for trial in ["low", "high"] {
        // Each trial uses a fresh object graph and a distinct owned button.
        let app = AXUIElementCreateApplication(pid)
        guard let button = findButton(app, "icu-slow-" + trial) else { refuse("Owned diagnostic button missing") }
        let readTimeout = AXUIElementSetMessagingTimeout(button, 0.15)
        guard readTimeout == .success else { refuse("Could not set prior query timeout") }
        guard attribute(button, kAXRoleAttribute) as? String == kAXButtonRole else { refuse("Owned control role mismatch") }
        let mutationTimeout: Float = trial == "high" ? 2.0 : 0.15
        let configure = trial == "high" ? AXUIElementSetMessagingTimeout(button, mutationTimeout) : .success
        guard configure == .success else { refuse("Mutation timeout setup failed; no action dispatched") }
        let started = stamp()
        let error = AXUIElementPerformAction(button, kAXPressAction as CFString) // exactly one dispatch
        let ended = stamp()
        trials.append(["trial": trial, "mutation_timeout_seconds": mutationTimeout,
                       "prior_query_timeout_seconds": 0.15, "dispatch_count": 1,
                       "ax_error": error.rawValue, "started": started, "ended": ended,
                       "elapsed_seconds": ended["monotonic"]! - started["monotonic"]!])
        Thread.sleep(forTimeInterval: 0.6) // let the timed-out receiver finish before the next independent trial
    }
    let receiver = load("receiver-events.json") ?? [:]
    let result: [String: Any] = ["diagnostic_only": true, "receiver_pid": pid,
                               "trials": trials, "receiver": receiver]
    save("diagnostic-result.json", result)
    let data = try JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
    print(String(decoding: data, as: UTF8.self))
default: refuse("Expected receiver or probe")
}
