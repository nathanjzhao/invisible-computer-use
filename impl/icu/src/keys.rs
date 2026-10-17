//! window2 `press_key` takes X keysym-style chords (`Control_L+Shift_L+period`,
//! `Return`, `KP_0`); cua takes `press_key {key}` for a single key and
//! `hotkey {keys: [modifiers..., key]}` for a chord, with its own key names.
//! This table is INFERRED from the two vocabularies (the golden documents the
//! keysym side, `platform-macos/src/input/keyboard.rs` the cua side).
use serde_json::{json, Value};

/// A keysym token -> cua key name (modifiers map to cua modifier names).
pub fn cua_key(token: &str) -> String {
    let t = token.trim();
    let lower = t.to_ascii_lowercase();
    let mapped = match lower.as_str() {
        "control_l" | "control_r" | "control" | "ctrl" => "ctrl",
        "shift_l" | "shift_r" | "shift" => "shift",
        // All native hotkey parsers accept alt; Windows/Linux do not classify
        // the macOS spelling option as a modifier in the keys array.
        "alt_l" | "alt_r" | "alt" | "option" => "alt",
        // on macOS X11 keysyms, Meta is the Command key; Super is mapped to Command too
        "meta_l" | "meta_r" | "meta" | "super_l" | "super_r" | "super" | "command" | "cmd" | "win" => "cmd",
        "return" | "enter" | "kp_enter" => "return",
        "tab" => "tab",
        "escape" | "esc" => "escape",
        "space" => "space",
        "backspace" => "backspace",
        "delete" | "kp_delete" => "forward_delete",
        "up" | "kp_up" => "up",
        "down" | "kp_down" => "down",
        "left" | "kp_left" => "left",
        "right" | "kp_right" => "right",
        "home" | "kp_home" => "home",
        "end" | "kp_end" => "end",
        "prior" | "page_up" | "kp_prior" => "pageup",
        "next" | "page_down" | "kp_next" => "pagedown",
        "period" => ".", "comma" => ",", "minus" => "-", "plus" => "+", "equal" => "=", "slash" => "/",
        "backslash" => "\\", "semicolon" => ";", "apostrophe" => "'", "grave" => "`",
        "bracketleft" => "[", "bracketright" => "]", "less" => ",", "greater" => ".",
        "insert" => "insert", "print" => "print",
        _ => "",
    };
    if !mapped.is_empty() { return mapped.to_string(); }
    for prefix in ["kp_", "numpad_"] {
        if let Some(d) = lower.strip_prefix(prefix) { if d.len() == 1 && d.chars().all(|c| c.is_ascii_digit()) { return d.to_string(); } }
    }
    if lower.starts_with('f') && lower[1..].chars().all(|c| c.is_ascii_digit()) && lower.len() <= 3 { return lower; }
    // single characters and anything else pass through lower-cased (`a`, `1`)
    lower
}

const MODIFIERS: [&str; 4] = ["ctrl", "shift", "alt", "cmd"];

/// Split a chord on `+` (whitespace around `+` ignored, per the golden) into the
/// cua tool + arguments: one non-modifier key -> `press_key`, a chord -> `hotkey`.
pub fn translate_chord(chord: &str) -> Result<(&'static str, Value), String> {
    let tokens: Vec<String> = chord.split('+').map(cua_key).filter(|t| !t.is_empty()).collect();
    if tokens.is_empty() { return Err(format!("empty key chord {chord:?}")); }
    let (mods, keys): (Vec<&String>, Vec<&String>) = tokens.iter().partition(|t| MODIFIERS.contains(&t.as_str()));
    match keys.len() {
        0 => Err(format!("key chord {chord:?} has no non-modifier key")),
        1 if mods.is_empty() => Ok(("press_key", json!({"key": keys[0]}))),
        1 => Ok(("hotkey", json!({"keys": mods.iter().map(|m| m.as_str()).chain(std::iter::once(keys[0].as_str())).collect::<Vec<_>>()}))),
        _ => Err(format!("key chord {chord:?} has more than one non-modifier key")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_examples_translate() {
        assert_eq!(translate_chord("a").unwrap(), ("press_key", json!({"key": "a"})));
        assert_eq!(translate_chord("space").unwrap(), ("press_key", json!({"key": "space"})));
        assert_eq!(translate_chord("Return").unwrap(), ("press_key", json!({"key": "return"})));
        assert_eq!(translate_chord("Tab").unwrap(), ("press_key", json!({"key": "tab"})));
        assert_eq!(translate_chord("Control_L+a").unwrap(), ("hotkey", json!({"keys": ["ctrl", "a"]})));
        assert_eq!(translate_chord("Control_L + Shift_L + period").unwrap(), ("hotkey", json!({"keys": ["ctrl", "shift", "."]})));
        assert_eq!(translate_chord("KP_0").unwrap(), ("press_key", json!({"key": "0"})));
        assert_eq!(translate_chord("Super_L+c").unwrap(), ("hotkey", json!({"keys": ["cmd", "c"]})));
        assert_eq!(translate_chord("Meta_L+c").unwrap(), ("hotkey", json!({"keys": ["cmd", "c"]})), "Meta is Command on macOS X11");
        assert_eq!(translate_chord("Numpad_0").unwrap(), ("press_key", json!({"key": "0"})));
        assert_eq!(translate_chord("greater").unwrap(), ("press_key", json!({"key": "."})), "the unshifted key; shift is the caller's chord");
    }

    #[test]
    fn malformed_chords_are_rejected() {
        assert!(translate_chord("").is_err());
        assert!(translate_chord("Control_L").is_err(), "modifier alone");
        assert!(translate_chord("a+b").is_err(), "two keys");
    }
}
