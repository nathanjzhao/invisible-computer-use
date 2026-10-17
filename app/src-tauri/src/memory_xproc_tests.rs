//! Two app processes on one memory store (gaps-windows-2, 2026-10-04).
//!
//! The store lock (`MemoryService::store`) is a file lock on the folder on Unix and on a `.lock`
//! file inside it everywhere else. Its in-process half and its two-instance half were tested with
//! threads in one process; these tests run real processes: the test executable starts itself
//! with a role in the environment (the `xproc_child` test below is the child's entry, a no-op
//! otherwise) and the children use the same `MemoryService` code the app does, on one folder.
//!
//! What each test says about the lock:
//! - writers never corrupt the store and never lose each other's updates (two processes each own
//!   a field of `settings.json` and read, change and write it a few hundred times; without the
//!   lock a stale read writes the other process's field back);
//! - a second instance waits for the holder and then goes (it does not fail, and it does not
//!   go ahead early);
//! - a lock whose holder was killed locks nothing (the `.lock` file is still there on Windows);
//! - retention pruned from two processes at once removes what is expired exactly once and keeps
//!   the rest.
use super::*;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Folder(PathBuf);
impl Folder {
    fn new(tag: &str) -> Folder {
        let p = std::env::temp_dir().join(format!("icu-xproc-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Folder(p)
    }
    fn root(&self) -> PathBuf { self.0.join("app-memory") }
}
impl Drop for Folder { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

const NOW: Ts = 1_790_000_000;

fn run(id: &str, ended: Ts) -> RunRecord {
    RunRecord { shared_preferences:vec![], journal_chunk:None, run_id: id.into(), started: ended - 20, ended, app_name: "Calculator".into(), bundle_id: Some("com.apple.calculator".into()),
        goal: "Show results with two decimals.".into(), outcome: "done".into(), outcome_text: "ok".into(), decisions: 3, steps: vec![],
        brain: "claude-code:m".into(), brain_choice: "claude-code".into(), external_context: false, used_memory: vec![], answered: vec![],
        classified: false, cleared: vec![], trajectory: None }
}

fn report(v: Value) { println!("XPROC {v}"); }

/// The child's entry. Without `ICU_XPROC_ROLE` it does nothing, so a plain `cargo test` passes it.
#[test]
fn xproc_child() {
    let Ok(role) = std::env::var("ICU_XPROC_ROLE") else { return };
    let root = PathBuf::from(std::env::var("ICU_XPROC_ROOT").unwrap());
    let n: usize = std::env::var("ICU_XPROC_N").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let id = std::env::var("ICU_XPROC_ID").unwrap_or_default();
    let m = MemoryService::open(&root);
    match role.as_str() {
        // read, change one field, write it, n times; and read the store the way every check does
        "writer" => {
            let field = std::env::var("ICU_XPROC_FIELD").unwrap_or_default();
            let (mut errors, mut anomalies, mut last, mut read_failures) = (vec![], 0, 0, 0);
            for i in 0..n {
                if !field.is_empty() {
                    last = 100 + i as i64;
                    let mut patch = serde_json::Map::new();
                    patch.insert(field.clone(), json!(last));
                    if let Err(e) = m.update_settings(&Value::Object(patch)) { errors.push(format!("update_settings {i}: {e}")); }
                }
                // a bare read of settings.json while the others rename a new one over it, as every unlocked check does:
                // the open must succeed and what it reads must parse (a failed read falls back to defaults and hides it)
                match std::fs::read(root.join("settings.json")) {
                    Ok(bytes) => if serde_json::from_slice::<Value>(&bytes).is_err() { read_failures += 1; },
                    Err(e) => { read_failures += 1; errors.push(format!("read settings.json {i}: {e}")); }
                }
                match m.record_run(&run(&format!("r-{id}-{i}"), NOW), &RunOptions { allow_foreground:false, use_screenshots:false, use_memory: true, remember: true, brain_choice: "claude-code".into() }) {
                    Ok(true) => {}
                    other => errors.push(format!("record_run {i}: {other:?}")),
                }
                // another process is writing settings.json: a read must see a whole file, and the
                // delete generation must not fall back to 0 or move (nothing is deleted here)
                if m.generation() != 0 { anomalies += 1; }
                let s = m.settings();
                if s.validate().is_err() { anomalies += 1; }
            }
            report(json!({"id": id, "last": last, "errors": errors, "anomalies": anomalies, "read_failures": read_failures}));
        }
        // take the store lock and keep it for `ms` milliseconds (a test that wants it held until it kills this process
        // passes a long one: a failing parent must not leave a holder behind for good)
        "holder" => {
            let ms: u64 = std::env::var("ICU_XPROC_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(1000);
            let g = m.store().unwrap();
            report(json!({"locked": true}));
            std::thread::sleep(Duration::from_millis(ms));
            drop(g);
            report(json!({"released": true}));
        }
        "pruner" => {
            let mut total = 0;
            for _ in 0..n { total += m.prune(NOW); }
            report(json!({"id": id, "removed": total}));
        }
        "deleter" => {
            report(json!({"deleting":true}));
            let removed=m.delete("all").unwrap();
            report(json!({"deleted":true,"removed":removed}));
        }
        other => panic!("unknown role {other}"),
    }
}

fn child(role: &str, root: &Path, env: &[(&str, String)]) -> Child {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["xproc_child", "--nocapture", "--test-threads=1"]).env("ICU_XPROC_ROLE", role).env("ICU_XPROC_ROOT", root)
        .stdout(Stdio::piped()).stderr(Stdio::inherit());
    for (k, v) in env { c.env(k, v); }
    c.spawn().expect("the test executable starts itself")
}

/// The child's `XPROC` lines, each handed over as it arrives.
fn lines(c: &mut Child) -> std::sync::mpsc::Receiver<Value> {
    let (tx, rx) = std::sync::mpsc::channel();
    let out = c.stdout.take().unwrap();
    std::thread::spawn(move || {
        for l in BufReader::new(out).lines().map_while(Result::ok) {
            // libtest prints `test <name> ... ` before the test runs, so a line can start with that
            if let Some(i) = l.find("XPROC ") { let _ = tx.send(serde_json::from_str(&l[i + 6..]).unwrap()); }
        }
    });
    rx
}

fn finish(mut c: Child, rx: std::sync::mpsc::Receiver<Value>) -> Value {
    let v = rx.recv_timeout(Duration::from_secs(300)).expect("the child reported");
    assert!(c.wait().unwrap().success(), "the child exited cleanly");
    v
}

#[test]
fn two_processes_write_one_store_without_corruption_or_lost_updates() {
    let f = Folder::new("write");
    let root = f.root();
    MemoryService::open(&root).update_settings(&json!({"idleMinutes": 31})).unwrap();
    const N: usize = 150;
    let mut kids: Vec<(Child, std::sync::mpsc::Receiver<Value>)> = vec![];
    for (id, field) in [("a", "idleMinutes"), ("b", "retentionDays"), ("c", "")] {
        let mut c = child("writer", &root, &[("ICU_XPROC_ID", id.into()), ("ICU_XPROC_N", N.to_string()), ("ICU_XPROC_FIELD", field.into())]);
        let rx = lines(&mut c);
        kids.push((c, rx));
    }
    let reports: Vec<Value> = kids.into_iter().map(|(c, rx)| finish(c, rx)).collect();
    for r in &reports {
        assert_eq!(r["errors"], json!([]), "{r}");
        assert_eq!(r["anomalies"], 0, "a read saw a half-written store: {r}");
        assert_eq!(r["read_failures"], 0, "settings.json could not be read whole while another process replaced it: {r}");
    }
    // every process's own update survived the others: each field holds its owner's last value
    let m = MemoryService::open(&root);
    let s = m.settings();
    assert_eq!((s.idle_minutes, s.retention_days), (100 + N as i64 - 1, 100 + N as i64 - 1), "an update was lost: {s:?}");
    assert_eq!(m.generation(), 0);
    // all the runs are there, whole, and no temp file is left behind
    let runs = m.pending_runs();
    assert_eq!(runs.len(), 3 * N, "a run journal was lost or unreadable");
    let leftovers: Vec<String> = std::fs::read_dir(root.join("runs")).unwrap().chain(std::fs::read_dir(&root).unwrap())
        .filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".tmp")).collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn a_second_instance_waits_for_the_holder_and_then_goes() {
    let f = Folder::new("wait");
    let root = f.root();
    let m = MemoryService::open(&root);
    m.update_settings(&json!({"idleMinutes": 31})).unwrap();
    let mut holder = child("holder", &root, &[("ICU_XPROC_MS", "1500".into())]);
    let rx = lines(&mut holder);
    assert_eq!(rx.recv_timeout(Duration::from_secs(120)).unwrap(), json!({"locked": true}));
    let t = Instant::now();
    let s = m.update_settings(&json!({"idleMinutes": 32})).expect("the second instance goes once the holder lets go");
    let waited = t.elapsed();
    assert!(waited >= Duration::from_millis(900), "the second instance did not wait for the holder: {waited:?}");
    assert!(waited < Duration::from_secs(30), "it waited far longer than the holder held: {waited:?}");
    assert_eq!(s.idle_minutes, 32);
    assert_eq!(MemoryService::open(&root).settings().idle_minutes, 32);
    assert_eq!(rx.recv_timeout(Duration::from_secs(30)).unwrap(), json!({"released": true}), "the holder released before the second instance went");
    assert!(holder.wait().unwrap().success());
}

#[test]
fn a_lock_left_by_a_killed_process_locks_nothing() {
    let f = Folder::new("stale");
    let root = f.root();
    let m = std::sync::Arc::new(MemoryService::open(&root));
    m.update_settings(&json!({"idleMinutes": 31})).unwrap();
    let mut holder = child("holder", &root, &[("ICU_XPROC_MS", "90000".into())]);
    let rx = lines(&mut holder);
    assert_eq!(rx.recv_timeout(Duration::from_secs(120)).unwrap(), json!({"locked": true}));
    // while it lives, the other instance is held
    let (tx, done) = std::sync::mpsc::channel();
    { let m = m.clone(); std::thread::spawn(move || { let r = m.update_settings(&json!({"idleMinutes": 33})); let _ = tx.send(r.map(|s| s.idle_minutes)); }); }
    assert!(done.recv_timeout(Duration::from_millis(800)).is_err(), "the second instance went while the holder held the lock");
    // the holder dies without releasing anything
    holder.kill().unwrap();
    holder.wait().unwrap();
    let t = Instant::now();
    assert_eq!(done.recv_timeout(Duration::from_secs(30)).expect("the lock is released when its holder dies").unwrap(), 33);
    assert!(t.elapsed() < Duration::from_secs(10), "{:?}", t.elapsed());
    // and a fresh start after the death is not held either
    assert!(root.join(".lock").exists() == cfg!(not(unix)), "the .lock file stays behind off Unix, and there is none on Unix");
    let t = Instant::now();
    MemoryService::open(&root).update_settings(&json!({"idleMinutes": 34})).unwrap();
    assert!(t.elapsed() < Duration::from_secs(5), "a stale lock file held the next start: {:?}", t.elapsed());
    // a .lock nobody ever locked is no obstacle either
    std::fs::write(root.join(".lock"), b"left by an old build").unwrap();
    MemoryService::open(&root).update_settings(&json!({"idleMinutes": 35})).unwrap();
}

#[test]
fn retention_pruned_from_two_processes_removes_what_expired_once_and_keeps_the_rest() {
    let f = Folder::new("prune");
    let root = f.root();
    let m = MemoryService::open(&root);
    m.update_settings(&json!({"idleMinutes": 31})).unwrap(); // settings.json exists, as it does once the panel was opened
    let keep = m.settings().retention_days * 86_400;
    let (expired, fresh) = (60usize, 12usize);
    let opts = RunOptions { allow_foreground:false, use_screenshots:false, use_memory: true, remember: true, brain_choice: "claude-code".into() };
    for i in 0..expired { m.record_run(&run(&format!("old-{i}"), NOW - keep - 10 - i as i64), &opts).unwrap(); }
    for i in 0..fresh { m.record_run(&run(&format!("new-{i}"), NOW - 5 - i as i64), &opts).unwrap(); }
    let mut kids = vec![];
    for id in ["p", "q"] {
        let mut c = child("pruner", &root, &[("ICU_XPROC_ID", id.into()), ("ICU_XPROC_N", "3".into())]);
        let rx = lines(&mut c);
        kids.push((c, rx));
    }
    // a third process keeps writing fresh runs while the pruners work
    let mut w = child("writer", &root, &[("ICU_XPROC_ID", "w".into()), ("ICU_XPROC_N", "40".into())]);
    let wrx = lines(&mut w);
    let removed: i64 = kids.into_iter().map(|(c, rx)| finish(c, rx)["removed"].as_i64().unwrap()).sum();
    let wr = finish(w, wrx);
    assert_eq!(wr["errors"], json!([]), "{wr}");
    assert_eq!(removed, expired as i64, "every expired run is removed once in all");
    let left: BTreeSet<String> = MemoryService::open(&root).pending_runs().into_iter().map(|r| r.run_id).collect();
    assert_eq!(left.len(), fresh + 40, "{left:?}");
    assert!(left.iter().all(|id| !id.starts_with("old-")));
    assert!((0..fresh).all(|i| left.contains(&format!("new-{i}"))), "a fresh run was pruned");
}

#[test]
fn retention_in_another_process_cannot_republish_inference_evidence() {
    struct PrunedDuringInference { root: PathBuf, pruned: std::sync::atomic::AtomicBool }
    impl Models for PrunedDuringInference {
        fn complete(&self, stage: Stage, _: &str, _: &str, _: &str) -> Result<String,String> {
            match stage {
                Stage::Extract => {
                    let mut c=child("pruner",&self.root,&[("ICU_XPROC_N","1".into())]);
                    let rx=lines(&mut c);let report=finish(c,rx);
                    assert_eq!(report["removed"],1);
                    self.pruned.store(true,std::sync::atomic::Ordering::SeqCst);
                    Ok(json!({"title":"Calculator result","asked":"The user checked the result.",
                        "outcome":"The display was visible.","worked":[],"corrections":[],"preferences":[],
                        "app_facts":["In Calculator the result display is above the buttons."]}).to_string())
                },
                _ => panic!("A pruned extraction must not proceed to consolidation")
            }
        }
    }
    let folder=Folder::new("prune-during-inference");let root=folder.root();let m=MemoryService::open(&root);
    let keep=m.settings().retention_secs();
    m.update_settings(&json!({"idleMinutes":1})).unwrap();
    m.record_run(&run("expired-raw",NOW-keep-3600),&RunOptions::default()).unwrap();
    let models=PrunedDuringInference {root,pruned:std::sync::atomic::AtomicBool::new(false)};
    let report=m.tick(NOW-keep-1,false,&models);
    assert!(models.pruned.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(report.idle_reason.as_deref(),Some("memory was deleted during the pass"));
    assert!(!report.consolidated && report.extracted.is_empty(),"The stale extraction committed: {report:?}");
    assert!(m.durable().entries.is_empty(),"Expired evidence returned through a delayed model reply");
    assert!(m.summaries().is_empty());assert!(m.pending_runs().is_empty());
}

#[test]
fn deletion_acknowledges_after_the_current_native_action_barrier() {
    let folder=Folder::new("recall-action-barrier");let root=folder.root();let m=MemoryService::open(&root);
    m.update_settings(&json!({"idleMinutes":31})).unwrap();let revision=m.generation();
    let mut process=None;
    m.with_recall_revision(revision,|| {
        let mut c=child("deleter",&root,&[]);let rx=lines(&mut c);
        assert_eq!(rx.recv_timeout(Duration::from_secs(30)).unwrap(),json!({"deleting":true}));
        assert!(rx.recv_timeout(Duration::from_millis(250)).is_err(),"Delete acknowledged during the guarded native action");
        process=Some((c,rx));Ok(())
    }).unwrap();
    let (c,rx)=process.unwrap();assert_eq!(finish(c,rx)["deleted"],true);
    assert!(!m.recall_revision_is_current(revision));
    assert!(m.with_recall_revision(revision,||->Result<(),String>{panic!("Deleted context reached another action")}).is_err());
}
