//! Execution model (spec 05/06/07/12/17/20): the SYNC action path (deadline +
//! structured refusal before dispatch + optional verify/settle), the MODE
//! PROFILES that preset Codex's real knobs, and EVENT polling.
//!
//! RECREATES Codex's structure, does not invent it:
//! - Sync path: Sky's window2 IPC is synchronous JSON-RPC with a per-request
//!   `deadlineUnixMilliseconds` (native-pipe.js). Ours enforces a deadline as a
//!   measured budget (`deadline_exceeded` on the result). The Sky adapter also refuses an
//!   expired absolute deadline before dispatch and after preparation/settling. cua actions take no
//!   deadline parameter and we do not hard-cancel a synchronous driver call,
//!   which is the honest limit. `verify` runs the driver's `verify_state`
//!   predicates after the action; `settle` polls the AX lineage until the tree
//!   stops changing (`waitForUIToSettle`, spec 07/20, parameters not constants).
//! - Mode = Confirmations Policy mode (14) x delivery mode (05) x CU flags
//!   (cursor/PiP/recording/simulate-click, 06/08/17). Presets attended /
//!   headless / replay. Which of these Codex gates via SlimCore flags (spec 20)
//!   is dynamic; we express them as config presets over the same surface.
//! - Events: Codex is event-stream driven (Skysight consumes an event stream;
//!   the AX tree carries revisions + notifications). We poll and emit
//!   ax-changed / focus-changed / permission-changed composed with the axdiff
//!   revision engine; the driver has no push channel (documented limit).
use serde::{Deserialize, Serialize};

/// A Sky wire deadline is an absolute admission limit, separate from the advisory
/// duration on ordinary layer calls. Never convert an expired deadline into a new budget.
pub(crate) fn remaining_deadline_ms(deadline: Option<u64>) -> Result<Option<u64>, crate::ToolError> {
    let Some(deadline) = deadline else { return Ok(None) };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| crate::ToolError::new(crate::CODE_DEADLINE_EXCEEDED, "Cannot establish the request deadline: system clock is before the Unix epoch"))?
        .as_millis();
    if u128::from(deadline) <= now {
        return Err(crate::ToolError::new(crate::CODE_DEADLINE_EXCEEDED, "Request deadline elapsed before dispatch; no action was performed"));
    }
    Ok(Some((u128::from(deadline) - now) as u64))
}

/// A named preset over Codex's real behaviour surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode { Attended, Headless, Replay }

impl Mode {
    pub fn parse(s: &str) -> Mode {
        match s.trim().to_ascii_lowercase().as_str() {
            "headless" | "background" => Mode::Headless,
            "replay" | "verify" => Mode::Replay,
            _ => Mode::Attended,
        }
    }
    pub fn name(self) -> &'static str { match self { Mode::Attended => "attended", Mode::Headless => "headless", Mode::Replay => "replay" } }
    /// The default cua `delivery_mode` for input tools.
    pub fn delivery(self) -> &'static str { match self { Mode::Attended => "foreground", _ => "background" } }
    /// Headless is focus-free only: an explicit `foreground` request is refused.
    pub fn allows_foreground(self) -> bool { !matches!(self, Mode::Headless) }
    /// The agent cursor default (attended shows it; headless hides it).
    pub fn cursor_default(self) -> bool { matches!(self, Mode::Attended) }
    /// Replay turns recording on by default.
    pub fn recording_default(self) -> bool { matches!(self, Mode::Replay) }
}

/// The delivery mode chosen for one action, and whether it was refused.
pub fn resolve_delivery(mode: Mode, requested: Option<&str>) -> Result<&'static str, String> {
    match requested {
        None => Ok(mode.delivery()),
        Some("background") => Ok("background"),
        Some("foreground") if mode.allows_foreground() => Ok("foreground"),
        Some("foreground") => Err(format!("delivery_mode \"foreground\" is not allowed in {} mode (focus-free only)", mode.name())),
        Some(other) => Err(format!("delivery_mode must be background or foreground, got {other:?}")),
    }
}

/// One event from a poll (spec 19 event stream / AX revisions).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum Event {
    #[serde(rename = "ax-changed")]
    AxChanged { window: u64, revision: u64, snapshot_count: u64 },
    #[serde(rename = "focus-changed")]
    FocusChanged { app: String, window: Option<u64> },
    #[serde(rename = "permission-changed")]
    PermissionChanged { scope: String, status: String },
}

/// What a poll compares against: the last AX revision per window, the frontmost
/// window id, and the permission statuses.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventCursor {
    pub ax: std::collections::BTreeMap<u64, u64>,
    pub focus: Option<u64>,
    pub permissions: std::collections::BTreeMap<String, String>,
}

/// Diff two cursors into events (pure, so the loop is testable without a driver).
pub fn diff_cursors(prev: &EventCursor, now: &EventCursor, focus_app: &str, ax_counts: &std::collections::BTreeMap<u64, u64>) -> Vec<Event> {
    let mut events = Vec::new();
    for (win, rev) in &now.ax {
        if prev.ax.get(win) != Some(rev) {
            events.push(Event::AxChanged { window: *win, revision: *rev, snapshot_count: ax_counts.get(win).copied().unwrap_or(0) });
        }
    }
    if prev.focus != now.focus {
        events.push(Event::FocusChanged { app: focus_app.to_string(), window: now.focus });
    }
    for (scope, status) in &now.permissions {
        if prev.permissions.get(scope).map(String::as_str) != Some(status.as_str()) {
            events.push(Event::PermissionChanged { scope: scope.clone(), status: status.clone() });
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_presets_match_codex_knobs() {
        assert_eq!((Mode::Attended.delivery(), Mode::Attended.allows_foreground(), Mode::Attended.cursor_default()), ("foreground", true, true));
        assert_eq!((Mode::Headless.delivery(), Mode::Headless.allows_foreground(), Mode::Headless.cursor_default()), ("background", false, false));
        assert!(Mode::Replay.recording_default());
        assert_eq!(Mode::parse("background"), Mode::Headless);
    }

    #[test]
    fn delivery_resolution_refuses_foreground_only_in_headless() {
        assert_eq!(resolve_delivery(Mode::Attended, None).unwrap(), "foreground");
        assert_eq!(resolve_delivery(Mode::Headless, None).unwrap(), "background");
        assert_eq!(resolve_delivery(Mode::Headless, Some("background")).unwrap(), "background");
        assert!(resolve_delivery(Mode::Headless, Some("foreground")).is_err());
        assert_eq!(resolve_delivery(Mode::Attended, Some("foreground")).unwrap(), "foreground");
        assert!(resolve_delivery(Mode::Attended, Some("sideways")).is_err());
    }

    #[test]
    fn events_are_emitted_only_on_change() {
        let counts = [(1u64, 5u64)].into_iter().collect();
        let prev = EventCursor { ax: [(1, 3)].into_iter().collect(), focus: Some(1), permissions: [("accessibility".into(), "granted".into())].into_iter().collect() };
        let now = EventCursor { ax: [(1, 4)].into_iter().collect(), focus: Some(2), permissions: [("accessibility".into(), "denied".into())].into_iter().collect() };
        let ev = diff_cursors(&prev, &now, "Safari", &counts);
        assert!(ev.contains(&Event::AxChanged { window: 1, revision: 4, snapshot_count: 5 }));
        assert!(ev.contains(&Event::FocusChanged { app: "Safari".into(), window: Some(2) }));
        assert!(ev.contains(&Event::PermissionChanged { scope: "accessibility".into(), status: "denied".into() }));
        // no change -> no events
        assert!(diff_cursors(&now, &now, "Safari", &counts).is_empty());
    }
}
