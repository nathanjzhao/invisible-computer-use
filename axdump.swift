import Cocoa
import ApplicationServices
let a = CommandLine.arguments
let pid = pid_t(a[1])!, title = a[2]
let doPress = a.count > 3 && a[3] == "press"
print("AXIsProcessTrusted:", AXIsProcessTrusted())
let app = AXUIElementCreateApplication(pid)
AXUIElementSetMessagingTimeout(app, 2.0)
// The robustness trick Sky/cua use: force AX to fully materialize the app tree
// (esp. Electron/Chromium and freshly-launched apps) and retry the window query.
AXUIElementSetAttributeValue(app, "AXManualAccessibility" as CFString, kCFBooleanTrue)
AXUIElementSetAttributeValue(app, "AXEnhancedUserInterface" as CFString, kCFBooleanTrue)
func attr(_ el: AXUIElement, _ k: String) -> CFTypeRef? { var r: CFTypeRef?; let e = AXUIElementCopyAttributeValue(el, k as CFString, &r); return e == .success ? r : nil }
func role(_ el: AXUIElement) -> String { (attr(el, kAXRoleAttribute as String) as? String) ?? "?" }
func title_(_ el: AXUIElement) -> String { (attr(el, kAXTitleAttribute as String) as? String) ?? "" }
var winsRef: CFTypeRef?
var we = AXError.success
for attempt in 0..<20 {
    we = AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &winsRef)
    if let w = winsRef as? [AXUIElement], !w.isEmpty { print("windows ready after \(attempt) retries"); break }
    usleep(100_000)
}
print("windows err:", we.rawValue, "count:", (winsRef as? [AXUIElement])?.count ?? -1)
guard let wins = winsRef as? [AXUIElement] else { print("no windows"); exit(1) }
func walk(_ el: AXUIElement, _ depth: Int) {
    let r = role(el), t = title_(el)
    var actions: CFArray?; AXUIElementCopyActionNames(el, &actions)
    let acts = (actions as? [String]) ?? []
    print(String(repeating: "  ", count: depth) + "\(r) '\(t)' actions=\(acts)")
    if doPress && r == kAXButtonRole {
        let pe = AXUIElementPerformAction(el, kAXPressAction as CFString)
        print(String(repeating: "  ", count: depth) + ">> AXPress button err=\(pe.rawValue)")
    }
    if doPress && r == kAXTextFieldRole {
        let fe = AXUIElementSetAttributeValue(el, kAXFocusedAttribute as CFString, kCFBooleanTrue)
        let ve = AXUIElementSetAttributeValue(el, kAXValueAttribute as CFString, "typed via AX" as CFString)
        print(String(repeating: "  ", count: depth) + ">> focus err=\(fe.rawValue) setValue err=\(ve.rawValue)")
    }
    if let ch = attr(el, kAXChildrenAttribute as String) as? [AXUIElement] { for c in ch { walk(c, depth + 1) } }
}
for w in wins { print("WINDOW title='\(title_(w))' role=\(role(w))"); if title_(w) == title || wins.count == 1 { walk(w, 1) } }
