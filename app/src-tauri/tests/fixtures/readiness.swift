// Owned disposable-desktop fixture. No role or busy metadata is fabricated:
// NSProgressIndicator supplies its ordinary AppKit accessibility representation.
// Usage: readiness <fresh-output-directory> delayed|permanent|noeffect
import Cocoa

let environment = ProcessInfo.processInfo.environment
guard environment["ICU_DISPOSABLE_HOST"] == "1",
      environment["ICU_E2E_NATIVE_READINESS"] == "1",
      !FileManager.default.fileExists(atPath: "/Users/nathanzhao") else {
    fatalError("explicit disposable Mac required; never run on Nathan's computer")
}
let arguments = CommandLine.arguments
guard arguments.count == 3, ["delayed", "permanent", "noeffect"].contains(arguments[2]) else {
    fatalError("expected output directory and delayed|permanent|noeffect")
}
let output = URL(fileURLWithPath: arguments[1], isDirectory: true)
guard let privateRoot = environment["ICU_E2E_NATIVE_READINESS_WORKDIR"],
      output.resolvingSymlinksInPath().path.hasPrefix(
        URL(fileURLWithPath: privateRoot).resolvingSymlinksInPath().path + "/") else {
    fatalError("fixture output must belong to the explicit private work directory")
}
let mode = arguments[2]
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let activity = ProcessInfo.processInfo.beginActivity(
    options: [.userInitiated, .idleSystemSleepDisabled], reason: "readiness native acceptance")

func now() -> Double { Date().timeIntervalSince1970 }
func writeJSON(_ name: String, _ value: Any) {
    let bytes = try! JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    try! bytes.write(to: output.appendingPathComponent(name), options: .atomic)
}

final class ReadinessFixture: NSObject {
    let target: NSWindow
    let decoy: NSWindow
    let result = NSTextField(frame: NSRect(x: 20, y: 125, width: 200, height: 30))
    let spinner = NSProgressIndicator(frame: NSRect(x: 250, y: 125, width: 28, height: 28))
    var targetEffects = 0
    var decoyEffects = 0
    var busy = false
    var started: Double? = nil
    var completed: Double? = nil
    var events: [[String: Any]] = []
    var foregroundPids = Set<Int32>()
    var everActive = false
    var timer: Timer?

    override init() {
        func window(_ title: String, _ x: CGFloat) -> NSWindow {
            let w = NSWindow(contentRect: NSRect(x: x, y: 80, width: 340, height: 210),
                styleMask: [.titled, .resizable], backing: .buffered, defer: false)
            w.title = title
            w.isReleasedWhenClosed = false
            return w
        }
        target = window("ICU Readiness Target", 80)
        decoy = window("ICU Readiness Decoy", 460)
        super.init()
        result.stringValue = "0"
        result.isEditable = true
        result.setAccessibilityLabel("Result")
        target.contentView!.addSubview(result)
        spinner.style = .spinning
        spinner.isIndeterminate = true
        spinner.isDisplayedWhenStopped = false
        spinner.setAccessibilityLabel("Loading result")
        spinner.setAccessibilityIdentifier("readiness-spinner")

        for w in [target, decoy] {
            let button = NSButton(frame: NSRect(x: 20, y: 40, width: 180, height: 36))
            button.title = "Compute"
            button.bezelStyle = .rounded
            button.target = self
            button.action = #selector(compute(_:))
            w.contentView!.addSubview(button)
        }
        let decoyResult = NSTextField(frame: NSRect(x: 20, y: 125, width: 200, height: 30))
        decoyResult.stringValue = "decoy unchanged"
        decoyResult.isEditable = true
        decoyResult.setAccessibilityLabel("Result")
        decoy.contentView!.addSubview(decoyResult)
        // Showing owned fixture windows is permitted only on a disposable host.
        // Neither window is made key and the accessory app is never activated.
        decoy.orderFrontRegardless()
        target.orderFrontRegardless()
        sampleOwnership()
        record("ready")
        writeJSON("ready.json", ["pid": ProcessInfo.processInfo.processIdentifier,
            "target_window_id": target.windowNumber, "decoy_window_id": decoy.windowNumber,
            "front_pid": NSWorkspace.shared.frontmostApplication?.processIdentifier ?? -1])
        timer = Timer.scheduledTimer(withTimeInterval: 0.02, repeats: true) { [weak self] _ in
            self?.sampleOwnership()
        }
        Timer.scheduledTimer(withTimeInterval: 45, repeats: false) { _ in NSApp.terminate(nil) }
    }

    func sampleOwnership() {
        let previousActive = everActive
        let previousCount = foregroundPids.count
        everActive = everActive || NSApp.isActive
        foregroundPids.insert(NSWorkspace.shared.frontmostApplication?.processIdentifier ?? -1)
        if previousActive != everActive || previousCount != foregroundPids.count { save() }
    }

    func save() {
        var state: [String: Any] = ["mode": mode, "result": result.stringValue,
            "busy": busy, "target_effects": targetEffects, "decoy_effects": decoyEffects,
            "ever_active": everActive, "foreground_pids": foregroundPids.sorted(), "events": events]
        if let started = started { state["started_at"] = started }
        if let completed = completed { state["completed_at"] = completed }
        writeJSON("state.json", state)
    }

    func record(_ kind: String) {
        events.append(["kind": kind, "at": now(), "target_effects": targetEffects,
            "decoy_effects": decoyEffects, "result": result.stringValue,
            "active": NSApp.isActive, "front_pid": NSWorkspace.shared.frontmostApplication?.processIdentifier ?? -1])
        save()
    }

    @objc func compute(_ sender: NSButton) {
        if sender.window === decoy {
            decoyEffects += 1
            record("decoy_compute")
            return
        }
        targetEffects += 1
        started = now()
        if mode == "noeffect" {
            // Record delivery out of band; leave every exposed control unchanged.
            record("target_noeffect")
            return
        }
        busy = true
        result.stringValue = "Working"
        target.contentView!.addSubview(spinner)
        spinner.startAnimation(nil)
        record("target_started")
        if mode == "delayed" {
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) { [weak self] in
                guard let self = self else { return }
                self.spinner.stopAnimation(nil)
                self.spinner.removeFromSuperview()
                self.result.stringValue = "42"
                self.busy = false
                self.completed = now()
                self.record("target_completed")
            }
        }
    }
}

let fixture = ReadinessFixture()
withExtendedLifetime((activity, fixture)) { app.run() }
