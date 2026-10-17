//! Desktop-user KWin identity. Capability equality is a constrained provenance
//! check, not a unique executable measurement; see the W2d audit threat model.
use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::Write,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{
            ffi::OsStrExt,
            fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
            net::UnixStream,
        },
    },
    path::{Component, PathBuf},
    time::{Duration, Instant},
};

fn root_protected(uid: u32, mode: u32, regular: bool) -> bool {
    uid == 0 && mode & 0o022 == 0 && regular
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileIdentity(u64, u64, u32, u32, u32, i64, i64, i64, i64);
fn file_identity(m: &Metadata) -> FileIdentity {
    FileIdentity(
        m.dev(),
        m.ino(),
        m.uid(),
        m.gid(),
        m.mode(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    )
}

struct Installed {
    path: PathBuf,
    file: File,
    identity: FileIdentity,
}
impl Installed {
    fn open(path: PathBuf) -> Option<Self> {
        // /proc/exe supplies the path; never locate the listener through PATH.
        // Resolve directory aliases, then check every component through root.
        if !fs::symlink_metadata(&path).ok()?.is_file() {
            return None;
        }
        let path = fs::canonicalize(path).ok()?;
        let mut current = PathBuf::from("/");
        for part in path.components() {
            if let Component::Normal(part) = part {
                current.push(part);
            }
            let m = fs::symlink_metadata(&current).ok()?;
            if !root_protected(
                m.uid(),
                m.mode(),
                if current == path {
                    m.is_file()
                } else {
                    m.is_dir()
                },
            ) {
                return None;
            }
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)
            .ok()?;
        let identity = file_identity(&file.metadata().ok()?);
        (identity == file_identity(&fs::symlink_metadata(&path).ok()?)).then_some(Self {
            path,
            file,
            identity,
        })
    }
    fn unchanged(&self) -> bool {
        Self::open(self.path.clone()).is_some_and(|f| f.identity == self.identity)
            && self
                .file
                .metadata()
                .is_ok_and(|m| file_identity(&m) == self.identity)
    }
    fn is_executable_of(&self, pid: u32) -> bool {
        let proc = format!("/proc/{pid}/exe");
        fs::read_link(&proc).ok().as_ref() == Some(&self.path)
            && File::open(proc)
                .and_then(|f| f.metadata())
                .is_ok_and(|m| file_identity(&m) == self.identity)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileCaps {
    permitted: u64,
    effective: bool,
}
fn parse_file_caps(raw: &[u8]) -> Option<FileCaps> {
    let word = |offset| -> Option<u32> {
        Some(u32::from_le_bytes(
            raw.get(offset..offset + 4)?.try_into().ok()?,
        ))
    };
    let magic = word(0)?;
    let size = match magic & 0xff00_0000 {
        0x0100_0000 => 12,
        0x0200_0000 => 20,
        0x0300_0000 => 24,
        _ => return None,
    };
    if raw.len() != size || magic & 0x00ff_fffe != 0 {
        return None;
    }
    // Namespaced file grants and inheritable-only grants do not establish this
    // initial-namespace, root-installed provenance chain.
    if word(8)? != 0 || (size >= 20 && word(16)? != 0) || (size == 24 && word(20)? != 0) {
        return None;
    }
    let permitted = u64::from(word(4)?)
        | if size >= 20 {
            u64::from(word(12)?) << 32
        } else {
            0
        };
    Some(FileCaps {
        permitted,
        effective: magic & 1 != 0,
    })
}
fn file_caps(file: &File) -> Option<FileCaps> {
    let mut raw = [0u8; 24];
    let n = unsafe {
        libc::fgetxattr(
            file.as_raw_fd(),
            c"security.capability".as_ptr(),
            raw.as_mut_ptr().cast(),
            raw.len(),
        )
    };
    if n < 0 {
        return (std::io::Error::last_os_error().raw_os_error() == Some(libc::ENODATA)).then_some(
            FileCaps {
                permitted: 0,
                effective: false,
            },
        );
    }
    parse_file_caps(&raw[..n as usize])
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Status {
    uids: Vec<u32>,
    permitted: u64,
    effective: u64,
    ambient: u64,
    inheritable: u64,
}
fn status(pid: u32) -> Option<Status> {
    let text = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let get = |key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(key))
            .map(str::trim)
    };
    let caps = |key| u64::from_str_radix(get(key)?, 16).ok();
    Some(Status {
        uids: get("Uid:")?
            .split_whitespace()
            .map(str::parse)
            .collect::<std::result::Result<_, _>>()
            .ok()?,
        permitted: caps("CapPrm:")?,
        effective: caps("CapEff:")?,
        ambient: caps("CapAmb:")?,
        inheritable: caps("CapInh:")?,
    })
}
fn caps_allowed(
    file: FileCaps,
    process: &Status,
    uid: u32,
    proc_owner: u32,
    proc_group: u32,
    exe_matches: bool,
) -> bool {
    if uid == 0 || process.uids != [uid; 4] || process.ambient != 0 {
        return false;
    }
    if process.permitted != file.permitted {
        return false;
    }
    if process.effective != if file.effective { file.permitted } else { 0 } {
        return false;
    }
    if file.permitted == 0 {
        return exe_matches;
    }
    // proc_pid(5): non-dumpable task files are owned by namespace root (or
    // global root when namespace root is unmapped). The separate UID-map guard
    // is necessary; ownership alone cannot exclude user-namespace capabilities.
    proc_owner == 0 && proc_group == 0
}

fn host_uid_map(raw: &str) -> bool {
    raw.split_whitespace().collect::<Vec<_>>() == ["0", "0", "4294967295"]
}
fn host_capability_scope(pid: u32) -> bool {
    // An ordinary user-created namespace cannot map the entire host UID range,
    // including host root. CapPrm in such a namespace is not host authority.
    fs::read_to_string(format!("/proc/{pid}/uid_map")).is_ok_and(|s| host_uid_map(&s))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Generation {
    parent: u32,
    start: u64,
}
fn generation(pid: u32) -> Option<Generation> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields: Vec<_> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    Some(Generation {
        parent: fields.get(1)?.parse().ok()?,
        start: fields.get(19)?.parse().ok()?,
    })
}
fn stable_generation(before: &Generation, after: &Generation) -> bool {
    before == after
}
fn direct_child(listener: u32, server: u32, parent: u32) -> bool {
    server != listener && parent == listener
}
struct Process {
    pid: u32,
    fd: OwnedFd,
    generation: Generation,
}
impl Process {
    fn pin(pid: u32, fd: Option<OwnedFd>) -> Option<Self> {
        if pid == 0 || pid > i32::MAX as u32 {
            return None;
        }
        let fd = match fd {
            Some(fd) => fd,
            None => {
                let n = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
                if n < 0 {
                    return None;
                }
                unsafe { OwnedFd::from_raw_fd(n as i32) }
            }
        };
        let info = fs::read_to_string(format!("/proc/self/fdinfo/{}", fd.as_raw_fd())).ok()?;
        if info
            .lines()
            .find_map(|l| l.strip_prefix("Pid:"))
            .and_then(|s| s.trim().parse().ok())
            != Some(pid)
        {
            return None;
        }
        let p = Self {
            pid,
            fd,
            generation: generation(pid)?,
        };
        p.unchanged().then_some(p)
    }
    fn unchanged(&self) -> bool {
        let mut poll = libc::pollfd {
            fd: self.fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        (unsafe { libc::poll(&mut poll, 1, 0) == 0 })
            && generation(self.pid).is_some_and(|g| stable_generation(&self.generation, &g))
    }
}

/// Hold both the process generation and the kernel's running executable file.
/// The installed path is checked without following a final symlink; comparing
/// only its name or metadata would admit a replaced executable.
pub(super) struct Executable {
    process: Process,
    installed: Installed,
    running: File,
}
impl Executable {
    pub(super) fn pin(pid: u32, expected: &str) -> Option<Self> {
        let process = Process::pin(pid, None)?;
        let proc = format!("/proc/{pid}/exe");
        let running = File::open(&proc).ok()?;
        let path = fs::read_link(&proc).ok()?;
        if path.file_name()?.to_str()? != expected || !fs::symlink_metadata(&path).ok()?.is_file() {
            return None;
        }
        let installed = Installed::open(path)?;
        let executable = Self {
            process,
            installed,
            running,
        };
        executable.unchanged().then_some(executable)
    }
    pub(super) fn unchanged(&self) -> bool {
        self.process.unchanged()
            && self.installed.unchanged()
            && self
                .running
                .metadata()
                .is_ok_and(|m| file_identity(&m) == self.installed.identity)
            && self.installed.is_executable_of(self.process.pid)
    }
}

fn endpoint_allowed(
    runtime_uid: u32,
    runtime_mode: u32,
    is_dir: bool,
    socket_uid: u32,
    is_socket: bool,
    uid: u32,
) -> bool {
    uid != 0
        && runtime_uid == uid
        && runtime_mode & 0o7777 == 0o700
        && is_dir
        && socket_uid == uid
        && is_socket
}
struct Endpoint {
    runtime: PathBuf,
    directory: File,
    path: PathBuf,
    runtime_identity: FileIdentity,
    socket_identity: FileIdentity,
    uid: u32,
}
impl Endpoint {
    fn open(runtime: PathBuf, display: PathBuf, uid: u32) -> Option<Self> {
        if !runtime.is_absolute() {
            return None;
        }
        let display = if display.is_absolute() {
            display.strip_prefix(&runtime).ok()?.to_owned()
        } else {
            display
        };
        if display.components().count() != 1
            || !matches!(display.components().next()?, Component::Normal(_))
        {
            return None;
        }
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&runtime)
            .ok()?;
        let path = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd())).join(display);
        let r = directory.metadata().ok()?;
        let s = fs::symlink_metadata(&path).ok()?;
        if !endpoint_allowed(
            r.uid(),
            r.mode(),
            r.is_dir(),
            s.uid(),
            s.file_type().is_socket(),
            uid,
        ) {
            return None;
        }
        let e = Self {
            runtime,
            directory,
            path,
            runtime_identity: file_identity(&r),
            socket_identity: file_identity(&s),
            uid,
        };
        e.unchanged().then_some(e)
    }
    fn unchanged(&self) -> bool {
        // Directory timestamps change as unrelated clients create runtime files;
        // only its object identity, ownership and permissions are significant.
        let same_dir = |m: Metadata| {
            let f = file_identity(&m);
            f.0 == self.runtime_identity.0
                && f.1 == self.runtime_identity.1
                && m.uid() == self.uid
                && m.mode() & 0o7777 == 0o700
                && m.is_dir()
        };
        self.directory.metadata().is_ok_and(&same_dir)
            && fs::symlink_metadata(&self.runtime).is_ok_and(same_dir)
            && fs::symlink_metadata(&self.path)
                .is_ok_and(|m| file_identity(&m) == self.socket_identity)
    }
}

fn peer(socket: &UnixStream) -> Option<libc::ucred> {
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of_val(&cred) as libc::socklen_t;
    (unsafe {
        libc::getsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    } == 0
        && len as usize == std::mem::size_of_val(&cred)
        && cred.pid > 0)
        .then_some(cred)
}

fn connect_bounded(path: &std::path::Path) -> Option<UnixStream> {
    let raw = path.as_os_str().as_bytes();
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if raw.is_empty() || raw.contains(&0) || raw.len() >= address.sun_path.len() {
        return None;
    }
    address.sun_family = libc::AF_UNIX as _;
    for (out, byte) in address.sun_path.iter_mut().zip(raw) {
        *out = *byte as _;
    }
    let fd = unsafe {
        libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
            0,
        )
    };
    if fd < 0 {
        return None;
    }
    let socket = unsafe { UnixStream::from_raw_fd(fd) };
    let result = unsafe {
        libc::connect(
            fd,
            (&address as *const libc::sockaddr_un).cast(),
            (std::mem::offset_of!(libc::sockaddr_un, sun_path) + raw.len() + 1) as _,
        )
    };
    if result != 0 {
        if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINPROGRESS) {
            return None;
        }
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLOUT,
            revents: 0,
        };
        if unsafe { libc::poll(&mut poll, 1, 150) } != 1 || poll.revents != libc::POLLOUT {
            return None;
        }
        if socket.take_error().ok()?.is_some() {
            return None;
        }
    }
    socket.set_nonblocking(false).ok()?;
    Some(socket)
}
fn peer_pidfd(socket: &UnixStream) -> Option<Option<OwnedFd>> {
    let mut fd: i32 = -1;
    let mut len = std::mem::size_of_val(&fd) as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            77, /* SO_PEERPIDFD, Linux 6.5 */
            (&mut fd as *mut i32).cast(),
            &mut len,
        )
    };
    if result != 0 {
        return (std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOPROTOOPT))
            .then_some(None);
    }
    if fd < 0 {
        return None;
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    (len as usize == std::mem::size_of::<i32>()).then_some(Some(fd))
}
fn credential_matches(first: (u32, u32, u32), next: (u32, u32, u32)) -> bool {
    first == next
}
const SO_PASSPIDFD: i32 = 76; // Linux 6.5; libc may predate the UAPI.
const SCM_PIDFD: i32 = 4;
fn enable_reply_pidfd(socket: &UnixStream) -> Option<bool> {
    let enabled: i32 = 1;
    if unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            SO_PASSPIDFD,
            (&enabled as *const i32).cast(),
            std::mem::size_of_val(&enabled) as _,
        )
    } == 0
    {
        return Some(true);
    }
    (std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOPROTOOPT)).then_some(false)
}

// Consume the kernel's ancillary buffer before deciding whether to accept it.
// Invalid credentials must not leak descriptors in a later control message.
fn control_messages(control: &[u8], truncated: bool) -> Option<((u32, u32, u32), Option<OwnedFd>)> {
    let mut credentials = None;
    let mut pidfd = None;
    let mut valid = !truncated;
    let base = unsafe { libc::CMSG_LEN(0) as usize };
    let align = std::mem::size_of::<usize>();
    let mut offset = 0;
    while control.len().saturating_sub(offset) >= base {
        let h = unsafe {
            std::ptr::read_unaligned(control.as_ptr().add(offset).cast::<libc::cmsghdr>())
        };
        if h.cmsg_len < base {
            valid = false;
            offset += base; // Still inspect later headers and close their FDs.
            continue;
        }
        if h.cmsg_len > control.len() - offset {
            valid = false;
            break; // No trustworthy next boundary in an overrun header.
        }
        let data = &control[offset + base..offset + h.cmsg_len];
        if h.cmsg_level == libc::SOL_SOCKET
            && h.cmsg_type == libc::SCM_CREDENTIALS
            && data.len() == std::mem::size_of::<libc::ucred>()
        {
            let c = unsafe { std::ptr::read_unaligned(data.as_ptr().cast::<libc::ucred>()) };
            valid &= credentials.is_none() && c.pid > 0;
            credentials = Some((c.pid as u32, c.uid, c.gid));
        } else if h.cmsg_level == libc::SOL_SOCKET
            && matches!(h.cmsg_type, libc::SCM_RIGHTS | SCM_PIDFD)
        {
            let is_pidfd = h.cmsg_type == SCM_PIDFD && data.len() == std::mem::size_of::<i32>();
            valid &= is_pidfd && pidfd.is_none();
            for chunk in data.chunks_exact(std::mem::size_of::<i32>()) {
                let fd = i32::from_ne_bytes(chunk.try_into().unwrap());
                if fd < 0 {
                    valid = false;
                    continue;
                }
                let fd = unsafe { OwnedFd::from_raw_fd(fd) };
                if is_pidfd && pidfd.is_none() {
                    pidfd = Some(fd);
                }
                // All other owned FDs close here, even after a malformed header.
            }
        } else {
            valid = false;
        }
        offset += (h.cmsg_len + align - 1) & !(align - 1);
    }
    valid.then_some((credentials?, pidfd))
}

fn receive(
    socket: &UnixStream,
    bytes: &mut [u8],
) -> Option<(usize, (u32, u32, u32), Option<OwnedFd>)> {
    let mut iov = libc::iovec {
        iov_base: bytes.as_mut_ptr().cast(),
        iov_len: bytes.len(),
    };
    let mut control = [0usize; 64]; // aligned, including space to close unexpected FDs
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = std::mem::size_of_val(&control);
    let n = unsafe { libc::recvmsg(socket.as_raw_fd(), &mut msg, libc::MSG_CMSG_CLOEXEC) };
    if n <= 0 {
        return None;
    }
    let raw = unsafe { std::slice::from_raw_parts(control.as_ptr().cast(), msg.msg_controllen) };
    let (credentials, pidfd) = control_messages(
        raw,
        msg.msg_flags & (libc::MSG_CTRUNC | libc::MSG_TRUNC) != 0,
    )?;
    Some((n as usize, credentials, pidfd))
}
fn sync_reply(raw: &[u8]) -> bool {
    if raw.len() != 24 {
        return false;
    }
    let word = |n| u32::from_ne_bytes(raw[n..n + 4].try_into().unwrap());
    // callback(2).done(serial), followed by display(1).delete_id(2).
    word(0) == 2
        && word(4) == 12 << 16
        && word(12) == 1
        && word(16) == (12 << 16 | 1)
        && word(20) == 2
}
fn sync_server(socket: &mut UnixStream, uid: u32) -> Option<Process> {
    let pidfd_required = enable_reply_pidfd(socket)?;
    let enabled: i32 = 1;
    if unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PASSCRED,
            (&enabled as *const i32).cast(),
            std::mem::size_of_val(&enabled) as _,
        )
    } != 0
    {
        return None;
    }
    let deadline = Instant::now() + Duration::from_millis(150);
    socket
        .set_write_timeout(Some(Duration::from_millis(150)))
        .ok()?;
    let request: Vec<_> = [1u32, 12 << 16, 2]
        .into_iter()
        .flat_map(u32::to_ne_bytes)
        .collect();
    socket.write_all(&request).ok()?;
    let mut response = [0u8; 24];
    let mut used = 0;
    let mut first = None;
    let mut process = None;
    while used < response.len() {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        if remaining.is_zero() {
            return None;
        }
        socket.set_read_timeout(Some(remaining)).ok()?;
        let (n, cred, pidfd) = receive(socket, &mut response[used..])?;
        if cred.1 != uid || (pidfd_required && pidfd.is_none()) {
            return None;
        }
        let sender = Process::pin(cred.0, pidfd)?;
        if let Some(first) = first {
            if !credential_matches(first, cred) {
                return None;
            }
        } else {
            process = Some(sender);
            first = Some(cred);
        }
        used += n;
    }
    if !sync_reply(&response) {
        return None;
    }
    process.filter(Process::unchanged)
}

pub(super) struct Identity {
    endpoint: Endpoint,
    socket: UnixStream,
    listener: Process,
    server: Process,
    wrapper: Installed,
    executable: Installed,
    caps: FileCaps,
}
impl Identity {
    pub(super) fn connect() -> Option<Self> {
        let uid = unsafe { libc::geteuid() };
        if !host_capability_scope(std::process::id()) {
            return None;
        }
        let endpoint = Endpoint::open(
            std::env::var_os("XDG_RUNTIME_DIR")?.into(),
            std::env::var_os("WAYLAND_DISPLAY")?.into(),
            uid,
        )?;
        let mut socket = connect_bounded(&endpoint.path)?;
        let cred = peer(&socket)?;
        if cred.uid != uid {
            return None;
        }
        let listener = Process::pin(cred.pid as u32, peer_pidfd(&socket)?)?;
        let path = fs::read_link(format!("/proc/{}/exe", listener.pid)).ok()?;
        if path.file_name()? != "kwin_wayland_wrapper" {
            return None;
        }
        let wrapper = Installed::open(path)?;
        if !wrapper.is_executable_of(listener.pid) {
            return None;
        }
        // Stock wrapper uses QProcess's PATH lookup. This sibling is the installed
        // candidate whose protected file grants we check, not proof of that lookup.
        let executable = Installed::open(wrapper.path.parent()?.join("kwin_wayland"))?;
        if executable.path.parent() != wrapper.path.parent() {
            return None;
        }
        let caps = file_caps(&executable.file)?;
        let server = sync_server(&mut socket, uid)?;
        let identity = Self {
            endpoint,
            socket,
            listener,
            server,
            wrapper,
            executable,
            caps,
        };
        identity.unchanged().then_some(identity)
    }
    pub(super) fn pid(&self) -> u32 {
        self.server.pid
    }
    pub(super) fn listener_pid(&self) -> u32 {
        self.listener.pid
    }
    pub(super) fn unchanged(&self) -> bool {
        let check = || -> Option<bool> {
            let listener = status(self.listener.pid)?;
            let server = status(self.server.pid)?;
            let proc = fs::symlink_metadata(format!("/proc/{}/stat", self.server.pid)).ok()?;
            let cred = peer(&self.socket)?;
            Some(
                self.endpoint.unchanged()
                    && self.listener.unchanged()
                    && self.server.unchanged()
                    && cred.pid as u32 == self.listener.pid
                    && cred.uid == self.endpoint.uid
                    && direct_child(
                        self.listener.pid,
                        self.server.pid,
                        self.server.generation.parent,
                    )
                    && listener.uids == [self.endpoint.uid; 4]
                    && self.wrapper.unchanged()
                    && self.wrapper.is_executable_of(self.listener.pid)
                    && self.executable.unchanged()
                    && file_caps(&self.executable.file) == Some(self.caps)
                    && caps_allowed(
                        self.caps,
                        &server,
                        self.endpoint.uid,
                        proc.uid(),
                        proc.gid(),
                        self.executable.is_executable_of(self.server.pid),
                    )
                    && self.listener.unchanged()
                    && self.server.unchanged()
                    && host_capability_scope(self.listener.pid)
                    && host_capability_scope(self.server.pid),
            )
        };
        check() == Some(true)
    }
}

pub(super) fn bus_matches(
    owner: &str,
    desktop: &str,
    expected_owner: &str,
    pid: Option<u32>,
    uid: Option<u32>,
    server: u32,
) -> bool {
    owner.starts_with(':')
        && owner == desktop
        && owner == expected_owner
        && pid == Some(server)
        && uid == Some(unsafe { libc::geteuid() })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn executable_pin_rejects_wrong_name_exec_and_exit() {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};
        let mut child = Command::new("/bin/sh")
            .args(["-c", "printf 'ready\\n'; read line; exec /bin/sleep 20"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        // Pin the established fixture, not an unsynchronized child startup.
        let mut ready = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut ready).unwrap();
        assert_eq!(ready, "ready\n");
        let path = fs::read_link(format!("/proc/{}/exe", child.id())).unwrap();
        let name = path.file_name().unwrap().to_str().unwrap();
        let pin = Executable::pin(child.id(), name).unwrap();
        assert!(pin.unchanged());
        assert!(Executable::pin(child.id(), "not-the-compositor").is_none());
        child.stdin.take().unwrap().write_all(b"go\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while pin.unchanged() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(!pin.unchanged(), "exec changed the pinned executable");
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(!pin.unchanged());
    }
    #[test]
    fn installed_executable_refuses_final_symlink() {
        let root = tempfile::tempdir().unwrap();
        let link = root.path().join("sleep");
        std::os::unix::fs::symlink("/usr/bin/sleep", &link).unwrap();
        assert!(Installed::open(link).is_none());
    }
    #[test]
    fn malformed_control_still_closes_later_rights_and_pidfds() {
        use std::io::Read;
        use std::os::fd::IntoRawFd;
        for kind in [libc::SCM_RIGHTS, SCM_PIDFD] {
            let (writer, mut reader) = UnixStream::pair().unwrap();
            reader
                .set_read_timeout(Some(Duration::from_millis(200)))
                .unwrap();
            let fd = writer.into_raw_fd();
            let base = unsafe { libc::CMSG_LEN(0) as usize };
            let space = unsafe { libc::CMSG_SPACE(4) as usize };
            let mut raw = vec![0u8; base + space];
            unsafe {
                std::ptr::write_unaligned(
                    raw.as_mut_ptr().cast::<libc::cmsghdr>(),
                    libc::cmsghdr {
                        cmsg_len: 0,
                        cmsg_level: libc::SOL_SOCKET,
                        cmsg_type: libc::SCM_CREDENTIALS,
                    },
                );
                std::ptr::write_unaligned(
                    raw.as_mut_ptr().add(base).cast::<libc::cmsghdr>(),
                    libc::cmsghdr {
                        cmsg_len: base + 4,
                        cmsg_level: libc::SOL_SOCKET,
                        cmsg_type: kind,
                    },
                );
            }
            raw[base * 2..base * 2 + 4].copy_from_slice(&fd.to_ne_bytes());
            assert!(control_messages(&raw, false).is_none());
            // A closed number can be reused by another parallel test. EOF
            // proves this exact socket description was released, without that race.
            assert_eq!(reader.read(&mut [0; 1]).unwrap(), 0);
        }
    }
    #[test]
    fn kernel_reply_pidfd_binds_to_sender() {
        let (a, mut b) = UnixStream::pair().unwrap();
        if !enable_reply_pidfd(&a).unwrap() {
            return;
        } // Older kernel fallback.
        let enabled: i32 = 1;
        assert_eq!(
            unsafe {
                libc::setsockopt(
                    a.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_PASSCRED,
                    (&enabled as *const i32).cast(),
                    4,
                )
            },
            0
        );
        b.write_all(b"reply").unwrap();
        let (_, cred, fd) = receive(&a, &mut [0; 16]).unwrap();
        assert_eq!(cred.0, std::process::id());
        assert!(fd.is_some(), "supported kernel must supply SCM_PIDFD");
        assert!(Process::pin(cred.0, fd).unwrap().unchanged());
        b.write_all(b"reply").unwrap();
        let (_, cred, fd) = receive(&a, &mut [0; 16]).unwrap();
        assert!(
            Process::pin(cred.0 + 1, fd).is_none(),
            "numeric PID cannot replace sender"
        );
    }
    fn process() -> Status {
        Status {
            uids: vec![1000; 4],
            permitted: 0x800000,
            effective: 0x800000,
            ambient: 0,
            inheritable: 0,
        }
    }
    fn caps() -> FileCaps {
        FileCaps {
            permitted: 0x800000,
            effective: true,
        }
    }
    #[test]
    fn stock_capabilities_admitted() {
        assert!(caps_allowed(caps(), &process(), 1000, 0, 0, false));
    }
    #[test]
    fn namespace_capabilities_are_not_host_authority() {
        assert!(host_uid_map("         0          0 4294967295\n"));
        for map in [
            "",
            "0 1000 1",
            "1000 1000 1",
            "0 0 65536",
            "0 0 4294967296",
            "0 0 4294967295\n1000 1000 1",
        ] {
            assert!(!host_uid_map(map), "{map}");
        }
        assert!(!host_capability_scope(u32::MAX));
    }
    #[test]
    fn different_session_refused() {
        assert!(!super::super::session_allowed(
            Some("a"),
            Some("b"),
            Some(true),
            Some(false),
            Some(false)
        ));
    }
    #[test]
    fn silent_sync_server_refused_within_budget() {
        let (mut a, _b) = UnixStream::pair().unwrap();
        let at = Instant::now();
        assert!(sync_server(&mut a, unsafe { libc::geteuid() }).is_none());
        assert!(at.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn listener_executable_not_root_owned_refused() {
        assert!(!root_protected(1000, 0o100755, true));
    }
    #[test]
    fn writable_executable_or_directory_refused() {
        for mode in [0o100775, 0o100757, 0o40777] {
            assert!(!root_protected(0, mode, true));
        }
        assert!(!root_protected(0, 0o100755, false));
    }
    #[test]
    fn server_not_listener_child_refused() {
        assert!(!direct_child(10, 20, 9));
        assert!(!direct_child(10, 10, 10));
        assert!(direct_child(10, 20, 10));
    }
    #[test]
    fn permitted_mismatch_refused() {
        let mut p = process();
        p.permitted = 0;
        assert!(!caps_allowed(caps(), &p, 1000, 0, 0, false));
    }
    #[test]
    fn effective_mismatch_refused() {
        let mut p = process();
        p.effective = 0;
        assert!(!caps_allowed(caps(), &p, 1000, 0, 0, false));
    }
    #[test]
    fn file_without_caps_process_with_caps_refused() {
        let mut p = process();
        p.effective = 0;
        assert!(!caps_allowed(
            FileCaps {
                permitted: 0,
                effective: false
            },
            &p,
            1000,
            0,
            0,
            true
        ));
    }
    #[test]
    fn cap_free_requires_readable_matching_executable() {
        let mut p = process();
        p.permitted = 0;
        p.effective = 0;
        let f = FileCaps {
            permitted: 0,
            effective: false,
        };
        assert!(caps_allowed(f, &p, 1000, 1000, 1000, true));
        assert!(!caps_allowed(f, &p, 1000, 1000, 1000, false));
    }
    #[test]
    fn dumpable_or_user_namespace_root_refused() {
        assert!(!caps_allowed(caps(), &process(), 1000, 1000, 0, false));
        assert!(!caps_allowed(caps(), &process(), 1000, 1000, 1000, false));
        assert!(!caps_allowed(caps(), &process(), 1000, 0, 1000, false));
    }
    #[test]
    fn ambient_capabilities_refused() {
        let mut p = process();
        p.ambient = 0x800000;
        assert!(!caps_allowed(caps(), &p, 1000, 0, 0, false));
    }
    #[test]
    fn wrong_process_uid_refused() {
        let mut p = process();
        p.uids[0] = 0;
        assert!(!caps_allowed(caps(), &p, 1000, 0, 0, false));
    }
    #[test]
    fn root_desktop_refused() {
        let mut p = process();
        p.uids = vec![0; 4];
        assert!(!caps_allowed(caps(), &p, 0, 0, 0, false));
    }
    #[test]
    fn start_time_change_refused() {
        let a = Generation {
            parent: 10,
            start: 12,
        };
        let b = Generation {
            parent: 10,
            start: 13,
        };
        assert!(stable_generation(&a, &a));
        assert!(!stable_generation(&a, &b));
    }
    #[test]
    fn reparenting_refused() {
        assert!(!stable_generation(
            &Generation {
                parent: 10,
                start: 12
            },
            &Generation {
                parent: 11,
                start: 12
            }
        ));
    }
    #[test]
    fn exited_pidfd_refused() {
        let mut child = std::process::Command::new("sleep")
            .arg("20")
            .spawn()
            .unwrap();
        let pinned = Process::pin(child.id(), None).unwrap();
        assert!(pinned.unchanged());
        child.kill().unwrap();
        // Keep the zombie's /proc generation readable, isolating pidfd liveness
        // from the separate missing-process check.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe {
                libc::waitid(
                    libc::P_PID,
                    child.id(),
                    &mut info,
                    libc::WEXITED | libc::WNOWAIT,
                )
            },
            0
        );
        assert!(!pinned.unchanged());
        child.wait().unwrap();
    }
    #[test]
    fn wrong_socket_owner_refused() {
        assert!(!endpoint_allowed(1000, 0o40700, true, 1001, true, 1000));
    }
    #[test]
    fn bad_runtime_refused() {
        assert!(!endpoint_allowed(1001, 0o40700, true, 1000, true, 1000));
        assert!(!endpoint_allowed(1000, 0o40750, true, 1000, true, 1000));
        assert!(!endpoint_allowed(1000, 0o40700, false, 1000, true, 1000));
        assert!(!endpoint_allowed(1000, 0o40700, true, 1000, false, 1000));
    }
    #[test]
    fn missing_socket_and_traversal_refused() {
        let d = tempfile::tempdir().unwrap();
        let uid = unsafe { libc::geteuid() };
        assert!(Endpoint::open(d.path().into(), "missing".into(), uid).is_none());
        assert!(Endpoint::open(d.path().into(), "../wayland-0".into(), uid).is_none());
        assert!(Endpoint::open(d.path().into(), "/tmp/wayland-0".into(), uid).is_none());
    }
    #[test]
    fn socket_replacement_refused() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        fs::set_permissions(d.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let path = d.path().join("wayland-0");
        let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let e = Endpoint::open(d.path().into(), "wayland-0".into(), unsafe {
            libc::geteuid()
        })
        .unwrap();
        assert!(e.unchanged());
        fs::remove_file(&path).unwrap();
        let _replacement = std::os::unix::net::UnixListener::bind(&path).unwrap();
        assert!(!e.unchanged());
    }
    #[test]
    fn owner_change_mid_check_refused() {
        let uid = Some(unsafe { libc::geteuid() });
        assert!(bus_matches(":1.2", ":1.2", ":1.2", Some(10), uid, 10));
        assert!(!bus_matches(":1.3", ":1.3", ":1.2", Some(10), uid, 10));
        assert!(!bus_matches(":1.2", ":1.3", ":1.2", Some(10), uid, 10));
    }
    #[test]
    fn bus_pid_uid_and_unknown_credentials_refused() {
        let uid = Some(unsafe { libc::geteuid() });
        assert!(!bus_matches(":1.2", ":1.2", ":1.2", Some(11), uid, 10));
        assert!(!bus_matches(":1.2", ":1.2", ":1.2", None, uid, 10));
        assert!(!bus_matches(":1.2", ":1.2", ":1.2", Some(10), None, 10));
        assert!(!bus_matches(
            ":1.2",
            ":1.2",
            ":1.2",
            Some(10),
            Some(uid.unwrap() + 1),
            10
        ));
    }
    #[test]
    fn multi_segment_credentials_must_all_match() {
        let first = (10, 1000, 1000);
        for bad in [(11, 1000, 1000), (10, 1001, 1000), (10, 1000, 1001)] {
            assert!(!credential_matches(first, bad));
        }
        assert!(credential_matches(first, first));
    }
    #[test]
    fn missing_segment_credentials_refused() {
        let (a, mut b) = UnixStream::pair().unwrap();
        b.write_all(&[0; 12]).unwrap();
        assert!(receive(&a, &mut [0; 24]).is_none());
    }
    #[test]
    fn fragmented_sync_reply_admitted() {
        let (mut a, mut b) = UnixStream::pair().unwrap();
        let server = std::thread::spawn(move || {
            use std::io::Read;
            let mut request = [0; 12];
            b.read_exact(&mut request).unwrap();
            let response: Vec<_> = [2u32, 12 << 16, 42, 1, 12 << 16 | 1, 2]
                .into_iter()
                .flat_map(u32::to_ne_bytes)
                .collect();
            for chunk in response.chunks(3) {
                b.write_all(chunk).unwrap();
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        assert_eq!(
            sync_server(&mut a, unsafe { libc::geteuid() }).unwrap().pid,
            std::process::id()
        );
        server.join().unwrap();
    }
    #[test]
    fn kernel_credentials_change_between_reply_segments_refused() {
        let (mut a, mut b) = UnixStream::pair().unwrap();
        let sender = std::thread::spawn(move || {
            use std::io::Read;
            let mut request = [0; 12];
            b.read_exact(&mut request).unwrap();
            let reply: Vec<_> = [2u32, 12 << 16, 42, 1, 12 << 16 | 1, 2]
                .into_iter()
                .flat_map(u32::to_ne_bytes)
                .collect();
            b.write_all(&reply[..12]).unwrap();
            // The child uses only async-signal-safe syscalls after fork. It
            // sends the second half with its own kernel credentials.
            let child = unsafe { libc::fork() };
            assert!(child >= 0);
            if child == 0 {
                unsafe {
                    let sent = libc::write(b.as_raw_fd(), reply.as_ptr().add(12).cast(), 12);
                    libc::_exit(if sent == 12 { 0 } else { 1 });
                }
            }
            let mut result = 0;
            assert_eq!(unsafe { libc::waitpid(child, &mut result, 0) }, child);
            assert_eq!(result, 0);
        });
        assert!(sync_server(&mut a, unsafe { libc::geteuid() }).is_none());
        sender.join().unwrap();
    }
    #[test]
    fn malformed_wayland_reply_refused() {
        let good: Vec<_> = [2u32, 12 << 16, 42, 1, 12 << 16 | 1, 2]
            .into_iter()
            .flat_map(u32::to_ne_bytes)
            .collect();
        assert!(sync_reply(&good));
        for position in [0, 4, 12, 16, 20] {
            let mut bad = good.clone();
            bad[position] ^= 1;
            assert!(!sync_reply(&bad));
        }
        assert!(!sync_reply(&good[..12]));
    }
    #[test]
    fn unknown_capability_format_refused() {
        let raw: Vec<_> = [0x02000001u32, 0x800000, 0, 0, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        assert_eq!(parse_file_caps(&raw), Some(caps()));
        for offset in [2, 3, 8, 16] {
            let mut bad = raw.clone();
            bad[offset] = 0xff;
            assert!(parse_file_caps(&bad).is_none());
        }
        assert!(parse_file_caps(&raw[..16]).is_none());
        let namespaced: Vec<_> = [0x03000001u32, 0x800000, 0, 0, 0, 1000]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        assert!(parse_file_caps(&namespaced).is_none());
    }
}
