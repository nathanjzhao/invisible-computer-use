//! Terminal probe for a real Mac: the macOS source and the recorder, nothing else. Writes every
//! record and marker to `<out>/records.jsonl` and every raw terminal read the source took to
//! `<out>/reads.jsonl` (to see what the AX text looked like), until `<out>/stop` exists.
//!
//!   term_probe <out dir>
#[cfg(target_os = "macos")]
fn main() {
    use skysight::host::Source;
    use skysight::stream::{Observation, ObservationSettings, Output, Recorder};
    use std::io::Write;
    if std::env::args().nth(1).as_deref() == Some("--diag") { println!("{}", skysight::macos::diag()); return; }
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("term_probe <out dir>"));
    std::fs::create_dir_all(&out).unwrap();
    let mut recs = std::fs::File::create(out.join("records.jsonl")).unwrap();
    let mut reads = std::fs::File::create(out.join("reads.jsonl")).unwrap();
    let mut src = skysight::macos::MacSource::new();
    if let Err(e) = src.start() { eprintln!("start: {e}"); std::process::exit(2); }
    let mut r = Recorder::new(ObservationSettings::default(), std::process::id() as i64);
    let write = |recs: &mut std::fs::File, o: Vec<Output>| {
        for x in o {
            let line = match x {
                Output::Record(e) => serde_json::json!({"record": e}),
                Output::Suppressed(s) => serde_json::json!({"suppressed": s}),
                #[allow(unreachable_patterns)]
                _ => continue,
            };
            writeln!(recs, "{line}").unwrap();
        }
        recs.flush().unwrap();
    };
    write(&mut recs, r.start(skysight::now_secs()));
    println!("PROBE_READY");
    while !out.join("stop").exists() {
        if let Some((obs, ts)) = src.next(std::time::Duration::from_millis(100)) {
            if let Observation::TerminalValue { window_id, value } = &obs {
                writeln!(reads, "{}", serde_json::json!({"ts": ts, "window": window_id, "value": value})).unwrap();
                reads.flush().unwrap();
            }
            write(&mut recs, r.observe(obs, ts));
        }
        write(&mut recs, r.tick(skysight::now_secs()));
    }
    write(&mut recs, r.stop(skysight::now_secs(), "stopped"));
    src.stop();
}

#[cfg(not(target_os = "macos"))]
fn main() { eprintln!("macOS only"); }
