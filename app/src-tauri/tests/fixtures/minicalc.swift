// A tiny real AppKit calculator used as a deterministic end-to-end target for
// the agent loop. It is a normal accessibility-exposing app (NSButtons ->
// AXButton, an NSTextField display -> AXTextField), driven through the real
// cua-driver exactly like any other app. App Nap is disabled and the window is
// pinned to one Space, so it stays realized (a live AX tree) no matter which
// Space the user is on — which makes the focus-free drive reproducible instead
// of flapping the way Apple's App-Napped Calculator does off the current Space.
//
// Prints `WINDOW_ID=<n>` / `PID=<n>` on stdout so the harness can target it.
import Cocoa

let app = NSApplication.shared
app.setActivationPolicy(.accessory) // no Dock icon; never self-fronts

let token = ProcessInfo.processInfo.beginActivity(
    options: [.userInitiated, .idleSystemSleepDisabled], reason: "minicalc-e2e")

final class Calc: NSObject {
    let display = NSTextField(frame: NSRect(x: 20, y: 250, width: 280, height: 34))
    var acc = 0            // accumulated value
    var cur = 0            // current entry
    var op: String? = nil  // pending operation
    var entering = false

    override init() {
        super.init()
        // Editable so AppKit exposes it as an AXTextField with a readable AXValue
        // (a plain non-editable label is pruned from the AX tree, leaving the
        // agent blind to the running result). We only ever drive the buttons, so
        // it never actually receives typed input.
        display.isEditable = true
        display.isSelectable = true
        display.alignment = .right
        display.font = .monospacedDigitSystemFont(ofSize: 26, weight: .regular)
        display.stringValue = "0"
        display.identifier = NSUserInterfaceItemIdentifier("calc-display")
    }
    @objc func digit(_ sender: NSButton) {
        let d = Int(sender.title) ?? 0
        cur = entering ? cur * 10 + d : d
        entering = true
        display.stringValue = String(cur)
    }
    @objc func mul(_ sender: NSButton) { startOp("×") }
    @objc func plus(_ sender: NSButton) { startOp("+") }
    func startOp(_ o: String) {
        acc = entering ? cur : acc
        op = o; entering = false
        // Reflect the pending operation on the display so an agent can SEE the
        // operator registered (e.g. "6 ×") and move on, instead of re-pressing it.
        display.stringValue = "\(acc) \(o)"
    }
    @objc func equals(_ sender: NSButton) {
        let rhs = entering ? cur : acc
        switch op {
        case "×": acc = acc * rhs
        case "+": acc = acc + rhs
        default: acc = rhs
        }
        op = nil; cur = acc; entering = false
        display.stringValue = String(acc)
    }
    @objc func clear(_ sender: NSButton) {
        acc = 0; cur = 0; op = nil; entering = false; display.stringValue = "0"
    }
}

let calc = Calc()
let win = NSWindow(contentRect: NSRect(x: 80, y: 80, width: 320, height: 300),
                   styleMask: [.titled, .resizable], backing: .buffered, defer: false)
win.title = "MiniCalc"
win.isReleasedWhenClosed = false
win.collectionBehavior = [.managed]
win.contentView?.addSubview(calc.display)

// A small keypad. Each button's AXButton label is its title, e.g. "6", "×", "=".
func button(_ title: String, _ x: CGFloat, _ y: CGFloat, _ action: Selector) {
    let b = NSButton(frame: NSRect(x: x, y: y, width: 60, height: 44))
    b.title = title
    b.bezelStyle = .rounded
    b.target = calc
    b.action = action
    b.setButtonType(.momentaryPushIn)
    b.identifier = NSUserInterfaceItemIdentifier("calc-btn-\(title)")
    win.contentView?.addSubview(b)
}
// Row of digits 6 and 7, the × operator, and = — enough for 6×7. Plus a few more
// so the tree looks like a real keypad, not a two-button stub.
let digits: [(String, CGFloat, CGFloat)] = [
    ("7", 20, 190), ("8", 90, 190), ("9", 160, 190),
    ("4", 20, 140), ("5", 90, 140), ("6", 160, 140),
    ("1", 20, 90),  ("2", 90, 90),  ("3", 160, 90),
    ("0", 20, 40),
]
for (t, x, y) in digits { button(t, x, y, #selector(Calc.digit(_:))) }
button("×", 230, 140, #selector(Calc.mul(_:)))
button("+", 230, 190, #selector(Calc.plus(_:)))
button("=", 230, 90,  #selector(Calc.equals(_:)))
button("C", 230, 40,  #selector(Calc.clear(_:)))

win.orderFrontRegardless()
FileHandle.standardOutput.write("WINDOW_ID=\(win.windowNumber)\nPID=\(ProcessInfo.processInfo.processIdentifier)\n".data(using: .utf8)!)
withExtendedLifetime(token) { app.run() }
