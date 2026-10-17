//! Out-of-window persistent-state predicates for `verify_state` (spec 12): "did the file get
//! written", "is the process still running".
//!
//! The driver's `verify_state` checks bounded predicates against ONE explicitly authorized
//! window. A task like "save the document" ends outside any window, so the layer evaluates two
//! more predicate kinds itself:
//!
//! ```json
//! {"file":    {"path": "/abs/path", "exists": true, "size_bytes": 12, "sha256": "...", "contains": "text"}}
//! {"process": {"name": "TextEdit", "exists": true}}      // or {"pid": 4242, "exists": false}
//! ```
//!
//! They keep the driver's tri-state contract: `satisfied`, `unsatisfied`, `unknown`, and unknown
//! never implies success. A file's absence is provable (a missing path is `unsatisfied` for
//! `exists: true`, `satisfied` for `exists: false`); a path that cannot be read at all (permission,
//! I/O error) is `unknown` with `observation_unavailable`, never a guess.
//!
//! These escape the "one authorized window" invariant, so they sit behind an explicit capability
//! (`Features::persistent_predicates`, `ICU_PERSISTENT_PREDICATES=1`) and, when
//! `ICU_PREDICATE_ROOTS` is set, only for paths under those roots. Off, every such predicate is
//! `unknown` with reason `capability_disabled`. Contents are never returned, only the booleans and
//! the metadata the predicate asked about (size, hash, modified time).
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Largest file `sha256`/`contains` will read.
pub const MAX_READ_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status { Satisfied, Unsatisfied, Unknown }

impl Status {
    pub fn name(self) -> &'static str { match self { Status::Satisfied => "satisfied", Status::Unsatisfied => "unsatisfied", Status::Unknown => "unknown" } }
    /// The driver's aggregate rule: unsatisfied > unknown > satisfied.
    pub fn aggregate(all: impl IntoIterator<Item = Status>) -> Status {
        let mut out = Status::Satisfied;
        for s in all {
            match s { Status::Unsatisfied => return Status::Unsatisfied, Status::Unknown => out = Status::Unknown, Status::Satisfied => {} }
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub status: Status,
    pub unknown_reason: Option<&'static str>,
    /// Bounded projection of what was observed (never file contents).
    pub observed: Option<Value>,
}

impl Outcome {
    fn sat(b: bool, observed: Value) -> Outcome { Outcome { status: if b { Status::Satisfied } else { Status::Unsatisfied }, unknown_reason: None, observed: Some(observed) } }
    fn unknown(reason: &'static str, observed: Option<Value>) -> Outcome { Outcome { status: Status::Unknown, unknown_reason: Some(reason), observed } }
    pub fn to_json(&self, index: usize) -> Value {
        json!({"index": index, "status": self.status.name(), "unknown_reason": self.unknown_reason,
               "observed_json": self.observed.as_ref().map(Value::to_string)})
    }
}

/// Whether a predicate is one of ours (evaluated here) rather than the driver's.
pub fn is_persistent(p: &Value) -> bool { p.get("file").is_some() || p.get("process").is_some() }

/// The capability and its scope.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    pub enabled: bool,
    /// When non-empty, file paths must be under one of these.
    pub roots: Vec<PathBuf>,
}

/// Process listing, injectable for tests.
pub trait Processes {
    /// `(pid, executable basename or command)` for every running process, or None if it cannot be listed.
    fn list(&self) -> Option<Vec<(i64, String)>>;
}

pub struct SystemProcesses;
impl Processes for SystemProcesses {
    fn list(&self) -> Option<Vec<(i64, String)>> {
        #[cfg(unix)]
        {
            let out = std::process::Command::new("ps").args(["-axo", "pid=,comm="]).output().ok()?;
            if !out.status.success() { return None; }
            Some(String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| {
                let l = l.trim_start();
                let (pid, name) = l.split_once(char::is_whitespace)?;
                let name = name.trim();
                let base = name.rsplit('/').next().unwrap_or(name);
                Some((pid.parse().ok()?, base.to_string()))
            }).collect())
        }
        #[cfg(windows)]
        {
            let out = std::process::Command::new("tasklist").args(["/FO", "CSV", "/NH"]).output().ok()?;
            if !out.status.success() { return None; }
            Some(String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| {
                let mut f = l.trim().trim_matches('"').split("\",\"");
                let name = f.next()?; let pid = f.next()?;
                Some((pid.parse().ok()?, name.to_string()))
            }).collect())
        }
        #[cfg(not(any(unix, windows)))]
        { None }
    }
}

fn under_roots(path: &Path, roots: &[PathBuf]) -> bool {
    if roots.is_empty() { return true; }
    // compare canonical forms where the path exists; an absent path is checked lexically,
    // and `..` never counts as inside
    if path.components().any(|c| matches!(c, std::path::Component::ParentDir)) { return false; }
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let target = match path.parent().map(canon) { Some(parent) => parent.join(path.file_name().unwrap_or_default()), None => canon(path) };
    roots.iter().any(|r| target.starts_with(canon(r)))
}

/// SHA-256 of a byte string, lowercase hex.
pub fn sha256_hex_of(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    h.finish_hex()
}

fn sha256_hex(path: &Path) -> std::io::Result<String> {
    // a tiny SHA-256 so the layer needs no new dependency for one predicate
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop { let n = f.read(&mut buf)?; if n == 0 { break; } h.update(&buf[..n]); }
    Ok(h.finish_hex())
}

/// Evaluate one persistent predicate once.
pub fn evaluate(p: &Value, policy: &Policy, procs: &dyn Processes) -> Outcome {
    if !policy.enabled {
        return Outcome::unknown("capability_disabled", None);
    }
    if let Some(f) = p.get("file") { return evaluate_file(f, policy); }
    if let Some(pr) = p.get("process") { return evaluate_process(pr, procs); }
    Outcome::unknown("invalid_predicate", None)
}

fn evaluate_file(f: &Value, policy: &Policy) -> Outcome {
    let Some(path) = f.get("path").and_then(Value::as_str).filter(|s| !s.is_empty()) else { return Outcome::unknown("invalid_predicate", None) };
    let path = Path::new(path);
    if !path.is_absolute() { return Outcome::unknown("invalid_predicate", Some(json!({"reason": "path must be absolute"}))); }
    let known = ["path", "exists", "size_bytes", "sha256", "contains", "modified_after_ms"];
    if f.as_object().is_some_and(|o| o.keys().any(|k| !known.contains(&k.as_str()))) { return Outcome::unknown("invalid_predicate", Some(json!({"reason": "unknown field"}))); }
    if !under_roots(path, &policy.roots) { return Outcome::unknown("invalid_predicate", Some(json!({"reason": "path is outside the permitted roots"}))); }
    let want_exists = f.get("exists").and_then(Value::as_bool).unwrap_or(true);
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // absence is provable; asking about content of an absent file is unsatisfied too
            return Outcome::sat(!want_exists, json!({"exists": false}));
        }
        Err(_) => return Outcome::unknown("observation_unavailable", None),
    };
    let modified_ms = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64);
    let mut observed = json!({"exists": true, "size_bytes": meta.len(), "is_dir": meta.is_dir(), "modified_unix_ms": modified_ms});
    if !want_exists { return Outcome::sat(false, observed); }
    let mut ok = true;
    if let Some(n) = f.get("size_bytes").and_then(Value::as_u64) { ok &= meta.len() == n; }
    if let Some(after) = f.get("modified_after_ms").and_then(Value::as_u64) {
        match modified_ms { Some(m) => ok &= m > after, None => return Outcome::unknown("observation_unavailable", Some(observed)) }
    }
    if f.get("sha256").is_some() || f.get("contains").is_some() {
        if meta.is_dir() { return Outcome::unknown("invalid_predicate", Some(observed)); }
        if meta.len() > MAX_READ_BYTES { return Outcome::unknown("observation_unavailable", Some(json!({"reason": "file is larger than the read cap", "size_bytes": meta.len()}))); }
        if let Some(want) = f.get("sha256").and_then(Value::as_str) {
            match sha256_hex(path) {
                Ok(got) => { ok &= got.eq_ignore_ascii_case(want); observed["sha256_matches"] = json!(got.eq_ignore_ascii_case(want)); }
                Err(_) => return Outcome::unknown("observation_unavailable", Some(observed)),
            }
        }
        if let Some(needle) = f.get("contains").and_then(Value::as_str) {
            match std::fs::read(path) {
                Ok(bytes) => { let hit = String::from_utf8_lossy(&bytes).contains(needle); ok &= hit; observed["contains"] = json!(hit); }
                Err(_) => return Outcome::unknown("observation_unavailable", Some(observed)),
            }
        }
    }
    Outcome::sat(ok, observed)
}

fn evaluate_process(p: &Value, procs: &dyn Processes) -> Outcome {
    let name = p.get("name").and_then(Value::as_str).filter(|s| !s.is_empty());
    let pid = p.get("pid").and_then(Value::as_i64);
    if name.is_none() == pid.is_none() { return Outcome::unknown("invalid_predicate", Some(json!({"reason": "give exactly one of name or pid"}))); }
    if p.as_object().is_some_and(|o| o.keys().any(|k| !["name", "pid", "exists"].contains(&k.as_str()))) { return Outcome::unknown("invalid_predicate", Some(json!({"reason": "unknown field"}))); }
    let want = p.get("exists").and_then(Value::as_bool).unwrap_or(true);
    let Some(list) = procs.list() else { return Outcome::unknown("observation_unavailable", None) };
    let matches: Vec<i64> = list.iter().filter(|(id, comm)| match (name, pid) {
        (Some(n), _) => comm.eq_ignore_ascii_case(n) || comm.trim_end_matches(".exe").eq_ignore_ascii_case(n.trim_end_matches(".exe")),
        (_, Some(want_pid)) => *id == want_pid,
        _ => false,
    }).map(|(id, _)| *id).collect();
    // a process list is exhaustive, so absence is provable
    Outcome::sat(matches.is_empty() != want, json!({"running": !matches.is_empty(), "count": matches.len()}))
}

/// One monotonic budget and consecutive-success count for persistent or mixed verification.
/// Zero requests one immediate sample; a nonzero budget cannot accept a late sample as success.
/// Synchronous observations cannot be cancelled, so a single read may overrun the budget.
pub(crate) struct Poll {
    started: std::time::Instant,
    timeout: std::time::Duration,
    need: u64,
    consecutive: u64,
    pub(crate) samples: u64,
}

impl Poll {
    pub(crate) fn new(timeout_ms: u64, stable_samples: u64) -> Self {
        Self { started: std::time::Instant::now(), timeout: std::time::Duration::from_millis(timeout_ms),
            need: stable_samples.max(1), consecutive: 0, samples: 0 }
    }
    pub(crate) fn elapsed_ms(&self) -> u64 { self.started.elapsed().as_millis().min(u64::MAX as u128) as u64 }
    pub(crate) fn can_observe(&self) -> bool { self.timeout.is_zero() || self.started.elapsed() < self.timeout }
    pub(crate) fn record(&mut self, satisfied: bool) -> bool {
        self.samples = self.samples.saturating_add(1);
        self.consecutive = if satisfied && self.can_observe() { self.consecutive.saturating_add(1) } else { 0 };
        self.consecutive >= self.need
    }
    /// Never begin another cycle after sleeping to the deadline.
    pub(crate) fn wait(&self) -> bool {
        let remaining = self.timeout.saturating_sub(self.started.elapsed());
        if remaining.is_zero() { return false; }
        std::thread::sleep(remaining.min(std::time::Duration::from_millis(100)));
        self.started.elapsed() < self.timeout
    }
}

/// Poll the predicates (indexes are the caller's) until all are satisfied for `stable_samples`
/// consecutive samples or `timeout_ms` passes, like the driver's loop. Returns the final outcomes,
/// whether the last sample set was stable, the samples taken and the elapsed milliseconds.
pub fn poll(preds: &[(usize, Value)], policy: &Policy, procs: &dyn Processes, timeout_ms: u64, stable_samples: u64) -> (Vec<(usize, Outcome)>, bool, u64, u64) {
    let mut polling = Poll::new(timeout_ms, stable_samples);
    loop {
        let mut outcomes: Vec<(usize, Outcome)> = preds.iter().map(|(i, p)| (*i, evaluate(p, policy, procs))).collect();
        let agg = Status::aggregate(outcomes.iter().map(|(_, o)| o.status));
        let stable = polling.record(agg == Status::Satisfied);
        // a capability or shape problem will not change by waiting: stop at once
        let permanent = outcomes.iter().any(|(_, o)| matches!(o.unknown_reason, Some("capability_disabled" | "invalid_predicate")));
        if stable || permanent || !polling.wait() {
            // Like the driver, a last satisfied sample is not success without the required streak.
            if !stable && agg == Status::Satisfied {
                for (_, o) in &mut outcomes { o.status = Status::Unknown; o.unknown_reason = Some("stability_unproven"); }
            }
            return (outcomes, stable, polling.samples, polling.elapsed_ms());
        }
    }
}

// ---- SHA-256 (FIPS 180-4), small and tested against the standard vectors ----------------------
struct Sha256 { h: [u32; 8], buf: Vec<u8>, len: u64 }
const K: [u32; 64] = [
    0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
    0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
    0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
    0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2];
impl Sha256 {
    fn new() -> Sha256 { Sha256 { h: [0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19], buf: Vec::new(), len: 0 } }
    fn block(h: &mut [u32; 8], b: &[u8]) {
        let mut w = [0u32; 64];
        for i in 0..16 { w[i] = u32::from_be_bytes([b[4*i], b[4*i+1], b[4*i+2], b[4*i+3]]); }
        for i in 16..64 {
            let s0 = w[i-15].rotate_right(7) ^ w[i-15].rotate_right(18) ^ (w[i-15] >> 3);
            let s1 = w[i-2].rotate_right(17) ^ w[i-2].rotate_right(19) ^ (w[i-2] >> 10);
            w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }
        let mut v = *h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 { h[i] = h[i].wrapping_add(v[i]); }
    }
    fn update(&mut self, data: &[u8]) {
        self.len += data.len() as u64;
        self.buf.extend_from_slice(data);
        let full = self.buf.len() / 64 * 64;
        for c in self.buf[..full].chunks(64) { Self::block(&mut self.h, c); }
        self.buf.drain(..full);
    }
    fn finish_hex(mut self) -> String {
        let bits = self.len * 8;
        let mut tail = std::mem::take(&mut self.buf);
        tail.push(0x80);
        while tail.len() % 64 != 56 { tail.push(0); }
        tail.extend_from_slice(&bits.to_be_bytes());
        for c in tail.chunks(64) { Self::block(&mut self.h, c); }
        self.h.iter().map(|x| format!("{x:08x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(Option<Vec<(i64, String)>>);
    impl Processes for Fixed { fn list(&self) -> Option<Vec<(i64, String)>> { self.0.clone() } }
    fn on() -> Policy { Policy { enabled: true, roots: vec![] } }
    fn none() -> Fixed { Fixed(Some(vec![])) }

    #[test]
    fn sha256_matches_the_standard_vectors() {
        let hex = |data: &[u8]| { let mut h = Sha256::new(); h.update(data); h.finish_hex() };
        assert_eq!(hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        // crosses the 55/56/64 byte padding edges and a multi-block message
        assert_eq!(hex(&b"a".repeat(1_000_000)), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
        assert_eq!(hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
    }

    #[test]
    fn a_file_the_task_should_have_written_is_checked_by_existence_size_hash_and_content() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("out.txt");
        let path = f.to_str().unwrap();
        let p = |extra: Value| { let mut o = json!({"path": path}); o.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone()); json!({"file": o}) };
        assert_eq!(evaluate(&p(json!({})), &on(), &none()).status, Status::Unsatisfied, "not written yet");
        assert_eq!(evaluate(&p(json!({"exists": false})), &on(), &none()).status, Status::Satisfied, "absence is provable");
        std::fs::write(&f, "abc").unwrap();
        assert_eq!(evaluate(&p(json!({})), &on(), &none()).status, Status::Satisfied);
        assert_eq!(evaluate(&p(json!({"exists": false})), &on(), &none()).status, Status::Unsatisfied);
        assert_eq!(evaluate(&p(json!({"size_bytes": 3})), &on(), &none()).status, Status::Satisfied);
        assert_eq!(evaluate(&p(json!({"size_bytes": 4})), &on(), &none()).status, Status::Unsatisfied);
        assert_eq!(evaluate(&p(json!({"sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"})), &on(), &none()).status, Status::Satisfied);
        assert_eq!(evaluate(&p(json!({"sha256": "00"})), &on(), &none()).status, Status::Unsatisfied);
        assert_eq!(evaluate(&p(json!({"contains": "b"})), &on(), &none()).status, Status::Satisfied);
        assert_eq!(evaluate(&p(json!({"contains": "z"})), &on(), &none()).status, Status::Unsatisfied);
        // contents are never reported, only the booleans
        let o = evaluate(&p(json!({"contains": "b"})), &on(), &none());
        assert!(!o.observed.unwrap().to_string().contains("abc"));
    }

    #[test]
    fn unreadable_or_malformed_is_unknown_never_a_guess() {
        let dir = tempfile::tempdir().unwrap();
        let p = |v: Value| evaluate(&v, &on(), &none());
        assert_eq!(p(json!({"file": {"path": "relative/x"}})).unknown_reason, Some("invalid_predicate"));
        assert_eq!(p(json!({"file": {}})).unknown_reason, Some("invalid_predicate"));
        assert_eq!(p(json!({"file": {"path": dir.path().to_str().unwrap(), "sha256": "x"}})).unknown_reason, Some("invalid_predicate"), "a directory has no hash");
        assert_eq!(p(json!({"file": {"path": "/x", "bogus": 1}})).unknown_reason, Some("invalid_predicate"));
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            let sub = dir.path().join("locked"); std::fs::create_dir(&sub).unwrap(); std::fs::write(sub.join("f"), "x").unwrap();
            std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o000)).unwrap();
            let r = p(json!({"file": {"path": sub.join("f").to_str().unwrap()}}));
            std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o755)).unwrap();
            if unsafe { libc::geteuid() } != 0 { assert_eq!((r.status, r.unknown_reason), (Status::Unknown, Some("observation_unavailable"))); }
        }
    }

    #[test]
    fn the_capability_is_off_by_default_and_roots_confine_paths() {
        let dir = tempfile::tempdir().unwrap();
        let inside = dir.path().join("a.txt"); std::fs::write(&inside, "x").unwrap();
        let pred = json!({"file": {"path": inside.to_str().unwrap()}});
        let off = Policy::default();
        let o = evaluate(&pred, &off, &none());
        assert_eq!((o.status, o.unknown_reason), (Status::Unknown, Some("capability_disabled")), "off: unknown, not unsatisfied");
        assert!(evaluate(&json!({"process": {"name": "x"}}), &off, &none()).unknown_reason == Some("capability_disabled"));
        let scoped = Policy { enabled: true, roots: vec![dir.path().to_path_buf()] };
        assert_eq!(evaluate(&pred, &scoped, &none()).status, Status::Satisfied);
        let outside = json!({"file": {"path": "/etc/hosts"}});
        assert_eq!(evaluate(&outside, &scoped, &none()).unknown_reason, Some("invalid_predicate"));
        let dotdot = json!({"file": {"path": format!("{}/../../etc/hosts", dir.path().display())}});
        assert_eq!(evaluate(&dotdot, &scoped, &none()).unknown_reason, Some("invalid_predicate"), "`..` does not escape the root");
    }

    #[test]
    fn process_predicates_match_by_name_or_pid_and_absence_is_provable() {
        let procs = Fixed(Some(vec![(10, "launchd".into()), (4242, "TextEdit".into()), (7, "notepad.exe".into())]));
        let e = |v: Value| evaluate(&v, &on(), &procs);
        assert_eq!(e(json!({"process": {"name": "textedit"}})).status, Status::Satisfied, "case-insensitive");
        assert_eq!(e(json!({"process": {"name": "notepad"}})).status, Status::Satisfied, ".exe is optional");
        assert_eq!(e(json!({"process": {"name": "Pages"}})).status, Status::Unsatisfied);
        assert_eq!(e(json!({"process": {"name": "Pages", "exists": false}})).status, Status::Satisfied);
        assert_eq!(e(json!({"process": {"pid": 4242}})).status, Status::Satisfied);
        assert_eq!(e(json!({"process": {"pid": 4243, "exists": false}})).status, Status::Satisfied);
        assert_eq!(e(json!({"process": {"pid": 1, "name": "x"}})).unknown_reason, Some("invalid_predicate"), "exactly one selector");
        assert_eq!(e(json!({"process": {}})).unknown_reason, Some("invalid_predicate"));
        assert_eq!(evaluate(&json!({"process": {"name": "x"}}), &on(), &Fixed(None)).unknown_reason, Some("observation_unavailable"));
    }

    #[cfg(unix)]
    #[test]
    fn the_system_process_list_sees_this_process() {
        let me = std::process::id() as i64;
        let o = evaluate(&json!({"process": {"pid": me}}), &on(), &SystemProcesses);
        assert_eq!(o.status, Status::Satisfied, "{o:?}");
        let o = evaluate(&json!({"process": {"pid": 2_000_000_000}}), &on(), &SystemProcesses);
        assert_eq!(o.status, Status::Unsatisfied);
    }

    #[test]
    fn aggregation_follows_the_drivers_rule() {
        use Status::*;
        assert_eq!(Status::aggregate([Satisfied, Satisfied]), Satisfied);
        assert_eq!(Status::aggregate([Satisfied, Unknown]), Unknown);
        assert_eq!(Status::aggregate([Unknown, Unsatisfied]), Unsatisfied);
        assert_eq!(Status::aggregate([]), Satisfied);
    }

    #[test]
    fn polling_waits_for_a_file_that_appears_and_gives_up_at_the_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("late.txt");
        let preds = vec![(2usize, json!({"file": {"path": f.to_str().unwrap()}}))];
        let writer = { let f = f.clone(); std::thread::spawn(move || { std::thread::sleep(std::time::Duration::from_millis(250)); std::fs::write(f, "x").unwrap(); }) };
        let (out, stable, samples, _) = poll(&preds, &on(), &none(), 3000, 2);
        writer.join().unwrap();
        assert!(stable && samples >= 3, "stable={stable} samples={samples}");
        assert_eq!(out[0].0, 2, "the caller's index is kept");
        assert_eq!(out[0].1.status, Status::Satisfied);
        let (out, stable, _, elapsed) = poll(&[(0, json!({"file": {"path": dir.path().join("never").to_str().unwrap()}}))], &on(), &none(), 300, 1);
        assert!(!stable && out[0].1.status == Status::Unsatisfied && (300..1500).contains(&elapsed), "{elapsed}");
        // a disabled capability does not burn the whole timeout
        let (_, _, samples, elapsed) = poll(&preds, &Policy::default(), &none(), 5000, 1);
        assert_eq!(samples, 1);
        assert!(elapsed < 500);
    }
}
