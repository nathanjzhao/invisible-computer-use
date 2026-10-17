// Runs the production frame loader in WebKit without a native window or input.
import AppKit
import WebKit

let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
let originalFront = NSWorkspace.shared.frontmostApplication?.processIdentifier
var becameForeground = app.isActive
let activationObserver = NSWorkspace.shared.notificationCenter.addObserver(
    forName: NSWorkspace.didActivateApplicationNotification, object: nil, queue: .main
) { notification in
    if let activated = notification.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication,
       activated.processIdentifier == ProcessInfo.processInfo.processIdentifier { becameForeground = true }
}
let module = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
let script = try String(contentsOfFile: CommandLine.arguments[2], encoding: .utf8)
let png = Data(base64Encoded: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=")!
let html = "<!doctype html><body><script type='module'>\(script)</script></body>"
var requests: [String] = []
var stops: [String] = []

func finish(_ result: [String: Any]) {
    var report = result
    let focusKept = originalFront == NSWorkspace.shared.frontmostApplication?.processIdentifier
    report["requests"] = requests
    report["stopped_requests"] = stops
    report["frontmost_unchanged"] = focusKept
    report["probe_never_activated"] = !becameForeground && !app.isActive
    print(String(data: try! JSONSerialization.data(withJSONObject: report, options: [.sortedKeys]), encoding: .utf8)!)
    fflush(stdout)
    exit(result["ok"] as? Bool == true && !becameForeground && !app.isActive ? 0 : 1)
}

final class Scheme: NSObject, WKURLSchemeHandler {
    func webView(_ webView: WKWebView, start task: WKURLSchemeTask) {
        let url = task.request.url!
        requests.append(url.absoluteString)
        if url.path == "/hang" { return }
        let status = url.path == "/refused" ? 204 : 200
        let payload: Data
        let mime: String
        if url.scheme == "frame" {
            payload = status == 204 ? Data() : png
            mime = "image/png"
        } else if url.path == "/status" {
            payload = try! JSONSerialization.data(withJSONObject: ["requests": requests, "stopped_requests": stops])
            mime = "application/json"
        } else if url.path == "/frames.js" {
            payload = module
            mime = "text/javascript"
        } else {
            payload = html.data(using: .utf8)!
            mime = "text/html"
        }
        let response = HTTPURLResponse(url: url, statusCode: status, httpVersion: "HTTP/1.1", headerFields: [
            "Content-Type": mime, "Access-Control-Allow-Origin": "*", "Cache-Control": "no-store"
        ])!
        task.didReceive(response)
        task.didReceive(payload)
        task.didFinish()
    }
    func webView(_ webView: WKWebView, stop task: WKURLSchemeTask) {
        stops.append(task.request.url!.absoluteString)
    }
}

final class Results: NSObject, WKScriptMessageHandler {
    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        finish(message.body as? [String: Any] ?? ["ok": false, "error": "invalid result"])
    }
}
let config = WKWebViewConfiguration()
// The app captures only while visible. Exercise that scheduling behavior while
// keeping this probe hidden: otherwise WebKit suspends its JavaScript timers.
if #available(macOS 14.0, *) { config.preferences.inactiveSchedulingPolicy = .none }
let scheme = Scheme()
let results = Results()
config.setURLSchemeHandler(scheme, forURLScheme: "frame")
config.setURLSchemeHandler(scheme, forURLScheme: "tauri")
config.userContentController.add(results, name: "result")
let view = WKWebView(frame: NSRect(x: 0, y: 0, width: 300, height: 200), configuration: config)
view.load(URLRequest(url: URL(string: "tauri://localhost")!))
DispatchQueue.main.asyncAfter(deadline: .now() + 30) { finish(["ok": false, "error": "probe timeout"]) }
app.run()
