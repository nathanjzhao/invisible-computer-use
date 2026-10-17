//! Per-value truncation descriptor (spec 19, `AXTruncationDescriptor { availableRange, count }`).
//!
//! The stock cua tree caps the NUMBER of elements (`max_elements`, `max_depth`) but sends every
//! element's value whole, so one long text area can bury the rest of the window. Codex clips a long
//! value and says how much of it is shown: the range it kept and the full length, "not just a
//! boolean". This module does the same on the driver's `get_window_state` document before the
//! lineage and the Sky renderer see it, so it holds on macOS, Windows and Linux alike.
//!
//! The unit is Unicode scalar values (`str::chars`), the closest portable reading of Swift's
//! `Character` count: a clip never splits a code point. INFERRED (not recoverable from the
//! binary): the default budget [`DEFAULT_MAX_VALUE_CHARS`] and the marker wording.
use serde_json::{json, Value};

/// Default per-value budget in characters. `0` turns clipping off.
pub const DEFAULT_MAX_VALUE_CHARS: usize = 4096;

/// What was clipped for one element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Truncation {
    /// The driver's `element_index` in the snapshot that was clipped.
    pub element_index: i64,
    /// The characters kept, as a half-open range from the start of the value.
    pub start: usize,
    pub end: usize,
    /// The value's full length in characters.
    pub count: usize,
}

impl Truncation {
    /// The model-facing form: `{available_range: {start, end}, count}`.
    pub fn to_json(&self, stable_id: Option<u64>) -> Value {
        let mut v = json!({"available_range": {"start": self.start, "end": self.end}, "count": self.count});
        if let Some(id) = stable_id { v["element_index"] = json!(id); }
        v
    }
}

/// The marker appended to a clipped value so the rendered row says what it lost.
pub fn marker(t: &Truncation) -> String {
    format!("\u{2026} [value truncated: showing characters {}..{} of {}; ask get_window_state for max_value_chars 0 for all of it]", t.start, t.end, t.count)
}

/// Clip every element `value` longer than `max_chars` characters in a cua `get_window_state`
/// document, in place. Returns one [`Truncation`] per clipped element. `max_chars == 0` clips
/// nothing. Only the `value` field is touched: selection and document text are separate fields
/// that carry their own length.
pub fn clip_values(doc: &mut Value, max_chars: usize) -> Vec<Truncation> {
    let mut out = Vec::new();
    if max_chars == 0 { return out; }
    let Some(elements) = doc.get_mut("elements").and_then(Value::as_array_mut) else { return out };
    for (position, el) in elements.iter_mut().enumerate() {
        let Some(text) = el.get("value").and_then(Value::as_str) else { continue };
        // byte length bounds char count from above: skip the char walk for short values
        if text.len() <= max_chars { continue; }
        let text = text.to_owned();
        let count = text.chars().count();
        if count <= max_chars { continue; }
        let end = text.char_indices().nth(max_chars).map(|(i, _)| i).unwrap_or(text.len());
        let t = Truncation {
            element_index: el.get("element_index").and_then(Value::as_i64).unwrap_or(position as i64),
            start: 0, end: max_chars, count,
        };
        let clipped = format!("{}{}", &text[..end], marker(&t));
        el["value"] = json!(clipped);
        // a text area's title often repeats the whole text (the real macOS tree renders it ahead of the
        // value: found on a Mac, where clipping `value` alone left the 20 000 characters in the row)
        for key in ["title", "description", "help", "label"] {
            let Some(other) = el.get(key).and_then(Value::as_str).map(str::to_owned) else { continue };
            if other == text {
                el[key] = json!(clipped);
            } else if other.len() > max_chars && other.chars().count() > max_chars {
                let end = other.char_indices().nth(max_chars).map(|(i, _)| i).unwrap_or(other.len());
                let own = Truncation { element_index: t.element_index, start: 0, end: max_chars, count: other.chars().count() };
                el[key] = json!(format!("{}{}", &other[..end], marker(&own)));
            }
        }
        out.push(t);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(values: &[&str]) -> Value {
        json!({"elements": values.iter().enumerate().map(|(i, v)| json!({"element_index": i, "role": "AXTextArea", "value": v})).collect::<Vec<_>>()})
    }

    #[test]
    fn a_long_value_is_clipped_with_the_range_and_the_full_count() {
        let long = "a".repeat(50);
        let mut d = doc(&["short", &long]);
        let t = clip_values(&mut d, 10);
        assert_eq!(t, vec![Truncation { element_index: 1, start: 0, end: 10, count: 50 }]);
        assert_eq!(d["elements"][0]["value"], "short", "values inside the budget are untouched");
        let v = d["elements"][1]["value"].as_str().unwrap();
        assert!(v.starts_with("aaaaaaaaaa\u{2026} [value truncated: showing characters 0..10 of 50"), "{v}");
        assert!(!v.contains(&"a".repeat(11)), "nothing past the budget survives: {v}");
    }

    #[test]
    fn a_title_that_repeats_the_value_is_clipped_with_it() {
        let long = "b".repeat(50);
        let mut d = json!({"elements": [{"element_index": 0, "role": "AXTextArea", "title": long, "value": long, "description": "short"}]});
        clip_values(&mut d, 10);
        let row = d["elements"][0].to_string();
        assert!(!row.contains(&"b".repeat(11)), "no copy of the text past the budget survives: {row}");
        assert_eq!(d["elements"][0]["description"], "short");
        assert_eq!(d["elements"][0]["title"], d["elements"][0]["value"]);
    }

    #[test]
    fn the_budget_is_in_characters_and_never_splits_a_code_point() {
        // 6 scalar values, 18 bytes: a byte slice at 4 would cut a code point
        let mut d = doc(&["\u{4f60}\u{597d}\u{4e16}\u{754c}\u{ff01}\u{ff01}"]);
        let t = clip_values(&mut d, 4);
        assert_eq!(t[0].count, 6);
        assert!(d["elements"][0]["value"].as_str().unwrap().starts_with("\u{4f60}\u{597d}\u{4e16}\u{754c}\u{2026}"));
        // exactly at the budget is not clipped, one over is
        let mut d = doc(&["abcd", "abcde"]);
        let t = clip_values(&mut d, 4);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].element_index, 1);
    }

    #[test]
    fn zero_means_no_clipping() {
        let long = "x".repeat(100_000);
        let mut d = doc(&[&long]);
        assert!(clip_values(&mut d, 0).is_empty());
        assert_eq!(d["elements"][0]["value"].as_str().unwrap().len(), 100_000);
    }

    #[test]
    fn non_string_and_missing_values_are_left_alone() {
        let mut d = json!({"elements": [{"element_index": 0, "value": 42}, {"element_index": 1}, {"element_index": 2, "value": null}]});
        assert!(clip_values(&mut d, 1).is_empty());
        assert_eq!(d["elements"][0]["value"], 42);
        assert!(clip_values(&mut json!({}), 5).is_empty());
    }

    #[test]
    fn the_model_form_carries_range_count_and_the_stable_id() {
        let t = Truncation { element_index: 3, start: 0, end: 10, count: 50 };
        assert_eq!(t.to_json(Some(7)), json!({"available_range": {"start": 0, "end": 10}, "count": 50, "element_index": 7}));
        assert_eq!(t.to_json(None), json!({"available_range": {"start": 0, "end": 10}, "count": 50}));
    }
}
