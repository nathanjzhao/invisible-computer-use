//! The facade on a REAL cua-driver document (conformance/fixtures/clock_gws.json,
//! Clock.app captured 2026-09-10, screenshot shrunk to a 1x1 PNG).
use axdiff::layer::{observe, LayerConfig};
use serde_json::Value;
use std::path::PathBuf;
use window2::*;

fn fixture() -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../conformance/fixtures/clock_gws.json");
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}
fn tmp() -> PathBuf {
    std::env::temp_dir().join(format!("window2-fixture-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()))
}

#[test]
fn real_clock_document_maps_to_the_golden_window_state() {
    let doc = fixture();
    let cfg = LayerConfig { state_dir: tmp(), ..LayerConfig::default() };
    let o = observe(&doc, "55372", "25198", Some("com.apple.clock".into()), &cfg);
    assert_eq!(o.tag, "FULL(first)");
    let tree = TreeInput::from(&o);
    let st = window_state_from_cua(&doc, "com.apple.clock", StateRequest { include_screenshot: true, include_text: true }, Some(&tree)).unwrap();

    assert_eq!(st.window, Window { app: "com.apple.clock".into(), id: 25198, title: Some("Clock".into()) });
    let sc = &st.screenshots[0];
    assert_eq!((sc.id.as_str(), sc.z_index), ("screenshot-0", 0));
    assert!(sc.url.starts_with("data:image/png;base64,iVBORw0KGgo"));
    assert_eq!((sc.origin_x, sc.origin_y, sc.width, sc.height), (Some(352.0), Some(98.0), Some(1024.0), Some(768.0)));

    let a = st.accessibility.as_ref().unwrap();
    // the pack rides in front of the tree on first touch
    assert!(a.tree.starts_with("<app_specific_instructions>\n## Clock Computer Use"));
    // the Sky header, then every element in the document as a tab-indented row
    let body = a.tree.split_once("</app_specific_instructions>\n").unwrap().1;
    assert!(body.starts_with("Window: \"Clock\", App: Clock.\n"), "{}", &body[..80.min(body.len())]);
    let rows: Vec<&str> = body.lines().skip(1).map(|l| l.trim_start_matches('\t')).collect();
    let is_row = |l: &str| l.split_once(' ').map_or(!l.is_empty() && l.bytes().all(|b| b.is_ascii_digit()), |(n, _)| n.parse::<u64>().is_ok());
    // every element is a row, except the closed menus under menu bar items (golden rule M2)
    let els = doc["elements"].as_array().unwrap();
    let parent = |e: &Value| e["parent_index"].as_i64().and_then(|p| els.iter().find(|x| x["element_index"].as_i64() == Some(p)));
    let hidden = |e: &Value| { let mut p = parent(e); while let Some(x) = p { if x["role"] == "AXMenuBarItem" { return true; } p = parent(x); } false };
    assert_eq!(rows.iter().filter(|l| is_row(l)).count(), els.iter().filter(|e| !hidden(e)).count());
    assert_eq!(rows.first().copied().map(|l| l.split(' ').next().unwrap()), Some("0"), "ids start at 0");
    // selected_elements are the presented tree's own rows, one per cua `selected: true`
    let n_selected = doc["elements"].as_array().unwrap().iter().filter(|e| e["selected"] == true).count();
    let sel = a.selected_elements.as_ref().unwrap();
    assert_eq!(sel.len(), n_selected);
    for s in sel { assert!(rows.contains(&s.as_str()), "{s} not in tree"); }
    assert_eq!((st.accessibility_revision, st.accessibility_snapshot_count), (Some(1), Some(1)));
    assert_eq!(st.gaps.len(), 4);

    // serialized keys are exactly the golden's (plus the underscore diagnostic and counters)
    let v = serde_json::to_value(&st).unwrap();
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, vec!["_gaps", "accessibility", "accessibilityRevision", "accessibilitySnapshotCount", "screenshots", "window"]);
}
