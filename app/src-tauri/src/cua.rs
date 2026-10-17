//! Per-window bridge to the app-owned cua-driver daemon.
//!
//! Owned startup is the default. Explicit configuration can select an existing
//! endpoint or the vendor installation. Requests never switch daemons after an
//! owned startup failure. The daemon holds the native OS permissions; this
//! client addresses each operation by process and window identifiers.

use serde_json::{json, Value};
use crate::{subprocess, transport};
use crate::action_failure::ActionFailure;
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
// The direct daemon-socket transport is unix-only (macOS/Linux). On Windows the
// daemon speaks over a named pipe, so we fall back to the CLI there.
use std::time::{Duration, Instant};

#[path = "daemon_gate.rs"]
mod daemon_gate;

#[cfg(windows)]
#[path = "windows_pipe.rs"]
mod windows_pipe;

// ── Our own driver ──────────────────────────────────────────────────────────
//
// The app prefers OUR build of cua-driver (from vendor-cua + our patch, signed
// with a stable Developer ID) over the separately-installed vendor CuaDriver.app.
// Ours skips the per-click window-change poll (ICU_NO_WINDOW_OBSERVE), so clicks
// are ~200ms instead of ~1.25s. It ships INSIDE the app bundle (Tauri externalBin
// -> next to the app executable); we also find a dev/script install in
// ~/.local/bin. When present the app launches it on its own socket with the fast
// env. Startup failure is reported without switching daemons. Select the vendor
// installation explicitly with ICU_OWN_DRIVER=0.

/// Find the platform's exact `cua-driver` executable directly inside `dir`,
/// matching the sidecar bundled next to the app executable
/// (Contents/MacOS/cua-driver). Split out so the resolution is unit-testable
/// without depending on the real executable's location.
fn driver_in_dir(dir: &std::path::Path) -> Option<String> {
    let direct = dir.join(format!("cua-driver{}", std::env::consts::EXE_SUFFIX));
    executable_file(&direct).then(|| direct.to_string_lossy().into_owned())
}

fn executable_file(path: &std::path::Path) -> bool {
    let Ok(meta) = path.metadata() else { return false };
    if !meta.is_file() { return false; }
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; meta.permissions().mode() & 0o111 != 0 }
    #[cfg(not(unix))]
    { true }
}

/// Locate our driver binary: an explicit ICU_CUA_BIN, else bundled next to the
/// app executable (Contents/MacOS/), else a dev/script install in ~/.local/bin.
fn resolve_own_driver_bin() -> Option<String> {
    resolve_binary_override(std::env::var("ICU_CUA_BIN").ok(), || {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if let Some(bin) = driver_in_dir(dir) {
                return Some(bin);
            }
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let local = format!("{home}/.local/bin/cua-driver");
    executable_file(std::path::Path::new(&local)).then_some(local)
    })
}

fn resolve_binary_override(explicit: Option<String>, fallback: impl FnOnce() -> Option<String>) -> Option<String> {
    // Command::new will validate the override, including PATH-based names.
    // A miss must not silently substitute another executable.
    explicit.or_else(fallback)
}

/// Owned startup is the default even when installation is incomplete.
/// ICU_OWN_DRIVER=0 is an explicit choice of the vendor installation.
fn own_driver_enabled() -> bool {
    prefer_owned(std::env::var("ICU_OWN_DRIVER").ok().as_deref())
}

fn prefer_owned(setting: Option<&str>) -> bool {
    !matches!(setting, Some("0") | Some(""))
}

/// The private socket used only for the app-owned child.
fn own_socket() -> String {
    #[cfg(test)]
    if let Ok(path) = std::env::var("ICU_TEST_OWN_SOCKET") { return path; }
    #[cfg(target_os = "windows")]
    { return format!(r"\\.\pipe\icu-cua-driver-{}", std::process::id()); }
    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        #[cfg(target_os = "macos")]
        let cache = format!("{home}/Library/Caches");
        #[cfg(not(target_os = "macos"))]
        let cache = std::env::var("XDG_CACHE_HOME").unwrap_or_else(|_| format!("{home}/.cache"));
        #[cfg(target_os = "linux")]
        return linux_owned_socket(&cache, std::env::var_os("XDG_RUNTIME_DIR").as_deref(), std::process::id());
        #[cfg(not(target_os = "linux"))]
        format!("{cache}/icu/driver-{}.sock", std::process::id())
    }
}

#[cfg(target_os = "linux")]
fn linux_owned_socket(cache: &str, runtime: Option<&std::ffi::OsStr>, pid: u32) -> String {
    use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
    // A valid cache directory can exceed sockaddr_un's pathname limit. Session
    // sockets belong in the desktop's private runtime directory, not its cache.
    if let Some(runtime) = runtime.map(std::path::Path::new) {
        let path = runtime.join("icu").join(format!("driver-{pid}.sock"));
        if runtime.is_absolute() && path.as_os_str().as_bytes().len() < 108
            && std::fs::symlink_metadata(runtime).is_ok_and(|m|
                m.is_dir() && m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o777 == 0o700)
        {
            if let Some(path) = path.to_str() { return path.to_owned(); }
        }
    }
    format!("{cache}/icu/driver-{pid}.sock")
}

#[cfg(all(test, target_os = "linux"))]
mod linux_socket_tests {
    use super::linux_owned_socket;
    use std::os::unix::{fs::{DirBuilderExt, PermissionsExt}, net::{UnixListener, UnixStream}};

    #[test]
    fn a_long_cache_path_keeps_a_real_private_session_socket_usable() {
        let root = std::env::temp_dir().join(format!("icu-runtime-socket-{}", std::process::id()));
        std::fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
        let _cleanup = Cleanup(root.clone());
        let cache = format!("/tmp/{}", "long-cache-".repeat(24));
        let fallback = linux_owned_socket(&cache, None, 42);
        assert_eq!(UnixListener::bind(&fallback).unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
        let socket = linux_owned_socket(&cache, Some(root.as_os_str()), 42);
        std::fs::create_dir(root.join("icu")).unwrap();
        let listener = UnixListener::bind(&socket).unwrap();
        let _client = UnixStream::connect(&socket).unwrap();
        let _accepted = listener.accept().unwrap();

        let alias = root.join("alias");
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        assert_eq!(linux_owned_socket(&cache, Some(alias.as_os_str()), 42), fallback);
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(linux_owned_socket(&cache, Some(root.as_os_str()), 42), fallback);
        assert_eq!(linux_owned_socket(&cache, Some(std::ffi::OsStr::new(".")), 42), fallback);
    }
}

#[derive(Default)]
struct DriverProcess {
    child: Option<std::process::Child>,
    retry_after: Option<Instant>,
    endpoint: Option<OwnedEndpoint>,
    closed: bool,
    generation: u64,
}
static PROCESS: std::sync::OnceLock<Mutex<DriverProcess>> = std::sync::OnceLock::new();
static DRIVER_CLOSING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn socket_listening(sock: &str) -> bool {
    #[cfg(unix)]
    { transport::connect(sock).is_ok() }
    #[cfg(windows)]
    { std::fs::OpenOptions::new().read(true).write(true).open(sock).is_ok() }
    #[cfg(not(any(unix, windows)))]
    { let _ = sock; false }
}

struct OwnedEndpoint {
    path: String,
    #[cfg(unix)]
    identity: (u64, u64),
}

#[cfg(unix)]
fn socket_identity(path: &str) -> Option<(u64, u64)> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let metadata = std::fs::symlink_metadata(path).ok()?;
    metadata.file_type().is_socket().then(|| (metadata.dev(), metadata.ino()))
}

fn listener_pid(path: &str) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let socket = transport::connect(path).ok()?;
        #[cfg(target_os = "linux")]
        unsafe {
            let mut credentials: libc::ucred = std::mem::zeroed();
            let mut size = std::mem::size_of_val(&credentials) as libc::socklen_t;
            return (libc::getsockopt(socket.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED,
                (&mut credentials as *mut libc::ucred).cast(), &mut size) == 0 && credentials.pid > 0)
                .then_some(credentials.pid as u32);
        }
        #[cfg(target_os = "macos")]
        unsafe {
            let mut pid: libc::pid_t = 0;
            let mut size = std::mem::size_of_val(&pid) as libc::socklen_t;
            return (libc::getsockopt(socket.as_raw_fd(), libc::SOL_LOCAL, libc::LOCAL_PEERPID,
                (&mut pid as *mut libc::pid_t).cast(), &mut size) == 0 && pid > 0).then_some(pid as u32);
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        { let _ = socket; None }
    }
    #[cfg(windows)]
    {
        windows_pipe::listener_pid(path, Duration::from_millis(250))
    }
    #[cfg(not(any(unix, windows)))]
    { let _ = path; None }
}

impl OwnedEndpoint {
    fn capture(path: &str, pid: u32) -> Option<Self> {
        #[cfg(unix)]
        let identity = socket_identity(path)?;
        if listener_pid(path) != Some(pid) { return None; }
        #[cfg(unix)]
        if socket_identity(path) != Some(identity) { return None; }
        Some(Self { path: path.into(), #[cfg(unix)] identity })
    }
    fn matches(&self, path: &str, pid: u32) -> bool {
        if !self.same_location(path) { return false; }
        listener_pid(path) == Some(pid)
    }
    fn same_location(&self, path: &str) -> bool {
        if self.path != path { return false; }
        #[cfg(unix)]
        if socket_identity(path) != Some(self.identity) { return false; }
        true
    }
    fn remove_stale(self) {
        #[cfg(unix)]
        if socket_identity(&self.path) == Some(self.identity) {
            let _ = std::fs::remove_file(self.path);
        }
    }
}

fn wait_for_child(child: &mut std::process::Child, budget: Duration) -> bool {
    let deadline = Instant::now() + budget;
    loop {
        if matches!(child.try_wait(), Ok(Some(_))) { return true; }
        if Instant::now() >= deadline { return false; }
        std::thread::sleep(Duration::from_millis(10));
    }
}

impl DriverProcess {
    fn shutdown(&mut self) {
        self.closed = true;
        self.stop_child(Duration::from_secs(15));
    }

    fn stop_child(&mut self, grace: Duration) -> bool {
        if let Some(child) = self.child.as_mut() {
            // This pipe is bound to the actual child, not a socket pathname.
            // EOF also reaches the driver if the host exits without running Drop.
            drop(child.stdin.take());
            if !wait_for_child(child, grace) {
                let _ = child.kill();
                if !wait_for_child(child, Duration::from_secs(1)) { return false; }
            }
        }
        self.child = None;
        if let Some(endpoint) = self.endpoint.take() { endpoint.remove_stale(); }
        true
    }

    fn ensure(&mut self, bin: &str, sock: &str) -> bool {
        if self.closed { return false; }
        if let Some(child) = &mut self.child {
            if matches!(child.try_wait(), Ok(Some(_))) {
                self.child = None;
                if let Some(endpoint) = self.endpoint.take() { endpoint.remove_stale(); }
            }
        }
        // A busy listener is not evidence that its live owner must be killed.
        // Keep the child and let the bounded transport report unavailability.
        if let (Some(child), Some(endpoint)) = (&self.child, &self.endpoint) {
            if endpoint.same_location(sock) { return endpoint.matches(sock, child.id()); }
        }
        // A listener without our live child is not an owned daemon. It could
        // be left over from a crashed app with a reused PID; never adopt it.
        if socket_listening(sock) {
            return self.child.as_ref().is_some_and(|child| self.endpoint.as_ref()
                .is_some_and(|endpoint| endpoint.matches(sock, child.id())));
        }
        if self.retry_after.is_some_and(|t| Instant::now() < t) { return false; }
        // A stale successful launch must not be cached forever. Reap the old
        // child before replacing it, and retry failed starts after a short pause.
        if !self.stop_child(Duration::from_secs(15)) { return false; }
        self.retry_after = Some(Instant::now() + Duration::from_secs(5));
        #[cfg(unix)]
        if let Some(dir) = std::path::Path::new(sock).parent() {
            if std::fs::create_dir_all(dir).is_err() { return false; }
        }
        let mut command = Command::new(bin);
        // Input revives per-session cursors. The app renders its own feedback,
        // so disable native overlays for the lifetime of its owned daemon.
        command.args(["serve", "--no-overlay", "--socket", sock])
            .env("CUA_DRIVER_EMBEDDED", "1")
            .env("CUA_DRIVER_PARENT_LIVENESS_STDIN", "1")
            .env("ICU_NO_WINDOW_OBSERVE", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(test)]
        command.stderr(std::process::Stdio::inherit());
        #[cfg(windows)]
        { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
        let Ok(mut child) = command.spawn() else { return false };
        let startup_deadline = Instant::now() + OWNED_DRIVER_STARTUP;
        while Instant::now() < startup_deadline {
            if !matches!(child.try_wait(), Ok(None)) { let _ = child.wait(); return false; }
            if let Some(endpoint) = OwnedEndpoint::capture(sock, child.id()) {
                self.child = Some(child);
                self.endpoint = Some(endpoint);
                self.generation = self.generation.wrapping_add(1);
                self.retry_after = None;
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        #[cfg(test)]
        eprintln!("owned driver startup timeout: pid={} socket={sock:?} bytes={} exists={} peer={:?}", child.id(), sock.len(), std::path::Path::new(sock).exists(), listener_pid(sock));
        let _ = child.kill();
        let _ = child.wait();
        false
    }
}

/// A Windows cold start may spend several seconds loading accessibility providers
/// and checking executable trust. Keep its bounded startup budget the same as the
/// lifecycle tests; readiness still returns immediately once the owned pipe opens.
#[cfg(all(not(test),not(windows)))]
const OWNED_DRIVER_STARTUP: Duration = Duration::from_secs(3);
#[cfg(any(test,windows))]
const OWNED_DRIVER_STARTUP: Duration = Duration::from_secs(10);

impl Drop for DriverProcess {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn own_driver_active() -> bool {
    // An explicit socket is an external endpoint. Never accidentally launch
    // another daemon because a separate binary override is also configured.
    if std::env::var_os("ICU_CUA_SOCKET").is_some() || !own_driver_enabled() { return false; }
    let Some(bin) = resolve_own_driver_bin() else { return false; };
    let mut process = PROCESS.get_or_init(|| Mutex::new(DriverProcess::default())).lock().unwrap();
    let previous = process.generation;
    let active = process.ensure(&bin, &own_socket());
    if active && process.generation != previous { *CURSOR_INIT.lock().unwrap() = false; }
    active
}

/// Stop only this app's child. An externally configured endpoint is never owned
/// by this process. Per-process sockets prevent an updater restart from adopting
/// a daemon executing the previous bundle's driver.
pub fn shutdown_owned_driver() {
    DRIVER_CLOSING.store(true, std::sync::atomic::Ordering::Release);
    let process = PROCESS.get_or_init(|| Mutex::new(DriverProcess::default()));
    process.lock().unwrap_or_else(std::sync::PoisonError::into_inner).shutdown();
}

/// Cancel only an outstanding portal setup in this app's already owned driver.
/// Bypass DAEMON_IO because the input waiting on consent holds it. This readies
/// no new daemon and cannot grant permission or undo input already delivered.
#[cfg(target_os = "linux")]
pub fn cancel_pending_portal_setup() -> Result<(), String> {
    if std::env::var_os("ICU_CUA_SOCKET").is_some() || !own_driver_enabled() { return Ok(()); }
    let Some(process) = PROCESS.get() else { return Ok(()); };
    let mut process = process.lock().map_err(|_| "driver state unavailable")?;
    let (Some(child), Some(endpoint)) = (&process.child, &process.endpoint) else { return Ok(()); };
    if !endpoint.matches(&endpoint.path, child.id()) { return Err("owned driver endpoint changed".into()); }
    let response = transport::roundtrip(&endpoint.path, &json!({"method":"cancel_pending_portal_setup", "client_kind":"cli"}))
        .map_err(|e| e.to_string())?;
    if response.get("cancellation_requested").and_then(Value::as_bool) == Some(true) {
        // The worker deliberately retains denial/cancellation for its lifetime.
        // Reap only this owned generation after cancelling the pending grant.
        // The next explicit task receives a new daemon and fresh portal consent.
        if !process.stop_child(Duration::from_secs(2)) { return Err("cancelled input worker did not stop".into()); }
        daemon_reset_session();
    }
    Ok(())
}

/// Where the cua-driver daemon listens, per platform (mirrors the driver's own
/// namespace convention). Overridable via ICU_CUA_SOCKET.
fn socket_path() -> Result<String, String> {
    if DRIVER_CLOSING.load(std::sync::atomic::Ordering::Acquire) { return Err("the app is shutting down".into()); }
    let path = select_socket(std::env::var("ICU_CUA_SOCKET").ok().as_deref(), own_driver_enabled(),
        &own_socket(), &vendor_socket(), own_driver_active)?;
    // Startup can overlap exit. Do not dispatch a queued request after waiting
    // for a driver whose owner has already closed admission.
    if DRIVER_CLOSING.load(std::sync::atomic::Ordering::Acquire) { return Err("the app is shutting down".into()); }
    Ok(path)
}

fn select_socket(explicit: Option<&str>, owned: bool, own: &str, vendor: &str, start: impl FnOnce() -> bool) -> Result<String, String> {
    if let Some(path) = explicit {
        return if path.is_empty() { Err("ICU_CUA_SOCKET must not be empty".into()) } else { Ok(path.into()) };
    }
    if owned {
        return if start() { Ok(own.into()) } else {
            Err("app-owned driver could not start; check the configured or bundled driver".into())
        };
    }
    Ok(vendor.into())
}

fn vendor_socket() -> String {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        return format!("{home}/Library/Caches/cua-driver/cua-driver.sock");
    }
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        return format!("{home}/.cache/cua-driver/cua-driver.sock");
    }
    #[cfg(target_os = "windows")]
    {
        return r"\\.\pipe\cua-driver".to_string();
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        return "/tmp/cua-driver.sock".to_string();
    }
}

/// The cua-driver CLI binary. On macOS it's the TCC-granted app bundle; elsewhere
/// we rely on it being on PATH. Overridable via ICU_CUA_BIN.
fn cua_bin() -> String {
    if let Ok(s) = std::env::var("ICU_CUA_BIN") {
        return s;
    }
    if own_driver_enabled() {
        if let Some(bin) = resolve_own_driver_bin() {
            return bin;
        }
    }
    #[cfg(target_os = "macos")]
    {
        return "/Applications/CuaDriver.app/Contents/MacOS/cua-driver".to_string();
    }
    #[cfg(not(target_os = "macos"))]
    {
        return "cua-driver".to_string();
    }
}

/// One call via the CLI subprocess, whose SDK validates daemon compatibility.
/// It uses the same selected endpoint as the direct transport. Input can reach
/// this fallback only before any action bytes were sent, preserving no replay.
/// Returns the structured result printed by the CLI.
pub fn call(tool: &str, args: &Value) -> Result<Value, String> {
    let result = cli_result(tool, &cli_output(tool, args, false)?)?;
    remember_capture_permission_denial(&result);
    Ok(result)
}

fn cli_output(tool: &str, args: &Value, envelope: bool) -> Result<String, String> {
    preflight_owned_request(tool, args)?;
    let mut command = Command::new(cua_bin());
    command.arg("--socket").arg(socket_path()?).arg("call").arg(tool).arg(args.to_string())
        .env_remove(action_outcomes::CLI_ENVELOPE_ENV);
    if envelope { command.env(action_outcomes::CLI_ENVELOPE_ENV, "1"); }
    let out = subprocess::output(&mut command, subprocess::CALL_TIMEOUT)
        .map_err(|e| format!("spawn cua-driver: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("cua-driver {tool}: {}", stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn call_action(tool: &str, args: &Value) -> Result<Value, ActionFailure> {
    let stdout = cli_output(tool, args, true)?;
    let result = action_outcomes::cli_tool_result(&stdout).map_err(|error|
        ActionFailure::Unknown(format!("{tool}: {error}; action outcome is unknown and was not replayed")))?;
    remember_capture_permission_denial(&result);
    if let Some(error) = action_outcomes::DriverError::from_tool_result(&result) {
        return Err(ActionFailure::Driver(error));
    }
    Ok(action_outcomes::tool_payload(&result))
}

/// The CLI prints a result's JSON, or, for a result with only text content
/// (how a failed tool answers, e.g. Windows `set_value` on a button), that text,
/// and still exits 0. That text is the driver's refusal, not unparseable output.
fn cli_result(tool: &str, stdout: &str) -> Result<Value, String> {
    let stdout = stdout.trim();
    serde_json::from_str::<Value>(stdout).map_err(|_| {
        let text: String = stdout.chars().take(500).collect();
        let text = text.strip_prefix(&format!("{tool}: ")).unwrap_or(&text);
        if text.is_empty() { format!("{tool} refused: the driver returned no result") } else { format!("{tool} refused: {text}") }
    })
}

// ── Direct daemon-socket transport (no subprocess) ──────────────────────────
//
// The CLI `call` fork/execs a large binary per request. wasteful for the live
// feed's ~2-3 captures/sec. We speak the daemon's newline-delimited JSON wire
// protocol (cua-driver-core::daemon::{DaemonRequest,DaemonResponse}) straight
// over its unix socket instead. Reads may fall back to the CLI; actions do so
// only on a pre-dispatch transport failure. One long-lived
// session (the daemon keeps it valid across calls, so no per-call session leak),
// serialized by a mutex so the shared session is never used concurrently.

#[cfg(unix)]
static DAEMON_IO: daemon_gate::Gate = daemon_gate::Gate::new();
static DAEMON_SESSION: Mutex<Option<String>> = Mutex::new(None);

/// One stable session label per app run, shared by BOTH the reads (get_state /
/// captures) and the input actions (click / type / scroll). Reads keep it live,
/// so an input on the same session is always addressable. that's what lets us
/// deliver inputs over the fast socket instead of forking the CLI. It must be
/// unique per run: a named session tombstones once it ends and a reused label is
/// then rejected ("session has ended"), so a fixed constant would be dead on the
/// next launch. Re-minted on any session error (see `daemon_reset_session`).
fn daemon_session() -> String {
    let mut g = DAEMON_SESSION.lock().unwrap();
    if let Some(s) = g.as_ref() {
        return s.clone();
    }
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let s = format!("icu-agent-{}-{}", std::process::id(), nanos);
    *g = Some(s.clone());
    s
}

fn daemon_reset_session() {
    *DAEMON_SESSION.lock().unwrap() = None;
    *CURSOR_INIT.lock().unwrap() = false; // new session: re-apply the cursor-hidden setup
}

// The owned daemon disables overlay rendering at startup with --no-overlay.
// Session setup also hides its cursor as best-effort compatibility for external
// endpoints. Native input may revive a per-session cursor, so this call alone
// does not enforce an external daemon's rendering policy.
static CURSOR_INIT: Mutex<bool> = Mutex::new(false);
static CURSOR_SETUP: Mutex<()> = Mutex::new(());

fn ensure_cursor_hidden() -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let _setup = loop {
        match CURSOR_SETUP.try_lock() {
            Ok(guard) => break guard,
            Err(std::sync::TryLockError::Poisoned(_)) => return Err("cursor setup failed; input not sent".into()),
            Err(_) if Instant::now() >= deadline => return Err("cursor setup busy; input not sent".into()),
            Err(_) => std::thread::sleep(Duration::from_millis(5)),
        }
    };
    // A recovered child has a new session registry and cursor configuration.
    // Start it before consulting the cached setup flag.
    socket_path()?;
    {
        let mut done = CURSOR_INIT.lock().unwrap();
        if *done {
            return Ok(());
        }
        *done = true; // fire exactly once per session. never re-fire per frame
    }
    let sid = daemon_session();
    // Establish the socket session with a READ first: the daemon auto-creates a
    // session for a read but rejects a MUTATION on an unknown session as "session
    // ended", so without this the first click would fail over and fall back to
    // the slow CLI fork. Then hide the cursor on that same session. Persistent
    // rendering policy comes from owned-daemon startup. Both use the socket.
    #[cfg(unix)]
    let initialized = {
        let read = json!({ "method": "call", "name": "list_windows", "args": {}, "session_id": sid, "client_kind": "cli" });
        let hide = json!({ "method": "call", "name": "set_agent_cursor_enabled",
            "args": { "session": sid, "enabled": false }, "session_id": sid, "client_kind": "cli" });
        daemon_roundtrip(&read).is_ok() && daemon_roundtrip(&hide).is_ok()
    };
    #[cfg(not(unix))]
    let initialized = call("set_agent_cursor_enabled", &json!({ "session": sid, "enabled": false })).is_ok();
    if !initialized { *CURSOR_INIT.lock().unwrap() = false; }
    Ok(())
}

/// Serialize protocol calls and preserve whether any request could have been sent.
fn daemon_roundtrip(req: &Value) -> Result<Value, transport::Error> {
    daemon_exchange(req, transport::roundtrip)
}

/// Admission only waits before any byte is sent, so it cannot cause a replay.
/// A passive read may hold the daemon for its whole 12 s budget, so an input
/// outlasts it instead of failing behind a slow tree or frame read.
const PASSIVE_ADMISSION: Duration = Duration::from_secs(2);
const INPUT_ADMISSION: Duration = Duration::from_secs(15);

fn daemon_exchange(
    req: &Value,
    roundtrip: fn(&str, &Value) -> Result<Value, transport::Error>,
) -> Result<Value, transport::Error> {
    daemon_exchange_within(req, roundtrip, PASSIVE_ADMISSION)
}

fn daemon_exchange_within(
    req: &Value,
    roundtrip: fn(&str, &Value) -> Result<Value, transport::Error>,
    admission: Duration,
) -> Result<Value, transport::Error> {
    if let Some(tool) = req.get("name").and_then(Value::as_str) {
        preflight_owned_request(tool, &req["args"]).map_err(transport::Error::Failed)?;
    }
    #[cfg(unix)]
    let _guard = DAEMON_IO.acquire(admission)
        .map_err(|error| transport::Error::Failed(error.into()))?;
    // A sibling preview may have been queued before the first denial arrived.
    // Recheck while holding the dispatch lock, then latch the response before
    // releasing it so that queued automatic captures cannot repeat consent.
    if let Some(tool) = req.get("name").and_then(Value::as_str) {
        preflight_owned_request(tool, &req["args"]).map_err(transport::Error::Failed)?;
    }
    let result = roundtrip(&socket_path().map_err(transport::Error::Failed)?, req);
    if let Ok(value) = &result { remember_capture_permission_denial(value); }
    if let Err(transport::Error::Tool(ActionFailure::Driver(error))) = &result {
        if let Some(fields) = &error.structured { remember_capture_permission_denial(fields); }
    }
    result
}

#[cfg(test)]
mod driver_selection_tests {
    use super::*;
    #[test]
    fn input_outlasts_a_passive_read_holding_the_daemon() {
        // A passive read holds the gate for its whole 12 s request budget.
        assert!(INPUT_ADMISSION > Duration::from_secs(12));
        assert!(PASSIVE_ADMISSION < INPUT_ADMISSION);
    }
    #[test]
    fn failed_owned_start_never_selects_vendor_socket() {
        let result = select_socket(None, true, "owned.sock", "vendor.sock", || false);
        assert!(result.is_err(), "failed owned startup selected {result:?}");
    }
    #[test]
    fn explicit_socket_does_not_launch_a_daemon() {
        assert_eq!(select_socket(Some("fixture.sock"), true, "owned.sock", "vendor.sock", || panic!("unexpected launch")).unwrap(), "fixture.sock");
        assert_eq!(select_socket(None, false, "owned.sock", "vendor.sock", || panic!("unexpected launch")).unwrap(), "vendor.sock");
    }
    #[test]
    fn invalid_explicit_binary_does_not_start_a_different_binary() {
        let chosen = "missing-explicit-test-driver".to_string();
        assert_eq!(resolve_binary_override(Some(chosen.clone()), || Some("unexpected-fallback".into())), Some(chosen));
    }
    #[test]
    fn owned_driver_is_the_default_even_when_installation_is_incomplete() {
        assert!(prefer_owned(None));
        assert!(prefer_owned(Some("1")));
        assert!(!prefer_owned(Some("0")));
    }
    #[test]
    fn empty_endpoint_is_an_error_without_startup() {
        assert!(select_socket(Some(""), true, "owned.sock", "vendor.sock", || panic!("unexpected launch")).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn preexisting_listener_is_not_adopted_as_an_owned_child() {
        let path = std::env::temp_dir().join(format!("icu-other-{}.sock", std::process::id()));
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let mut process = DriverProcess::default();
        assert!(!process.ensure("missing-owned-binary", path.to_str().unwrap()));
        assert!(process.child.is_none());
        drop(process);
        assert!(path.exists(), "must not unlink somebody else's listener");
        drop(listener);
        std::fs::remove_file(path).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn failed_cursor_setup_is_not_cached_as_ready() {
        // Isolate configuration and session globals from the rest of the suite.
        // The only endpoint is a nonexistent path in this test's private dir.
        if std::env::var_os("ICU_CURSOR_SETUP_TEST_CHILD").is_none() {
            let scratch = CaptureFile::new().unwrap();
            let out = crate::child_test_output(Command::new(std::env::current_exe().unwrap())
                .args(["cua::driver_selection_tests::failed_cursor_setup_is_not_cached_as_ready", "--exact", "--nocapture"])
                .env("ICU_CURSOR_SETUP_TEST_CHILD", "1")
                .env("ICU_CUA_SOCKET", scratch.0.join("missing.sock")), Duration::from_secs(15)).unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
            return;
        }
        *CURSOR_INIT.lock().unwrap() = false;
        let _ = ensure_cursor_hidden();
        assert!(!*CURSOR_INIT.lock().unwrap(), "failed setup must remain retryable");
    }
}

/// A daemon `call` returning the `structuredContent` shape (matching CLI `call`).
/// If the shared session has expired, resets it and errors so the caller can
/// fall back; the next call mints a fresh session and the fast path resumes.
fn daemon_call(tool: &str, args: &Value) -> Result<Value, transport::Error> {
    daemon_call_sess(tool, args, &daemon_session())
}

/// Like `daemon_call` but with an explicit protocol `session_id`. Inputs pass
/// the same `daemon_session` the reads use so the protocol session and the args'
/// session are ONE. the daemon has seen it start (via the observe reads), so it
/// never rejects the action as "session ended".
fn daemon_call_sess(tool: &str, args: &Value, session_id: &str) -> Result<Value, transport::Error> {
    daemon_call_within(tool, args, session_id, PASSIVE_ADMISSION)
}

fn daemon_call_within(tool: &str, args: &Value, session_id: &str, admission: Duration) -> Result<Value, transport::Error> {
    preflight_owned_request(tool,args).map_err(transport::Error::Failed)?;
    ensure_cursor_hidden().map_err(transport::Error::Failed)?;
    let req = json!({
        "method": "call",
        "name": tool,
        "args": args,
        "session_id": session_id,
        "client_kind": "cli",
    });
    let result = daemon_exchange_within(&req, transport::roundtrip, admission)?;
    let structured = result
        .get("structuredContent")
        .cloned()
        .unwrap_or_else(|| result.clone());
    if session_ended(&structured) {
        daemon_reset_session();
        return Err(transport::Error::Tool(ActionFailure::Driver(action_outcomes::DriverError { message:"daemon session ended; action not replayed".into(), structured:Some(structured) })));
    }
    Ok(structured)
}

fn session_ended(structured: &Value) -> bool {
    structured
        .get("refusal")
        .and_then(|r| r.get("code"))
        .and_then(Value::as_str)
        .map(|c| matches!(c, "session_ended" | "session_not_found" | "unknown_session"))
        .unwrap_or(false)
}

/// Read-only call: try the socket, preserving CLI compatibility for transport
/// failures. An occupied daemon queue is not permission to bypass serialization.
fn read_call(tool: &str, args: &Value) -> Result<Value, String> {
    daemon_gate::passive(daemon_call(tool, args).map_err(|error| error.to_string()), || call(tool, args))
}

/// Prefix of a capture error that is the driver's typed answer about the
/// window, not a transport failure. The frame protocol shows these on the tile.
pub const CAPTURE_REFUSED: &str = "capture refused: ";

// Denial belongs to the selected capture endpoint, never another driver's
// grants. A restart at the same endpoint conservatively keeps the latch until
// explicit retry; passive polls and session restarts cannot erase a denial.
static CAPTURE_PERMISSION_DENIALS: Mutex<Vec<(String,Value)>> = Mutex::new(Vec::new());
pub(crate) const CAPTURE_PERMISSION_MESSAGE: &str = "Screen capture was denied. Automatic capture is paused. Choose Retry access in Copilot after updating capture access.";

fn capture_permission_key() -> String {
    if uses_host_owned_driver() { return "app".into(); }
    format!("driver:{}",std::env::var("ICU_CUA_SOCKET").unwrap_or_else(|_|vendor_socket()))
}

fn capture_denial_for(key: &str) -> Option<Value> {
    CAPTURE_PERMISSION_DENIALS.lock().unwrap_or_else(|poisoned|poisoned.into_inner())
        .iter().find(|(identity,_)|identity==key).map(|(_,error)|error.clone())
}

fn capture_permission_denial() -> Option<Value> { capture_denial_for(&capture_permission_key()) }

pub(crate) fn capture_permission_error() -> Option<String> {
    capture_permission_denial().map(|_| CAPTURE_PERMISSION_MESSAGE.into())
}

pub(crate) fn retry_capture_permission_after_user_action(screen_recording_granted: bool) {
    if screen_recording_granted {
        let key=capture_permission_key();
        CAPTURE_PERMISSION_DENIALS.lock().unwrap_or_else(|poisoned|poisoned.into_inner()).retain(|(identity,_)|identity!=&key);
    }
}

fn confirmed_capture_denial(result: &Value) -> Option<Value> {
    let fields = result.get("structuredContent").unwrap_or(result);
    [fields.get("screenshot_error"), fields.pointer("/refusal/detail/screenshot_error"), Some(fields)]
        .into_iter().flatten().find(|error|error.get("code").and_then(Value::as_str) == Some("capture_permission_denied")).cloned()
}

fn remember_capture_permission_denial(result: &Value) {
    if let Some(error) = confirmed_capture_denial(result) {
        let key=capture_permission_key();
        let mut latches=CAPTURE_PERMISSION_DENIALS.lock().unwrap_or_else(|poisoned|poisoned.into_inner());
        if let Some((_,denial))=latches.iter_mut().find(|(identity,_)|identity==&key) { *denial=error; }
        else { latches.push((key,error)); }
    }
}

pub(crate) fn uses_host_owned_driver() -> bool {
    std::env::var_os("ICU_CUA_SOCKET").is_none() && own_driver_enabled()
}

/// A passive query against the already selected external endpoint. Unlike
/// ordinary reads this never initializes a cursor/session by listing windows.
pub(crate) fn passive_permission_probe(tool: &str, args: &Value) -> Result<Value, String> {
    debug_assert!(!uses_host_owned_driver());
    #[cfg(unix)]
    {
        let request = json!({"method":"call","name":tool,"args":args,"client_kind":"cli"});
        let result = daemon_exchange(&request, transport::roundtrip).map_err(|error| error.to_string())?;
        return Ok(result.get("structuredContent").cloned().unwrap_or(result));
    }
    #[cfg(not(unix))]
    { call(tool,args) }
}

/// Host grants apply only to the embedded driver. A confirmed capture denial
/// applies only to the endpoint that returned it. Nothing here requests access.
fn preflight_owned_request(tool: &str, args: &Value) -> Result<(), String> {
    let (ax, capture) = request_permissions(tool, args);
    if capture && capture_permission_denial().is_some() {
        return Err(format!("{CAPTURE_REFUSED}capture_permission_denied: {CAPTURE_PERMISSION_MESSAGE}"));
    }
    if !uses_host_owned_driver() || (!ax && !capture) { return Ok(()); }
    preflight_io(&crate::permissions::io_status(), ax, capture)
}

fn preflight_io(status: &crate::permissions::Status, ax: bool, capture: bool) -> Result<(), String> {
    match crate::permissions::missing_io_permission(status, ax, capture) {
        Some(message) if capture => Err(format!("{CAPTURE_REFUSED}permission_required: {message}")),
        Some(message) => Err(format!("operation refused: permissions_not_granted. {message}")),
        None => Ok(()),
    }
}

fn request_permissions(tool: &str, args: &Value) -> (bool, bool) {
    match tool {
        "get_window_state" => (args.get("include_accessibility_tree").and_then(Value::as_bool) != Some(false),
            args.get("include_screenshot").and_then(Value::as_bool) != Some(false) || args.get("screenshot_out_file").is_some()),
        "get_accessibility_tree" => (true, false),
        "get_desktop_state" => (false, true),
        // Native pixel routes take a fresh capture to validate the coordinate
        // frame. Even an element click can enter Finder's selection fallback.
        "click" | "double_click" | "right_click" | "drag" | "mouse_button_down"
            | "mouse_button_up" | "mouse_drag" => (true, true),
        "scroll" => (true, args.get("x").is_some() || args.get("y").is_some() || args.get("scope").and_then(Value::as_str) == Some("desktop")),
        "type_text" | "press_key" | "hotkey" | "set_value" | "select_text" | "perform_secondary_action" => (true, false),
        _ => (false, false),
    }
}

/// The user-facing reason for a typed `screenshot_error`, if the driver sent one.
fn capture_refusal(structured: &Value) -> Option<String> {
    let error = structured.get("screenshot_error")?;
    let code = error.get("code").and_then(Value::as_str)?;
    let text = match code {
        "window_minimized" => "window is minimized. Restore it to see it here.",
        "window_not_viewable" => "window is hidden or on another desktop.",
        "window_offscreen" => "window is partly off screen and no compositor keeps those pixels. Move it fully on screen.",
        "surface_identity_unproven" => "Wayland can't prove which window these pixels belong to.",
        "capture_failed" => "the window could not be captured.",
        other => return Some(format!("{CAPTURE_REFUSED}{other}")),
    };
    Some(format!("{CAPTURE_REFUSED}{text} ({code})"))
}

/// Capture a window's PNG inline over the socket (no subprocess, no temp file):
/// the daemon returns the image as base64 in the result `content[]`. A typed
/// refusal is final; other errors let callers fall back to the CLI + file path.
fn daemon_capture(pid: i64, window_id: i64, max_dim: i64) -> Result<(Vec<u8>, String, Option<String>), String> {
    ensure_cursor_hidden()?;
    let req = json!({
        "method": "call",
        "name": "get_window_state",
        "args": {
            "pid": pid,
            "window_id": window_id,
            "include_screenshot": true,
            "include_accessibility_tree": false,
            "max_dimension": max_dim,
        },
        "session_id": daemon_session(),
        "client_kind": "cli",
    });
    // Read-only, so a failed call comes back whole: a capture-only refusal is
    // an isError result whose typed reason is in structuredContent.
    let result = daemon_exchange(&req, transport::roundtrip_tool_result).map_err(|e| e.to_string())?;
    if let Some(sc) = result.get("structuredContent") {
        if session_ended(sc) {
            daemon_reset_session();
            return Err("daemon session ended".into());
        }
    }
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return Err(result.get("structuredContent").and_then(capture_refusal)
            .unwrap_or_else(|| "daemon capture: tool error".into()));
    }
    daemon_frame(&result)
}

/// The image of a capture-only `get_window_state` result, its mime type and
/// the driver's id for that capture (`screenshot_id`, when it issues one).
fn daemon_frame(result: &Value) -> Result<(Vec<u8>, String, Option<String>), String> {
    let screenshot_id = result.get("structuredContent")
        .and_then(observed_screenshot_id)
        .map(str::to_owned);
    let content = result
        .get("content")
        .and_then(Value::as_array)
        .ok_or("daemon capture: no content")?;
    for item in content {
        if item.get("type").and_then(Value::as_str) == Some("image") {
            if let Some(b64) = item.get("data").and_then(Value::as_str) {
                use base64::Engine;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .map_err(|e| format!("daemon capture b64: {e}"))?;
                if bytes.is_empty() {
                    return Err("daemon capture: empty image".into());
                }
                let mime = item
                    .get("mimeType")
                    .and_then(Value::as_str)
                    .unwrap_or("image/png")
                    .to_string();
                return Ok((bytes, mime, screenshot_id));
            }
        }
    }
    if let Some(sc) = result.get("structuredContent") {
        if let Some(reason) = capture_refusal(sc).or_else(|| refusal_reason(sc)) {
            return Err(reason);
        }
    }
    Err("daemon capture: no inline screenshot".into())
}

/// Is the driver daemon reachable? (a cheap probe)
pub fn ping() -> Result<(), String> {
    // list_windows is the lightest real call; treat any parseable reply as up.
    read_call("list_windows", &json!({})).map(|_| ())
}

/// Addressable content windows. Platform-specific layer and workspace metadata
/// are optional; missing values must not be fabricated into a refusal.
/// Titles are shown in the UI and never logged here.
pub fn list_windows() -> Result<Value, String> {
    let v = read_call("list_windows", &json!({}))?;
    project_windows(&v, std::process::id() as i64)
}

/// History observes the actual front window, including this app. The target picker
/// intentionally removes our own windows, so it cannot be used as a focus source.
pub(crate) fn history_windows()->Result<Value,String>{
    let value=read_call("list_windows",&json!({}))?;
    project_history_windows(&value)
}
fn project_history_windows(value:&Value)->Result<Value,String>{
    let windows=value["windows"].as_array().ok_or("Window discovery did not return windows")?;
    let mut visible:Vec<_>=windows.iter().filter(|w|w["is_on_screen"]==true&&w["on_current_space"]==true&&w["layer"].as_i64().unwrap_or(0)==0&&w["pid"].as_i64().unwrap_or(0)>0&&w["window_id"].as_i64().unwrap_or(0)>0).cloned().collect();
    visible.sort_by(|a,b|b["z_index"].as_i64().unwrap_or(i64::MIN).cmp(&a["z_index"].as_i64().unwrap_or(i64::MIN)));
    Ok(json!({"windows":visible}))
}

fn project_windows(v: &Value, self_pid: i64) -> Result<Value, String> {
    let cur = v.get("current_space_id").cloned().unwrap_or(Value::Null);
    let skip = ["Cua Driver", "CuaDriver", "Window Server", "Dock", "Spotlight"];
    let windows = v.get("windows").and_then(Value::as_array).ok_or_else(|| {
        let reason = v.get("code").and_then(Value::as_str).unwrap_or("missing windows array");
        format!("window discovery failed: {reason}")
    })?;
    let mut wins: Vec<Value> = windows.iter()
        .filter(|w| {
            let b = w.get("bounds");
            let wdt = b.and_then(|b| b.get("width")).and_then(Value::as_f64).unwrap_or(0.0);
            let hgt = b.and_then(|b| b.get("height")).and_then(Value::as_f64).unwrap_or(0.0);
            let content_layer = match w.get("layer") {
                None | Some(Value::Null) => true,
                Some(layer) => layer.as_i64() == Some(0),
            };
            let app = w.get("app_name").and_then(Value::as_str).unwrap_or("");
            let pid = w.get("pid").and_then(Value::as_i64).unwrap_or(0);
            let id = w.get("window_id").and_then(Value::as_i64).unwrap_or(0);
            let origin_valid = ["x", "y"].iter().all(|key| b.and_then(|b| b.get(*key)).and_then(Value::as_f64).is_some_and(f64::is_finite));
            // A window with no title that the system says is off screen is a helper's hidden
            // surface (macOS 26 runs an "AutoFill" process per app with text fields), not a
            // target anyone picks. Untitled windows on screen, and titled ones anywhere, stay.
            let untitled = w.get("title").and_then(Value::as_str).is_none_or(|t| t.trim().is_empty());
            let hidden_helper = untitled && w.get("is_on_screen").and_then(Value::as_bool) == Some(false);
            content_layer && pid > 0 && id > 0 && pid != self_pid && origin_valid && !hidden_helper
                // Compact content windows are valid targets (Calculator is 198 points wide).
                // The title/visibility and layer checks above exclude hidden helpers.
                && wdt.is_finite() && hgt.is_finite() && wdt > 0.0 && hgt > 0.0 && !skip.contains(&app)
        })
        .cloned()
        .collect();
    // current-Space + on-screen first, then by z-order (likely the real content windows).
    wins.sort_by(|a, b| {
        let key = |w: &Value| {
            let on = w.get("on_current_space").and_then(Value::as_bool).unwrap_or(false) as i64;
            let scr = w.get("is_on_screen").and_then(Value::as_bool).unwrap_or(false) as i64;
            let z = w.get("z_index").and_then(Value::as_i64).unwrap_or(0);
            (on * 2 + scr, z)
        };
        key(b).cmp(&key(a))
    });
    let mut out = json!({ "current_space_id": cur, "windows": wins });
    // The driver says when a whole class of windows cannot appear (native
    // Wayland windows with its Wayland backend off); the picker shows why.
    if let Some(coverage) = v.get("window_coverage").filter(|c| c.get("complete") == Some(&Value::Bool(false))) {
        out["window_coverage"] = coverage.clone();
    }
    Ok(out)
}

/// How many displays are attached. Lets the UI offer one whole-screen tile per
/// monitor (the daemon's get_desktop_state only ever sees the main display).
pub fn display_count() -> i64 {
    #[cfg(target_os = "macos")]
    {
        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" {
            fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
        }
        let mut count: u32 = 0;
        let ok = unsafe { CGGetActiveDisplayList(0, std::ptr::null_mut(), &mut count) };
        if ok == 0 && count > 0 { count as i64 } else { 1 }
    }
    #[cfg(not(target_os = "macos"))]
    {
        1
    }
}

/// Name and subtitle for the single whole-screen tile, matching what the
/// driver's `get_desktop_state` actually captures on this platform. macOS and
/// Windows capture the primary display; Linux X11 captures the whole root
/// window, which spans every RandR monitor.
pub fn desktop_label() -> (&'static str, &'static str) {
    if cfg!(target_os = "linux") {
        ("All displays", "every monitor as one screen")
    } else {
        ("Main display", "primary screen")
    }
}

/// Shrink an encoded PNG so its longest edge is at most `max_dim`, in process:
/// no `sips` subprocess and no temp-file rewrite per frame. Already-small
/// images and any decode/encode failure return the input unchanged, so the
/// feed degrades to full size rather than breaking.
#[cfg(target_os = "macos")]
fn downscale_png(bytes: Vec<u8>, max_dim: i64) -> Vec<u8> {
    if max_dim < 1 {
        return bytes;
    }
    let bound = max_dim as u32;
    let Ok(img) = image::load_from_memory(&bytes) else { return bytes };
    if img.width().max(img.height()) <= bound {
        return bytes;
    }
    // resize fits WITHIN (bound, bound) keeping aspect, so it bounds the longest edge.
    let resized = img.resize(bound, bound, image::imageops::FilterType::Triangle);
    let mut out = std::io::Cursor::new(Vec::new());
    match resized.write_to(&mut out, image::ImageFormat::Png) {
        Ok(()) => out.into_inner(),
        Err(_) => bytes,
    }
}

/// Capture a whole display (1 = main, 2 = secondary, ...) as raw PNG bytes.
/// Uses macOS `screencapture -D<n>`, the one mechanism that can address a
/// non-primary monitor (the daemon's desktop capture is main-display-only).
/// `max_dim` bounds the longest edge via an in-process resize.
pub fn display_shot_bytes(display: i64, max_dim: i64) -> Result<Vec<u8>, String> {
    #[cfg(target_os = "macos")]
    {
        // This subprocess belongs to Copilot even with an external driver.
        preflight_io(&crate::permissions::io_status(), false, true)?;
        if capture_denial_for("app").is_some() {
            return Err(format!("{CAPTURE_REFUSED}capture_permission_denied: {CAPTURE_PERMISSION_MESSAGE}"));
        }
        let capture = CaptureFile::new()?;
        let path = capture.path();
        if display < 1 || display > display_count() || max_dim < 1 {
            return Err("invalid display or capture size".into());
        }
        let out = subprocess::output(Command::new("/usr/sbin/screencapture")
            .arg("-x")
            .arg(format!("-D{display}"))
            .arg(&path), std::time::Duration::from_secs(5))
            .map_err(|e| format!("screencapture: {e}"))?;
        if !out.status.success() {
            return Err(format!("screencapture -D{display} failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        let bytes = std::fs::read(&path).map_err(|e| format!("read display shot: {e}"))?;
        if bytes.is_empty() {
            return Err("empty display capture".into());
        }
        // Downscale in process instead of spawning `sips` on the temp file.
        Ok(downscale_png(bytes, max_dim))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (display, max_dim);
        Err("per-display capture is macOS-only".into())
    }
}

/// Capture one window as an image data URL (no AX walk, so it stays fast enough
/// for a live feed). Works for off-Space windows too. `max_dim` bounds the
/// longest edge. get_window_state writes the PNG to `screenshot_file_path` and
/// returns metadata; capture can succeed even when input routes are refused.
pub fn window_shot(pid: i64, window_id: i64, max_dim: i64) -> Result<String, String> {
    let (bytes, mime) = window_shot_bytes(pid, window_id, max_dim)?;
    Ok(data_url(&mime, &bytes))
}

/// Capture one window as raw image bytes + mime. Used by the custom frame
/// protocol so the webview never has to decode a giant base64 string on its
/// main thread (the pinwheel source).
pub fn window_shot_bytes(pid: i64, window_id: i64, max_dim: i64) -> Result<(Vec<u8>, String), String> {
    window_frame(pid, window_id, max_dim).map(|(bytes, mime, _)| (bytes, mime))
}

/// One window capture as raw image bytes, mime and the driver's
/// `screenshot_id` for it. A pixel read off these bytes is bound to that id.
pub fn window_frame(pid: i64, window_id: i64, max_dim: i64) -> Result<(Vec<u8>, String, Option<String>), String> {
    preflight_owned_request("get_window_state", &json!({"include_accessibility_tree":false,"include_screenshot":true}))?;
    // Fast path: pull the PNG inline over the daemon socket (no subprocess, no
    // temp file). A typed refusal or admission timeout must not launch another
    // capture. Other read transport failures retain the CLI compatibility path.
    let captured = daemon_capture(pid, window_id, max_dim);
    match &captured {
        Err(refused) if refused.starts_with(CAPTURE_REFUSED) => return Err(refused.clone()),
        _ => {}
    }
    daemon_gate::passive(captured, || cli_window_frame(pid, window_id, max_dim))
}

fn cli_window_frame(pid: i64, window_id: i64, max_dim: i64) -> Result<(Vec<u8>, String, Option<String>), String> {
    let capture = CaptureFile::new()?;
    let path = capture.path();
    let path_s = path.to_string_lossy().to_string();
    let _ = std::fs::remove_file(&path);
    let res = call(
        "get_window_state",
        &json!({
            "pid": pid,
            "window_id": window_id,
            "include_screenshot": true,
            "include_accessibility_tree": false,
            "max_dimension": max_dim,
            "screenshot_out_file": path_s,
        }),
    )?;
    let mime = res
        .get("screenshot_mime_type")
        .and_then(Value::as_str)
        .unwrap_or("image/png")
        .to_string();
    let reported = res
        .get("screenshot_file_path")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from);
    let screenshot_id = observed_screenshot_id(&res).map(str::to_owned);
    for candidate in [reported.as_deref(), Some(path.as_path())].into_iter().flatten() {
        if let Ok(bytes) = std::fs::read(candidate) {
            if !bytes.is_empty() {
                return Ok((bytes, mime, screenshot_id));
            }
        }
    }
    if let Some(reason) = capture_refusal(&res).or_else(|| refusal_reason(&res)) {
        return Err(reason);
    }
    Err("no screenshot returned".into())
}

// ── Driving: actions + AX state (the "hands") ───────────────────────────────

/// Bring a specific window to the foreground on the CURRENT Space (in-Space focus
/// via cua-driver). Does not cross Spaces.
pub fn activate(pid: i64, window_id: i64) -> Result<Value, String> {
    call("bring_to_front", &json!({ "pid": pid, "window_id": window_id }))
}

/// Cross-Space activation. On macOS `open -a <app>` activates the app and the OS
/// switches the current Space to the app's window (the default "switch to a Space
/// with open windows" behavior). this is how we drive a window on another Space.
/// The Spaces model is macOS-specific; on Windows/Linux this is a no-op and the
/// driver's own focus (bring_to_front) handles virtual-desktops/workspaces.
pub fn activate_app(app_name: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        subprocess::output(Command::new("open")
            .arg("-a")
            .arg(app_name), std::time::Duration::from_secs(5))
            .map_err(|e| format!("open -a {app_name}: {e}"))?;
        std::thread::sleep(std::time::Duration::from_millis(1100));
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app_name;
    }
    Ok(())
}

/// Ensure a window is on the current Space so it can be driven. If it's already
/// current, no-op. Otherwise activate it (macOS switches Space via the app; other
/// platforms use the driver's native focus) and re-check.
pub fn ensure_current_space(pid: i64, window_id: i64, app_name: &str) -> bool {
    if is_on_current_space(pid, window_id) {
        return true;
    }
    let _ = activate_app(app_name);
    #[cfg(not(target_os = "macos"))]
    let _ = activate(pid, window_id);
    is_on_current_space(pid, window_id)
}

pub fn is_on_current_space(pid: i64, window_id: i64) -> bool {
    let Ok(v) = call("list_windows", &json!({})) else { return false };
    v.get("windows")
        .and_then(Value::as_array)
        .map(|ws| {
            ws.iter().any(|w| {
                w.get("pid").and_then(Value::as_i64) == Some(pid)
                    && w.get("window_id").and_then(Value::as_i64) == Some(window_id)
                    && w.get("on_current_space").and_then(Value::as_bool) == Some(true)
            })
        })
        .unwrap_or(false)
}

/// Run one input tool in the BACKGROUND (window2 model): input is delivered to
/// the target window without foregrounding it, so driving a tile never steals
/// your focus unless this task has explicit foreground authority. Off-Space windows
/// come back as a refusal instead of being force-activated; the user brings a
/// window over on purpose (the tile's Activate control), so focus is never yanked
/// out from under them. `cua::activate` remains the one, explicit way to fore-
/// ground a window outside an explicitly authorized foreground task.
fn input(tool: &str, mut args: Value) -> Result<Value, ActionFailure> {
    preflight_owned_request(tool, &args)
        .map_err(|message| ActionFailure::refused("permissions_not_granted", message))?;
    let keyboard = matches!(tool, "press_key" | "hotkey");
    let platform = if keyboard {
        read_call("health_report", &json!({})).ok()
            .and_then(|v| v.get("platform").and_then(Value::as_str).map(str::to_owned))
    } else { None };
    // PID-routed macOS keys can reset global HID idle state even when the
    // foreground app, keyboard focus and pointer remain unchanged. The app's
    // background-only contract excludes that synthetic event route. Keep the
    // refusal before either socket delivery or CLI fallback; semantic AX
    // editing and clicks remain available. An unknown platform fails closed.
    if keyboard && !(matches!(platform.as_deref(), Some("linux" | "win32")) || (args.get("delivery_mode").and_then(Value::as_str)==Some("foreground") && matches!(platform.as_deref(),Some("darwin")))) {
        return Err(ActionFailure::refused("background_unavailable", format!("{tool} refused: background_unavailable. Synthetic keyboard input cannot preserve shared input state on this target. Use set_value to edit a control, click a visible control, or finish without the key.")));
    }
    let specs: Vec<&str> = match tool {
        "press_key" => args.get("key").and_then(Value::as_str).into_iter().collect(),
        "hotkey" => args.get("keys").and_then(Value::as_array)
            .map(|keys| keys.iter().filter_map(Value::as_str).collect()).unwrap_or_default(),
        _ => Vec::new(),
    };
    if let Some(spec) = specs.into_iter().find(|spec| confirmations::press_key_hard_denied(spec).is_some()) {
        // The app bypasses Layer::dispatch, so it must enforce the shared rule
        // before either socket delivery or CLI fallback can receive a key.
        if let Some(reason) = confirmations::press_key_hard_denied_on_platform(spec, platform.as_deref()) {
            return Err(ActionFailure::refused("hard_denied", format!("{tool} refused: {reason}")));
        }
    }
    // Pin every input to the one stable session the reads use (kept live by the
    // observe step), so the daemon always knows it, and deliver over the DIRECT
    // SOCKET. Forking the cua-driver CLI costs ~150ms per action (measured) vs a
    // ~3ms socket roundtrip. that fork, not the click, is the per-step stall.
    // The socket reaches the same daemon (same TCC identity the reads use), so
    // it's identical in effect. Fall back to the CLI ONLY when the daemon can't
    // be reached at all, or the direct transport is unavailable on this OS.
    // In both cases no request was sent. Any error AFTER connecting
    // (write/read/timeout) might have delivered the action, so we never retry it.
    let session = daemon_session();
    if let Value::Object(map) = &mut args {
        map.entry("session").or_insert_with(|| json!(session));
        // Ask the daemon to skip its post-action window-change poll. our agent
        // loop re-observes the whole window after every action, so we don't need
        // the driver to detect side-effects for us. Honored by drivers that read
        // `_skip_window_change_detection`; a no-op on ones that don't.
        map.entry("_skip_window_change_detection").or_insert(json!(true));
    }
    let result = transport::deliver(daemon_call_within(tool, &args, &session, INPUT_ADMISSION), || call_action(tool, &args));
    if result.is_err() { daemon_reset_session(); }
    let value = result?;
    if let Some(error) = coded_answer(&value) { return Err(error); }
    Ok(value)
}

/// Interpret native metadata before the UI renders it as a string.
fn coded_answer(value: &Value) -> Option<ActionFailure> {
    action_outcomes::DriverError::from_action_payload(value).map(ActionFailure::Driver)
}

/// Click an element by its stable token from the latest get_state (cua-driver
/// refuses a bare element_index. it needs the token or a snapshot_id). Focuses
/// the element too, so a following type lands there.
pub fn click_token(pid: i64, window_id: i64, element_token: &str) -> Result<Value, String> {
    input(
        "click",
        json!({ "pid": pid, "window_id": window_id, "element_token": element_token }),
    ).map_err(|error| error.to_string())
}

/// Click an element with the full daemon surface: `button` ("left"/"right"/
/// "middle"), `count` (2 = double-click), and `action` (an AX action name:
/// "press"/"show_menu"/"pick"/"confirm"/"cancel"/"open"). `action` is how the
/// Codex `perform_secondary_action` verb (Raise/Expand/Collapse/menus) is reached
/// here. it drives the element's own accessibility action, still focus-free.
pub fn click_element(
    pid: i64,
    window_id: i64,
    element_token: &str,
    button: Option<String>,
    action: Option<String>,
    count: Option<i64>,
) -> Result<Value, ActionFailure> {
    let mut args = json!({ "pid": pid, "window_id": window_id, "element_token": element_token });
    if let Some(b) = button {
        args["button"] = json!(b);
    }
    if let Some(a) = action {
        args["action"] = json!(a);
    }
    if let Some(c) = count {
        args["count"] = json!(c);
    }
    input("click", args)
}

/// Replace the value of an editable element by token (AX `set_value`), focus-free.
pub fn set_value_token(pid: i64, window_id: i64, element_token: &str, value: String) -> Result<Value, ActionFailure> {
    input(
        "set_value",
        json!({ "pid": pid, "window_id": window_id, "element_token": element_token, "value": value }),
    )
}

/// Press-drag-release from (from_x, from_y) to (to_x, to_y) in the window-local
/// screenshot-pixel space `get_window_state` reports. Focus-free by default.
/// `screenshot_id` names the observation the points were read from. The live
/// tile re-captures the same window on the same daemon, so without it the
/// driver can only check the points against whichever capture ran last.
pub fn drag(pid: i64, window_id: i64, from_x: f64, from_y: f64, to_x: f64, to_y: f64, screenshot_id: Option<&str>) -> Result<Value, String> {
    drag_with_delivery(pid,window_id,from_x,from_y,to_x,to_y,screenshot_id,false).map_err(|error| error.to_string())
}
#[allow(clippy::too_many_arguments)]
pub fn drag_with_delivery(pid:i64,window_id:i64,from_x:f64,from_y:f64,to_x:f64,to_y:f64,screenshot_id:Option<&str>,allow_foreground:bool)->Result<Value,ActionFailure>{
    input("drag",authorized_delivery(drag_args(pid,window_id,from_x,from_y,to_x,to_y,screenshot_id),allow_foreground))
}

fn drag_args(pid: i64, window_id: i64, from_x: f64, from_y: f64, to_x: f64, to_y: f64, screenshot_id: Option<&str>) -> Value {
    let mut args = json!({ "pid": pid, "window_id": window_id, "from_x": from_x, "from_y": from_y, "to_x": to_x, "to_y": to_y });
    if let Some(id) = screenshot_id {
        args["screenshot_id"] = json!(id);
    }
    args
}

/// The capture id of an observation from `get_state` (or a capture's
/// structured result). Every platform's driver issues one per capture.
pub fn observed_screenshot_id(state: &Value) -> Option<&str> {
    state.get("screenshot_id").and_then(Value::as_str)
}

/// List installed + running apps (Codex `list_apps`) so a target that isn't open
/// yet can still be discovered and launched. Read-only.
pub fn list_apps() -> Result<Value, String> {
    read_call("list_apps", &json!({}))
}

/// Launch an app IN THE BACKGROUND (Codex `launch_app`, but focus-free. the
/// daemon does not front the target and re-demotes it if it self-activates). Give
/// a bundle id (`com.apple.calculator`) or a display name (`Calculator`). Returns
/// the launched app's pid + windows so a caller can drive it without a second
/// round-trip. This is distinct from `activate`, which intentionally foregrounds.
pub fn launch_app(name_or_bundle: &str) -> Result<Value, String> {
    let key = if name_or_bundle.contains('.') && !name_or_bundle.contains(' ') {
        "bundle_id"
    } else {
        "name"
    };
    call("launch_app", &json!({ key: name_or_bundle }))
}

/// A pixel click. `screenshot_id` names the capture the point was read from
/// (a live tile's displayed frame), so the driver refuses it with
/// `stale_capture` when the window changed size since that frame.
pub fn click(pid: i64, window_id: i64, x: f64, y: f64, button: Option<String>, screenshot_id: Option<&str>) -> Result<Value, String> {
    input("click", click_args(pid, window_id, x, y, button, screenshot_id)).map_err(|error| error.to_string())
}

fn click_args(pid: i64, window_id: i64, x: f64, y: f64, button: Option<String>, screenshot_id: Option<&str>) -> Value {
    let mut args = json!({ "pid": pid, "window_id": window_id, "x": x, "y": y, "button": button.unwrap_or_else(|| "left".into()) });
    if let Some(id) = screenshot_id {
        args["screenshot_id"] = json!(id);
    }
    args
}

pub fn click_pixel_with_delivery(pid:i64,window_id:i64,x:f64,y:f64,button:&str,count:i64,capture:&str,foreground:bool)->Result<Value,ActionFailure>{
    if !(1..=3).contains(&count)||!matches!(button,"left"|"right"|"middle")||capture.is_empty(){return Err(ActionFailure::refused("invalid_arguments", "click refused: invalid_arguments. valid button, count and screenshot identity required"))}
    let mut args=click_args(pid,window_id,x,y,Some(button.into()),Some(capture));args["count"]=json!(count);
    input("click",authorized_delivery(args,foreground))
}

pub fn type_text(pid:i64,window_id:i64,text:String)->Result<Value,String>{type_text_with_delivery(pid,window_id,text,false).map_err(|error| error.to_string())}
pub fn type_text_with_delivery(pid:i64,window_id:i64,text:String,allow_foreground:bool)->Result<Value,ActionFailure>{
    input("type_text",authorized_delivery(json!({"pid":pid,"window_id":window_id,"text":text}),allow_foreground))
}

pub fn press_key(pid:i64,window_id:i64,key:String)->Result<Value,String>{press_key_with_delivery(pid,window_id,key,false).map_err(|error| error.to_string())}

fn authorized_delivery(mut args:Value,allow_foreground:bool)->Value{
    if allow_foreground{args["delivery_mode"]=json!("foreground");}args
}

pub fn press_key_with_delivery(pid: i64, window_id: i64, key: String, allow_foreground:bool) -> Result<Value, ActionFailure> {
    if key.is_empty() { return Err(ActionFailure::refused("invalid_arguments", "key must not be empty")); }
    // The native press_key field names one key. Chords use the hotkey array;
    // sending a combined string can instead act on just its first character.
    if key != "+" && key.contains('+') {
        let modifier = |part: &str| match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "control_l" | "control_r" => Some("ctrl"),
            "shift" | "shift_l" | "shift_r" => Some("shift"),
            "alt" | "option" | "option_l" | "option_r" | "alt_l" | "alt_r" => Some("alt"),
            "cmd" | "cmd_l" | "cmd_r" | "command" | "command_l" | "command_r" | "meta" | "meta_l" | "meta_r" | "super" | "super_l" | "super_r"
                | "win" | "windows" | "os" | "lwin" | "rwin" => Some("cmd"),
            _ => None,
        };
        let parts: Vec<_> = key.split('+').map(str::trim).collect();
        let (last, mods) = parts.split_last().unwrap();
        if last.is_empty() || modifier(last).is_some() { return Err(ActionFailure::refused("invalid_arguments", "shortcut needs modifiers followed by one key")); }
        let mut keys: Vec<String> = mods.iter().map(|part| modifier(part).map(str::to_owned)
            .ok_or_else(|| ActionFailure::refused("invalid_arguments", format!("unsupported shortcut modifier {part:?}")))).collect::<Result<_, _>>()?;
        keys.push((*last).to_string());
        return input("hotkey", authorized_delivery(json!({ "pid": pid, "window_id": window_id, "keys": keys }),allow_foreground));
    }
    input("press_key", authorized_delivery(json!({ "pid": pid, "window_id": window_id, "key": key }),allow_foreground))
}

/// A pointer scroll at a point of a live tile. `screenshot_id` names the frame
/// the point was read from, as for [`click`]: without it the driver reads the
/// point against this session's latest capture of the window, which is not the
/// frame the tile is showing once a newer one has arrived.
pub fn scroll(pid: i64, window_id: i64, x: f64, y: f64, dy: i64, amount: i64, screenshot_id: Option<&str>) -> Result<Value, String> {
    scroll_with_point(pid, window_id, Some((x, y)), dy, amount, screenshot_id)
}

/// A direction-only model action uses the driver's focused-region route.
/// Logical window bounds are not a screenshot coordinate or a chosen target.
pub fn scroll_window(pid: i64, window_id: i64, dy: i64, amount: i64) -> Result<Value, String> {
    scroll_with_point(pid, window_id, None, dy, amount, None)
}

pub fn scroll_region(pid: i64, window_id: i64, dy: i64, amount: i64, by: &str) -> Result<Value, ActionFailure> {
    input("scroll", model_scroll_args(pid, window_id, None, dy, amount, by).map_err(|e| ActionFailure::refused("invalid_arguments", e))?)
}

pub fn scroll_token(pid: i64, window_id: i64, token: &str, dy: i64, amount: i64, by: &str) -> Result<Value, ActionFailure> {
    input("scroll", model_scroll_args(pid, window_id, Some(token), dy, amount, by).map_err(|e| ActionFailure::refused("invalid_arguments", e))?)
}

fn model_scroll_args(pid: i64, window_id: i64, token: Option<&str>, dy: i64, amount: i64, by: &str) -> Result<Value, String> {
    if dy == 0 || !(1..=50).contains(&amount) || !matches!(by,"line"|"page") {
        return Err("scroll needs non-zero dy, amount 1..50, and line or page units".into());
    }
    let mut args = json!({"pid":pid,"window_id":window_id,"direction":scroll_direction(dy),"amount":amount,"by":by});
    if let Some(token) = token {
        if token.is_empty() {return Err("scroll target token is empty".into());}
        args["element_token"] = json!(token);
    }
    Ok(args)
}

/// Driver direction for an app scroll delta. Negative scrolls content down,
/// the natural wheel direction the live tile sends (`scrollDelta` in interact.ts).
pub fn scroll_direction(dy: i64) -> &'static str {
    if dy < 0 { "down" } else { "up" }
}

fn scroll_with_point(pid: i64, window_id: i64, point: Option<(f64, f64)>, dy: i64, amount: i64, screenshot_id: Option<&str>) -> Result<Value, String> {
    if dy == 0 { return Err("scroll delta must be non-zero".into()); }
    if !(1..=50).contains(&amount) { return Err("scroll amount must be between 1 and 50".into()); }
    input("scroll", scroll_args(pid, window_id, point, dy, amount, screenshot_id)).map_err(|error| error.to_string())
}

fn scroll_args(pid: i64, window_id: i64, point: Option<(f64, f64)>, dy: i64, amount: i64, screenshot_id: Option<&str>) -> Value {
    let mut args = json!({ "pid": pid, "window_id": window_id,
        "direction": scroll_direction(dy), "by": "line", "amount": amount });
    if let Some((x, y)) = point {
        args["x"] = json!(x);
        args["y"] = json!(y);
        // An id binds a pixel; a direction-only scroll has none to bind.
        if let Some(id) = screenshot_id { args["screenshot_id"] = json!(id); }
    }
    args
}

/// Full window state for the agent to reason over: the AX element tree (indexed)
/// plus a fresh screenshot as a data URL. Works off-Space for observation.
/// Unix observations share the tile/input daemon session. A private output file
/// retains the prior target-window-only capture scope without a CLI subprocess.
/// Captures remain enabled for readiness even when model images are off.
pub fn get_state(pid: i64, window_id: i64, max_elements: i64) -> Result<Value, String> {
    let capture_error = if uses_host_owned_driver() {
        let grants = crate::permissions::io_status();
        preflight_io(&grants, true, false)?;
        if grants.supported && !grants.screen_recording {
            Some(json!({"code":"screen_recording_permission_required",
                "message":"Screen Recording access is missing. AX-only observation remains available; choose Grant access in Copilot to enable images."}))
        } else { capture_permission_denial() }
    } else { capture_permission_denial() };
    let capture = capture_error.is_none();
    let args = json!({
        "pid": pid,
        "window_id": window_id,
        "include_screenshot": capture,
        "include_accessibility_tree": true,
        // Match the live tile's budget; never change its coordinate basis.
        "max_dimension": 640,
        "max_elements": max_elements,
    });
    #[cfg(unix)]
    let result = transport::deliver(daemon_observation(&args), || cli_observation(args)).map_err(|error| error.to_string());
    #[cfg(not(unix))]
    let result = cli_observation(args);
    result.map(|mut state| {
        if !capture {
            if let Some(fields) = state.as_object_mut() {
                for key in ["screenshot_data_url","screenshot_png_b64","screenshot_id","screenshots","screenshot_file_path"] { fields.remove(key); }
                fields.insert("screenshot_frame_valid".into(),json!(false));
                fields.insert("screenshot_error".into(),capture_error.clone().expect("capture disabled only with a reason"));
            }
        }
        state
    })
}

#[cfg(unix)]
fn daemon_observation(args: &Value) -> Result<Value, transport::Error> {
    // Inline native captures also enumerate and capture auxiliary surfaces.
    // The established file-output contract captures only the requested window;
    // preserve that scope when moving observations from CLI to direct sockets.
    let capture = if args.get("include_screenshot").and_then(Value::as_bool) == Some(false) {
        None
    } else { Some(CaptureFile::new().map_err(transport::Error::Failed)?) };
    let mut request_args = args.clone();
    if let Some(file) = &capture {
        let _ = std::fs::remove_file(file.path());
        request_args["screenshot_out_file"] = json!(file.path().to_string_lossy());
    }
    ensure_cursor_hidden().map_err(transport::Error::Failed)?;
    let result = daemon_exchange(&json!({
        "method": "call", "name": "get_window_state", "args": request_args,
        "session_id": daemon_session(), "client_kind": "cli",
    }), transport::roundtrip_tool_result)?;
    let mut state = result.get("structuredContent").cloned().unwrap_or_else(|| result.clone());
    if session_ended(&state) {
        daemon_reset_session();
        return Err(transport::Error::Failed("daemon session ended; observation unavailable".into()));
    }
    if result.get("isError").and_then(Value::as_bool) == Some(true)
        || state.get("isError").and_then(Value::as_bool) == Some(true)
        || state.get("error").is_some_and(|error| !error.is_null() && error != &Value::Bool(false))
    {
        let code = state.get("code").and_then(Value::as_str).unwrap_or("observation_failed");
        let message = state.get("message").or_else(|| state.get("error")).and_then(Value::as_str)
            .unwrap_or("driver reported an observation error");
        return Err(transport::Error::Failed(format!("get_window_state refused ({code}): {}", message.chars().take(4096).collect::<String>())));
    }
    if let Some(file) = &capture { attach_observation_file(&mut state,&file.path()); }
    Ok(state)
}

/// Compatibility path for platforms without direct transport or a connection
/// failure before observation dispatch. Completed refusals are never retried.
fn cli_observation(mut args: Value) -> Result<Value, String> {
    // screenshot_out_file forces capture in the native contract, even when
    // include_screenshot=false. Never add it to a permitted AX-only read.
    if args.get("include_screenshot").and_then(Value::as_bool) == Some(false) {
        return call("get_window_state", &args);
    }
    let capture = CaptureFile::new()?;
    let path = capture.path();
    let _ = std::fs::remove_file(&path);
    args["screenshot_out_file"] = json!(path.to_string_lossy());
    let mut res = call("get_window_state", &args)?;
    attach_observation_file(&mut res,&path);
    Ok(res)
}

/// Only bytes from this request's private output path can become its image.
/// Failed captures and unrelated driver-reported paths never supply pixels.
fn attach_observation_file(state: &mut Value, path: &std::path::Path) {
    let refused = state.get("isError").and_then(Value::as_bool) == Some(true)
        || state.get("screenshot_frame_valid").and_then(Value::as_bool) == Some(false)
        || state.get("screenshot_error").is_some_and(|error|!error.is_null())
        || state.get("error").is_some_and(|error|!error.is_null() && error != &Value::Bool(false));
    let matching_path = match state.get("screenshot_file_path") {
        None | Some(Value::Null) => true,
        Some(Value::String(reported)) => std::path::Path::new(reported) == path,
        _ => false,
    };
    let mime = state.get("screenshot_mime_type").and_then(Value::as_str).unwrap_or("image/png").to_string();
    if let Some(fields) = state.as_object_mut() {
        // File-output observations do not consume inline or auxiliary pixels.
        for key in ["screenshot_data_url","screenshot_png_b64","screenshots"] { fields.remove(key); }
        if !refused && matching_path {
            if let Ok(bytes) = std::fs::read(path) {
                if !bytes.is_empty() { fields.insert("screenshot_data_url".into(),json!(data_url(&mime,&bytes))); }
            }
        }
    }
}

#[cfg(all(test, unix))]
#[path = "observation_tests.rs"]
mod observation_tests;

/// Whole-computer view: capture the full display (get_desktop_state has no
/// downscale, so shrink the PNG in process to keep the feed light).
pub fn desktop_shot(max_dim: i64) -> Result<String, String> {
    let bytes = desktop_shot_bytes(max_dim)?;
    Ok(data_url("image/png", &bytes))
}

pub fn desktop_shot_bytes(max_dim: i64) -> Result<Vec<u8>, String> {
    let capture = CaptureFile::new()?;
    let path = capture.path();
    let path_s = path.to_string_lossy().to_string();
    let _ = std::fs::remove_file(&path);
    let res = call("get_desktop_state", &json!({ "screenshot_out_file": path_s }))?;
    if let Some(r) = refusal_reason(&res) {
        return Err(r);
    }
    let file = res
        .get("screenshot_file_path")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
        .unwrap_or(path);
    let bytes = std::fs::read(&file).map_err(|e| format!("read desktop shot: {e}"))?;
    if bytes.is_empty() {
        return Err("empty desktop capture".into());
    }
    // Downscale in process instead of spawning `sips` on the temp file.
    #[cfg(target_os = "macos")]
    let bytes = downscale_png(bytes, max_dim);
    #[cfg(not(target_os = "macos"))]
    let _ = max_dim;
    Ok(bytes)
}

fn refusal_reason(res: &Value) -> Option<String> {
    // get_window_state reports background_input.routes[].status == "refused"
    let routes = res.get("background_input")?.get("routes")?.as_array()?;
    let all_refused = !routes.is_empty()
        && routes
            .iter()
            .all(|r| r.get("status").and_then(Value::as_str) == Some("refused"));
    if all_refused {
        let reason = routes
            .first()
            .and_then(|r| r.get("reason"))
            .and_then(Value::as_str)
            .unwrap_or("refused");
        Some(format!("driver refused: {reason} (bring the window to the current Space)"))
    } else {
        None
    }
}

fn inline_screenshot(res: &Value) -> Option<String> {
    let shots = res.get("screenshots")?.as_array()?;
    // highest z-index = topmost surface for this window
    let best = shots.iter().max_by_key(|s| s.get("z_index").and_then(Value::as_i64).unwrap_or(0))?;
    for k in ["url", "data_url"] {
        if let Some(u) = best.get(k).and_then(Value::as_str) {
            return Some(u.to_string());
        }
    }
    for k in ["data", "base64", "png_base64"] {
        if let Some(b) = best.get(k).and_then(Value::as_str) {
            return Some(format!("data:image/png;base64,{b}"));
        }
    }
    None
}

/// Own a private capture directory for exactly one in-flight request.
struct CaptureFile(std::path::PathBuf);
impl CaptureFile {
    fn new() -> Result<Self, String> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let dir = std::env::temp_dir().join(format!("icu-capture-{}-{}-{}",
                std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(), NEXT.fetch_add(1, Ordering::Relaxed)));
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            { use std::os::unix::fs::DirBuilderExt; builder.mode(0o700); }
            match builder.create(&dir) {
                Ok(()) => return Ok(Self(dir)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("capture directory: {e}")),
            }
        }
        Err("could not allocate capture directory".into())
    }
    fn path(&self) -> std::path::PathBuf { self.0.join("frame.png") }
}
impl Drop for CaptureFile {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:{mime};base64,{b64}")
}

/// The fixture drivers are Python scripts run through `/usr/bin/python3`. On a Mac that is the
/// Xcode command line tools shim, and its first start after a reboot (an empty xcrun cache in
/// the per-boot temp dir) takes 2 s idle and over 5 s under load, as long as the product's
/// startup deadline. Every test that starts a fixture driver calls this first, so one start
/// pays that cost outside any deadline and the rest find the cache warm.
#[cfg(all(test, unix))]
pub(crate) fn warm_python_shim() {
    static WARM: std::sync::Once = std::sync::Once::new();
    WARM.call_once(|| { let _ = Command::new("/usr/bin/python3").args(["-c", "pass"]).output(); });
}

#[cfg(all(test, unix))]
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;

#[cfg(all(test, unix))]
mod lifetime_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fixture() -> (CaptureFile, String, String) {
        warm_python_shim();
        let directory = CaptureFile::new().unwrap();
        let binary = directory.0.join("lifetime-driver");
        std::fs::write(&binary, include_str!("../../tests/fixtures/fake_lifetime_driver.py")).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = directory.0.join("lifetime.sock");
        (directory, binary.to_string_lossy().into(), socket.to_string_lossy().into())
    }

    #[test]
    fn owned_driver_opts_out_of_native_overlay_rendering() {
        let (directory, binary, socket) = fixture();
        let mut process = DriverProcess::default();
        assert!(process.ensure(&binary, &socket), "fixture driver did not start");
        process.shutdown();
        let startup: Value = serde_json::from_slice(&std::fs::read(directory.0.join("startup.json")).unwrap()).unwrap();
        let args = startup["argv"].as_array().unwrap();
        assert!(args.iter().any(|arg| arg == "--no-overlay"),
            "per-session hiding is revived by input; owned app driver must opt out at startup: {args:?}");
        assert_eq!(args.last().and_then(Value::as_str), Some(socket.as_str()));
    }

    #[test]
    fn app_key_policy_uses_the_target_driver_platform() {
        if let Ok(platform) = std::env::var("ICU_KEY_POLICY_CHILD") {
            let command = press_key(42, 7, "Cmd+c".into());
            let control = press_key(42, 7, "Ctrl+c".into());
            let single = press_key(42, 7, "tab".into());
            let approved = press_key_with_delivery(42,7,"Cmd+s".into(),true);
            shutdown_owned_driver();
            assert_eq!(approved.is_ok(), matches!(platform.as_str(),"linux"|"darwin"), "foreground permission must not override an unknown platform or Windows key policy: {approved:?}");
            assert_eq!(command.is_ok(), platform == "linux",
                "{platform}: app key policy result {command:?}");
            assert_eq!(control.is_ok(), matches!(platform.as_str(), "linux" | "win32"), "{control:?}");
            assert_eq!(single.is_ok(), matches!(platform.as_str(), "linux" | "win32"), "{single:?}");
            if !matches!(platform.as_str(), "linux" | "win32") {
                assert!(control.unwrap_err().contains("background_unavailable"));
                assert!(single.unwrap_err().contains("background_unavailable"));
            }
            return;
        }
        for platform in ["win32", "darwin", "macos", "linux", "missing", "error"] {
            let (directory, binary, socket) = fixture();
            use cua_driver_contract::{CONTRACT_VERSION, TOOLS_LIST_SCHEMA_VERSION, CAPABILITY_VERSION, MCP_PROTOCOL_VERSION};
            std::fs::write(directory.0.join("metadata.json"), json!({
                "driver_version":"fixture", "contract_version":CONTRACT_VERSION,
                "tools_list_schema_version":TOOLS_LIST_SCHEMA_VERSION,
                "capability_version":CAPABILITY_VERSION,"mcp_protocol_version":MCP_PROTOCOL_VERSION,
            }).to_string()).unwrap();
            let health = match platform {
                "missing" => json!({}),
                "error" => json!({"isError":true,"content":[{"type":"text","text":"fixture health unavailable"}]}),
                _ => json!({"platform":platform}),
            };
            std::fs::write(directory.0.join("health.json"), health.to_string()).unwrap();
            let result = crate::child_test_output(Command::new(std::env::current_exe().unwrap())
                .args(["cua::lifetime_tests::app_key_policy_uses_the_target_driver_platform", "--exact", "--nocapture"])
                .env("ICU_KEY_POLICY_CHILD", platform).env("ICU_CUA_BIN", binary)
                .env("ICU_TEST_OWN_SOCKET", socket).env("ICU_OWN_DRIVER", "1")
                .env_remove("ICU_CUA_SOCKET"), Duration::from_secs(15)).unwrap();
            let rows: Vec<Value> = std::fs::read_to_string(directory.0.join("requests.jsonl")).unwrap_or_default()
                .lines().map(|line| serde_json::from_str(line).unwrap()).collect();
            assert!(result.status.success(), "{platform}: {}\n{}\nrequests: {rows:?}",
                String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
            let keys: Vec<_> = rows.iter().filter(|r| matches!(r["name"].as_str(), Some("press_key" | "hotkey"))).collect();
            let expected = match platform {
                "linux" => vec![json!(["cmd", "c"]), json!(["ctrl", "c"]),json!(["cmd","s"])],
                "darwin"=>vec![json!(["cmd","s"])],
                "win32" => vec![json!(["ctrl", "c"])],
                _ => vec![],
            };
            assert_eq!(keys.iter().filter(|r| r["name"] == "hotkey").map(|r| r["args"]["keys"].clone()).collect::<Vec<_>>(), expected);
            let singles: Vec<_> = keys.iter().filter(|r| r["name"] == "press_key").collect();
            assert_eq!(singles.len(), usize::from(matches!(platform, "linux" | "win32")));
            assert!(singles.iter().all(|r| r["args"]["key"] == "tab"));
            assert!(keys.iter().all(|r|r["args"]["pid"]==42&&r["args"]["window_id"]==7));
            assert_eq!(keys.iter().filter(|r|r["args"]["delivery_mode"]=="foreground").count(),usize::from(matches!(platform,"linux"|"darwin")));
            assert!(keys.iter().all(|r| r.get("transport").is_none()), "keys unexpectedly reached CLI replay: {keys:?}");
        }
    }

    #[test]
    fn app_chords_use_the_native_hotkey_shape() {
        if std::env::var_os("ICU_CHORD_SHAPE_CHILD").is_some() {
            let chord = press_key(42, 7, "Ctrl+Shift+Tab".into());
            let single = press_key(42, 7, "return".into());
            let plus = press_key(42, 7, "+".into());
            let left_command = press_key(42, 7, "Command_L+a".into());
            let right_command = press_key(42, 7, "Command_R+a".into());
            for invalid in ["", "Ctrl+", "Ctrl++", "Ctrl+Shift", "a+b"] {
                assert!(press_key(42, 7, invalid.into()).is_err(), "ambiguous chord was sent: {invalid:?}");
            }
            shutdown_owned_driver();
            assert!(chord.is_ok() && single.is_ok() && plus.is_ok() && left_command.is_ok() && right_command.is_ok(), "chord={chord:?}, single={single:?}, plus={plus:?}, left={left_command:?}, right={right_command:?}");
            return;
        }
        let (directory, binary, socket) = fixture();
        // Exercise wire translation on a target that supports background keys.
        std::fs::write(directory.0.join("health.json"), json!({"platform":"linux"}).to_string()).unwrap();
        use cua_driver_contract::{CONTRACT_VERSION, TOOLS_LIST_SCHEMA_VERSION, CAPABILITY_VERSION, MCP_PROTOCOL_VERSION};
        std::fs::write(directory.0.join("metadata.json"), json!({
            "driver_version":"fixture", "contract_version":CONTRACT_VERSION,
            "tools_list_schema_version":TOOLS_LIST_SCHEMA_VERSION,
            "capability_version":CAPABILITY_VERSION,"mcp_protocol_version":MCP_PROTOCOL_VERSION,
        }).to_string()).unwrap();
        let result = crate::child_test_output(Command::new(std::env::current_exe().unwrap())
            .args(["cua::lifetime_tests::app_chords_use_the_native_hotkey_shape", "--exact", "--nocapture"])
            .env("ICU_CHORD_SHAPE_CHILD", "1").env("ICU_CUA_BIN", binary)
            .env("ICU_TEST_OWN_SOCKET", socket).env("ICU_OWN_DRIVER", "1")
            .env_remove("ICU_CUA_SOCKET"), Duration::from_secs(15)).unwrap();
        assert!(result.status.success(), "{}\n{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
        let rows: Vec<Value> = std::fs::read_to_string(directory.0.join("requests.jsonl")).unwrap()
            .lines().map(|line| serde_json::from_str(line).unwrap())
            .filter(|r: &Value| matches!(r["name"].as_str(), Some("press_key" | "hotkey"))).collect();
        assert_eq!(rows.len(), 5, "{rows:?}");
        assert_eq!(rows[0]["name"], "hotkey", "a chord reached the single-key native API: {rows:?}");
        assert_eq!(rows[0]["args"]["keys"], json!(["ctrl", "shift", "Tab"]));
        assert_eq!(rows[1]["name"], "press_key");
        assert_eq!(rows[1]["args"]["key"], "return");
        assert_eq!(rows[2]["name"], "press_key");
        assert_eq!(rows[2]["args"]["key"], "+");
        assert_eq!(rows[3]["name"], "hotkey");
        assert_eq!(rows[3]["args"]["keys"],json!(["cmd","a"]));
        assert_eq!(rows[4]["args"]["keys"],json!(["cmd","a"]));
        for row in &rows {
            assert_eq!(row["args"]["pid"], 42);
            assert_eq!(row["args"]["window_id"], 7);
            assert!(row.get("transport").is_none());
        }
    }

    #[test]
    fn shutdown_is_terminal_even_when_late_workers_request_a_driver() {
        let (_directory, binary, socket) = fixture();
        let mut process = DriverProcess::default();
        assert!(process.ensure(&binary, &socket));
        process.shutdown();
        // Do not let a stale socket make this guard pass accidentally.
        let _ = std::fs::remove_file(&socket);
        assert!(!process.ensure(&binary, &socket), "a late worker relaunched the driver after shutdown");
        assert!(process.child.is_none());
    }

    #[test]
    fn drop_signals_parent_lifetime_and_waits_for_owned_cleanup() {
        let (_directory, binary, socket) = fixture();
        let mut process = DriverProcess::default();
        assert!(process.ensure(&binary, &socket));
        let pid = process.child.as_ref().unwrap().id();
        drop(process);
        assert!(std::path::Path::new(&socket).with_extension("cleaned").exists(), "owned cleanup was killed instead of drained");
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
        assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }

    #[test]
    fn shutdown_before_start_never_launches_a_child() {
        let (_directory, binary, socket) = fixture();
        let mut process = DriverProcess::default();
        process.shutdown();
        assert!(!process.ensure(&binary, &socket));
        assert!(!std::path::Path::new(&socket).exists());
    }

    #[test]
    fn kernel_peer_identity_must_match_the_spawned_child() {
        let directory = CaptureFile::new().unwrap();
        let path = directory.0.join("peer.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let path = path.to_str().unwrap();
        assert!(OwnedEndpoint::capture(path, std::process::id()).is_some());
        assert!(OwnedEndpoint::capture(path, std::process::id() + 1).is_none());
        drop(listener);
    }

    #[test]
    fn shutdown_preserves_a_replacement_endpoint() {
        let (_directory, binary, socket) = fixture();
        let mut process = DriverProcess::default();
        assert!(process.ensure(&binary, &socket));
        std::fs::remove_file(&socket).unwrap();
        let replacement = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        assert!(!process.ensure(&binary, &socket), "replacement listener was adopted");
        process.shutdown();
        assert!(std::path::Path::new(&socket).exists(), "replacement endpoint was deleted");
        drop(replacement);
    }

    #[test]
    fn ignored_lifetime_signal_has_a_bounded_kill_fallback() {
        let child = Command::new("/usr/bin/python3").args(["-c", "import time; time.sleep(60)"])
            .stdin(std::process::Stdio::piped()).spawn().unwrap();
        let mut process = DriverProcess::default();
        process.child = Some(child);
        let start = Instant::now();
        assert!(process.stop_child(Duration::from_millis(50)));
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(process.child.is_none());
    }

    #[test]
    fn first_input_after_restart_reinitializes_session_and_cursor() {
        if std::env::var_os("ICU_RECOVERY_CURSOR_CHILD").is_none() {
            let (directory, binary, socket) = fixture();
            use cua_driver_contract::{CONTRACT_VERSION, TOOLS_LIST_SCHEMA_VERSION, CAPABILITY_VERSION, MCP_PROTOCOL_VERSION};
            std::fs::write(directory.0.join("metadata.json"), json!({
                "driver_version":"fixture", "contract_version":CONTRACT_VERSION,
                "tools_list_schema_version":TOOLS_LIST_SCHEMA_VERSION,
                "capability_version":CAPABILITY_VERSION,"mcp_protocol_version":MCP_PROTOCOL_VERSION,
            }).to_string()).unwrap();
            let result = crate::child_test_output(Command::new(std::env::current_exe().unwrap())
                .args(["cua::lifetime_tests::first_input_after_restart_reinitializes_session_and_cursor","--exact","--nocapture"])
                .env("ICU_RECOVERY_CURSOR_CHILD","1").env("ICU_CUA_BIN",binary)
                .env("ICU_TEST_OWN_SOCKET",socket)
                .env("ICU_OWN_DRIVER","1").env_remove("ICU_CUA_SOCKET"), Duration::from_secs(10)).unwrap();
            assert!(result.status.success(), "{}\n{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
            let rows: Vec<Value> = std::fs::read_to_string(directory.0.join("requests.jsonl")).unwrap()
                .lines().map(|line| serde_json::from_str(line).unwrap()).collect();
            assert_eq!(rows.iter().filter(|row| row["name"] == "click").count(), 2);
            assert_eq!(rows.iter().filter(|row| row["name"] == "set_agent_cursor_enabled").count(), 2);
            return;
        }
        let first = input("click",json!({"pid":42,"window_id":7,"x":1,"y":1}));
        assert!(first.is_ok(), "first input failed at {}: {first:?}", own_socket());
        {
            let mut process = PROCESS.get().unwrap().lock().unwrap();
            let child = process.child.as_mut().unwrap();
            child.kill().unwrap(); child.wait().unwrap();
        }
        let second = input("click",json!({"pid":42,"window_id":7,"x":1,"y":1}));
        shutdown_owned_driver();
        let _ = std::fs::remove_file(std::path::Path::new(&own_socket()).with_extension("cleaned"));
        assert!(second.is_ok(), "restarted driver lost session/cursor initialization: {second:?}");
        assert!(input("click",json!({"pid":42,"window_id":7,"x":1,"y":1})).is_err());
        assert!(PROCESS.get().unwrap().lock().unwrap().child.is_none());
    }

    #[test]
    fn scroll_uses_native_direction_contract() {
        if std::env::var_os("ICU_SCROLL_PROTOCOL_CHILD").is_none() {
            let (directory, binary, socket) = fixture();
            use cua_driver_contract::{CONTRACT_VERSION, TOOLS_LIST_SCHEMA_VERSION, CAPABILITY_VERSION, MCP_PROTOCOL_VERSION};
            std::fs::write(directory.0.join("metadata.json"), json!({
                "driver_version":"fixture", "contract_version":CONTRACT_VERSION,
                "tools_list_schema_version":TOOLS_LIST_SCHEMA_VERSION,
                "capability_version":CAPABILITY_VERSION,"mcp_protocol_version":MCP_PROTOCOL_VERSION,
            }).to_string()).unwrap();
            let result = crate::child_test_output(Command::new(std::env::current_exe().unwrap())
                .args(["cua::lifetime_tests::scroll_uses_native_direction_contract","--exact","--nocapture"])
                .env("ICU_SCROLL_PROTOCOL_CHILD","1").env("ICU_CUA_BIN",binary)
                .env("ICU_TEST_OWN_SOCKET",socket)
                .env("ICU_OWN_DRIVER","1").env_remove("ICU_CUA_SOCKET"), Duration::from_secs(15)).unwrap();
            let rows: Vec<Value> = std::fs::read_to_string(directory.0.join("requests.jsonl")).unwrap_or_default()
                .lines().map(|line| serde_json::from_str(line).unwrap()).filter(|r: &Value| r["name"] == "scroll").collect();
            assert!(result.status.success(), "{}\n{}\nfixture: {}\nrequests: {rows:?}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr), directory.0.display());
            assert_eq!(rows.len(), 2, "invalid deltas/amounts must not reach the driver");
            for (row, direction) in rows.iter().zip(["down", "up"]) {
                assert_eq!(row["args"]["direction"], direction);
                assert_eq!(row["args"]["by"], "line");
                assert_eq!(row["args"]["amount"], 3);
                assert_eq!(row["args"]["x"], 12.0);
                assert_eq!(row["args"]["y"], 34.0);
                assert!(row["args"].get("scroll_y").is_none());
            }
            return;
        }
        let down = scroll(42, 7, 12.0, 34.0, -3, 3, None);
        let up = scroll(42, 7, 12.0, 34.0, 3, 3, None);
        let zero = scroll(42, 7, 12.0, 34.0, 0, 3, None);
        let no_ticks = scroll(42, 7, 12.0, 34.0, -3, 0, None);
        let excessive = scroll(42, 7, 12.0, 34.0, -3, 51, None);
        shutdown_owned_driver();
        assert!(down.is_ok(), "downward scroll refused: {down:?}");
        assert!(up.is_ok(), "upward scroll refused: {up:?}");
        assert!(zero.is_err() && no_ticks.is_err() && excessive.is_err());
    }

    #[test]
    fn agent_and_preview_captures_use_the_same_resolution() {
        if std::env::var_os("ICU_CAPTURE_BUDGET_CHILD").is_none() {
            let (directory, binary, socket) = fixture();
            use cua_driver_contract::{CONTRACT_VERSION, TOOLS_LIST_SCHEMA_VERSION, CAPABILITY_VERSION, MCP_PROTOCOL_VERSION};
            std::fs::write(directory.0.join("metadata.json"), json!({
                "driver_version":"fixture", "contract_version":CONTRACT_VERSION,
                "tools_list_schema_version":TOOLS_LIST_SCHEMA_VERSION,
                "capability_version":CAPABILITY_VERSION,"mcp_protocol_version":MCP_PROTOCOL_VERSION,
            }).to_string()).unwrap();
            let result = crate::child_test_output(Command::new(std::env::current_exe().unwrap())
                .args(["cua::lifetime_tests::agent_and_preview_captures_use_the_same_resolution","--exact","--nocapture"])
                .env("ICU_CAPTURE_BUDGET_CHILD","1").env("ICU_CUA_BIN",binary)
                .env("ICU_TEST_OWN_SOCKET",socket)
                .env("ICU_OWN_DRIVER","1").env_remove("ICU_CUA_SOCKET"), Duration::from_secs(15)).unwrap();
            assert!(result.status.success(), "{}\n{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
            let rows: Vec<Value> = std::fs::read_to_string(directory.0.join("requests.jsonl")).unwrap()
                .lines().map(|line| serde_json::from_str(line).unwrap()).filter(|r: &Value| r["name"] == "get_window_state").collect();
            assert_eq!(rows.len(), 3, "two previews and one agent observation must reach the fixture");
            assert!(rows.iter().all(|row|row["args"]["max_dimension"]==640),
                "agent and preview images must keep the same 640-pixel coordinate basis: {rows:?}");
            assert!(rows.iter().all(|row|row["transport"].is_null()),
                "all three captures must use the daemon transport without CLI fallback: {rows:?}");
            assert!(rows[0]["args"].get("screenshot_out_file").is_none());
            assert!(rows[2]["args"].get("screenshot_out_file").is_none());
            let output=std::path::Path::new(rows[1]["args"]["screenshot_out_file"].as_str()
                .expect("agent observation must retain the target-only file-output contract"));
            let captured:Vec<Value>=std::fs::read_to_string(directory.0.join("capture-outputs.jsonl")).unwrap()
                .lines().map(|line|serde_json::from_str(line).unwrap()).collect();
            assert_eq!(captured.len(),1,"only the agent capture uses a private output file");
            assert_eq!(captured[0]["path"],output.to_string_lossy().as_ref());
            assert_eq!(captured[0]["parent_mode"],0o700,"capture directory must be private at dispatch");
            assert_eq!(captured[0]["existed_before"],false,"the request must not reuse stale file bytes");
            assert!(captured[0]["bytes"].as_u64().unwrap()>0,"native fixture must write the requested frame");
            assert!(!output.exists() && !output.parent().unwrap().exists(),"capture file and private directory must be removed after the observation returns");
            return;
        }
        let before = window_shot_bytes(42, 7, 640);
        let observation = get_state(42, 7, 200);
        let after = window_shot_bytes(42, 7, 640);
        shutdown_owned_driver();
        assert!(!before.unwrap().0.is_empty());
        assert!(observation.unwrap()["screenshot_data_url"].as_str().unwrap().starts_with("data:image/png;base64,"));
        assert!(!after.unwrap().0.is_empty());
    }

    #[test]
    fn typed_capture_refusal_reaches_the_frame_without_a_cli_retry() {
        if std::env::var_os("ICU_CAPTURE_REFUSAL_CHILD").is_none() {
            let (directory, binary, socket) = fixture();
            use cua_driver_contract::{CONTRACT_VERSION, TOOLS_LIST_SCHEMA_VERSION, CAPABILITY_VERSION, MCP_PROTOCOL_VERSION};
            std::fs::write(directory.0.join("metadata.json"), json!({
                "driver_version":"fixture", "contract_version":CONTRACT_VERSION,
                "tools_list_schema_version":TOOLS_LIST_SCHEMA_VERSION,
                "capability_version":CAPABILITY_VERSION,"mcp_protocol_version":MCP_PROTOCOL_VERSION,
            }).to_string()).unwrap();
            // The shape the Linux driver returns for a minimized window on the
            // capture-only path: isError, no image, typed screenshot_error.
            std::fs::write(directory.0.join("capture-refusal.json"), json!({
                "isError": true,
                "content": [{"type":"text","text":"No content produced for window_id 7: ... (window_minimized: window 7 is minimized)"}],
                "structuredContent": {"window_id":7,"pid":42,"screenshot_frame_valid":false,
                    "screenshot_error":{"code":"window_minimized","window_id":7,"message":"window 7 is minimized",
                        "detail":"all Linux window capture backends failed ... BadMatch"}}
            }).to_string()).unwrap();
            let result = crate::child_test_output(Command::new(std::env::current_exe().unwrap())
                .args(["cua::lifetime_tests::typed_capture_refusal_reaches_the_frame_without_a_cli_retry","--exact","--nocapture"])
                .env("ICU_CAPTURE_REFUSAL_CHILD","1").env("ICU_CUA_BIN",binary)
                .env("ICU_TEST_OWN_SOCKET",socket)
                .env("ICU_OWN_DRIVER","1").env_remove("ICU_CUA_SOCKET"), Duration::from_secs(15)).unwrap();
            assert!(result.status.success(), "{}\n{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
            let rows: Vec<Value> = std::fs::read_to_string(directory.0.join("requests.jsonl")).unwrap()
                .lines().map(|line| serde_json::from_str(line).unwrap()).filter(|r: &Value| r["name"] == "get_window_state").collect();
            assert_eq!(rows.len(), 1, "a typed refusal must not be retried through the CLI: {rows:?}");
            assert_eq!(rows[0]["transport"], Value::Null);
            return;
        }
        let refused = window_shot_bytes(42, 7, 640).expect_err("a minimized window has no frame");
        shutdown_owned_driver();
        assert!(refused.starts_with(CAPTURE_REFUSED), "{refused}");
        assert!(refused.contains("minimized") && refused.contains("(window_minimized)"), "{refused}");
        assert!(!refused.contains("BadMatch"), "backend dump must not reach the tile: {refused}");
    }

    #[test]
    fn a_coded_answer_with_a_delivery_effect_stays_typed() {
        assert!(coded_answer(&json!({"ok":true})).is_none());
        let refusal = coded_answer(&json!({"code":"background_unavailable","message":"needs the window in front"})).unwrap();
        assert!(refusal.is_refused());
        assert_eq!(refusal.code(), Some("background_unavailable"));
        let partial = coded_answer(&json!({"code":"cancelled","effect":"partial","message":"stopped mid-drag","delivery":{"delivered_count":3}})).unwrap();
        assert!(!partial.is_refused());
        assert_eq!(partial.effect(), Some("partial"));
        assert_eq!(partial.evidence()["delivery"]["delivered_count"], 3);
    }

    #[test]
    fn tool_error_inside_successful_transport_stops_the_batch_and_reaches_the_model() {
        if std::env::var_os("ICU_TOOL_ERROR_CHILD").is_none() {
            // A code-less error and a partial delivery end the run; a typed
            // refusal whose effect is `none` reaches the model.
            for (structured, expect) in [
                (None, "failed"),
                (Some(json!({"exit_code":1})), "failed"),
                (Some(json!({"code":"type_text_incomplete","effect":"partial","delivered_chars":5})), "failed"),
                (Some(json!({"code":"stale_capture","effect":"none"})), "refused"),
                (Some(json!({"status":"refused","refusal":{"code":"stale_capture","message":"native click was rejected"}})), "refused"),
                (Some(json!({"code":"cancelled","effect":"none"})), "refused"),
                (Some(json!({"status":"uncertain","effect":"unverifiable","code":"action_outcome_unknown","verified":false})), "failed"),
                (Some(json!({"status":"refused","refusal":{"code":"menu_action_failed"}})), "failed"),
                (Some(json!({"code":"unknown_new_error","refusal":{"code":"stale_capture","effect":"none"}})), "failed"),
                (Some(json!({"code":"stale_capture","effect":"none","delivery":{"mode":"background","delivered_count":2}})), "failed"),
                (Some(json!({"code":"stale_capture","effect":"partial","refusal":{"status":"refused","effect":"none"}})), "failed"),
            ] {
                let (directory, binary, socket) = fixture();
                use cua_driver_contract::{CONTRACT_VERSION, TOOLS_LIST_SCHEMA_VERSION, CAPABILITY_VERSION, MCP_PROTOCOL_VERSION};
                std::fs::write(directory.0.join("metadata.json"), json!({
                    "driver_version":"fixture", "contract_version":CONTRACT_VERSION,
                    "tools_list_schema_version":TOOLS_LIST_SCHEMA_VERSION,
                    "capability_version":CAPABILITY_VERSION,"mcp_protocol_version":MCP_PROTOCOL_VERSION,
                }).to_string()).unwrap();
                let mut error = json!({"isError":true,"content":[{"type":"text","text":"native click was rejected"}]});
                if let Some(structured) = &structured { error["structuredContent"] = structured.clone(); }
                std::fs::write(directory.0.join("click-error.json"), error.to_string()).unwrap();
                let result = crate::child_test_output(Command::new(std::env::current_exe().unwrap())
                    .args(["cua::lifetime_tests::tool_error_inside_successful_transport_stops_the_batch_and_reaches_the_model","--exact","--nocapture"])
                    .env("ICU_TOOL_ERROR_CHILD","1").env("ICU_TOOL_ERROR_EXPECT",expect).env("ICU_TOOL_ERROR_PAYLOAD",serde_json::to_string(&structured).unwrap()).env("ICU_CUA_BIN",binary)
                    .env("ICU_TEST_OWN_SOCKET",socket)
                    .env("ICU_OWN_DRIVER","1").env_remove("ICU_CUA_SOCKET"), Duration::from_secs(15)).unwrap();
                let rows: Vec<Value> = std::fs::read_to_string(directory.0.join("requests.jsonl")).unwrap_or_default()
                    .lines().map(|line| serde_json::from_str(line).unwrap()).collect();
                assert_eq!(rows.iter().filter(|row| row["name"] == "click").count(), 1,
                    "guard did not reach the failing click or replayed it: {rows:?}");
                let received = std::fs::read_to_string(directory.0.join("received-text.txt")).unwrap_or_default();
                assert!(result.status.success(), "structured={structured:?}; receiver={received:?}; requests={rows:?}\n{}\n{}",
                    String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
                assert!(received.is_empty(), "failed click was followed by typing into an unknown field");
                assert!(!rows.iter().any(|row| row["name"] == "type_text"));
            }
            return;
        }
        let refused = std::env::var("ICU_TOOL_ERROR_EXPECT").unwrap() == "refused";
        struct BatchBrain(std::sync::atomic::AtomicUsize);
        impl crate::brain::Brain for BatchBrain {
            fn name(&self) -> String { "fixture".into() }
            fn decide(&self, _: &str, _memory: Option<&str>, _: &str, _: &[String]) -> Result<crate::brain::Decision, String> {
                use crate::brain::{Action, Decision};
                let actions = if self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0 {
                    vec![Action::Click { target:Some("Editor".into()),element_index:-1,button:None,count:None,action:None },
                        Action::Type { text:"must not arrive".into() }]
                } else { vec![Action::Done { summary:"finished".into() }] };
                Ok(Decision { thought:String::new(),action:None,actions, fallback: None })
            }
        }
        let brain = BatchBrain(std::sync::atomic::AtomicUsize::new(0));
        let events = Mutex::new(Vec::new());
        crate::agent::run_loop("click Editor, then type",42,7,"fixture",&brain,&|kind,data| {
            events.lock().unwrap().push((kind.to_string(),data));
        });
        shutdown_owned_driver();
        let events = events.into_inner().unwrap();
        let actions: Vec<_> = events.iter().filter(|(kind,_)| kind == "act").collect();
        assert_eq!(actions[0].1["ok"], false, "native failure was reported as success: {events:?}");
        assert!(actions[0].1["error"].as_str().unwrap().contains("native click was rejected"));
        let payload: Value = serde_json::from_str(&std::env::var("ICU_TOOL_ERROR_PAYLOAD").unwrap()).unwrap();
        if let Some(code) = payload.get("code").or_else(||payload.pointer("/refusal/code")) {
            assert_eq!(&actions[0].1["code"], code);
            assert_eq!(&actions[0].1["outcome"]["code"], code);
        }
        for key in ["effect", "status", "delivery", "delivered_chars", "verified"] {
            if let Some(value) = payload.get(key) { assert_eq!(&actions[0].1["outcome"][key], value, "lost {key}"); }
        }
        if refused {
            // The driver refused before sending anything, so the model gets the
            // answer with a fresh view instead of the run ending.
            assert_eq!(actions.len(), 2, "tool failure did not stop the batch: {events:?}");
            assert_eq!(actions[0].1["status"], "refused");
            assert_eq!(actions[1].1["status"], "not_run", "{events:?}");
            assert!(!events.iter().any(|(kind,_)| kind == "error"), "{events:?}");
            assert_eq!(events.last().unwrap().0, "done");
            assert_eq!(brain.0.load(std::sync::atomic::Ordering::Relaxed), 2);
        } else {
            // Delivery unknown or partial: the run ends and nothing is replayed.
            assert_eq!(actions.len(), 1, "a failed step was followed by another: {events:?}");
            assert_eq!(actions[0].1["status"], "failed");
            assert_eq!(events.last().unwrap().0, "error", "{events:?}");
            assert_eq!(brain.0.load(std::sync::atomic::Ordering::Relaxed), 1);
        }
    }

    #[test]
    fn model_scroll_does_not_invent_a_pixel_target() {
        if std::env::var_os("ICU_MODEL_SCROLL_CHILD").is_none() {
            let (directory, binary, socket) = fixture();
            use cua_driver_contract::{CONTRACT_VERSION, TOOLS_LIST_SCHEMA_VERSION, CAPABILITY_VERSION, MCP_PROTOCOL_VERSION};
            std::fs::write(directory.0.join("metadata.json"), json!({
                "driver_version":"fixture", "contract_version":CONTRACT_VERSION,
                "tools_list_schema_version":TOOLS_LIST_SCHEMA_VERSION,
                "capability_version":CAPABILITY_VERSION,"mcp_protocol_version":MCP_PROTOCOL_VERSION,
            }).to_string()).unwrap();
            let result = crate::child_test_output(Command::new(std::env::current_exe().unwrap())
                .args(["cua::lifetime_tests::model_scroll_does_not_invent_a_pixel_target","--exact","--nocapture"])
                .env("ICU_MODEL_SCROLL_CHILD","1").env("ICU_CUA_BIN",binary)
                .env("ICU_TEST_OWN_SOCKET",socket)
                .env("ICU_OWN_DRIVER","1").env_remove("ICU_CUA_SOCKET"), Duration::from_secs(15)).unwrap();
            assert!(result.status.success(), "{}\n{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
            let rows: Vec<Value> = std::fs::read_to_string(directory.0.join("requests.jsonl")).unwrap()
                .lines().map(|line| serde_json::from_str(line).unwrap()).collect();
            let scrolls: Vec<_> = rows.iter().filter(|r| r["name"] == "scroll").collect();
            assert_eq!(scrolls.len(), 1);
            let args = &scrolls[0]["args"];
            assert!(args.get("x").is_none() && args.get("y").is_none(), "model invented a pixel target: {args}");
            assert_eq!(args["direction"], "down");
            assert_eq!(args["pid"], 42);
            assert_eq!(args["window_id"], 7);
            let reads: Vec<_> = rows.iter().filter(|r| r["name"] == "get_window_state").collect();
            assert!((2..=4).contains(&reads.len()) && reads.iter().all(|r| r["transport"].is_null()), "readiness may poll privately: {reads:?}");
            return;
        }
        struct ScrollBrain(std::sync::atomic::AtomicBool);
        impl crate::brain::Brain for ScrollBrain {
            fn name(&self) -> String { "fixture".into() }
            fn decide(&self, _: &str, _memory: Option<&str>, _: &str, _: &[String]) -> Result<crate::brain::Decision, String> {
                let action = if self.0.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    crate::brain::Action::Done { summary:"finished".into() }
                } else { crate::brain::Action::Scroll { dy:-3, target:None, element_index:-1, amount:3, by:None } };
                Ok(crate::brain::Decision { thought:String::new(), action:Some(action), actions:vec![], fallback: None })
            }
        }
        let brain = ScrollBrain(std::sync::atomic::AtomicBool::new(false));
        let done = std::sync::atomic::AtomicBool::new(false);
        let observed = std::sync::atomic::AtomicUsize::new(0);
        crate::agent::run_loop("scroll the focused region down",42,7,"fixture",&brain,&|kind,_| {
            if kind == "done" { done.store(true,std::sync::atomic::Ordering::Relaxed); }
            if kind == "observe" { observed.fetch_add(1,std::sync::atomic::Ordering::Relaxed); }
        });
        shutdown_owned_driver();
        assert!(done.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(observed.load(std::sync::atomic::Ordering::Relaxed), 2);
    }    #[test]
    fn model_scroll_binds_the_selected_element_and_page_units() {
        if std::env::var_os("ICU_MODEL_TARGET_SCROLL_CHILD").is_none() {
            let (directory, binary, socket) = fixture();
            use cua_driver_contract::{CONTRACT_VERSION, TOOLS_LIST_SCHEMA_VERSION, CAPABILITY_VERSION, MCP_PROTOCOL_VERSION};
            std::fs::write(directory.0.join("metadata.json"), json!({
                "driver_version":"fixture", "contract_version":CONTRACT_VERSION,
                "tools_list_schema_version":TOOLS_LIST_SCHEMA_VERSION,
                "capability_version":CAPABILITY_VERSION,"mcp_protocol_version":MCP_PROTOCOL_VERSION,
            }).to_string()).unwrap();
            let result = crate::child_test_output(Command::new(std::env::current_exe().unwrap())
                .args(["cua::lifetime_tests::model_scroll_binds_the_selected_element_and_page_units","--exact","--nocapture"])
                .env("ICU_MODEL_TARGET_SCROLL_CHILD","1").env("ICU_CUA_BIN",binary)
                .env("ICU_TEST_OWN_SOCKET",socket)
                .env("ICU_OWN_DRIVER","1").env_remove("ICU_CUA_SOCKET"), Duration::from_secs(15)).unwrap();
            assert!(result.status.success(), "{}\n{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
            let rows: Vec<Value> = std::fs::read_to_string(directory.0.join("requests.jsonl")).unwrap()
                .lines().map(|line| serde_json::from_str(line).unwrap()).collect();
            let scrolls: Vec<_> = rows.iter().filter(|r| r["name"] == "scroll").collect();
            assert_eq!(scrolls.len(), 1);
            let args = &scrolls[0]["args"];
            assert!(args.get("x").is_none() && args.get("y").is_none(), "model invented a pixel target: {args}");
            assert_eq!(args["direction"], "down");
            assert_eq!(args["element_token"],"fixture-token");assert_eq!(args["by"],"page");assert_eq!(args["amount"],2);
            assert_eq!(args["pid"], 42);
            assert_eq!(args["window_id"], 7);
            let reads: Vec<_> = rows.iter().filter(|r| r["name"] == "get_window_state").collect();
            assert!((2..=4).contains(&reads.len()) && reads.iter().all(|r| r["transport"].is_null()), "readiness may poll privately: {reads:?}");
            return;
        }
        struct ScrollBrain(std::sync::atomic::AtomicBool);
        impl crate::brain::Brain for ScrollBrain {
            fn name(&self) -> String { "fixture".into() }
            fn decide(&self, _: &str, _memory: Option<&str>, _: &str, _: &[String]) -> Result<crate::brain::Decision, String> {
                let action = if self.0.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    crate::brain::Action::Done { summary:"finished".into() }
                } else { crate::brain::Action::Scroll { dy:-3, target:Some("Editor".into()), element_index:-1, amount:2, by:Some("page".into()) } };
                Ok(crate::brain::Decision { thought:String::new(), action:Some(action), actions:vec![], fallback: None })
            }
        }
        let brain = ScrollBrain(std::sync::atomic::AtomicBool::new(false));
        let done = std::sync::atomic::AtomicBool::new(false);
        let observed = std::sync::atomic::AtomicUsize::new(0);
        crate::agent::run_loop("scroll the focused region down",42,7,"fixture",&brain,&|kind,_| {
            if kind == "done" { done.store(true,std::sync::atomic::Ordering::Relaxed); }
            if kind == "observe" { observed.fetch_add(1,std::sync::atomic::Ordering::Relaxed); }
        });
        shutdown_owned_driver();
        assert!(done.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(observed.load(std::sync::atomic::Ordering::Relaxed), 2);
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn cli_action_envelope_retains_errors_and_never_replays_ambiguous_output() {
        if let Ok(expect) = std::env::var("ICU_CLI_ENVELOPE_CHILD") {
            let result = transport::deliver(Err(transport::Error::Unavailable("fixture has no socket".into())),
                || call_action("click", &json!({"pid":42,"window_id":7})));
            match expect.as_str() {
                "success" => assert_eq!(result.unwrap()["ok"], true),
                "refused" => {
                    let error = result.unwrap_err();
                    assert!(error.is_refused());
                    assert_eq!(error.code(), Some("stale_capture"));
                }
                "partial" => {
                    let error = result.unwrap_err();
                    assert!(!error.is_refused());
                    assert_eq!(error.effect(), Some("partial"));
                    assert_eq!(error.evidence()["delivery"]["delivered_count"], 2);
                }
                _ => assert!(!result.unwrap_err().is_refused()),
            }
            return;
        }
        let scratch = CaptureFile::new().unwrap();
        let binary = scratch.0.join("fake-cli");
        let calls = scratch.0.join("calls");
        std::fs::write(&binary, r#"#!/bin/sh
printf '%s\n' "$CUA_CLI_RESULT_ENVELOPE" >> "$ICU_CLI_FIXTURE_CALLS"
printf '%s\n' "$ICU_CLI_FIXTURE_REPLY"
"#).unwrap();
        std::fs::set_permissions(&binary, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
        let tool_error = |payload: Value| json!({"cua_cli_result_envelope":1,"result":{
            "isError":true,"structuredContent":payload,"content":[{"type":"text","text":"native failure"}]}});
        for (reply, expect) in [
            (json!({"cua_cli_result_envelope":1,"result":{"content":[],"structuredContent":{"ok":true}}}), "success"),
            (tool_error(json!({"code":"stale_capture","effect":"none"})), "refused"),
            (tool_error(json!({"code":"cancelled","effect":"partial","delivery":{"delivered_count":2}})), "partial"),
            (json!({"code":"stale_capture","effect":"none"}), "unknown"),
            (json!({"isError":true,"content":[],"structuredContent":{"code":"stale_capture","effect":"none"}}), "unknown"),
            (json!({"cua_cli_result_envelope":2,"result":{"content":[],"structuredContent":{"ok":true}}}), "unknown"),
        ] {
            std::fs::write(&calls, "").unwrap();
            let out = crate::child_test_output(Command::new(std::env::current_exe().unwrap())
                .args(["cua::tests::cli_action_envelope_retains_errors_and_never_replays_ambiguous_output", "--exact", "--nocapture"])
                .env("ICU_CLI_ENVELOPE_CHILD", expect).env("ICU_CUA_BIN", &binary)
                .env("ICU_CUA_SOCKET", scratch.0.join("unused.sock"))
                .env("ICU_CLI_FIXTURE_REPLY", reply.to_string()).env("ICU_CLI_FIXTURE_CALLS", &calls), Duration::from_secs(15)).unwrap();
            assert!(out.status.success(), "{reply}: {}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            assert_eq!(std::fs::read_to_string(&calls).unwrap(), "1\n", "CLI was replayed or envelope was not requested: {reply}");
        }
    }

    /// The Windows replay's failed `set_value` (the driver CLI's exact stdout, exit 0).
    const SET_VALUE_REFUSAL: &str = "set_value: element [0] does not implement ValuePattern or RangeValuePattern. For controls with TogglePattern (CheckBox) or SelectionItemPattern (RadioButton / ComboBoxItem), use the `click` tool instead.";

    #[test]
    fn a_cli_text_error_is_the_drivers_refusal() {
        if std::env::var_os("ICU_CLI_TEXT_CHILD").is_some() {
            let err = call("set_value", &json!({"pid": 42, "window_id": 7, "element_token": "0", "value": "7"})).unwrap_err();
            println!("CLI_TEXT {err}");
            return;
        }
        let dir = std::env::temp_dir().join(format!("icu-cli-text-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("cua-driver");
        std::fs::write(&bin, format!("#!/bin/sh\ncat <<'EOF'\n{SET_VALUE_REFUSAL}\nEOF\n")).unwrap();
        std::fs::set_permissions(&bin, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let out = crate::child_test_output(Command::new(std::env::current_exe().unwrap())
            .args(["cua::tests::a_cli_text_error_is_the_drivers_refusal", "--exact", "--nocapture"])
            .env("ICU_CLI_TEXT_CHILD", "1").env("ICU_CUA_BIN", &bin)
            .env("ICU_CUA_SOCKET", dir.join("unused.sock")).env_remove("ICU_OWN_DRIVER"), Duration::from_secs(15)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{stdout}\n{}", String::from_utf8_lossy(&out.stderr));
        let line = stdout.lines().find_map(|l| l.strip_prefix("CLI_TEXT ")).unwrap_or_else(|| panic!("no result: {stdout}"));
        assert_eq!(line, format!("set_value refused: {}", SET_VALUE_REFUSAL.strip_prefix("set_value: ").unwrap()));
    }

    const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

    #[test]
    fn concurrent_capture_files_are_private_and_self_cleaning() {
        let first = CaptureFile::new().unwrap();
        let second = CaptureFile::new().unwrap();
        let a = first.path();
        let b = second.path();
        assert_ne!(a, b);
        std::fs::write(&a, b"first").unwrap();
        std::fs::write(&b, b"second").unwrap();
        drop(first);
        assert!(!a.exists());
        assert_eq!(std::fs::read(&b).unwrap(), b"second");
        drop(second);
        assert!(!b.exists());
    }

    #[test]
    fn owned_driver_recovers_from_failed_start_and_later_exit() {
        use std::os::unix::fs::PermissionsExt;
        super::warm_python_shim();
        let scratch = CaptureFile::new().unwrap();
        let bin = scratch.0.join("fake-driver");
        let sock = scratch.0.join("fake.sock");
        let bin_s = bin.to_str().unwrap();
        let sock_s = sock.to_str().unwrap();
        std::fs::write(&bin, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut process = DriverProcess::default();
        assert!(!process.ensure(bin_s, sock_s));
        assert!(process.child.is_none());
        // This fixture only accepts and closes local sockets. It has no desktop API.
        std::fs::write(&bin, include_str!("../../tests/fixtures/fake_lifetime_driver.py")).unwrap();
        process.retry_after = None;
        assert!(process.ensure(bin_s, sock_s));
        let first_pid = process.child.as_ref().unwrap().id();
        process.child.as_mut().unwrap().kill().unwrap();
        process.child.as_mut().unwrap().wait().unwrap();
        assert!(process.ensure(bin_s, sock_s));
        assert_ne!(process.child.as_ref().unwrap().id(), first_pid);
        let owned_pid = process.child.as_ref().unwrap().id();
        drop(process);
        // The fixture child has been killed and reaped, not left running after
        // the owning app exits. This does not address any external daemon.
        assert_eq!(unsafe { libc::kill(owned_pid as i32, 0) }, -1);
        assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }

    #[test]
    fn app_socket_is_distinct_from_the_legacy_shared_endpoint() {
        let socket = own_socket();
        assert!(socket.ends_with(&format!("driver-{}.sock", std::process::id())));
        assert!(!socket.ends_with("/driver.sock"));
    }

    #[test]
    fn display_count_enumerates_at_least_the_main_display() {
        assert!(display_count() >= 1, "must see at least the main display");
    }

    #[test]
    #[ignore = "requires explicit desktop capture authorization"]
    fn per_display_capture_produces_a_valid_png() {
        // Exercises the exact per-display capture path (screencapture -D<n> ->
        // in-process downscale -> PNG bytes) that a SECONDARY monitor uses. Only the display index
        // differs, so verifying display 1 (which always exists) exercises the code
        // a "Display N" tile runs.
        let bytes = display_shot_bytes(1, 400).expect("display 1 capture should succeed");
        assert!(bytes.len() > 200, "capture should be a non-trivial image");
        assert_eq!(bytes[..8], PNG_MAGIC, "output must be a valid PNG");
    }

    #[test]
    fn out_of_range_display_errors_without_panicking() {
        // The branch a request for a non-existent display hits (e.g. asking for a
        // secondary monitor that isn't attached). Must be a graceful Err.
        assert!(display_shot_bytes(99, 400).is_err(), "invalid display index must error");
    }

    #[test]
    fn downscale_bounds_the_longest_edge_in_process() {
        // The replacement for the per-frame `sips` subprocess: a tile asks for a
        // 200px frame, a bigger capture comes back bounded to 200 on its longest
        // edge, re-encoded as a valid PNG, via the pure in-process path.
        let mut big = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::new(800, 600))
            .write_to(&mut big, image::ImageFormat::Png).unwrap();
        let shrunk = downscale_png(big.into_inner(), 200);
        assert_eq!(shrunk[..8], PNG_MAGIC, "downscaled output is still a PNG");
        let decoded = image::load_from_memory(&shrunk).unwrap();
        assert_eq!(decoded.width().max(decoded.height()), 200, "longest edge bounded to max_dim");
        assert_eq!((decoded.width(), decoded.height()), (200, 150), "aspect ratio preserved");

        // An already-small capture is returned untouched (no needless re-encode).
        let mut small = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::new(120, 90))
            .write_to(&mut small, image::ImageFormat::Png).unwrap();
        let original = small.into_inner();
        assert_eq!(downscale_png(original.clone(), 200), original, "small image passes through unchanged");
    }

    #[test]
    fn driver_in_dir_finds_the_bundled_sidecar() {
        // The exact resolution the built Copilot.app relies on: our driver bundled
        // next to the app executable as Contents/MacOS/cua-driver. Deterministic -
        // no dependency on where the test binary itself lives.
        let dir = std::env::temp_dir().join(format!("icu-drv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // Empty dir: nothing to find.
        assert_eq!(driver_in_dir(&dir), None, "empty dir must resolve to None");

        let exact = dir.join("cua-driver");
        std::fs::create_dir(&exact).unwrap();
        assert_eq!(driver_in_dir(&dir), None, "a directory is not a sidecar");
        std::fs::remove_dir(&exact).unwrap();
        std::fs::write(&exact, b"x").unwrap();
        assert_eq!(driver_in_dir(&dir), None, "non-executable file must be rejected");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exact, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(driver_in_dir(&dir), Some(exact.to_string_lossy().into_owned()));
        std::fs::remove_file(&exact).unwrap();
        for name in ["cua-driver-uia", "cua-driver-aarch64-apple-darwin.dSYM", "cua-driver-x86_64-unknown-linux-gnu"] {
            let path = dir.join(name);
            std::fs::write(&path, b"x").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        assert_eq!(driver_in_dir(&dir), None, "helpers and other architectures must not be selected");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    #[ignore = "requires a real secondary display and explicit capture authorization"]
    fn secondary_display_capture_when_present() {
        // Directly exercises display_shot_bytes(2) against a real SECONDARY monitor
        // when one is attached (ICU_TEST_SECONDARY_DISPLAY=1 is set by the harness
        // that brings up a second display). Skips cleanly on a single-monitor box.
        if display_count() < 2 || std::env::var("ICU_TEST_SECONDARY_DISPLAY").is_err() {
            eprintln!("secondary_display_capture: skipped (need a 2nd display + ICU_TEST_SECONDARY_DISPLAY=1)");
            return;
        }
        let bytes = display_shot_bytes(2, 500).expect("display 2 capture should succeed");
        assert!(bytes.len() > 200, "secondary capture should be a non-trivial image");
        assert_eq!(bytes[..8], PNG_MAGIC, "secondary capture must be a valid PNG");
    }
}


#[cfg(test)]
mod window_projection_tests {
    #[test]
    fn history_keeps_our_front_window_and_uses_native_z_order(){
        let value=serde_json::json!({"windows":[
            {"pid":44,"window_id":7,"bundle_id":"fixture.behind","is_on_screen":true,"on_current_space":true,"layer":0,"z_index":1},
            {"pid":std::process::id(),"window_id":8,"bundle_id":"com.nathanzhao.app","is_on_screen":true,"on_current_space":true,"layer":0,"z_index":3},
            {"pid":45,"window_id":9,"bundle_id":"fixture.overlay","is_on_screen":true,"on_current_space":true,"layer":4,"z_index":4}
        ]});
        let projected=super::project_history_windows(&value).unwrap();assert_eq!(projected["windows"].as_array().unwrap().len(),2);assert_eq!(projected["windows"][0]["bundle_id"],"com.nathanzhao.app");
    }
    use super::*;
    #[test]
    fn desktop_tile_label_matches_the_captured_extent() {
        // Linux get_desktop_state grabs the whole X root (every RandR monitor);
        // study/audit/displays-2026-09-25 shows a 3200x1080 two-monitor frame.
        let (name, subtitle) = super::desktop_label();
        if cfg!(target_os = "linux") {
            assert_eq!(name, "All displays");
            assert!(!subtitle.contains("primary"), "{subtitle}");
        } else {
            assert_eq!((name, subtitle), ("Main display", "primary screen"));
        }
    }

    #[test]
    fn captured_linux_windows_are_offered_without_macos_layer_metadata() {
        let capture: Value = serde_json::from_str(include_str!("../../tests/fixtures/linux-windows.json")).unwrap();
        let projected = project_windows(&capture["response"], capture["app_pid"].as_i64().unwrap()).unwrap();
        let windows = projected["windows"].as_array().unwrap();
        assert_eq!(windows.len(), 2, "calculator and guard must survive; the app itself must not");
        assert!(windows.iter().any(|w| w["title"] == "ICU Calculator"));
        assert!(windows.iter().all(|w| w["pid"] != capture["app_pid"]));
        assert!(windows.iter().all(|w| w.get("on_current_space").is_none()), "unknown workspace is not fabricated");
    }
    #[test]
    fn macos_noncontent_layers_and_invalid_targets_stay_excluded() {
        let window = |pid: Value, id: Value, layer: Value| json!({"pid":pid,"window_id":id,"layer":layer,"app_name":"Copilot","bounds":{"x":-500,"y":20,"width":640,"height":480}});
        let value = json!({"windows":[window(json!(22),json!(1),json!(0)),window(json!(22),json!(2),json!(101)),window(Value::Null,json!(3),json!(0)),window(json!(22),Value::Null,json!(0)),window(json!(99),json!(4),json!(0))]});
        let projected = project_windows(&value, 99).unwrap();
        assert_eq!(projected["windows"].as_array().unwrap().len(), 1);
        assert_eq!(projected["windows"][0]["window_id"], 1, "another app with the same name is valid");
    }
    #[test]
    fn untitled_off_screen_windows_stay_out_of_the_picker() {
        let window = |id: i64, title: Value, on_screen: Value| json!({"pid":22,"window_id":id,"layer":0,"app_name":"AutoFill","title":title,"is_on_screen":on_screen,"bounds":{"x":0,"y":0,"width":640,"height":480}});
        let value = json!({"windows":[
            window(1, json!(""), json!(false)), window(2, Value::Null, json!(false)), window(3, json!("  "), json!(false)),
            window(4, json!(""), json!(true)), window(5, json!("Report"), json!(false)), window(6, Value::Null, Value::Null)]});
        let ids: Vec<i64> = project_windows(&value, 99).unwrap()["windows"].as_array().unwrap().iter().map(|w| w["window_id"].as_i64().unwrap()).collect();
        let mut ids = ids; ids.sort();
        assert_eq!(ids, vec![4, 5, 6], "only untitled windows known to be off screen are dropped");
    }
    #[test]
    fn foreground_authority_changes_only_delivery_and_defaults_off(){
        let original=json!({"pid":12,"window_id":34,"key":"Return"});
        assert_eq!(authorized_delivery(original.clone(),false),original);
        let allowed=authorized_delivery(original,true);
        assert_eq!(allowed["delivery_mode"],"foreground");assert_eq!(allowed["pid"],12);assert_eq!(allowed["window_id"],34);
        assert!(!crate::memory::RunOptions::default().allow_foreground);
        let checkpoint=crate::task_progress::Checkpoint::new("run-focus","Save the note","Editor",None);
        assert!(!serde_json::to_string(&checkpoint).unwrap().contains("foreground"));
    }
    #[test]
    fn compact_real_windows_are_selectable_but_empty_surfaces_are_not(){
        let window=|id:i64,width:f64,height:f64|json!({"pid":22,"window_id":id,"layer":0,"app_name":"Calculator","title":"Calculator","is_on_screen":true,"bounds":{"x":100,"y":100,"width":width,"height":height}});
        let projected=project_windows(&json!({"windows":[window(1,198.,350.),window(2,96.,80.),window(3,0.,350.),window(4,198.,0.)]}),99).unwrap();
        let mut ids=projected["windows"].as_array().unwrap().iter().map(|w|w["window_id"].as_i64().unwrap()).collect::<Vec<_>>();ids.sort();assert_eq!(ids,vec![1,2]);
    }
    #[test]
    fn capture_refusals_map_driver_codes_to_tile_text() {
        let refusal = |code: &str| capture_refusal(&json!({"screenshot_error":{"code":code}}));
        for code in ["window_minimized", "window_not_viewable", "window_offscreen", "surface_identity_unproven", "capture_failed", "new_code"] {
            let text = refusal(code).unwrap();
            assert!(text.starts_with(CAPTURE_REFUSED) && text.contains(code), "{text}");
        }
        assert!(capture_refusal(&json!({"window_id":7})).is_none());
    }
    #[test]
    fn wayland_window_omission_reaches_the_picker() {
        let coverage = json!({"complete":false,"code":"native_wayland_windows_not_listed","listed":"xwayland_only","message":"m"});
        let projected = project_windows(&json!({"windows":[],"window_coverage":coverage}), 99).unwrap();
        assert_eq!(projected["window_coverage"]["code"], "native_wayland_windows_not_listed");
        assert!(project_windows(&json!({"windows":[]}), 99).unwrap().get("window_coverage").is_none());
    }
    #[test]
    fn missing_window_array_is_not_an_empty_success() {
        assert!(project_windows(&json!({"code":"permissions_not_granted"}), 99).is_err());
    }
}

#[cfg(test)]
mod screenshot_binding_tests {
    use super::{click_args, daemon_frame, drag_args, observed_screenshot_id, scroll_args};
    use serde_json::json;

    /// The tile's wheel gesture names the frame its point was read from, as its
    /// click does, so a newer capture of the window cannot rescale it. A scroll
    /// with no point (the model's direction-only scroll) has no pixel to bind.
    #[test]
    fn a_tile_scroll_names_the_frame_it_was_read_from() {
        let args = scroll_args(42, 7, Some((160.0, 100.0)), -3, 3, Some("f0000abcd-3"));
        assert_eq!(args["screenshot_id"], "f0000abcd-3");
        assert_eq!((args["x"].as_f64(), args["y"].as_f64()), (Some(160.0), Some(100.0)));
        assert_eq!(args["direction"], "down");
        assert!(scroll_args(42, 7, Some((1.0, 1.0)), 3, 3, None).get("screenshot_id").is_none());
        assert!(scroll_args(42, 7, None, 3, 3, Some("f0000abcd-3")).get("screenshot_id").is_none());
    }

    #[test]
    fn a_drag_names_the_capture_its_points_came_from() {
        let state = json!({ "screenshot_id": "w0000abcd-7", "screenshot_width": 336 });
        let args = drag_args(42, 7, 1.0, 2.0, 3.0, 4.0, observed_screenshot_id(&state));
        assert_eq!(args["screenshot_id"], "w0000abcd-7");
        assert_eq!(args["from_x"], 1.0);
    }

    #[test]
    fn an_observation_without_an_id_sends_none() {
        let args = drag_args(42, 7, 1.0, 2.0, 3.0, 4.0, observed_screenshot_id(&json!({})));
        assert!(args.get("screenshot_id").is_none());
    }

    #[test]
    fn a_tile_click_names_the_frame_it_was_read_from() {
        let args = click_args(42, 7, 160.0, 100.0, None, Some("f0000abcd-3"));
        assert_eq!(args["screenshot_id"], "f0000abcd-3");
        assert_eq!(args["button"], "left");
        assert!(click_args(42, 7, 1.0, 1.0, None, None).get("screenshot_id").is_none());
    }

    #[test]
    fn a_daemon_frame_carries_its_capture_id() {
        let png = [0x89u8, b'P', b'N', b'G'];
        use base64::Engine;
        let result = json!({
            "content": [{"type":"image","mimeType":"image/png","data": base64::engine::general_purpose::STANDARD.encode(png)}],
            "structuredContent": {"screenshot_id": "f0000abcd-9", "screenshot_width": 640}
        });
        let (bytes, mime, id) = daemon_frame(&result).unwrap();
        assert_eq!((bytes.as_slice(), mime.as_str(), id.as_deref()), (&png[..], "image/png", Some("f0000abcd-9")));
        let mut without = result.clone();
        without["structuredContent"].as_object_mut().unwrap().remove("screenshot_id");
        assert_eq!(daemon_frame(&without).unwrap().2, None);
    }
}
