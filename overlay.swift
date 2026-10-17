import Cocoa

// Click-through, non-activating ghost cursor, clipped to the target window's VISIBLE region.
// usage: overlay <targetWindowId> <sx> <sy> <seconds> <logpath>
let a = CommandLine.arguments
let targetId = CGWindowID(a[1])!, sx = Double(a[2])!, sy = Double(a[3])!, secs = Double(a[4])!, logPath = a[5]
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let screen = NSScreen.screens[0]

func subtract(_ rects: [CGRect], _ o: CGRect) -> [CGRect] {
    var out: [CGRect] = []
    for r in rects {
        let i = r.intersection(o)
        if i.isNull || i.isEmpty { out.append(r); continue }
        if i.minY > r.minY { out.append(CGRect(x: r.minX, y: r.minY, width: r.width, height: i.minY - r.minY)) }
        if i.maxY < r.maxY { out.append(CGRect(x: r.minX, y: i.maxY, width: r.width, height: r.maxY - i.maxY)) }
        if i.minX > r.minX { out.append(CGRect(x: r.minX, y: i.minY, width: i.minX - r.minX, height: i.height)) }
        if i.maxX < r.maxX { out.append(CGRect(x: i.maxX, y: i.minY, width: r.maxX - i.maxX, height: i.height)) }
    }
    return out
}
// Visible rects of the target window (CG coords): its bounds minus every layer-0 window in front of it.
// nil = target not on this Space / minimized / gone.
func visibleRects() -> [CGRect]? {
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
    var front: [CGRect] = []
    for w in list {
        guard let layer = w[kCGWindowLayer as String] as? Int, layer == 0 else { continue }
        let id = (w[kCGWindowNumber as String] as! NSNumber).uint32Value
        let b = CGRect(dictionaryRepresentation: w[kCGWindowBounds as String] as! CFDictionary)!
        if id == targetId { return front.reduce([b]) { subtract($0, $1) } }
        front.append(b)
    }
    return nil
}

class GhostView: NSView {
    override var isFlipped: Bool { true }
    var t: Double = 0
    var visible: [CGRect]? = nil
    let start = CGPoint(x: sx - 140, y: sy - 90), end = CGPoint(x: sx, y: sy)
    override func draw(_ r: NSRect) {
        guard let vis = visible, !vis.isEmpty else { return }
        let clip = NSBezierPath(); for v in vis { clip.appendRect(v) }; clip.addClip()
        let k = min(1, t / 0.5), e = 1 - pow(1 - k, 3)
        let p = CGPoint(x: start.x + (end.x - start.x) * e, y: start.y + (end.y - start.y) * e)
        if t > 0.5 && t < 1.3 {
            let rr = (t - 0.5) / 0.8, radius = 6 + 30 * rr
            let ring = NSBezierPath(ovalIn: NSRect(x: p.x - radius, y: p.y - radius, width: radius * 2, height: radius * 2))
            ring.lineWidth = 3; NSColor.systemOrange.withAlphaComponent(1 - rr).setStroke(); ring.stroke()
        }
        let s = 2.2
        let pts: [(Double, Double)] = [(0,0),(0,17),(4.5,13),(7.5,20),(10.5,18.5),(7.5,11.5),(12.5,11.5)]
        let path = NSBezierPath(); path.move(to: NSPoint(x: p.x, y: p.y))
        for (dx, dy) in pts.dropFirst() { path.line(to: NSPoint(x: p.x + dx * s, y: p.y + dy * s)) }
        path.close()
        NSColor.systemOrange.withAlphaComponent(0.9).setFill(); path.fill()
        NSColor.white.setStroke(); path.lineWidth = 1.5; path.stroke()
        ("agent" as NSString).draw(at: NSPoint(x: p.x + 30, y: p.y + 34), withAttributes: [.font: NSFont.boldSystemFont(ofSize: 13),
            .foregroundColor: NSColor.white, .backgroundColor: NSColor.systemOrange.withAlphaComponent(0.9)])
    }
}
let panel = NSPanel(contentRect: screen.frame, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
panel.level = .screenSaver
panel.backgroundColor = .clear; panel.isOpaque = false; panel.hasShadow = false
panel.ignoresMouseEvents = true
panel.collectionBehavior = [.canJoinAllSpaces, .stationary, .fullScreenAuxiliary, .ignoresCycle]
let view = GhostView(frame: NSRect(origin: .zero, size: screen.frame.size))
panel.contentView = view
panel.orderFrontRegardless()
let t0 = Date()
var everKey = false, everActive = false, framesVisible = 0, framesHidden = 0, frame = 0, onScreen = false
Timer.scheduledTimer(withTimeInterval: 1.0 / 60, repeats: true) { _ in
    frame += 1
    if frame % 3 == 1 { view.visible = visibleRects() }      // 20 Hz window-list query
    view.t = Date().timeIntervalSince(t0)
    everKey = everKey || panel.isKeyWindow; everActive = everActive || app.isActive
    if let v = view.visible { onScreen = true; if v.contains(where: { $0.contains(CGPoint(x: sx, y: sy)) }) { framesVisible += 1 } else { framesHidden += 1 } } else { framesHidden += 1 }
    view.needsDisplay = true
    if view.t > secs {
        let d: [String: Any] = ["overlayEverKey": everKey, "overlayEverActive": everActive, "ignoresMouse": panel.ignoresMouseEvents,
                                "framesPointVisible": framesVisible, "framesPointHidden": framesHidden, "targetOnScreen": onScreen,
                                "visibleRects": (view.visible ?? []).map { [$0.minX, $0.minY, $0.width, $0.height] }]
        try? JSONSerialization.data(withJSONObject: d).write(to: URL(fileURLWithPath: logPath))
        exit(0)
    }
}
app.run()
