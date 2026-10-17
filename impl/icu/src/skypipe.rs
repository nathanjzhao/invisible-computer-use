//! The length-prefixed Sky native pipe (specs/00-COVERAGE.md:151, specs/13-protocol-tools-browser.md:59-61).
//!
//! Codex's `@oai/sky` client (`targets/mac/native-pipe.js`, `client.js`) does not speak MCP. It
//! connects to a unix socket and exchanges JSON-RPC 2.0 messages, each preceded by a 4-byte
//! little-endian length (at most 8 MiB): a `ping {clientApiVersion}` handshake whose answer must
//! echo the client's version, then `request {clientApiVersion, request, requestType,
//! deadlineUnixMilliseconds, codexTurnMetadata}` for `ComputerUseIPCListAppsRequest`,
//! `...AppPolicyRequest`, `...AppStartRequest`, `...AppGetSkyshotRequest` and
//! `...AppPerformActionRequest`. cua-driver speaks newline JSON instead, and `icu mcp` speaks MCP.
//! This module is the third door: `icu sky-pipe` serves that exact wire protocol over the layer, so
//! the unmodified Codex client code can drive any app this layer can, with the layer's policy,
//! lineage, packs and error codes behind it.
//!
//! The shapes are the ones captured from the real service in
//! `study/audit/golden-trace-prep-2026-09-25/validation/probe/sky-ipc.jsonl`
//! (`app`/`skyshot`/`screenshot`, `target`/`decision`, the `-10010` and `-10005` errors); the
//! conformance suite replays that capture's requests against this server and compares the shapes.
//!
//! What differs from the real service, on purpose or for lack of evidence:
//! - The client names an APP; the layer works on windows. An app resolves to its frontmost window,
//!   and the window an `AppGetSkyshot` read is the one later actions go to. A vanished window or
//!   changed process requires a new observation; input never falls through to another window.
//! - The screenshot is a PNG file (`image/png`) in the layer's state dir, not a JPEG in the
//!   service's temp dir; the `file://` URL and `mimeType` fields are the real ones.
//! - Coordinates are window-relative logical points, as in window2; Sky's own pixel space for these
//!   requests is not recovered. `pages` for `scroll` is INFERRED as 10 wheel notches.
//! - Approval is the client's: in Codex the JS client raises the elicitation and the service only
//!   reports `decision` (allowed / denied / forbidden). Here an action on an app that still needs
//!   approval under `ICU_REQUIRE_APP_APPROVAL` answers -10006; set it to 0 when the client
//!   approves for itself, as Codex's does.
//! - Audio requests are a non-goal (spec 16) and answer -10003.
//! - Only plain-text paste and text selection are supported. Other formats and cursor-placement
//!   modes are refused instead of performing a different edit. Absolute request deadlines are
//!   checked after queuing and before input; in-flight synchronous actions cannot be cancelled.
//! - A unix socket (mode 0600 in a 0700 directory, and the peer's uid must be ours: the analogue
//!   of `ComputerUseIPCSenderContext`, not its audit-token check) or, on Windows, a named pipe
//!   (`\\.\pipe\icu-computeruse-<user>` unless told otherwise) whose DACL names this user and
//!   SYSTEM only, closed to remote clients, and created with `FILE_FLAG_FIRST_PIPE_INSTANCE` so a
//!   process that already owns the name is an error here and not a server we hand requests to.
//!   Node's `net.createConnection` takes either path, so Codex's client code connects unchanged.
use super::*;
use std::io::{Read, Write};

pub const API_VERSION: &str = "CodexComputerUseIPC-5";
/// `k` in native-pipe.js: a frame larger than this is refused.
pub const MAX_FRAME_BYTES: usize = 8_388_608;
const REQ_PREFIX: &str = "ComputerUseIPC";

/// `ServerErrorCode` numbers this server answers with (errors.js).
pub const ERR_COULD_NOT_RESOLVE_REQUEST_TYPE: i64 = -10003;
pub const ERR_UNKNOWN: i64 = -10005;
pub const ERR_APP_NOT_ALLOWED: i64 = -10006;
pub const ERR_RUNNING_APPLICATION_NOT_FOUND: i64 = -10007;
pub const ERR_INVALID_APP: i64 = -10010;
pub const ERR_INCOMPATIBLE_CLIENT_VERSION: i64 = -10013;

#[derive(Debug, PartialEq, Eq)]
pub struct FrameError(pub String);

/// `encodeMessageFrame`: `u32` little-endian length, then the UTF-8 payload.
pub fn encode_frame(payload: &[u8]) -> Result<Vec<u8>, FrameError> {
    if payload.len() > MAX_FRAME_BYTES { return Err(FrameError(format!("Sky Computer Use native pipe frame is too large: {}", payload.len()))); }
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

/// `decodeMessageFrames`, incremental: bytes in, whole messages out, the remainder kept.
#[derive(Default)]
pub struct FrameDecoder { buf: Vec<u8> }

impl FrameDecoder {
    pub fn push(&mut self, data: &[u8]) -> Result<Vec<String>, FrameError> {
        self.buf.extend_from_slice(data);
        let mut out = Vec::new();
        let mut at = 0;
        while self.buf.len() - at >= 4 {
            let len = u32::from_le_bytes([self.buf[at], self.buf[at + 1], self.buf[at + 2], self.buf[at + 3]]) as usize;
            if len > MAX_FRAME_BYTES { return Err(FrameError(format!("Sky Computer Use native pipe frame is too large: {len}"))); }
            if self.buf.len() - at < 4 + len { break; }
            out.push(String::from_utf8(self.buf[at + 4..at + 4 + len].to_vec()).map_err(|_| FrameError("frame is not UTF-8".into()))?);
            at += 4 + len;
        }
        self.buf.drain(..at);
        Ok(out)
    }
}

fn rpc_result(id: &Value, result: Value) -> Value { json!({"jsonrpc": "2.0", "id": id, "result": result}) }
fn rpc_error(id: &Value, code: i64, message: impl Into<String>) -> Value { json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}}) }

/// Our typed refusal as the number and message the client's `SkyComputerUseError` carries.
pub fn error_number(e: &ToolError) -> i64 {
    match e.code() {
        // the policy's approval is the client's in Codex; unapproved here reads as not allowed
        Some(CODE_APP_APPROVAL_REQUIRED) => ERR_APP_NOT_ALLOWED,
        _ => e.structured.get("number").and_then(Value::as_i64).unwrap_or(ERR_UNKNOWN),
    }
}

pub struct SkyPipe {
    layer: Mutex<Layer>,
    /// app id -> the window its last `AppGetSkyshot` read, which later actions on that app target.
    current: Mutex<HashMap<String, ObservedWindow>>,
    shots: std::sync::atomic::AtomicU64,
}

struct AppRef { id: String, name: String, pid: Option<i64>, windows: Vec<Value> }

#[derive(Clone, Copy)]
struct ObservedWindow { id: u64, pid: Option<i64> }

impl SkyPipe {
    pub fn new(layer: Layer) -> SkyPipe { SkyPipe { layer: Mutex::new(layer), current: Mutex::new(HashMap::new()), shots: std::sync::atomic::AtomicU64::new(0) } }

    /// One JSON-RPC message in, the response out (None for something that is not a request).
    pub fn handle(&self, msg: &Value) -> Option<Value> {
        let id = msg.get("id").cloned()?;
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let supported = |p: &Value| p.get("clientApiVersion").and_then(Value::as_str) == Some(API_VERSION);
        Some(match method {
            "ping" => {
                if supported(&params) { rpc_result(&id, json!({"serverApiVersion": API_VERSION})) }
                else { rpc_error(&id, ERR_INCOMPATIBLE_CLIENT_VERSION, format!("Incompatible client API version {}; this server speaks {API_VERSION}", params.get("clientApiVersion").and_then(Value::as_str).unwrap_or("(none)"))) }
            }
            "request" => {
                if !supported(&params) { return Some(rpc_error(&id, ERR_INCOMPATIBLE_CLIENT_VERSION, format!("Incompatible client API version {}; this server speaks {API_VERSION}", params.get("clientApiVersion").and_then(Value::as_str).unwrap_or("(none)")))); }
                let request_type = params.get("requestType").and_then(Value::as_str).unwrap_or("");
                let request = params.get("request").cloned().unwrap_or(json!({}));
                let deadline = match params.get("deadlineUnixMilliseconds") {
                    None => None,
                    Some(v) => match v.as_u64() {
                        Some(d) => Some(d),
                        None => return Some(rpc_error(&id, ERR_UNKNOWN, "deadlineUnixMilliseconds must be a nonnegative integer")),
                    },
                };
                match self.dispatch(request_type, &request, deadline) {
                    Ok(v) => rpc_result(&id, v),
                    Err((code, message)) => rpc_error(&id, code, message),
                }
            }
            other => rpc_error(&id, ERR_COULD_NOT_RESOLVE_REQUEST_TYPE, format!("Could not resolve method {other:?}")),
        })
    }

    fn dispatch(&self, request_type: &str, req: &Value, deadline: Option<u64>) -> Result<Value, (i64, String)> {
        let layer = self.layer.lock().unwrap();
        let from_tool = |e: ToolError| (error_number(&e), e.message);
        // Check after acquiring the dispatch lock so time queued behind another client counts.
        exec::remaining_deadline_ms(deadline).map_err(from_tool)?;
        let kind = request_type.strip_prefix(REQ_PREFIX).and_then(|r| r.strip_suffix("Request")).unwrap_or("");
        match kind {
            "ListApps" => self.list_apps(&layer).map_err(from_tool),
            "AppPolicy" => self.app_policy(&layer, Self::app_of(req)?).map_err(from_tool),
            "AppStart" => self.app_start(&layer, Self::app_of(req)?).map_err(from_tool),
            "AppGetSkyshot" => self.get_skyshot(&layer, Self::app_of(req)?, req.get("disableDiff").and_then(Value::as_bool).unwrap_or(false)).map_err(from_tool),
            "AppPerformAction" => self.perform_action(&layer, Self::app_of(req)?, req.get("action").unwrap_or(&Value::Null), deadline).map_err(from_tool),
            _ => Err((ERR_COULD_NOT_RESOLVE_REQUEST_TYPE, format!("Could not resolve request type {request_type}"))),
        }
    }

    fn app_of(req: &Value) -> Result<&str, (i64, String)> {
        req.get("app").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).ok_or((ERR_UNKNOWN, "Computer Use server error -10005: app is required".into()))
    }

    /// The raw driver listing (pid, name, bundle id, running, active), keyed by bundle id.
    fn raw_apps(layer: &Layer) -> Vec<Value> {
        layer.cua().call("list_apps", json!({})).ok().and_then(|v| v.get("apps").and_then(Value::as_array).cloned()).unwrap_or_default()
    }

    fn resolve_app(&self, layer: &Layer, wanted: &str) -> Result<AppRef, ToolError> {
        let apps = layer.list_apps()?;
        let found: Vec<Value> = apps.as_array().map(|a| a.iter().filter(|x| facade::matches_app(x, wanted)).cloned().collect()).unwrap_or_default();
        match found.len() {
            0 => Err(ToolError::new(CODE_INVALID_APP, format!("Invalid app: {wanted}"))),
            1 => {
                let a = &found[0];
                let id = a["id"].as_str().unwrap_or(wanted).to_string();
                let pid = Self::raw_apps(layer).iter().find(|r| r.get("bundle_id").and_then(Value::as_str) == Some(id.as_str())).and_then(|r| r.get("pid").and_then(Value::as_i64)).filter(|p| *p > 0);
                Ok(AppRef { name: a["displayName"].as_str().unwrap_or(&id).to_string(), id, pid, windows: a["windows"].as_array().cloned().unwrap_or_default() })
            }
            _ => Err(ToolError::new(CODE_AMBIGUOUS_APP, format!("{wanted:?} matches {} apps", found.len()))),
        }
    }

    fn app_json(app: &AppRef) -> Value {
        let mut o = json!({"bundleIdentifier": app.id});
        if let Some(p) = app.pid { o["pid"] = json!(p); }
        o
    }

    /// The window an action on `app` goes to: the one its last skyshot read, else the frontmost.
    fn window_for(&self, app: &AppRef, for_input: bool) -> Result<Value, ToolError> {
        let pinned = self.current.lock().unwrap().get(&app.id).copied();
        if let Some(pin) = pinned {
            if pin.pid == app.pid {
                if let Some(window) = app.windows.iter().find(|w| w["id"].as_u64() == Some(pin.id)) {
                    return Ok(window.clone());
                }
            }
            if for_input {
                return Err(ToolError::new(CODE_RUNNING_APPLICATION_NOT_FOUND,
                    format!("The observed window of {} is no longer available; call getAppState again before input", app.name)));
            }
        }
        crate::facade::pick_window(&app.windows)
            .ok_or_else(|| ToolError::new(CODE_RUNNING_APPLICATION_NOT_FOUND, format!("Computer Use server error -10007: {} has no open window", app.name)))
    }

    fn list_apps(&self, layer: &Layer) -> Result<Value, ToolError> {
        let raw = Self::raw_apps(layer);
        let apps: Vec<Value> = raw.iter().filter_map(|a| {
            let id = a.get("bundle_id").and_then(Value::as_str).filter(|s| !s.is_empty())?;
            Some(json!({"bundleIdentifier": id, "displayName": a.get("name").and_then(Value::as_str).unwrap_or(id),
                        "isRunning": a.get("running").and_then(Value::as_bool).unwrap_or(true), "isFrontmost": a.get("active").and_then(Value::as_bool).unwrap_or(false)}))
        }).collect();
        Ok(json!(apps))
    }

    fn app_policy(&self, layer: &Layer, wanted: &str) -> Result<Value, ToolError> {
        let app = self.resolve_app(layer, wanted)?;
        let decision = layer.check_app_policy(&app.id).err().and_then(|e| e.structured.get("decision").and_then(Value::as_str).map(str::to_string)).unwrap_or_else(|| "allowed".into());
        Ok(json!({"target": {"bundleIdentifier": app.id, "displayName": app.name, "risk": layer.cfg.risk_level},
                  "allowPersistentApproval": layer.cfg.allow_persistent_approval, "decision": decision}))
    }

    fn app_start(&self, layer: &Layer, wanted: &str) -> Result<Value, ToolError> {
        let launch = layer.launch_app(&json!({"app": wanted}));
        // a refusal other than "not installed" is the answer; launching an app that is already
        // running is fine, so look the app up either way
        if let Err(e) = &launch { if matches!(e.code(), Some(CODE_APP_NOT_ALLOWED | CODE_INVALID_APP)) { return Err(launch.unwrap_err()); } }
        let app = self.resolve_app(layer, wanted)?;
        Ok(json!({"app": Self::app_json(&app)}))
    }

    fn get_skyshot(&self, layer: &Layer, wanted: &str, disable_diff: bool) -> Result<Value, ToolError> {
        let app = self.resolve_app(layer, wanted)?;
        layer.check_app_policy(&app.id)?;
        let window = self.window_for(&app, false)?;
        let mut req = json!({"window": window, "include_screenshot": true, "include_text": true});
        if disable_diff { req["disableDiff"] = json!(true); }
        let st = layer.get_window_state(&req)?;
        self.current.lock().unwrap().insert(app.id.clone(), ObservedWindow { id: window["id"].as_u64().unwrap_or(0), pid: app.pid });
        let tree = st["accessibility"]["tree"].as_str().unwrap_or("").to_string();
        // window2 carries the per-app pack inline at first touch; Sky returns it beside the text
        let (instructions, text) = match tree.strip_prefix("<app_specific_instructions>\n").and_then(|t| t.split_once("\n</app_specific_instructions>\n")) {
            Some((pack, rest)) => (Some(pack.to_string()), rest.to_string()),
            None => (None, tree),
        };
        let mut skyshot = json!({"text": text});
        if let Some(url) = st["screenshots"][0]["url"].as_str() {
            skyshot["screenshot"] = self.write_screenshot(layer, &app, url).unwrap_or(Value::Null);
        }
        let mut out = json!({"app": Self::app_json(&app), "skyshot": skyshot});
        if let Some(p) = instructions { out["appSpecificInstructions"] = json!(p); }
        Ok(out)
    }

    /// The window's screenshot as a PNG file, returned the way the real service returns its JPEG.
    fn write_screenshot(&self, layer: &Layer, app: &AppRef, data_url: &str) -> Option<Value> {
        use base64::Engine;
        let b64 = data_url.split_once("base64,")?.1;
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
        let dir = layer.cfg.state_dir.join("sky-screenshots");
        std::fs::create_dir_all(&dir).ok()?;
        let n = self.shots.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let safe: String = app.name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
        let path = dir.join(format!("{safe}-{}-{n}.png", std::process::id()));
        std::fs::write(&path, bytes).ok()?;
        let url = format!("file://{}", path.display()).replace(' ', "%20");
        Some(json!({"url": url, "mimeType": "image/png"}))
    }

    /// `at`: `{coordinate: {_0: [x, y]}}` or `{elementID: {_0: "7"}}` (click and scroll).
    fn at_of(at: &Value) -> Option<Result<(f64, f64), u64>> {
        if let Some(c) = at.pointer("/coordinate/_0").and_then(Value::as_array) {
            return Some(Ok((c.first()?.as_f64()?, c.get(1)?.as_f64()?)));
        }
        let id = at.pointer("/elementID/_0").or_else(|| at.get("elementID"))?;
        Some(Err(id.as_str().and_then(|s| s.parse().ok()).or_else(|| id.as_u64())?))
    }

    fn perform_action(&self, layer: &Layer, wanted: &str, action: &Value, deadline: Option<u64>) -> Result<Value, ToolError> {
        let app = self.resolve_app(layer, wanted)?;
        let window = self.window_for(&app, true)?;
        let bad = |what: &str| ToolError::new(CODE_INVALID_ARGUMENT, format!("Computer Use server error {ERR_UNKNOWN}: {what}"));
        let obj = action.as_object().filter(|o| o.len() == 1).ok_or_else(|| bad("action must name exactly one action"))?;
        let (kind, p) = obj.iter().next().unwrap();
        let element = |v: &Value| -> Result<u64, ToolError> {
            v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_u64()).ok_or_else(|| bad("elementID must be an integer id"))
        };
        let (tool, mut args) = match kind.as_str() {
            "click" => {
                let button = match p.get("mouseButton").and_then(Value::as_u64) { Some(1) => "r", Some(2) => "m", _ => "l" };
                let mut a = json!({"mouse_button": button, "click_count": p.get("clickCount").and_then(Value::as_u64).unwrap_or(1)});
                match Self::at_of(&p["at"]).ok_or_else(|| bad("click needs `at`"))? {
                    Ok((x, y)) => { a["x"] = json!(x); a["y"] = json!(y); }
                    Err(id) => a["element_index"] = json!(id),
                }
                ("click", a)
            }
            "drag" => {
                let pt = |k: &str| p.get(k).and_then(Value::as_array).and_then(|a| Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?)));
                let (f, t) = (pt("from").ok_or_else(|| bad("drag needs `from`"))?, pt("to").ok_or_else(|| bad("drag needs `to`"))?);
                ("drag", json!({"from_x": f.0, "from_y": f.1, "to_x": t.0, "to_y": t.1}))
            }
            "paste" => {
                if p.get("format").is_some_and(|v| v.as_str() != Some("text")) {
                    return Err(bad("paste supports only format 'text'; HTML and Markdown paste are unavailable"));
                }
                ("paste", json!({"text": p.get("text").and_then(Value::as_str).ok_or_else(|| bad("paste needs `text`"))?}))
            }
            "performSecondaryAction" => ("perform_secondary_action", json!({"action": p.get("action").and_then(Value::as_str).ok_or_else(|| bad("performSecondaryAction needs `action`"))?, "element_index": element(&p["elementID"])?})),
            "pressKey" => ("press_key", json!({"key": p.get("_0").and_then(Value::as_str).ok_or_else(|| bad("pressKey needs a key"))?})),
            "setValue" => ("set_value", json!({"element_index": element(&p["elementID"])?, "value": p.get("value").and_then(Value::as_str).ok_or_else(|| bad("setValue needs `value`"))?})),
            "selectText" => {
                if p.get("selection").is_some_and(|v| v.as_str() != Some("text")) {
                    return Err(bad("selectText supports only selection 'text'; cursor_before and cursor_after are unavailable"));
                }
                let mut a = json!({"element_index": element(&p["elementID"])?, "text": p.get("text").and_then(Value::as_str).ok_or_else(|| bad("selectText needs `text`"))?});
                for k in ["prefix", "suffix"] { if let Some(v) = p.get(k).and_then(Value::as_str) { a[k] = json!(v); } }
                ("select_text", a)
            }
            "type" => ("type_text", json!({"text": p.get("_0").and_then(Value::as_str).ok_or_else(|| bad("type needs text"))?})),
            "scroll" => {
                let (x, y) = match Self::at_of(&p["at"]).ok_or_else(|| bad("scroll needs `at`"))? {
                    Ok(xy) => xy,
                    Err(stable) => self.element_center(layer, window["id"].as_u64().unwrap_or(0), stable)
                        .ok_or_else(|| ToolError::new(CODE_STALE_ELEMENT, format!("Computer Use server error {ERR_UNKNOWN}: {stable} is an invalid element ID")))?,
                };
                let pages = p.get("pages").and_then(Value::as_f64).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(1.0);
                // INFERRED: a page is ten wheel notches (the layer turns 40 points of delta into one)
                let delta = pages * 400.0;
                let (sx, sy) = match p.get("direction").and_then(Value::as_str) { Some("up") => (0.0, -delta), Some("down") => (0.0, delta), Some("left") => (-delta, 0.0), Some("right") => (delta, 0.0), _ => return Err(bad("scroll needs a direction")) };
                ("scroll", json!({"x": x, "y": y, "scrollX": sx, "scrollY": sy}))
            }
            other => return Err(ToolError::new(CODE_INVALID_ARGUMENT, format!("Could not resolve action {other}"))),
        };
        args["window"] = window;
        layer.input_before(tool, &args, deadline)?;
        Ok(json!({"app": Self::app_json(&app)}))
    }

    /// An element's center in window-relative logical points, from the latest state's pixels.
    fn element_center(&self, layer: &Layer, window: u64, stable: u64) -> Option<(f64, f64)> {
        let t = layer.targets_for(window)?;
        let (px, py) = t.centers_px.get(&stable).copied()?;
        let scale = t.pixel_scale.filter(|s| *s > 0.0)?;
        Some((px / scale, py / scale))
    }
}

// ---- the socket -----------------------------------------------------------------------------------

/// Serve one connection until it closes: frames in, frames out. Any byte stream will do (a unix
/// socket, a Windows named pipe, a test's in-memory pair).
pub fn serve_connection<S: Read + Write>(pipe: &SkyPipe, mut stream: S) {
    let mut decoder = FrameDecoder::default();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = match stream.read(&mut buf) { Ok(0) | Err(_) => return, Ok(n) => n };
        let messages = match decoder.push(&buf[..n]) { Ok(m) => m, Err(_) => return };
        for raw in messages {
            // an unparseable or id-less message gets no answer and ends the connection, as the real
            // service closes without a framed reply to what it does not understand
            let Ok(msg) = serde_json::from_str::<Value>(&raw) else { return };
            let Some(reply) = pipe.handle(&msg) else { return };
            let Ok(frame) = encode_frame(reply.to_string().as_bytes()) else { return };
            if stream.write_all(&frame).is_err() { return; }
        }
    }
}

/// Whether the peer of this socket is the same user.
#[cfg(unix)]
fn peer_is_us(stream: &std::os::unix::net::UnixStream) -> bool {
    use std::os::fd::AsRawFd;
    #[cfg(target_os = "linux")]
    unsafe {
        let mut cred: libc::ucred = std::mem::zeroed();
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let r = libc::getsockopt(stream.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, &mut cred as *mut _ as *mut libc::c_void, &mut len);
        r == 0 && cred.uid == libc::geteuid()
    }
    #[cfg(not(target_os = "linux"))]
    unsafe {
        let (mut uid, mut gid): (libc::uid_t, libc::gid_t) = (0, 0);
        libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) == 0 && uid == libc::geteuid()
    }
}

/// Bind `path` (0600, in a 0700 directory), then serve connections, one thread each, until the
/// process ends. A stale socket file with nobody listening is replaced; a live one is an error.
#[cfg(unix)]
pub fn serve(pipe: std::sync::Arc<SkyPipe>, path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let listener = bind(path)?;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        if !peer_is_us(&stream) { continue; }
        let pipe = pipe.clone();
        std::thread::spawn(move || serve_connection(&pipe, stream));
    }
    Ok(())
}

#[cfg(unix)]
pub fn bind(path: &std::path::Path) -> std::io::Result<std::os::unix::net::UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    if path.exists() {
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return Err(std::io::Error::new(std::io::ErrorKind::AddrInUse, format!("{} is already being served", path.display())));
        }
        std::fs::remove_file(path)?;
    }
    std::os::unix::net::UnixListener::bind(path)
}
