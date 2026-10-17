//! A short-lived owner for the explicit audio recorder. Parent death closes
//! stdin, allowing a bounded flush instead of killing the recorder mid-file.
use std::{ffi::OsString, io::{Read, Write}, path::{Path, PathBuf}, process::{Command, Output},
    sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Duration};

const FLAG: &str = "--icu-audio-supervisor";

struct Request { helper: PathBuf, root: PathBuf, source: String, milliseconds: u64 }
impl Request {
    fn parse(args: &[OsString]) -> Result<Self, String> {
        if args.len() != 4 { return Err("Invalid audio supervisor arguments".into()); }
        let root = PathBuf::from(&args[0]);
        let source = args[1].to_str().ok_or("Invalid audio source")?.to_owned();
        let milliseconds = args[2].to_str().and_then(|v|v.parse::<u64>().ok()).ok_or("Invalid audio duration")?;
        let helper = PathBuf::from(&args[3]);
        if !matches!(source.as_str(), "system" | "microphone" | "both") || !(100..=3_600_000).contains(&milliseconds)
            || !root.is_absolute() || !root.is_dir() || !helper.is_absolute() || !helper.is_file() {
            return Err("Invalid audio recording request".into());
        }
        Ok(Self { helper, root, source, milliseconds })
    }
}

pub fn capture(helper: &Path, root: &Path, source: &str, milliseconds: u64, stop: Arc<AtomicBool>) -> Result<Output, String> {
    if stop.load(Ordering::SeqCst) { return Err("Audio recording was cancelled before start".into()); }
    let executable = std::env::current_exe().map_err(|e|e.to_string())?;
    crate::subprocess::audio_supervisor_output(Command::new(executable).arg(FLAG)
        .arg(root).arg(source).arg(milliseconds.to_string()).arg(helper), stop, Duration::from_millis(milliseconds.saturating_add(22_000)))
}

fn run(request: Request) -> Result<Output, String> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new().name("audio-startup".into()).spawn(move || {
        let mut handshake = [0; 6];
        let result = std::io::stdin().lock().read_exact(&mut handshake).map(|()|handshake);
        let _ = sender.send(result);
    }).map_err(|e|format!("Cannot watch audio startup: {e}"))?;
    let handshake = receiver.recv_timeout(Duration::from_secs(5))
        .map_err(|_|"Audio owner did not start the recording")?
        .map_err(|_|"Audio owner left before startup")?;
    if handshake != *b"start\n" { return Err("Invalid audio startup handshake".into()); }
    let stop = Arc::new(AtomicBool::new(false));
    let signal = stop.clone();
    std::thread::Builder::new().name("audio-owner".into()).spawn(move || {
        // Stop, malformed input and EOF all revoke the recording. A separate
        // pipe feeds the recorder, so it cannot keep this owner pipe alive.
        let _ = std::io::stdin().lock().read(&mut [0; 1]);
        signal.store(true, Ordering::SeqCst);
    }).map_err(|e|format!("Cannot watch audio owner: {e}"))?;
    // The recorder still gets the normal job/process-tree containment. Windows
    // kills that job if this supervisor fails. The bundled recorder also has
    // its own bounded EOF handler. If the app fails, EOF above requests Stop
    // and this path permits at most six seconds to flush.
    crate::subprocess::recorder_output(Command::new(request.helper).arg("audio")
        .arg("--out").arg(request.root).arg("--source").arg(request.source)
        .arg("--max-duration-ms").arg(request.milliseconds.to_string()), stop, Duration::from_millis(request.milliseconds + 18_000))
}

/// Must run before Tauri, memory workers or any GUI initialization.
pub fn entry() -> Option<i32> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_none_or(|arg| arg != FLAG) { return None; }
    Some(finish(Request::parse(&args[1..]).and_then(run)))
}

fn finish(result: Result<Output, String>) -> i32 {
    match result {
        Ok(output) => {
            // The app may already be gone. A broken output pipe must not undo
            // the completed recording or prevent process-tree cleanup.
            let _ = std::io::stdout().lock().write_all(&output.stdout);
            let _ = std::io::stderr().lock().write_all(&output.stderr);
            output.status.code().unwrap_or(1)
        }
        Err(error) => { let _ = writeln!(std::io::stderr().lock(), "Audio recording: {error}"); 1 }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{os::unix::fs::PermissionsExt, process::{Child, Stdio}, time::{Instant, SystemTime, UNIX_EPOCH}};

    // Invoked only in a separate test process, never through the production CLI.
    #[test]
    fn worker() {
        let Ok(value) = std::env::var("ICU_TEST_AUDIO_REQUEST") else { return };
        let args: Vec<String> = serde_json::from_str(&value).unwrap();
        let args: Vec<_> = args.into_iter().map(OsString::from).collect();
        std::process::exit(finish(Request::parse(&args).and_then(run)));
    }

    struct Fixture { root: PathBuf, child: Child, completed: bool }
    impl Fixture {
        fn start(body: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!("icu-audio-owner-{}-{}-{}", std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),NEXT.fetch_add(1,Ordering::Relaxed)));
            std::fs::create_dir(&root).unwrap();
            let helper = root.join("taiku-observer");
            std::fs::write(&helper, format!("#!/bin/sh\necho $$ >\"$3/recorder.pid\"\n{body}\n")).unwrap();
            std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
            let args = vec![root.to_str().unwrap(), "system", "30000", helper.to_str().unwrap()];
            let child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "audio_supervisor::tests::worker", "--nocapture"])
                .env("ICU_TEST_AUDIO_REQUEST", serde_json::to_string(&args).unwrap())
                .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
            Self { root, child, completed: false }
        }
        fn ready(&self) {
            let end = Instant::now() + Duration::from_secs(5);
            while !self.root.join("recorder.pid").exists() && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(self.root.join("recorder.pid").exists(), "recorder never started");
        }
        fn wait(&mut self) -> std::process::ExitStatus {
            let end = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(status) = self.child.try_wait().unwrap() { self.completed = true; return status; }
                assert!(Instant::now() < end, "audio owner outlived its deadline");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            // A completed guardian has reaped its recorder. Never signal the
            // old group ID after that point, when the OS may reuse it.
            if !self.completed {
                if let Ok(pid) = std::fs::read_to_string(self.root.join("recorder.pid")) {
                    if let Ok(pid) = pid.trim().parse::<i32>() { unsafe { libc::kill(-pid, libc::SIGKILL); } }
                }
            }
            let _ = self.child.kill(); let _ = self.child.wait();
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn owner_eof_flushes_the_existing_recorder() {
        let mut f = Fixture::start("read -r control\ntest \"$control\" = stop || exit 2\nprintf complete >\"$3/finalized\"\nprintf '{\"finalized\":true}\\n'");
        let mut input = f.child.stdin.take().unwrap(); input.write_all(b"start\n").unwrap();
        f.ready(); drop(input);
        assert!(f.wait().success());
        assert_eq!(std::fs::read_to_string(f.root.join("finalized")).unwrap(), "complete");
    }

    #[test]
    fn owner_departure_before_handshake_never_starts_capture() {
        let mut f = Fixture::start("exit 0");
        drop(f.child.stdin.take());
        assert!(!f.wait().success());
        assert!(!f.root.join("recorder.pid").exists());
    }

    #[test]
    fn explicit_stop_completes_the_supervised_parent_protocol() {
        let stop = Arc::new(AtomicBool::new(false));
        let signal = stop.clone();
        std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(80)); signal.store(true, Ordering::SeqCst); });
        let output = crate::subprocess::audio_supervisor_output(Command::new("/bin/sh")
            .args(["-c", "read -r start; test \"$start\" = start || exit 2; read -r stop; test \"$stop\" = stop || exit 3; echo finalized"]),
            stop, Duration::from_secs(2)).unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"finalized\n");
    }

    #[test]
    fn an_unresponsive_recorder_is_reaped_after_owner_death() {
        let mut f = Fixture::start("while :; do sleep 1; done");
        let mut input = f.child.stdin.take().unwrap(); input.write_all(b"start\n").unwrap();
        f.ready(); let pid: i32 = std::fs::read_to_string(f.root.join("recorder.pid")).unwrap().trim().parse().unwrap();
        let start = Instant::now(); drop(input);
        assert!(!f.wait().success());
        assert!(start.elapsed() < Duration::from_secs(9));
        assert_eq!(unsafe { libc::kill(-pid, 0) }, -1, "recorder process group survived");
    }
}
