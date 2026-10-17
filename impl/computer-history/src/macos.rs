//! Bounded native focus sampling. No event injection, screenshots, or raw keyboard hooks.
//! AX secure-field and polling boundaries follow the taiku observer design. Identity uses
//! the native CGWindowID, never a title match. Browser context has separate host consent.
#[path = "macos_browser.rs"]
mod browser_native;
use crate::{
    capture::{Content, Source},
    policy::{browser, Context, Website},
    store::CaptureMode,
    Error, Result,
};
pub use browser_native::request_browser_permission;
use std::{
    ffi::{c_char, c_void, CString},
    ptr,
};
type Ref = *const c_void;
#[repr(C)]
struct ProcessSerialNumber {
    high: u32,
    low: u32,
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(value: Ref);
    fn CFRetain(value: Ref) -> Ref;
    fn CFEqual(a: Ref, b: Ref) -> u8;
    fn CFGetTypeID(value: Ref) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFURLGetTypeID() -> usize;
    fn CFURLGetString(url: Ref) -> Ref;
    fn CFStringCreateWithCString(alloc: Ref, text: *const c_char, encoding: u32) -> Ref;
    fn CFStringGetLength(value: Ref) -> isize;
    fn CFStringGetCString(value: Ref, buffer: *mut c_char, size: isize, encoding: u32) -> u8;
    fn CFDictionaryGetValue(value: Ref, key: Ref) -> Ref;
    fn CFBooleanGetTypeID() -> usize;
    fn CFBooleanGetValue(value: Ref) -> u8;
    fn CFArrayGetCount(value: Ref) -> isize;
    fn CFArrayGetTypeID() -> usize;
    fn CFArrayGetValueAtIndex(value: Ref, index: isize) -> Ref;
    fn CFNumberGetValue(value: Ref, ty: isize, out: *mut c_void) -> u8;
    static kCFBooleanTrue: Ref;
}
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> Ref;
    fn AXUIElementCreateSystemWide() -> Ref;
    fn AXUIElementGetTypeID() -> usize;
    fn AXUIElementCopyAttributeValue(element: Ref, name: Ref, value: *mut Ref) -> i32;
    fn AXUIElementSetMessagingTimeout(element: Ref, timeout: f32) -> i32;
    fn AXUIElementSetAttributeValue(element: Ref, name: Ref, value: Ref) -> i32;
    fn AXUIElementGetPid(element: Ref, pid: *mut i32) -> i32;
    fn _AXUIElementGetWindow(element: Ref, window: *mut u32) -> i32;
    fn CGWindowListCopyWindowInfo(options: u32, relative: u32) -> Ref;
    fn CGSessionCopyCurrentDictionary() -> Ref;
    fn GetFrontProcess(process: *mut ProcessSerialNumber) -> i16;
    fn GetProcessPID(process: *const ProcessSerialNumber, pid: *mut i32) -> i32;
}
#[link(name = "AppKit", kind = "framework")]
extern "C" {}
#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> Ref;
    fn sel_registerName(name: *const c_char) -> Ref;
    fn objc_msgSend();
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
}
const UTF8: u32 = 0x08000100;
struct Owned(Ref);
impl Owned {
    fn from(value: Ref) -> Option<Self> {
        (!value.is_null()).then_some(Self(value))
    }
    fn string(value: &str) -> Self {
        let s = CString::new(value).expect("constant attribute");
        Self(unsafe { CFStringCreateWithCString(ptr::null(), s.as_ptr(), UTF8) })
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) }
        }
    }
}
struct Pool(*mut c_void);
impl Pool {
    fn new() -> Self {
        Self(unsafe { objc_autoreleasePoolPush() })
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        unsafe { objc_autoreleasePoolPop(self.0) }
    }
}
unsafe fn send(receiver: Ref, selector: &str) -> Ref {
    let name = CString::new(selector).unwrap();
    let call: unsafe extern "C" fn(Ref, Ref) -> Ref =
        std::mem::transmute(objc_msgSend as *const ());
    call(receiver, sel_registerName(name.as_ptr()))
}
fn string(value: Ref, limit: usize) -> Option<String> {
    if value.is_null() {
        return None;
    }
    unsafe {
        if CFGetTypeID(value) != CFStringGetTypeID() {
            return None;
        }
        let length = CFStringGetLength(value);
        // Bound allocation even if an application exposes its entire document as a value.
        if !(0..=65536).contains(&length) {
            return None;
        }
        let mut bytes = vec![0u8; length as usize * 4 + 1];
        if CFStringGetCString(value, bytes.as_mut_ptr().cast(), bytes.len() as isize, UTF8) == 0 {
            return None;
        }
        let length = bytes.iter().position(|v| *v == 0)?;
        Some(
            std::str::from_utf8(&bytes[..length])
                .ok()?
                .chars()
                .take(limit)
                .collect(),
        )
    }
}
fn attr(element: Ref, key: &str) -> Option<Owned> {
    attr_result(element, key).ok()
}
fn attr_result(element: Ref, key: &str) -> std::result::Result<Owned, i32> {
    let key = Owned::string(key);
    let mut value = ptr::null();
    let error = unsafe { AXUIElementCopyAttributeValue(element, key.0, &mut value) };
    let owned = Owned::from(value);
    if error == 0 {
        owned.ok_or(-25212)
    } else {
        Err(error)
    }
}
fn text_attr(element: Ref, key: &str, cap: usize) -> Option<String> {
    string(attr(element, key)?.0, cap)
}
fn page_url(element: Ref) -> Option<String> {
    let value = attr(element, "AXURL")?;
    let raw = if unsafe { CFGetTypeID(value.0) == CFURLGetTypeID() } {
        unsafe { CFURLGetString(value.0) }
    } else {
        value.0
    };
    crate::browser::document_url(&string(raw, 16385)?)
}
fn dict(value: Ref, key: &str) -> Ref {
    unsafe { CFDictionaryGetValue(value, Owned::string(key).0) }
}
fn number(value: Ref) -> Option<i64> {
    let mut n = 0i64;
    (!value.is_null() && unsafe { CFNumberGetValue(value, 4, (&mut n as *mut i64).cast()) } != 0)
        .then_some(n)
}
fn boolean(value: Ref) -> Option<bool> {
    if value.is_null() || unsafe { CFGetTypeID(value) != CFBooleanGetTypeID() } {
        None
    } else {
        Some(unsafe { CFBooleanGetValue(value) != 0 })
    }
}
pub fn session_active() -> bool {
    let Some(session) = Owned::from(unsafe { CGSessionCopyCurrentDictionary() }) else {
        return false;
    };
    let locked = dict(session.0, "CGSSessionScreenIsLocked");
    (locked.is_null() || boolean(locked) == Some(false))
        && boolean(dict(session.0, "kCGSSessionOnConsoleKey")) == Some(true)
}
fn identity() -> Option<Context> {
    if !session_active() {
        return None;
    }
    let _pool = Pool::new();
    // NSWorkspace.frontmostApplication can remain stale without a main Cocoa run loop.
    // The Process Manager query works for both headless and GUI collector hosts.
    let mut serial = ProcessSerialNumber { high: 0, low: 0 };
    let mut pid = 0i32;
    if unsafe { GetFrontProcess(&mut serial) != 0 || GetProcessPID(&serial, &mut pid) != 0 }
        || pid <= 0
    {
        return None;
    }
    let app = unsafe {
        let call: unsafe extern "C" fn(Ref, Ref, i32) -> Ref =
            std::mem::transmute(objc_msgSend as *const ());
        call(
            objc_getClass(c"NSRunningApplication".as_ptr()),
            sel_registerName(c"runningApplicationWithProcessIdentifier:".as_ptr()),
            pid,
        )
    };
    if app.is_null() {
        return None;
    }
    let application = string(unsafe { send(app, "bundleIdentifier") }, 512)?;
    // A browser's link-preview panel can be the top layer-zero CG window. When AX
    // access is available, its actual focused window supplies the native identity.
    // This reads identity only, before any optional control labels or text.
    let focused_window = if unsafe { AXIsProcessTrusted() } {
        Owned::from(unsafe { AXUIElementCreateApplication(pid) }).and_then(|app| {
            unsafe { AXUIElementSetMessagingTimeout(app.0, 0.2) };
            let window = attr(app.0, "AXFocusedWindow")?;
            let mut id = 0u32;
            (unsafe { _AXUIElementGetWindow(window.0, &mut id) } == 0 && id > 0)
                .then_some(i64::from(id))
        })
    } else {
        None
    };
    let windows = Owned::from(unsafe { CGWindowListCopyWindowInfo(1 | 16, 0) })?;
    for i in 0..unsafe { CFArrayGetCount(windows.0) } {
        let w = unsafe { CFArrayGetValueAtIndex(windows.0, i) };
        if number(dict(w, "kCGWindowOwnerPID")) == Some(i64::from(pid))
            && number(dict(w, "kCGWindowLayer")) == Some(0)
        {
            let window_id = number(dict(w, "kCGWindowNumber"))?;
            if focused_window.is_some_and(|id| id != window_id) {
                continue;
            }
            if window_id <= 0 {
                return None;
            }
            let website = if browser(&application) {
                Website::Unknown
            } else {
                Website::NotWeb
            };
            return Some(Context {
                application,
                process_id: i64::from(pid),
                window_id,
                website,
            });
        }
    }
    None
}
struct Focus {
    app: Owned,
    window: Owned,
    element: Owned,
}
fn same_native(actual: Option<Context>, expected: &Context) -> bool {
    actual.is_some_and(|v| {
        v.application == expected.application
            && v.process_id == expected.process_id
            && v.window_id == expected.window_id
    })
}
fn browser_window_matches(context: &Context, bounds: [i32; 4]) -> bool {
    let Some(windows) = Owned::from(unsafe { CGWindowListCopyWindowInfo(16, 0) }) else {
        return false;
    };
    let mut matching = Vec::new();
    for i in 0..unsafe { CFArrayGetCount(windows.0) } {
        let w = unsafe { CFArrayGetValueAtIndex(windows.0, i) };
        if number(dict(w, "kCGWindowOwnerPID")) != Some(context.process_id)
            || number(dict(w, "kCGWindowLayer")) != Some(0)
        {
            continue;
        }
        let frame = dict(w, "kCGWindowBounds");
        if frame.is_null() {
            return false;
        }
        let mut n = [0f64; 4];
        for (slot, key) in n.iter_mut().zip(["X", "Y", "Width", "Height"]) {
            let v = dict(frame, key);
            if v.is_null() || unsafe { CFNumberGetValue(v, 6, (slot as *mut f64).cast()) } == 0 {
                return false;
            }
        }
        let actual = [n[0], n[1], n[0] + n[2], n[1] + n[3]];
        if actual
            .iter()
            .zip(bounds)
            .all(|(a, b)| (a - f64::from(b)).abs() < 0.75)
        {
            matching.push(number(dict(w, "kCGWindowNumber")));
        }
    }
    // Native IDs and unique geometry bind the scripting window even when titles match.
    matching == [Some(context.window_id)]
}
fn focus(context: &Context) -> std::result::Result<Focus, &'static str> {
    let app = Owned::from(unsafe {
        AXUIElementCreateApplication(
            context
                .process_id
                .try_into()
                .map_err(|_| "invalid_process")?,
        )
    })
    .ok_or("no_accessibility_app")?;
    unsafe { AXUIElementSetMessagingTimeout(app.0, 0.2) };
    if crate::browser::chrome(&context.application) {
        // Chrome activates native accessibility on its application's AXRole read.
        // This is its supported non-VoiceOver entry point, including cold startup.
        let _ = text_attr(app.0, "AXRole", 128);
        // Chromium builds its web AX tree on demand. Do not use AXEnhancedUserInterface,
        // which also changes unrelated AppKit window behavior.
        unsafe {
            AXUIElementSetAttributeValue(
                app.0,
                Owned::string("AXManualAccessibility").0,
                kCFBooleanTrue,
            )
        };
    }
    let window = attr(app.0, "AXFocusedWindow").ok_or("no_focused_window")?;
    let mut id = 0u32;
    if unsafe { _AXUIElementGetWindow(window.0, &mut id) } != 0
        || i64::from(id) != context.window_id
    {
        return Err("focused_window_identity_mismatch");
    }
    let element = attr(app.0, "AXFocusedUIElement")
        .or_else(|| {
            let system = Owned::from(unsafe { AXUIElementCreateSystemWide() })?;
            unsafe { AXUIElementSetMessagingTimeout(system.0, 0.2) };
            attr(system.0, "AXFocusedUIElement")
        })
        .or_else(|| {
            if !crate::browser::chrome(&context.application) {
                return None;
            }
            warm_browser_tree(window.0);
            attr(app.0, "AXFocusedUIElement")
        })
        .ok_or("no_focused_element")?;
    let mut pid = 0;
    if unsafe { AXUIElementGetPid(element.0, &mut pid) } != 0
        || i64::from(pid) != context.process_id
    {
        return Err("focused_element_process_mismatch");
    }
    // Independently bind the focused element's owning window to the same native ID.
    let owner = attr(element.0, "AXWindow").ok_or("no_element_window")?;
    if unsafe { _AXUIElementGetWindow(owner.0, &mut id) } != 0 || i64::from(id) != context.window_id
    {
        return Err("element_window_identity_mismatch");
    }
    Ok(Focus {
        app,
        window,
        element,
    })
}
/// Chromium initializes renderer accessibility when a client requests its view hierarchy.
/// Visit structure only, with bounded work; never read labels, values or link URLs here.
fn warm_browser_tree(window: Ref) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(750);
    let mut pending = vec![(Owned(unsafe { CFRetain(window) }), 0usize)];
    let mut visited = 0;
    while let Some((node, depth)) = pending.pop() {
        if visited >= 128 || std::time::Instant::now() >= deadline {
            return;
        }
        visited += 1;
        unsafe { AXUIElementSetMessagingTimeout(node.0, 0.15) };
        if text_attr(node.0, "AXRole", 128).as_deref() == Some("AXWebArea") {
            return;
        }
        if depth >= 16 {
            continue;
        }
        let Some(children) = attr(node.0, "AXChildren") else {
            continue;
        };
        if unsafe { CFGetTypeID(children.0) != CFArrayGetTypeID() } {
            continue;
        }
        let count = unsafe { CFArrayGetCount(children.0) }.min(128 - visited);
        for i in (0..count).rev() {
            let child = unsafe { CFArrayGetValueAtIndex(children.0, i) };
            if !child.is_null() && unsafe { CFGetTypeID(child) == AXUIElementGetTypeID() } {
                pending.push((Owned(unsafe { CFRetain(child) }), depth + 1));
            }
        }
    }
}
/// Walk structure only. Unknown subroles/ancestry are not permission to read content.
fn eligible(element: Ref, expected_web: Option<&str>) -> std::result::Result<String, &'static str> {
    let role = text_attr(element, "AXRole", 129).ok_or("unknown_role")?;
    let mut current = None;
    let mut saw_web = false;
    for _ in 0..32 {
        let node = current.as_ref().map_or(element, |v: &Owned| v.0);
        let r = text_attr(node, "AXRole", 128).ok_or("unknown_ancestor_role")?;
        // Ordinary AppKit fields report attributeUnsupported; Chromium reports noValue
        // for an absent optional subrole. Timeouts and malformed values remain failures.
        let sub = match attr_result(node, "AXSubrole") {
            Ok(value) => Some(string(value.0, 128).ok_or("invalid_subrole")?),
            Err(-25205 | -25212) => None,
            Err(_) if matches!(r.as_str(), "AXTextField" | "AXTextArea" | "AXComboBox") => {
                return Err("unknown_text_subrole")
            }
            Err(_) => None,
        };
        if r.contains("Secure") || sub.as_deref().is_some_and(|s| s.contains("Secure")) {
            return Err("secure_or_web_content");
        }
        if r == "AXWebArea" {
            let Some(expected) = expected_web else {
                return Err("secure_or_web_content");
            };
            let actual = page_url(node).ok_or("unknown_web_document")?;
            if !crate::browser::same_document(&actual, expected) {
                return Err("different_web_document");
            }
            saw_web = true;
        }
        if r == "AXWindow" {
            if expected_web.is_some() && !saw_web {
                return Err("browser_ui_excluded");
            }
            return Ok(role);
        }
        current = Some(attr(node, "AXParent").ok_or("unknown_parent")?);
    }
    Err("ancestry_limit")
}
#[derive(Default)]
pub struct MacSource {
    focus: Option<Focus>,
    pub last_skip: Option<&'static str>,
    page: Option<crate::browser::Page>,
}
/// Content-free adapter diagnostics used by the explicit disposable-host probe.
pub fn focus_status() -> serde_json::Value {
    let Some(c) = identity() else {
        return serde_json::json!({"source":null});
    };
    let Some(app) = Owned::from(unsafe { AXUIElementCreateApplication(c.process_id as i32) })
    else {
        return serde_json::json!({"app":null});
    };
    unsafe { AXUIElementSetMessagingTimeout(app.0, 0.2) };
    let manual = if crate::browser::chrome(&c.application) {
        Some(unsafe {
            AXUIElementSetAttributeValue(
                app.0,
                Owned::string("AXManualAccessibility").0,
                kCFBooleanTrue,
            )
        })
    } else {
        None
    };
    let app_focus = attr_result(app.0, "AXFocusedUIElement")
        .map(|_| 0)
        .unwrap_or_else(|e| e);
    let window = attr_result(app.0, "AXFocusedWindow")
        .map(|w| {
            attr_result(w.0, "AXFocusedUIElement")
                .map(|_| 0)
                .unwrap_or_else(|e| e)
        })
        .unwrap_or_else(|e| e);
    let system = Owned::from(unsafe { AXUIElementCreateSystemWide() }).map(|s| {
        attr_result(s.0, "AXFocusedUIElement")
            .map(|_| 0)
            .unwrap_or_else(|e| e)
    });
    serde_json::json!({"application":c.application,"pid":c.process_id,"window":c.window_id,"manual_status":manual,"app_focus_status":app_focus,"window_focus_status":window,"system_focus_status":system})
}
impl Source for MacSource {
    fn identify(&mut self) -> Result<Option<Context>> {
        self.focus = None;
        self.last_skip = None;
        self.page = None;
        Ok(identity())
    }
    fn resolve(&mut self, context: &Context, enabled: bool) -> Result<Option<Context>> {
        if !enabled || !crate::browser::chrome(&context.application) {
            return Ok(Some(context.clone()));
        }
        match browser_native::snapshot(context.process_id)? {
            crate::browser::Snapshot::Private => {
                self.last_skip = Some("private_browser");
                Ok(None)
            }
            crate::browser::Snapshot::Other => {
                self.last_skip = Some("unknown_browser_document");
                Ok(None)
            }
            crate::browser::Snapshot::Public(page) => {
                if !same_native(identity(), context)
                    || !browser_window_matches(context, page.bounds)
                {
                    self.last_skip = Some("unbound_browser_window");
                    return Ok(None);
                }
                let mut context = context.clone();
                context.website = Website::Url(page.url.clone());
                self.page = Some(page);
                Ok(Some(context))
            }
        }
    }
    fn read(&mut self, context: &Context, mode: CaptureMode) -> Result<Option<Content>> {
        if (context.website != Website::NotWeb && self.page.is_none())
            || !same_native(identity(), context)
        {
            self.last_skip = Some("web_or_changed_identity");
            return Ok(None);
        }
        if !unsafe { AXIsProcessTrusted() } {
            return Err(Error::new(
                "computerHistoryAccessibilityRequired",
                "Enable Accessibility for Copilot to record focused controls",
            ));
        }
        let f = match focus(context) {
            Ok(f) => f,
            Err(reason) => {
                self.last_skip = Some(reason);
                return Ok(None);
            }
        };
        let expected_web = self.page.as_ref().map(|p| p.url.as_str());
        let role = match eligible(f.element.0, expected_web) {
            Ok(role) => role,
            Err(reason) => {
                self.last_skip = Some(reason);
                return Ok(None);
            }
        };
        let content = Content {
            role,
            // Read one character beyond each storage cap so truncation is observable.
            label: text_attr(f.element.0, "AXTitle", 513)
                .filter(|v| !v.is_empty())
                .or_else(|| text_attr(f.element.0, "AXDescription", 513)),
            identifier: text_attr(f.element.0, "AXIdentifier", 257),
            value: if mode == CaptureMode::Values {
                text_attr(f.element.0, "AXValue", 2049)
            } else {
                None
            },
        };
        // Recheck security after the read as controls can change in place.
        if eligible(f.element.0, expected_web).is_err() {
            return Ok(None);
        }
        self.focus = Some(f);
        Ok(Some(content))
    }
    fn verify(&mut self, context: &Context) -> Result<bool> {
        if !same_native(identity(), context) {
            return Ok(false);
        }
        if let Some(page) = &self.page {
            if browser_native::snapshot(context.process_id)?
                != crate::browser::Snapshot::Public(page.clone())
                || !browser_window_matches(context, page.bounds)
                || !same_native(identity(), context)
            {
                return Ok(false);
            }
        }
        if let Some(previous) = &self.focus {
            let Ok(current) = focus(context) else {
                return Ok(false);
            };
            if unsafe {
                CFEqual(previous.app.0, current.app.0) == 0
                    || CFEqual(previous.window.0, current.window.0) == 0
                    || CFEqual(previous.element.0, current.element.0) == 0
            } {
                return Ok(false);
            }
            if eligible(
                current.element.0,
                self.page.as_ref().map(|p| p.url.as_str()),
            )
            .is_err()
            {
                return Ok(false);
            }
        }
        Ok(session_active())
    }
}

/// Bounded native window text for the separately approved interaction scope.
/// Container text is never read because it can aggregate protected descendants.
/// Browsers and embedded web areas stay excluded from this native-only scope.
pub fn native_window_text(context: &Context) -> Result<Option<Content>> {
    if context.website != Website::NotWeb || !same_native(identity(), context) {
        return Ok(None);
    }
    let f = match focus(context) { Ok(f) => f, Err(_) => return Ok(None) };
    if eligible(f.element.0, None).is_err() { return Ok(None); }
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(750);
    let mut pending = vec![(Owned(unsafe { CFRetain(f.window.0) }), 0usize)];
    let mut text = Vec::new();
    let mut visited = 0usize;
    let mut incomplete = false;
    while let Some((node, depth)) = pending.pop() {
        if visited >= 128 || std::time::Instant::now() >= deadline { incomplete = true; break; }
        visited += 1;
        unsafe { AXUIElementSetMessagingTimeout(node.0, 0.1) };
        let Some(role) = text_attr(node.0, "AXRole", 128) else { incomplete = true; continue; };
        if role == "AXWebArea" || role.contains("Secure")
            || boolean(attr(node.0, "AXHidden").as_ref().map_or(ptr::null(), |v| v.0)) == Some(true) {
            continue;
        }
        let leaf = matches!(role.as_str(), "AXTextField" | "AXTextArea" | "AXStaticText" | "AXButton" |
            "AXCheckBox" | "AXRadioButton" | "AXPopUpButton" | "AXMenuButton" | "AXSlider" | "AXComboBox");
        if leaf {
            if eligible(node.0, None).is_err() { continue; }
            let label = text_attr(node.0, "AXTitle", 256).filter(|v| !v.is_empty())
                .or_else(|| text_attr(node.0, "AXDescription", 256)).unwrap_or_default();
            let value = text_attr(node.0, "AXValue", 2049).unwrap_or_default();
            if eligible(node.0, None).is_err() { continue; }
            incomplete |= value.chars().count() > 2048;
            if !label.is_empty() || !value.is_empty() {
                text.push(format!("{role} {label:?}: {:?}", value.chars().take(2048).collect::<String>()));
            }
        }
        if depth >= 16 { incomplete = true; continue; }
        let Some(children) = attr(node.0, "AXChildren") else { continue; };
        if unsafe { CFGetTypeID(children.0) != CFArrayGetTypeID() } { continue; }
        let count = unsafe { CFArrayGetCount(children.0) };
        if count > 128 { incomplete = true; }
        for index in (0..count.min(128)).rev() {
            let child = unsafe { CFArrayGetValueAtIndex(children.0, index) };
            if !child.is_null() && unsafe { CFGetTypeID(child) == AXUIElementGetTypeID() } {
                pending.push((Owned(unsafe { CFRetain(child) }), depth + 1));
            }
        }
    }
    if !same_native(identity(), context) || !session_active() || eligible(f.element.0, None).is_err() { return Ok(None); }
    let mut value = text.join("\n");
    if incomplete { value.push_str("\n[Partial accessible window snapshot]"); }
    if value.is_empty() { return Ok(None); }
    Ok(Some(Content { role: "AXWindowSnapshot".into(), label: Some("Native window context".into()),
        identifier: None, value: Some(value) }))
}
