//! Conservative screenshot-retention heuristic over accessibility elements and diff kind.
//! An unchanged accessibility tree cannot establish that visual content is unchanged.
//! Layer also requires prior visual grounding and requested text before omitting an image.
//! This selects returned content; automatic mode still captures before classification.
//! Driver observations require explicit completeness evidence. Missing evidence is unknown,
//! not a legacy opt-out: current native bounded projections retain their screenshots.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    pub screenshot_needed: bool,
    pub reason: String,
}

/// Classify an exhaustive element list. Use `classify_observation` for driver documents.
/// Visual surfaces and unlabeled actions need pixels even when AX is unchanged. Only known
/// small/unchanged text renders may omit them. The caller must separately establish that an
/// image was previously returned to this session for this window and that text is requested.
pub fn classify(elements: &[serde_json::Value], diff_kind: &str) -> Verdict {
    let actionable: Vec<&serde_json::Value> = elements.iter()
        .filter(|e| e.get("actions").and_then(|a| a.as_array()).map(|a| !a.is_empty()).unwrap_or(false))
        .collect();
    let labeled = |e: &serde_json::Value| e.get("label").and_then(|l| l.as_str()).map(|s| !s.trim().is_empty()).unwrap_or(false);
    let n = actionable.len();
    let unlabeled = actionable.iter().filter(|e| !labeled(e)).count();
    let has_image = elements.iter().any(|e| {
        let role = e.get("role").and_then(|r| r.as_str()).unwrap_or("").to_ascii_lowercase();
        role.contains("image") || role.contains("canvas") || role.contains("icon")
            || (role.contains("webarea") || role.contains("web area")) && !labeled(e)
    });
    let v = |needed: bool, reason: &str| Verdict { screenshot_needed: needed, reason: reason.to_string() };
    if n == 0 { return v(true, "no actionable text elements; grounding needs the pixels"); }
    if has_image { return v(true, "a visual surface may change without an accessibility change; retain the pixels"); }
    if unlabeled > 0 { return v(true, format!("{unlabeled} of {n} actionable elements are unlabeled; text alone is ambiguous").as_str()); }
    match diff_kind.to_ascii_lowercase().as_str() {
        "no_change" => v(false, "the labeled accessibility tree is unchanged and no visual-risk surface was identified"),
        "diff" => v(false, "the actionable elements are labeled and the change is small; the text is sufficient"),
        "full_first" | "full(first)" | "full" | "full_fallback" => v(true, "a full accessibility render requires visual grounding"),
        _ => v(true, "unrecognized accessibility diff kind; retain visual grounding"),
    }
}

/// An absent image/canvas in a partial projection is not evidence that the window has none.
/// Only explicit `elements_complete: true` permits omission, with no contradictory counts
/// or truncation diagnostics. Older drivers without completeness metadata retain images.
pub fn classify_observation(doc: &serde_json::Value, diff_kind: &str) -> Verdict {
    let elements = doc.get("elements").and_then(|e| e.as_array()).map(Vec::as_slice).unwrap_or(&[]);
    let verdict = classify(elements, diff_kind);
    if verdict.screenshot_needed { return verdict; }
    let reason = if doc.get("elements_complete").and_then(|v| v.as_bool()) != Some(true) {
        Some("accessibility coverage is incomplete or unknown; omitted elements may carry visual state")
    } else if ["returned_element_count", "total_element_count"].iter().any(|key| {
        doc.get(key).is_some_and(|count| count.as_u64() != Some(elements.len() as u64))
    }) {
        Some("accessibility element counts contradict complete coverage; retain the pixels")
    } else if doc.get("truncated").and_then(|v| v.as_bool()) == Some(true)
        || doc.pointer("/ax_diagnostics/truncated").and_then(|v| v.as_bool()) == Some(true)
        || doc.get("tree_markdown").and_then(|v| v.as_str()).is_some_and(|tree| {
            // The macOS walker emits this diagnostic when its element limit is reached.
            tree.lines().any(|line| line.trim_start().starts_with("⚠️  AX tree truncated at "))
        }) {
        Some("accessibility truncation is reported; omitted elements may carry visual state")
    } else { None };
    match reason {
        Some(reason) => Verdict { screenshot_needed: true, reason: reason.into() },
        None => verdict,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn el(role: &str, label: &str, actionable: bool) -> serde_json::Value {
        json!({"role": role, "label": label, "actions": if actionable { vec!["AXPress"] } else { Vec::<&str>::new() }})
    }

    #[test]
    fn heuristic_flags_the_unsafe_cases_only() {
        // unchanged labeled UI with no identified visual risk can use text
        assert!(!classify(&[el("AXButton", "OK", true)], "no_change").screenshot_needed);
        // labeled small diff: no
        assert!(!classify(&[el("AXButton", "OK", true), el("AXButton", "Cancel", true)], "diff").screenshot_needed);
        // no actionable text: yes
        assert!(classify(&[el("AXStaticText", "label only", false)], "diff").screenshot_needed);
        // mostly unlabeled actionable: yes
        let mostly_blank = vec![el("AXButton", "", true), el("AXButton", "", true), el("AXButton", "Go", true)];
        assert!(classify(&mostly_blank, "diff").screenshot_needed);
        // image surface: yes
        assert!(classify(&[el("AXButton", "OK", true), el("AXImage", "", false)], "diff").screenshot_needed);
        // first full render of a labeled window: yes (establish grounding once)
        assert!(classify(&[el("AXButton", "OK", true), el("AXButton", "Cancel", true)], "full_first").screenshot_needed);
        // and the reason is always populated
        assert!(!classify(&[el("AXButton", "OK", true)], "diff").reason.is_empty());
    }

    #[test]
    fn unchanged_ax_does_not_suppress_visual_risk() {
        for role in ["AXImage", "image", "canvas", "icon", "AXWebArea", "web area"] {
            assert!(classify(&[el("AXButton", "OK", true), el(role, "", false)], "NO_CHANGE").screenshot_needed, "{role}");
        }
        assert!(classify(&[el("AXButton", "OK", true), el("AXButton", "", true)], "no_change").screenshot_needed);
        assert!(classify(&[], "no_change").screenshot_needed);
        assert!(classify(&[el("AXStaticText", "Status", false)], "no_change").screenshot_needed);
    }

    #[test]
    fn actual_full_tags_and_unknown_diff_kinds_keep_images() {
        for tag in ["FULL(first)", "FULL", "FULL_FALLBACK", "full_first", "unknown"] {
            assert!(classify(&[el("AXButton", "OK", true)], tag).screenshot_needed, "{tag}");
        }
    }
}
