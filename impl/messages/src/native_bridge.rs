//! Bounded LaunchServices broker shared by native helpers. Ordinary calls do not activate UI.
use crate::{
    Error, Result,
    worker::{Limits, supervise},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
#[derive(Deserialize)]
struct NativeReply {
    ok: bool,
    data: Option<Value>,
    code: Option<String>,
    message: Option<String>,
}
pub fn invoke(
    executable: &Path,
    request: Value,
    cancel: Arc<AtomicBool>,
    wall: Duration,
) -> Result<Value> {
    #[cfg(target_os = "macos")]
    if let Some(bundle) = executable
        .ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
    {
        return invoke_gui(executable, bundle, request, cancel, wall);
    }
    let input = serde_json::to_vec(&request).map_err(|_| unavailable())?;
    if input.len() > 1024 * 1024 {
        return Err(crate::invalid("Native helper request is too large"));
    }
    let mut command = Command::new(executable);
    command.env_clear();
    let output = supervise(
        command,
        &input,
        &Limits {
            wall,
            output_bytes: 2 * 1024 * 1024,
            ..Limits::default()
        },
        cancel,
    )?;
    let reply: NativeReply = serde_json::from_slice(&output).map_err(|_| unavailable())?;
    if reply.ok {
        reply.data.ok_or_else(unavailable)
    } else {
        Err(Error {
            code: reply
                .code
                .unwrap_or_else(|| "messagesNativeHelperUnavailable".into()),
            message: reply
                .message
                .unwrap_or_else(|| "Native helper request failed".into()),
        })
    }
}

#[cfg(target_os = "macos")]
fn invoke_gui(
    executable: &Path,
    bundle: &Path,
    mut request: Value,
    cancel: Arc<AtomicBool>,
    wall: Duration,
) -> Result<Value> {
    use std::{
        fs::OpenOptions,
        io::{Read, Write},
        os::unix::fs::OpenOptionsExt,
        process::{Child, Stdio},
        sync::atomic::Ordering,
        time::Instant,
    };
    if !executable.is_file() || cancel.load(Ordering::SeqCst) {
        return Err(if cancel.load(Ordering::SeqCst) {
            Error::new(
                "messagesRequestCanceled",
                "Native helper request was canceled",
            )
        } else {
            unavailable()
        });
    }
    let dir = tempfile::Builder::new()
        .prefix("icu-native-")
        .tempdir()
        .map_err(|_| unavailable())?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
            .map_err(|_| unavailable())?;
    }
    let input = dir.path().join("input.json");
    let output = dir.path().join("output.json");
    let lifetime = dir.path().join("alive");
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).map_err(|_| unavailable())?;
    let nonce = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    request["nonce"] = json!(nonce);
    request["lifetime"] = json!(lifetime);
    request["parent_pid"] = json!(std::process::id());
    unsafe {
        let mut info: libc::proc_bsdinfo = std::mem::zeroed();
        let size = std::mem::size_of_val(&info) as i32;
        let copied = libc::proc_pidinfo(
            std::process::id() as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        );
        if copied != size {
            return Err(unavailable());
        }
        request["parent_started"] = json!([info.pbi_start_tvsec, info.pbi_start_tvusec]);
    }
    for (path, data) in [
        (
            &input,
            serde_json::to_vec(&request).map_err(|_| unavailable())?,
        ),
        (&output, vec![]),
        (&lifetime, vec![]),
    ] {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .and_then(|mut f| f.write_all(&data))
            .map_err(|_| unavailable())?;
    }
    struct Running {
        child: Child,
        lifetime: PathBuf,
    }
    impl Drop for Running {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.lifetime);
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
    // LaunchServices provides the helper's app identity for OS consent. An SSH parent
    // otherwise changes permission attribution, even after the app itself has been approved.
    let child = Command::new("/usr/bin/open")
        .args(["-W", "-n", "-g", "--stdin"])
        .arg(&input)
        .arg("--stdout")
        .arg(&output)
        .args(["--stderr", "/dev/null"])
        .arg(bundle)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| unavailable())?;
    let mut running = Running { child, lifetime };
    let started = Instant::now();
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(Error::new(
                "messagesRequestCanceled",
                "Native helper request was canceled",
            ));
        }
        if started.elapsed() >= wall {
            return Err(Error::new(
                "messagesQueryTimedOut",
                "The native helper query exceeded its deadline",
            ));
        }
        if running
            .child
            .try_wait()
            .map_err(|_| unavailable())?
            .is_some()
        {
            // LaunchServices may return ESRCH from -W when a short-lived helper has
            // already exited. Only the nonce-bound complete reply proves success.
            let mut data = Vec::new();
            std::fs::File::open(&output)
                .map_err(|_| unavailable())?
                .take(2 * 1024 * 1024 + 4097)
                .read_to_end(&mut data)
                .map_err(|_| unavailable())?;
            if data.len() > 2 * 1024 * 1024 + 4096 {
                return Err(unavailable());
            }
            let text = std::str::from_utf8(&data).map_err(|_| unavailable())?;
            let mut lines = text.lines();
            let header: Value = serde_json::from_str(lines.next().ok_or_else(unavailable)?)
                .map_err(|_| unavailable())?;
            if header["event"] != "started" || header["nonce"] != nonce {
                return Err(unavailable());
            }
            let reply: NativeReply = serde_json::from_str(lines.next().ok_or_else(unavailable)?)
                .map_err(|_| unavailable())?;
            if lines.next().is_some() {
                return Err(unavailable());
            }
            if reply.ok {
                return reply.data.ok_or_else(unavailable);
            }
            return Err(Error {
                code: reply
                    .code
                    .unwrap_or_else(|| "messagesNativeHelperUnavailable".into()),
                message: reply
                    .message
                    .unwrap_or_else(|| "Native helper request failed".into()),
            });
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn unavailable() -> Error {
    Error::new(
        "messagesNativeHelperUnavailable",
        "The native helper is unavailable or returned an incomplete response",
    )
}
