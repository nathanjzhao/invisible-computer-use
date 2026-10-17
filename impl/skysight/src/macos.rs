//! The macOS observer behind the recorder: a listen-only `CGEventTap` (keys and mouse
//! buttons; it can neither change nor drop an event) on its own run-loop thread, and
//! accessibility reads for the app and window in front, the element an input went
//! to, a browser window's URL, selected text and a terminal's text. Nothing here posts
//! an event, activates an app or raises a window. Needs Accessibility and Input
//! Monitoring; without them [`Source::start`] fails and says which.
//!
//! The only write to another app is `AXManualAccessibility = true` on a Chromium
//! browser, which makes it publish its page tree (and URL) the way Safari always does.

#![allow(non_upper_case_globals, non_snake_case, clippy::upper_case_acronyms)]

use crate::host::{Permissions, Source};
use crate::stream::{self, AppInfo, Button, Key, Mods, Observation, Target, WindowInfo};
use crate::Ts;
use std::collections::{BTreeMap, VecDeque};
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFIndex = isize;
type AXUIElementRef = *const c_void;
type CGEventRef = *const c_void;
type CFMachPortRef = *const c_void;
type CFRunLoopSourceRef = *const c_void;
type CFRunLoopRef = *const c_void;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct CGPoint { x: f64, y: f64 }

#[repr(C)]
struct CFRange { location: CFIndex, length: CFIndex }

type AXObserverRef = *const c_void;
type AxCallback = extern "C" fn(AXObserverRef, AXUIElementRef, CFStringRef, *mut c_void);
type TapCallback = extern "C" fn(*const c_void, u32, CGEventRef, *mut c_void) -> CGEventRef;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFRunLoopDefaultMode: CFStringRef;
    static kCFRunLoopCommonModes: CFStringRef;
    fn CFRelease(cf: CFTypeRef);
    fn CFGetTypeID(cf: CFTypeRef) -> usize;
    fn CFNumberGetTypeID() -> usize;
    fn CFNumberGetValue(n: CFTypeRef, ty: CFIndex, out: *mut c_void) -> bool;
    fn CFStringGetTypeID() -> usize;
    fn CFBooleanGetTypeID() -> usize;
    fn CFURLGetTypeID() -> usize;
    fn CFArrayGetTypeID() -> usize;
    fn CFBooleanGetValue(b: CFTypeRef) -> bool;
    fn CFStringCreateWithBytes(alloc: CFTypeRef, bytes: *const u8, len: CFIndex, enc: u32, ext: bool) -> CFStringRef;
    fn CFStringGetLength(s: CFStringRef) -> CFIndex;
    fn CFStringGetCharacters(s: CFStringRef, range: CFRange, buffer: *mut u16);
    fn CFStringGetMaximumSizeForEncoding(len: CFIndex, enc: u32) -> CFIndex;
    fn CFStringGetCString(s: CFStringRef, buf: *mut u8, size: CFIndex, enc: u32) -> bool;
    fn CFURLGetString(url: CFTypeRef) -> CFStringRef;
    fn CFURLCreateFromFileSystemRepresentation(alloc: CFTypeRef, buf: *const u8, len: CFIndex, dir: bool) -> CFTypeRef;
    fn CFBundleCreate(alloc: CFTypeRef, url: CFTypeRef) -> CFTypeRef;
    fn CFBundleGetIdentifier(b: CFTypeRef) -> CFStringRef;
    fn CFBundleGetValueForInfoDictionaryKey(b: CFTypeRef, key: CFStringRef) -> CFTypeRef;
    fn CFArrayGetCount(a: CFTypeRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(a: CFTypeRef, i: CFIndex) -> CFTypeRef;
    fn CFDictionaryGetValue(d: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, src: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRunInMode(mode: CFStringRef, secs: f64, return_after_source: bool) -> i32;
    fn CFMachPortCreateRunLoopSource(alloc: CFTypeRef, port: CFMachPortRef, order: CFIndex) -> CFRunLoopSourceRef;
    fn CFMachPortInvalidate(port: CFMachPortRef);
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {
    #[link_name = "OBJC_CLASS_$_NSEvent"]
    static NS_EVENT_CLASS: c_void;
}
#[link(name = "objc")]
extern "C" {
    fn sel_registerName(name: *const c_char) -> *const c_void;
    fn objc_msgSend();
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
    fn pthread_main_np() -> i32;
    static _dispatch_main_q: c_void;
    fn dispatch_async_f(queue: *const c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
}

/// AppKit interprets the current event flags. CGEvent's attached Unicode may
/// still describe the modifier state from when an injected event was created.
/// Preserve explicit Unicode strings and bound reads before copying them.
fn delivered_characters(event: CGEventRef) -> Option<([u16; 8], usize)> {
    if unsafe { pthread_main_np() } == 0 {
        // AppKit consults the input-source registry, which is main-queue-only.
        // Do this from the consumer, never block the event tap. A busy UI
        // yields no text rather than stalling capture or inventing characters.
        struct Request { event: Cf, reply: Sender<Option<([u16; 8], usize)>> }
        extern "C" fn decode(context: *mut c_void) {
            let request = unsafe { Box::from_raw(context.cast::<Request>()) };
            let _ = request.reply.send(delivered_characters(request.event.0));
        }
        let (tx, rx) = channel();
        let request = Box::new(Request { event: Cf(unsafe { CFRetain(event) }), reply: tx });
        unsafe { dispatch_async_f(std::ptr::addr_of!(_dispatch_main_q), Box::into_raw(request).cast(), decode) };
        return rx.recv_timeout(Duration::from_millis(100)).ok().flatten();
    }
    struct Pool(*mut c_void);
    impl Drop for Pool { fn drop(&mut self) { unsafe { objc_autoreleasePoolPop(self.0) }; } }
    unsafe {
        let _pool = Pool(objc_autoreleasePoolPush());
        let make: extern "C" fn(*const c_void, *const c_void, CGEventRef) -> *const c_void = std::mem::transmute(objc_msgSend as *const ());
        let get: extern "C" fn(*const c_void, *const c_void) -> CFStringRef = std::mem::transmute(objc_msgSend as *const ());
        let native = make(std::ptr::addr_of!(NS_EVENT_CLASS), sel_registerName(c"eventWithCGEvent:".as_ptr()), event);
        if native.is_null() { return None; }
        let text = get(native, sel_registerName(c"characters".as_ptr()));
        if text.is_null() || CFGetTypeID(text) != CFStringGetTypeID() { return None; }
        let mut buffer = [0; 8];
        let mut count = CFStringGetLength(text).clamp(0, buffer.len() as CFIndex) as usize;
        CFStringGetCharacters(text, CFRange { location: 0, length: count as CFIndex }, buffer.as_mut_ptr());
        if count > 0 && (0xd800..=0xdbff).contains(&buffer[count - 1]) { count -= 1; }
        Some((buffer, count))
    }
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(tap: u32, place: u32, options: u32, mask: u64, cb: TapCallback, user: *mut c_void) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventGetFlags(e: CGEventRef) -> u64;
    fn CGEventCreateCopy(event: CGEventRef) -> CGEventRef;
    fn CGEventGetIntegerValueField(e: CGEventRef, field: u32) -> i64;
    fn CGEventGetLocation(e: CGEventRef) -> CGPoint;
    fn CGSessionCopyCurrentDictionary() -> CFTypeRef;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CFTypeRef;
    fn CGRequestListenEventAccess() -> bool;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(el: AXUIElementRef, attr: CFStringRef, out: *mut CFTypeRef) -> i32;
    fn AXUIElementSetAttributeValue(el: AXUIElementRef, attr: CFStringRef, value: CFTypeRef) -> i32;
    fn AXUIElementCopyElementAtPosition(el: AXUIElementRef, x: f32, y: f32, out: *mut AXUIElementRef) -> i32;
    fn AXUIElementGetPid(el: AXUIElementRef, pid: *mut i32) -> i32;
    fn AXUIElementSetMessagingTimeout(el: AXUIElementRef, secs: f32) -> i32;
    fn _AXUIElementGetWindow(el: AXUIElementRef, id: *mut u32) -> i32;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXObserverCreate(pid: i32, cb: AxCallback, out: *mut AXObserverRef) -> i32;
    fn AXObserverAddNotification(obs: AXObserverRef, el: AXUIElementRef, name: CFStringRef, refcon: *mut c_void) -> i32;
    fn AXObserverGetRunLoopSource(obs: AXObserverRef) -> CFRunLoopSourceRef;
    fn CFRunLoopRemoveSource(rl: CFRunLoopRef, src: CFRunLoopSourceRef, mode: CFStringRef);
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn IsSecureEventInputEnabled() -> bool;
}

extern "C" {
    static kCFBooleanTrue: CFTypeRef;
    fn proc_pidpath(pid: i32, buf: *mut c_void, size: u32) -> i32;
    fn proc_pidinfo(pid: i32, flavor: i32, arg: u64, buf: *mut c_void, size: i32) -> i32;
}

/// When process `pid` started (seconds and microseconds since the epoch, from `proc_bsdinfo`), so a
/// reused pid is told from the process that had it. `None` when no process has the pid.
pub fn proc_start(pid: i32) -> Option<u64> {
    const PROC_PIDTBSDINFO: i32 = 3;
    let mut buf = [0u8; 136];
    let n = unsafe { proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, buf.as_mut_ptr() as *mut c_void, buf.len() as i32) };
    if n < 136 { return None; }
    let sec = u64::from_ne_bytes(buf[120..128].try_into().ok()?);
    let usec = u64::from_ne_bytes(buf[128..136].try_into().ok()?);
    Some(sec * 1_000_000 + usec)
}

const UTF8: u32 = 0x0800_0100;
// Observe after the session has annotated the event's routing information.
const kCGAnnotatedSessionEventTap: u32 = 2;
const kCGHeadInsertEventTap: u32 = 0;
const kCGEventTapOptionListenOnly: u32 = 1;
/// Not a CGEvent type: the focused element of a watched browser changed (an AXObserver).
const AX_FOCUS: u32 = 1000;
const LEFT_DOWN: u32 = 1;
const LEFT_UP: u32 = 2;
const RIGHT_DOWN: u32 = 3;
const LEFT_DRAGGED: u32 = 6;
const KEY_DOWN: u32 = 10;
const OTHER_DOWN: u32 = 25;
const TAP_DISABLED_TIMEOUT: u32 = 0xFFFF_FFFE;
const TAP_DISABLED_USER: u32 = 0xFFFF_FFFF;
const FIELD_KEYCODE: u32 = 9;
const FIELD_CLICK_STATE: u32 = 1;
/// `kCGEventTargetUnixProcessID`: the process the event is routed to.
const FIELD_TARGET_PID: u32 = 40;
const FLAG_SHIFT: u64 = 0x0002_0000;
const FLAG_CTRL: u64 = 0x0004_0000;
const FLAG_ALT: u64 = 0x0008_0000;
const FLAG_CMD: u64 = 0x0010_0000;

/// An owned CF object, released on drop.
struct Cf(CFTypeRef);
impl Drop for Cf { fn drop(&mut self) { if !self.0.is_null() { unsafe { CFRelease(self.0) } } } }
// AX and CF objects are safe to use from one thread at a time; the source moves to the
// host thread once and stays there.
unsafe impl Send for Cf {}

fn cfstr(s: &str) -> Cf { Cf(unsafe { CFStringCreateWithBytes(std::ptr::null(), s.as_ptr(), s.len() as CFIndex, UTF8, false) }) }

fn to_string(s: CFTypeRef) -> Option<String> {
    if s.is_null() || unsafe { CFGetTypeID(s) != CFStringGetTypeID() } { return None; }
    unsafe {
        let len = CFStringGetLength(s);
        let size = CFStringGetMaximumSizeForEncoding(len, UTF8) + 1;
        let mut buf = vec![0u8; size.max(1) as usize];
        if !CFStringGetCString(s, buf.as_mut_ptr(), size, UTF8) { return None; }
        let n = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
        buf.truncate(n);
        String::from_utf8(buf).ok()
    }
}

fn attr(el: AXUIElementRef, name: &str) -> Option<Cf> {
    if el.is_null() { return None; }
    let key = cfstr(name);
    let mut out: CFTypeRef = std::ptr::null();
    let err = unsafe { AXUIElementCopyAttributeValue(el, key.0, &mut out) };
    (err == 0 && !out.is_null()).then_some(Cf(out))
}

fn attr_string(el: AXUIElementRef, name: &str) -> Option<String> {
    let v = attr(el, name)?;
    if unsafe { CFGetTypeID(v.0) == CFURLGetTypeID() } { return to_string(unsafe { CFURLGetString(v.0) }); }
    to_string(v.0).filter(|s| !s.is_empty())
}

/// The pid owning the frontmost normal window (layer 0), from the window server.
fn front_window_pid() -> Option<i32> {
    let list = unsafe { CGWindowListCopyWindowInfo(1 | 16, 0) }; // on screen only, no desktop elements
    if list.is_null() { return None; }
    let list = Cf(list);
    let (layer_key, pid_key) = (cfstr("kCGWindowLayer"), cfstr("kCGWindowOwnerPID"));
    let num = |d: CFTypeRef, k: &Cf| -> Option<i64> {
        let v = unsafe { CFDictionaryGetValue(d, k.0) };
        if v.is_null() { return None; }
        let mut n: i64 = 0;
        unsafe { CFNumberGetValue(v, 4, &mut n as *mut i64 as *mut c_void) }.then_some(n)
    };
    let n = unsafe { CFArrayGetCount(list.0) };
    (0..n).map(|i| unsafe { CFArrayGetValueAtIndex(list.0, i) }).find(|d| num(*d, &layer_key) == Some(0))
        .and_then(|d| num(d, &pid_key)).map(|p| p as i32).filter(|p| *p > 0)
}

/// The focused application's element: from the system-wide element, or, when that fails (seen on
/// macOS 26 test Macs: kAXErrorCannotComplete while per-app reads work), the app owning the
/// frontmost window.
fn focused_app_el(sys: AXUIElementRef) -> Option<Cf> {
    attr(sys, "AXFocusedApplication").or_else(|| front_window_pid().map(|p| Cf(unsafe { AXUIElementCreateApplication(p) })))
}

/// The focused UI element, the same way.
fn focused_el(sys: AXUIElementRef) -> Option<Cf> {
    attr(sys, "AXFocusedUIElement").or_else(|| { let a = focused_app_el(sys)?; attr(a.0, "AXFocusedUIElement") })
}

/// The pid of the process an element belongs to.
fn el_pid(el: AXUIElementRef) -> Option<i32> {
    let mut pid = 0i32;
    (unsafe { AXUIElementGetPid(el, &mut pid) } == 0 && pid > 0).then_some(pid)
}

/// A terminal's text: the focused element's value, or, when focus is on the scroll area around
/// it (Terminal.app on macOS 26), the text area inside it.
fn terminal_text(el: AXUIElementRef) -> Option<String> {
    if let Some(v) = attr_string(el, "AXValue") { return Some(v); }
    let mut level = children(el);
    for _ in 0..2 {
        for c in &level {
            if attr_string(c.0, "AXRole").as_deref() == Some("AXTextArea") { if let Some(v) = attr_string(c.0, "AXValue") { return Some(v); } }
        }
        level = level.iter().flat_map(|c| children(c.0)).take(64).collect();
    }
    None
}

/// What the Accessibility API gives this process about the focused app (for a probe on a test Mac).
pub fn diag() -> String {
    let sys = Cf(unsafe { AXUIElementCreateSystemWide() });
    let app = focused_app_el(sys.0);
    let mut pid = 0i32;
    if let Some(a) = &app { unsafe { AXUIElementGetPid(a.0, &mut pid) }; }
    let win = app.as_ref().and_then(|a| attr(a.0, "AXFocusedWindow"));
    let el = focused_el(sys.0);
    let role = el.as_ref().and_then(|e| attr_string(e.0, "AXRole"));
    let kids: Vec<String> = el.as_ref().map(|e| children(e.0).iter().filter_map(|c| attr_string(c.0, "AXRole")).collect()).unwrap_or_default();
    let text = el.as_ref().and_then(|e| terminal_text(e.0)).map(|t| t.chars().count());
    format!("trusted={} pid={pid} window={} focused_role={role:?} children={kids:?} text_chars={text:?}", unsafe { AXIsProcessTrusted() }, win.is_some())
}

fn children(el: AXUIElementRef) -> Vec<Cf> {
    let Some(arr) = attr(el, "AXChildren") else { return vec![] };
    if unsafe { CFGetTypeID(arr.0) != CFArrayGetTypeID() } { return vec![]; }
    let n = unsafe { CFArrayGetCount(arr.0) };
    (0..n).map(|i| unsafe { CFArrayGetValueAtIndex(arr.0, i) }).filter(|c| !c.is_null())
        .map(|c| { unsafe { CFRetain(c) }; Cf(c) }).collect()
}

extern "C" { fn CFRetain(cf: CFTypeRef) -> CFTypeRef; }

fn target_of(el: AXUIElementRef) -> Target {
    let role = attr_string(el, "AXRole");
    let subrole = attr_string(el, "AXSubrole");
    let secure = role.as_deref() == Some("AXSecureTextField") || subrole.as_deref() == Some("AXSecureTextField");
    let label = if secure { None } else { ["AXTitle", "AXDescription", "AXPlaceholderValue", "AXHelp"].iter().find_map(|a| attr_string(el, a))
        .or_else(|| attr(el, "AXTitleUIElement").and_then(|t| attr_string(t.0, "AXValue"))) };
    let mut pid = 0i32;
    let pid = (unsafe { AXUIElementGetPid(el, &mut pid) } == 0 && pid > 0).then_some(pid as i64);
    let native_window_id = native_window_ancestry(el);
    Target { role, subrole, label, secure, pid, native_window_id }
}

fn native_window_ancestry(el: AXUIElementRef) -> Option<u32> {
    let mut current = Cf(unsafe { CFRetain(el) });
    for _ in 0..32 {
        let role = attr_string(current.0, "AXRole")?;
        if role == "AXWebArea" || role.contains("Secure")
            || attr_string(current.0, "AXSubrole").is_some_and(|s| s.contains("Secure")) { return None; }
        if role == "AXWindow" {
            let mut id = 0u32;
            return (unsafe { _AXUIElementGetWindow(current.0, &mut id) } == 0 && id > 0).then_some(id);
        }
        current = attr(current.0, "AXParent")?;
    }
    None
}

/// Percent-decoding for a `file://` document URL.
fn file_path(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let b = rest.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).ok()?, 16) { out.push(v); i += 3; continue; }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

/// What the tap thread hands over.
/// `target_pid` and `secure` are read in the tap callback, as the event happens: by the time the
/// host thread handles it, a Cmd-Tab may have moved focus on (review round 2, N5).
/// Retain the native element at that same boundary. Looking up current focus
/// while draining the queue attributes an earlier key to the next field.
struct Raw { ty: u32, keycode: i64, flags: u64, at: CGPoint, clicks: i64, when: Ts, target_pid: i32, secure: bool, target: Option<Cf>, keyboard: Option<Cf> }

struct TapCtx { tx: Sender<Raw>, port: AtomicPtr<c_void>, system: Cf }

extern "C" fn tap_cb(_proxy: *const c_void, ty: u32, event: CGEventRef, user: *mut c_void) -> CGEventRef {
    let ctx = unsafe { &*(user as *const TapCtx) };
    if ty == TAP_DISABLED_TIMEOUT || ty == TAP_DISABLED_USER {
        let p = ctx.port.load(Ordering::SeqCst);
        if !p.is_null() { unsafe { CGEventTapEnable(p, true) }; }
        return event;
    }
    let mut raw = Raw { ty, keycode: 0, flags: 0, at: CGPoint::default(), clicks: 0, when: crate::now_secs(), target_pid: 0, secure: false, target: None, keyboard: None };
    unsafe {
        raw.flags = CGEventGetFlags(event);
        raw.at = CGEventGetLocation(event);
        if ty == KEY_DOWN {
            raw.keycode = CGEventGetIntegerValueField(event, FIELD_KEYCODE);
            raw.target_pid = CGEventGetIntegerValueField(event, FIELD_TARGET_PID) as i32;
            raw.secure = secure_input_held();
            if !raw.secure {
                raw.target = focused_el(ctx.system.0);
                if let Some(target) = raw.target.as_ref() {
                    let role = attr_string(target.0, "AXRole");
                    let subrole = attr_string(target.0, "AXSubrole");
                    raw.secure = role.is_none() || role.as_deref() == Some("AXSecureTextField")
                        || subrole.as_deref() == Some("AXSecureTextField");
                }
                if !raw.secure {
                    let copied = CGEventCreateCopy(event);
                    if !copied.is_null() { raw.keyboard = Some(Cf(copied)); }
                }
            }
        } else {
            raw.clicks = CGEventGetIntegerValueField(event, FIELD_CLICK_STATE);
            if matches!(ty, LEFT_DOWN | RIGHT_DOWN | OTHER_DOWN) {
                let mut element = std::ptr::null();
                if AXUIElementCopyElementAtPosition(ctx.system.0, raw.at.x as f32, raw.at.y as f32, &mut element) == 0 && !element.is_null() {
                    raw.target = Some(Cf(element));
                }
            }
        }
    }
    let _ = ctx.tx.send(raw);
    event
}

/// A watched browser moved keyboard focus (Tab into a frame, a page focusing a field): handed over
/// like a tap event, so the page and frames are read again before the next key is judged (round 6, L2).
extern "C" fn ax_focus_cb(_obs: AXObserverRef, el: AXUIElementRef, _name: CFStringRef, user: *mut c_void) {
    let ctx = unsafe { &*(user as *const TapCtx) };
    let mut pid = 0i32;
    unsafe { AXUIElementGetPid(el, &mut pid) };
    let raw = Raw { ty: AX_FOCUS, keycode: 0, flags: 0, at: CGPoint::default(), clicks: 0, when: crate::now_secs(), target_pid: pid, secure: false, target: None, keyboard: None };
    let _ = ctx.tx.send(raw);
}

/// Any process in the session holds secure input (a password field, a terminal's secure keyboard
/// entry, a password manager), as Codex stamps `kCGSSessionSecureInputPID` on every record. While
/// it does, no key and no text is recorded, in whichever app has focus (review delta (c)).
fn secure_input_held() -> bool { let on = unsafe { IsSecureEventInputEnabled() }; on || MacSource::secure_input_pid().is_some() }

struct Tap { stop: Arc<AtomicBool>, thread: Option<std::thread::JoinHandle<()>>, watch: Sender<(i32, u64)> }

impl Tap {
    fn start(tx: Sender<Raw>) -> Result<Tap, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let (ready_tx, ready_rx) = channel::<Result<(), String>>();
        let (watch, watch_rx) = channel::<(i32, u64)>();
        let thread = std::thread::Builder::new().name("skysight-tap".into()).spawn(move || {
            let system = Cf(unsafe { AXUIElementCreateSystemWide() });
            let ctx = Box::into_raw(Box::new(TapCtx { tx, port: AtomicPtr::new(std::ptr::null_mut()), system }));
            let mask: u64 = [LEFT_DOWN, LEFT_UP, RIGHT_DOWN, LEFT_DRAGGED, KEY_DOWN, OTHER_DOWN].iter().map(|t| 1u64 << t).sum();
            let port = unsafe { CGEventTapCreate(kCGAnnotatedSessionEventTap, kCGHeadInsertEventTap, kCGEventTapOptionListenOnly, mask, tap_cb, ctx as *mut c_void) };
            if port.is_null() {
                let _ = ready_tx.send(Err("could not create the event tap: Input Monitoring is not granted to this app".into()));
                drop(unsafe { Box::from_raw(ctx) });
                return;
            }
            unsafe {
                (*ctx).port.store(port as *mut c_void, Ordering::SeqCst);
                let src = CFMachPortCreateRunLoopSource(std::ptr::null(), port, 0);
                CFRunLoopAddSource(CFRunLoopGetCurrent(), src, kCFRunLoopCommonModes);
                CGEventTapEnable(port, true);
                let _ = ready_tx.send(Ok(()));
                // browsers' focus observers, by pid and process start (round 7, L4: removed when the
                // browser exits, and a reused pid gets a new one)
                let mut observers: stream::Watches<AXObserverRef> = Default::default();
                let release = |o: AXObserverRef| { CFRunLoopRemoveSource(CFRunLoopGetCurrent(), AXObserverGetRunLoopSource(o), kCFRunLoopDefaultMode); CFRelease(o); };
                let mut pruned = Instant::now();
                while !flag.load(Ordering::SeqCst) {
                    // browsers to watch for focus changes, added by the host thread as they come to the front
                    while let Ok((pid, start)) = watch_rx.try_recv() {
                        let old = observers.add(pid, start, || {
                            let mut obs: AXObserverRef = std::ptr::null();
                            if AXObserverCreate(pid, ax_focus_cb, &mut obs) != 0 || obs.is_null() { return None; }
                            let app = Cf(AXUIElementCreateApplication(pid));
                            let name = cfstr("AXFocusedUIElementChanged");
                            if AXObserverAddNotification(obs, app.0, name.0, ctx as *mut c_void) == 0 {
                                CFRunLoopAddSource(CFRunLoopGetCurrent(), AXObserverGetRunLoopSource(obs), kCFRunLoopDefaultMode);
                                Some(obs)
                            } else { CFRelease(obs); None }
                        });
                        old.into_iter().for_each(release);
                    }
                    if pruned.elapsed() >= Duration::from_secs(2) {
                        pruned = Instant::now();
                        observers.prune(proc_start).into_iter().for_each(release);
                    }
                    CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.1, false);
                }
                observers.drain().into_iter().for_each(release);
                CGEventTapEnable(port, false);
                CFMachPortInvalidate(port);
                CFRelease(src);
                CFRelease(port);
                drop(Box::from_raw(ctx));
            }
        }).map_err(|e| e.to_string())?;
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Tap { stop, thread: Some(thread), watch }),
            Ok(Err(e)) => { let _ = thread.join(); Err(e) }
            Err(_) => { stop.store(true, Ordering::SeqCst); Err("the event tap did not start".into()) }
        }
    }
    fn stop(&mut self) { self.stop.store(true, Ordering::SeqCst); if let Some(t) = self.thread.take() { let _ = t.join(); } }
}

impl Drop for Tap { fn drop(&mut self) { self.stop(); } }

fn key_of(code: i64) -> (Key, &'static str) {
    match code {
        36 | 76 => (Key::Return, "return"), 48 => (Key::Tab, "tab"), 51 => (Key::Backspace, "delete"), 53 => (Key::Escape, "escape"),
        117 => (Key::ForwardDelete, "forwarddelete"), 123 => (Key::Arrow, "left"), 124 => (Key::Arrow, "right"), 125 => (Key::Arrow, "down"), 126 => (Key::Arrow, "up"),
        49 => (Key::Char, "space"),
        0 => (Key::Char, "a"), 1 => (Key::Char, "s"), 2 => (Key::Char, "d"), 3 => (Key::Char, "f"), 4 => (Key::Char, "h"), 5 => (Key::Char, "g"), 6 => (Key::Char, "z"),
        7 => (Key::Char, "x"), 8 => (Key::Char, "c"), 9 => (Key::Char, "v"), 11 => (Key::Char, "b"), 12 => (Key::Char, "q"), 13 => (Key::Char, "w"), 14 => (Key::Char, "e"),
        15 => (Key::Char, "r"), 16 => (Key::Char, "y"), 17 => (Key::Char, "t"), 31 => (Key::Char, "o"), 32 => (Key::Char, "u"), 34 => (Key::Char, "i"), 35 => (Key::Char, "p"),
        37 => (Key::Char, "l"), 38 => (Key::Char, "j"), 40 => (Key::Char, "k"), 45 => (Key::Char, "n"), 46 => (Key::Char, "m"),
        18 => (Key::Char, "1"), 19 => (Key::Char, "2"), 20 => (Key::Char, "3"), 21 => (Key::Char, "4"), 23 => (Key::Char, "5"), 22 => (Key::Char, "6"),
        26 => (Key::Char, "7"), 28 => (Key::Char, "8"), 25 => (Key::Char, "9"), 29 => (Key::Char, "0"),
        96..=111 | 118..=122 => (Key::Other, "f-key"),
        _ => (Key::Char, ""),
    }
}

enum Task { Selection, Terminal(Cf, u32) }

#[derive(Clone, PartialEq, Eq)]
struct Front { pid: i32, window: Option<u32>, title: Option<String> }

/// The macOS [`Source`].
pub struct MacSource {
    tap: Option<Tap>,
    rx: Option<Receiver<Raw>>,
    lookahead: Option<Raw>,
    system: Option<Cf>,
    queue: VecDeque<(Observation, Ts)>,
    scheduled: Vec<(Instant, Task)>,
    last_poll: Instant,
    /// App identities by pid and process start (fresh review, F5).
    apps: stream::Watches<AppInfo>,
    front: Option<(Front, AppInfo, Option<WindowInfo>)>,
    url_checked: Instant,
    locked: Option<bool>,
    down: Option<(CGPoint, i64, bool, Option<Cf>)>,
    manual_ax: stream::Watches<()>,
    /// Browsers whose focus changes the tap thread observes.
    watched: BTreeMap<i32, u64>,
    /// Terminal windows read since their last Return: the next key in one that is not is preceded
    /// by a read, so the recorder sees the bare prompt the line is typed after (fresh review 3).
    term_read: std::collections::BTreeSet<u32>,
}

unsafe impl Send for MacSource {}

impl Default for MacSource { fn default() -> Self { MacSource::new() } }

impl MacSource {
    pub fn new() -> MacSource {
        MacSource { tap: None, rx: None, lookahead: None, system: None, queue: VecDeque::new(), scheduled: Vec::new(), last_poll: Instant::now() - Duration::from_secs(10),
            apps: Default::default(), front: None, url_checked: Instant::now(), locked: None, down: None, manual_ax: Default::default(), watched: Default::default(), term_read: Default::default() }
    }

    fn sys(&self) -> AXUIElementRef { self.system.as_ref().map(|s| s.0).unwrap_or(std::ptr::null()) }

    fn app_info(&mut self, pid: i32) -> AppInfo {
        // no start time (the process is gone or unreadable): read afresh and keep nothing (re-review 8)
        let start = proc_start(pid);
        if let Some(a) = start.and_then(|st| self.apps.get(pid, st)) { return a.clone(); }
        let mut buf = vec![0u8; 4096];
        let n = unsafe { proc_pidpath(pid, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) };
        let exe = (n > 0).then(|| String::from_utf8_lossy(&buf[..n as usize]).to_string());
        let bundle_path = exe.as_deref().and_then(|p| p.find(".app/").map(|i| p[..i + 4].to_string()));
        let mut info = AppInfo { pid: pid as i64, path: bundle_path.clone().or(exe.clone()), ..Default::default() };
        info.name = exe.as_deref().and_then(|p| p.rsplit('/').next()).unwrap_or("").to_string();
        if let Some(bp) = bundle_path {
            unsafe {
                let url = Cf(CFURLCreateFromFileSystemRepresentation(std::ptr::null(), bp.as_ptr(), bp.len() as CFIndex, true));
                if !url.0.is_null() {
                    let b = Cf(CFBundleCreate(std::ptr::null(), url.0));
                    if !b.0.is_null() {
                        info.bundle_id = to_string(CFBundleGetIdentifier(b.0));
                        let val = |k: &str| { let key = cfstr(k); CFBundleGetValueForInfoDictionaryKey(b.0, key.0) };
                        let truthy = |v: CFTypeRef| !v.is_null() && ((CFGetTypeID(v) == CFBooleanGetTypeID() && CFBooleanGetValue(v))
                            || to_string(v).is_some_and(|s| s == "1" || s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("yes")));
                        info.background_only = truthy(val("LSBackgroundOnly")) || truthy(val("LSUIElement"));
                        if let Some(n) = to_string(val("CFBundleDisplayName")).or_else(|| to_string(val("CFBundleName"))) { info.name = n; }
                    }
                }
            }
            if info.name.is_empty() { info.name = bp.rsplit('/').next().unwrap_or("").trim_end_matches(".app").to_string(); }
        }
        if let Some(st) = start { self.apps.add(pid, st, || Some(info.clone())); }
        info
    }

    /// The process holding session secure input, if any (`kCGSSessionSecureInputPID`).
    fn secure_input_pid() -> Option<i64> {
        unsafe {
            let d = Cf(CGSessionCopyCurrentDictionary());
            if d.0.is_null() { return None; }
            let key = cfstr("kCGSSessionSecureInputPID");
            let v = CFDictionaryGetValue(d.0, key.0);
            if v.is_null() || CFGetTypeID(v) != CFNumberGetTypeID() { return None; }
            let mut n = 0i64;
            (CFNumberGetValue(v, 4, &mut n as *mut i64 as *mut c_void) && n > 0).then_some(n)
        }
    }

    fn screen_locked() -> bool {
        unsafe {
            let d = Cf(CGSessionCopyCurrentDictionary());
            if d.0.is_null() { return false; }
            let key = cfstr("CGSSessionScreenIsLocked");
            let v = CFDictionaryGetValue(d.0, key.0);
            !v.is_null() && CFGetTypeID(v) == CFBooleanGetTypeID() && CFBooleanGetValue(v)
        }
    }

    /// Labels on a window's toolbar and tab bar (for the private-window check).
    fn chrome_labels(window: AXUIElementRef) -> Vec<String> {
        let mut out = Vec::new();
        let mut q: VecDeque<(Cf, usize)> = children(window).into_iter().map(|c| (c, 1)).collect();
        let mut seen = 0;
        while let Some((c, depth)) = q.pop_front() {
            seen += 1;
            if seen > 200 { break; }
            let role = attr_string(c.0, "AXRole").unwrap_or_default();
            if role == "AXWebArea" || role == "AXScrollArea" { continue; }
            for a in ["AXTitle", "AXDescription", "AXPlaceholderValue", "AXHelp"] { if let Some(s) = attr_string(c.0, a) { out.push(s); } }
            if role == "AXStaticText" { if let Some(s) = attr_string(c.0, "AXValue") { if s.len() < 40 { out.push(s); } } }
            if depth < 5 { q.extend(children(c.0).into_iter().map(|k| (k, depth + 1))); }
        }
        out
    }

    /// Before a key or a click is queued: the app it goes to must be the one the recorder was told
    /// is in front. The focus poll runs every 500 ms, so a Cmd-Tab (or a click into another app's
    /// window) and a key right after it would otherwise be filed, and judged, under the app that
    /// was in front before. When `pid` is not the app last polled, poll now; when the poll still
    /// does not show it (a click on a window behind), report that app with no window known. In a
    /// browser the URL is read again (at most every 500 ms), so a navigation that kept the title
    /// is seen before the key is judged.
    fn ensure_front(&mut self, pid: Option<i32>, now: Ts) {
        let front = self.front.as_ref().map(|(f, _, _)| f.pid);
        let browser = self.front.as_ref().is_some_and(|(_, a, _)| stream::is_browser(a.bundle_id.as_deref()));
        let Some(pid) = pid else { if front.is_none() { self.poll_focus(now, false); } return };
        if front != Some(pid) {
            self.poll_focus(now, true);
            if self.front.as_ref().map(|(f, _, _)| f.pid) != Some(pid) {
                let app = self.app_info(pid);
                self.front = Some((Front { pid, window: None, title: None }, app.clone(), None));
                self.queue.push_back((Observation::Focus { app, window: None }, now));
            }
        } else if browser && self.url_checked.elapsed() >= Duration::from_millis(500) {
            self.poll_focus(now, true);
        }
    }

    fn poll_focus(&mut self, now: Ts, force_url: bool) {
        let locked = Self::screen_locked();
        if self.locked != Some(locked) { self.locked = Some(locked); self.queue.push_back((Observation::Locked(locked), now)); }
        if locked { return; }
        let Some(app_el) = focused_app_el(self.sys()) else { return };
        let mut pid = 0i32;
        if unsafe { AXUIElementGetPid(app_el.0, &mut pid) } != 0 || pid <= 0 { return; }
        let app = self.app_info(pid);
        let browser = stream::is_browser(app.bundle_id.as_deref());
        if browser && app.bundle_id.as_deref().is_some_and(|b| b != "com.apple.Safari" && b != "com.apple.SafariTechnologyPreview" && b != "org.mozilla.firefox")
            && proc_start(pid).is_some_and(|st| self.manual_ax.get(pid, st).is_none() && { self.manual_ax.add(pid, st, || Some(())); true }) {
            let key = cfstr("AXManualAccessibility");
            unsafe { AXUIElementSetAttributeValue(app_el.0, key.0, kCFBooleanTrue) };
        }
        if browser {
            // a pid whose process changed (the browser quit and the pid went to another) is watched anew
            if let Some(start) = proc_start(pid).filter(|st| self.watched.get(&pid) != Some(st)) {
                self.watched.retain(|p, st| proc_start(*p) == Some(*st));
                self.watched.insert(pid, start);
                if let Some(t) = &self.tap { let _ = t.watch.send((pid, start)); }
            }
        }
        let win = attr(app_el.0, "AXFocusedWindow");
        let mut window = None;
        let mut front = Front { pid, window: None, title: None };
        if let Some(w) = &win {
            let mut id = 0u32;
            unsafe { _AXUIElementGetWindow(w.0, &mut id) };
            let title = attr_string(w.0, "AXTitle");
            front.window = Some(id);
            front.title = title.clone();
            let changed = self.front.as_ref().map(|(f, _, _)| f) != Some(&front);
            let recheck = browser && (force_url || self.url_checked.elapsed() >= Duration::from_secs(3));
            if !changed && !recheck {
                window = self.front.as_ref().and_then(|(_, _, w)| w.clone());
            } else {
                let document = attr_string(w.0, "AXDocument").and_then(|u| file_path(&u));
                let (url, private) = if browser {
                    self.url_checked = Instant::now();
                    // The page the focused element is in, by walking up from it (a few AX calls); else
                    // the window's web area, found by a bounded walk down. Not a web area kept from
                    // an earlier read: on the Mac run a new tab with the same title kept answering
                    // with the old tab's URL, and text typed on a blocked site was filed under the
                    // open one.
                    // Every frame on the walk up is judged too, and the page's URL is the one kept
                    // (round 5, M1). The walk down takes the window's only page, none when there
                    // are several (round 5, L2: a side panel or a docked inspector).
                    let page = self.focused_page_urls(pid).unwrap_or_else(|| stream::PageUrls {
                        url: stream::sole_web_area(w, 600, 14, |c: &Cf| children(c.0), |c: &Cf| attr_string(c.0, "AXRole").as_deref() == Some("AXWebArea"))
                            .and_then(|wa| attr_string(wa.0, "AXURL")),
                        frames: Vec::new(),
                    });
                    (page, stream::looks_private(title.as_deref(), &Self::chrome_labels(w.0)))
                } else { (stream::PageUrls::default(), false) };
                window = Some(WindowInfo { id, title, url: url.url, document, private, frame_urls: url.frames });
            }
            // A terminal window coming to the front: read its text as the baseline.
            if changed && stream::is_terminal(app.bundle_id.as_deref()) {
                if let Some(el) = focused_el(self.sys()).filter(|e| stream::ax_element_is_front(el_pid(e.0), Some(pid))) {
                    self.scheduled.push((Instant::now(), Task::Terminal(el, id)));
                }
            }
        }
        self.front = Some((front, app.clone(), window.clone()));
        self.queue.push_back((Observation::Focus { app, window }, now));
    }

    /// The page and frame URLs around the focused element of app `pid` ([`stream::page_urls_up`],
    /// at most 120 parents). `None` when focus is not in a page (the address bar, a start page).
    fn focused_page_urls(&self, pid: i32) -> Option<stream::PageUrls> {
        let el = focused_el(self.sys())?;
        let mut owner = 0i32;
        if unsafe { AXUIElementGetPid(el.0, &mut owner) } != 0 || owner != pid { return None; }
        Self::page_urls_of(el)
    }

    fn page_urls_of(el: Cf) -> Option<stream::PageUrls> {
        stream::page_urls_up(el, 120, |e: &Cf| attr_string(e.0, "AXRole").as_deref() == Some("AXWebArea"),
            |e: &Cf| attr_string(e.0, "AXURL"), |e: &Cf| attr(e.0, "AXParent"))
    }

    /// A click in a browser: the page and frames around the element clicked, when they differ from
    /// what the last poll saw (a click into a frame of another site while focus was elsewhere), are
    /// sent as a focus change first, so the click is judged by them.
    fn note_click_page(&mut self, at: CGPoint, now: Ts) {
        let Some((f, app, Some(w))) = self.front.clone() else { return };
        if !stream::is_browser(app.bundle_id.as_deref()) { return; }
        let mut el: AXUIElementRef = std::ptr::null();
        let err = unsafe { AXUIElementCopyElementAtPosition(self.sys(), at.x as f32, at.y as f32, &mut el) };
        if err != 0 || el.is_null() { return; }
        let el = Cf(el);
        let mut owner = 0i32;
        if unsafe { AXUIElementGetPid(el.0, &mut owner) } != 0 || owner != f.pid { return; }
        let Some(page) = Self::page_urls_of(el) else { return };
        if page.url == w.url && page.frames == w.frame_urls { return; }
        let window = WindowInfo { url: page.url, frame_urls: page.frames, ..w };
        self.front = Some((f, app.clone(), Some(window.clone())));
        self.queue.push_back((Observation::Focus { app, window: Some(window) }, now));
    }

    /// The focused element, only when it belongs to the app the recorder was told is in front
    /// (round 7: the fallback for a failing system-wide element takes the frontmost window's owner,
    /// which may not be the active app).
    fn front_el(&self) -> Option<Cf> {
        let front = self.front.as_ref().map(|(f, _, _)| f.pid);
        focused_el(self.sys()).filter(|e| stream::ax_element_is_front(el_pid(e.0), front))
    }

    /// Preserve input order while checking whether an idle flush is safe.
    /// A delayed consumer must not flush a prefix while the tap still has the
    /// suffix queued with its original source time.
    pub fn has_pending(&mut self) -> bool {
        if !self.queue.is_empty() || self.lookahead.is_some() { return true; }
        self.lookahead = self.rx.as_ref().and_then(|rx| rx.try_recv().ok());
        self.lookahead.is_some()
    }

    /// Read the focused terminal's text into the queue now.
    fn read_terminal_now(&mut self, wid: u32, now: Ts) {
        if let Some(el) = self.front_el() {
            // a busy terminal must not stall the tap's queue: a quarter second, then no read (the line is dropped)
            unsafe { AXUIElementSetMessagingTimeout(el.0, 0.25) };
            if let Some(v) = terminal_text(el.0) {
                let start = v.char_indices().rev().nth(20_000).map(|(i, _)| i).unwrap_or(0);
                self.queue.push_back((Observation::TerminalValue { window_id: wid, value: v[start..].to_string() }, now));
            }
        }
    }

    fn handle(&mut self, r: Raw) {
        let now = r.when;
        match r.ty {
            KEY_DOWN => {
                // The first key of a terminal line: read the terminal before anything else, for the
                // prompt the line is typed after (fresh review 3, item 1). Round 4, item 1: taken after
                // the focus and target round trips, the read could already show a key or two.
                {
                    let (key, name) = key_of(r.keycode);
                    let ctrl = r.flags & FLAG_CTRL != 0;
                    let ends = key == Key::Return || (ctrl && matches!(name, "c" | "d"));
                    let secure = r.secure || secure_input_held();
                    if let Some(wid) = self.front.as_ref().filter(|(_, a, _)| stream::is_terminal(a.bundle_id.as_deref())).map(|(f, _, _)| f.window.unwrap_or(0)) {
                        if !secure && !ends && self.term_read.insert(wid) { self.read_terminal_now(wid, now); }
                    }
                }
                // Resolve only the retained source element, never the field
                // that happens to be focused after this event was queued.
                let captured_pid = r.target.as_ref().and_then(|e| el_pid(e.0));
                let pid = if r.target_pid > 0 { Some(r.target_pid) } else { captured_pid };
                self.ensure_front(pid, now);
                let mods = Mods { cmd: r.flags & FLAG_CMD != 0, ctrl: r.flags & FLAG_CTRL != 0, alt: r.flags & FLAG_ALT != 0, shift: r.flags & FLAG_SHIFT != 0 };
                let secure_input = r.secure || secure_input_held();
                let (key, name) = key_of(r.keycode);
                let chars = if secure_input { String::new() } else { r.keyboard.as_ref()
                    .and_then(|event| delivered_characters(event.0))
                    .map(|(chars, count)| String::from_utf16_lossy(&chars[..count])).unwrap_or_default() };
                let target = if secure_input || captured_pid != pid { None }
                    else { r.target.as_ref().map(|e| target_of(e.0)) };
                if (mods.shift && key == Key::Arrow) || (mods.cmd && name == "a") { self.scheduled.push((Instant::now() + Duration::from_millis(150), Task::Selection)); }
                let terminal = self.front.as_ref().is_some_and(|(_, a, _)| stream::is_terminal(a.bundle_id.as_deref()));
                let wid = self.front.as_ref().and_then(|(f, _, _)| f.window).unwrap_or(0);
                // a line ends: a Return, or Control-C / Control-D
                let ends = key == Key::Return || (mods.ctrl && matches!(name, "c" | "d"));
                // a terminal that came to the front with this key: read now, late (the recorder takes
                // echoed keys off the prompt's end)
                if terminal && !secure_input && !ends && self.term_read.insert(wid) { self.read_terminal_now(wid, now); }
                if terminal && ends { self.term_read.remove(&wid); }
                if key == Key::Return && !secure_input && terminal {
                    // Read the terminal now, before the Return is judged: the line typed is taken
                    // from this read, after the prompt read before its first key (review round 2,
                    // N3/N6; fresh review 3). Later reads diff the output.
                    self.read_terminal_now(wid, now);
                    for ms in [1200u64, 4000] {
                        if let Some(el) = self.front_el() { self.scheduled.push((Instant::now() + Duration::from_millis(ms), Task::Terminal(el, wid))); }
                    }
                }
                self.queue.push_back((Observation::Key { chars, key, mods, target, secure_input, key_name: name.into() }, now));
            }
            AX_FOCUS => {
                if self.front.as_ref().is_some_and(|(f, _, _)| f.pid == r.target_pid) { self.poll_focus(now, true); }
            }
            LEFT_DOWN => { self.down = Some((r.at, r.clicks, false, r.target)); }
            LEFT_DRAGGED => { if let Some(d) = &mut self.down { if (d.0.x - r.at.x).abs() + (d.0.y - r.at.y).abs() > 6.0 { d.2 = true; } } }
            LEFT_UP => {
                let Some((at, clicks, dragged, captured)) = self.down.take() else { return };
                let pid = captured.as_ref().and_then(|e| el_pid(e.0));
                self.ensure_front(pid, now);
                self.note_click_page(at, now);
                let target = captured.as_ref().map(|e| target_of(e.0));
                let ctrl = r.flags & FLAG_CTRL != 0;
                if dragged {
                    self.queue.push_back((Observation::Drag { target }, now));
                    self.scheduled.push((Instant::now() + Duration::from_millis(150), Task::Selection));
                } else {
                    self.queue.push_back((Observation::Click { button: Button::Left, count: clicks.max(1) as u32, ctrl, target }, now));
                    if clicks >= 2 { self.scheduled.push((Instant::now() + Duration::from_millis(150), Task::Selection)); }
                }
            }
            RIGHT_DOWN | OTHER_DOWN => {
                let pid = r.target.as_ref().and_then(|e| el_pid(e.0));
                self.ensure_front(pid, now);
                self.note_click_page(r.at, now);
                let target = r.target.as_ref().map(|e| target_of(e.0));
                let button = if r.ty == RIGHT_DOWN { Button::Right } else { Button::Other };
                self.queue.push_back((Observation::Click { button, count: r.clicks.max(1) as u32, ctrl: false, target }, now));
            }
            _ => {}
        }
    }

    fn run_due(&mut self) {
        let now = Instant::now();
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.scheduled).into_iter().partition(|(t, _)| *t <= now);
        self.scheduled = later;
        for (_, task) in due {
            let ts = crate::now_secs();
            match task {
                Task::Selection => {
                    if secure_input_held() { continue; }
                    if let Some(el) = focused_el(self.sys()) {
                        // The selection is read 150 ms after the gesture: focus may have moved to
                        // another app by then, so the app it is in is brought up to date first and
                        // the selection is judged as that app's (fresh review, F7).
                        let mut pid = 0i32;
                        if unsafe { AXUIElementGetPid(el.0, &mut pid) } != 0 || pid <= 0 { continue; }
                        self.ensure_front(Some(pid), ts);
                        let t = target_of(el.0);
                        if t.is_secure() { continue; }
                        if let Some(text) = attr_string(el.0, "AXSelectedText") { self.queue.push_back((Observation::Selection { text, target: Some(t) }, ts)); }
                    }
                }
                Task::Terminal(el, wid) => {
                    // still the app and window in front (the read is taken later than it was planned)
                    let front = self.front.as_ref().map(|(f, _, _)| (f.pid, f.window.unwrap_or(0)));
                    if !stream::ax_element_is_front(el_pid(el.0), front.map(|f| f.0)) || front.map(|f| f.1) != Some(wid) { continue; }
                    if let Some(v) = terminal_text(el.0) {
                        let start = v.char_indices().rev().nth(20_000).map(|(i, _)| i).unwrap_or(0);
                        self.queue.push_back((Observation::TerminalValue { window_id: wid, value: v[start..].to_string() }, ts));
                    }
                }
            }
        }
    }
}

/// Ask macOS for Input Monitoring (shows the system prompt once); call only on a user's request.
pub fn request_input_monitoring() -> bool { unsafe { CGRequestListenEventAccess() } }

pub fn permissions() -> Permissions {
    Permissions { accessibility: Some(unsafe { AXIsProcessTrusted() }), input_monitoring: Some(unsafe { CGPreflightListenEventAccess() }) }
}

impl Source for MacSource {
    fn platform(&self) -> &'static str { "macos" }
    fn permissions(&self) -> Permissions { permissions() }

    fn start(&mut self) -> Result<(), String> {
        if !unsafe { AXIsProcessTrusted() } { return Err("Accessibility is not granted to this app".into()); }
        let (tx, rx) = channel();
        self.tap = Some(Tap::start(tx)?);
        self.rx = Some(rx);
        let sys = Cf(unsafe { AXUIElementCreateSystemWide() });
        unsafe { AXUIElementSetMessagingTimeout(sys.0, 0.5) };
        self.system = Some(sys);
        self.front = None;
        self.locked = None;
        self.queue.clear();
        self.lookahead = None;
        self.scheduled.clear();
        self.watched.clear();
        // a new session: the first key of every terminal line is read before again (round 6)
        self.term_read.clear();
        Ok(())
    }

    fn next(&mut self, timeout: Duration) -> Option<(Observation, Ts)> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(o) = self.queue.pop_front() { return Some(o); }
            if let Some(raw) = self.lookahead.take() { self.handle(raw); continue; }
            if self.last_poll.elapsed() >= Duration::from_millis(500) {
                self.poll_focus(crate::now_secs(), false);
                // AX can take longer than the polling interval. Starting the
                // interval before that read can starve the queued key stream.
                self.last_poll = Instant::now();
                continue;
            }
            self.run_due();
            if !self.queue.is_empty() { continue; }
            let left = deadline.checked_duration_since(Instant::now())?;
            let wait = left.min(Duration::from_millis(100));
            match self.rx.as_ref()?.recv_timeout(wait) {
                Ok(raw) => self.handle(raw),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return None,
            }
        }
    }

    fn stop(&mut self) {
        if let Some(mut t) = self.tap.take() { t.stop(); }
        self.rx = None;
        self.lookahead = None;
        self.scheduled.clear();
        self.queue.clear();
        self.system = None;
    }
}

#[cfg(test)]
mod event_character_tests {
    use super::*;
    extern "C" {
        fn CGEventCreateKeyboardEvent(source: CFTypeRef, key: u16, down: bool) -> CGEventRef;
        fn CGEventSetFlags(event: CGEventRef, flags: u64);
        fn CGEventKeyboardSetUnicodeString(event: CGEventRef, length: usize, text: *const u16);
    }
    fn event(key: u16, flags: u64) -> Cf {
        // Creating AppKit keyboard events can initialize WindowServer/AppKit
        // state even when no input is posted. Keep this off a working desktop.
        assert_eq!(std::env::var("ICU_DISPOSABLE_HOST").as_deref(), Ok("1"));
        assert_eq!(std::env::var("ICU_E2E_NATIVE_EVENT_CHARACTERS").as_deref(), Ok("1"));
        assert!(!std::path::Path::new("/Users/nathanzhao").exists());
        assert!(!std::env::var("HOME").unwrap_or_default().starts_with("/Users/nathanzhao"));
        let event = Cf(unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), key, true) });
        assert!(!event.0.is_null());
        unsafe { CGEventSetFlags(event.0, flags) };
        event
    }
    fn read(event: &Cf) -> String {
        let (text, count) = delivered_characters(event.0).expect("Native keyboard event characters");
        String::from_utf16(&text[..count]).expect("Complete Unicode scalars")
    }
    #[test]
    #[ignore = "requires explicit disposable macOS GUI host and application main dispatch loop"]
    fn interpreted_characters_use_current_flags_without_posting_input() {
        assert_eq!(read(&event(46, FLAG_SHIFT)), "M");
        assert_eq!(read(&event(0, FLAG_CTRL)), "\u{1}");
        assert_eq!(read(&event(0, 0)), "a");
    }
    #[test]
    #[ignore = "requires explicit disposable macOS GUI host and application main dispatch loop"]
    fn interpreted_characters_preserve_explicit_unicode_without_posting_input() {
        let event = event(0, 0);
        let text: Vec<u16> = "桜🙂".encode_utf16().collect();
        unsafe { CGEventKeyboardSetUnicodeString(event.0, text.len(), text.as_ptr()) };
        assert_eq!(read(&event), "桜🙂");
    }
    #[test]
    #[ignore = "requires explicit disposable macOS GUI host and application main dispatch loop"]
    fn character_limit_does_not_split_a_surrogate_pair() {
        let event = event(0, 0);
        let text: Vec<u16> = "abcdefg🙂".encode_utf16().collect();
        unsafe { CGEventKeyboardSetUnicodeString(event.0, text.len(), text.as_ptr()) };
        assert_eq!(read(&event), "abcdefg");
    }
}
