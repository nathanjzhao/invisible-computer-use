//! The one connection to cua-driver: a persistent `cua-driver [--socket S] mcp`
//! child speaking JSON-RPC 2.0 over stdio (the driver's own MCP transport, which
//! proxies to the daemon when a socket is given). Every layer call goes through
//! [`Cua::call`]; tests substitute a fake.
use serde_json::{json, Value};
use std::process::Stdio;
use std::sync::{Mutex, TryLockError};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

/// Shared, lossless driver failure. Delivery policy stays in each caller.
pub use action_outcomes::DriverError as CuaError;

pub trait Cua: Send {
    /// Invoke a driver tool; `Ok` carries the structured content (or the parsed
    /// text content when the driver returns only text).
    fn call(&self, tool: &str, args: Value) -> Result<Value, CuaError>;
    /// The driver's `tools/list` names.
    fn tool_names(&self) -> Result<Vec<String>, CuaError>;
    /// The `inputSchema` the driver advertises for `tool`, when it can say. The default cannot,
    /// which callers must treat as "takes no optional field they need".
    fn tool_schema(&self, _tool: &str) -> Option<Value> { None }
}

/// Keep the local MCP diagnostics available after connection setup fails.
/// The failed session never reconnects or replays an action implicitly.
pub struct UnavailableCua(pub CuaError);
impl Cua for UnavailableCua {
    fn call(&self, _: &str, _: Value) -> Result<Value, CuaError> { Err(self.0.clone()) }
    fn tool_names(&self) -> Result<Vec<String>, CuaError> { Err(self.0.clone()) }
}

/// Synchronous public facade, safe to construct/call/drop inside any caller
/// runtime. Only its dedicated OS thread creates, enters, and drops Tokio.
/// Calls still block this synchronous API's caller; async hosts can choose
/// spawn_blocking for scheduling, but no nested-runtime panic is possible.
pub struct McpChild {
    sender: Option<std::sync::mpsc::SyncSender<Exchange>>,
    worker: Option<std::thread::JoinHandle<()>>,
    timeout: Duration,
    pub server_info: Value,
}
struct Exchange {
    method: String,
    params: Value,
    deadline: Instant,
    result: std::sync::mpsc::SyncSender<Result<Value, CuaError>>,
}
/// The budget for one request. Probes (`tools/list`, `health_report`) get a short one. A desktop
/// capture gets room for the driver's own deadline: on a portal desktop (GNOME, KDE) the driver waits
/// up to 15 s for the portal and gives up at 30 s with a typed `desktop_capture_unavailable`
/// (`portal_timeout`, `capture_timeout`); a layer budget at or under that would cut it off first and
/// report an unknown outcome instead of the refusal. A capture only observes, so waiting is safe.
pub const DESKTOP_CAPTURE_BUDGET: Duration = Duration::from_secs(35);
pub fn request_budget(method: &str, params: &Value, timeout: Duration) -> Duration {
    let tool = params.get("name").and_then(Value::as_str);
    if method == "tools/list" || tool == Some("health_report") { return timeout.min(Duration::from_millis(1500)); }
    if tool == Some("get_desktop_state") { return timeout.max(DESKTOP_CAPTURE_BUDGET); }
    timeout
}
fn configured_timeout() -> Duration {
    Duration::from_millis(std::env::var("ICU_DRIVER_TIMEOUT_MS").ok()
        .and_then(|s| s.parse::<u64>().ok()).filter(|v| *v > 0 && *v <= 300_000).unwrap_or(15_000))
}
impl McpChild {
    pub fn spawn(driver: &str, socket: Option<&str>) -> Result<Self, CuaError> {
        let timeout = configured_timeout();
        let deadline = Instant::now() + timeout.min(Duration::from_millis(1500));
        let (sender, requests) = std::sync::mpsc::sync_channel::<Exchange>(1);
        let (ready, initialized) = std::sync::mpsc::sync_channel(1);
        let driver = driver.to_owned(); let socket = socket.map(str::to_owned);
        let worker = std::thread::Builder::new().name("icu-driver-transport".into()).spawn(move || {
            let child = match WorkerChild::spawn_until(&driver, socket.as_deref(), deadline) {
                Ok(child) => child,
                Err(e) => { let _ = ready.send(Err(e)); return; }
            };
            if ready.send(Ok(child.server_info.clone())).is_err() { return; }
            while let Ok(exchange) = requests.recv() {
                // An expired queued request is never sent, and never retried.
                let r = child.exchange(&exchange.method, exchange.params, true, exchange.deadline);
                let _ = exchange.result.send(r);
            }
            // Child kill/reap, pipes and Runtime::drop all happen on this thread.
        }).map_err(|e| transport(format!("driver worker: {e}")))?;
        let server_info = initialized.recv_timeout(deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(500));
        match server_info {
            Ok(Ok(server_info)) => Ok(Self { sender: Some(sender), worker: Some(worker), timeout, server_info }),
            other => {
                drop(sender);
                if worker.is_finished() { let _ = worker.join(); }
                Err(match other { Ok(Err(e)) => e, _ => transport("driver initialization deadline exceeded or worker exited") })
            }
        }
    }
    fn request(&self, method: &str, params: Value) -> Result<Value, CuaError> {
        let budget = request_budget(method, &params, self.timeout);
        let deadline = Instant::now() + budget;
        let (result, response) = std::sync::mpsc::sync_channel(1);
        let mut exchange = Exchange { method: method.into(), params, deadline, result };
        loop {
            match self.sender.as_ref().unwrap().try_send(exchange) {
                Ok(()) => break,
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => return Err(transport("driver worker exited")),
                Err(std::sync::mpsc::TrySendError::Full(pending)) => {
                    if Instant::now() >= deadline { return Err(transport("driver queue deadline exceeded; request not dispatched")); }
                    exchange = pending;
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
        response.recv_timeout(deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(500))
            .map_err(|_| transport("driver response deadline exceeded or worker exited; action completion unknown"))?
    }
}
impl Drop for McpChild {
    fn drop(&mut self) {
        self.sender.take(); // closes the request channel before joining
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}

/// A persistent transport. Each exchange owns both pipes until its response arrives.
/// The same monotonic deadline covers lock acquisition, writes, flushes, partial
/// frames, and every skipped notification/garbage line. A failed transport is
/// terminal: never retry an input action whose completion is now unknown.
struct WorkerChild {
    runtime: tokio::runtime::Runtime,
    connection: Mutex<Connection>,
    pub server_info: Value,
}

struct Connection {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    failed: Option<CuaError>,
}

/// Make sure a driver daemon answers on `socket`, starting `cua-driver serve` there if
/// nothing does, and return the socket path for `--socket`.
///
/// Without a socket, macOS `cua-driver mcp` proxies to a daemon inside `CuaDriver.app`
/// and tries to launch that app when none is listening, which `impl/install.sh` never
/// installs ("cua-driver mcp exited before replying" on a clean Mac). `mcp --direct`
/// avoids the app but runs AppKit without a main run loop, so NSWorkspace state such
/// as the frontmost app stays frozen at its first value (measured on macOS 15.7), which
/// focus suppression and ambient capture both read. The installed binary's own
/// `serve` runs that loop. It starts without the cursor overlay, whose full-screen
/// window would otherwise become the front process, and outlives this icu process
/// the way the app daemon did. As a standalone daemon it holds its own Accessibility
/// and Screen Recording grants.
#[cfg(unix)]
pub fn ensure_daemon(driver: &str, socket: &std::path::Path) -> Result<String, CuaError> {
    let path = socket.to_string_lossy().into_owned();
    let listening = || std::os::unix::net::UnixStream::connect(socket).is_ok();
    let lock_path = format!("{path}.start.lock");
    if listening() {
        // A starter that was killed, or gave up, leaves its record behind. With the start
        // lock free no start is under way, so the record is stale: clear it (round-5 F2).
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(&lock_path) {
            if f.try_lock().is_ok() { let _ = f.set_len(0); }
        }
        return Ok(path);
    }
    if let Some(dir) = socket.parent() {
        std::fs::create_dir_all(dir).map_err(|e| transport(format!("daemon socket directory: {e}")))?;
    }
    // Two icu processes starting together would each start a `serve`, and the later one
    // unlinks the earlier one's socket and binds its own, leaving a daemon running on an
    // unlinked path. The start is serialized on a sibling lock file (released on return,
    // or by the kernel if this process dies); the loser waits, then finds the winner listening.
    let mut lock = std::fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(&lock_path)
        .map_err(|e| transport(format!("daemon start lock {lock_path}: {e}")))?;
    lock.lock().map_err(|e| transport(format!("daemon start lock {lock_path}: {e}")))?;
    // The lock alone does not tell a later holder that the winner's `serve` is still
    // loading: the winner may have been killed, or given up at its deadline, with its
    // daemon alive (round-4 review R4-X3). So the start lock file records the pid of the
    // `serve` it started until that daemon listens, and a patched `serve` holds
    // `<socket>.lock` from before its load. While either says a daemon is on its way,
    // this waits for it rather than start a second one. The record is the pid and the
    // process's start time, so a pid reused by another process (or pid 1) is not taken
    // for that `serve` (round-5 F2).
    let record = |lock: &mut std::fs::File, pid: Option<u32>| {
        use std::io::{Seek, Write};
        let _ = lock.set_len(0);
        let _ = lock.rewind();
        if let Some(pid) = pid { let _ = write!(lock, "{}", start_record(pid)); }
    };
    if listening() { record(&mut lock, None); return Ok(path); }
    let recorded = { use std::io::Read; let mut s = String::new(); let _ = (&lock).read_to_string(&mut s); parse_start_record(&s) };
    let held = path.clone();
    let waits: [(Option<String>, Box<dyn Fn() -> bool>); 2] = [
        (recorded.map(|(pid, _)| format!("`serve` pid {pid}, started by an earlier icu,")),
         Box::new(move || recorded.is_some_and(|(pid, start)| process_alive(pid) && process_start(pid) == start
             && (start.is_some() || !cfg!(any(target_os = "macos", target_os = "linux")))))),
        (Some(format!("the `serve` holding {path}.lock")), Box::new(move || serve_lock_held(&held))),
    ];
    for (what, alive) in waits {
        let Some(what) = what else { continue };
        if !alive() { continue; }
        let deadline = Instant::now() + DAEMON_START;
        while alive() && Instant::now() < deadline {
            if listening() { record(&mut lock, None); return Ok(path); }
            std::thread::sleep(Duration::from_millis(100));
        }
        if listening() { record(&mut lock, None); return Ok(path); }
        if alive() {
            return Err(transport(format!("{what} has not listened on {path} within {}s; not starting a second daemon", DAEMON_START.as_secs())));
        }
    }
    let mut cmd = std::process::Command::new(driver);
    cmd.args(daemon_args(&path)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd.spawn().map_err(|e| transport(format!("failed to start `{driver} serve`: {e}")))?;
    record(&mut lock, Some(child.id()));
    let deadline = Instant::now() + DAEMON_START;
    let mut exited = None;
    let result = 'wait: {
        while Instant::now() < deadline {
            if listening() { record(&mut lock, None); break 'wait Ok(path); }
            // A driver that exits without listening will never listen: say so now, not in 15 s.
            // (A daemon started outside icu may have won the socket, which is why this re-checks,
            // and a patched `serve` that found `<socket>.lock` held exits as already running
            // while the holder loads, which is why this keeps waiting on that holder.)
            if exited.is_none() { exited = child.try_wait().ok().flatten(); }
            if let Some(status) = exited {
                if listening() { record(&mut lock, None); break 'wait Ok(path); }
                if !serve_lock_held(&path) {
                    record(&mut lock, None);
                    break 'wait Err(transport(format!("`{driver} serve` exited ({status}) before listening on {path}; \
                        for a driver without a daemon mode set ICU_DRIVER_DAEMON=0")));
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err(transport(format!("`{driver} serve` did not listen on {path} within {}s", DAEMON_START.as_secs())))
    };
    // The daemon is this process's child for as long as it lives. Unreaped, it would stay a
    // zombie after it dies, and a zombie still answers kill(pid, 0), so every later start in
    // a long-lived icu (an MCP server) would wait on it and refuse (round-5 F3).
    if exited.is_none() { std::thread::spawn(move || { let _ = child.wait(); }); }
    result
}

/// `<pid> <start time>` for the start record, or the pid alone where the start time is unknown.
#[cfg(unix)]
fn start_record(pid: u32) -> String {
    match process_start(pid) { Some(t) => format!("{pid} {t}"), None => pid.to_string() }
}

#[cfg(unix)]
fn parse_start_record(s: &str) -> Option<(u32, Option<u64>)> {
    let mut words = s.split_whitespace();
    let pid = words.next()?.parse().ok()?;
    Some((pid, words.next().and_then(|t| t.parse().ok())))
}

/// When the process with this pid started, in a unit fixed per platform, or `None` for a
/// process that is gone, a zombie, or unreadable. Together with the pid it names one process.
#[cfg(target_os = "macos")]
fn process_start(pid: u32) -> Option<u64> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let n = unsafe { libc::proc_pidinfo(libc::pid_t::try_from(pid).ok()?, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) };
    (n == size && info.pbi_status != libc::SZOMB).then(|| info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
}

#[cfg(target_os = "linux")]
fn process_start(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // Fields after the parenthesized command: state is the first, starttime the 20th.
    let rest: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    (rest.first() != Some(&"Z")).then(|| rest.get(19)?.parse().ok()).flatten()
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn process_start(_pid: u32) -> Option<u64> { None }

/// How long one daemon start may take to listen (shorter under test, so giving up is testable).
#[cfg(unix)]
const DAEMON_START: Duration = Duration::from_secs(if cfg!(test) { 4 } else { 15 });

/// Whether a process with this pid exists (a zombie counts; `EPERM` means it exists).
#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else { return false };
    pid > 0 && (unsafe { libc::kill(pid, 0) } == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM))
}

/// Whether another process holds `<socket>.lock`, which a patched `serve` takes before
/// its load and keeps for its life. Probing takes the lock for an instant when it is
/// free, so a `serve` started outside icu in that instant is refused as already running.
#[cfg(unix)]
fn serve_lock_held(socket: &str) -> bool {
    let Ok(file) = std::fs::OpenOptions::new().write(true).open(format!("{socket}.lock")) else { return false };
    matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock))
}

#[cfg(unix)]
fn daemon_args(socket: &str) -> [&str; 4] {
    ["serve", "--no-overlay", "--socket", socket]
}

fn transport(message: impl Into<String>) -> CuaError {
    CuaError { message: message.into(), structured: None }
}

impl WorkerChild {
    fn spawn_until(driver: &str, socket: Option<&str>, deadline: Instant) -> Result<WorkerChild, CuaError> {
        if Instant::now() >= deadline { return Err(transport("driver initialization deadline exceeded before spawn")); }
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()
            .map_err(|e| transport(format!("driver runtime: {e}")))?;
        let mut cmd = Command::new(driver);
        if let Some(s) = socket { cmd.args(["--socket", s]); }
        let mut child = {
            let _entered = runtime.enter();
            cmd.arg("mcp").stdin(Stdio::piped()).stdout(Stdio::piped())
                .stderr(Stdio::inherit()).kill_on_drop(true).spawn()
                .map_err(|e| transport(format!("failed to start `{driver} mcp`: {e}")))?
        };
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let connection = Connection { child, stdin, stdout, next_id: 1, failed: None };
        let mut me = WorkerChild { runtime, connection: Mutex::new(connection), server_info: Value::Null };
        let init = me.exchange("initialize", json!({"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "icu", "version": env!("CARGO_PKG_VERSION")}}), true, deadline)?;
        me.exchange("notifications/initialized", json!({}), false, deadline)?;
        me.server_info = init.get("serverInfo").cloned().unwrap_or(Value::Null);
        Ok(me)
    }

    fn exchange(&self, method: &str, params: Value, reply: bool, deadline: Instant) -> Result<Value, CuaError> {
        let expired = || transport(format!("driver wall-clock deadline exceeded during {method}; action completion unknown"));
        let mut c = loop {
            match self.connection.try_lock() {
                Ok(c) => break c,
                Err(TryLockError::Poisoned(_)) => return Err(transport("driver connection poisoned")),
                Err(TryLockError::WouldBlock) => {
                    if Instant::now() >= deadline { return Err(expired()); }
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        };
        if let Some(e) = &c.failed { return Err(e.clone()); }
        if Instant::now() >= deadline { return Err(expired()); }
        let result = self.runtime.block_on(async {
            tokio::time::timeout_at(deadline.into(), async {
                let id = c.next_id;
                c.next_id += 1;
                let mut msg = json!({"jsonrpc": "2.0", "method": method, "params": params});
                if reply { msg["id"] = json!(id); }
                let mut bytes = serde_json::to_vec(&msg).map_err(|e| transport(e.to_string()))?;
                bytes.push(b'\n');
                if Instant::now() >= deadline { return Err(expired()); }
                c.stdin.write_all(&bytes).await.map_err(|e| transport(format!("driver stdin: {e}")))?;
                c.stdin.flush().await.map_err(|e| transport(format!("driver flush: {e}")))?;
                if !reply { return Ok(Value::Null); }
                let mut frame = Vec::new();
                loop {
                    // Explicit check: an always-ready noisy stream must not starve the timer.
                    if Instant::now() >= deadline { return Err(expired()); }
                    let chunk = c.stdout.fill_buf().await.map_err(|e| transport(format!("driver stdout: {e}")))?;
                    if chunk.is_empty() { return Err(transport("cua-driver mcp exited before replying")); }
                    let newline = chunk.iter().position(|b| *b == b'\n');
                    let take = newline.map(|n| n + 1).unwrap_or(chunk.len());
                    if frame.len() + take > 32 * 1024 * 1024 { return Err(transport("driver frame exceeds 32 MiB")); }
                    frame.extend_from_slice(&chunk[..take]);
                    c.stdout.consume(take);
                    if newline.is_none() { continue; }
                    let parsed = serde_json::from_slice::<Value>(&frame);
                    frame.clear();
                    let Ok(msg) = parsed else { continue };
                    if Instant::now() >= deadline { return Err(expired()); }
                    if msg.get("id").and_then(Value::as_u64) != Some(id) { continue; }
                    if let Some(err) = msg.get("error") {
                        return Err(CuaError { message: err.get("message").and_then(Value::as_str).unwrap_or("rpc error").to_string(), structured: Some(err.clone()) });
                    }
                    return msg.get("result").cloned().ok_or_else(|| transport("driver response has no result or error"));
                }
            }).await.unwrap_or_else(|_| Err(expired()))
        });
        if let Err(e) = &result {
            if e.structured.is_none() {
                c.failed = Some(e.clone());
                // Close this transport immediately, including on initialization failure.
                // Tokio's child reaper remains a fallback if wait itself fails.
                let _ = c.child.start_kill();
                self.runtime.block_on(async { let _ = tokio::time::timeout(Duration::from_millis(250), c.child.wait()).await; });
            }
        }
        result
    }
}

impl Drop for WorkerChild {
    fn drop(&mut self) {
        if let Ok(c) = self.connection.get_mut() {
            let _ = c.child.start_kill();
            self.runtime.block_on(async { let _ = tokio::time::timeout(Duration::from_millis(250), c.child.wait()).await; });
        }
    }
}

/// Normalize an MCP `tools/call` result: `structuredContent` wins, else the text
/// content parsed as JSON, else `{"text": ...}`; `isError` becomes `Err`.
///
/// A payload with no screenshot of its own (neither `screenshots` nor
/// `screenshot_png_b64`) gets the first image content part as
/// `screenshot_png_b64` / `screenshot_mime_type`, the merge `cua-driver call` does. The Linux
/// and Windows drivers send a window capture only as an image part (final-linux F2).
pub fn normalize_result(result: &Value) -> Result<Value, CuaError> {
    if let Some(error) = CuaError::from_tool_result(result) { return Err(error); }
    let mut payload = action_outcomes::tool_payload(result);
    let image = result.get("content").and_then(Value::as_array)
        .and_then(|c| c.iter().find(|i| i.get("type").and_then(Value::as_str) == Some("image")));
    if let (Some(o), Some(image)) = (payload.as_object_mut(), image) {
        let has_own = o.get("screenshots").and_then(Value::as_array).is_some_and(|a| !a.is_empty())
            || o.get("screenshot_png_b64").and_then(Value::as_str).is_some_and(|b| !b.is_empty());
        if let (false, Some(data)) = (has_own, image.get("data").and_then(Value::as_str)) {
            o.insert("screenshot_png_b64".into(), json!(data));
            o.insert("screenshot_mime_type".into(), image.get("mimeType").cloned().unwrap_or_else(|| json!("image/png")));
        }
    }
    Ok(payload)
}

impl Cua for McpChild {
    fn call(&self, tool: &str, args: Value) -> Result<Value, CuaError> {
        let result = self.request("tools/call", json!({"name": tool, "arguments": args}))?;
        normalize_result(&result)
    }
    fn tool_names(&self) -> Result<Vec<String>, CuaError> {
        let r = self.request("tools/list", json!({}))?;
        let tools = r.get("tools").and_then(Value::as_array).ok_or_else(|| transport("malformed tools/list result"))?;
        tools.iter().map(|t| t.get("name").and_then(Value::as_str).map(str::to_string)
            .ok_or_else(|| transport("malformed tools/list entry"))).collect()
    }
    fn tool_schema(&self, tool: &str) -> Option<Value> {
        let r = self.request("tools/list", json!({})).ok()?;
        r.get("tools")?.as_array()?.iter().find(|t| t.get("name").and_then(Value::as_str) == Some(tool))?.get("inputSchema").cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_desktop_capture_is_given_longer_than_the_drivers_own_portal_deadline() {
        let t = Duration::from_secs(15);
        let capture = json!({"name": "get_desktop_state", "arguments": {}});
        assert!(request_budget("tools/call", &capture, t) > Duration::from_secs(30), "the driver answers a stuck portal at 30 s");
        assert_eq!(request_budget("tools/call", &capture, Duration::from_secs(120)), Duration::from_secs(120), "a longer configured budget wins");
        assert_eq!(request_budget("tools/call", &json!({"name": "click"}), t), t, "other calls keep the configured budget");
        assert_eq!(request_budget("tools/list", &json!({}), t), Duration::from_millis(1500));
        assert_eq!(request_budget("tools/call", &json!({"name": "health_report"}), t), Duration::from_millis(1500));
    }

    #[test]
    fn normalize_prefers_structured_then_text_json_and_maps_is_error() {
        let ok = json!({"content": [{"type": "text", "text": "{\"a\":1}"}], "structuredContent": {"b": 2}});
        assert_eq!(normalize_result(&ok).unwrap(), json!({"b": 2}));
        let text_only = json!({"content": [{"type": "text", "text": "{\"a\":1}"}]});
        assert_eq!(normalize_result(&text_only).unwrap(), json!({"a": 1}));
        let plain = json!({"content": [{"type": "text", "text": "hello"}]});
        assert_eq!(normalize_result(&plain).unwrap(), json!({"text": "hello"}));
        let err = json!({"content": [{"type": "text", "text": "click refused"}], "isError": true, "structuredContent": {"code": "host_not_interactive"}});
        let e = normalize_result(&err).unwrap_err();
        assert_eq!((e.message.as_str(), e.code()), ("click refused", Some("host_not_interactive")));
    }

    /// final-linux F2: the Linux (and Windows) driver sends a capture only as an image part.
    /// It is merged the way `cua-driver call` merges it; a payload that has its own screenshot
    /// keeps it.
    #[test]
    fn normalize_merges_an_image_part_into_a_payload_without_a_screenshot() {
        let linux = json!({"content": [{"type": "text", "text": "tree"}, {"type": "image", "data": "iVBORw0K", "mimeType": "image/png"}], "structuredContent": {"screenshot_id": "f1-2", "screenshot_width": 400}});
        let got = normalize_result(&linux).unwrap();
        assert_eq!((got["screenshot_png_b64"].as_str(), got["screenshot_mime_type"].as_str(), got["screenshot_id"].as_str()), (Some("iVBORw0K"), Some("image/png"), Some("f1-2")));
        let mac = json!({"content": [{"type": "image", "data": "PART", "mimeType": "image/png"}], "structuredContent": {"screenshots": [{"png_b64": "OWN"}]}});
        assert!(normalize_result(&mac).unwrap().get("screenshot_png_b64").is_none());
        let own = json!({"content": [{"type": "image", "data": "PART"}], "structuredContent": {"screenshot_png_b64": "OWN"}});
        assert_eq!(normalize_result(&own).unwrap()["screenshot_png_b64"], "OWN");
        let zoom = json!({"content": [{"type": "image", "data": "JPG", "mimeType": "image/jpeg"}], "structuredContent": {"width": 1}});
        assert_eq!(normalize_result(&zoom).unwrap()["screenshot_mime_type"], "image/jpeg");
    }

    #[cfg(unix)]
    #[test]
    fn managed_daemon_runs_the_standalone_serve_loop_without_overlay() {
        assert_eq!(daemon_args("/tmp/d.sock"), ["serve", "--no-overlay", "--socket", "/tmp/d.sock"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_driver_that_exits_without_listening_fails_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let started = Instant::now();
        let e = ensure_daemon("/usr/bin/false", &dir.path().join("d.sock")).unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(3), "waited {:?}: {}", started.elapsed(), e.message);
        assert!(e.message.contains("exited") && e.message.contains("ICU_DRIVER_DAEMON=0"), "{}", e.message);
    }

    /// Two icu processes starting the daemon at once must leave one daemon, not one on the
    /// socket and an orphan on an unlinked path (round-2 review A3). The stand-in follows
    /// the vendor serve's standalone order: refuse if the socket answers, load, unlink, bind.
    #[cfg(unix)]
    #[test]
    fn simultaneous_starts_leave_one_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let driver = dir.path().join("serve.py");
        let log = dir.path().join("bound.log");
        install_executable(&driver, &format!(r#"#!/usr/bin/env python3
import os, socket, sys, time
path = sys.argv[sys.argv.index("--socket") + 1]
try:
    socket.socket(socket.AF_UNIX).connect(path); sys.exit(1)
except OSError:
    pass
time.sleep(0.5)
try: os.unlink(path)
except FileNotFoundError: pass
srv = socket.socket(socket.AF_UNIX); srv.bind(path); srv.listen(8)
open({log:?}, "a").write(f"{{os.getpid()}}\n")
while True: srv.accept()[0].close()
"#));
        let sock = dir.path().join("d.sock");
        let d = driver.to_str().unwrap().to_owned();
        let starts: Vec<_> = (0..2).map(|_| {
            let (d, sock) = (d.clone(), sock.clone());
            std::thread::spawn(move || ensure_daemon(&d, &sock))
        }).collect();
        let results: Vec<_> = starts.into_iter().map(|t| t.join().unwrap()).collect();
        std::thread::sleep(Duration::from_millis(1500));
        let pids: Vec<String> = std::fs::read_to_string(&log).unwrap_or_default().lines().map(str::to_owned).collect();
        for pid in &pids { let _ = std::process::Command::new("kill").arg(pid).status(); }
        assert!(results.iter().all(Result::is_ok), "{results:?}");
        assert_eq!(pids.len(), 1, "daemons bound: {pids:?}");
    }

    /// Linux refuses to exec a file that any process still holds open for writing (ETXTBSY).
    /// Every test thread forks, and a child forked between this thread's `open` and `close`
    /// of a script it is writing keeps the write handle until its own exec, so a script
    /// written and run at once fails now and then, in whichever test is unlucky. Spawn the
    /// file until the kernel stops refusing it: once one exec works, no forked child still
    /// holds the handle (a fork after the close inherits nothing).
    #[cfg(unix)]
    fn wait_until_executable(path: &std::path::Path) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            // No arguments: the stand-ins read `--socket` first and exit at once.
            let null = std::process::Stdio::null;
            match std::process::Command::new(path).stdin(null()).stdout(null()).stderr(null()).spawn() {
                Ok(mut child) => { let _ = child.wait(); return; }
                Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                Err(e) => panic!("{}: {e}", path.display()),
            }
        }
    }

    #[cfg(unix)]
    fn install_executable(path: &std::path::Path, content: &str) {
        std::fs::write(path, content).unwrap();
        std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        wait_until_executable(path);
    }

    /// The Linux flake behind `Text file busy` in the stand-in tests: a handle still open for
    /// writing (what a concurrently forked child holds) makes a plain exec fail, and the wait
    /// returns only once it is closed.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_script_still_held_open_for_writing_is_waited_for_not_failed() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("s.sh");
        std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let held = std::fs::OpenOptions::new().write(true).open(&script).unwrap();
        let busy = std::process::Command::new(&script).spawn().unwrap_err();
        assert_eq!(busy.raw_os_error(), Some(libc::ETXTBSY), "premise: a held write handle blocks exec: {busy}");
        let closer = std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(300)); drop(held); });
        let started = Instant::now();
        wait_until_executable(&script);
        assert!(started.elapsed() >= Duration::from_millis(250), "returned while the handle was open: {:?}", started.elapsed());
        std::process::Command::new(&script).status().expect("runs once the wait returns");
        closer.join().unwrap();
    }

    /// A stand-in `serve` in the vendor's standalone order. With `lock`, it holds
    /// `<socket>.lock` from before its load and exits if it is held, as the patched serve does.
    #[cfg(unix)]
    fn stand_in(dir: &std::path::Path, load_s: f64, lock: bool) -> (String, std::path::PathBuf) {
        let driver = dir.join(if lock { "serve-lock.py" } else { "serve.py" });
        let log = dir.join("bound.log");
        install_executable(&driver, &format!(r#"#!/usr/bin/env python3
import fcntl, os, socket, sys, time
path = sys.argv[sys.argv.index("--socket") + 1]
if {lock}:
    held = open(path + ".lock", "a")
    try: fcntl.flock(held, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError: sys.exit(1)
try:
    socket.socket(socket.AF_UNIX).connect(path); sys.exit(1)
except OSError:
    pass
time.sleep({load_s})
try: os.unlink(path)
except FileNotFoundError: pass
srv = socket.socket(socket.AF_UNIX); srv.bind(path); srv.listen(8)
open({log:?}, "a").write(f"{{os.getpid()}}\n")
while True: srv.accept()[0].close()
"#, lock = if lock { "True" } else { "False" }));
        (driver.to_str().unwrap().to_owned(), log)
    }

    #[cfg(unix)]
    fn bound_and_killed(log: &std::path::Path) -> Vec<String> {
        std::thread::sleep(Duration::from_millis(1500));
        let pids: Vec<String> = std::fs::read_to_string(log).unwrap_or_default().lines().map(str::to_owned).collect();
        for pid in &pids { let _ = std::process::Command::new("kill").arg(pid).status(); }
        pids
    }

    /// Round-4 review R4-X3: the icu that won the start lock was killed (or gave up at its
    /// deadline) while its `serve` loaded; the next icu found nothing listening and started
    /// a second daemon. The winner's record of its serve's pid is what the next one reads.
    #[cfg(unix)]
    #[test]
    fn a_start_whose_icu_died_is_waited_for_not_repeated() {
        let dir = tempfile::tempdir().unwrap();
        let (driver, log) = stand_in(dir.path(), 1.5, false);
        let sock = dir.path().join("d.sock");
        let path = sock.to_str().unwrap();
        let orphan = std::process::Command::new(&driver).args(daemon_args(path)).spawn().unwrap();
        std::fs::write(format!("{path}.start.lock"), start_record(orphan.id())).unwrap();
        let started = Instant::now();
        let r = ensure_daemon(&driver, &sock);
        let pids = bound_and_killed(&log);
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(pids, vec![orphan.id().to_string()], "daemons bound: {pids:?}");
        assert!(started.elapsed() >= Duration::from_millis(1000), "returned before the orphan listened: {:?}", started.elapsed());
        assert_eq!(std::fs::read_to_string(format!("{path}.start.lock")).unwrap(), "", "the record is cleared once it listens");
    }

    /// A patched `serve` started outside icu holds `<socket>.lock` while it loads; icu waits
    /// for it instead of starting one that would be refused (or, unpatched, bind over it).
    #[cfg(unix)]
    #[test]
    fn a_serve_holding_the_socket_lock_is_waited_for() {
        let dir = tempfile::tempdir().unwrap();
        let (driver, log) = stand_in(dir.path(), 1.5, true);
        let sock = dir.path().join("d.sock");
        let path = sock.to_str().unwrap();
        let outside = std::process::Command::new(&driver).args(daemon_args(path)).spawn().unwrap();
        while !std::path::Path::new(&format!("{path}.lock")).exists() { std::thread::sleep(Duration::from_millis(10)); }
        std::thread::sleep(Duration::from_millis(200));
        let r = ensure_daemon(&driver, &sock);
        let pids = bound_and_killed(&log);
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(pids, vec![outside.id().to_string()], "daemons bound: {pids:?}");
    }

    /// Round-5 F2: the record names a live process, but not the `serve` it was written
    /// for (a reused pid, or pid 1, which answers kill with EPERM). A start goes ahead.
    #[cfg(unix)]
    #[test]
    fn a_record_naming_another_process_is_not_waited_for() {
        let mut other = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let (pid, start) = parse_start_record(&start_record(other.id())).unwrap();
        assert!(start.is_some(), "no start time read for a live child");
        for rec in [format!("{pid} {}", start.unwrap() + 1), "1".to_owned(), format!("1 {}", start.unwrap())] {
            let dir = tempfile::tempdir().unwrap();
            let (driver, log) = stand_in(dir.path(), 0.3, true);
            let sock = dir.path().join("d.sock");
            std::fs::write(format!("{}.start.lock", sock.display()), &rec).unwrap();
            let started = Instant::now();
            let r = ensure_daemon(&driver, &sock);
            let pids = bound_and_killed(&log);
            assert!(r.is_ok() && started.elapsed() < DAEMON_START, "record {rec:?}: {r:?} after {:?}", started.elapsed());
            assert_eq!(pids.len(), 1, "record {rec:?}: daemons bound {pids:?}");
        }
        let _ = other.kill(); let _ = other.wait();
    }

    /// Round-5 F2: the starter was killed mid-load, so nobody cleared its record. A later
    /// start that finds the daemon listening and the start lock free clears it.
    #[cfg(unix)]
    #[test]
    fn a_stale_record_is_cleared_once_the_daemon_listens() {
        let dir = tempfile::tempdir().unwrap();
        let (driver, log) = stand_in(dir.path(), 0.3, true);
        let sock = dir.path().join("d.sock");
        let path = sock.to_str().unwrap();
        let daemon = std::process::Command::new(&driver).args(daemon_args(path)).spawn().unwrap();
        std::fs::write(format!("{path}.start.lock"), start_record(daemon.id())).unwrap();
        while std::os::unix::net::UnixStream::connect(&sock).is_err() { std::thread::sleep(Duration::from_millis(20)); }
        let r = ensure_daemon(&driver, &sock);
        let record = std::fs::read_to_string(format!("{path}.start.lock")).unwrap();
        bound_and_killed(&log);
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(record, "", "the stale record stays");
    }

    /// Round-5 F3: icu gave up on a slow `serve` and kept running (an MCP server). The daemon
    /// listened later, then died. It must be reaped, not left a zombie that kill(pid, 0) counts
    /// as alive, and the next start must go ahead at once.
    #[cfg(unix)]
    #[test]
    fn a_daemon_given_up_on_is_reaped_when_it_dies() {
        let dir = tempfile::tempdir().unwrap();
        let (driver, log) = stand_in(dir.path(), DAEMON_START.as_secs_f64() + 1.0, true);
        let sock = dir.path().join("d.sock");
        let e = ensure_daemon(&driver, &sock).unwrap_err();
        assert!(e.message.contains("did not listen"), "{}", e.message);
        while std::os::unix::net::UnixStream::connect(&sock).is_err() { std::thread::sleep(Duration::from_millis(50)); }
        let pids = bound_and_killed(&log);
        let pid: u32 = pids[0].parse().unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while process_alive(pid) && Instant::now() < deadline { std::thread::sleep(Duration::from_millis(50)); }
        assert!(!process_alive(pid), "the dead daemon {pid} was not reaped");
        let started = Instant::now();
        let (driver2, log2) = stand_in(dir.path(), 0.3, false);
        let r = ensure_daemon(&driver2, &sock);
        let pids2 = bound_and_killed(&log2);
        assert!(r.is_ok() && started.elapsed() < DAEMON_START, "{r:?} after {:?}", started.elapsed());
        assert_eq!(pids2.len(), 2, "daemons bound: {pids2:?}");
    }

    /// Round-6 B4: the Linux start time is field 22 of /proc/<pid>/stat, found after the
    /// last `)`, so a command name holding `) S 1 ` and spaces does not shift it.
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_start_time_survives_a_forged_command_name() {
        // The kernel names a process after the basename it was executed by, so a symlink
        // to sleep with a hostile name gives that command name.
        let dir = tempfile::tempdir().unwrap();
        let sleep = ["/bin/sleep", "/usr/bin/sleep"].into_iter().find(|p| std::path::Path::new(p).exists()).unwrap();
        let named = dir.path().join("a) S 1 2 3 ) Z");
        std::os::unix::fs::symlink(sleep, &named).unwrap();
        let mut child = std::process::Command::new(&named).arg("30").spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let stat = loop {
            let stat = std::fs::read_to_string(format!("/proc/{}/stat", child.id())).unwrap();
            if stat.contains(" (a) S 1 2 3 ) Z) ") { break stat; }
            if Instant::now() >= deadline {
                let _ = child.kill(); let _ = child.wait();
                panic!("child did not finish exec: {stat}");
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        // The name adds 6 words; counted from the line start, field 22 would be priority.
        let naive: u64 = stat.split_whitespace().nth(21).unwrap().parse().unwrap();
        let start: u64 = stat.split_whitespace().nth(27).unwrap().parse().unwrap();
        assert_ne!(naive, start);
        assert_eq!(process_start(child.id()), Some(start), "{stat}");
        let _ = child.kill(); let _ = child.wait();
        assert_eq!(process_start(child.id()), None);
    }

    /// Round-6 B4: an exited child that is not yet reaped is alive to kill(pid, 0) but has
    /// no start time, so a record naming it is not waited for.
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_zombie_has_no_start_time() {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let path = format!("/proc/{}/stat", child.id());
        let deadline = Instant::now() + Duration::from_secs(5);
        while !std::fs::read_to_string(&path).unwrap().rsplit_once(')').unwrap().1.trim_start().starts_with('Z') {
            assert!(Instant::now() < deadline, "never became a zombie");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(process_alive(child.id()), "a zombie answers kill(pid, 0)");
        assert_eq!(process_start(child.id()), None);
        child.wait().unwrap();
        assert!(!process_alive(child.id()));
    }

    /// Round-6 B4: processes exiting while their start time is read give either their one
    /// start time or `None`, never another value and never a panic.
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_start_time_read_races_an_exit() {
        let mut nones = 0;
        for _ in 0..300 {
            let mut child = std::process::Command::new("true").spawn().unwrap();
            let pid = child.id();
            let reader = std::thread::spawn(move || {
                let mut seen = std::collections::BTreeSet::new();
                while let Some(t) = process_start(pid) { seen.insert(t); if seen.len() > 1 { break; } }
                seen
            });
            child.wait().unwrap();
            let seen = reader.join().unwrap();
            assert!(seen.len() <= 1, "pid {pid} read two start times {seen:?}");
            nones += usize::from(seen.is_empty());
        }
        eprintln!("B4 race: 300 exits, {nones} read None first");
    }

    /// Round-6 B4: a pid reused by a later process (forced with ns_last_pid, which needs
    /// root) reads a different start time, so the earlier `serve`'s record is not waited for.
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_reused_pid_is_not_the_recorded_serve() {
        const LAST: &str = "/proc/sys/kernel/ns_last_pid";
        if unsafe { libc::geteuid() } != 0 || std::fs::OpenOptions::new().write(true).open(LAST).is_err() {
            eprintln!("B4 reuse: SKIPPED, {LAST} not writable (needs root)");
            return;
        }
        for attempt in 0..20 {
            let mut first = std::process::Command::new("sleep").arg("30").spawn().unwrap();
            let pid = first.id();
            let record = start_record(pid);
            let _ = first.kill(); first.wait().unwrap();
            std::thread::sleep(Duration::from_millis(30));
            std::fs::write(LAST, (pid - 1).to_string()).unwrap();
            let mut second = std::process::Command::new("sleep").arg("30").spawn().unwrap();
            if second.id() != pid { let _ = second.kill(); let _ = second.wait(); continue; }
            let (_, start) = parse_start_record(&record).unwrap();
            let now = process_start(pid);
            eprintln!("B4 reuse: attempt {attempt} pid {pid} recorded {record:?} now {now:?}");
            assert!(process_alive(pid) && start.is_some() && now.is_some() && now != start);
            let dir = tempfile::tempdir().unwrap();
            let (driver, log) = stand_in(dir.path(), 0.3, true);
            let sock = dir.path().join("d.sock");
            std::fs::write(format!("{}.start.lock", sock.display()), &record).unwrap();
            let started = Instant::now();
            let r = ensure_daemon(&driver, &sock);
            let pids = bound_and_killed(&log);
            let _ = second.kill(); let _ = second.wait();
            assert!(r.is_ok() && started.elapsed() < DAEMON_START, "{r:?} after {:?}", started.elapsed());
            assert_eq!(pids.len(), 1, "daemons bound: {pids:?}");
            return;
        }
        panic!("the pid was never reused");
    }
}
