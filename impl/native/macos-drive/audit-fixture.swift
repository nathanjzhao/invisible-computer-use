import Cocoa
import CoreGraphics
import ApplicationServices
import Darwin

func emit(_ value: [String: Any]) {
    let data = try! JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
    print(String(data: data, encoding: .utf8)!)
}
func state() -> [String: Any] {
    let pointer = CGEvent(source: nil)!.location
    let inputTypes:[CGEventType] = [.leftMouseDown,.leftMouseUp,.rightMouseDown,.rightMouseUp,.keyDown,.keyUp,.flagsChanged,.otherMouseDown,.otherMouseUp]
    let counts=inputTypes.map { CGEventSource.counterForEventType(.hidSystemState,eventType:$0) }
    var psn = [UInt8](repeating: 0, count: 8)
    var owner: Any = NSNull()
    if let lib = dlopen("/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight", RTLD_LAZY),
       let symbol = dlsym(lib, "_SLPSGetFrontProcess") {
        typealias ReadFront = @convention(c) (UnsafeMutableRawPointer) -> Int32
        let read = unsafeBitCast(symbol, to: ReadFront.self)
        if psn.withUnsafeMutableBytes({ read($0.baseAddress!) }) == 0 { owner = psn }
    }
    return ["front_pid": NSWorkspace.shared.frontmostApplication?.processIdentifier ?? -1,
            "pointer": ["x": pointer.x, "y": pointer.y], "hid_input_counts":counts, "windowserver_front_psn": owner]
}
let arguments = CommandLine.arguments
func inputEventRecord(_ event: NSEvent) -> [String:Any] {
    let local=event.locationInWindow
    let hit=event.window?.contentView?.hitTest(local)
    var row:[String:Any] = ["type":event.type.rawValue,
        "window_id":event.windowNumber,"x":local.x,"y":local.y,
        "hit_identifier":hit?.accessibilityIdentifier() ?? "",
        "hit_class":hit.map {String(describing:type(of:$0))} ?? "nil"]
    switch event.type {
    case .leftMouseDown,.leftMouseUp,.rightMouseDown,.rightMouseUp,.otherMouseDown,.otherMouseUp:
        row["click_count"]=event.clickCount; row["button"]=event.buttonNumber
    case .scrollWheel:
        row["delta_y"]=event.scrollingDeltaY; row["delta_x"]=event.scrollingDeltaX
        row["precise"]=event.hasPreciseScrollingDeltas
    default: break // Drag/move events need no mouse-down-only accessors.
    }
    return row
}
if arguments.count == 2 && arguments[1] == "inspect-wheel" {
    let event=CGEvent(scrollWheelEvent2Source:nil,units:.line,wheelCount:2,wheel1:-1,wheel2:0,wheel3:0)!
    emit(inputEventRecord(NSEvent(cgEvent:event)!))
    exit(0)
}
if arguments.count == 4 && ["ax-observe", "ax-enable-observe"].contains(arguments[1]) {
    let pid = pid_t(arguments[2])!, requested = UInt32(arguments[3])!
    let app = AXUIElementCreateApplication(pid)
    AXUIElementSetMessagingTimeout(app, 2)
    var enable: [String:Any] = [:]
    if arguments[1] == "ax-enable-observe" {
        let first = AXUIElementSetAttributeValue(app, "AXManualAccessibility" as CFString, kCFBooleanTrue)
        enable["manual_error"] = first.rawValue
        if first == .attributeUnsupported {
            enable["enhanced_error"] = AXUIElementSetAttributeValue(app, "AXEnhancedUserInterface" as CFString, kCFBooleanTrue).rawValue
        }
    }
    var value: CFTypeRef?
    let error = AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &value)
    var rows = [[String:Any]]()
    if error == .success, let array = value as? [AXUIElement] {
        for window in array {
            var title: CFTypeRef?
            let titleError = AXUIElementCopyAttributeValue(window, kAXTitleAttribute as CFString, &title)
            var number: UInt32 = 0; var windowError: Int32 = -1
            if let lib = dlopen("/System/Library/Frameworks/ApplicationServices.framework/ApplicationServices", RTLD_LAZY),
               let symbol = dlsym(lib, "_AXUIElementGetWindow") {
                typealias GetWindow = @convention(c) (AXUIElement, UnsafeMutablePointer<UInt32>) -> Int32
                windowError = unsafeBitCast(symbol, to: GetWindow.self)(window, &number)
            }
            rows.append(["window_id":number, "window_id_ax_error":windowError,
                         "title":title as? String ?? "", "title_ax_error":titleError.rawValue])
        }
    }
    let target = NSRunningApplication(processIdentifier: pid)
    let cg = (CGWindowListCopyWindowInfo(.optionIncludingWindow, requested) as? [[String:Any]])?.first
    emit(["observer_pid":ProcessInfo.processInfo.processIdentifier, "observer_trusted":AXIsProcessTrusted(),
          "observer_bundle":Bundle.main.bundleIdentifier ?? "", "target_pid":pid,
          "target_bundle":target?.bundleIdentifier ?? "", "target_hidden":target?.isHidden ?? false, "target_executable":target?.executableURL?.path ?? "",
          "enablement":enable, "ax_error":error.rawValue, "value_kind":value == nil ? "null" : String(describing: type(of:value!)),
          "windows":rows, "cg_window":cg ?? [:]])
    exit(0)
}
// Bounded diagnostic only: independent public Quartz wheel construction.
final class WheelTrace {
    let file: FileHandle
    init(_ path: String) { FileManager.default.createFile(atPath:path,contents:nil); file=try! FileHandle(forWritingTo:URL(fileURLWithPath:path)) }
    func write(_ value: [String:Any]) { var data=try! JSONSerialization.data(withJSONObject:value,options:[.sortedKeys]);data.append(10);file.write(data) }
}
if arguments.count == 4 && arguments[1] == "trace-wheel" {
    let pid=pid_t(arguments[2])!
    guard NSRunningApplication(processIdentifier:pid)?.bundleIdentifier=="local.icu.audit-fixture" else {exit(4)}
    let sink=WheelTrace(arguments[3]); let pointer=Unmanaged.passUnretained(sink).toOpaque()
    let mask:CGEventMask=(1<<22)|(1<<5)|(1<<25)|(1<<26)
    guard let tap=CGEvent.tapCreateForPid(pid:pid,place:.headInsertEventTap,options:.listenOnly,eventsOfInterest:mask,callback:{ _,type,event,info in
        if let info=info {
            Unmanaged<WheelTrace>.fromOpaque(info).takeUnretainedValue().write([
                "type":type.rawValue,"pid":event.getIntegerValueField(.eventTargetUnixProcessID),
                "window":event.getIntegerValueField(CGEventField(rawValue:51)!),"x":event.location.x,"y":event.location.y,
                "source":event.getIntegerValueField(.eventSourceUnixProcessID),"delta":event.getIntegerValueField(.scrollWheelEventDeltaAxis1)])
        }
        return Unmanaged.passUnretained(event)
    },userInfo:pointer) else {sink.write(["error":"tap unavailable","trusted":AXIsProcessTrusted()]);exit(5)}
    let source=CFMachPortCreateRunLoopSource(nil,tap,0)!
    CFRunLoopAddSource(CFRunLoopGetCurrent(),source,.commonModes)
    sink.write(["ready":true,"target_pid":pid,"observer_pid":ProcessInfo.processInfo.processIdentifier,"trusted":AXIsProcessTrusted()])
    withExtendedLifetime(sink) { CFRunLoopRunInMode(.defaultMode,20,false) }
    CGEvent.tapEnable(tap:tap,enable:false)
    exit(0)
}
if arguments.count == 7 && arguments[1] == "wheel-probe" {
    guard ProcessInfo.processInfo.environment["ICU_BOUNDED_TEST"] == "1" else { exit(3) }
    let pid=pid_t(arguments[2])!, wid=UInt32(arguments[3])!, x=Double(arguments[4])!, y=Double(arguments[5])!
    let variant=arguments[6]
    let cg=(CGWindowListCopyWindowInfo(.optionIncludingWindow,wid) as? [[String:Any]])?.first
    guard (cg?[kCGWindowOwnerPID as String] as? Int32)==pid,
          NSRunningApplication(processIdentifier:pid)?.bundleIdentifier=="local.icu.audit-fixture",
          let b=cg?[kCGWindowBounds as String] as? [String:Double] else { exit(4) }
    let sx=b["X"]!+x, sy=b["Y"]!+y
    let source=variant.hasPrefix("nil") ? nil : CGEventSource(stateID:.hidSystemState)
    let pixel=variant.hasSuffix("pixel")
    if variant.contains("primer") {
        let move=CGEvent(mouseEventSource:source,mouseType:.mouseMoved,mouseCursorPosition:CGPoint(x:sx,y:sy),mouseButton:.left)!
        move.setIntegerValueField(CGEventField(rawValue:51)!,value:Int64(wid))
        move.setIntegerValueField(.mouseEventWindowUnderMousePointer,value:Int64(wid))
        move.setIntegerValueField(.mouseEventWindowUnderMousePointerThatCanHandleThisEvent,value:Int64(wid))
        move.postToPid(pid)
        Thread.sleep(forTimeInterval:0.03)
    }
    let event:CGEvent
    if variant.contains("mousebase") {
        event=NSEvent.mouseEvent(with:.mouseMoved,location:CGPoint(x:x,y:b["Height"]!-y),modifierFlags:[],timestamp:ProcessInfo.processInfo.systemUptime,windowNumber:Int(wid),context:nil,eventNumber:1,clickCount:0,pressure:0)!.cgEvent!
        event.type = .scrollWheel
        event.setIntegerValueField(.scrollWheelEventDeltaAxis1,value:-3)
        event.setDoubleValueField(.scrollWheelEventFixedPtDeltaAxis1,value:-3)
        event.setIntegerValueField(.scrollWheelEventPointDeltaAxis1,value:-30)
    } else {
        event=CGEvent(scrollWheelEvent2Source:source,units:pixel ? .pixel : .line,wheelCount:2,wheel1:pixel ? -40 : -3,wheel2:0,wheel3:0)!
    }
    if !variant.contains("mousebase") { event.location=CGPoint(x:sx,y:sy) }
    event.setIntegerValueField(CGEventField(rawValue:51)!,value:Int64(wid))
    event.setIntegerValueField(.eventTargetUnixProcessID,value:Int64(pid))
    if variant.contains("tagged"), let symbol=dlsym(dlopen("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics",RTLD_LAZY),"CGEventSetWindowLocation") {
        typealias SetLocal = @convention(c) (UnsafeMutableRawPointer,Double,Double)->Void
        unsafeBitCast(symbol,to:SetLocal.self)(Unmanaged.passUnretained(event).toOpaque(),x,y)
    }
    if variant.contains("continuous") { event.setIntegerValueField(.scrollWheelEventIsContinuous,value:1) }
    if variant.contains("phase") { event.setIntegerValueField(.scrollWheelEventScrollPhase,value:2) }
    if variant.contains("group") { event.setIntegerValueField(CGEventField(rawValue:58)!,value:12345) }
    if variant.contains("cmd") { event.flags = .maskCommand }
    if variant.contains("nstype") { event.setIntegerValueField(CGEventField(rawValue:55)!,value:22) }
    let ns=NSEvent(cgEvent:event)
    emit(["group":event.getIntegerValueField(CGEventField(rawValue:58)!),"field55":event.getIntegerValueField(CGEventField(rawValue:55)!),"ns_field55":ns?.cgEvent?.getIntegerValueField(CGEventField(rawValue:55)!) ?? -1,
          "continuous":event.getIntegerValueField(.scrollWheelEventIsContinuous),"ns_precise":ns?.hasPreciseScrollingDeltas ?? false,
          "ns_type":ns?.type.rawValue ?? 0,"ns_window":ns?.windowNumber ?? -1,"ns_x":ns?.locationInWindow.x ?? 0,"ns_y":ns?.locationInWindow.y ?? 0,"ns_delta":ns?.scrollingDeltaY ?? 0,
          "variant":variant,"type":event.type.rawValue,"window":event.getIntegerValueField(CGEventField(rawValue:51)!),
          "delta":event.getIntegerValueField(.scrollWheelEventDeltaAxis1),"points":event.getIntegerValueField(.scrollWheelEventPointDeltaAxis1),
          "source":event.getIntegerValueField(.eventSourceStateID),"x":event.location.x,"y":event.location.y])
    if variant.contains("gesture") {
        for (phase,delta) in [(128,0),(1,-20),(2,-20),(4,0)] {
            let e=CGEvent(scrollWheelEvent2Source:source,units:.pixel,wheelCount:2,wheel1:Int32(delta),wheel2:0,wheel3:0)!
            e.location=CGPoint(x:sx,y:sy)
            e.setIntegerValueField(CGEventField(rawValue:51)!,value:Int64(wid))
            e.setIntegerValueField(.eventTargetUnixProcessID,value:Int64(pid))
            e.setIntegerValueField(.scrollWheelEventScrollPhase,value:Int64(phase))
            e.postToPid(pid)
            Thread.sleep(forTimeInterval:0.02)
        }
    } else if variant.contains("roundtrip"), let converted=ns?.cgEvent { converted.postToPid(pid) }
    else { event.postToPid(pid) }
    exit(0)
}
if arguments.count > 1 && arguments[1] == "state" { emit(state()); exit(0) }
// Setup/cleanup restore is conditional. CGWarpMouseCursorPosition quantizes
// fractional coordinates on this host, so never call it for an unmoved pointer.
if (arguments.count == 5 && arguments[1] == "restore") ||
   (arguments.count == 3 && arguments[1] == "restore-focus") {
    let pid = pid_t(arguments[2])!
    if NSWorkspace.shared.frontmostApplication?.processIdentifier != pid {
        NSRunningApplication(processIdentifier: pid)?.activate(options: [.activateIgnoringOtherApps])
        let deadline = Date().addingTimeInterval(2)
        while NSWorkspace.shared.frontmostApplication?.processIdentifier != pid && Date() < deadline {
            RunLoop.current.run(until: Date().addingTimeInterval(0.02))
        }
    }
    if arguments[1] == "restore" {
        let saved = CGPoint(x: Double(arguments[3])!, y: Double(arguments[4])!)
        if CGEvent(source: nil)!.location != saved { CGWarpMouseCursorPosition(saved) }
    }
    emit(state()); exit(0)
}
if arguments.count != 3 || arguments[1] != "fixture" { exit(2) }
let output = URL(fileURLWithPath: arguments[2], isDirectory: true)
func record(_ kind: String, _ extra: [String: Any] = [:]) {
    var row = extra; row["kind"] = kind; row["time"] = Date().timeIntervalSince1970
    var data = try! JSONSerialization.data(withJSONObject: row, options: [.sortedKeys]); data.append(10)
    let url = output.appendingPathComponent("fixture-events.jsonl")
    if !FileManager.default.fileExists(atPath: url.path) { FileManager.default.createFile(atPath: url.path, contents: nil) }
    let file = try! FileHandle(forWritingTo: url); file.seekToEndOfFile(); file.write(data); try! file.close()
}
@objc(AuditApplication)
final class AuditApplication: NSApplication {
    // NSTextView consumes a drag in its own tracking loop, bypassing sendEvent
    // and local monitors. Observe the actual dequeue without changing delivery.
    override func nextEvent(matching mask: NSEvent.EventTypeMask, until expiration: Date?, inMode mode: RunLoop.Mode, dequeue flag: Bool) -> NSEvent? {
        let event = super.nextEvent(matching:mask, until:expiration, inMode:mode, dequeue:flag)
        if flag, let event=event, [.leftMouseDragged,.rightMouseDragged,.otherMouseDragged].contains(event.type) {
            record("tracking_event", inputEventRecord(event))
        }
        return event
    }
    override func reportException(_ exception: NSException) {
        record("appkit_exception", ["name":exception.name.rawValue,"reason":exception.reason ?? ""])
    }
    override func sendEvent(_ event: NSEvent) {
        if event.type == .scrollWheel {
            record("application_scroll_dispatch", ["active":isActive,"on_active_space":event.window?.isOnActiveSpace ?? false,"main_window":mainWindow?.windowNumber ?? -1,"flags":event.modifierFlags.rawValue,"uptime":ProcessInfo.processInfo.systemUptime,"timestamp":event.timestamp,"resolved_window":event.window?.windowNumber ?? -1,"lookup_window":window(withWindowNumber:event.windowNumber)?.windowNumber ?? -1,"window_id":event.windowNumber,"x":event.locationInWindow.x,"y":event.locationInWindow.y,"dy":event.scrollingDeltaY])
        }
        super.sendEvent(event)
    }
}
final class AuditWindow: NSWindow {
    override func sendEvent(_ event: NSEvent) {
        if event.type == .scrollWheel {
            let hit = contentView?.hitTest(event.locationInWindow)
            record("window_scroll_dispatch", ["window_id":windowNumber,"x":event.locationInWindow.x,"y":event.locationInWindow.y,"hit":hit.map {String(describing:type(of:$0))} ?? "nil"])
        }
        super.sendEvent(event)
    }
    override func orderOut(_ sender: Any?) {
        record("window_order_out", ["stack":Thread.callStackSymbols]); super.orderOut(sender)
    }
    override func close() { record("window_close", ["stack":Thread.callStackSymbols]); super.close() }
    override func miniaturize(_ sender: Any?) { record("window_miniaturize", ["stack":Thread.callStackSymbols]); super.miniaturize(sender) }
    deinit { record("window_deallocated") }
}
final class AuditButton: NSButton {
    override func otherMouseDown(with event: NSEvent) { record("middle_button_down") }
    override func otherMouseUp(with event: NSEvent) { record("middle_button_up") }
}
final class AuditText: NSTextView {
    override func rightMouseDown(with event: NSEvent) {
        record("right_down")
        let menu = NSMenu(title: "ICU Audit Menu")
        menu.addItem(withTitle: "Audit menu marker", action: nil, keyEquivalent: "")
        menu.addItem(withTitle: "Second audit item", action: nil, keyEquivalent: "")
        let captureBounds=Timer(timeInterval:0.06,repeats:false) { _ in
            let own=ProcessInfo.processInfo.processIdentifier
            let rows=(CGWindowListCopyWindowInfo(.optionOnScreenOnly,kCGNullWindowID) as? [[String:Any]] ?? []).filter {
                ($0[kCGWindowOwnerPID as String] as? Int32)==own && ($0[kCGWindowLayer as String] as? Int ?? 0)>0
            }
            record("menu_surface_bounds", ["surfaces":rows])
        }
        RunLoop.current.add(captureBounds,forMode:.common)
        NSMenu.popUpContextMenu(menu, with: event, for: self)
        record("menu_closed")
    }
    override func otherMouseDown(with event: NSEvent) { record("middle_down"); super.otherMouseDown(with: event) }
    override func otherMouseUp(with event: NSEvent) { record("middle_up"); super.otherMouseUp(with: event) }
    override func mouseDown(with event: NSEvent) { record("left_down", ["click_count": event.clickCount]); super.mouseDown(with: event); record("text_mouse_down_returned") }
    override func mouseDragged(with event: NSEvent) { record("dragged"); super.mouseDragged(with: event) }
    override func mouseUp(with event: NSEvent) { record("left_up"); super.mouseUp(with: event) }
    override func scrollWheel(with event: NSEvent) { record("scroll", ["dy": event.scrollingDeltaY]); super.scrollWheel(with: event) }
}
final class App: NSObject, NSApplicationDelegate {
    deinit { record("fixture_delegate_deallocated") }
    var window: NSWindow!
    var count = 0
    var scroll: NSScrollView!
    var text: AuditText!
    func reportState() {
        guard let scroll = scroll, let text = text else { return }
        let range = text.selectedRange()
        let selected = (text.string as NSString).substring(with: range)
        let data: [String: Any] = ["scroll_y":scroll.contentView.documentVisibleRect.origin.y,
            "scroll_x":scroll.contentView.documentVisibleRect.origin.x,
            "selected_text":selected, "selected_location":range.location,"selected_length":range.length,
            "visible_height":scroll.contentView.documentVisibleRect.height,
            "window_id":window.windowNumber, "window_is_key":window.isKeyWindow,
            "app_active":NSApplication.shared.isActive,"app_key_window":NSApplication.shared.keyWindow?.windowNumber ?? -1,
            "pid":ProcessInfo.processInfo.processIdentifier]
        try! JSONSerialization.data(withJSONObject:data,options:[.sortedKeys]).write(to:output.appendingPathComponent("fixture-state.json"),options:.atomic)
    }
    func applicationDidBecomeActive(_ notification: Notification) {
        window?.makeMain(); window?.makeKey(); reportState()
    }
    @objc func clicked(_ sender: NSButton) {
        count += 1
        sender.title = count % 2 == 0 ? "Play" : "Pause"
        record("button_action", ["count": count, "label": sender.title])
    }
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .leftMouseUp, .leftMouseDragged, .rightMouseDown, .otherMouseDown, .otherMouseUp, .scrollWheel]) { event in
            record("input_event", inputEventRecord(event))
            return event
        }
        window = AuditWindow(contentRect: NSRect(x: 120, y: 180, width: 640, height: 480),
                          styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        // The user's front app may own a full-screen Space. Keep only this
        // temporary utility on that Space after restoring the original front.
        window.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .canJoinAllApplications]
        window.title = "ICU Audit Fixture"
        NotificationCenter.default.addObserver(forName:NSWindow.didChangeOcclusionStateNotification, object:window, queue:.main) { [weak self] _ in
            guard let w = self?.window else { return }
            record("window_visibility", ["visible":w.isVisible,"miniaturized":w.isMiniaturized,"occlusion":w.occlusionState.rawValue,"app_hidden":NSApplication.shared.isHidden,"self_ax_count":NSApplication.shared.accessibilityWindows()?.count ?? -1])
        }
        window.isReleasedWhenClosed = false
        if ProcessInfo.processInfo.environment["AUDIT_FIXTURE_MOUSE_MOVED"] == "1" { window.acceptsMouseMovedEvents = true }
        let button = AuditButton(title: "Play", target: self, action: #selector(clicked(_:)))
        button.frame = NSRect(x: 30, y: 405, width: 140, height: 40)
        button.setAccessibilityIdentifier("audit-play-control")
        window.contentView!.addSubview(button)
        scroll = NSScrollView(frame: NSRect(x: 20, y: 20, width: 600, height: 360))
        scroll.hasVerticalScroller = true
        text = AuditText(frame: NSRect(x: 0, y: 0, width: 580, height: 1600))
        text.string = "the quick brown fox jumps over the lazy dog\n" + String(repeating: "Audit fixture scroll and drag text\n", count: 65)
        text.font = NSFont.monospacedSystemFont(ofSize: 16, weight: .regular)
        text.setAccessibilityIdentifier("audit-text")
        scroll.documentView = text; window.contentView!.addSubview(scroll)
        scroll.contentView.postsBoundsChangedNotifications = true
        NotificationCenter.default.addObserver(forName:NSView.boundsDidChangeNotification,object:scroll.contentView,queue:.main) { [weak self] _ in
            self?.reportState()
            if let y = self?.scroll.contentView.documentVisibleRect.origin.y { record("scroll_position", ["y":y]) }
        }
        NotificationCenter.default.addObserver(forName:NSTextView.didChangeSelectionNotification,object:text,queue:.main) { [weak self] _ in self?.reportState() }

        window.orderFrontRegardless()
        if ProcessInfo.processInfo.environment["AUDIT_FIXTURE_KEY_WINDOW"] == "1" { window.makeKey() }
        window.makeFirstResponder(text)
        // Report after AppKit's launch/window animation completes. The harness
        // restores the original foreground app before it tests any action.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) { [self] in
        let info = (CGWindowListCopyWindowInfo(.optionIncludingWindow, CGWindowID(window.windowNumber)) as? [[String: Any]])?.first
        let ready: [String: Any] = ["pid": ProcessInfo.processInfo.processIdentifier, "window_id": window.windowNumber,
                                   "bounds": info?[kCGWindowBounds as String] ?? [:],
                                   "bundle_id": Bundle.main.bundleIdentifier ?? "", "application_class":String(describing:type(of:NSApplication.shared)),
                                   "self_accessibility_windows": NSApplication.shared.accessibilityWindows()?.count ?? -1,
                                   "self_window_is_accessibility_element": window.isAccessibilityElement()]
        reportState()
        try! JSONSerialization.data(withJSONObject: ready).write(to: output.appendingPathComponent("fixture-ready.json"))
        }
        Timer.scheduledTimer(withTimeInterval: 180, repeats: false) { _ in NSApplication.shared.terminate(nil) }
    }
}
let application = AuditApplication.shared
let delegate = App(); application.delegate = delegate
application.setActivationPolicy(.accessory)
application.run()
