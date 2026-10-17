import Cocoa
let pid = pid_t(CommandLine.arguments[1])!
for (label, opt) in [("onscreen", CGWindowListOption.optionOnScreenOnly), ("all", CGWindowListOption.optionAll)] {
    let list = CGWindowListCopyWindowInfo([opt, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
    for w in list where (w[kCGWindowOwnerPID as String] as? pid_t) == pid {
        print(label, w[kCGWindowNumber as String] ?? "", "layer=\(w[kCGWindowLayer as String] ?? "")", "name=\(w[kCGWindowName as String] ?? "nil")", "alpha=\(w[kCGWindowAlpha as String] ?? "")", w[kCGWindowBounds as String] ?? "")
    }
}
