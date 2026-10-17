import Cocoa

// Vanilla AppKit app that logs every event it receives. Stand-in for an unmodified third-party app.
// usage: target <role> <cgx> <cgy> <w> <h> <logpath> <r,g,b>
let args = CommandLine.arguments
guard args.count >= 8 else { print("usage: target <role> <cgx> <cgy> <w> <h> <log> <r,g,b>"); exit(2) }
let role = args[1]
let cgx = Double(args[2])!, cgy = Double(args[3])!, w = Double(args[4])!, h = Double(args[5])!
let logPath = args[6]
let rgb = args[7].split(separator: ",").map { Double($0)! / 255.0 }

FileManager.default.createFile(atPath: logPath, contents: nil)
let fh = FileHandle(forWritingAtPath: logPath)!
func log(_ dict: [String: Any]) {
    var d = dict
    d["t"] = Date().timeIntervalSince1970
    d["role"] = role
    if let app = NSApp { d["active"] = app.isActive; d["hasKeyWindow"] = app.keyWindow != nil }
    if let data = try? JSONSerialization.data(withJSONObject: d) { fh.write(data); fh.write("\n".data(using: .utf8)!) }
}

let typeNames: [NSEvent.EventType: String] = [
    .leftMouseDown: "leftMouseDown", .leftMouseUp: "leftMouseUp", .rightMouseDown: "rightMouseDown",
    .rightMouseUp: "rightMouseUp", .mouseMoved: "mouseMoved", .keyDown: "keyDown", .keyUp: "keyUp",
    .flagsChanged: "flagsChanged", .scrollWheel: "scrollWheel", .leftMouseDragged: "leftMouseDragged",
]

class EventLoggingApp: NSApplication {
    override func sendEvent(_ event: NSEvent) {
        if let name = typeNames[event.type] {
            var d: [String: Any] = ["ev": name, "winNum": event.windowNumber, "hasWindow": event.window != nil]
            if event.type == .keyDown || event.type == .keyUp {
                d["chars"] = event.characters ?? ""; d["keyCode"] = Int(event.keyCode)
            } else {
                let p = event.locationInWindow
                d["x"] = Double(p.x); d["y"] = Double(p.y)
                if let win = event.window {
                    let sp = win.convertPoint(toScreen: p)
                    d["sx"] = Double(sp.x); d["sy"] = Double(primaryH - sp.y)   // CG coords (top-left origin)
                } else {
                    d["sx"] = Double(p.x); d["sy"] = Double(primaryH - p.y)     // windowless: location is screen coords
                }
                if let cg = event.cgEvent {
                    d["f91"] = Int(cg.getIntegerValueField(.mouseEventWindowUnderMousePointer))
                    d["f92"] = Int(cg.getIntegerValueField(.mouseEventWindowUnderMousePointerThatCanHandleThisEvent))
                }
            }
            log(d)
        }
        super.sendEvent(event)
    }
}

let app = EventLoggingApp.shared
app.setActivationPolicy(.regular)
let primaryH = Double(NSScreen.screens[0].frame.height)
func toCG(_ r: NSRect) -> [String: Double] { ["x": Double(r.minX), "y": primaryH - Double(r.maxY), "w": Double(r.width), "h": Double(r.height)] }

class TargetView: NSView {
    override var isFlipped: Bool { true }
    override func draw(_ r: NSRect) {
        NSColor(srgbRed: rgb[0], green: rgb[1], blue: rgb[2], alpha: 1).setFill(); bounds.fill()
        let attrs: [NSAttributedString.Key: Any] = [.font: NSFont.boldSystemFont(ofSize: 28), .foregroundColor: NSColor.white]
        (role as NSString).draw(at: NSPoint(x: 20, y: bounds.height - 60), withAttributes: attrs)
    }
    override func mouseDown(with e: NSEvent) { log(["hit": "view.mouseDown"]) }
}

var field: NSTextField!
var button: NSButton!
var win: NSWindow!

class Controller: NSObject, NSTextFieldDelegate, NSApplicationDelegate, NSWindowDelegate {
    @objc func pressed(_ sender: Any?) { log(["hit": "buttonPressed"]) }
    func controlTextDidChange(_ n: Notification) { log(["hit": "textChanged", "value": field.stringValue]) }
    func applicationDidBecomeActive(_ n: Notification) { log(["hit": "appDidBecomeActive"]) }
    func windowDidBecomeKey(_ n: Notification) { log(["hit": "windowDidBecomeKey"]) }
    func applicationDidFinishLaunching(_ n: Notification) {
        NSApp.unhide(nil)
        win.makeKeyAndOrderFront(nil)
        win.orderFrontRegardless()
        win.makeFirstResponder(field)
        log(["hit": "order", "visible": win.isVisible, "onActiveSpace": win.isOnActiveSpace, "appHidden": NSApp.isHidden, "occlusion": Int(win.occlusionState.rawValue)])
        let view = win.contentView!
        let bRect = win.convertToScreen(view.convert(button.frame, to: nil))
        let fRect = win.convertToScreen(view.convert(field.frame, to: nil))
        log(["hit": "geometry", "winNum": win.windowNumber, "pid": Int(ProcessInfo.processInfo.processIdentifier),
             "frame": toCG(win.frame), "button": toCG(bRect), "field": toCG(fRect)])
    }
}
let ctl = Controller()

let contentRect = NSRect(x: cgx, y: primaryH - cgy - h, width: w, height: h)
win = NSWindow(contentRect: contentRect, styleMask: [.titled, .closable], backing: .buffered, defer: false)
win.title = role
win.isReleasedWhenClosed = false
win.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
win.delegate = ctl
let view = TargetView(frame: NSRect(origin: .zero, size: contentRect.size))
win.contentView = view
button = NSButton(title: "Press me", target: ctl, action: #selector(Controller.pressed(_:)))
button.frame = NSRect(x: 20, y: 20, width: 160, height: 32)
view.addSubview(button)
field = NSTextField(frame: NSRect(x: 20, y: 70, width: 300, height: 28))
field.placeholderString = "type here"
field.delegate = ctl
view.addSubview(field)
app.delegate = ctl
app.run()
