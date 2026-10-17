// User-side instrument for macOS package acceptance on a disposable Mac.
//
// It plays the human (real HID-level clicks and keys into the foreground app)
// and the observer (AX tree, frontmost app, keyboard focus, pointer, HID idle,
// on-screen windows of a given owner). It is never run on a developer's Mac.
//
//   macos_ui tree <pid> [maxNodes]                  AX tree JSON (web content included)
//   macos_ui press <pid> <json-matcher>             AXPress the unique match
//   macos_ui frame <pid> <json-matcher>             screen frame of the unique match
//   macos_ui click <x> <y> [right]                  HID click (moves the real pointer)
//   macos_ui move <x> <y>                           HID pointer move
//   macos_ui type <text>                            HID unicode key events
//   macos_ui key <keycode> [cmd,shift,alt,ctrl]     HID virtual key
//   macos_ui scroll <x> <y> <lines>                 HID wheel at a point
//   macos_ui sample <out.jsonl> <ownerPid,...>      ownership samples until SIGTERM
//   macos_ui state [ownerPid,...]                   one ownership sample
//   macos_ui input-source <id|current>              enable+select a TIS input source
//   macos_ui paste-set <text>                       general pasteboard string
//   macos_ui pid-of <bundle-id>                     running pids for a bundle id
//   macos_ui pidpath <pid>                          executable path of a pid
//   macos_ui windows <pid>                          CGWindowList records for an owner
import AppKit
import ApplicationServices
import Carbon

func emit(_ v: Any) {
    let data = try! JSONSerialization.data(withJSONObject: v, options: [.sortedKeys])
    FileHandle.standardOutput.write(data); FileHandle.standardOutput.write("\n".data(using: .utf8)!)
}
func fail(_ message: String) -> Never { emit(["ok": false, "error": message]); exit(2) }

func attr(_ e: AXUIElement, _ name: String) -> CFTypeRef? {
    var v: CFTypeRef?
    return AXUIElementCopyAttributeValue(e, name as CFString, &v) == .success ? v : nil
}
func str(_ e: AXUIElement, _ name: String) -> String? {
    guard let v = attr(e, name) else { return nil }
    if let s = v as? String { return s }
    if let n = v as? NSNumber { return n.stringValue }
    return nil
}
func frame(_ e: AXUIElement) -> [String: Double]? {
    guard let p = attr(e, kAXPositionAttribute), let s = attr(e, kAXSizeAttribute) else { return nil }
    var pt = CGPoint.zero, sz = CGSize.zero
    AXValueGetValue(p as! AXValue, .cgPoint, &pt); AXValueGetValue(s as! AXValue, .cgSize, &sz)
    return ["x": pt.x, "y": pt.y, "w": sz.width, "h": sz.height]
}
func children(_ e: AXUIElement) -> [AXUIElement] { (attr(e, kAXChildrenAttribute) as? [AXUIElement]) ?? [] }

struct Node { let el: AXUIElement; let info: [String: Any] }
func walk(_ pid: pid_t, limit: Int = 3000) -> [Node] {
    let root = AXUIElementCreateApplication(pid)
    AXUIElementSetAttributeValue(root, "AXManualAccessibility" as CFString, kCFBooleanTrue)
    var out: [Node] = []
    var parents: [Int] = []
    var queue: [(AXUIElement, String, Int, Int)] = [(root, "0", 0, -1)]
    var head = 0
    while head < queue.count && out.count < limit {
        let (e, path, depth, parent) = queue[head]; head += 1
        var info: [String: Any] = ["path": path, "depth": depth]
        for (k, a) in [("role", kAXRoleAttribute), ("subrole", kAXSubroleAttribute), ("title", kAXTitleAttribute),
                       ("desc", kAXDescriptionAttribute), ("value", kAXValueAttribute), ("help", kAXHelpAttribute),
                       ("placeholder", "AXPlaceholderValue"), ("identifier", "AXIdentifier")] {
            if let s = str(e, a), !s.isEmpty { info[k] = String(s.prefix(400)) }
        }
        if let f = frame(e) { info["frame"] = f }
        if let focused = attr(e, kAXFocusedAttribute) as? Bool, focused { info["focused"] = true }
        if let enabled = attr(e, kAXEnabledAttribute) as? Bool, !enabled { info["disabled"] = true }
        let index = out.count
        out.append(Node(el: e, info: info)); parents.append(parent)
        if depth < 40 {
            for (i, c) in children(e).enumerated() { queue.append((c, "\(path).\(i)", depth + 1, index)) }
        }
    }
    // A readable name for web controls whose label lives in descendant text.
    // BFS lists children after parents and in order, so a reverse pass can
    // fold each child's name into its parent.
    var kidNames = [[String]](repeating: [], count: out.count)
    var names = [String](repeating: "", count: out.count)
    for i in stride(from: out.count - 1, through: 0, by: -1) {
        let info = out[i].info
        var own = [info["title"], info["desc"], (info["role"] as? String) == "AXStaticText" ? info["value"] : nil]
            .compactMap { $0 as? String }.joined(separator: " ")
        if own.isEmpty { own = kidNames[i].reversed().joined(separator: " ") }
        names[i] = own.trimmingCharacters(in: .whitespaces)
        if parents[i] >= 0 && !names[i].isEmpty { kidNames[parents[i]].append(names[i]) }
    }
    return out.enumerated().map { i, n in
        var info = n.info; info["name"] = names[i]
        return Node(el: n.el, info: info)
    }
}
func matches(_ info: [String: Any], _ m: [String: Any]) -> Bool {
    let name = (info["name"] as? String) ?? ""
    if let r = m["role"] as? String, (info["role"] as? String) != r { return false }
    if let roles = m["roles"] as? [String], !roles.contains((info["role"] as? String) ?? "") { return false }
    if let x = m["name"] as? String, name != x { return false }
    if let x = m["contains"] as? String, !name.localizedCaseInsensitiveContains(x) { return false }
    if let x = m["placeholder_contains"] as? String, !((info["placeholder"] as? String) ?? "").contains(x) { return false }
    if let x = m["value_contains"] as? String, !((info["value"] as? String) ?? "").contains(x) { return false }
    if let f = info["frame"] as? [String: Double], m["visible"] as? Bool == true, (f["w"] ?? 0) <= 0 || (f["h"] ?? 0) <= 0 { return false }
    return true
}
func unique(_ pid: pid_t, _ json: String) -> Node {
    let m = try! JSONSerialization.jsonObject(with: json.data(using: .utf8)!) as! [String: Any]
    let found = walk(pid).filter { matches($0.info, m) }
    let index = m["index"] as? Int
    if index == nil && found.count != 1 { fail("expected one match, found \(found.count): \(found.prefix(5).map { $0.info })") }
    if let i = index, i >= found.count { fail("index \(i) out of range for \(found.count) matches") }
    return found[index ?? 0]
}

func post(_ e: CGEvent?) { e?.post(tap: .cghidEventTap); usleep(12000) }
func mouse(_ type: CGEventType, _ p: CGPoint, _ button: CGMouseButton = .left) {
    post(CGEvent(mouseEventSource: CGEventSource(stateID: .hidSystemState), mouseType: type, mouseCursorPosition: p, mouseButton: button))
}
func key(_ code: CGKeyCode, _ flags: CGEventFlags = []) {
    let src = CGEventSource(stateID: .hidSystemState)
    let d = CGEvent(keyboardEventSource: src, virtualKey: code, keyDown: true); d?.flags = flags; post(d)
    let u = CGEvent(keyboardEventSource: src, virtualKey: code, keyDown: false); u?.flags = flags; post(u)
}

func ownership(_ owners: [pid_t]) -> [String: Any] {
    var s: [String: Any] = ["time": Date().timeIntervalSince1970]
    s["front_pid"] = NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0
    s["menubar_pid"] = NSWorkspace.shared.menuBarOwningApplication?.processIdentifier ?? 0
    // The system-wide focus attributes are unavailable to this tool on the
    // test VM, so read keyboard focus from the frontmost application element.
    let front = NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0
    let system = AXUIElementCreateSystemWide()
    let frontApp = AXUIElementCreateApplication(front)
    let focusedApp = attr(system, kAXFocusedApplicationAttribute).map { $0 as! AXUIElement }
    if let a = focusedApp { var pid: pid_t = 0; AXUIElementGetPid(a, &pid); s["ax_focused_app"] = pid }
    let focusRoot = focusedApp ?? frontApp
    if let w = attr(focusRoot, kAXFocusedWindowAttribute) { s["focused_window"] = str(w as! AXUIElement, kAXTitleAttribute) ?? "" }
    if let f = attr(system, kAXFocusedUIElementAttribute) ?? attr(focusRoot, kAXFocusedUIElementAttribute) {
        let el = f as! AXUIElement
        var pid: pid_t = 0; AXUIElementGetPid(el, &pid)
        s["focused_element"] = ["pid": pid, "role": str(el, kAXRoleAttribute) ?? "", "label": str(el, kAXDescriptionAttribute) ?? str(el, kAXTitleAttribute) ?? ""]
    }
    let p = CGEvent(source: nil)?.location ?? .zero
    s["pointer"] = [p.x, p.y]
    s["hid_idle"] = CGEventSource.secondsSinceLastEventType(.hidSystemState, eventType: CGEventType(rawValue: ~0)!)
    if !owners.isEmpty {
        let list = (CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]]) ?? []
        s["owned_onscreen_windows"] = list.filter { owners.contains(($0[kCGWindowOwnerPID as String] as? pid_t) ?? -1) }.map {
            ["owner": $0[kCGWindowOwnerPID as String] ?? 0, "name": $0[kCGWindowName as String] ?? "", "layer": $0[kCGWindowLayer as String] ?? 0,
             "alpha": $0[kCGWindowAlpha as String] ?? 0, "bounds": $0[kCGWindowBounds as String] ?? [:]]
        }
    }
    return s
}
func owners(_ s: String?) -> [pid_t] { (s ?? "").split(separator: ",").compactMap { pid_t($0) } }

let a = CommandLine.arguments
guard a.count > 1 else { fail("missing command") }
switch a[1] {
case "trusted":
    emit(["ok": true, "ax": AXIsProcessTrusted(), "screen": CGPreflightScreenCaptureAccess(), "post": CGPreflightPostEventAccess()])
case "tree":
    let limit = a.count > 3 ? Int(a[3])! : 3000
    emit(["ok": true, "nodes": walk(pid_t(a[2])!, limit: limit).map { $0.info }])
case "press":
    let n = unique(pid_t(a[2])!, a[3])
    let r = AXUIElementPerformAction(n.el, kAXPressAction as CFString)
    emit(["ok": r == .success, "code": r.rawValue, "node": n.info])
case "frame":
    emit(["ok": true, "node": unique(pid_t(a[2])!, a[3]).info])
case "setfocus":
    let n = unique(pid_t(a[2])!, a[3])
    let r = AXUIElementSetAttributeValue(n.el, kAXFocusedAttribute as CFString, kCFBooleanTrue)
    emit(["ok": r == .success, "code": r.rawValue])
case "click":
    let p = CGPoint(x: Double(a[2])!, y: Double(a[3])!)
    let right = a.count > 4 && a[4] == "right"
    mouse(.mouseMoved, p)
    mouse(right ? .rightMouseDown : .leftMouseDown, p, right ? .right : .left)
    mouse(right ? .rightMouseUp : .leftMouseUp, p, right ? .right : .left)
    emit(["ok": true])
case "move":
    mouse(.mouseMoved, CGPoint(x: Double(a[2])!, y: Double(a[3])!)); emit(["ok": true])
case "type":
    // WebKit text fields ignore unicode-string events on key code 0, so ASCII
    // is typed as real US-layout key codes, the way a keyboard produces it.
    let plain: [Character: CGKeyCode] = ["a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7, "c": 8, "v": 9, "b": 11,
        "q": 12, "w": 13, "e": 14, "r": 15, "y": 16, "t": 17, "1": 18, "2": 19, "3": 20, "4": 21, "6": 22, "5": 23, "=": 24,
        "9": 25, "7": 26, "-": 27, "8": 28, "0": 29, "]": 30, "o": 31, "u": 32, "[": 33, "i": 34, "p": 35, "\n": 36, "l": 37,
        "j": 38, "'": 39, "k": 40, ";": 41, "\\": 42, ",": 43, "/": 44, "n": 45, "m": 46, ".": 47, "\t": 48, " ": 49, "`": 50]
    let shifted: [Character: CGKeyCode] = ["!": 18, "@": 19, "#": 20, "$": 21, "%": 23, "^": 22, "&": 26, "*": 28, "(": 25,
        ")": 29, "_": 27, "+": 24, "{": 33, "}": 30, "|": 42, ":": 41, "\"": 39, "<": 43, ">": 47, "?": 44, "~": 50]
    for ch in a[2] {
        if let code = plain[ch] { key(code) }
        else if let code = shifted[ch] { key(code, .maskShift) }
        else if ch.isUppercase, let code = plain[Character(ch.lowercased())] { key(code, .maskShift) }
        else { fail("no US key for \(ch)") }
        usleep(8000)
    }
    emit(["ok": true])
case "key":
    var flags: CGEventFlags = []
    for m in (a.count > 3 ? a[3] : "").split(separator: ",") {
        switch m { case "cmd": flags.insert(.maskCommand); case "shift": flags.insert(.maskShift)
        case "alt": flags.insert(.maskAlternate); case "ctrl": flags.insert(.maskControl); default: fail("bad modifier \(m)") }
    }
    key(CGKeyCode(a[2])!, flags); emit(["ok": true])
case "scroll":
    let p = CGPoint(x: Double(a[2])!, y: Double(a[3])!)
    mouse(.mouseMoved, p)
    let e = CGEvent(scrollWheelEvent2Source: CGEventSource(stateID: .hidSystemState), units: .line, wheelCount: 1, wheel1: Int32(a[4])!, wheel2: 0, wheel3: 0)
    e?.location = p; post(e); emit(["ok": true])
case "state":
    emit(ownership(owners(a.count > 2 ? a[2] : nil)))
case "sample":
    let url = URL(fileURLWithPath: a[2]); FileManager.default.createFile(atPath: url.path, contents: nil)
    let handle = try! FileHandle(forWritingTo: url)
    let watched = owners(a.count > 3 ? a[3] : nil)
    signal(SIGTERM) { _ in exit(0) }
    while true {
        // Spinning the run loop keeps NSWorkspace's frontmost application current.
        RunLoop.current.run(until: Date().addingTimeInterval(0.015))
        var data = try! JSONSerialization.data(withJSONObject: ownership(watched), options: [.sortedKeys])
        data.append(0x0a); handle.write(data)
    }
case "input-source":
    if a[2] == "current" {
        let s = TISCopyCurrentKeyboardInputSource().takeRetainedValue()
        emit(["ok": true, "id": Unmanaged<CFString>.fromOpaque(TISGetInputSourceProperty(s, kTISPropertyInputSourceID)).takeUnretainedValue() as String])
    } else {
        let filter = [kTISPropertyInputSourceID as String: a[2]] as CFDictionary
        guard let list = TISCreateInputSourceList(filter, true)?.takeRetainedValue() as? [TISInputSource], let s = list.first else { fail("no input source \(a[2])") }
        let enabled = TISEnableInputSource(s), selected = TISSelectInputSource(s)
        emit(["ok": enabled == noErr && selected == noErr, "enable": enabled, "select": selected])
    }
case "paste-set":
    NSPasteboard.general.clearContents(); emit(["ok": NSPasteboard.general.setString(a[2], forType: .string)])
case "pid-of":
    emit(["ok": true, "pids": NSRunningApplication.runningApplications(withBundleIdentifier: a[2]).map { $0.processIdentifier }])
case "pidpath":
    var buf = [CChar](repeating: 0, count: 4096)
    let n = proc_pidpath(pid_t(a[2])!, &buf, UInt32(buf.count))
    emit(["ok": n > 0, "path": n > 0 ? String(cString: buf) : ""])
case "windows":
    let pid = pid_t(a[2])!
    let list = (CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]]) ?? []
    emit(["ok": true, "windows": list.filter { ($0[kCGWindowOwnerPID as String] as? pid_t) == pid }.map {
        ["number": $0[kCGWindowNumber as String] ?? 0, "name": $0[kCGWindowName as String] ?? "", "layer": $0[kCGWindowLayer as String] ?? 0,
         "onscreen": $0[kCGWindowIsOnscreen as String] ?? false, "alpha": $0[kCGWindowAlpha as String] ?? 0, "bounds": $0[kCGWindowBounds as String] ?? [:]]
    }])
case "pngcolumn":
    // Pixels down column 0 of a PNG (a `screencapture -R x,y,1,h` strip).
    guard let data = FileManager.default.contents(atPath: a[2]), let rep = NSBitmapImageRep(data: data) else { fail("unreadable png") }
    emit(["ok": true, "rgb": (0..<rep.pixelsHigh).map { i -> [Int] in
        let c = rep.colorAt(x: 0, y: i)!.usingColorSpace(.deviceRGB)!
        return [Int(c.redComponent * 255), Int(c.greenComponent * 255), Int(c.blueComponent * 255)]
    }])
case "quit":
    // Quit one process, not whichever process LaunchServices maps to a bundle id.
    guard let app = NSRunningApplication(processIdentifier: pid_t(a[2])!) else { fail("no such app") }
    emit(["ok": app.terminate()])
case "onscreen":
    let list = (CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]]) ?? []
    emit(["ok": true, "windows": list.map {
        ["owner": $0[kCGWindowOwnerPID as String] ?? 0, "owner_name": $0[kCGWindowOwnerName as String] ?? "", "name": $0[kCGWindowName as String] ?? "",
         "layer": $0[kCGWindowLayer as String] ?? 0, "bounds": $0[kCGWindowBounds as String] ?? [:]]
    }])
default:
    fail("unknown command \(a[1])")
}
