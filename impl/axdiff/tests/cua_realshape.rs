// Integration test over cua-driver's REAL get_window_state schema shape:
// a flat `elements` array (depth-first) with element_index / parent_index /
// depth / role / label / value, including the observed "flattened container"
// pattern where rows carry parent_index=0 (the window) but depth=2. Verified
// against live Activity Monitor output on 2026-09-10.
use axdiff::*;
use serde_json::json;

fn gws(elements: serde_json::Value) -> serde_json::Value {
    json!({ "elements": elements })
}

#[test]
fn adapter_reconstructs_flattened_rows_and_diffs() {
    // turn 1: window(0) with two rows (parent_index=0, depth=2) each holding a cell.
    let t1 = gws(json!([
        {"element_index":0,"role":"AXWindow","label":"Activity Monitor","depth":0},
        {"element_index":1,"parent_index":0,"role":"AXRow","label":"procA","depth":2},
        {"element_index":2,"parent_index":1,"role":"AXCell","value":"10%","depth":3},
        {"element_index":3,"parent_index":0,"role":"AXRow","label":"procB","depth":2},
        {"element_index":4,"parent_index":3,"role":"AXCell","value":"20%","depth":3}
    ]));
    // turn 2: procA cell value changes; procB row is removed and NOTHING of the
    // same role is added — an unambiguous delete, so it must show as `-`.
    let t2 = gws(json!([
        {"element_index":0,"role":"AXWindow","label":"Activity Monitor","depth":0},
        {"element_index":1,"parent_index":0,"role":"AXRow","label":"procA","depth":2},
        {"element_index":2,"parent_index":1,"role":"AXCell","value":"55%","depth":3}
    ]));
    // turn 3: a genuinely new row appears with nothing removed — an unambiguous
    // add, so it must show as `+`.
    let t3 = gws(json!([
        {"element_index":0,"role":"AXWindow","label":"Activity Monitor","depth":0},
        {"element_index":1,"parent_index":0,"role":"AXRow","label":"procA","depth":2},
        {"element_index":2,"parent_index":1,"role":"AXCell","value":"55%","depth":3},
        {"element_index":9,"parent_index":0,"role":"AXRow","label":"procC","depth":2},
        {"element_index":10,"parent_index":9,"role":"AXCell","value":"5%","depth":3}
    ]));

    let f1 = cua::forest_from_gws(&t1);
    // window is the single root; both rows are its direct children (parent_index authority)
    assert_eq!(f1.len(), 1, "one root (window)");
    assert_eq!(f1[0].role, "AXWindow");
    assert_eq!(f1[0].children.len(), 2, "two rows under the window");
    assert_eq!(f1[0].children[0].children.len(), 1, "row holds one cell");

    let r1 = Revision::initial(&f1);
    let r2 = r1.next(&cua::forest_from_gws(&t2));
    let s = render_diff(&r2, true, &DiffOptions { app_name: "Activity Monitor".into(), ..Default::default() }).text().to_string();
    assert!(s.starts_with(HDR_INCREMENTAL), "expected incremental header:\n{s}");
    // procA's cell value 10% -> 55% is a change on the SAME element id (stable identity)
    assert!(s.contains("~") && s.contains("55%"), "value change not diffed:\n{s}");
    // procB row + its cell were removed -> incremental dash lines (delete robustness)
    assert!(s.contains("- ["), "removed rows missing:\n{s}");

    let r3 = r2.next(&cua::forest_from_gws(&t3));
    let s3 = render_diff(&r3, true, &DiffOptions { app_name: "Activity Monitor".into(), ..Default::default() }).text().to_string();
    // procC row + its cell are additions (add robustness)
    assert!(s3.contains("+") && s3.contains("procC"), "added row missing:\n{s3}");
}

#[test]
fn label_change_with_platform_identity_inherits_the_id_as_a_change() {
    // A unique AXIdentifier supports this relabel; snapshot tokens differ. A control whose LABEL
    // mutates in place — the canonical Play<->Pause button — must diff as `~`
    // with the SAME stable id, not remove+add. Before FIX 3 the (role,label) key
    // churned it into +/-.
    let t1 = gws(json!([
        {"element_index":0,"role":"AXWindow","label":"Player","depth":0},
        {"element_index":1,"parent_index":0,"role":"AXButton","label":"Play","identifier":"play-control","element_token":"s1:1","depth":1}
    ]));
    let t2 = gws(json!([
        {"element_index":0,"role":"AXWindow","label":"Player","depth":0},
        {"element_index":1,"parent_index":0,"role":"AXButton","label":"Pause","identifier":"play-control","element_token":"s2:1","depth":1}
    ]));
    let r1 = Revision::initial(&cua::forest_from_gws(&t1));
    // capture the button's stable id from turn 1's full render
    let full = render_diff(&r1, false, &DiffOptions::default()).text().to_string();
    let play_id: u64 = full.lines()
        .find(|l| l.contains("AXButton") && l.contains("Play"))
        .and_then(|l| l.split('[').nth(1)).and_then(|r| r.split(']').next())
        .and_then(|n| n.trim().parse().ok())
        .expect("Play button id in full render");

    let r2 = r1.next(&cua::forest_from_gws(&t2));
    let s = render_diff(&r2, true, &DiffOptions::default()).text().to_string();
    // it is a change, and specifically the same id, and it is NOT remove+add
    assert!(s.contains(&format!("~ [{play_id}] AXButton")) && s.contains("Pause"),
        "Play->Pause must be `~ [{play_id}] AXButton ... Pause`, got:\n{s}");
    assert!(!s.contains(&format!("- [{play_id}]")), "the button must not be removed:\n{s}");
    assert!(!s.contains("+ ["), "the button must not be re-added:\n{s}");
}

#[test]
fn adapter_handles_empty_degraded_tree() {
    // cua returns elements:[] when the window is ax_window_unresolved; adapter -> empty forest.
    let empty = gws(json!([]));
    assert!(cua::forest_from_gws(&empty).is_empty());
    let r = Revision::initial(&cua::forest_from_gws(&empty));
    // initial of an empty tree renders empty full, no panic.
    assert_eq!(render_diff(&r, false, &DiffOptions::default()).text(), "");
}

#[test]
fn passive_readout_changes_are_visible_without_acquiring_input_targets() {
    let snapshot = |value| json!({
        "elements": [
            {"element_index":0,"role":"Window","label":"Calculator"},
            {"element_index":1,"parent_index":0,"role":"Button","label":"="}
        ],
        "passive_elements": [{"parent_index":0,"role":"Text","label":"Result","value":value}]
    });
    let first = Revision::initial(&cua::forest_from_gws(&snapshot("0")));
    let next = first.next(&cua::forest_from_gws(&snapshot("42")));
    let text = render_diff(&next, true, &DiffOptions::default()).text().to_owned();
    assert!(text.contains("42"), "passive result was omitted: {text}");
    assert_eq!(next.index_map().len(), 2, "read-only text became an input target");
    assert_eq!(next.index_map()[&1], first.index_map()[&1]);
}

#[test]
fn passive_entries_cannot_smuggle_native_indices_or_inherit_unrelated_children() {
    let snapshot = json!({"structuredContent": {
        "elements":[
            {"element_index":0,"role":"Window","label":"W"},
            {"element_index":1,"parent_index":0,"role":"Button","label":"Run"}
        ],
        "passive_elements":[{"role":"Text","label":"Read only","element_index":99,"element_token":"forged"}]
    }});
    let forest = cua::forest_from_gws(&snapshot);
    assert_eq!(forest.len(), 2);
    assert_eq!(forest[1].label, "Read only");
    assert_eq!(forest[1].source, None);
    assert!(forest[1].children.is_empty());
    assert_eq!(forest[0].children.len(), 1);
    assert_eq!(Revision::initial(&forest).index_map().len(), 2);
}

#[test]
fn captured_linux_readout_diff_keeps_only_the_real_buttons_addressable() {
    let before: serde_json::Value = serde_json::from_str(include_str!("fixtures/linux-passive-initial.json")).unwrap();
    let after: serde_json::Value = serde_json::from_str(include_str!("fixtures/linux-passive-final.json")).unwrap();
    let first = Revision::initial(&cua::forest_from_gws(&before));
    let forest = cua::forest_from_gws(&after);
    let next = first.next(&forest);
    assert_eq!(next.index_map(), first.index_map(), "passive result churn changed button identities");
    assert_eq!(next.index_map().len(), 5);
    let result = forest.iter().find(|node| node.role == "label" && node.label == "42").unwrap();
    assert!(result.source.is_none(), "passive readout acquired an input handle");
    let diff = render_diff(&next, true, &DiffOptions::default());
    assert!(matches!(diff, DiffOutput::Diff(_)));
    assert!(diff.text().contains("42"));
}
