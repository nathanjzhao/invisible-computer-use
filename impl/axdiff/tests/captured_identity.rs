// Cross-turn identity over REAL captured Linux and Windows driver output.
//
// Linux: raw `get_window_state` responses from the patched driver on a Namespace
// X11 host (GTK3 fixture, AT-SPI), one per mutation turn. Provenance and the
// probe are in study/audit/linux-identity-2026-09-25. The "Keep" button carries
// a toolkit AT-SPI accessible id (`btn-keep`); every other control has none.
//
// Windows: the two observations of the installed app's calculator run
// (study/audit/windows-app-2026-09-23, windows_compute.zip), reconstructed from
// the driver's captured UIA tree markdown, whose `id=` field is the AutomationId.
//
// Contract: a platform identifier keeps a relabel as one change; without one,
// neither position, snapshot index nor element_token proves identity, and an
// ambiguous replacement is reported as removal plus addition.
use axdiff::*;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

fn load(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/captured/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).expect(&path)
}

const LINUX: [&str; 7] = [
    "linux-gtk3-turn-01-initial",
    "linux-gtk3-turn-02-relabel-with-accessible-id",
    "linux-gtk3-turn-03-relabel-without-id",
    "linux-gtk3-turn-04-reorder",
    "linux-gtk3-turn-05-replace",
    "linux-gtk3-turn-06-dup-siblings",
    "linux-gtk3-turn-07-dup-remove-first",
];

/// Advance a layer lineage through the captured turns: label -> ids per turn.
fn lineage(turns: &[Value]) -> Vec<(Revision, HashMap<i64, u64>)> {
    let mut out: Vec<(Revision, HashMap<i64, u64>)> = Vec::new();
    for doc in turns {
        let forest = cua::forest_from_gws(doc);
        let rev = match out.last() {
            None => Revision::initial(&forest),
            Some((prev, _)) => prev.next(&forest),
        };
        let map = rev.index_map();
        out.push((rev, map));
    }
    out
}

fn ids_by_label(doc: &Value, index_map: &HashMap<i64, u64>) -> Vec<(String, u64)> {
    doc["elements"].as_array().unwrap().iter().map(|e| {
        let idx = e["element_index"].as_i64().unwrap();
        (e["label"].as_str().unwrap_or("").to_string(), index_map[&idx])
    }).collect()
}

fn id_of(doc: &Value, index_map: &HashMap<i64, u64>, label: &str) -> Vec<u64> {
    ids_by_label(doc, index_map).into_iter().filter(|(l, _)| l == label).map(|(_, id)| id).collect()
}

fn native_id(doc: &Value, label: &str) -> Vec<u64> {
    doc["elements"].as_array().unwrap().iter()
        .filter(|e| e["label"] == label).map(|e| e["stable_id"].as_u64().unwrap()).collect()
}

#[test]
fn captured_linux_accessible_id_keeps_a_relabel_in_native_and_layer_lineages() {
    let turns: Vec<Value> = LINUX.iter().map(|n| load(n)).collect();
    // The driver exported the toolkit id only where the toolkit set one.
    let ids: Vec<&str> = turns[0]["elements"].as_array().unwrap().iter()
        .filter_map(|e| e["identifier"].as_str()).collect();
    assert_eq!(ids, ["btn-keep"]);
    // Native lineage (captured stable_id): Keep -> Kept is one element.
    assert_eq!(native_id(&turns[0], "Keep"), native_id(&turns[1], "Kept"));
    let text = turns[1]["diff"]["text"].as_str().unwrap();
    assert!(text.contains("~ [5] push button \"Kept\""), "{text}");
    // Layer lineage over the same raw responses agrees.
    let l = lineage(&turns);
    assert_eq!(id_of(&turns[0], &l[0].1, "Keep"), id_of(&turns[1], &l[1].1, "Kept"));
    let s = render_diff(&l[1].0, true, &DiffOptions::default()).text().to_string();
    assert!(s.contains("~ [") && s.contains("\"Kept\"") && !s.contains("+ [") , "{s}");
}

#[test]
fn captured_linux_without_identity_never_reuses_an_id_by_position() {
    let turns: Vec<Value> = LINUX.iter().map(|n| load(n)).collect();
    let l = lineage(&turns);
    for (turn, (_, map)) in turns.iter().zip(&l) {
        // index_map covers exactly the indexed rows, and ids are distinct.
        let ids: Vec<u64> = ids_by_label(turn, map).into_iter().map(|(_, id)| id).collect();
        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), ids.len());
    }
    // Alpha -> Play at the same position, no platform id: a new element in
    // both the native driver lineage and the layer lineage.
    let (a_native, p_native) = (native_id(&turns[1], "Alpha"), native_id(&turns[2], "Play"));
    assert_ne!(a_native, p_native);
    assert_ne!(id_of(&turns[1], &l[1].1, "Alpha"), id_of(&turns[2], &l[2].1, "Play"));
    // Reorder of uniquely labelled siblings keeps every button id.
    for label in ["Charlie", "Play", "Bravo", "Kept"] {
        assert_eq!(native_id(&turns[2], label), native_id(&turns[3], label), "{label}");
        assert_eq!(id_of(&turns[2], &l[2].1, label), id_of(&turns[3], &l[3].1, label), "{label}");
    }
    // A/B/C -> B/C/D: Delta is new and does not inherit the removed Play id.
    let play = native_id(&turns[3], "Play")[0];
    let delta = native_id(&turns[4], "Delta")[0];
    assert_ne!(play, delta);
    assert!(!ids_by_label(&turns[4], &l[4].1).iter().any(|(_, id)| *id == id_of(&turns[3], &l[3].1, "Play")[0]));
    // Two identical "Delete" siblings, first removed: no survivor may claim
    // either old id (native and layer), the removal is reported honestly.
    let before: HashSet<u64> = native_id(&turns[5], "Delete").into_iter().collect();
    let after = native_id(&turns[6], "Delete");
    assert_eq!((before.len(), after.len()), (2, 1));
    assert!(!before.contains(&after[0]));
    let lb: HashSet<u64> = id_of(&turns[5], &l[5].1, "Delete").into_iter().collect();
    assert!(!lb.contains(&id_of(&turns[6], &l[6].1, "Delete")[0]));
    let text = turns[6]["diff"]["text"].as_str().unwrap();
    for id in &before { assert!(text.contains(&format!("- [{id}]")), "{text}"); }
}

#[test]
fn snapshot_tokens_and_indices_are_not_identity() {
    // Rewrite every turn so tokens and indices are identical across turns: the
    // adapter must produce exactly the same lineage, i.e. it never reads them
    // as identity. Then drop the platform id: the relabel becomes add+remove.
    let turns: Vec<Value> = LINUX.iter().map(|n| load(n)).collect();
    let normalized: Vec<Value> = turns.iter().map(|t| {
        let mut t = t.clone();
        for (i, e) in t["elements"].as_array_mut().unwrap().iter_mut().enumerate() {
            e["element_token"] = Value::String(format!("s00000001:{i}"));
        }
        t
    }).collect();
    let a = lineage(&turns);
    let b = lineage(&normalized);
    for i in 0..turns.len() {
        assert_eq!(ids_by_label(&turns[i], &a[i].1), ids_by_label(&normalized[i], &b[i].1));
    }
    let stripped: Vec<Value> = turns[..2].iter().map(|t| {
        let mut t = t.clone();
        for e in t["elements"].as_array_mut().unwrap() { e.as_object_mut().unwrap().remove("identifier"); }
        t
    }).collect();
    let c = lineage(&stripped);
    assert_ne!(id_of(&stripped[0], &c[0].1, "Keep"), id_of(&stripped[1], &c[1].1, "Kept"));
}

#[test]
fn captured_windows_automation_ids_hold_across_the_real_observation_pair() {
    let (t1, t2) = (load("windows-calculator-1"), load("windows-calculator-2"));
    let l = lineage(&[t1.clone(), t2.clone()]);
    // All ten indexed controls keep their ids. The passive readout carries its
    // text as its name and has no AutomationId, so 0 -> 42 is honestly a
    // removed and an added read-only row, never an input target.
    assert_eq!(ids_by_label(&t1, &l[0].1), ids_by_label(&t2, &l[1].1));
    let s = render_diff(&l[1].0, true, &DiffOptions::default()).text().to_string();
    let changed: Vec<&str> = s.lines().filter(|l| l.starts_with(['+', '-', '~'])).collect();
    assert_eq!(changed.len(), 2, "{s}");
    assert!(changed[0].starts_with("+ [") && changed[0].ends_with("Text \"42\""), "{s}");
    assert!(changed[1].starts_with("- ["), "{s}");
    assert!(t2["passive_elements"].as_array().unwrap().iter().all(|p| p.get("element_index").is_none()));
    let targets: HashSet<u64> = l[1].1.values().copied().collect();
    let added: u64 = changed[0][3..].split(']').next().unwrap().parse().unwrap();
    assert!(!targets.contains(&added), "a passive row must not become an input target");
}

#[test]
fn windows_automation_id_relabel_and_duplicate_ids_on_the_captured_shape() {
    // Derived from the captured tree (labelled inference, not a live capture):
    // relabel "6" -> "six" on AutomationId button0, and a second control that
    // reuses button1's AutomationId.
    let t1 = load("windows-calculator-1");
    let mut relabel = t1.clone();
    relabel["elements"][0]["label"] = Value::String("six".into());
    let l = lineage(&[t1.clone(), relabel.clone()]);
    assert_eq!(id_of(&t1, &l[0].1, "6"), id_of(&relabel, &l[1].1, "six"));
    // Without the AutomationId the same relabel is a replacement.
    let mut bare = t1.clone();
    for e in bare["elements"].as_array_mut().unwrap() { e.as_object_mut().unwrap().remove("identifier"); }
    let mut bare_relabel = bare.clone();
    bare_relabel["elements"][0]["label"] = Value::String("six".into());
    let lb = lineage(&[bare.clone(), bare_relabel.clone()]);
    assert_ne!(id_of(&bare, &lb[0].1, "6"), id_of(&bare_relabel, &lb[1].1, "six"));
    // Duplicate AutomationIds cannot establish a relabel.
    let mut dup = t1.clone();
    dup["elements"][0]["identifier"] = Value::String("button1".into());
    let mut dup_relabel = dup.clone();
    dup_relabel["elements"][0]["label"] = Value::String("six".into());
    let ld = lineage(&[dup.clone(), dup_relabel.clone()]);
    assert_ne!(id_of(&dup, &ld[0].1, "6"), id_of(&dup_relabel, &ld[1].1, "six"));
}
