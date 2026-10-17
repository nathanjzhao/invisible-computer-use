import Cocoa

// usage: inject <pid> <windowTitle> <sx> <sy> <text|-> <mode: pid|global>
// sx, sy are CG screen coords (top-left origin). Prints a JSON report to stdout.
let a = CommandLine.arguments
guard a.count >= 6 else { print("usage: inject <pid> <title> <sx> <sy> <text|-> [pid|global]"); exit(2) }
let pid = pid_t(a[1])!, title = a[2], sx = Double(a[3])!, sy = Double(a[4])!
let text = a[5] == "-" ? "" : a[5]
let mode = a.count > 6 ? a[6] : "pid"
let keysOnly = mode == "pidkeys"

func front() -> [String: Any] {
    let f = NSWorkspace.shared.frontmostApplication
    return ["name": f?.localizedName ?? "?", "pid": Int(f?.processIdentifier ?? -1)]
}
func cursor() -> [String: Double] { let p = CGEvent(source: nil)!.location; return ["x": Double(p.x), "y": Double(p.y)] }

struct WinInfo { let id: CGWindowID; let bounds: CGRect; let owner: String; let name: String; let ownerPid: pid_t; let z: Int }
func windows() -> [WinInfo] {
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
    return list.enumerated().compactMap { (i, w) in
        guard let layer = w[kCGWindowLayer as String] as? Int, layer == 0 else { return nil }
        let id = (w[kCGWindowNumber as String] as! NSNumber).uint32Value
        let b = CGRect(dictionaryRepresentation: w[kCGWindowBounds as String] as! CFDictionary)!
        return WinInfo(id: id, bounds: b, owner: w[kCGWindowOwnerName as String] as? String ?? "?",
                       name: w[kCGWindowName as String] as? String ?? "", ownerPid: (w[kCGWindowOwnerPID as String] as! NSNumber).int32Value, z: i)
    }
}
var all = windows()
var onScreen = true
if !all.contains(where: { $0.ownerPid == pid && $0.name == title }) {
    onScreen = false
    let list = CGWindowListCopyWindowInfo([.optionAll, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
    all = list.enumerated().compactMap { (i, w) in
        guard let layer = w[kCGWindowLayer as String] as? Int, layer == 0, (w[kCGWindowAlpha as String] as? Double ?? 1) > 0 else { return nil }
        let id = (w[kCGWindowNumber as String] as! NSNumber).uint32Value
        let b = CGRect(dictionaryRepresentation: w[kCGWindowBounds as String] as! CFDictionary)!
        return WinInfo(id: id, bounds: b, owner: w[kCGWindowOwnerName as String] as? String ?? "?",
                       name: w[kCGWindowName as String] as? String ?? "", ownerPid: (w[kCGWindowOwnerPID as String] as! NSNumber).int32Value, z: i)
    }
}
guard let target = all.first(where: { $0.ownerPid == pid && $0.name == title }) else {
    print("{\"error\":\"window not found\"}"); exit(1)
}
let pt = CGPoint(x: sx, y: sy)
let atPoint = all.filter { $0.bounds.contains(pt) }   // front-to-back
let topmost = atPoint.first

let before = ["front": front(), "cursor": cursor()] as [String: Any]

let src = CGEventSource(stateID: .privateState)
func post(_ e: CGEvent) { if mode == "global" { e.post(tap: .cghidEventTap) } else { e.postToPid(pid) } }
func mouse(_ t: CGEventType, click: Int64) -> CGEvent {
    let e = CGEvent(mouseEventSource: src, mouseType: t, mouseCursorPosition: pt, mouseButton: .left)!
    e.setIntegerValueField(.mouseEventClickState, value: click)
    if mode == "pid" || mode == "ax" {
        e.setIntegerValueField(.mouseEventWindowUnderMousePointer, value: Int64(target.id))
        e.setIntegerValueField(.mouseEventWindowUnderMousePointerThatCanHandleThisEvent, value: Int64(target.id))
    }
    return e
}
var axInfo: [String: Any] = [:]
func axFind(role: String) -> AXUIElement? {
    let appEl = AXUIElementCreateApplication(pid)
    var winsRef: CFTypeRef?
    AXUIElementCopyAttributeValue(appEl, kAXWindowsAttribute as CFString, &winsRef)
    guard let wins = winsRef as? [AXUIElement] else { return nil }
    for w in wins {
        var tRef: CFTypeRef?; AXUIElementCopyAttributeValue(w, kAXTitleAttribute as CFString, &tRef)
        guard (tRef as? String) == title else { continue }
        var queue = [w]
        while !queue.isEmpty {
            let el = queue.removeFirst()
            var rRef: CFTypeRef?; AXUIElementCopyAttributeValue(el, kAXRoleAttribute as CFString, &rRef)
            if (rRef as? String) == role { return el }
            var cRef: CFTypeRef?; AXUIElementCopyAttributeValue(el, kAXChildrenAttribute as CFString, &cRef)
            if let ch = cRef as? [AXUIElement] { queue += ch }
        }
    }
    return nil
}
if keysOnly {
    // keystrokes only, no mouse
    for ch in text.utf16 { var c = ch
        let d = CGEvent(keyboardEventSource: src, virtualKey: 0, keyDown: true)!; d.keyboardSetUnicodeString(stringLength:1,unicodeString:&c); d.postToPid(pid)
        let u = CGEvent(keyboardEventSource: src, virtualKey: 0, keyDown: false)!; u.keyboardSetUnicodeString(stringLength:1,unicodeString:&c); u.postToPid(pid); usleep(10_000) }
} else if mode == "ax" {
    if let b = axFind(role: kAXButtonRole) { axInfo["press"] = AXUIElementPerformAction(b, kAXPressAction as CFString).rawValue } else { axInfo["press"] = "button not found" }
    if let f = axFind(role: kAXTextFieldRole) { axInfo["focus"] = AXUIElementSetAttributeValue(f, kAXFocusedAttribute as CFString, kCFBooleanTrue).rawValue } else { axInfo["focus"] = "field not found" }
    usleep(80_000)
} else {
    post(mouse(.mouseMoved, click: 0)); usleep(30_000)
    post(mouse(.leftMouseDown, click: 1)); usleep(40_000)
    post(mouse(.leftMouseUp, click: 1)); usleep(80_000)
}

for ch in text.utf16 {
    var c = ch
    let down = CGEvent(keyboardEventSource: src, virtualKey: 0, keyDown: true)!
    down.keyboardSetUnicodeString(stringLength: 1, unicodeString: &c)
    post(down)
    let up = CGEvent(keyboardEventSource: src, virtualKey: 0, keyDown: false)!
    up.keyboardSetUnicodeString(stringLength: 1, unicodeString: &c)
    post(up)
    usleep(12_000)
}
usleep(300_000)
let after = ["front": front(), "cursor": cursor()] as [String: Any]
if mode == "global" { let b = before["cursor"] as! [String: Double]; CGWarpMouseCursorPosition(CGPoint(x: b["x"]!, y: b["y"]!)) }

let report: [String: Any] = [
    "mode": mode, "pid": Int(pid), "targetOnScreen": onScreen, "point": ["x": sx, "y": sy],
    "targetWindow": ["id": Int(target.id), "z": target.z, "bounds": ["x": target.bounds.minX, "y": target.bounds.minY, "w": target.bounds.width, "h": target.bounds.height]],
    "topmostAtPoint": topmost.map { ["owner": $0.owner, "name": $0.name, "pid": Int($0.ownerPid), "id": Int($0.id), "z": $0.z] } ?? [:],
    "windowsAtPoint": atPoint.map { "\($0.owner):\($0.name)#\($0.id)" },
    "before": before, "after": after, "ax": axInfo,
]
let data = try! JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
print(String(data: data, encoding: .utf8)!)
