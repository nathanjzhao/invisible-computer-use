//! Bounded subprocess output. Drain both pipes while waiting and reap on failure.
use std::io::Read;
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub const CALL_TIMEOUT: Duration = Duration::from_secs(15);
const OUTPUT_LIMIT: u64 = 32 * 1024 * 1024;

#[cfg(windows)]
#[path = "subprocess_windows.rs"]
mod windows;

/// Children still running on behalf of this app, by leader PID, with the
/// command's tree tag. Unix children lead their own process groups, so host
/// exit would otherwise leave them running. A leader stays registered, and
/// unreaped, until its command is finished with: the zombie keeps its PID, and
/// so the group ID, from being reused while descendants may still hold the pipes.
struct Live { closed: bool, groups: Option<std::collections::HashMap<u32, String>> }
static LIVE: std::sync::Mutex<Live> = std::sync::Mutex::new(Live { closed: false, groups: None });

/// Every command gets `ICU_TREE_TAG=<app pid>-<run>-<n>` in its environment,
/// which its descendants inherit unless they rewrite their environment. The kill
/// walk uses it to find a descendant that both left the group and lost its
/// parent. `<run>` is this process's start time in nanoseconds: a helper a
/// command left running (the daemon `icu` starts keeps its tag for as long as it
/// lives) must not match a later app run that got the same pid and counted to the
/// same `<n>`, or that run's timeout would kill it.
const TREE_TAG: &str = "ICU_TREE_TAG";
static NEXT_TAG: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static RUN_NONCE: std::sync::OnceLock<u128> = std::sync::OnceLock::new();

fn command_tag(pid: u32, run: u128, n: u64) -> String { format!("{pid}-{run:x}-{n}") }

fn next_tag() -> String {
    let run = *RUN_NONCE.get_or_init(|| std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    command_tag(std::process::id(), run, NEXT_TAG.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}
fn live() -> std::sync::MutexGuard<'static, Live> { LIVE.lock().unwrap_or_else(std::sync::PoisonError::into_inner) }

/// App exit: refuse new commands and end every in-flight command tree. Each
/// waiting caller then reaps its own child and reports shutdown. Windows trees
/// are also contained by kill-on-close job objects if the host dies first.
pub fn close() {
    let mut live = live();
    live.closed = true;
    #[cfg(unix)]
    for (group, tag) in live.groups.iter().flatten() {
        kill_tree(*group, tag);
    }
}

/// SIGKILL a command's tree: its process group, every process descended from
/// the leader or from a group member, and every process carrying the command's
/// tree tag. The descendant walk reaches a process that left the group
/// (`setsid`) while its parent chain still leads back; the tag reaches one whose
/// parent exited first, which was reparented to init/launchd. A process that
/// does both and also clears its environment is still out of reach. Everything
/// found is stopped first, so nothing forks past the walk.
#[cfg(unix)]
fn kill_tree(leader: u32, tag: &str) {
    let leader = leader as i32;
    let me = std::process::id() as i32;
    let entry = format!("{TREE_TAG}={tag}");
    let mut found = std::collections::HashSet::new();
    unsafe { libc::kill(-leader, libc::SIGSTOP); }
    loop {
        let table = process_table();
        let before = found.len();
        let tagged: std::collections::HashSet<i32> = table.iter()
            .filter(|&&(pid, _, _)| pid > 1 && pid != me && !found.contains(&pid) && has_env_entry(pid, &entry))
            .map(|&(pid, _, _)| pid).collect();
        let mut grew = true;
        while grew {
            grew = false;
            for &(pid, ppid, pgid) in &table {
                if pid <= 1 || pid == me || found.contains(&pid) { continue; }
                if pgid == leader || ppid == leader || found.contains(&ppid) || tagged.contains(&pid) {
                    unsafe { libc::kill(pid, libc::SIGSTOP); }
                    found.insert(pid);
                    grew = true;
                }
            }
        }
        if found.len() == before { break; }
    }
    unsafe { libc::kill(-leader, libc::SIGKILL); }
    for pid in found { unsafe { libc::kill(pid, libc::SIGKILL); } }
}

/// `(pid, ppid, pgid)` for every process this user can see.
#[cfg(target_os = "macos")]
fn process_table() -> Vec<(i32, i32, i32)> {
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    let mut pids = vec![0i32; count.max(0) as usize + 64];
    let bytes = (pids.len() * std::mem::size_of::<i32>()) as libc::c_int;
    let count = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    pids.truncate(count.max(0) as usize);
    pids.into_iter().filter_map(|pid| {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        let read = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), size) };
        (read == size).then_some((pid, info.pbi_ppid as i32, info.pbi_pgid as i32))
    }).collect()
}

/// Whether `pid`'s initial environment holds exactly `entry` (`NAME=value`),
/// from `KERN_PROCARGS2`: argc, the exec path, argv, then the environment.
/// Only this user's processes are readable, which is all the walk needs, and
/// macOS leaves the environment out for platform binaries (`/bin/sh`,
/// `/bin/sleep`), so the tag finds an orphaned script interpreter or app
/// binary but not an orphaned system tool.
#[cfg(target_os = "macos")]
fn has_env_entry(pid: i32, entry: &str) -> bool {
    // Sized by KERN_ARGMAX: a size query on KERN_PROCARGS2 itself under-reports
    // and returns a tail without the environment (measured on macOS 26).
    let mut argmax: libc::c_int = 0;
    let mut size = std::mem::size_of::<libc::c_int>();
    let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
    if unsafe { libc::sysctl(mib.as_mut_ptr(), 2, (&mut argmax as *mut libc::c_int).cast(), &mut size, std::ptr::null_mut(), 0) } != 0 { return false; }
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut size = argmax.max(0) as libc::size_t;
    let mut buf = vec![0u8; size];
    if unsafe { libc::sysctl(mib.as_mut_ptr(), 3, buf.as_mut_ptr().cast(), &mut size, std::ptr::null_mut(), 0) } != 0 { return false; }
    buf.truncate(size);
    if buf.len() < 4 { return false; }
    // The exec path, NUL padding, then argv and the environment. The padding's
    // length varies with the path, and an empty argument is one more NUL, so the
    // two cannot be told apart and argc cannot be used to skip the arguments:
    // counting them would take the first environment entry for an argument
    // whenever argv starts with an empty string. Every string after the path is
    // compared instead; an argument equal to a whole `NAME=value` entry is as
    // good a sign of this command's tree as the environment.
    let mut strings = buf[4..].split(|b| *b == 0).skip_while(|s| s.is_empty());
    if strings.next().is_none() { return false; }
    strings.any(|s| s == entry.as_bytes())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn has_env_entry(pid: i32, entry: &str) -> bool {
    std::fs::read(format!("/proc/{pid}/environ"))
        .is_ok_and(|env| env.split(|b| *b == 0).any(|s| s == entry.as_bytes()))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn process_table() -> Vec<(i32, i32, i32)> {
    let Ok(dir) = std::fs::read_dir("/proc") else { return Vec::new() };
    dir.flatten().filter_map(|entry| {
        let pid: i32 = entry.file_name().to_str()?.parse().ok()?;
        let stat = std::fs::read_to_string(entry.path().join("stat")).ok()?;
        // `pid (comm) state ppid pgrp ...`; comm may itself contain ") ".
        let mut rest = stat.get(stat.rfind(')')? + 2..)?.split_whitespace().skip(1);
        Some((pid, rest.next()?.parse().ok()?, rest.next()?.parse().ok()?))
    }).collect()
}

/// Whether the leader has exited, without reaping it on Unix (see [`Live`]).
#[cfg(unix)]
fn has_exited(child: &mut Child) -> Result<bool, String> {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let flags = libc::WEXITED | libc::WNOHANG | libc::WNOWAIT;
    if unsafe { libc::waitid(libc::P_PID, child.id() as libc::id_t, &mut info, flags) } != 0 {
        let e = std::io::Error::last_os_error();
        if e.kind() == std::io::ErrorKind::Interrupted { return Ok(false); }
        return Err(format!("wait: {e}"));
    }
    #[cfg(target_os = "linux")]
    let pid = unsafe { info.si_pid() };
    #[cfg(not(target_os = "linux"))]
    let pid = info.si_pid;
    Ok(pid != 0)
}

#[cfg(windows)]
fn has_exited(child: &mut Child) -> Result<bool, String> {
    child.try_wait().map(|s| s.is_some()).map_err(|e| format!("wait: {e}"))
}

fn terminate(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

pub fn output(command: &mut Command, timeout: Duration) -> Result<Output, String> {
    output_with_stdin(command, None, timeout)
}

/// [`output`], with `input` written to the child's stdin, which is then
/// closed: a read past the input sees EOF, as with no input at all.
pub fn output_with_stdin(command: &mut Command, input: Option<Vec<u8>>, timeout: Duration) -> Result<Output, String> {
    output_controlled(command, input, None, timeout, false)
}

/// Keep stdin open while a recorder is active. A stop request writes one line
/// and closes the pipe, allowing the owned helper to flush before the deadline.
pub fn recorder_output(command: &mut Command, stop: std::sync::Arc<std::sync::atomic::AtomicBool>, timeout: Duration) -> Result<Output, String> {
    output_controlled(command, None, Some(stop), timeout, false)
}

/// The fixed audio supervisor owns the recorder's containment. Its stdin must
/// survive until Stop or parent death, so it can flush before its own deadline.
/// Never use this for general commands or model-selected tools.
pub(crate) fn audio_supervisor_output(command: &mut Command, stop: std::sync::Arc<std::sync::atomic::AtomicBool>, timeout: Duration) -> Result<Output, String> {
    output_controlled(command, None, Some(stop), timeout, true)
}

fn output_controlled(command: &mut Command, input: Option<Vec<u8>>, stop: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>, timeout: Duration, audio_supervisor: bool) -> Result<Output, String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let deadline = Instant::now() + timeout;
    let tag = next_tag();
    command.env(TREE_TAG, &tag);
    command.stdin(if input.is_some() || stop.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    // Registration is atomic with spawn, so exit cannot miss a new child.
    let mut registry = live();
    if registry.closed { return Err("the app is shutting down".into()); }
    #[cfg(windows)]
    let (mut child, job) = if audio_supervisor {
        use std::os::windows::process::CommandExt;
        (command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
            .spawn().map_err(|e| format!("spawn audio supervisor: {e}"))?, None)
    } else {
        let (child, job) = windows::spawn(command)?;
        (child, Some(job))
    };
    #[cfg(not(windows))]
    let mut child = command.spawn().map_err(|e| format!("spawn: {e}"))?;
    let id = child.id();
    registry.groups.get_or_insert_with(Default::default).insert(id, tag.clone());
    drop(registry);
    let unregister = |registry: &mut Live| { if let Some(groups) = registry.groups.as_mut() { groups.remove(&id); } };
    let mut recorder_stdin = if stop.is_some() { child.stdin.take() } else { None };
    if audio_supervisor {
        // A bounded handshake prevents starting a recorder from an accidental
        // command-line invocation. No sensor starts if the owner already left.
        if let Some(stdin) = recorder_stdin.as_mut() {
            let _ = std::io::Write::write_all(stdin, b"start\n");
        }
    }
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        // A child that exits or is killed without reading everything closes
        // the pipe, which ends this write with an error; that is not ours to report.
        std::thread::spawn(move || { let _ = std::io::Write::write_all(&mut stdin, &input); });
    }
    let (tx, rx) = mpsc::channel();
    let pipes: [Box<dyn Read + Send>; 2] = [
        Box::new(child.stdout.take().unwrap()), Box::new(child.stderr.take().unwrap()),
    ];
    for (index, pipe) in pipes.into_iter().enumerate() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = pipe.take(OUTPUT_LIMIT + 1).read_to_end(&mut bytes)
                .map_err(|e| format!("read output: {e}"))
                .and_then(|_| if bytes.len() as u64 > OUTPUT_LIMIT {
                    Err("subprocess output exceeds 32 MiB".into())
                } else { Ok(bytes) });
            let _ = tx.send((index, result));
        });
    }
    drop(tx);
    let result = (|| {
        let mut streams: [Option<Vec<u8>>; 2] = [None, None];
        let mut exited = false;
        let mut stopped_at = None;
        loop {
            if stop.as_ref().is_some_and(|stop| stop.load(std::sync::atomic::Ordering::SeqCst)) {
                let since = stopped_at.get_or_insert_with(Instant::now);
                let grace = if audio_supervisor { 8 } else { 6 };
                if since.elapsed() > Duration::from_secs(grace) { return Err("audio helper did not finish after Stop; recording is incomplete".into()); }
                if let Some(mut stdin) = recorder_stdin.take() { let _ = std::io::Write::write_all(&mut stdin, b"stop\n"); }
            }
            while let Ok((index, bytes)) = rx.try_recv() { streams[index] = Some(bytes?); }
            if !exited { exited = has_exited(&mut child)?; }
            // Checked after the leader exits too: descendants may still hold the pipes.
            if live().closed { return Err("the app is shutting down".into()); }
            if exited && streams.iter().all(Option::is_some) {
                // Unregister before reaping: exit must never signal a process
                // group whose leader was reaped and whose ID may be reused.
                unregister(&mut live());
                let status = child.wait().map_err(|e| format!("wait: {e}"))?;
                return Ok(Output { status, stdout: streams[0].take().unwrap(), stderr: streams[1].take().unwrap() });
            }
            let remaining = deadline.checked_duration_since(Instant::now())
                .ok_or("subprocess deadline exceeded; action outcome may be unknown")?;
            std::thread::sleep(remaining.min(Duration::from_millis(10)));
        }
    })();
    if result.is_err() {
        {
            let mut registry = live();
            // The leader is still unreaped here, so its group ID is still ours.
            #[cfg(unix)]
            kill_tree(id, &tag);
            unregister(&mut registry);
        }
        terminate(&mut child);
    }
    #[cfg(windows)]
    if let Some(job) = job { job.finish()?; }
    result
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn recorder_stop_flushes_and_closes_its_stdin() {
        let stop=std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let signal=stop.clone();std::thread::spawn(move||{std::thread::sleep(Duration::from_millis(80));signal.store(true,std::sync::atomic::Ordering::SeqCst);});
        let result=recorder_output(Command::new("/bin/sh").args(["-c","read command; test \"$command\" = stop; echo finalized"]),stop,Duration::from_secs(2)).unwrap();
        assert!(result.status.success());assert_eq!(result.stdout,b"finalized\n");
    }
    #[test]
    fn timeout_reaps_child_even_when_a_descendant_holds_the_pipes() {
        let start = Instant::now();
        let err = output(Command::new("/bin/sh").args(["-c", "sleep 10 & wait"]), Duration::from_millis(100)).unwrap_err();
        assert!(err.contains("deadline"), "{err}");
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("icu-subprocess-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }
    fn pids(path: &std::path::Path) -> Vec<i32> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            if text.ends_with('\n') { return text.split_whitespace().map(|p| p.parse().unwrap()).collect(); }
            assert!(Instant::now() < deadline, "no pid file at {}", path.display());
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn gone(pid: i32) -> bool {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if unsafe { libc::kill(pid, 0) } != 0 { return true; }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }
    /// Re-run one test as a child process: `close()` is global, and the child
    /// exits right after it, the way the app does, so no waiting caller can
    /// reap or kill anything on close's behalf.
    fn in_child(test: &str, pidfile: &std::path::Path) -> Output {
        output(Command::new(std::env::current_exe().unwrap())
            .args([format!("subprocess::tests::{test}").as_str(), "--exact", "--nocapture"])
            .env("ICU_SUBPROCESS_CHILD", pidfile), Duration::from_secs(20)).unwrap()
    }

    #[test]
    fn exit_kills_a_registered_tree_that_ignores_sigterm() {
        if let Some(pidfile) = std::env::var_os("ICU_SUBPROCESS_CHILD") {
            let script = format!("trap '' TERM; sleep 30 & echo $$ $! > '{}'; wait", pidfile.to_str().unwrap());
            std::thread::spawn(move || output(Command::new("/bin/sh").args(["-c", &script]), CALL_TIMEOUT));
            pids(std::path::Path::new(&pidfile));
            close();
            std::process::exit(0);
        }
        let pidfile = scratch("sigterm");
        let child = in_child("exit_kills_a_registered_tree_that_ignores_sigterm", &pidfile);
        assert!(child.status.success(), "{}", String::from_utf8_lossy(&child.stderr));
        for pid in pids(&pidfile) { assert!(gone(pid), "{pid} survived app exit"); }
    }

    #[test]
    fn exit_kills_descendants_after_their_leader_exits() {
        if let Some(pidfile) = std::env::var_os("ICU_SUBPROCESS_CHILD") {
            // The leader exits at once; the background sleep keeps the pipes open.
            let script = format!("sleep 30 & echo $! > '{}'; exit 0", pidfile.to_str().unwrap());
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || tx.send(output(Command::new("/bin/sh").args(["-c", &script]), CALL_TIMEOUT)));
            pids(std::path::Path::new(&pidfile));
            std::thread::sleep(Duration::from_millis(300));
            close();
            // The waiting caller reports shutdown now, not at its 15 s deadline.
            let result = rx.recv_timeout(Duration::from_secs(2)).expect("caller still waiting after close");
            assert_eq!(result.unwrap_err(), "the app is shutting down");
            std::process::exit(0);
        }
        let pidfile = scratch("leader-exit");
        let child = in_child("exit_kills_descendants_after_their_leader_exits", &pidfile);
        assert!(child.status.success(), "{}", String::from_utf8_lossy(&child.stderr));
        for pid in pids(&pidfile) { assert!(gone(pid), "descendant {pid} outlived app exit"); }
    }

    #[test]
    fn timeout_kills_a_descendant_that_left_the_group() {
        let pidfile = scratch("setsid");
        let script = format!("python3 -c 'import os, time; os.setsid(); open(\"{}\", \"w\").write(\"%d\\n\" % os.getpid()); time.sleep(30)' & wait", pidfile.display());
        let err = output(Command::new("/bin/sh").args(["-c", &script]), Duration::from_secs(2)).unwrap_err();
        assert!(err.contains("deadline"), "{err}");
        let pid = pids(&pidfile)[0];
        assert!(gone(pid), "setsid descendant {pid} survived the timeout kill");
    }

    /// A descendant that leaves the group and whose parent exits first is
    /// reparented to init/launchd; only the inherited tree tag still ties it to
    /// the command (round-2 review A9).
    #[test]
    fn timeout_kills_a_setsid_orphan_by_its_tree_tag() {
        let pidfile = scratch("orphan");
        let script = format!("python3 -c 'import os, time; os.setsid(); open(\"{}\", \"w\").write(\"%d\\n\" % os.getpid()); time.sleep(30)' & exit 0", pidfile.display());
        let err = output(Command::new("/bin/sh").args(["-c", &script]), Duration::from_secs(2)).unwrap_err();
        assert!(err.contains("deadline"), "{err}");
        let pid = pids(&pidfile)[0];
        let alive = !gone(pid);
        if alive { unsafe { libc::kill(pid, libc::SIGKILL); } }
        assert!(!alive, "setsid orphan {pid} survived the timeout kill");
    }

    #[test]
    fn the_tree_tag_matches_whole_environment_entries_only() {
        // Not /bin/sh: macOS withholds a platform binary's environment from other
        // processes, so the tag cannot reach those (see `has_env_entry`).
        let pidfile = scratch("tag");
        let script = format!("import os, time; open('{}', 'w').write('%d\\n' % os.getpid()); time.sleep(5)", pidfile.display());
        let mut child = Command::new("python3").args(["-c", &script]).env(TREE_TAG, "7-12").spawn().unwrap();
        let pid = pids(&pidfile)[0];
        let seen = [has_env_entry(pid, "ICU_TREE_TAG=7-12"), has_env_entry(pid, "ICU_TREE_TAG=7-1"), has_env_entry(pid, "ICU_TREE_TAG=7-123")];
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(seen, [true, false, false]);
    }

    /// KERN_PROCARGS2 pads the exec path with NULs and then lists argv. An empty
    /// argv[0] is one more NUL, so a parser that skips padding and then counts
    /// `argc` strings is out by the empty arguments and reads the first
    /// environment entries as arguments. The tag must be found whatever the
    /// arguments are, even when the tag is the first environment entry (the
    /// environment is sorted, and the other names sort after it). The process is
    /// this test binary (an interpreter would rewrite its own argv).
    #[test]
    fn nap() {
        if std::env::var_os("ZZ_NAP").is_some() { std::thread::sleep(Duration::from_secs(5)); }
    }

    #[test]
    fn the_tree_tag_is_found_whatever_the_arguments_are() {
        use std::os::unix::process::CommandExt;
        for args in [vec!["--exact", "subprocess::tests::nap", "--nocapture", "", "x", ""], vec!["--exact", "subprocess::tests::nap", "--nocapture"]] {
            let mut child = Command::new(std::env::current_exe().unwrap()).arg0("").args(&args)
                .env_clear().env("ZZ_NAP", "1").env("Z_ONE", "1").env("Z_TWO", "2").env(TREE_TAG, "7-12").spawn().unwrap();
            let pid = child.id() as i32;
            std::thread::sleep(Duration::from_millis(300));
            let seen = has_env_entry(pid, "ICU_TREE_TAG=7-12");
            let wrong = has_env_entry(pid, "ICU_TREE_TAG=7-1");
            let _ = child.kill();
            let _ = child.wait();
            assert!(seen, "an empty argv[0] hid the environment entry ({args:?})");
            assert!(!wrong, "a prefix of the tag matched ({args:?})");
        }
    }

    /// A helper left running by an earlier app run keeps its tag. Another run
    /// with the same pid that counts to the same command must not own it.
    #[test]
    fn a_tag_is_not_reused_by_another_run_with_the_same_pid_and_count() {
        assert_ne!(command_tag(7, 0xaaaa, 1), command_tag(7, 0xbbbb, 1));
        assert_ne!(command_tag(7, 0xaaaa, 1), command_tag(7, 0xaaaa, 2));
        let (a, b) = (next_tag(), next_tag());
        assert_ne!(a, b);
        let run = format!("-{:x}-", RUN_NONCE.get().unwrap());
        assert!(a.starts_with(&format!("{}-", std::process::id())) && a.contains(&run), "{a}");
    }

    /// X7, the documented rule: a command that finished is unregistered before
    /// its leader is reaped, and what it left running is left running. `icu`
    /// starts the daemon that way (a helper with its output detached, which
    /// outlives the command that started it), and the tag walk runs only for a
    /// command that is still in flight: a timeout or app exit.
    #[test]
    fn a_helper_a_finished_command_left_running_is_not_killed_or_registered() {
        let (helper_file, leader_file) = (scratch("x7"), scratch("x7-leader"));
        let script = format!("echo $$ > '{}'; python3 -c 'import os, time; os.setsid(); open(\"{}\", \"w\").write(\"%d\\n\" % os.getpid()); time.sleep(30)' >/dev/null 2>&1 & while [ ! -s '{}' ]; do sleep 0.05; done; exit 0", leader_file.display(), helper_file.display(), helper_file.display());
        let out = output(Command::new("/bin/sh").args(["-c", &script]), Duration::from_secs(10)).unwrap();
        assert!(out.status.success());
        let (helper, leader) = (pids(&helper_file)[0], pids(&leader_file)[0] as u32);
        // A killed orphan is reaped by launchd within a few ms; one still there
        // after half a second was left alone.
        std::thread::sleep(Duration::from_millis(500));
        let alive = unsafe { libc::kill(helper, 0) } == 0;
        unsafe { libc::kill(helper, libc::SIGKILL); }
        assert!(alive, "the helper of a finished command was killed");
        assert!(!live().groups.iter().flatten().any(|(group, _)| *group == leader), "a finished command stays registered");
    }

    #[test]
    fn drains_both_pipes_and_preserves_exit_status() {
        let out = output(Command::new("/bin/sh").args(["-c", "printf result; printf problem >&2; exit 7"]), Duration::from_secs(2)).unwrap();
        assert_eq!(out.status.code(), Some(7));
        assert_eq!(out.stdout, b"result");
        assert_eq!(out.stderr, b"problem");
    }
}
