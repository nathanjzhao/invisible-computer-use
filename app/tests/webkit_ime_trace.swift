// Records the DOM keyboard/composition/input event order that a real macOS
// WKWebView produces for a textarea, so manual-control IME handling can be
// checked against the engine Copilot actually ships in. Disposable Macs only:
// it takes the foreground so the system input method targets it.
//
//   webkit_ime_trace <out.json> <seconds>
// Prints READY with the textarea's screen point; the caller types into it.
import AppKit
import WebKit

let out = URL(fileURLWithPath: CommandLine.arguments[1])
let seconds = Double(CommandLine.arguments.count > 2 ? CommandLine.arguments[2] : "20")!
let app = NSApplication.shared
app.setActivationPolicy(.regular)
var events: [Any] = []

final class Sink: NSObject, WKScriptMessageHandler {
    func userContentController(_ c: WKUserContentController, didReceive m: WKScriptMessage) { events.append(m.body) }
}
let html = """
<!doctype html><body style="margin:0"><textarea id=t style="position:absolute;left:20px;top:20px;width:300px;height:80px"></textarea>
<script>
const t = document.getElementById('t'); const post = o => webkit.messageHandlers.log.postMessage(o);
for (const type of ['keydown','keyup','beforeinput','input','compositionstart','compositionupdate','compositionend'])
  t.addEventListener(type, e => post({type, key: e.key ?? null, code: e.code ?? null, keyCode: e.keyCode ?? null,
    isComposing: e.isComposing ?? null, data: e.data ?? null, inputType: e.inputType ?? null, value: t.value, t: performance.now()}));
t.focus();
</script>
"""
let config = WKWebViewConfiguration()
let sink = Sink()
config.userContentController.add(sink, name: "log")
let window = NSWindow(contentRect: NSRect(x: 200, y: 400, width: 360, height: 140), styleMask: [.titled], backing: .buffered, defer: false)
window.title = "WebKit IME trace"
let view = WKWebView(frame: window.contentView!.bounds, configuration: config)
window.contentView!.addSubview(view)
view.loadHTMLString(html, baseURL: nil)
window.makeKeyAndOrderFront(nil)
app.activate(ignoringOtherApps: true)
DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
    window.makeFirstResponder(view)
    let top = NSScreen.screens[0].frame.maxY
    // textarea centre: content origin + (20+150, 20+40) in top-left page coordinates
    let content = window.convertToScreen(window.contentView!.frame)
    print("READY \(Int(content.minX + 170)) \(Int(top - content.maxY + 60))"); fflush(stdout)
}
DispatchQueue.main.asyncAfter(deadline: .now() + seconds) {
    try! JSONSerialization.data(withJSONObject: events, options: [.prettyPrinted, .sortedKeys]).write(to: out)
    exit(0)
}
app.run()
