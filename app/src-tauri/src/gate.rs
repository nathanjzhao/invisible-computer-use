//! The confirmation gate for a run given unverified memory.
//!
//! A run is tainted when any memory entry given to the model was shown as an
//! unverified note (`memory::taints`): an observed entry, a Skysight line, or a
//! clean entry with none of the user's sentences to show. In a tainted run, a
//! step this module calls consequential (it sends, pays, deletes, submits or
//! publishes) waits for the user's yes in the app before it is dispatched. The
//! decision is code over what the action layer knows: the action, the element
//! it resolves to in the observation it was decided on, the focused element,
//! the keys pressed and the app. It never reads the memory text or the goal.
//!
//! How good the classification is: it reads English labels, titles,
//! descriptions, help text and identifiers of the target. It misses a button
//! whose only text is in another language or an icon with no text at all, a
//! send done by a key the rules below do not name, a step whose effect depends
//! on app state the tree does not show (a Return that submits a web form with
//! no visible submit button), and anything done outside the window. It
//! over-asks on labels that only open something ("Share" opening a sheet,
//! "Remove filter"), on Return in a single-line field, or with no known focus,
//! in a window that shows a "Send" or "Delete" button anywhere, on Delete keys
//! with no known focus in a file manager or a list with no text field, and on
//! every drag. GAPS.md lists what is left.
//!
//! What was decided about uncertainty (this gate is the boundary for a tainted
//! run, so doubt asks unless the doubt is provably harmless):
//! - The click's action string is read. `Name:Delete` style custom actions are
//!   read as control text; an unknown custom action on a row or cell asks.
//! - OK, Yes, Continue, Accept, Allow, Overwrite, Replace, Install, Reset and
//!   their obvious variants ask only when the tree shows a dialog, sheet or
//!   alert (the button's own text says nothing). The gate does not read the
//!   goal, so a bare "OK" in a plain window does not ask.
//! - Return in a focused multi-line text area does not ask because of a
//!   toolbar button, since Return cannot press a default button there.

use crate::brain::Action;
use serde_json::Value;

/// The user's answer to one confirmation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Consent { Approved, Declined, TimedOut, Stopped }

impl Consent {
    pub(crate) fn word(self) -> &'static str {
        match self { Consent::Approved => "approved", Consent::Declined => "declined", Consent::TimedOut => "timeout", Consent::Stopped => "stopped" }
    }
}

/// Asks the user. `open` registers the request before it is shown, so an
/// answer that comes at once is not lost; `wait` blocks for the answer.
pub(crate) trait Confirm {
    fn open(&self, id: &str);
    /// What the request asks (`{id, action, why, app}`), kept for a window that reloads while it waits.
    fn show(&self, _id: &str, _info: &Value) {}
    fn wait(&self, id: &str) -> Consent;
}

/// No one to ask (a headless run): every request is declined.
pub(crate) struct NoOne;
impl Confirm for NoOne {
    fn open(&self, _: &str) {}
    fn wait(&self, _: &str) -> Consent { Consent::Declined }
}

/// What the loop needs to gate a run.
pub(crate) struct Gate<'a> {
    /// An unverified memory note was given to the model this run.
    pub tainted: bool,
    pub bundle_id: Option<&'a str>,
    pub confirm: &'a dyn Confirm,
}

impl Gate<'static> {
    /// A run given no unverified note: nothing is gated.
    pub(crate) fn open() -> Gate<'static> { Gate { tainted: false, bundle_id: None, confirm: &NoOne } }
}

/// Phrases on a control that make pressing it consequential, with what it
/// does. Matched as whole words of the control's text.
const PHRASES: &[(&str, &str)] = &[
    ("send", "sends"), ("share", "sends"), ("upload", "sends"), ("invite", "sends"), ("tweet", "sends"),
    ("reply all", "sends"),
    ("publish", "publishes"), ("post", "publishes"), ("deploy", "publishes"),
    ("merge pull request", "publishes"), ("squash and merge", "publishes"), ("rebase and merge", "publishes"),
    ("pay", "pays"), ("buy", "pays"), ("purchase", "pays"), ("checkout", "pays"), ("check out", "pays"),
    ("place order", "pays"), ("order now", "pays"), ("submit order", "pays"), ("subscribe", "pays"), ("donate", "pays"),
    ("tip", "pays"), ("transfer", "pays"), ("wire", "pays"),
    ("delete", "deletes"), ("remove", "deletes"), ("trash", "deletes"), ("erase", "deletes"), ("discard", "deletes"),
    ("destroy", "deletes"), ("wipe", "deletes"), ("uninstall", "deletes"), ("empty trash", "deletes"), ("empty bin", "deletes"),
    ("move to bin", "deletes"), ("permanently", "deletes"),
    ("submit", "submits"), ("confirm", "submits"), ("approve", "submits"),
    ("unsubscribe", "unsubscribes"), ("sign out", "signs out"), ("log out", "signs out"), ("clear all", "deletes"),
];

/// Button texts that say nothing about what they commit to: the question is in
/// the dialog, not on the button. They ask only when a dialog is showing.
const DIALOG_COMMITS: &[&str] = &["ok", "yes", "continue", "accept", "allow", "overwrite", "replace", "install", "reset", "proceed", "agree"];
/// Apps where Checkout switches a branch (a version-control client or editor), not a payment.
const VCS_APPS: &[&str] = &["github desktop", "fork", "sourcetree", "tower", "gitkraken", "sublime merge", "gitx", "gitup", "xcode",
    "visual studio code", "code", "cursor"];
/// Roles that are a navigation entry (a sidebar row, a list item), not a button.
const NAV_ROLES: &[&str] = &["row", "outline", "cell", "listitem", "list item", "treeitem", "tree item", "sourcelist"];
/// Apps where Delete or BackSpace with no known focus acts on a selected file or message.
const LIST_APPS: &[&str] = &["finder", "explorer", "file explorer", "windows explorer", "files", "nautilus", "dolphin", "thunar", "nemo",
    "pcmanfm", "mail", "outlook", "reminders", "thunderbird"];
const LIST_BUNDLES: &[&str] = &["com.apple.finder", "com.apple.mail", "com.apple.reminders", "com.microsoft.Outlook", "org.mozilla.thunderbird"];

/// Apps where Return in the message field sends it.
const CHAT_BUNDLES: &[&str] = &["com.apple.MobileSMS", "com.apple.iChat", "com.tinyspeck.slackmacgap", "com.hnc.Discord", "net.whatsapp.WhatsApp",
    "desktop.WhatsApp", "ru.keepcoder.Telegram", "org.telegram.desktop", "com.microsoft.teams", "com.microsoft.teams2",
    "org.whispersystems.signal-desktop", "com.facebook.archon", "us.zoom.xos"];
const CHAT_NAMES: &[&str] = &["messages", "slack", "discord", "whatsapp", "telegram", "microsoft teams", "signal", "messenger", "zoom"];
/// Apps where Return runs what was typed.
const TERMINAL_BUNDLES: &[&str] = &["com.apple.Terminal", "com.googlecode.iterm2", "dev.warp.Warp-Stable", "com.mitchellh.ghostty",
    "net.kovidgoyal.kitty", "io.alacritty", "com.github.wez.wezterm", "co.zeit.hyper", "org.tabby"];
const TERMINAL_NAMES: &[&str] = &["terminal", "terminal.app", "iterm", "iterm2", "warp", "ghostty", "kitty", "alacritty", "wezterm", "konsole", "xterm",
    "windows powershell", "powershell", "command prompt", "windows terminal", "windowsterminal", "hyper", "tabby",
    "gnome-terminal", "gnome-terminal-server", "tilix", "terminator", "xfce4-terminal", "rio"];
/// Editors with an integrated terminal: Return runs a command only when the
/// focus is in the terminal, which the tree has to show (see `enter_reason`).
const EDITOR_BUNDLES: &[&str] = &["com.microsoft.VSCode", "com.microsoft.VSCodeInsiders", "com.todesktop.230313mzl4w4u92", "com.vscodium"];
const EDITOR_NAMES: &[&str] = &["code", "visual studio code", "cursor", "vscodium", "windsurf"];

/// The words of a control's text: camelCase and separators split, lower case.
fn words(text: &str) -> Vec<String> {
    let mut spaced = String::with_capacity(text.len() + 8);
    let mut prev: Option<char> = None;
    for c in text.chars() {
        if c.is_uppercase() && prev.is_some_and(|p| p.is_lowercase()) { spaced.push(' '); }
        spaced.push(c);
        prev = Some(c);
    }
    spaced.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
}

/// What a control's text says it does, if it is one of [`PHRASES`].
fn phrase_in(text: &str) -> Option<(&'static str, &'static str)> {
    let w = words(text);
    PHRASES.iter().copied().find(|(p, _)| {
        let p: Vec<&str> = p.split(' ').collect();
        w.windows(p.len()).any(|win| win.iter().zip(&p).all(|(a, b)| a == b))
    })
}

/// A text role: typing into it, or reading it, is not pressing anything.
fn is_text_role(e: &Value) -> bool {
    let role = e.get("role").and_then(Value::as_str).unwrap_or("").to_lowercase();
    ["text", "edit", "entry", "search", "combo"].iter().any(|k| role.contains(k))
}

/// A multi-line text role: Return there is a line break, it presses no default button.
fn is_multiline_role(e: &Value) -> bool {
    let role = e.get("role").and_then(Value::as_str).unwrap_or("").to_lowercase();
    ["textarea", "text area", "multiline", "richedit", "document"].iter().any(|k| role.contains(k))
}

fn role_of(e: &Value) -> String { e.get("role").and_then(Value::as_str).unwrap_or("").to_lowercase() }

/// A dialog, sheet or alert shows in the observation (a role or subrole says so).
fn dialog_shown(state: &Value) -> bool {
    state.get("elements").and_then(Value::as_array).into_iter().flatten().any(|e| {
        ["role", "subrole"].iter().filter_map(|k| e.get(*k).and_then(Value::as_str))
            .any(|r| { let r = r.to_lowercase(); ["sheet", "dialog", "alert"].iter().any(|k| r.contains(k)) })
    })
}

/// The fields of an element that say what it does.
const TEXT_FIELDS: &[&str] = &["label", "title", "description", "help", "identifier", "name"];

/// Why pressing `e` is consequential, from its own text. `app` and `state`
/// settle the labels that depend on where they are (Checkout, a bare Trash
/// row, OK in a dialog).
fn element_reason(e: &Value, state: &Value, app: &str) -> Option<String> {
    if is_text_role(e) { return None; }
    let nav = NAV_ROLES.iter().any(|r| role_of(e).contains(r));
    TEXT_FIELDS.iter().filter_map(|k| e.get(*k).and_then(Value::as_str)).find_map(|t| {
        let shown = || t.chars().take(60).collect::<String>();
        if let Some((phrase, does)) = phrase_in(t) {
            let w = words(t);
            // A sidebar row named only "Trash" opens the folder.
            if nav && w == ["trash"] { return None; }
            // Checkout in a version-control client switches branch.
            if phrase == "checkout" && VCS_APPS.iter().any(|a| app.trim().eq_ignore_ascii_case(a)) { return None; }
            if phrase == "check out" && VCS_APPS.iter().any(|a| app.trim().eq_ignore_ascii_case(a)) { return None; }
            return Some(format!("its target {:?} {does}", shown()));
        }
        // "OK", "Replace All", "Install Now": the question is in the dialog.
        let w = words(t);
        if w.len() <= 3 && w.first().is_some_and(|f| DIALOG_COMMITS.contains(&f.as_str())) && dialog_shown(state) {
            return Some(format!("its target {:?} answers a dialog that is showing", shown()));
        }
        None
    })
}

/// Why a click with accessibility action `ax` is consequential beyond the
/// control's own text: a `Name:...` custom action is read as text, and an
/// unknown custom action on a row or cell asks, since what it runs is not shown.
fn action_reason(ax: &str, target: Option<&Value>) -> Option<String> {
    let name = ax.trim().strip_prefix("Name:").or_else(|| ax.trim().strip_prefix("name:"))?;
    let name = name.lines().next().unwrap_or("").trim();
    if let Some((_, does)) = phrase_in(name) { return Some(format!("its custom action {name:?} {does}")); }
    let row = target.is_some_and(|e| { let r = role_of(e); NAV_ROLES.iter().any(|k| r.contains(k)) });
    row.then(|| format!("its custom action {name:?} runs something the window does not show"))
}

fn focused(state: &Value) -> Option<&Value> {
    state.get("elements")?.as_array()?.iter().find(|e| e.get("focused").and_then(Value::as_bool) == Some(true))
}

fn app_in(bundle: Option<&str>, app: &str, bundles: &[&str], names: &[&str]) -> bool {
    bundle.is_some_and(|b| bundles.iter().any(|x| x.eq_ignore_ascii_case(b)))
        || names.iter().any(|n| app.trim().eq_ignore_ascii_case(n))
}

/// Some element's own text (any text field) mentions `word`.
fn mentions(e: &Value, word: &str) -> bool {
    TEXT_FIELDS.iter().filter_map(|k| e.get(*k).and_then(Value::as_str)).any(|t| words(t).iter().any(|w| w == word))
}

/// Why Return (or a typed line break, or an accessibility confirm) is
/// consequential here: in a chat app it sends, in a terminal it runs the
/// command, and in a single-line field or with no known focus it may press the
/// default button, which the tree does not mark, so a window showing a
/// consequential control counts. In a multi-line text area it is a line break.
fn enter_reason(state: &Value, bundle: Option<&str>, app: &str) -> Option<String> {
    if app_in(bundle, app, CHAT_BUNDLES, CHAT_NAMES) { return Some(format!("Return sends the message in {app}")); }
    if app_in(bundle, app, TERMINAL_BUNDLES, TERMINAL_NAMES) { return Some(format!("Return runs the command in {app}")); }
    let focus = focused(state);
    if app_in(bundle, app, EDITOR_BUNDLES, EDITOR_NAMES) {
        // An editor's Return is a line break unless the focus is in its terminal,
        // or the focus is not known and a terminal is on screen.
        let in_terminal = match focus {
            Some(f) => mentions(f, "terminal"),
            None => state.get("elements").and_then(Value::as_array).into_iter().flatten().any(|e| mentions(e, "terminal")),
        };
        if in_terminal { return Some(format!("Return can run a command in the terminal of {app}")); }
    }
    if let Some(why) = focus.and_then(|f| element_reason(f, state, app)) { return Some(why.replacen("its target", "the focused control", 1)); }
    // Return in a multi-line text area inserts a line break; no default button is pressed.
    if focus.is_some_and(is_multiline_role) { return None; }
    let shown = state.get("elements").and_then(Value::as_array).into_iter().flatten().find_map(|e| element_reason(e, state, app))?;
    Some(format!("Return can press a default button, and {}", shown.replacen("its target", "the window shows", 1)))
}

/// Whether the tree can tell where a Delete key lands. With a focused element
/// it is known. Without one, a text field on screen means it may be editing text.
fn delete_lands_on_item(state: &Value, bundle: Option<&str>, app: &str) -> bool {
    if app_in(bundle, app, LIST_BUNDLES, LIST_APPS) { return true; }
    let els: Vec<&Value> = state.get("elements").and_then(Value::as_array).into_iter().flatten().collect();
    let list = els.iter().any(|e| { let r = role_of(e); NAV_ROLES.iter().any(|k| r.contains(k)) || r.contains("list") || r.contains("table") });
    list && !els.iter().any(|e| is_text_role(e))
}

/// A key chord as (modifiers, key), lower case. A key that is only blanks is a space.
fn chord(key: &str) -> (Vec<String>, String) {
    let mut parts: Vec<String> = key.split('+').map(|p| if !p.is_empty() && p.trim().is_empty() { "space".to_string() } else { p.trim().to_lowercase() })
        .filter(|p| !p.is_empty()).collect();
    if key.trim().is_empty() && !key.is_empty() { parts = vec!["space".into()]; }
    let Some((last, mods)) = parts.split_last() else { return (vec![], String::new()) };
    let modifier = |m: &str| -> String {
        let m = m.trim_end_matches("_l").trim_end_matches("_r");
        match m { "command" | "cmd" | "super" | "meta" | "win" | "windows" | "os" | "lwin" | "rwin" => "cmd", "control" | "ctrl" => "ctrl",
                  "option" | "opt" | "alt" => "alt", other => other }.to_string()
    };
    (mods.iter().map(|m| modifier(m)).collect(), last.clone())
}

/// Why `action`, run against the observation `state` in `app`, is consequential:
/// it sends, pays, deletes, submits or publishes. `target` is the element a
/// click or set_value resolves to, when the observation has it. `None` means
/// the step goes ahead without asking.
pub(crate) fn consequential(action: &Action, target: Option<&Value>, state: &Value, bundle: Option<&str>, app: &str) -> Option<String> {
    match action {
        Action::Click { target: label, action: ax, .. } => {
            let ax_trim = ax.as_deref().map(str::trim).unwrap_or("");
            match ax_trim {
                "cancel" | "show_menu" => return None,
                "confirm" => return enter_reason(state, bundle, app),
                _ => {}
            }
            if let Some(why) = action_reason(ax_trim, target) { return Some(why); }
            match target {
                Some(e) => element_reason(e, state, app),
                // Unresolved: the label the model asked for is all there is.
                None => label.as_deref().and_then(phrase_in).map(|(_, does)| format!("its target {:?} {does}", label.as_deref().unwrap_or(""))),
            }
        }
        // Setting a text field is typing; setting a switch or a menu is pressing it.
        Action::SetValue { .. } => target.and_then(|e| element_reason(e, state, app)),
        Action::Type { text } if text.contains(['\n', '\r']) => enter_reason(state, bundle, app),
        Action::Type { .. } => None,
        Action::Key { key } => {
            let (mods, k) = chord(key);
            let has = |m: &str| mods.iter().any(|x| x == m);
            let enter = matches!(k.as_str(), "return" | "enter" | "kp_enter" | "keypad_enter" | "numpad_enter" | "keypadenter" | "numpadenter" | "linefeed");
            let delete = matches!(k.as_str(), "delete" | "backspace" | "back_space" | "forwarddelete" | "forward_delete" | "kp_delete" | "del");
            if enter && (has("cmd") || has("ctrl")) { return Some(format!("{key} sends in most mail and chat apps")); }
            // Shift+Return is a line break in chat and text fields. A terminal may still run it.
            if enter && has("shift") && !app_in(bundle, app, TERMINAL_BUNDLES, TERMINAL_NAMES) { return None; }
            if enter { return enter_reason(state, bundle, app); }
            if delete {
                let in_text = focused(state).is_some_and(is_text_role);
                // In a text field every Delete chord (word, line, Shift) edits text.
                if in_text { return None; }
                if has("cmd") || has("ctrl") || has("shift") { return Some(format!("{key} deletes the selected item in most apps")); }
                return match focused(state) {
                    Some(_) => Some(format!("{key} with no text field focused can delete the selected item")),
                    // No focus reported (Windows and Linux never report it): ask only where the key acts on a selected item.
                    None if delete_lands_on_item(state, bundle, app) => Some(format!("{key} with no known focus can delete the selected item in {app}")),
                    None => None,
                };
            }
            if has("cmd") && has("shift") && k == "d" { return Some(format!("{key} sends the message in Mail")); }
            if k == "space" && mods.is_empty() { return focused(state).and_then(|f| element_reason(f, state, app)).map(|w| w.replacen("its target", "the focused control", 1)); }
            None
        }
        Action::Drag { .. } => Some("where a drag drops is not known before it runs".into()),
        Action::PixelClick { .. } => Some("a screenshot coordinate has no verified control label; this run used untrusted memory".into()),
        Action::Scroll { .. } | Action::Done { .. } | Action::Fail { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn click(label: &str) -> Action { Action::Click { target: Some(label.into()), element_index: -1, button: None, count: None, action: None } }
    fn key(k: &str) -> Action { Action::Key { key: k.into() } }
    fn el(role: &str, label: &str) -> Value { json!({"element_index": 0, "role": role, "label": label}) }

    #[test]
    fn controls_are_read_by_their_own_text() {
        let s = json!({"elements": []});
        for (role, label) in [("AXButton", "Send"), ("AXButton", "Send Now"), ("AXMenuItem", "Move to Trash"), ("AXButton", "Pay $12.00"),
                              ("AXButton", "Place order"), ("AXButton", "Delete local copy"), ("AXLink", "Publish"), ("AXButton", "Submit"),
                              ("AXButton", "Check out"), ("AXButton", "Empty Trash"), ("button", "Upload")] {
            assert!(consequential(&click(label), Some(&el(role, label)), &s, None, "App").is_some(), "{role} {label}");
        }
        // Identifiers and descriptions count, camelCase split.
        let icon = json!({"element_index": 0, "role": "AXButton", "identifier": "trashButton"});
        assert!(consequential(&click(""), Some(&icon), &s, None, "Mail").unwrap().contains("trashButton"));
        let described = json!({"element_index": 0, "role": "AXButton", "label": "", "description": "Send message"});
        assert!(consequential(&click(""), Some(&described), &s, None, "Mail").is_some());
        for (role, label) in [("AXButton", "Save"), ("AXButton", "Sending options"), ("AXButton", "Posted"), ("AXButton", "Cancel"),
                              ("AXButton", "Tips"), ("AXTextField", "Send to"), ("AXStaticText", "Delete"), ("AXButton", "6")] {
            assert!(consequential(&click(label), Some(&el(role, label)), &s, None, "App").is_none(), "{role} {label}");
        }
        // Opening a menu or cancelling is not the action itself.
        let menu = Action::Click { target: Some("Delete".into()), element_index: -1, button: None, count: None, action: Some("show_menu".into()) };
        assert!(consequential(&menu, Some(&el("AXButton", "Delete")), &s, None, "App").is_none());
        // Unresolved, the model's own label is read.
        assert!(consequential(&click("Send"), None, &s, None, "App").is_some());
        // A switch named for deleting is pressed by set_value; a text field is typed into.
        let set = Action::SetValue { target: Some("x".into()), element_index: -1, value: "1".into() };
        assert!(consequential(&set, Some(&el("AXCheckBox", "Delete after upload")), &s, None, "App").is_some());
        assert!(consequential(&set, Some(&el("AXTextField", "Delete after upload")), &s, None, "App").is_none());
    }

    #[test]
    fn keys_are_read_with_the_app_and_the_focus() {
        let plain = json!({"elements": [el("AXButton", "6")]});
        let field = json!({"elements": [{"element_index": 0, "role": "AXTextField", "label": "Message", "focused": true}]});
        let mail = json!({"elements": [el("AXTextArea", "Body"), {"element_index": 1, "role": "AXButton", "label": "Send"}]});
        for k in ["cmd+Return", "Control_L+Return", "super+Delete", "cmd+BackSpace", "shift+Delete", "cmd+shift+d"] {
            assert!(consequential(&key(k), None, &plain, None, "Notes").is_some(), "{k}");
        }
        assert!(consequential(&key("Return"), None, &plain, None, "Calculator").is_none());
        assert!(consequential(&key("Return"), None, &field, Some("com.apple.MobileSMS"), "Messages").unwrap().contains("sends the message"));
        assert!(consequential(&key("Return"), None, &field, None, "Slack").is_some(), "by name when there is no bundle id");
        assert!(consequential(&key("Return"), None, &plain, Some("com.apple.Terminal"), "Terminal").unwrap().contains("runs the command"));
        assert!(consequential(&key("Return"), None, &mail, None, "Mail").unwrap().contains("\"Send\""));
        // Delete in a focused text field edits text; with no focus it may delete an item.
        assert!(consequential(&key("BackSpace"), None, &field, None, "Notes").is_none());
        assert!(consequential(&key("Delete"), None, &plain, None, "Mail").is_some());
        assert!(consequential(&key("cmd+a"), None, &plain, None, "Notes").is_none());
        // A typed line break is a Return.
        let typed = Action::Type { text: "hi\n".into() };
        assert!(consequential(&typed, None, &field, None, "Messages").is_some());
        assert!(consequential(&Action::Type { text: "hi".into() }, None, &field, None, "Messages").is_none());
        // Drags are not known; scrolls never matter.
        assert!(consequential(&Action::Drag { from_x: 0.0, from_y: 0.0, to_x: 1.0, to_y: 1.0 }, None, &plain, None, "Finder").is_some());
        assert!(consequential(&Action::Scroll { dy:-3, target:None, element_index:-1, amount:3, by:None }, None, &plain, None, "Mail").is_none());
    }

    // ---- the review's cases (study/audit/review-2026-10-01-rest-correct) ----

    fn click_with(label: &str, ax: &str) -> Action { Action::Click { target: Some(label.into()), element_index: -1, button: None, count: None, action: Some(ax.into()) } }
    fn ask(a: &Action, target: Option<&Value>, state: &Value, bundle: Option<&str>, app: &str) -> bool { consequential(a, target, state, bundle, app).is_some() }
    fn field() -> Value { json!({"elements": [{"element_index": 0, "role": "AXTextField", "label": "Message", "focused": true}]}) }
    fn empty() -> Value { json!({"elements": []}) }

    #[test]
    fn a_custom_action_is_read_as_what_it_runs() {
        let row = el("AXRow", "Re: invoice 4411");
        assert!(ask(&click_with("Re: invoice 4411", "Name:Delete\nTarget:0x0\nSelector:(null)"), Some(&row), &empty(), None, "Mail"));
        // A name the gate does not know, on a row: what it runs is not shown, so it asks.
        let why = consequential(&click_with("x", "Name:Frobnicate\nTarget:0x0"), Some(&row), &empty(), None, "Mail").unwrap();
        assert!(why.contains("Frobnicate"), "{why}");
        // On a button the control's own text decides, and a harmless custom name does not ask.
        assert!(!ask(&click_with("Save", "Name:Highlight"), Some(&el("AXButton", "Save")), &empty(), None, "App"));
        // The plain actions still behave as before.
        assert!(!ask(&click_with("Re: invoice", "press"), Some(&row), &empty(), None, "Mail"));
    }

    #[test]
    fn answer_buttons_ask_only_in_a_dialog() {
        let dialog = json!({"elements": [{"element_index": 0, "role": "AXSheet", "label": "Delete 12 files?"}, el("AXButton", "OK")]});
        let plain = json!({"elements": [el("AXButton", "OK")]});
        for label in ["OK", "Yes", "Continue", "Accept", "Allow", "Overwrite", "Replace", "Install", "Reset", "Replace All", "Continue Anyway", "Install Now"] {
            let b = el("AXButton", label);
            assert!(ask(&click(label), Some(&b), &dialog, None, "Finder"), "{label} in a dialog");
            assert!(!ask(&click(label), Some(&b), &plain, None, "Finder"), "{label} with no dialog");
        }
        let alert = json!({"elements": [{"element_index": 0, "role": "AXGroup", "subrole": "AXSystemDialog"}]});
        assert!(ask(&click("OK"), Some(&el("AXButton", "OK")), &alert, None, "Finder"), "subrole is read");
        // "Don't Allow" and long sentences are not answer buttons.
        assert!(!ask(&click("Don't Allow"), Some(&el("AXButton", "Don't Allow")), &dialog, None, "Finder"));
        assert!(!ask(&click("OK to proceed with the sample document"), Some(&el("AXButton", "OK to proceed with the sample document")), &dialog, None, "Finder"));
        // Words that say what they do ask without a dialog.
        for label in ["Unsubscribe", "Sign out", "Clear All"] {
            assert!(ask(&click(label), Some(&el("AXButton", label)), &plain, None, "Mail"), "{label}");
        }
    }

    #[test]
    fn terminals_editors_and_key_spellings() {
        let plain = json!({"elements": [el("AXButton", "6")]});
        for app in ["Windows Terminal", "Hyper", "Tabby", "gnome-terminal-server", "Terminal.app"] {
            assert!(ask(&key("Return"), None, &plain, None, app), "{app}");
        }
        assert!(ask(&key("Return"), None, &plain, Some("co.zeit.hyper"), "x"));
        // An editor asks only when its terminal is the focus, or on screen with no focus known.
        let term_focus = json!({"elements": [{"element_index": 0, "role": "Edit", "label": "Terminal 1, bash", "focused": true}]});
        let editor_focus = json!({"elements": [{"element_index": 0, "role": "AXTextArea", "label": "main.rs", "focused": true},
                                               {"element_index": 1, "role": "AXTab", "label": "Terminal"}]});
        let term_shown = json!({"elements": [{"element_index": 0, "role": "Tab", "label": "Terminal"}]});
        for app in ["Code", "Cursor"] {
            assert!(ask(&key("Return"), None, &term_focus, None, app), "{app} terminal focus");
            assert!(ask(&key("Return"), None, &term_shown, None, app), "{app} focus unknown, terminal shown");
            assert!(!ask(&key("Return"), None, &editor_focus, None, app), "{app} editor focus");
            assert!(!ask(&key("Return"), None, &plain, None, app), "{app} no terminal on screen");
        }
        let chat = field();
        for k in ["keypad_enter", "KP_Enter", "numpad_enter"] { assert!(ask(&key(k), None, &chat, Some("com.apple.MobileSMS"), "Messages"), "{k}"); }
        for k in ["os+Return", "lwin+Return", "Win_R+Return", "rwin+Return"] { assert!(ask(&key(k), None, &plain, None, "Notes"), "{k}"); }
        let send_focused = json!({"elements": [{"element_index": 0, "role": "AXButton", "label": "Send", "focused": true}]});
        for k in [" ", "space", "Space"] { assert!(ask(&key(k), None, &send_focused, None, "Mail"), "{k:?}"); }
        assert!(!ask(&key(" "), None, &field(), None, "Mail"), "a space typed into a field");
    }

    #[test]
    fn return_in_a_multiline_text_area_is_a_line_break() {
        let notes = json!({"elements": [
            {"element_index": 0, "role": "AXTextArea", "label": "Note body", "focused": true},
            {"element_index": 1, "role": "AXButton", "label": "Share"}]});
        assert!(!ask(&key("Return"), None, &notes, Some("com.apple.Notes"), "Notes"));
        assert!(!ask(&Action::Type { text: "a\nb".into() }, None, &notes, Some("com.apple.Notes"), "Notes"));
        // A single-line field with the same toolbar can press the default button.
        let single = json!({"elements": [{"element_index": 0, "role": "AXTextField", "label": "Title", "focused": true},
                                         {"element_index": 1, "role": "AXButton", "label": "Share"}]});
        assert!(ask(&key("Return"), None, &single, Some("com.apple.Notes"), "Notes"));
        // Focus unknown: doubt asks.
        let unknown = json!({"elements": [{"element_index": 0, "role": "AXTextArea", "label": "Note body"}, {"element_index": 1, "role": "AXButton", "label": "Share"}]});
        assert!(ask(&key("Return"), None, &unknown, Some("com.apple.Notes"), "Notes"));
        // A chat app's compose box is a text area too, and Return there still sends.
        let slack = json!({"elements": [{"element_index": 0, "role": "AXTextArea", "label": "Message", "focused": true}]});
        assert!(ask(&key("Return"), None, &slack, Some("com.tinyspeck.slackmacgap"), "Slack"));
        // Return on a focused Send button still asks.
        let button = json!({"elements": [{"element_index": 0, "role": "AXButton", "label": "Send", "focused": true}]});
        assert!(ask(&key("Return"), None, &button, None, "Mail"));
    }

    #[test]
    fn shift_return_and_word_delete_in_a_field_edit_text() {
        let f = field();
        assert!(!ask(&key("shift+Return"), None, &f, Some("com.tinyspeck.slackmacgap"), "Slack"));
        assert!(!ask(&key("Shift_L+Return"), None, &f, Some("com.apple.MobileSMS"), "Messages"));
        // A terminal can run it, and ctrl or cmd with Return still sends.
        assert!(ask(&key("shift+Return"), None, &f, Some("com.apple.Terminal"), "Terminal"));
        assert!(ask(&key("ctrl+shift+Return"), None, &f, None, "Slack"));
        for k in ["ctrl+BackSpace", "cmd+Delete", "shift+Delete", "ctrl+Delete"] {
            assert!(!ask(&key(k), None, &f, None, "Notepad"), "{k} in a text field");
            // Out of a text field the chord acts on the selected item.
            assert!(ask(&key(k), None, &json!({"elements": [el("AXButton", "6")]}), None, "Notepad"), "{k} outside a text field");
        }
    }

    #[test]
    fn delete_with_no_reported_focus_asks_only_for_items() {
        // Windows and Linux never report `focused`.
        let text = json!({"elements": [{"element_index": 0, "role": "Edit", "label": "Message"}]});
        assert!(!ask(&key("BackSpace"), None, &text, None, "Notepad"));
        assert!(!ask(&key("Delete"), None, &text, None, "Word"));
        // A file manager or mail client acts on the selected item.
        assert!(ask(&key("Delete"), None, &text, None, "File Explorer"));
        assert!(ask(&key("BackSpace"), None, &text, Some("com.apple.finder"), "Finder"));
        assert!(ask(&key("Delete"), None, &empty(), None, "Mail"));
        // A list with no text field on screen, in an app not named above.
        let list = json!({"elements": [el("Table", "Files"), el("ListItem", "report.pdf")]});
        assert!(ask(&key("Delete"), None, &list, None, "Some App"));
        // A focused non-text control still asks.
        let button = json!({"elements": [{"element_index": 0, "role": "AXButton", "label": "6", "focused": true}]});
        assert!(ask(&key("Delete"), None, &button, None, "Notes"));
        // Nothing to read at all and not a list app: not asked.
        assert!(!ask(&key("Delete"), None, &empty(), None, "Calculator"));
    }

    #[test]
    fn forward_trash_checkout_and_merge_labels() {
        let s = empty();
        // Forward opens a draft; its Send is gated. Merge Cells is spreadsheet formatting.
        assert!(!ask(&click("Forward"), Some(&el("AXButton", "Forward")), &s, None, "Safari"));
        assert!(!ask(&click("Merge Cells"), Some(&el("AXButton", "Merge Cells")), &s, None, "Numbers"));
        // Merging a pull request is a publish.
        for label in ["Merge pull request", "Squash and merge", "Rebase and merge"] {
            assert!(ask(&click(label), Some(&el("AXButton", label)), &s, None, "Safari"), "{label}");
        }
        // A sidebar row named Trash opens the folder; a Trash button or Move to Trash deletes.
        for role in ["AXRow", "AXOutlineRow", "AXCell", "ListItem"] {
            assert!(!ask(&click("Trash"), Some(&el(role, "Trash")), &s, None, "Mail"), "{role}");
        }
        assert!(ask(&click("Trash"), Some(&el("AXButton", "Trash")), &s, None, "Mail"));
        assert!(ask(&click("Move to Trash"), Some(&el("AXRow", "Move to Trash")), &s, None, "Mail"));
        assert!(ask(&click("Empty Trash"), Some(&el("AXRow", "Empty Trash")), &s, None, "Finder"));
        // Checkout is a payment step on the web, a branch switch in a version-control client.
        assert!(ask(&click("Checkout"), Some(&el("AXButton", "Checkout")), &s, None, "Safari"));
        assert!(!ask(&click("Checkout"), Some(&el("AXButton", "Checkout")), &s, None, "GitHub Desktop"));
        assert!(!ask(&click("Check out"), Some(&el("AXMenuItem", "Check out")), &s, None, "Fork"));
    }
}
