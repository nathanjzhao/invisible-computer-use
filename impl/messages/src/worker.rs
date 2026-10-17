//! One process per database query. A malformed archive may abort its worker, never the host.
//! Resource limits are containment against malformed data, not an OS permissions sandbox.
use crate::{
    Error, Result, activity,
    cursor::Page,
    database::{Database, Filter},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const MAX_REQUEST: usize = 1024 * 1024;
const MAX_RESPONSE: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Query {
    Destinations {
        groups: Vec<Vec<String>>,
    },
    StageFiles {
        paths: Vec<PathBuf>,
        directory: PathBuf,
    },
    ValidateFiles {
        files: Vec<crate::send::FileSnapshot>,
    },
    Image {
        chat_guid: String,
        message_guid: String,
        attachment_id: String,
    },
    Describe {
        chat_guids: Vec<String>,
    },
    Find {
        filter: Filter,
        name: Option<String>,
        limit: usize,
    },
    Read {
        filter: Filter,
        page: Option<Page>,
        limit: usize,
    },
    Count {
        query: activity::Query,
        page: Option<Page>,
        limit: usize,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    database: PathBuf,
    budget_ms: u64,
    query: Query,
}

/// Private worker metadata. The tool response must never serialize this structure.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttachmentLocator {
    pub chat_guid: String,
    pub id: String,
    pub message_guid: String,
    pub path: Option<PathBuf>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub data: Value,
    pub next_page: Option<Page>,
    pub attachments: Vec<AttachmentLocator>,
    /// Resolved metadata from the same SQLite snapshot as the candidate message page.
    pub chats: Vec<crate::database::Chat>,
}

#[derive(Clone, Debug)]
pub struct Limits {
    pub wall: Duration,
    pub memory_bytes: u64,
    pub output_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            wall: Duration::from_secs(8),
            memory_bytes: 512 * 1024 * 1024,
            output_bytes: MAX_RESPONSE,
        }
    }
}

#[derive(Clone)]
pub struct Worker {
    executable: PathBuf,
    database: PathBuf,
    pub limits: Limits,
}
impl Worker {
    pub fn new(executable: PathBuf, database: PathBuf) -> Result<Self> {
        if !executable.is_absolute() || !database.is_absolute() {
            return Err(crate::invalid("Worker and database paths must be absolute"));
        }
        Ok(Self {
            executable,
            database,
            limits: Limits::default(),
        })
    }
    /// Internal host query. Never expose its unfiltered response as a tool result.
    /// No ambient DB is selected; the host gates content release after resolving chats.
    pub fn query(&self, query: Query, cancel: Arc<AtomicBool>) -> Result<Response> {
        let request = Request {
            database: self.database.clone(),
            budget_ms: self.limits.wall.as_millis().min(u64::MAX as u128) as u64,
            query,
        };
        let input = serde_json::to_vec(&request)
            .map_err(|_| crate::invalid("Could not encode Messages query"))?;
        if input.len() > MAX_REQUEST {
            return Err(crate::invalid("Messages query is too large"));
        }
        let mut command = Command::new(&self.executable);
        command.arg("--query-worker").env_clear();
        let output = supervise(command, &input, &self.limits, cancel)?;
        serde_json::from_slice::<Result<Response>>(&output).map_err(|_| {
            Error::new(
                "messagesWorkerFailed",
                "Messages worker returned an invalid response",
            )
        })?
    }
}

fn perform(request: Request) -> Result<Response> {
    match &request.query {
        Query::StageFiles { paths, directory } => {
            return Ok(Response {
                data: serde_json::to_value(crate::send::stage_files(paths, directory)?)
                    .map_err(|_| crate::invalid("Could not encode staged files"))?,
                next_page: None,
                attachments: vec![],
                chats: vec![],
            });
        }
        Query::ValidateFiles { files } => {
            crate::send::validate_files(files)?;
            return Ok(Response {
                data: serde_json::json!({}),
                next_page: None,
                attachments: vec![],
                chats: vec![],
            });
        }
        _ => {}
    }
    let db = Database::open(
        &request.database,
        Duration::from_millis(request.budget_ms.min(30_000)),
        Arc::new(AtomicBool::new(false)),
    )?;
    let mut next_page = None;
    let mut attachments = Vec::new();
    let mut chats = Vec::new();
    let data = match request.query {
        Query::Destinations { groups } => {
            chats = db.destination_candidates(&groups)?;
            Ok(serde_json::json!({}))
        }
        Query::StageFiles { .. } | Query::ValidateFiles { .. } => unreachable!(),
        Query::Image {
            chat_guid,
            message_guid,
            attachment_id,
        } => {
            let path = db.image_attachment_path(&chat_guid, &message_guid, &attachment_id)?;
            serde_json::to_value(crate::attachment_image::load(&request.database, &path)?)
        }
        Query::Describe { chat_guids } => {
            let filter = Filter {
                chat_guids,
                ..Default::default()
            }
            .normalize()?;
            for guid in filter.chat_guids {
                chats.push(db.chat_by_guid(&guid)?);
            }
            Ok(serde_json::json!({}))
        }
        Query::Find {
            filter,
            name,
            limit,
        } => serde_json::to_value(db.find_chats(&filter, name.as_deref(), limit)?),
        Query::Read {
            filter,
            page,
            limit,
        } => {
            let page = page.map(Ok).unwrap_or_else(|| db.first_page())?;
            let (messages, next) = db.read_batch(&filter, &page, limit)?;
            next_page = next;
            for message in &messages {
                if !chats
                    .iter()
                    .any(|c: &crate::database::Chat| c.chat_guid == message.chat_guid)
                {
                    chats.push(db.chat_by_guid(&message.chat_guid)?);
                }
                for a in &message.attachments {
                    attachments.push(AttachmentLocator {
                        chat_guid: message.chat_guid.clone(),
                        id: a.id.clone(),
                        message_guid: message.guid.clone(),
                        path: a.path.clone(),
                    });
                }
            }
            serde_json::to_value(crate::database::MessagePage {
                messages,
                next_cursor: None,
            })
        }
        Query::Count { query, page, limit } => {
            let page = page.map(Ok).unwrap_or_else(|| db.first_page())?;
            let (report, next) = query.evaluate(&db, page, limit)?;
            next_page = next;
            serde_json::to_value(report)
        }
    }
    .map_err(|_| Error::new("messagesWorkerFailed", "Could not encode query response"))?;
    Ok(Response {
        data,
        next_page,
        attachments,
        chats,
    })
}

/// Private subprocess protocol. Not a model-callable entry point or an approval source.
pub fn entry() -> i32 {
    let response = (|| -> Result<Response> {
        let mut input = Vec::new();
        std::io::stdin()
            .take((MAX_REQUEST + 1) as u64)
            .read_to_end(&mut input)
            .map_err(|_| crate::invalid("Could not read query request"))?;
        if input.len() > MAX_REQUEST {
            return Err(crate::invalid("Messages query is too large"));
        }
        let request: Request = serde_json::from_slice(&input)
            .map_err(|_| crate::invalid("Malformed Messages worker request"))?;
        perform(request)
    })();
    let output = serde_json::to_vec(&response).unwrap_or_default();
    if output.len() > MAX_RESPONSE {
        let error: Result<Response> = Err(Error::new(
            "messagesResponseTooLarge",
            "Messages response exceeds its size limit; request fewer messages or larger time buckets",
        ));
        return if serde_json::to_writer(std::io::stdout(), &error).is_ok() {
            0
        } else {
            1
        };
    }
    if std::io::stdout().write_all(&output).is_ok() {
        0
    } else {
        1
    }
}

struct OwnedChild(Option<Child>);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(unix)]
fn nonblocking(pipe: &impl std::os::fd::AsRawFd) -> Result<()> {
    let fd = pipe.as_raw_fd();
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags == -1 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) == -1 {
            return Err(Error::new(
                "messagesWorkerFailed",
                "Could not configure worker pipes",
            ));
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn resident_bytes(pid: u32) -> Option<u64> {
    unsafe {
        let mut info: libc::proc_taskinfo = std::mem::zeroed();
        let size = std::mem::size_of_val(&info) as i32;
        let read = libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTASKINFO,
            0,
            (&mut info as *mut libc::proc_taskinfo).cast(),
            size,
        );
        (read == size).then_some(info.pti_resident_size)
    }
}
#[cfg(target_os = "linux")]
fn resident_bytes(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse::<u64>().ok())
        .and_then(|n| n.checked_mul(1024))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn supervise(
    mut command: Command,
    input: &[u8],
    limits: &Limits,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<u8>> {
    use std::os::unix::process::CommandExt;
    if cancel.load(Ordering::SeqCst) {
        return Err(Error::new(
            "messagesRequestCanceled",
            "Messages request was canceled",
        ));
    }
    if limits.wall.is_zero()
        || limits.wall > Duration::from_secs(180)
        || limits.output_bytes == 0
        || limits.output_bytes > MAX_RESPONSE
        || limits.memory_bytes < 1024 * 1024
    {
        return Err(crate::invalid("Invalid query worker limits"));
    }
    let cpu_seconds = limits.wall.as_secs().saturating_add(1);
    // Only async-signal-safe calls are allowed after fork and before exec.
    unsafe {
        command.pre_exec(move || {
            if libc::setpgid(0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            for (resource, value) in [
                (libc::RLIMIT_CORE, 0),
                (libc::RLIMIT_CPU, cpu_seconds),
                (libc::RLIMIT_NOFILE, 64),
            ] {
                let bound = libc::rlimit {
                    rlim_cur: value as libc::rlim_t,
                    rlim_max: value as libc::rlim_t,
                };
                if libc::setrlimit(resource, &bound) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let child = command.spawn().map_err(|_| {
        Error::new(
            "messagesWorkerFailed",
            "Could not launch Messages query worker",
        )
    })?;
    let mut owner = OwnedChild(Some(child));
    let child = owner.0.as_mut().unwrap();
    let mut writer = child.stdin.take();
    let mut reader = child.stdout.take().unwrap();
    nonblocking(writer.as_ref().unwrap())?;
    nonblocking(&reader)?;
    let start = Instant::now();
    let mut written = 0;
    let mut output = Vec::new();
    let mut scratch = [0u8; 8192];
    let mut memory_misses = 0;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(Error::new(
                "messagesRequestCanceled",
                "Messages request was canceled",
            ));
        }
        if start.elapsed() >= limits.wall {
            return Err(Error::new(
                "messagesQueryTimedOut",
                "Messages query worker exceeded its deadline",
            ));
        }
        let status = child
            .try_wait()
            .map_err(|_| Error::new("messagesWorkerFailed", "Could not inspect query worker"))?;
        if status.is_none() {
            if let Some(bytes) = resident_bytes(child.id()) {
                memory_misses = 0;
                if bytes > limits.memory_bytes {
                    return Err(Error::new(
                        "messagesMemoryLimit",
                        "Messages query worker exceeded its memory limit",
                    ));
                }
            } else {
                memory_misses += 1;
                if memory_misses > 10 {
                    return Err(Error::new(
                        "messagesWorkerFailed",
                        "Could not monitor query worker memory",
                    ));
                }
            }
        }
        if let Some(pipe) = writer.as_mut() {
            match pipe.write(&input[written..]) {
                Ok(n) => {
                    written += n;
                    if written == input.len() {
                        writer = None;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => {
                    writer = None;
                }
            }
        }
        let mut eof = false;
        // Bound each drain so a streaming child cannot starve deadline/cancel checks.
        for _ in 0..32 {
            match reader.read(&mut scratch) {
                Ok(0) => {
                    eof = true;
                    break;
                }
                Ok(n) => {
                    if output.len() + n > limits.output_bytes {
                        return Err(Error::new(
                            "messagesResponseTooLarge",
                            "Messages worker response exceeds its size limit",
                        ));
                    }
                    output.extend_from_slice(&scratch[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    return Err(Error::new(
                        "messagesWorkerFailed",
                        "Could not read query worker response",
                    ));
                }
            }
        }
        if let Some(status) = status {
            if !status.success() {
                return Err(Error::new(
                    "messagesWorkerFailed",
                    format!("Messages query worker exited before completing the query ({status})"),
                ));
            }
            if eof {
                owner.0 = None;
                return Ok(output);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(crate) fn supervise(_: Command, _: &[u8], _: &Limits, _: Arc<AtomicBool>) -> Result<Vec<u8>> {
    Err(Error::new(
        "messagesPlatformUnsupported",
        "Messages query workers require macOS or Linux",
    ))
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;
    fn python(script: &str) -> Command {
        let mut command = Command::new("python3");
        command.args(["-c", script]);
        command
    }
    #[test]
    fn supervisor_bounds_output_memory_time_and_abnormal_exit() {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut limits = Limits {
            wall: Duration::from_secs(3),
            ..Default::default()
        };
        assert_eq!(
            supervise(
                python("import sys; sys.stdout.buffer.write(sys.stdin.buffer.read())"),
                b"fixture",
                &limits,
                cancel.clone()
            )
            .unwrap(),
            b"fixture"
        );
        limits.output_bytes = 1024;
        assert_eq!(
            supervise(python("print('x'*2000)"), b"", &limits, cancel.clone())
                .unwrap_err()
                .code,
            "messagesResponseTooLarge"
        );
        limits.output_bytes = MAX_RESPONSE;
        limits.memory_bytes = 64 * 1024 * 1024;
        assert_eq!(
            supervise(
                python("import time; data=bytearray(150*1024*1024); time.sleep(10)"),
                b"",
                &limits,
                cancel.clone()
            )
            .unwrap_err()
            .code,
            "messagesMemoryLimit"
        );
        limits.wall = Duration::from_millis(150);
        assert_eq!(
            supervise(
                python("import time; time.sleep(10)"),
                b"",
                &limits,
                cancel.clone()
            )
            .unwrap_err()
            .code,
            "messagesQueryTimedOut"
        );
        limits.wall = Duration::from_secs(3);
        assert_eq!(
            supervise(
                python("import os, signal; os.kill(os.getpid(), signal.SIGKILL)"),
                b"",
                &limits,
                cancel
            )
            .unwrap_err()
            .code,
            "messagesWorkerFailed"
        );
    }
    #[test]
    fn cancellation_kills_and_reaps_the_worker() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pid");
        let script = format!(
            "import os,pathlib,time; pathlib.Path({:?}).write_text(str(os.getpid())); time.sleep(30)",
            path
        );
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let ready = path.clone();
        let thread = std::thread::spawn(move || {
            let start = Instant::now();
            while !ready.exists() && start.elapsed() < Duration::from_secs(2) {
                std::thread::sleep(Duration::from_millis(5));
            }
            signal.store(true, Ordering::SeqCst);
        });
        let result = supervise(python(&script), b"", &Limits::default(), cancel);
        thread.join().unwrap();
        assert_eq!(result.unwrap_err().code, "messagesRequestCanceled");
        let pid: i32 = std::fs::read_to_string(path).unwrap().parse().unwrap();
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    }
}
