// Owned fixture for disposable macOS hosts only. Never launch on the user's desktop.
import Cocoa
import ApplicationServices

let work = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let window = NSWindow(contentRect: NSRect(x: 120, y: 140, width: 520, height: 360), styleMask: [.titled, .closable], backing: .buffered, defer: false)
window.title = "Recovery fixture"
let sibling = NSWindow(contentRect: NSRect(x: 700, y: 140, width: 520, height: 360), styleMask: [.titled], backing: .buffered, defer: false)
sibling.title = "Recovery decoy"

final class Fixture: NSObject {
    var controls: [NSView] = []
    var detached: [NSView] = []
    var hits: [String] = []
    var sequence = 0
    var lastCommand = -1
    var timer: Timer?

    func button(_ targetWindow: NSWindow, _ label: String = "Choose", _ id: String = "choice") -> NSButton {
        let b = NSButton(title: label, target: self, action: #selector(pressed(_:)))
        b.frame = NSRect(x: 30, y: 240 - controls.count * 42, width: 180, height: 32)
        b.setAccessibilityIdentifier(id)
        b.setAccessibilityHelp("Recovery button")
        targetWindow.contentView!.addSubview(b)
        controls.append(b)
        return b
    }
    func field() {
        let f = NSTextField(string: "before")
        f.frame = NSRect(x: 30, y: 140, width: 280, height: 28)
        f.setAccessibilityIdentifier("entry")
        f.setAccessibilityTitle("Entry")
        window.contentView!.addSubview(f)
        controls.append(f)
    }
    func textArea() {
        let f = NSTextView(frame: NSRect(x: 30, y: 90, width: 280, height: 100))
        f.string = "seed"
        f.setAccessibilityLabel("Draft")
        // No identifier: AppKit returns AXErrorFailure for its unset AXIdentifier.
        window.contentView!.addSubview(f)
        controls.append(f)
    }
    func clear() {
        for v in controls { v.removeFromSuperview(); detached.append(v) }
        controls = []
    }
    @objc func pressed(_ sender: NSButton) {
        hits.append(sender.window === window ? "target" : "decoy")
        publish()
    }
    func publish() {
        let obj: [String: Any] = ["pid": ProcessInfo.processInfo.processIdentifier,
            "window": window.windowNumber, "sibling": sibling.windowNumber,
            "command": lastCommand, "hits": hits,
            "frontPID": NSWorkspace.shared.frontmostApplication?.processIdentifier ?? -1,
            "pointer": [NSEvent.mouseLocation.x, NSEvent.mouseLocation.y],
            "values": controls.compactMap { ($0 as? NSTextField)?.stringValue ?? ($0 as? NSTextView)?.string },
            "visibleControls": controls.count]
        let data = try! JSONSerialization.data(withJSONObject: obj, options: [.sortedKeys])
        try! data.write(to: work.appendingPathComponent("state.json"), options: .atomic)
    }
    func poll() {
        if let data = try? Data(contentsOf: work.appendingPathComponent("command.json")),
           let c = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
           let n = c["id"] as? Int, n != lastCommand, let mode = c["mode"] as? String {
            lastCommand = n
            switch mode {
            case "single": clear(); hits = []; _ = button(window)
            case "twins": clear(); hits = []; _ = button(window); _ = button(window)
            case "replace": clear(); _ = button(window)
            case "replace_twins": clear(); _ = button(window); _ = button(window)
            case "remove_first": if let v = controls.first { v.removeFromSuperview(); detached.append(v); controls.removeFirst() }
            case "remove_all": clear()
            case "changed": clear(); _ = button(window, "Changed")
            case "sibling_only": clear(); _ = button(sibling)
            case "oversized": clear(); for _ in 0..<2050 { _ = button(window) }
            case "field": clear(); hits = []; field()
            case "text_area": clear(); hits = []; textArea()
            case "text_area_replaced": clear(); textArea()
            case "quit": app.terminate(nil)
            default: break
            }
        }
        publish()
    }
}
let fixture = Fixture()
_ = fixture.button(window)
window.orderFrontRegardless()
sibling.orderFrontRegardless()
fixture.timer = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { _ in fixture.poll() }
fixture.publish()
app.run()
