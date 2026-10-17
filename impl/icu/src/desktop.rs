//! The full-desktop facade (spec 10 "Recreate/port plan" step 1, `specs/00-COVERAGE.md` Linux rows):
//! Sky's Linux client has no window model. `target: "linux"` is the whole desktop and every
//! coordinate is a desktop-screenshot pixel. Its agent contract (`linux-SKILL.md`,
//! `docs/sky-full-desktop-api.md`) is `get_screenshot`, `click`, `drag`, `move`, `press_key`,
//! `scroll`, `type_text` and the stateful `drag_handle`. This module serves all of them, over
//! cua-driver's own desktop scope, as flat verbs with no window argument:
//!
//! | verb | Sky input | cua-driver call |
//! | --- | --- | --- |
//! | `desktop_screenshot` | `get_screenshot()` | `get_desktop_state`, agent cursor drawn in at `mouse_size_px` |
//! | `desktop_click` | `{x, y, click_count, mouse_button, key, duration}` | `click {scope: desktop}`; `duration` and a held chord on a native driver |
//! | `desktop_drag` | `{path: [Point; 2+], key}` | `drag {scope: desktop}` for two points; a longer path or a chord needs the native `path`/`modifier` support |
//! | `desktop_move` | `{x, y, key}` | `move_cursor {scope: desktop}` |
//! | `desktop_press_key` | `{key, duration}` | `press_key {scope: desktop}`; `duration` is the native `hold_ms` |
//! | `desktop_scroll` | `{direction, pixels, x, y, key}` | `scroll {scope: desktop}`; `key` is the native `modifier` |
//! | `desktop_type_text` | `{text}` | `type_text {scope: desktop}` |
//! | `desktop_drag_handle` | `{action: start, x, y}`, `{action: move_to, x, y}`, `{action: end}` | the native `drag {scope: desktop, hold}` |
//!
//! The names carry a `desktop_` prefix because the window2 verbs already own `click`, `drag`,
//! `press_key`, `scroll` and `type_text`; they are INFERRED (Codex's are JavaScript methods on `sky`).
//!
//! What this layer adds to the raw driver, and why each is testable without a desktop:
//! - the settle window (`post_action_sleep_ms`) around every verb, through the layer's one settler;
//! - the agent cursor composited into the screenshot at the last desktop pointer pixel
//!   (`mouse_size_px`, default 12), remembered across one-shot `icu call` processes;
//! - Sky's argument vocabulary (`l`/`r`/`m`, `+`-separated keysym chords, a `path` of points)
//!   translated to the driver's;
//! - refusal before the driver is touched: a point outside the screen (the driver clamps it to an
//!   edge and clicks there), a malformed argument, a driver that lacks the desktop scope or the
//!   hold/path/chord support a call needs (`desktopUnavailable`: a stock driver would silently drop
//!   the extra field and do a different gesture);
//! - the approval gate. There is no app to approve, so the whole desktop is one pseudo-app,
//!   [`DESKTOP_APP_ID`], that the operator approves (or denies in `ICU_DENIED_APPS`) like any other.
use super::*;

/// The agent-facing contract for these verbs (Sky's `linux-SKILL.md`, in this layer's names), served
/// as the MCP resource [`LINUX_SKILL_URI`] and summarised in the `initialize` instructions.
pub const LINUX_SKILL: &str = include_str!("../skills/linux-SKILL.md");
pub const LINUX_SKILL_URI: &str = "icu://skills/linux-desktop";
/// The paragraph added to the `initialize` instructions.
pub const INSTRUCTIONS: &str = "For the whole screen, with no window to name, use desktop_screenshot, then desktop_click, desktop_drag (a path of two or more points), desktop_drag_handle (press, look, move, release), desktop_move and desktop_scroll in pixels of that screenshot, and desktop_press_key and desktop_type_text for the keyboard; the resource icu://skills/linux-desktop is their contract.";

/// The approval key for acting on the whole desktop.
pub const DESKTOP_APP_ID: &str = "icu.desktop";
pub const DESKTOP_TOOLS: [&str; 8] = ["desktop_screenshot", "desktop_click", "desktop_drag", "desktop_move", "desktop_press_key", "desktop_scroll", "desktop_type_text", "desktop_drag_handle"];
/// The seven that deliver input (they pass the gate; the screenshot only observes).
pub const DESKTOP_INPUT_TOOLS: [&str; 7] = ["desktop_click", "desktop_drag", "desktop_move", "desktop_press_key", "desktop_scroll", "desktop_type_text", "desktop_drag_handle"];
/// The field of the native driver's desktop `drag` that holds the mouse button between `desktop_drag_handle` calls.
pub const DRAG_HOLD_FIELD: &str = "hold";
/// One wheel click is this many pixels of Sky's `pixels` (a wheel moves in whole clicks, so the
/// distance is approximate; the reply carries the clicks actually sent).
pub const PIXELS_PER_CLICK: f64 = 40.0;
pub const DEFAULT_SCROLL_PIXELS: f64 = 120.0;
pub const MAX_SCROLL_CLICKS: u64 = 50;
pub const MAX_TYPED_CHARS: usize = 100_000;
/// Pixel index of the agent cursor's last desktop position in [`Layer::pointer`]; no window has this id.
pub(crate) const DESKTOP_KEY: u64 = u64::MAX;
/// Sky paths are "at least two"; the native driver takes at most this many.
pub const MAX_PATH_POINTS: usize = 256;
pub const MAX_CLICK_COUNT: u64 = 3;
pub const MAX_HOLD_MS: u64 = 10_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Click { pub x: f64, pub y: f64, pub count: u64, pub button: &'static str, pub key: Vec<String>, pub hold_ms: u64 }
#[derive(Debug, Clone, PartialEq)]
pub struct Drag { pub path: Vec<(f64, f64)>, pub key: Vec<String> }
#[derive(Debug, Clone, PartialEq)]
pub struct Move { pub x: f64, pub y: f64, pub key: Vec<String> }
#[derive(Debug, Clone, PartialEq)]
pub struct PressKey { pub key: String, pub modifiers: Vec<String>, pub hold_ms: u64 }
#[derive(Debug, Clone, PartialEq)]
pub struct Scroll { pub direction: &'static str, pub clicks: u64, pub at: Option<(f64, f64)>, pub key: Vec<String> }
#[derive(Debug, Clone, PartialEq)]
pub enum Handle { Start(f64, f64), MoveTo(f64, f64), End }

fn bad(msg: impl Into<String>, reason: &str) -> ToolError {
    let mut e = ToolError::new(CODE_INVALID_ARGUMENT, msg);
    e.structured["reason"] = json!(reason);
    e
}

/// Whether a drag handle started now would be lost: the driver ends with this call (`icu call`, no `ICU_SOCKET`) and on native Wayland
/// the held button is the driver process's own virtual pointer, which goes with it (X11 keeps the press in the server, and the next
/// process finds the hold). Measured on sway: the client sees the press and never the release.
pub(crate) fn handle_start_is_lost(one_shot: bool, shared_driver: bool, native_wayland: bool) -> bool { one_shot && !shared_driver && native_wayland }

/// Whether the driver this process starts would use the native Wayland backend: a Wayland display and the driver's opt-in.
fn driver_is_native_wayland() -> bool {
    let set = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    set("WAYLAND_DISPLAY").is_some() && set("CUA_DRIVER_RS_ENABLE_WAYLAND").is_some_and(|v| matches!(v.trim(), "1" | "true" | "yes" | "on"))
}

fn unavailable(msg: impl Into<String>, needs: &str) -> ToolError {
    let mut e = ToolError::new(CODE_DESKTOP_UNAVAILABLE, msg);
    e.structured["needs"] = json!(needs);
    e
}

/// A finite number argument.
fn number(v: &Value, what: &str) -> Result<f64, ToolError> {
    v.as_f64().filter(|n| n.is_finite()).ok_or_else(|| bad(format!("{what} must be a finite number"), "not_a_number"))
}

/// `{x, y}` -> a point.
pub fn parse_point(v: &Value, what: &str) -> Result<(f64, f64), ToolError> {
    let (Some(x), Some(y)) = (v.get("x"), v.get("y")) else { return Err(bad(format!("{what} needs numbers x and y"), "not_a_number")) };
    Ok((number(x, &format!("{what}.x"))?, number(y, &format!("{what}.y"))?))
}

/// A `+`-separated keysym chord (Sky `key`) as driver key names: Control_L -> ctrl, Super_L -> super.
/// Held for the whole gesture. Whitespace around `+` is ignored; an empty chord is an error.
pub fn parse_chord(chord: &str) -> Result<Vec<String>, ToolError> {
    let tokens: Vec<String> = chord.split('+').map(|t| t.trim()).filter(|t| !t.is_empty()).map(|t| {
        match t.to_ascii_lowercase().as_str() {
            "control_l" | "control_r" | "control" | "ctrl" => "ctrl".to_string(),
            "shift_l" | "shift_r" | "shift" => "shift".to_string(),
            "alt_l" | "alt_r" | "alt" | "option" => "alt".to_string(),
            "super_l" | "super_r" | "super" | "meta_l" | "meta_r" | "meta" | "win" | "cmd" | "command" => "super".to_string(),
            _ => t.to_string(),
        }
    }).collect();
    if tokens.is_empty() { return Err(bad(format!("key chord {chord:?} is empty"), "empty_chord")); }
    if tokens.len() > 4 { return Err(bad(format!("key chord {chord:?} holds more than four keys"), "chord_too_long")); }
    Ok(tokens)
}

fn parse_key(args: &Value) -> Result<Vec<String>, ToolError> {
    match args.get("key") {
        None | Some(Value::Null) => Ok(vec![]),
        Some(Value::String(s)) => parse_chord(s),
        Some(_) => Err(bad("key must be a string chord such as \"Control_L\" or \"Shift_L+Alt_L\"", "bad_key")),
    }
}

/// Sky's button spellings.
pub fn parse_button(args: &Value) -> Result<&'static str, ToolError> {
    match args.get("mouse_button") {
        None | Some(Value::Null) => Ok("left"),
        Some(v) => match v.as_str().map(str::to_ascii_lowercase).as_deref() {
            Some("left") | Some("l") => Ok("left"),
            Some("right") | Some("r") => Ok("right"),
            Some("middle") | Some("m") => Ok("middle"),
            _ => Err(bad("mouse_button must be left, right, middle, l, r or m", "bad_button")),
        },
    }
}

fn whole(v: &Value, what: &str, max: u64) -> Result<u64, ToolError> {
    match v.as_u64().or_else(|| v.as_f64().filter(|f| f.is_finite() && f.fract() == 0.0 && *f >= 0.0).map(|f| f as u64)) {
        Some(n) if n <= max => Ok(n),
        _ => Err(bad(format!("{what} must be a whole number from 0 to {max}"), "out_of_range")),
    }
}

pub fn parse_click(args: &Value) -> Result<Click, ToolError> {
    let (x, y) = (number(args.get("x").unwrap_or(&Value::Null), "x")?, number(args.get("y").unwrap_or(&Value::Null), "y")?);
    let count = match args.get("click_count") { None | Some(Value::Null) => 1, Some(v) => whole(v, "click_count", MAX_CLICK_COUNT)? };
    if count == 0 { return Err(bad("click_count must be at least 1", "out_of_range")); }
    let hold_ms = match args.get("duration") { None | Some(Value::Null) => 0, Some(v) => whole(v, "duration", MAX_HOLD_MS)? };
    Ok(Click { x, y, count, button: parse_button(args)?, key: parse_key(args)?, hold_ms })
}

pub fn parse_drag(args: &Value) -> Result<Drag, ToolError> {
    let items = args.get("path").and_then(Value::as_array).ok_or_else(|| bad("path must be an array of at least two {x, y} points", "bad_path"))?;
    if items.len() < 2 { return Err(bad(format!("path needs at least two points; got {}", items.len()), "bad_path")); }
    if items.len() > MAX_PATH_POINTS { return Err(bad(format!("path has {} points; at most {MAX_PATH_POINTS}", items.len()), "bad_path")); }
    let path = items.iter().enumerate().map(|(i, p)| parse_point(p, &format!("path[{i}]"))).collect::<Result<Vec<_>, _>>()?;
    Ok(Drag { path, key: parse_key(args)? })
}

pub fn parse_move(args: &Value) -> Result<Move, ToolError> {
    Ok(Move { x: number(args.get("x").unwrap_or(&Value::Null), "x")?, y: number(args.get("y").unwrap_or(&Value::Null), "y")?, key: parse_key(args)? })
}

/// X keysym spellings Sky's contract allows that the driver spells another way (`Page_Down` is
/// `pagedown` to it). Anything else goes through as given; the driver ignores case.
pub fn driver_key_name(key: &str) -> String {
    match key.to_ascii_lowercase().as_str() {
        "page_down" | "next" | "kp_next" => "pagedown".into(),
        "page_up" | "prior" | "kp_prior" => "pageup".into(),
        "caps_lock" => "capslock".into(),
        "num_lock" => "numlock".into(),
        "kp_enter" | "iso_enter" => "return".into(),
        _ => key.to_string(),
    }
}

/// Sky `press_key`: a key or a `+`-separated chord (`Control_L+a`, `Super_L+d`); the last name is the
/// key, the ones before it are held. `duration` holds the key down that many milliseconds.
pub fn parse_press_key(args: &Value) -> Result<PressKey, ToolError> {
    let chord = args.get("key").and_then(Value::as_str).ok_or_else(|| bad("key must be a string such as \"Return\" or \"Control_L+a\"", "bad_key"))?;
    let mut tokens = parse_chord(chord)?;
    let key = driver_key_name(&tokens.pop().expect("parse_chord returns at least one token"));
    let hold_ms = match args.get("duration") { None | Some(Value::Null) => 0, Some(v) => whole(v, "duration", MAX_HOLD_MS)? };
    Ok(PressKey { key, modifiers: tokens, hold_ms })
}

/// Sky `scroll`: `direction`, `pixels` (default [`DEFAULT_SCROLL_PIXELS`]), an optional origin and `key`.
pub fn parse_scroll(args: &Value) -> Result<Scroll, ToolError> {
    let direction = match args.get("direction").and_then(Value::as_str).map(str::to_ascii_lowercase).as_deref() {
        Some("up") | Some("u") => "up",
        Some("down") | Some("d") => "down",
        Some("left") | Some("l") => "left",
        Some("right") | Some("r") => "right",
        _ => return Err(bad("direction must be up, down, left, right, u, d, l or r", "bad_direction")),
    };
    let pixels = match args.get("pixels") {
        None | Some(Value::Null) => DEFAULT_SCROLL_PIXELS,
        Some(v) => number(v, "pixels")?,
    };
    if pixels <= 0.0 { return Err(bad("pixels must be greater than 0", "out_of_range")); }
    let clicks = ((pixels / PIXELS_PER_CLICK).ceil() as u64).clamp(1, MAX_SCROLL_CLICKS);
    let at = match (args.get("x").filter(|v| !v.is_null()), args.get("y").filter(|v| !v.is_null())) {
        (None, None) => None,
        (Some(x), Some(y)) => Some((number(x, "x")?, number(y, "y")?)),
        _ => return Err(bad("x and y go together: give both or neither", "not_a_number")),
    };
    Ok(Scroll { direction, clicks, at, key: parse_key(args)? })
}

/// Sky `type_text`.
pub fn parse_type_text(args: &Value) -> Result<String, ToolError> {
    let text = args.get("text").and_then(Value::as_str).ok_or_else(|| bad("text must be a string", "bad_text"))?;
    if text.is_empty() { return Err(bad("text is empty: there is nothing to type", "bad_text")); }
    if text.chars().count() > MAX_TYPED_CHARS { return Err(bad(format!("text is over {MAX_TYPED_CHARS} characters"), "bad_text")); }
    Ok(text.to_string())
}

/// Sky `drag_handle`: `start` and `move_to` take a point, `end` takes none.
pub fn parse_handle(args: &Value) -> Result<Handle, ToolError> {
    match args.get("action").and_then(Value::as_str) {
        Some("start") => { let (x, y) = parse_point(args, "start")?; Ok(Handle::Start(x, y)) }
        Some("move_to") => { let (x, y) = parse_point(args, "move_to")?; Ok(Handle::MoveTo(x, y)) }
        Some("end") => Ok(Handle::End),
        _ => Err(bad("action must be start, move_to or end", "bad_action")),
    }
}

/// A desktop pixel as the driver gets it, or a refusal when it is not on the screen. The driver
/// clamps an off-screen point to the nearest edge and acts there, so the check has to be ours.
pub fn pixel_on_screen(p: (f64, f64), screen: (f64, f64), what: &str) -> Result<(i64, i64), ToolError> {
    let (x, y) = (p.0.round(), p.1.round());
    if x < 0.0 || y < 0.0 || x >= screen.0 || y >= screen.1 {
        let mut e = bad(format!("{what} ({}, {}) is outside the {}x{} desktop; coordinates are desktop screenshot pixels", p.0, p.1, screen.0, screen.1), "point_off_screen");
        e.structured["screen"] = json!({"width": screen.0, "height": screen.1});
        return Err(e);
    }
    Ok((x as i64, y as i64))
}

impl Layer {
    /// Sky's approval, for the whole desktop: policy first, then the persisted allowlists.
    fn gate_desktop(&self, tool: &str) -> Result<(), ToolError> {
        self.check_app_policy(DESKTOP_APP_ID)?;
        if !self.cfg.require_app_approval || self.approval_store().is_allowed(DESKTOP_APP_ID) { return Ok(()); }
        let target = AppTarget { bundle_identifier: DESKTOP_APP_ID.into(), display_name: "the whole desktop".into(), risk: self.cfg.risk_level.clone(), warning_subtitle: None };
        let mut e = ToolError::new(CODE_APP_APPROVAL_REQUIRED, "Computer Use needs approval to use the whole desktop: raise the elicitation, then call approve_app");
        e.structured["elicitation"] = elicitation(&target, self.cfg.allow_persistent_approval, tool, None);
        e.structured["app"] = json!(DESKTOP_APP_ID);
        Err(e)
    }

    /// The driver must serve the desktop scope at all.
    fn require_desktop_scope(&self) -> Result<(), ToolError> {
        let names = self.cua.tool_names().map_err(ToolError::from_cua)?;
        let missing: Vec<&str> = ["get_desktop_state", "get_screen_size", "click", "drag", "move_cursor"].into_iter().filter(|t| !names.iter().any(|n| n == t)).collect();
        if missing.is_empty() { return Ok(()); }
        Err(unavailable(format!("this driver has no desktop scope (missing {}): the desktop verbs need a cua-driver with get_desktop_state and the desktop-scope click, drag and move_cursor", missing.join(", ")), "desktop_scope"))
    }

    /// Whether the driver's `tool` takes `field`. A driver that cannot say is treated as not taking
    /// it, because the alternative is sending a field a stock driver silently drops.
    fn driver_takes(&self, tool: &str, field: &str) -> bool {
        self.cua.tool_schema(tool).and_then(|s| s.get("properties").and_then(|p| p.get(field)).cloned()).is_some()
    }

    fn require_field(&self, tool: &str, field: &str, what: &str) -> Result<(), ToolError> {
        if self.driver_takes(tool, field) { return Ok(()); }
        Err(unavailable(format!("{what} needs a cua-driver whose desktop-scope {tool} takes `{field}` (the patched native driver); this driver would ignore it and do a different gesture, so nothing was sent"), &format!("{tool}.{field}")))
    }

    /// The screen in the frame screenshot pixels are in. `get_screen_size` is the cheap answer; on a
    /// pure Wayland session other than Hyprland the driver cannot give it (it reads the X11 root
    /// window and fails with "$DISPLAY variable not set", measured on sway), so the size comes from
    /// one `get_desktop_state`, whose capture is in that same frame. Without either, the action is
    /// refused: a bounds check against an unknown screen would be no check.
    fn screen_size(&self) -> Result<(f64, f64), ToolError> {
        let dims = |r: &Value, w: &str, h: &str| match (r.get(w).and_then(Value::as_f64), r.get(h).and_then(Value::as_f64)) {
            (Some(w), Some(h)) if w > 0.0 && h > 0.0 => Some((w, h)),
            _ => None,
        };
        match self.call("get_screen_size", json!({})) {
            Ok(r) => dims(&r, "width", "height").ok_or_else(|| ToolError::new(CODE_DRIVER_ERROR, "get_screen_size returned no width and height")),
            Err(first) => {
                let mut req = json!({});
                if let Some(s) = self.input_session() { req["session"] = json!(s); }
                let r = self.call("get_desktop_state", req).map_err(|_| first)?;
                dims(&r, "screen_width", "screen_height").ok_or_else(|| ToolError::new(CODE_DRIVER_ERROR, "get_desktop_state returned no screen_width and screen_height"))
            }
        }
    }

    fn remember_desktop_pointer(&self, at: (f64, f64)) {
        self.pointer.lock().unwrap().insert(DESKTOP_KEY, at);
        std::fs::create_dir_all(&self.cfg.state_dir).ok();
        std::fs::write(self.pointer_path(DESKTOP_KEY), serde_json::to_vec(&json!([at.0, at.1])).unwrap()).ok();
    }

    /// `get_screenshot()`: the whole desktop, with the agent cursor drawn in where the last desktop
    /// action put it. Coordinates in the PNG are the coordinates `desktop_click` takes.
    pub fn desktop_screenshot(&self, args: &Value) -> Result<Value, ToolError> {
        self.require_desktop_scope()?;
        let mut req = json!({});
        if let Some(s) = self.input_session() { req["session"] = json!(s); }
        // Sky's ActionSettler: a screenshot is never taken inside the last action's settle window.
        let waited_ms = self.settler.wait();
        let mut doc = self.call("get_desktop_state", req)?;
        let cursor = self.draw_agent_cursor(DESKTOP_KEY, &mut doc, args);
        let png = doc.get("screenshot_png_b64").and_then(Value::as_str).filter(|s| !s.is_empty())
            .ok_or_else(|| ToolError::new(CODE_DRIVER_ERROR, "get_desktop_state returned no screenshot"))?.to_string();
        let (w, h) = (doc.get("screenshot_width").cloned().unwrap_or(Value::Null), doc.get("screenshot_height").cloned().unwrap_or(Value::Null));
        let mut out = json!({
            "screenshots": [{"id": "desktop", "zIndex": 0, "url": format!("data:image/png;base64,{png}"), "originX": 0, "originY": 0, "width": w, "height": h}],
            "screen": {"width": doc.get("screen_width"), "height": doc.get("screen_height"), "scale_factor": doc.get("scale_factor")},
            "target": "desktop",
        });
        if let Some(px) = cursor { out["_cursor_in_screenshot"] = json!({"x": px.0, "y": px.1, "size_px": px.2}); }
        if waited_ms > 0 { out["_waited_ms"] = json!(waited_ms); }
        Ok(out)
    }

    /// Where the last desktop action left the pointer (remembered across one-shot `icu call` processes).
    fn desktop_pointer(&self) -> Option<(f64, f64)> {
        self.pointer.lock().unwrap().get(&DESKTOP_KEY).copied().or_else(|| {
            let v: Value = serde_json::from_slice(&std::fs::read(self.pointer_path(DESKTOP_KEY)).ok()?).ok()?;
            Some((v.get(0)?.as_f64()?, v.get(1)?.as_f64()?))
        })
    }

    /// The held drag, if any: written by `start`, read by `move_to` and `end`, so a handle survives
    /// across one-shot `icu call` processes the way the driver's held button does.
    pub(crate) fn handle_path(&self) -> std::path::PathBuf { self.cfg.state_dir.join("desktop_drag_handle.json") }
    fn handle_held(&self) -> Option<(f64, f64)> {
        let v: Value = serde_json::from_slice(&std::fs::read(self.handle_path()).ok()?).ok()?;
        Some((v.get("x")?.as_f64()?, v.get("y")?.as_f64()?))
    }
    fn set_handle(&self, at: Option<(f64, f64)>) {
        match at {
            Some(p) => { std::fs::create_dir_all(&self.cfg.state_dir).ok(); std::fs::write(self.handle_path(), serde_json::to_vec(&json!({"button": "left", "x": p.0, "y": p.1})).unwrap()).ok(); }
            None => { std::fs::remove_file(self.handle_path()).ok(); }
        }
    }

    /// A keyboard verb needs the driver's desktop-scope key tool and nothing about the screen.
    fn require_keyboard_scope(&self, tool: &str) -> Result<(), ToolError> {
        let names = self.cua.tool_names().map_err(ToolError::from_cua)?;
        if !names.iter().any(|n| n == tool) {
            return Err(unavailable(format!("this driver has no {tool} tool"), tool));
        }
        if !self.driver_takes(tool, "scope") {
            return Err(unavailable(format!("this driver's {tool} has no desktop scope; the desktop keyboard verbs need a cua-driver whose {tool} takes `scope: desktop`"), &format!("{tool}.scope")));
        }
        Ok(())
    }

    /// Every desktop verb that delivers input, after the checks they all share: the settle window,
    /// the approval, the bounds of the screen, and the refusals that keep a stock driver from
    /// silently doing a different gesture.
    pub fn desktop_input(&self, tool: &str, args: &Value) -> Result<Value, ToolError> {
        // Releasing a held button is always allowed, even to a stopped session: refusing it would
        // leave the button down.
        let ending = tool == "desktop_drag_handle" && args.get("action").and_then(Value::as_str) == Some("end");
        if !ending { self.pre_dispatch(tool)?; }
        // parse first: a malformed call is refused before it can ask for an approval
        enum Plan { Click(Click), Drag(Drag), Move(Move), Key(PressKey), Scroll(Scroll), Type(String), Handle(Handle) }
        let plan = match tool {
            "desktop_click" => Plan::Click(parse_click(args)?),
            "desktop_drag" => Plan::Drag(parse_drag(args)?),
            "desktop_move" => Plan::Move(parse_move(args)?),
            "desktop_press_key" => Plan::Key(parse_press_key(args)?),
            "desktop_scroll" => Plan::Scroll(parse_scroll(args)?),
            "desktop_type_text" => Plan::Type(parse_type_text(args)?),
            "desktop_drag_handle" => Plan::Handle(parse_handle(args)?),
            other => return Err(ToolError::new(CODE_INVALID_ARGUMENT, format!("unknown desktop tool {other:?}"))),
        };
        if matches!(plan, Plan::Key(_)) { self.hard_deny("press_key", &json!({"key": args.get("key")}))?; }
        if let Plan::Handle(h) = &plan {
            match (h, self.handle_held()) {
                (Handle::Start(..), Some(_)) => return Err(bad("a drag handle is already started: move_to and end it before starting another", "drag_already_started")),
                (Handle::MoveTo(..) | Handle::End, None) => return Err(bad("the drag handle must be started before it is moved or ended", "drag_not_started")),
                _ => {}
            }
        }
        if !ending { self.gate_desktop(tool)?; }
        let session = self.input_session();
        let with_session = |mut v: Value| { if let Some(s) = &session { v["session"] = json!(s); } v };

        // the keyboard verbs: no screen, no pointer
        if let Plan::Key(k) = &plan {
            self.require_keyboard_scope("press_key")?;
            let mut r = json!({"scope": "desktop", "key": k.key});
            if !k.modifiers.is_empty() { r["modifiers"] = json!(k.modifiers); }
            if k.hold_ms > 0 { self.require_field("press_key", "hold_ms", "a key press with a duration")?; r["hold_ms"] = json!(k.hold_ms); }
            return self.desktop_deliver("press_key", with_session(r), args, None, json!({}));
        }
        if let Plan::Type(text) = &plan {
            self.require_keyboard_scope("type_text")?;
            return self.desktop_deliver("type_text", with_session(json!({"scope": "desktop", "text": text})), args, None, json!({"_typed_chars": text.chars().count()}));
        }

        if let Plan::Handle(h) = &plan {
            self.require_desktop_scope()?;
            self.require_field("drag", DRAG_HOLD_FIELD, "a drag handle (a mouse button held across calls)")?;
            if matches!(h, Handle::Start(..)) && handle_start_is_lost(self.one_shot.load(std::sync::atomic::Ordering::SeqCst), self.cfg.socket.is_some(), driver_is_native_wayland()) {
                return Err(unavailable("a drag handle on native Wayland is held by the driver process, and `icu call` ends that process with the call, so the press would be lost and the release never sent; nothing was sent. Use `icu mcp` (one session) or set ICU_SOCKET to a running `cua-driver serve`", "driver_session"));
            }
            // the driver's `drag` schema requires from and to; a handle step repeats its one point
            let at = |hold: &str, p: (f64, f64)| json!({"scope": "desktop", "hold": hold, "from_x": p.0 as i64, "from_y": p.1 as i64, "to_x": p.0 as i64, "to_y": p.1 as i64});
            let (request, end) = match h {
                Handle::End => { let last = self.handle_held().expect("checked above"); (at("end", last), last) }
                Handle::Start(x, y) | Handle::MoveTo(x, y) => {
                    let screen = self.screen_size()?;
                    let (px, py) = pixel_on_screen((*x, *y), screen, "the drag handle point")?;
                    let p = (px as f64, py as f64);
                    (at(if matches!(h, Handle::Start(..)) { "start" } else { "move_to" }, p), p)
                }
            };
            let result = self.desktop_deliver("drag", with_session(request), args, Some(end), json!({}))?;
            self.set_handle(if matches!(h, Handle::End) { None } else { Some(end) });
            return Ok(result);
        }

        self.require_desktop_scope()?;
        let screen = self.screen_size()?;
        let (driver_tool, request, end, extra): (&str, Value, (f64, f64), Value) = match &plan {
            Plan::Click(c) => {
                let (x, y) = pixel_on_screen((c.x, c.y), screen, "the click point")?;
                let mut r = json!({"scope": "desktop", "x": x, "y": y, "count": c.count, "button": c.button});
                if !c.key.is_empty() { r["modifier"] = json!(c.key); }
                if c.hold_ms > 0 { self.require_field("click", "hold_ms", "a click with a duration")?; r["hold_ms"] = json!(c.hold_ms); }
                ("click", r, (x as f64, y as f64), json!({}))
            }
            Plan::Move(m) => {
                let (x, y) = pixel_on_screen((m.x, m.y), screen, "the move target")?;
                let mut r = json!({"scope": "desktop", "x": x, "y": y});
                if !m.key.is_empty() { self.require_field("move_cursor", "modifier", "a move under a held key")?; r["modifier"] = json!(m.key); }
                ("move_cursor", r, (x as f64, y as f64), json!({}))
            }
            Plan::Drag(d) => {
                let points: Vec<(i64, i64)> = d.path.iter().enumerate().map(|(i, p)| pixel_on_screen(*p, screen, &format!("path[{i}]"))).collect::<Result<_, _>>()?;
                let (first, last) = (points[0], points[points.len() - 1]);
                let mut r = json!({"scope": "desktop", "from_x": first.0, "from_y": first.1, "to_x": last.0, "to_y": last.1});
                if points.len() > 2 || !d.key.is_empty() {
                    // two points under no chord is the contract's own drag; anything more is native
                    self.require_field("drag", "path", "a drag through more than two points or under a held key")?;
                    r["path"] = json!(points.iter().map(|p| json!({"x": p.0, "y": p.1})).collect::<Vec<_>>());
                    if !d.key.is_empty() { r["modifier"] = json!(d.key); }
                }
                ("drag", r, (last.0 as f64, last.1 as f64), json!({}))
            }
            Plan::Scroll(s) => {
                // Sky's origin is optional: the pointer where it is, else the middle of the screen
                let origin = s.at.or_else(|| self.desktop_pointer()).unwrap_or((screen.0 / 2.0, screen.1 / 2.0));
                let (x, y) = pixel_on_screen(origin, screen, "the scroll origin")?;
                let mut r = json!({"scope": "desktop", "x": x, "y": y, "direction": s.direction, "amount": s.clicks});
                if !s.key.is_empty() { self.require_field("scroll", "modifier", "a scroll under a held key")?; r["modifier"] = json!(s.key); }
                ("scroll", r, (x as f64, y as f64), json!({"_scroll_clicks": s.clicks, "_scroll_pixels_per_click": PIXELS_PER_CLICK}))
            }
            Plan::Key(_) | Plan::Type(_) | Plan::Handle(_) => unreachable!("handled above"),
        };
        self.desktop_deliver(driver_tool, with_session(request), args, Some(end), extra)
    }

    /// The one place a desktop verb reaches the driver: wait out the last settle window, call, start
    /// this call's settle window, remember where the pointer is, and annotate the reply.
    fn desktop_deliver(&self, driver_tool: &str, request: Value, args: &Value, end: Option<(f64, f64)>, extra: Value) -> Result<Value, ToolError> {
        let waited_ms = self.settler.wait();
        let started = std::time::Instant::now();
        let mut result = self.call(driver_tool, request)?;
        let elapsed = started.elapsed().as_millis() as u64;
        self.settler.defer(args.get("post_action_sleep_ms").and_then(Value::as_u64).unwrap_or(self.cfg.features.post_action_sleep_ms));
        if let Some(end) = end { self.remember_desktop_pointer(end); }
        if let Some(o) = result.as_object_mut() {
            if waited_ms > 0 { o.insert("_waited_ms".into(), json!(waited_ms)); }
            o.insert("_elapsed_ms".into(), json!(elapsed));
            if let Some(end) = end { o.insert("_desktop_pointer".into(), json!({"x": end.0, "y": end.1})); }
            if let Some(e) = extra.as_object() { for (k, v) in e { o.insert(k.clone(), v.clone()); } }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The SKILL document and the tool list must say the same thing: every verb, every input and
    /// every refusal field the code emits is in the document, and the document names no verb that
    /// is not a tool.
    #[test]
    fn the_skill_document_and_the_tool_list_agree() {
        let defs = tool_defs();
        for name in DESKTOP_TOOLS {
            assert!(LINUX_SKILL.contains(&format!("`{name}`")), "the SKILL does not name {name}");
            let def = defs.iter().find(|d| d["name"] == name).unwrap_or_else(|| panic!("{name} is not a tool"));
            for prop in def["inputSchema"]["properties"].as_object().unwrap().keys() {
                assert!(LINUX_SKILL.contains(&format!("`{prop}`")), "the SKILL does not document {name}.{prop}");
            }
        }
        for word in LINUX_SKILL.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| w.starts_with("desktop_") && w.len() > "desktop_".len()) {
            assert!(DESKTOP_TOOLS.contains(&word), "the SKILL names {word}, which is not a desktop tool");
        }
        for needs in ["click.hold_ms", "drag.path", "move_cursor.modifier", "press_key.hold_ms", "scroll.modifier", "type_text.scope", "drag.hold"] { assert!(LINUX_SKILL.contains(needs), "{needs}"); }
        for code in [CODE_DESKTOP_UNAVAILABLE, CODE_INVALID_ARGUMENT, CODE_APP_APPROVAL_REQUIRED] {
            assert!(LINUX_SKILL.contains(code) && taxonomy(code).is_some(), "{code}");
        }
        assert!(LINUX_SKILL.contains("point_off_screen") && LINUX_SKILL.contains(DESKTOP_APP_ID));
        assert!(!LINUX_SKILL.contains('\u{2014}'), "no em-dashes in a document readers see");
        assert!(mcp_instructions().contains(LINUX_SKILL_URI), "the initialize instructions point at the resource");
    }

    #[test]
    fn sky_button_spellings_and_defaults() {
        let c = parse_click(&json!({"x": 10, "y": 20})).unwrap();
        assert_eq!((c.count, c.button, c.hold_ms, c.key.len()), (1, "left", 0, 0));
        for (given, want) in [("l", "left"), ("R", "right"), ("m", "middle"), ("middle", "middle")] {
            assert_eq!(parse_click(&json!({"x": 1, "y": 1, "mouse_button": given})).unwrap().button, want);
        }
        assert!(parse_click(&json!({"x": 1, "y": 1, "mouse_button": "x"})).is_err());
    }

    #[test]
    fn a_click_needs_finite_numbers_and_a_sane_count_and_duration() {
        for bad in [json!({"y": 1}), json!({"x": "1", "y": 1}), json!({"x": 1, "y": null}), json!({"x": 1, "y": 1, "click_count": 0}),
                    json!({"x": 1, "y": 1, "click_count": 4}), json!({"x": 1, "y": 1, "click_count": 1.5}), json!({"x": 1, "y": 1, "duration": -5}),
                    json!({"x": 1, "y": 1, "duration": 10001})] {
            let e = parse_click(&bad).unwrap_err();
            assert_eq!(e.code(), Some(CODE_INVALID_ARGUMENT), "{bad}");
        }
        let c = parse_click(&json!({"x": 1.6, "y": 2, "click_count": 2, "duration": 150})).unwrap();
        assert_eq!((c.count, c.hold_ms), (2, 150));
    }

    #[test]
    fn a_chord_becomes_driver_key_names() {
        assert_eq!(parse_chord("Control_L").unwrap(), ["ctrl"]);
        assert_eq!(parse_chord("Control_L+BackSpace").unwrap(), ["ctrl", "BackSpace"], "a key name keeps its case");
        assert_eq!(parse_press_key(&json!({"key": "Control_L+Page_Down"})).unwrap().key, "pagedown");
        assert_eq!(parse_chord("Shift_L + Alt_L").unwrap(), ["shift", "alt"]);
        assert_eq!(parse_chord("Super_L+Control_R").unwrap(), ["super", "ctrl"]);
        assert!(parse_chord("").is_err() && parse_chord(" + ").is_err());
        assert!(parse_chord("a+b+c+d+e").is_err());
        assert!(parse_click(&json!({"x": 1, "y": 1, "key": 5})).is_err());
    }

    #[test]
    fn a_drag_path_needs_two_or_more_points_each_with_x_and_y() {
        let ok = parse_drag(&json!({"path": [{"x": 0, "y": 0}, {"x": 5, "y": 5}, {"x": 9, "y": 1}], "key": "Control_L"})).unwrap();
        assert_eq!((ok.path.len(), ok.key.clone()), (3, vec!["ctrl".to_string()]));
        for bad in [json!({}), json!({"path": []}), json!({"path": [{"x": 1, "y": 1}]}), json!({"path": [{"x": 1, "y": 1}, {"x": 2}]}),
                    json!({"path": [{"x": 1, "y": 1}, "p"]}), json!({"path": {"x": 1}}), json!({"path": vec![json!({"x": 0, "y": 0}); MAX_PATH_POINTS + 1]})] {
            assert_eq!(parse_drag(&bad).unwrap_err().code(), Some(CODE_INVALID_ARGUMENT), "{bad}");
        }
    }

    #[test]
    fn a_point_must_be_on_the_screen_not_clamped_to_an_edge() {
        let screen = (800.0, 600.0);
        assert_eq!(pixel_on_screen((0.0, 0.0), screen, "p").unwrap(), (0, 0));
        assert_eq!(pixel_on_screen((799.4, 599.4), screen, "p").unwrap(), (799, 599));
        for off in [(800.0, 10.0), (10.0, 600.0), (-1.0, 5.0), (5.0, -0.6), (799.6, 5.0), (1e9, 0.0)] {
            let e = pixel_on_screen(off, screen, "p").unwrap_err();
            assert_eq!(e.structured["reason"], "point_off_screen", "{off:?}");
        }
    }
}
