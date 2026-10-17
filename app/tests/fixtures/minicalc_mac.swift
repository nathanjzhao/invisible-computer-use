// Owned AppKit receivers for native macOS package acceptance.
//
//   minicalc_mac <work-dir> calc      ICU Calculator + same-process ICU Focus Guard sibling
//   minicalc_mac <work-dir> guard     separate foreground app the "user" types into
//   minicalc_mac <work-dir> receiver  ICU Manual Receiver: text, pointer pad, scroll area
//   minicalc_mac <work-dir> scope     ICU Scope Alpha + ICU Scope Beta, one editable field each
//
// Every callback records NSApp.isActive and wall-clock time, so a probe can
// prove that background delivery never activated the receiver. Nothing here
// activates itself; the probe's own user-level clicks decide the foreground.
import Cocoa

let args = CommandLine.arguments
let work = URL(fileURLWithPath: args.count > 1 ? args[1] : FileManager.default.currentDirectoryPath)
let mode = args.count > 2 ? args[2] : "calc"
let app = NSApplication.shared
app.setActivationPolicy(.regular)
// Keep the receiver's accessibility tree realized while it is in the background.
let activity = ProcessInfo.processInfo.beginActivity(options: [.userInitiated, .idleSystemSleepDisabled], reason: "icu acceptance fixture")

func now() -> Double { Date().timeIntervalSince1970 }
func writeJSON(_ name: String, _ value: Any) {
    let data = try! JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    let tmp = work.appendingPathComponent(name + ".tmp")
    try! data.write(to: tmp)
    // rename(2) replaces atomically, so a reader never sees a missing file.
    precondition(rename(tmp.path, work.appendingPathComponent(name).path) == 0)
}
func appendJSONL(_ name: String, _ value: Any) {
    var data = try! JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    data.append(0x0a)
    let url = work.appendingPathComponent(name)
    if let handle = try? FileHandle(forWritingTo: url) {
        handle.seekToEndOfFile(); handle.write(data); handle.closeFile()
    } else {
        try! data.write(to: url)
    }
}
func makeWindow(_ title: String, _ frame: NSRect) -> NSWindow {
    let w = NSWindow(contentRect: frame, styleMask: [.titled, .resizable, .miniaturizable], backing: .buffered, defer: false)
    w.title = title
    w.isReleasedWhenClosed = false
    w.setFrameOrigin(frame.origin)
    return w
}
var windows: [NSWindow] = []

// MARK: calculator
final class Calc: NSObject, NSTextFieldDelegate {
    let display = NSTextField(frame: NSRect(x: 20, y: 250, width: 280, height: 34))
    var acc = 0, cur = 0, op: String? = nil, entering = false
    var pressed: [String] = []
    override init() {
        super.init()
        // Editable so AppKit exposes an AXTextField with a readable AXValue.
        display.isEditable = true
        display.alignment = .right
        display.font = .monospacedDigitSystemFont(ofSize: 26, weight: .regular)
        display.stringValue = "0"
        display.setAccessibilityLabel("Result")
        display.delegate = self
    }
    // Keystrokes that land in the display: the user's typing went astray.
    func controlTextDidChange(_ obj: Notification) {
        appendJSONL("display-edits.jsonl", ["text": display.stringValue, "active": NSApp.isActive, "time": now()])
    }
    @objc func press(_ sender: NSButton) {
        let t = sender.title
        pressed.append(t)
        if let d = Int(t) {
            cur = entering ? cur * 10 + d : d
            entering = true
            display.stringValue = String(cur)
        } else if t == "×" || t == "+" {
            acc = entering ? cur : acc; op = t; entering = false
            display.stringValue = "\(acc) \(t)"
        } else if t == "=" {
            let rhs = entering ? cur : acc
            acc = op == "×" ? acc * rhs : op == "+" ? acc + rhs : rhs
            op = nil; cur = acc; entering = false
            display.stringValue = String(acc)
        } else {
            acc = 0; cur = 0; op = nil; entering = false; display.stringValue = "0"
        }
        appendJSONL("receiver-events.jsonl", ["button": t, "display": display.stringValue, "active": NSApp.isActive,
                                              "key": sender.window?.isKeyWindow ?? false, "time": now()])
        writeJSON("fixture-state.json", ["display": display.stringValue, "buttons": pressed])
    }
}

// MARK: manual receiver
final class Pad: NSView {
    override var acceptsFirstResponder: Bool { false }
    override func draw(_ dirtyRect: NSRect) {
        NSColor.systemTeal.setFill(); bounds.fill()
        ("Click Pad" as NSString).draw(at: NSPoint(x: 8, y: 8), withAttributes: [.font: NSFont.systemFont(ofSize: 14)])
    }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func mouseDown(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        appendJSONL("pointer-events.jsonl", ["kind": "down", "x": p.x, "y": bounds.height - p.y, "clicks": event.clickCount,
                                             "button": event.buttonNumber, "active": NSApp.isActive, "time": now()])
    }
    override func rightMouseDown(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        appendJSONL("pointer-events.jsonl", ["kind": "right", "x": p.x, "y": bounds.height - p.y, "active": NSApp.isActive, "time": now()])
    }
}
final class Receiver: NSObject, NSTextViewDelegate, NSTextFieldDelegate {
    let draft = NSTextView(frame: NSRect(x: 0, y: 0, width: 400, height: 90))
    let secret = NSSecureTextField(frame: NSRect(x: 20, y: 20, width: 400, height: 24))
    let scroll = NSScrollView(frame: NSRect(x: 20, y: 60, width: 200, height: 120))
    var window: NSWindow!
    func save() {
        let text = draft.string
        let record: [String: Any] = ["text": text, "active": NSApp.isActive, "time": now(),
                                     "private_unchanged": secret.stringValue == "fixture-private-value",
                                     "draft_first_responder": window.firstResponder === draft,
                                     "marked": draft.hasMarkedText(),
                                     "caret": draft.selectedRange().location,
                                     "scroll_y": scroll.contentView.bounds.origin.y]
        writeJSON("receiver-state.json", record)
        appendJSONL("receiver-events.jsonl", record)
    }
    func textDidChange(_ notification: Notification) { save() }
    func controlTextDidChange(_ obj: Notification) { save() }
    @objc func scrolled(_ n: Notification) {
        appendJSONL("scroll-events.jsonl", ["y": scroll.contentView.bounds.origin.y, "active": NSApp.isActive, "time": now()])
        save()
    }
}

let calc = Calc()
let receiver = Receiver()
// Two documents in one process, for a task scoped to one of them.
final class Scope: NSObject, NSTextViewDelegate {
    var fields: [(String, NSTextView)] = []
    func save() { writeJSON("receiver-state.json", Dictionary(uniqueKeysWithValues: fields.map { ($0.0, $0.1.string) })) }
    func textDidChange(_ notification: Notification) {
        save()
        appendJSONL("receiver-events.jsonl", ["state": Dictionary(uniqueKeysWithValues: fields.map { ($0.0, $0.1.string) }),
                                              "active": NSApp.isActive, "time": now()])
    }
}
let scope = Scope()
func button(_ window: NSWindow, _ title: String, _ x: CGFloat, _ y: CGFloat, _ target: AnyObject, _ action: Selector) {
    let b = NSButton(frame: NSRect(x: x, y: y, width: 60, height: 44))
    b.title = title; b.bezelStyle = .rounded; b.target = target; b.action = action
    // Calculator keys carry a platform identifier (AXIdentifier); the sibling's
    // duplicate "Same" buttons deliberately do not.
    if window.title == "ICU Calculator" { b.identifier = NSUserInterfaceItemIdentifier("calc-btn-\(title)") }
    window.contentView?.addSubview(b)
}
func buttons(_ window: NSWindow, _ title: String) -> [NSButton] {
    (window.contentView?.subviews ?? []).compactMap { $0 as? NSButton }.filter { $0.title == title }
}

switch mode {
case "calc":
    let w = makeWindow("ICU Calculator", NSRect(x: 1500, y: 560, width: 320, height: 300))
    w.contentView?.addSubview(calc.display)
    for (t, x, y) in [("7", 20, 190), ("8", 90, 190), ("9", 160, 190), ("4", 20, 140), ("5", 90, 140), ("6", 160, 140),
                      ("1", 20, 90), ("2", 90, 90), ("3", 160, 90), ("0", 20, 40)] as [(String, CGFloat, CGFloat)] {
        button(w, t, x, y, calc, #selector(Calc.press(_:)))
    }
    button(w, "×", 230, 140, calc, #selector(Calc.press(_:)))
    button(w, "+", 230, 190, calc, #selector(Calc.press(_:)))
    button(w, "=", 230, 90, calc, #selector(Calc.press(_:)))
    button(w, "C", 230, 40, calc, #selector(Calc.press(_:)))
    // Same process and app name: exercises same-name reference aliases.
    let sibling = makeWindow("ICU Focus Guard", NSRect(x: 1500, y: 180, width: 320, height: 200))
    let label = NSTextField(labelWithString: "Keep focus here")
    label.frame = NSRect(x: 20, y: 120, width: 280, height: 24)
    sibling.contentView?.addSubview(label)
    // Identical labels: identity must come from structure, never guessed.
    button(sibling, "Same", 20, 40, calc, #selector(Calc.press(_:)))
    button(sibling, "Same", 120, 40, calc, #selector(Calc.press(_:)))
    windows = [w, sibling]
    writeJSON("fixture-state.json", ["display": "0", "buttons": [String]()])
case "guard":
    let w = makeWindow("ICU Foreground Guard", NSRect(x: 40, y: 40, width: 420, height: 130))
    let field = NSTextField(frame: NSRect(x: 20, y: 50, width: 380, height: 28))
    field.stringValue = "guard"
    field.setAccessibilityLabel("Guard input")
    w.contentView?.addSubview(field)
    windows = [w]
    final class GuardWatch: NSObject, NSTextFieldDelegate {
        func controlTextDidChange(_ obj: Notification) {
            let f = obj.object as! NSTextField
            appendJSONL("guard-events.jsonl", ["text": f.stringValue, "time": now()])
        }
    }
    let watch = GuardWatch()
    field.delegate = watch
    objc_setAssociatedObject(field, "watch", watch, .OBJC_ASSOCIATION_RETAIN)
    w.initialFirstResponder = field
case "receiver":
    let w = makeWindow("ICU Manual Receiver", NSRect(x: 1400, y: 300, width: 460, height: 420))
    receiver.window = w
    let draftScroll = NSScrollView(frame: NSRect(x: 20, y: 300, width: 420, height: 90))
    receiver.draft.string = "seed"
    receiver.draft.isRichText = false
    receiver.draft.setAccessibilityLabel("Draft")
    receiver.draft.delegate = receiver
    receiver.draft.isAutomaticQuoteSubstitutionEnabled = false
    receiver.draft.isAutomaticTextReplacementEnabled = false
    receiver.draft.isAutomaticSpellingCorrectionEnabled = false
    receiver.draft.isContinuousSpellCheckingEnabled = false
    draftScroll.documentView = receiver.draft
    w.contentView?.addSubview(draftScroll)
    let pad = Pad(frame: NSRect(x: 240, y: 60, width: 200, height: 220))
    pad.setAccessibilityLabel("Click Pad")
    w.contentView?.addSubview(pad)
    let doc = NSTextView(frame: NSRect(x: 0, y: 0, width: 190, height: 2000))
    doc.string = (1...120).map { "Line \($0)" }.joined(separator: "\n")
    doc.isEditable = false
    receiver.scroll.documentView = doc
    receiver.scroll.hasVerticalScroller = true
    receiver.scroll.contentView.postsBoundsChangedNotifications = true
    receiver.scroll.frame = NSRect(x: 20, y: 60, width: 200, height: 220)
    receiver.scroll.setAccessibilityLabel("Scroll Area")
    NotificationCenter.default.addObserver(receiver, selector: #selector(Receiver.scrolled(_:)),
                                           name: NSView.boundsDidChangeNotification, object: receiver.scroll.contentView)
    w.contentView?.addSubview(receiver.scroll)
    receiver.secret.stringValue = "fixture-private-value"
    receiver.secret.setAccessibilityLabel("Private control")
    receiver.secret.delegate = receiver
    w.contentView?.addSubview(receiver.secret)
    windows = [w]
    // Select Draft inside its own window before the app owns the desktop.
    w.initialFirstResponder = receiver.draft
    w.makeFirstResponder(receiver.draft)
    receiver.draft.setSelectedRange(NSRange(location: 4, length: 0))
case "scope":
    for (label, y) in [("Alpha", 700), ("Beta", 300)] as [(String, CGFloat)] {
        let w = makeWindow("ICU Scope " + label, NSRect(x: 1400, y: y, width: 300, height: 220))
        let caption = NSTextField(labelWithString: "Document " + label)
        caption.frame = NSRect(x: 20, y: 180, width: 260, height: 24)
        w.contentView?.addSubview(caption)
        let holder = NSScrollView(frame: NSRect(x: 20, y: 20, width: 260, height: 150))
        let view = NSTextView(frame: NSRect(x: 0, y: 0, width: 260, height: 150))
        view.string = label.lowercased(); view.isRichText = false
        view.isAutomaticQuoteSubstitutionEnabled = false; view.isAutomaticTextReplacementEnabled = false
        view.isAutomaticSpellingCorrectionEnabled = false; view.isContinuousSpellCheckingEnabled = false
        view.setAccessibilityLabel("Field " + label)
        view.delegate = scope
        holder.documentView = view
        w.contentView?.addSubview(holder)
        w.initialFirstResponder = view
        scope.fields.append((label, view))
        windows.append(w)
    }
    scope.save()
default:
    fatalError("unknown mode \(mode)")
}
for w in windows { w.orderFrontRegardless() }
// AppKit activation timeline, including focus-without-raise activations that
// never change the WindowServer frontmost application.
for (name, kind) in [(NSApplication.didBecomeActiveNotification, "active"), (NSApplication.didResignActiveNotification, "inactive")] {
    NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) { _ in
        appendJSONL("activation-events.jsonl", ["kind": kind, "mode": mode, "key_window": NSApp.keyWindow?.title ?? "", "time": now()])
    }
}
// The guard plays the user's foreground app, so it also watches every app's
// activation through NSWorkspace, plus its own key window. These are pushed
// notifications, not samples: an activation of any length is delivered, which
// a polling oracle can miss. Whichever layer a background action disturbs
// (WindowServer front process, AppKit active app, key window) shows up here.
if mode == "guard" {
    let ws = NSWorkspace.shared.notificationCenter
    for (name, kind) in [(NSWorkspace.didActivateApplicationNotification, "activate"),
                         (NSWorkspace.didDeactivateApplicationNotification, "deactivate")] {
        ws.addObserver(forName: name, object: nil, queue: .main) { n in
            let app = n.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication
            appendJSONL("workspace-events.jsonl", ["kind": kind, "pid": Int(app?.processIdentifier ?? -1),
                                                   "name": app?.localizedName ?? "", "time": now()])
        }
    }
    for (name, kind) in [(NSWindow.didResignKeyNotification, "resign_key"), (NSWindow.didBecomeKeyNotification, "become_key")] {
        NotificationCenter.default.addObserver(forName: name, object: windows[0], queue: .main) { _ in
            appendJSONL("workspace-events.jsonl", ["kind": kind, "pid": Int(ProcessInfo.processInfo.processIdentifier),
                                                   "name": "guard window", "time": now()])
        }
    }
}
if mode == "receiver" { receiver.save() }
// Screen rectangles in top-left global coordinates, for mapping tile points.
func screenRect(_ view: NSView) -> [String: Double] {
    let r = view.window!.convertToScreen(view.convert(view.bounds, to: nil))
    let top = NSScreen.screens[0].frame.maxY
    return ["x": r.minX, "y": top - r.maxY, "w": r.width, "h": r.height]
}
DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
    var ready: [String: Any] = ["pid": ProcessInfo.processInfo.processIdentifier,
                                "windows": windows.map { ["title": $0.title, "number": $0.windowNumber,
                                                          "frame": ["x": $0.frame.minX, "y": NSScreen.screens[0].frame.maxY - $0.frame.maxY,
                                                                    "w": $0.frame.width, "h": $0.frame.height]] }]
    if mode == "receiver", let pad = windows[0].contentView?.subviews.first(where: { $0 is Pad }) {
        ready["pad"] = screenRect(pad); ready["scroll"] = screenRect(receiver.scroll)
    }
    writeJSON("fixture-\(mode)-ready.json", ready)
}
// The user's own window management, requested by the probe through
// <work>/command-<mode>.json: minimize / restore / hide / show / offspace / onspace.
// offspace moves the first window to a new, never-shown Space (SkyLight SPI),
// which is what a window left on another desktop looks like to other apps.
typealias CGSConnection = Int32
let skylight = dlopen("/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight", RTLD_NOW)
func sym<T>(_ name: String, _: T.Type) -> T? { dlsym(skylight, name).map { unsafeBitCast($0, to: T.self) } }
let mainConnection = sym("CGSMainConnectionID", (@convention(c) () -> CGSConnection).self)
let spaceCreate = sym("CGSSpaceCreate", (@convention(c) (CGSConnection, Int32, CFDictionary?) -> UInt64).self)
let activeSpace = sym("CGSGetActiveSpace", (@convention(c) (CGSConnection) -> UInt64).self)
let addToSpaces = sym("CGSAddWindowsToSpaces", (@convention(c) (CGSConnection, CFArray, CFArray) -> Void).self)
let removeFromSpaces = sym("CGSRemoveWindowsFromSpaces", (@convention(c) (CGSConnection, CFArray, CFArray) -> Void).self)
var parkedSpace: UInt64 = 0
var homeSpace: UInt64 = 0
func windowState(_ w: NSWindow) -> [String: Any] {
    ["title": w.title, "miniaturized": w.isMiniaturized, "visible": w.isVisible, "on_active_space": w.isOnActiveSpace,
     "active": NSApp.isActive, "time": now()]
}
Timer.scheduledTimer(withTimeInterval: 0.2, repeats: true) { _ in
    let url = work.appendingPathComponent("command-\(mode).json")
    guard let data = try? Data(contentsOf: url), let cmd = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
          let name = cmd["cmd"] as? String else { return }
    try? FileManager.default.removeItem(at: url)
    let w = windows[0]
    var result: [String: Any] = ["cmd": name, "mode": mode]
    switch name {
    case "status": break
    // Identity scenarios (calc mode): windows[0] is ICU Calculator, windows[1] the sibling.
    case "set_display": calc.display.stringValue = cmd["value"] as? String ?? "42"
    case "relabel_nine": buttons(w, "9").first?.title = "Nine"
    case "remove_eight": buttons(w, "8").first?.removeFromSuperview()
    case "add_new": button(w, "New", 160, 40, calc, #selector(Calc.press(_:)))
    case "remove_first_same": buttons(windows[1], "Same").first?.removeFromSuperview()
    case "text", "reset_text":
        // The field editor holds the live text while the field is being edited.
        let field = w.contentView?.subviews.compactMap { $0 as? NSTextField }.first { $0.isEditable }
        if name == "reset_text" {
            let value = mode == "guard" ? "guard" : "0"
            if let editor = field?.currentEditor() { editor.string = value } else { field?.stringValue = value }
        }
        result["text"] = field?.currentEditor()?.string ?? field?.stringValue ?? ""
    case "clear":
        if mode == "calc" { calc.acc = 0; calc.cur = 0; calc.op = nil; calc.entering = false; calc.display.stringValue = "0"
            writeJSON("fixture-state.json", ["display": "0", "buttons": calc.pressed]) }
    case "minimize": w.miniaturize(nil)
    case "restore": w.deminiaturize(nil)
    case "hide": w.orderOut(nil)
    case "show": w.orderFrontRegardless()
    case "offspace", "onspace":
        guard let cid = mainConnection?(), let create = spaceCreate, let add = addToSpaces, let remove = removeFromSpaces, let current = activeSpace else {
            result["error"] = "SkyLight space SPI unavailable"; break
        }
        let ids = [NSNumber(value: w.windowNumber)] as CFArray
        if name == "offspace" {
            homeSpace = current(cid)
            if parkedSpace == 0 { parkedSpace = create(cid, 1, nil) }
            add(cid, ids, [NSNumber(value: parkedSpace)] as CFArray)
            remove(cid, ids, [NSNumber(value: homeSpace)] as CFArray)
        } else {
            add(cid, ids, [NSNumber(value: homeSpace)] as CFArray)
            remove(cid, ids, [NSNumber(value: parkedSpace)] as CFArray)
        }
        result["space"] = name == "offspace" ? parkedSpace : homeSpace
    default: result["error"] = "unknown command"
    }
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.8) {
        result["window"] = windowState(w)
        appendJSONL("command-results.jsonl", result)
    }
}
withExtendedLifetime(activity) { app.run() }
