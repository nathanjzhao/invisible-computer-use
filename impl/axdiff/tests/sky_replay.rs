//! Replays every captured Sky scenario in `conformance/golden/sky-replay/*.json`
//! through the layer and compares the text with what the real SkyComputerUseService
//! returned, byte for byte. A new capture drops in as another JSON file there.
use axdiff::layer::{observe, LayerConfig};
use serde_json::Value;
use std::path::{Path, PathBuf};

fn root() -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("../..") }

/// The text the model saw for one step, read from the capture itself.
fn expected(capture: &Path, expect: &Value) -> String {
    if let Some(f) = expect.get("text_file").and_then(Value::as_str) {
        return std::fs::read_to_string(capture.join(f)).unwrap();
    }
    let key = expect["results_key"].as_str().expect("expect.results_key or expect.text_file");
    let results: Value = serde_json::from_slice(&std::fs::read(capture.join("results.json")).unwrap()).unwrap();
    let text = results[key]["content"][0]["text"].as_str().unwrap_or_else(|| panic!("{key} missing from results.json")).to_string();
    match expect.get("json_field").and_then(Value::as_str) {
        Some(field) => serde_json::from_str::<Value>(&text).unwrap()[field].as_str().unwrap().to_string(),
        None => text,
    }
}

/// The service's own skyshot text on a 1-based line of `sky-ipc.jsonl`.
fn ipc_skyshot(capture: &Path, line: usize) -> String {
    let raw = std::fs::read_to_string(capture.join("sky-ipc.jsonl")).unwrap();
    let frame: Value = serde_json::from_str(raw.lines().nth(line - 1).unwrap()).unwrap();
    let msg: Value = serde_json::from_str(frame["raw"].as_str().unwrap()).unwrap();
    msg["result"]["skyshot"]["text"].as_str().unwrap_or_else(|| panic!("line {line} is not a skyshot")).to_string()
}

fn first_difference(a: &str, b: &str) -> String {
    let at = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
    let ctx = |s: &str| s.chars().skip(at.saturating_sub(40)).take(100).collect::<String>();
    format!("first difference at char {at}\n  ours: {:?}\n  sky:  {:?}", ctx(a), ctx(b))
}

#[test]
fn every_captured_sky_scenario_replays_byte_for_byte() {
    let dir = root().join("conformance/golden/sky-replay");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    assert!(!files.is_empty(), "no replay fixtures in {}", dir.display());
    let (mut compared, mut gaps) = (0, 0);
    for file in files {
        let fx: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        let capture = root().join(fx["capture"].as_str().unwrap());
        for session in fx["sessions"].as_array().unwrap() {
            let state = std::env::temp_dir().join(format!("sky-replay-{}-{}-{}", std::process::id(),
                file.file_stem().unwrap().to_string_lossy(), session["name"].as_str().unwrap()));
            let _ = std::fs::remove_dir_all(&state);
            for turn in session["turns"].as_array().unwrap() {
                let step = turn["step"].as_str().unwrap();
                let mut cfg = LayerConfig { state_dir: state.clone(), ..LayerConfig::default() };
                cfg.opts.disable_diff = turn.get("disable_diff").and_then(Value::as_bool).unwrap_or(false);
                let o = observe(&turn["doc"], session["pid"].as_str().unwrap(), session["window"].as_str().unwrap(),
                                session["bundle_id"].as_str().map(str::to_string), &cfg);
                let want = expected(&capture, &turn["expect"]);
                if let Some(line) = turn.get("ipc_line").and_then(Value::as_u64) {
                    assert!(want.ends_with(&ipc_skyshot(&capture, line as usize)), "{step}: expect and ipc_line disagree");
                }
                if let Some(gap) = turn.get("known_gap") {
                    // our driver's data cannot reproduce Sky here: pin our exact text, and
                    // fail if it drifts or starts matching Sky (then drop the gap)
                    let pinned = root().join(gap["ours"].as_str().unwrap());
                    if std::env::var_os("SKY_REPLAY_BLESS").is_some() { std::fs::write(&pinned, &o.text).unwrap(); }
                    let ours = std::fs::read_to_string(&pinned).unwrap_or_else(|e| panic!("{}: {e}", pinned.display()));
                    assert!(o.text == ours, "{}: {step} drifted from its pinned known gap {}\n{}", file.display(), pinned.display(), first_difference(&o.text, &ours));
                    assert!(o.text != want, "{}: {step} now matches Sky; remove its known_gap", file.display());
                    gaps += 1;
                    continue;
                }
                assert!(o.text == want, "{}: {step} differs from Sky\n{}\n--- ours ---\n{}\n--- sky ---\n{}",
                        file.display(), first_difference(&o.text, &want), o.text, want);
                compared += 1;
            }
            let _ = std::fs::remove_dir_all(&state);
        }
    }
    eprintln!("sky replay: {compared} captured observations match byte for byte, {gaps} pinned known gaps");
}
