//! Linux native identity and separately approved AT-SPI content. Unknown session
//! state is a refusal, including in containers without a real logind session.
use crate::linux_atspi::{self as ax, Atspi};
#[path = "linux_browser.rs"]
pub(crate) mod browser_native;
#[path = "linux_kwin.rs"]
mod kwin;
pub use crate::linux_atspi::{EventKind, Events, ObjectKey, PendingEvent};
use crate::{
    capture::{Content, Source},
    policy::{browser, Context, Website},
    store::CaptureMode,
    Error, Result,
};
use serde::Deserialize;
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};
use x11rb::{
    connection::{Connection, RequestConnection},
    protocol::xproto::{AtomEnum, ConnectionExt, MapState},
    rust_connection::RustConnection,
};

#[path = "linux_locker.rs"]
mod locker;

fn refusal(code: &str, message: &str) -> Error {
    Error::new(code, message)
}
pub fn is_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|s| s == "wayland")
}
fn stable_application(exe: &str, args: &[&str], cwd: &std::path::Path) -> Option<String> {
    let exe = exe.strip_suffix(" (deleted)").unwrap_or(exe);
    let name = std::path::Path::new(exe).file_name()?.to_str()?;
    let host = if name.starts_with("python") {
        Some("python")
    } else if matches!(name, "node" | "nodejs" | "java" | "electron") {
        Some(name)
    } else {
        None
    };
    let identity = if let Some(host) = host {
        // Never use a shared interpreter as an exclusion-rule identity. Unsupported
        // invocation forms refuse; argv is read from the verified process, not X properties.
        let mut args = args.iter().skip(1);
        let mut entry = None;
        let mut classpath = cwd.display().to_string();
        while let Some(arg) = args.next() {
            if matches!(*arg, "-m" | "--module") {
                let module = args.next()?;
                entry = Some(if browser(module) || terminal_application(module) {
                    (*module).into()
                } else {
                    format!("{host}:module:{module}")
                });
                break;
            }
            if matches!(*arg, "-jar") {
                let file = args.next()?;
                entry = Some(format!("{host}:{}", cwd.join(file).display()));
                break;
            }
            if matches!(*arg, "-cp" | "-classpath" | "--class-path") {
                classpath = args
                    .next()?
                    .split(':')
                    .map(|p| cwd.join(p).display().to_string())
                    .collect::<Vec<_>>()
                    .join(":");
                continue;
            }
            if matches!(*arg, "-c" | "-e" | "--eval") {
                return None;
            }
            if arg.starts_with('-') {
                continue;
            }
            let base = std::path::Path::new(arg).file_name()?.to_str()?;
            entry = Some(if browser(base) || terminal_application(base) {
                base.to_owned()
            } else if host == "java" {
                format!("java:class:{arg}@{classpath}")
            } else {
                format!("{host}:{}", cwd.join(arg).display())
            });
            break;
        }
        entry?
    } else {
        name.to_owned()
    };
    (!identity.is_empty() && identity.len() <= 512 && !identity.chars().any(char::is_control))
        .then_some(identity)
}
/// The live process identity used to scope Linux History, task memory and
/// reviewed workflows. Shared interpreters stay scoped to their entry point.
/// Refuse PID reuse across the lookup instead of using a UI title.
pub fn application_identity(pid: u32) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let process = std::path::PathBuf::from(format!("/proc/{pid}"));
    let owner = std::fs::metadata(&process).ok()?.uid();
    if pid == 0 || owner != unsafe { libc::geteuid() } {
        return None;
    }
    let start = process_start(pid)?;
    let identity = application(pid)?;
    (process_start(pid).as_ref() == Some(&start)
        && std::fs::metadata(process).ok()?.uid() == owner).then_some(identity)
}
fn application(pid: u32) -> Option<String> {
    let path = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    if raw.len() > 65536 {
        return None;
    }
    let args: Vec<_> = std::str::from_utf8(&raw)
        .ok()?
        .split('\0')
        .filter(|s| !s.is_empty())
        .collect();
    let cwd = std::fs::read_link(format!("/proc/{pid}/cwd")).ok()?;
    stable_application(path.to_str()?, &args, &cwd)
}
fn process_start(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    Some(stat.rsplit_once(')')?.1.split_whitespace().nth(19)?.into())
}
pub fn terminal_application(name: &str) -> bool {
    matches!(
        name.strip_suffix(" (deleted)")
            .unwrap_or(name)
            .to_ascii_lowercase()
            .as_str(),
        "gnome-terminal-server"
            | "gnome-console"
            | "kgx"
            | "konsole"
            | "xterm"
            | "uxterm"
            | "xfce4-terminal"
            | "mate-terminal"
            | "tilix"
            | "terminator"
            | "alacritty"
            | "kitty"
            | "foot"
            | "footclient"
            | "wezterm-gui"
            | "qterminal"
            | "lxterminal"
            | "st"
            | "urxvt"
            | "urxvtd"
            | "ptyxis"
            | "cosmic-term"
    )
}
fn session_allowed(
    mine: Option<&str>,
    target: Option<&str>,
    active: Option<bool>,
    locked: Option<bool>,
    lockers: Option<bool>,
) -> bool {
    mine.is_some_and(|s| !s.is_empty() && target == Some(s))
        && active == Some(true)
        && locked == Some(false)
        && lockers == Some(false)
}
const LOCKERS: &[(&str, &str)] = &[
    (
        "org.freedesktop.ScreenSaver",
        "/org/freedesktop/ScreenSaver",
    ),
    ("org.gnome.ScreenSaver", "/org/gnome/ScreenSaver"),
    ("org.mate.ScreenSaver", "/org/mate/ScreenSaver"),
    ("org.cinnamon.ScreenSaver", "/org/cinnamon/ScreenSaver"),
    ("org.xfce.ScreenSaver", "/org/xfce/ScreenSaver"),
];
#[derive(Clone)]
struct SessionGate {
    system: zbus::Connection,
    session: zbus::Connection,
    locker_scan: std::sync::Arc<std::sync::Mutex<Option<(Instant, Vec<u32>)>>>,
}
impl SessionGate {
    async fn new() -> Result<Self> {
        Ok(Self {
            system: ax::call(zbus::Connection::system()).await?,
            session: ax::call(zbus::Connection::session()).await?,
            locker_scan: Default::default(),
        })
    }
    async fn check(&self, pid: u32) -> Result<()> {
        let manager = ax::call(zbus::Proxy::new(
            &self.system,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        ))
        .await?;
        let mine = locker::process_session(&manager, std::process::id()).await?;
        let target = locker::process_session(&manager, pid).await?;
        let session = ax::call(
            zbus::proxy::Builder::<zbus::Proxy>::new(&self.system)
                .destination("org.freedesktop.login1")
                .unwrap()
                .path(mine.clone())
                .unwrap()
                .interface("org.freedesktop.login1.Session")
                .unwrap()
                .cache_properties(zbus::proxy::CacheProperties::No)
                .build(),
        )
        .await?;
        let active = ax::call(session.get_property::<bool>("Active")).await?;
        let locked = ax::call(session.get_property::<bool>("LockedHint")).await?;
        // Logind remains the primary gate, before any secondary locker query.
        if !session_allowed(
            Some(mine.as_str()),
            Some(target.as_str()),
            Some(active),
            Some(locked),
            Some(false),
        ) {
            return Err(refusal(
                "computerHistorySessionRefused",
                "History requires the same active, unlocked logind session",
            ));
        }
        let mut lockers = locker::active(&self.session, &manager, &mine).await?;
        // Bare lockers do not all update logind or expose a bus interface.
        // Refuse while any known locker process exists in our session.
        let locker_pids = {
            let mut cache = self.locker_scan.lock().map_err(|_| ax::unavailable())?;
            if cache
                .as_ref()
                .is_none_or(|(at, _)| at.elapsed() >= Duration::from_millis(250))
            {
                let mut pids = Vec::new();
                for entry in std::fs::read_dir("/proc")
                    .map_err(|_| ax::unavailable())?
                    .flatten()
                {
                    let Some(pid) = entry
                        .file_name()
                        .to_str()
                        .and_then(|s| s.parse::<u32>().ok())
                    else {
                        continue;
                    };
                    let comm =
                        std::fs::read_to_string(entry.path().join("comm")).unwrap_or_default();
                    if matches!(
                        comm.trim(),
                        "i3lock"
                            | "slock"
                            | "xsecurelock"
                            | "swaylock"
                            | "gtklock"
                            | "waylock"
                            | "hyprlock"
                    ) {
                        pids.push(pid);
                    }
                }
                *cache = Some((Instant::now(), pids));
            }
            cache.as_ref().unwrap().1.clone()
        };
        for locker_pid in locker_pids {
            let owner: Result<zbus::zvariant::OwnedObjectPath> =
                ax::call(manager.call("GetSessionByPID", &(locker_pid,))).await;
            if owner.as_ref().map_or(true, |s| s == &mine) {
                lockers = true;
            }
        }
        if !session_allowed(
            Some(mine.as_str()),
            Some(target.as_str()),
            Some(active),
            Some(locked),
            Some(lockers),
        ) {
            return Err(refusal(
                "computerHistorySessionRefused",
                "History requires the same active, unlocked logind session with no active locker",
            ));
        }
        Ok(())
    }
}
fn atom(c: &RustConnection, name: &[u8]) -> Option<u32> {
    c.intern_atom(true, name)
        .ok()?
        .reply()
        .ok()
        .map(|r| r.atom)
        .filter(|a| *a != 0)
}
fn property(c: &RustConnection, w: u32, name: &[u8], kind: AtomEnum) -> Option<Vec<u32>> {
    let reply = c
        .get_property(false, w, atom(c, name)?, kind, 0, 1024)
        .ok()?
        .reply()
        .ok()?;
    if reply.bytes_after != 0 {
        return None;
    }
    {
        let values = reply.value32()?.collect();
        Some(values)
    }
}
// Minimal XRes 1.2 LocalClientPID request. Keeping this local avoids widening the
// x11rb feature set used by the rest of the workspace. Unknown/remote clients refuse.
struct LocalPid(u32);
impl x11rb::x11_utils::TryParse for LocalPid {
    fn try_parse(bytes: &[u8]) -> std::result::Result<(Self, &[u8]), x11rb::errors::ParseError> {
        use x11rb::errors::ParseError;
        let word = |offset| -> std::result::Result<u32, ParseError> {
            Ok(u32::from_ne_bytes(
                bytes
                    .get(offset..offset + 4)
                    .ok_or(ParseError::InsufficientData)?
                    .try_into()
                    .unwrap(),
            ))
        };
        if bytes.first() != Some(&1)
            || word(4)? != 4
            || word(8)? != 1
            || word(36)? != 2
            || word(40)? != 4
            || word(44)? == 0
        {
            return Err(ParseError::InvalidValue);
        }
        Ok((Self(word(44)?), &bytes[48..]))
    }
}
fn xres_pid(c: &RustConnection, window: u32) -> Option<u32> {
    let opcode = c.extension_information("X-Resource").ok()??.major_opcode;
    let mut request = vec![opcode, 4, 0, 0]; // QueryClientIds
    request[2..4].copy_from_slice(&4u16.to_ne_bytes());
    for value in [1u32, window, 2] {
        request.extend(value.to_ne_bytes());
    }
    c.send_request_with_reply::<LocalPid>(&[std::io::IoSlice::new(&request)], vec![])
        .ok()?
        .reply()
        .ok()
        .map(|r| r.0)
}
fn x11_pid_agrees(claimed: &[u32], local: Option<u32>, focus_matches: bool) -> bool {
    claimed.len() == 1 && claimed[0] > 0 && local == Some(claimed[0]) && focus_matches
}
fn focus_matches(c: &RustConnection, window: u32) -> Option<bool> {
    let mut focus = c.get_input_focus().ok()?.reply().ok()?.focus;
    for _ in 0..48 {
        if focus == window {
            return Some(true);
        }
        if focus <= 1 {
            return Some(false);
        }
        let parent = c.query_tree(focus).ok()?.reply().ok()?.parent;
        if parent == focus {
            return Some(false);
        }
        focus = parent;
    }
    Some(false)
}
thread_local! {
    static X11: std::cell::RefCell<Option<(RustConnection, usize)>> = const { std::cell::RefCell::new(None) };
}
fn with_x11<T>(f: impl FnOnce(&RustConnection, usize) -> Option<T>) -> Option<T> {
    X11.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = RustConnection::connect(None).ok();
        }
        let (c, screen) = slot.as_ref()?;
        let result = f(c, *screen);
        if result.is_none() {
            *slot = None;
        }
        result
    })
}
pub(crate) fn x11_foreground() -> Option<(u32, i64)> {
    with_x11(x11_foreground_on)
}
fn root_ancestor(
    mut window: u32,
    root: u32,
    mut parent: impl FnMut(u32) -> Option<u32>,
) -> Option<u32> {
    for _ in 0..48 {
        let next = parent(window)?;
        if next == root {
            return Some(window);
        }
        if next == 0 || next == window {
            return None;
        }
        window = next;
    }
    None
}
fn windows_above(children: &[u32], active_ancestor: u32) -> Option<&[u32]> {
    if children.len() > 512 {
        return None;
    }
    let index = children.iter().position(|w| *w == active_ancestor)?;
    Some(&children[index + 1..])
}
fn x11_foreground_on(c: &RustConnection, screen: usize) -> Option<(u32, i64)> {
    let root = c.setup().roots[screen].root;
    let windows = property(&c, root, b"_NET_ACTIVE_WINDOW", AtomEnum::WINDOW)?;
    if windows.len() != 1 || windows[0] == 0 {
        return None;
    }
    let w = windows[0];
    if c.get_window_attributes(w).ok()?.reply().ok()?.map_state != MapState::VIEWABLE {
        return None;
    }
    let state = property(&c, w, b"WM_STATE", AtomEnum::ANY)?;
    if state.first() != Some(&1) {
        return None;
    } // ICCCM NormalState only (IconicState=3).
      // QueryTree is bottom-to-top. Mutter's permanent InputOnly guard window
      // sits below normal clients; only windows above the active root ancestor
      // can cover it. Unknown ancestry or an oversized root inventory refuses.
    let ancestor = root_ancestor(w, root, |child| {
        c.query_tree(child).ok()?.reply().ok().map(|r| r.parent)
    })?;
    let children = c.query_tree(root).ok()?.reply().ok()?.children;
    for &child in windows_above(&children, ancestor)?.iter().rev() {
        let attrs = c.get_window_attributes(child).ok()?.reply().ok()?;
        if attrs.map_state == MapState::VIEWABLE && attrs.override_redirect {
            let g = c.get_geometry(child).ok()?.reply().ok()?;
            let s = &c.setup().roots[screen];
            if g.x <= 0
                && g.y <= 0
                && g.width >= s.width_in_pixels
                && g.height >= s.height_in_pixels
            {
                return None;
            }
        }
    }
    let pids = property(&c, w, b"_NET_WM_PID", AtomEnum::CARDINAL)?;
    x11_pid_agrees(&pids, xres_pid(c, w), focus_matches(c, w)?).then(|| (pids[0], i64::from(w)))
}
pub(crate) fn x11_bounds(window: i64) -> Option<(i32, i32, i32, i32)> {
    let window = u32::try_from(window).ok()?;
    with_x11(|c, screen| {
        let root = c.setup().roots[screen].root;
        let g = c.get_geometry(window).ok()?.reply().ok()?;
        let p = c
            .translate_coordinates(window, root, 0, 0)
            .ok()?
            .reply()
            .ok()?;
        Some((
            i32::from(p.dst_x),
            i32::from(p.dst_y),
            i32::from(g.width),
            i32::from(g.height),
        ))
    })
}
#[derive(Deserialize)]
struct SwayNode {
    id: u64,
    pid: Option<u32>,
    #[serde(default)]
    focused: bool,
    #[serde(default)]
    nodes: Vec<SwayNode>,
    #[serde(default)]
    floating_nodes: Vec<SwayNode>,
}
fn sway_selection(root: SwayNode) -> Result<(u32, u64)> {
    let mut pending = vec![root];
    let mut found = vec![];
    let mut count = 0;
    while let Some(node) = pending.pop() {
        count += 1;
        if count > 4096 {
            return Err(ax::unavailable());
        }
        if node.focused {
            if let Some(pid) = node.pid {
                found.push((pid, node.id))
            }
        }
        pending.extend(node.nodes);
        pending.extend(node.floating_nodes);
    }
    if found.len() != 1 || found[0].0 == 0 || found[0].1 == 0 {
        return Err(refusal(
            "computerHistoryAmbiguousWayland",
            "Compositor focus is absent or ambiguous",
        ));
    }
    Ok(found[0])
}
fn sway_active() -> Result<(u32, u64)> {
    let path=std::env::var_os("SWAYSOCK").ok_or_else(||refusal("computerHistoryWaylandUnsupported","Wayland compositor identity is unsupported: GNOME, KWin and generic wlr require a verified compositor adapter"))?;
    let mut socket = UnixStream::connect(path).map_err(|_| ax::unavailable())?;
    socket
        .set_read_timeout(Some(Duration::from_millis(150)))
        .map_err(|_| ax::unavailable())?;
    socket
        .set_write_timeout(Some(Duration::from_millis(150)))
        .map_err(|_| ax::unavailable())?;
    // Verify the local peer really is Sway, not a supplied metadata file/socket.
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of_val(&cred) as libc::socklen_t;
    use std::os::fd::AsRawFd;
    if unsafe {
        libc::getsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    } != 0
        || cred.uid != unsafe { libc::geteuid() }
    {
        return Err(ax::unavailable());
    }
    let compositor = trusted_compositor(cred.pid as u32, "sway").ok_or_else(ax::unavailable)?;
    socket
        .write_all(b"i3-ipc\0\0\0\0\x04\0\0\0")
        .map_err(|_| ax::unavailable())?;
    let mut header = [0u8; 14];
    socket
        .read_exact(&mut header)
        .map_err(|_| ax::unavailable())?;
    let size = u32::from_le_bytes(header[6..10].try_into().unwrap()) as usize;
    if &header[..6] != b"i3-ipc"
        || u32::from_le_bytes(header[10..].try_into().unwrap()) != 4
        || size > 1024 * 1024
    {
        return Err(ax::unavailable());
    }
    let mut body = vec![0; size];
    socket
        .read_exact(&mut body)
        .map_err(|_| ax::unavailable())?;
    if !compositor.unchanged() {
        return Err(ax::unavailable());
    }
    // Deserialize only identity and focus; titles and URLs have no representation here.
    sway_selection(serde_json::from_slice(&body).map_err(|_| ax::unavailable())?)
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum CompositorToken {
    Sway(u64),
    Gnome(String, u64),
    Kwin(String, u64),
}
impl CompositorToken {
    fn frame_pid(&self, verified_pid: u32) -> Option<u32> {
        // Plasma services can expose hidden AT-SPI frames with stale Active
        // state. KWin's authenticated window PID scopes the accessibility tree;
        // active_frame still refuses multiple frames within that process.
        match self {
            Self::Kwin(..) => Some(verified_pid),
            _ => None,
        }
    }
}
#[derive(Deserialize)]
struct HelperWindow {
    #[serde(alias = "token")]
    id: u64,
    pid: u32,
    #[serde(alias = "focused")]
    active: bool,
    minimized: bool,
    visible: Option<bool>,
}
fn helper_selection(raw: &str) -> Result<(u32, u64)> {
    if raw.len() > 1024 * 1024 {
        return Err(ax::unavailable());
    }
    let windows: Vec<HelperWindow> = serde_json::from_str(raw).map_err(|_| ax::unavailable())?;
    if windows.len() > 4096 {
        return Err(ax::unavailable());
    }
    let mut ids = std::collections::HashSet::new();
    let mut active = Vec::new();
    for w in windows {
        if w.id == 0 || w.pid == 0 || !ids.insert(w.id) {
            return Err(ax::unavailable());
        }
        if w.active {
            if w.minimized || w.visible == Some(false) {
                return Err(ax::unavailable());
            }
            active.push((w.pid, w.id));
        }
    }
    if active.len() != 1 {
        return Err(refusal(
            "computerHistoryAmbiguousWayland",
            "Compositor focus is absent or ambiguous",
        ));
    }
    Ok(active[0])
}
fn trusted_compositor(pid: u32, expected: &str) -> Option<kwin::Executable> {
    kwin::Executable::pin(pid, expected)
}
impl SessionGate {
    fn compositor_snapshot(&self, capture_required: bool) -> Result<(String, bool, String)> {
        ax::run(async {
            let bus = ax::call(zbus::fdo::DBusProxy::new(&self.session)).await?;
            for (dest, path, desktop, executable, method, gnome) in [
                (
                    "org.cua.WinRects",
                    "/org/cua/WinRects",
                    "org.gnome.Shell",
                    "gnome-shell",
                    "GetRects",
                    true,
                ),
                (
                    "org.cua.KWinTarget",
                    "/org/cua/KWinTarget",
                    "org.kde.KWin",
                    "kwin_wayland",
                    "GetWindows",
                    false,
                ),
            ] {
                if !ax::call(bus.name_has_owner(dest.try_into().unwrap())).await? {
                    continue;
                }
                // Bind the unique connection. Another process cannot replace the helper
                // between checking its ownership and returning the window metadata.
                let owner = ax::call(bus.get_name_owner(dest.try_into().unwrap())).await?;
                let desktop_owner =
                    ax::call(bus.get_name_owner(desktop.try_into().unwrap())).await?;
                if owner != desktop_owner {
                    return Err(ax::unavailable());
                }
                let kwin_identity = if gnome {
                    None
                } else {
                    Some(kwin::Identity::connect().ok_or_else(ax::unavailable)?)
                };
                let uid =
                    ax::call(bus.get_connection_unix_user(owner.as_str().try_into().unwrap()))
                        .await?;
                let pid = ax::call(
                    bus.get_connection_unix_process_id(owner.as_str().try_into().unwrap()),
                )
                .await?;
                let gnome_identity = if gnome {
                    Some(trusted_compositor(pid, executable).ok_or_else(ax::unavailable)?)
                } else {
                    None
                };
                if let Some(identity) = &kwin_identity {
                    let credentials = ax::call(
                        bus.get_connection_credentials(owner.as_str().try_into().unwrap()),
                    )
                    .await?;
                    if !kwin::bus_matches(
                        owner.as_str(),
                        desktop_owner.as_str(),
                        owner.as_str(),
                        credentials.process_id(),
                        credentials.unix_user_id(),
                        identity.pid(),
                    ) || pid != identity.pid()
                        || uid != unsafe { libc::geteuid() }
                    {
                        return Err(ax::unavailable());
                    }
                    self.check(identity.listener_pid()).await?;
                } else if uid != unsafe { libc::geteuid() } {
                    return Err(ax::unavailable());
                }
                self.check(pid).await?;
                let proxy =
                    ax::call(zbus::Proxy::new(&self.session, owner.as_str(), path, dest)).await?;
                let version: u32 = ax::call(proxy.call("GetVersion", &())).await?;
                // Bundled WinRects v8 and KWinTarget v1. Refuse an unreviewed
                // protocol even when its owner is the right compositor.
                if (gnome && version != 8) || (!gnome && version != 1) {
                    return Err(ax::unavailable());
                }
                if capture_required {
                    let capture: u32 = ax::call(proxy.call("GetCaptureVersion", &())).await?;
                    picker_capture_protocol(gnome, capture)?;
                }
                let raw: String = ax::call(proxy.call(method, &())).await?;
                if let Some(identity) = &gnome_identity {
                    self.check(pid).await?;
                    if !identity.unchanged()
                        || ax::call(bus.get_name_owner(dest.try_into().unwrap())).await? != owner
                        || ax::call(bus.get_name_owner(desktop.try_into().unwrap())).await? != owner
                    {
                        return Err(ax::unavailable());
                    }
                }
                if let Some(identity) = &kwin_identity {
                    self.check(identity.pid()).await?;
                    self.check(identity.listener_pid()).await?;
                    let current_owner =
                        ax::call(bus.get_name_owner(dest.try_into().unwrap())).await?;
                    let current_desktop =
                        ax::call(bus.get_name_owner(desktop.try_into().unwrap())).await?;
                    let credentials = ax::call(
                        bus.get_connection_credentials(owner.as_str().try_into().unwrap()),
                    )
                    .await?;
                    if !kwin::bus_matches(
                        current_owner.as_str(),
                        current_desktop.as_str(),
                        owner.as_str(),
                        credentials.process_id(),
                        credentials.unix_user_id(),
                        identity.pid(),
                    ) || !identity.unchanged()
                    {
                        return Err(ax::unavailable());
                    }
                }
                return Ok((owner.to_string(), gnome, raw));
            }
            Err(refusal("computerHistoryWaylandUnsupported","Wayland identity requires Sway IPC or a verified GNOME/KWin helper; generic wlr toplevel app IDs cannot prove PID ownership"))
        })
    }
    fn compositor_active(&self) -> Result<(u32, CompositorToken)> {
        if std::env::var_os("SWAYSOCK").is_some() {
            return sway_active().map(|(pid, id)| (pid, CompositorToken::Sway(id)));
        }
        let (owner, gnome, raw) = self.compositor_snapshot(false)?;
        let (pid, id) = helper_selection(&raw)?;
        Ok((
            pid,
            if gnome {
                CompositorToken::Gnome(owner, id)
            } else {
                CompositorToken::Kwin(owner, id)
            },
        ))
    }
}
/// Native window metadata from the same authenticated compositor used by History.
/// No title/AT-SPI joins, XWayland dependency, or foreign-toplevel ID guesses.
#[derive(Clone, Debug, Deserialize)]
pub struct PickerWindow {
    #[serde(alias = "token")]
    pub id: u64,
    pub pid: u32,
    pub title: String,
    #[serde(default)]
    pub app_id: String,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub minimized: bool,
    pub visible: Option<bool>,
    pub stacking: Option<usize>,
}
fn picker_capture_protocol(gnome: bool, capture: u32) -> Result<()> {
    if capture != if gnome { 9 } else { 2 } {
        return Err(ax::unavailable());
    }
    Ok(())
}
fn picker_snapshot(raw: &str) -> Result<Vec<PickerWindow>> {
    if raw.len() > 1024 * 1024 {
        return Err(ax::unavailable());
    }
    let windows: Vec<PickerWindow> = serde_json::from_str(raw).map_err(|_| ax::unavailable())?;
    let mut ids = std::collections::HashSet::new();
    if windows.len() > 4096
        || windows.iter().any(|w| {
            w.id == 0
                || w.id > i64::MAX as u64
                || w.pid == 0
                || !ids.insert(w.id)
                || w.w > 32768
                || w.h > 32768
                || w.title.len() > 16384
                || w.app_id.len() > 4096
        })
    {
        return Err(ax::unavailable());
    }
    Ok(windows)
}
pub fn picker_windows() -> Result<Vec<PickerWindow>> {
    // Driver tools may call from a Tokio task. Keep History's synchronous
    // D-Bus runtime off that caller's executor, including diagnostics calls.
    std::thread::spawn(picker_windows_inner)
        .join()
        .map_err(|_| ax::unavailable())?
}
fn picker_windows_inner() -> Result<Vec<PickerWindow>> {
    if std::env::var_os("SWAYSOCK").is_some() {
        return Err(refusal(
            "nativeWaylandPickerUnsupported",
            "Native window picking is unsupported on Sway",
        ));
    }
    let gate = ax::run(SessionGate::new())?;
    let (_, _, raw) = gate.compositor_snapshot(true)?;
    picker_snapshot(&raw)
}

#[derive(Clone)]
struct Admitted {
    context: Context,
    start: String,
    frame: Option<ObjectKey>,
    compositor_id: Option<CompositorToken>,
}
#[derive(Default)]
struct Backoff {
    failures: u32,
    until: Option<Instant>,
}
impl Backoff {
    fn waiting(&self) -> bool {
        self.until.is_some_and(|at| Instant::now() < at)
    }
    fn fail(&mut self) {
        self.failures = self.failures.saturating_add(1);
        self.until =
            Some(Instant::now() + Duration::from_millis(250 * (1u64 << self.failures.min(4))));
    }
    fn reset(&mut self) {
        *self = Self::default();
    }
}
#[derive(Default)]
pub struct LinuxSource {
    gate: Option<SessionGate>,
    atspi: Option<Atspi>,
    admitted: Option<Admitted>,
    focused: Option<ObjectKey>,
    refusal: Option<String>,
    scan_scope: Option<(u32, String, ObjectKey)>,
    scan_backoff: Backoff,
    content_backoff: Backoff,
    identity_backoff: Backoff,
    focus_candidate: Option<ObjectKey>,
    browser_page: Option<browser_native::Page>,
}
impl LinuxSource {
    pub fn note_refusal(&mut self, error: &Error) {
        self.refusal = Some(error.to_string());
    }
    fn sample_result<T>(&mut self, value: Result<Option<T>>) -> Result<Option<T>> {
        match value {
            Ok(value) => {
                self.content_backoff.reset();
                Ok(value)
            }
            Err(e) => {
                self.note_refusal(&e);
                self.content_backoff.fail();
                Ok(None)
            }
        }
    }
    pub fn focus_events(&mut self, events: &[PendingEvent], lost: bool) {
        if lost {
            self.focus_candidate = None;
        }
        for event in events {
            if matches!(event.kind, EventKind::Focus | EventKind::Window) {
                self.focus_candidate = if event.kind == EventKind::Focus {
                    Some(event.object.clone())
                } else {
                    None
                };
            }
        }
    }
    pub fn scope_key(&self) -> Option<String> {
        self.admitted
            .as_ref()
            .map(|a| format!("{}:{:?}:{:?}", a.start, a.frame, a.compositor_id))
    }
    pub fn refusal(&self) -> Option<&str> {
        self.refusal.as_deref()
    }
    fn ensure(&mut self) -> Result<()> {
        if self.gate.is_none() {
            self.gate = Some(ax::run(SessionGate::new())?)
        }
        Ok(())
    }
    fn ax(&mut self) -> Result<Atspi> {
        if self.atspi.is_none() {
            self.atspi = Some(Atspi::new()?)
        }
        Ok(self.atspi.as_ref().unwrap().clone())
    }
    fn identify_inner(&mut self) -> Result<Context> {
        self.ensure()?;
        // Check our own session before touching any application's accessibility tree.
        ax::run(self.gate.as_ref().unwrap().check(std::process::id())).map_err(|_| {
            refusal(
                "computerHistorySessionUnknown",
                "History requires verified active and unlocked logind session state",
            )
        })?;
        let (pid, window_id, frame, compositor_id) = if is_wayland() {
            let (compositor_pid, id) = self.gate.as_ref().unwrap().compositor_active()?;
            let atspi = self.ax()?;
            let kind = application(compositor_pid)
                .as_deref()
                .and_then(browser_native::kind);
            let (frame, pid) = if let Some(kind) = kind {
                ax::run(atspi.active_browser_frame(compositor_pid, kind))?
            } else {
                ax::run(atspi.active_frame(id.frame_pid(compositor_pid)))?
            };
            if pid != compositor_pid {
                return Err(refusal(
                    "computerHistoryAmbiguousWayland",
                    "Compositor and AT-SPI active process disagree",
                ));
            }
            (pid, frame.window_id(), Some(frame), Some(id))
        } else {
            let (pid, id) = x11_foreground().ok_or_else(|| {
                refusal(
                    "computerHistoryWindowRefused",
                    "X11 active window is unknown, iconic or covered by a possible locker",
                )
            })?;
            let bounds = x11_bounds(id).ok_or_else(ax::unavailable)?;
            let atspi = self.ax()?;
            let frame = ax::run(atspi.x11_frame(pid, bounds))?;
            (pid, id, Some(frame), None)
        };
        ax::run(self.gate.as_ref().unwrap().check(pid))?;
        let application = application(pid).ok_or_else(ax::unavailable)?;
        let start = process_start(pid).ok_or_else(ax::unavailable)?;
        let scope = (
            pid,
            start.clone(),
            frame.clone().ok_or_else(ax::unavailable)?,
        );
        if self.scan_scope.as_ref() != Some(&scope) {
            self.scan_scope = Some(scope.clone());
            self.scan_backoff.reset();
            self.content_backoff.reset();
            self.focus_candidate = None;
        }
        let website = if browser(&application) || self.scan_backoff.waiting() {
            Website::Unknown
        } else {
            let atspi = self.ax()?;
            match ax::run(atspi.inspect(&scope.2, pid)) {
                Ok(scan) if scan.native => {
                    self.scan_backoff.reset();
                    // Bootstrap focus once; focus notifications replace this cached reference.
                    self.focus_candidate = scan.focused;
                    Website::NotWeb
                }
                Ok(_) => Website::Unknown,
                Err(e) => {
                    self.note_refusal(&e);
                    self.scan_backoff.fail();
                    Website::Unknown
                }
            }
        };
        if website == Website::Unknown && self.refusal.is_none() {
            self.refusal =
                Some("computerHistoryOriginUnknown: Browser or incomplete native tree".into());
        }
        let context = Context {
            application,
            process_id: i64::from(pid),
            window_id,
            website,
        };
        self.admitted = Some(Admitted {
            context: context.clone(),
            start,
            frame,
            compositor_id,
        });
        Ok(context)
    }
    pub fn verify_window(&self, context: &Context) -> Result<bool> {
        let Some(a) = &self.admitted else {
            return Ok(false);
        };
        if a.context != *context
            || process_start(context.process_id as u32).as_ref() != Some(&a.start)
        {
            return Ok(false);
        }
        let Some(gate) = &self.gate else {
            return Ok(false);
        };
        if ax::run(gate.check(context.process_id as u32)).is_err() {
            return Ok(false);
        }
        if let Some(id) = &a.compositor_id {
            if gate.compositor_active().ok() != Some((context.process_id as u32, id.clone())) {
                return Ok(false);
            }
            let Some(atspi) = &self.atspi else {
                return Ok(false);
            };
            let selected = if let Some(kind) = browser_native::kind(&context.application) {
                ax::run(atspi.active_browser_frame(context.process_id as u32, kind))
            } else {
                ax::run(atspi.active_frame(id.frame_pid(context.process_id as u32)))
            };
            Ok(selected.ok().as_ref().is_some_and(|(frame, pid)| {
                Some(frame) == a.frame.as_ref() && i64::from(*pid) == context.process_id
            }))
        } else {
            if x11_foreground() != Some((context.process_id as u32, context.window_id)) {
                return Ok(false);
            }
            if let Some(frame) = &a.frame {
                let Some(atspi) = &self.atspi else {
                    return Ok(false);
                };
                let Some(bounds) = x11_bounds(context.window_id) else {
                    return Ok(false);
                };
                if ax::run(atspi.x11_frame(context.process_id as u32, bounds))
                    .ok()
                    .as_ref()
                    != Some(frame)
                {
                    return Ok(false);
                }
            }
            Ok(true)
        }
    }
    fn frame(&mut self, context: &Context) -> Result<Option<(Atspi, ObjectKey)>> {
        if self.content_backoff.waiting()
            || context.website != Website::NotWeb
            || !self.verify_window(context)?
        {
            return Ok(None);
        }
        let atspi = self.ax()?;
        // verify_window just checked this exact frame against native identity.
        let Some(frame) = self.admitted.as_ref().and_then(|a| a.frame.clone()) else {
            return Ok(None);
        };
        Ok(Some((atspi, frame)))
    }
    pub fn start_events(&mut self, context: &Context) -> Result<Events> {
        let (atspi, frame) = self.frame(context)?.ok_or_else(ax::unavailable)?;
        Events::start(atspi, frame)
    }
    pub(crate) fn input_scope(&mut self, context: &Context) -> Result<(Atspi, ObjectKey)> {
        self.frame(context)?.ok_or_else(ax::unavailable)
    }
    pub(crate) fn gnome_pointer_scope(
        &mut self,
        context: &Context,
    ) -> Result<Option<(zbus::Connection, String, u64, Atspi, ObjectKey)>> {
        let Some((atspi, frame)) = self.frame(context)? else { return Ok(None); };
        let Some(CompositorToken::Gnome(owner, id)) = self.admitted.as_ref().and_then(|a| a.compositor_id.as_ref()) else {
            return Ok(None);
        };
        // frame() has rechecked the actual active logind session, compositor
        // owner/executable and the exact accessible frame. Pin that unique peer.
        Ok(Some((self.gate.as_ref().ok_or_else(ax::unavailable)?.session.clone(), owner.clone(), *id, atspi, frame)))
    }
    pub(crate) fn kwin_input_helper(&mut self, context: &Context) -> Result<(zbus::Connection, String, u64)> {
        if !self.verify_window(context)? { return Err(ax::unavailable()); }
        let admitted = self.admitted.as_ref().ok_or_else(ax::unavailable)?;
        let Some(CompositorToken::Kwin(owner, id)) = &admitted.compositor_id else {
            return Err(refusal("computerHistoryInputUnsupported", "Physical input observation requires a verified KWin History helper"));
        };
        // The existing gate checks the compositor executable, unique bus owner,
        // real logind/locker state and the exact admitted accessibility frame.
        Ok((self.gate.as_ref().ok_or_else(ax::unavailable)?.session.clone(), owner.clone(), *id))
    }
    pub fn focused_key(&self) -> Option<ObjectKey> {
        self.focused.clone()
    }
    pub fn event_content(
        &mut self,
        context: &Context,
        event: &PendingEvent,
    ) -> Result<Option<Content>> {
        if event.received.elapsed() > Duration::from_secs(2) {
            return Ok(None);
        }
        let Some((atspi, frame)) = self.frame(context)? else {
            return Ok(None);
        };
        let value = self.sample_result(ax::run(ax::read_checked(
            &atspi,
            &event.object,
            &frame,
            context.process_id as u32,
            false,
            false,
            false,
        )))?;
        let value = value
            .filter(|c| matches!(c.role.as_str(), "AXButton" | "AXCheckBox" | "AXRadioButton"));
        Ok(if self.verify_window(context)? {
            value
        } else {
            None
        })
    }
    pub fn object_content(
        &mut self,
        context: &Context,
        key: &ObjectKey,
    ) -> Result<Option<Content>> {
        let Some((atspi, frame)) = self.frame(context)? else {
            return Ok(None);
        };
        let value = self.sample_result(ax::run(ax::read_checked(
            &atspi,
            key,
            &frame,
            context.process_id as u32,
            false,
            false,
            false,
        )))?;
        Ok(if self.verify_window(context)? {
            value
        } else {
            None
        })
    }
    fn snapshot(&mut self, context: &Context, terminal: bool) -> Result<Option<(String, bool)>> {
        let Some((atspi, frame)) = self.frame(context)? else {
            return Ok(None);
        };
        let value = self.sample_result(ax::run(atspi.snapshot(
            &frame,
            context.process_id as u32,
            terminal,
        )))?;
        Ok(if self.verify_window(context)? {
            value
        } else {
            None
        })
    }
    pub fn native_window_text(&mut self, context: &Context) -> Result<Option<Content>> {
        if terminal_application(&context.application) {
            return Ok(None);
        }
        Ok(self.snapshot(context, false)?.map(|(text, _)| Content {
            role: "AXWindowSnapshot".into(),
            value: Some(text),
            ..Default::default()
        }))
    }
    pub fn terminal_text(&mut self, context: &Context) -> Result<Option<(String, bool)>> {
        if !terminal_application(&context.application) {
            return Ok(None);
        }
        self.snapshot(context, true)
    }
}
impl Source for LinuxSource {
    fn identify(&mut self) -> Result<Option<Context>> {
        let previous = self.admitted.take();
        self.focused = None;
        self.browser_page = None;
        if self.identity_backoff.waiting() {
            return Ok(None);
        }
        if !self.scan_backoff.waiting() && !self.content_backoff.waiting() {
            self.refusal = None;
        }
        match self.identify_inner() {
            Ok(c) => {
                self.identity_backoff.reset();
                if let (Some(previous), Some(current)) = (previous, self.admitted.as_mut()) {
                    if previous.context == current.context
                        && previous.start == current.start
                        && previous.compositor_id == current.compositor_id
                        && current.frame.is_none()
                    {
                        current.frame = previous.frame
                    }
                }
                Ok(Some(c))
            }
            Err(e) => {
                self.admitted = None;
                self.identity_backoff.fail();
                self.refusal = Some(e.to_string());
                Ok(None)
            }
        }
    }
    fn resolve(&mut self, context: &Context, browser_capture: bool) -> Result<Option<Context>> {
        if !self.verify_window(context)? {
            return Ok(None);
        }
        if !browser(&context.application) {
            return Ok(Some(context.clone()));
        }
        if !browser_capture {
            return Ok(None);
        }
        let Some(kind) = browser_native::kind(&context.application) else {
            return Ok(None);
        };
        if !browser_native::executable(context.process_id as u32, kind) {
            return Ok(None);
        }
        let Some(frame) = self.admitted.as_ref().and_then(|a| a.frame.clone()) else {
            return Ok(None);
        };
        let atspi = self.ax()?;
        let snapshot = ax::run(browser_native::snapshot(
            &atspi,
            &frame,
            context.process_id as u32,
            kind,
        ));
        let page = match snapshot {
            Ok(browser_native::Snapshot::Public(page)) => page,
            Ok(_) => return Ok(None),
            Err(error) => {
                self.note_refusal(&error);
                return Ok(None);
            }
        };
        if !self.verify_window(context)? {
            return Ok(None);
        }
        let mut resolved = context.clone();
        resolved.website = Website::Url(page.url.clone());
        self.browser_page = Some(page);
        self.admitted.as_mut().unwrap().context = resolved.clone();
        self.refusal = None;
        Ok(Some(resolved))
    }
    fn read(&mut self, context: &Context, mode: CaptureMode) -> Result<Option<Content>> {
        self.focused = None;
        if mode == CaptureMode::ApplicationMetadata {
            return Ok(None);
        }
        if let Some(page) = self.browser_page.clone() {
            if !self.verify(context)? {
                return Ok(None);
            }
            let Some(frame) = self.admitted.as_ref().and_then(|a| a.frame.clone()) else {
                return Ok(None);
            };
            let atspi = self.ax()?;
            let content = self.sample_result(ax::run(browser_native::read(
                &atspi,
                &frame,
                context.process_id as u32,
                &page,
                mode == CaptureMode::Values,
            )))?;
            return Ok(if self.verify(context)? { content } else { None });
        }
        if terminal_application(&context.application) {
            return Err(refusal(
                "computerHistoryEventSourceRequired",
                "Terminal content requires the stateful interaction recorder",
            ));
        }
        let Some((atspi, frame)) = self.frame(context)? else {
            return Ok(None);
        };
        let Some(key) = self.focus_candidate.clone() else {
            return Ok(None);
        };
        let content = self.sample_result(ax::run(ax::read_checked(
            &atspi,
            &key,
            &frame,
            context.process_id as u32,
            mode == CaptureMode::Values,
            true,
            false,
        )))?;
        if content.is_some() {
            self.focused = Some(key)
        }
        Ok(if self.verify(context)? { content } else { None })
    }
    fn verify(&mut self, context: &Context) -> Result<bool> {
        if !self.verify_window(context)? {
            return Ok(false);
        }
        if let Some(page) = &self.browser_page {
            let Some(kind) = browser_native::kind(&context.application) else {
                return Ok(false);
            };
            let Some(frame) = self.admitted.as_ref().and_then(|a| a.frame.as_ref()) else {
                return Ok(false);
            };
            let Some(atspi) = &self.atspi else {
                return Ok(false);
            };
            if !browser_native::executable(context.process_id as u32, kind) {
                return Ok(false);
            }
            return Ok(ax::run(browser_native::snapshot(
                atspi,
                frame,
                context.process_id as u32,
                kind,
            ))
            .ok()
                == Some(browser_native::Snapshot::Public(page.clone()))
                && self.verify_window(context)?);
        }
        let Some(key) = &self.focused else {
            return Ok(true);
        };
        let Some(frame) = self.admitted.as_ref().and_then(|a| a.frame.as_ref()) else {
            return Ok(false);
        };
        let Some(atspi) = &self.atspi else {
            return Ok(false);
        };
        // Recheck cached object state and ancestry; never rescan the whole tree.
        Ok(ax::run(atspi.verify_focus(key, frame, context.process_id as u32)).unwrap_or(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(flavor = "current_thread")]
    async fn picker_is_safe_inside_an_async_driver_runtime() {
        assert!(std::panic::catch_unwind(picker_windows).is_ok());
    }
    #[test]
    fn picker_requires_packaged_capture_protocol() {
        for (gnome, accepted) in [(true, 9), (false, 2)] {
            for version in 0..=10 {
                assert_eq!(
                    picker_capture_protocol(gnome, version).is_ok(),
                    version == accepted
                );
            }
        }
    }
    #[test]
    fn picker_requires_compositor_identity_and_bounded_geometry() {
        let row = serde_json::json!({"token":17,"pid":42,"title":"Native fixture","app_id":"fixture",
            "x":-2,"y":3,"w":640,"h":480,"minimized":false,"active":true,"stacking":1});
        let raw = serde_json::to_string(&vec![row.clone()]).unwrap();
        let windows = picker_snapshot(&raw).unwrap();
        assert_eq!((windows[0].id, windows[0].pid, windows[0].x), (17, 42, -2));
        assert!(
            picker_snapshot(&serde_json::to_string(&vec![row.clone(), row.clone()]).unwrap())
                .is_err()
        );
        for (key, value) in [("token", 0), ("pid", 0), ("w", 32769), ("h", 32769)] {
            let mut bad = row.clone();
            bad[key] = serde_json::json!(value);
            assert!(
                picker_snapshot(&serde_json::to_string(&vec![bad]).unwrap()).is_err(),
                "{key}"
            );
        }
        let mut too_large = row.clone();
        too_large["token"] = serde_json::json!(u64::MAX);
        assert!(picker_snapshot(&serde_json::to_string(&vec![too_large]).unwrap()).is_err());
        for (key, limit) in [("title", 16384), ("app_id", 4096)] {
            let mut bad = row.clone();
            bad[key] = serde_json::json!("x".repeat(limit + 1));
            assert!(picker_snapshot(&serde_json::to_string(&vec![bad]).unwrap()).is_err());
        }
        let many: Vec<_> = (1..=4097)
            .map(|id| {
                let mut w = row.clone();
                w["token"] = serde_json::json!(id);
                w
            })
            .collect();
        assert!(picker_snapshot(&serde_json::to_string(&many).unwrap()).is_err());
        assert!(picker_snapshot(&("[]".to_owned() + &" ".repeat(1024 * 1024))).is_err());
        assert!(picker_snapshot("[]").unwrap().is_empty());
    }
    #[test]
    fn updated_and_interpreted_application_identity() {
        let cwd = std::path::Path::new("/apps");
        assert_eq!(
            stable_application("/usr/lib/firefox/firefox (deleted)", &[], cwd).as_deref(),
            Some("firefox")
        );
        for exe in ["python3", "python3.12", "electron", "node", "java"] {
            let a = stable_application(exe, &[exe, "first.py"], cwd).unwrap();
            let b = stable_application(exe, &[exe, "second.py"], cwd).unwrap();
            assert_ne!(a, b, "shared host collapsed app rules");
            assert!(stable_application(exe, &[exe], cwd).is_none());
            assert!(stable_application(exe, &[exe, "-c", "code"], cwd).is_none());
        }
        assert_eq!(
            stable_application("python3", &["python3", "/usr/bin/qutebrowser"], cwd).as_deref(),
            Some("qutebrowser")
        );
        assert_eq!(
            stable_application("python3", &["python3", "-m", "qutebrowser"], cwd).as_deref(),
            Some("qutebrowser")
        );
        assert_eq!(
            stable_application("python3", &["python3", "/usr/bin/terminator"], cwd).as_deref(),
            Some("terminator")
        );
        assert!(terminal_application("xterm (deleted)"));
        assert_ne!(
            stable_application("java", &["java", "-cp", "one", "Main"], cwd),
            stable_application("java", &["java", "-cp", "two", "Main"], cwd)
        );
    }
    #[test]
    fn stable_identity_preserves_app_exclusions() {
        use crate::policy::{Behavior, Decision, Rule, Settings};
        let mut settings = Settings::default();
        settings.observation.default_application_behavior = Behavior::Observe;
        settings.observation.blocklist.push(Rule::App {
            bundle_id: "firefox".into(),
        });
        settings.observation.blocklist.push(Rule::App {
            bundle_id: "python:/apps/first.py".into(),
        });
        let cwd = std::path::Path::new("/apps");
        for (exe, args, blocked) in [
            ("/usr/bin/firefox (deleted)", vec![], true),
            ("python3", vec!["python3", "first.py"], true),
            ("python3", vec!["python3", "second.py"], false),
        ] {
            let context = Context {
                application: stable_application(exe, &args, cwd).unwrap(),
                process_id: 42,
                window_id: 1,
                website: Website::NotWeb,
            };
            assert_eq!(
                settings.evaluate_application(&context) == Decision::ApplicationBlocked,
                blocked
            );
        }
    }
    #[test]
    fn browser_variants_refused() {
        for name in [
            "firefox-bin",
            "waterfox",
            "floorp",
            "thorium-browser",
            "zen-bin",
            "opera-beta",
            "opera-developer",
            "microsoft-edge-beta",
            "microsoft-edge-dev",
            "firefox (deleted)",
        ] {
            assert!(browser(name), "{name}");
        }
    }
    #[test]
    fn x11_covering_windows_respect_server_stacking_order() {
        assert_eq!(windows_above(&[7, 10, 11, 12], 10), Some(&[11, 12][..]));
        assert_eq!(windows_above(&[7, 10], 10), Some(&[][..]));
        assert_eq!(windows_above(&[7, 10], 7), Some(&[10][..]));
        assert!(windows_above(&[7, 10], 9).is_none());
        assert!(windows_above(&[], 9).is_none());
        assert!(windows_above(&vec![7; 513], 7).is_none());
    }
    #[test]
    fn x11_root_ancestor_requires_a_bounded_complete_chain() {
        assert_eq!(
            root_ancestor(10, 1, |w| match w {
                10 => Some(9),
                9 => Some(1),
                _ => None,
            }),
            Some(9)
        );
        assert_eq!(root_ancestor(10, 1, |_| Some(1)), Some(10));
        assert!(root_ancestor(10, 1, |_| None).is_none());
        assert!(root_ancestor(10, 1, |_| Some(0)).is_none());
        assert!(root_ancestor(10, 1, Some).is_none());
        assert!(root_ancestor(10, 1, |w| Some(if w == 10 { 11 } else { 10 })).is_none());
        assert!(root_ancestor(10, 1, |w| Some(w + 1)).is_none());
    }
    #[test]
    fn task_identity_matches_current_native_executable() {
        let expected = std::fs::read_link("/proc/self/exe").unwrap();
        assert_eq!(application_identity(std::process::id()).as_deref(),
            expected.file_name().and_then(|s| s.to_str()));
        assert!(application_identity(0).is_none());
        assert!(application_identity(u32::MAX).is_none());
    }
    #[test]
    fn x11_identity_needs_server_pid_and_focus() {
        assert!(x11_pid_agrees(&[42], Some(42), true));
        assert!(!x11_pid_agrees(&[42], Some(43), true));
        assert!(!x11_pid_agrees(&[42], None, true));
        assert!(!x11_pid_agrees(&[42], Some(42), false));
        assert!(!x11_pid_agrees(&[0], Some(0), true));
        assert!(!x11_pid_agrees(&[42, 42], Some(42), true));
    }
    #[test]
    fn local_pid_reply_requires_one_local_process() {
        use x11rb::x11_utils::TryParse;
        let mut reply = vec![0; 48];
        reply[0] = 1;
        for (at, value) in [(4, 4u32), (8, 1), (36, 2), (40, 4), (44, 42)] {
            reply[at..at + 4].copy_from_slice(&value.to_ne_bytes());
        }
        assert_eq!(LocalPid::try_parse(&reply).unwrap().0 .0, 42);
        for at in [0, 4, 8, 36, 40, 44] {
            let mut bad = reply.clone();
            bad[at..at + 4].fill(0);
            assert!(LocalPid::try_parse(&bad).is_err(), "offset {at}");
        }
        assert!(LocalPid::try_parse(&reply[..47]).is_err());
    }
    #[test]
    fn sample_timeout_is_refusal_with_backoff() {
        let mut source = LinuxSource::default();
        let result = source.sample_result::<()>(Err(ax::unavailable()));
        assert!(result.unwrap().is_none());
        assert!(source
            .refusal()
            .unwrap()
            .contains("AccessibilityUnavailable"));
        assert!(source.content_backoff.waiting());
        source.content_backoff.until = Some(Instant::now() - Duration::from_millis(1));
        assert!(!source.content_backoff.waiting());
        assert_eq!(source.sample_result(Ok(Some(42))).unwrap(), Some(42));
        assert!(!source.content_backoff.waiting());
        let mut backoff = Backoff::default();
        backoff.fail();
        let first = backoff.until.unwrap();
        backoff.fail();
        assert!(backoff.until.unwrap() > first);
    }
    #[test]
    fn identity_failure_waits_without_reopening_buses() {
        let mut source = LinuxSource::default();
        source.identity_backoff.fail();
        source.refusal = Some("previous identity refusal".into());
        assert!(source.identify().unwrap().is_none());
        assert!(source.gate.is_none());
        assert_eq!(source.refusal(), Some("previous identity refusal"));
    }
    #[test]
    fn focus_notifications_invalidate_cached_object() {
        let mut source = LinuxSource::default();
        let object = ObjectKey {
            bus: ":1.2".into(),
            path: "/entry".into(),
        };
        let mut event = PendingEvent {
            timestamp_ms: 0,
            received: Instant::now(),
            object: object.clone(),
            kind: EventKind::Focus,
        };
        source.focus_events(&[event.clone()], false);
        assert_eq!(source.focus_candidate, Some(object));
        event.kind = EventKind::Window;
        source.focus_events(&[event], false);
        assert!(source.focus_candidate.is_none());
    }
    #[test]
    fn same_active_unlocked_session_allowed() {
        assert!(session_allowed(
            Some("c1"),
            Some("c1"),
            Some(true),
            Some(false),
            Some(false)
        ))
    }
    #[test]
    fn kwin_frame_scope_uses_verified_window_pid() {
        assert_eq!(
            CompositorToken::Kwin(":1.7".into(), 4).frame_pid(123),
            Some(123)
        );
        assert_eq!(
            CompositorToken::Kwin(":1.7".into(), 4).frame_pid(456),
            Some(456)
        );
        assert_eq!(
            CompositorToken::Gnome(":1.7".into(), 4).frame_pid(123),
            None
        );
        assert_eq!(CompositorToken::Sway(4).frame_pid(123), None);
    }
    #[test]
    fn locked_refused() {
        assert!(!session_allowed(
            Some("c1"),
            Some("c1"),
            Some(true),
            Some(true),
            Some(false)
        ))
    }
    #[test]
    fn unknown_session_refused() {
        for (a, b, c, d, e) in [
            (None, Some("c1"), Some(true), Some(false), Some(false)),
            (Some("c1"), None, Some(true), Some(false), Some(false)),
            (Some("c1"), Some("c1"), None, Some(false), Some(false)),
            (Some("c1"), Some("c1"), Some(true), None, Some(false)),
            (Some("c1"), Some("c1"), Some(true), Some(false), None),
        ] {
            assert!(!session_allowed(a, b, c, d, e))
        }
    }
    #[test]
    fn switched_out_session_refused() {
        assert!(!session_allowed(
            Some("c1"),
            Some("c1"),
            Some(false),
            Some(false),
            Some(false)
        ))
    }
    #[test]
    fn other_session_refused() {
        assert!(!session_allowed(
            Some("c1"),
            Some("c2"),
            Some(true),
            Some(false),
            Some(false)
        ))
    }
    #[test]
    fn active_locker_refused() {
        assert!(!session_allowed(
            Some("c1"),
            Some("c1"),
            Some(true),
            Some(false),
            Some(true)
        ))
    }
    fn tree(s: &str) -> SwayNode {
        serde_json::from_str(s).unwrap()
    }
    #[test]
    fn ambiguous_wayland_refused() {
        assert!(sway_selection(tree(r#"{"id":1,"nodes":[{"id":2,"pid":42,"focused":true},{"id":3,"pid":43,"focused":true}]}"#)).is_err())
    }
    #[test]
    fn missing_wayland_focus_refused() {
        assert!(sway_selection(tree(r#"{"id":1,"nodes":[]}"#)).is_err())
    }
    #[test]
    fn compositor_identity_ignores_titles() {
        assert_eq!(sway_selection(tree(r#"{"id":1,"name":"PRIVATE TITLE","nodes":[{"id":2,"pid":42,"focused":true,"name":"DO NOT RECORD"}]}"#)).unwrap(),(42,2))
    }
    #[test]
    fn linux_browser_identity_is_unknown_origin() {
        for b in [
            "firefox-esr",
            "google-chrome-stable",
            "chromium-browser",
            "brave-browser",
            "epiphany",
            "qutebrowser",
        ] {
            assert!(browser(b))
        }
    }
    #[test]
    fn helper_identity_ignores_content() {
        assert_eq!(helper_selection(r#"[{"id":2,"pid":42,"focused":true,"minimized":false,"visible":true,"title":"PRIVATE TITLE"}]"#).unwrap(),(42,2));
        assert_eq!(
            helper_selection(r#"[{"token":3,"pid":43,"active":true,"minimized":false}]"#).unwrap(),
            (43, 3)
        );
    }
    #[test]
    fn helper_unknown_focus_refused() {
        assert!(helper_selection(r#"[{"id":2,"pid":42,"minimized":false}]"#).is_err());
    }
    #[test]
    fn helper_ambiguous_focus_refused() {
        assert!(helper_selection(r#"[{"id":2,"pid":42,"active":true,"minimized":false},{"id":3,"pid":43,"active":true,"minimized":false}]"#).is_err());
    }
    #[test]
    fn helper_minimized_refused() {
        assert!(helper_selection(r#"[{"id":2,"pid":42,"active":true,"minimized":true}]"#).is_err());
    }
    #[test]
    fn helper_hidden_refused() {
        assert!(helper_selection(
            r#"[{"id":2,"pid":42,"active":true,"minimized":false,"visible":false}]"#
        )
        .is_err());
    }
    #[test]
    fn helper_duplicate_ids_refused() {
        assert!(helper_selection(r#"[{"id":2,"pid":42,"active":false,"minimized":false},{"id":2,"pid":43,"active":true,"minimized":false}]"#).is_err());
    }
    #[test]
    fn helper_missing_owner_refused() {
        assert!(trusted_compositor(u32::MAX, "gnome-shell").is_none());
    }
}
