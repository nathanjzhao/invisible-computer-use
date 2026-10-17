//! The Windows half of `icu sky-pipe`: the same length-prefixed protocol (`skypipe`) over a named
//! pipe instead of a unix socket.
//!
//! The names and the access list are plain functions so they are tested on every platform; the
//! pipe itself (`bind_pipe`, `serve_pipe`) is Windows only.
//!
//! What the pipe guarantees, and how:
//! - Only this user (and SYSTEM) can open it: the DACL is `D:P(A;;GA;;;<user sid>)(A;;GA;;;SY)`.
//!   The default DACL of a pipe also lets Everyone read it.
//! - No remote client: `PIPE_REJECT_REMOTE_CLIENTS`.
//! - The name is not shared with whoever got there first: the first instance is made with
//!   `FILE_FLAG_FIRST_PIPE_INSTANCE`, so a pipe that already exists under the name (another
//!   `icu sky-pipe`, or a process squatting on it) is `AddrInUse` and we serve nothing.
//! - The next instance is listening before a connection is handed to its thread, so a second
//!   client never meets `ERROR_PIPE_BUSY`.

/// The pipe a Windows `icu sky-pipe` serves when nothing names one: per user, because the pipe
/// namespace is machine-wide.
pub fn default_pipe_name(user: &str) -> String {
    let safe: String = user.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).collect();
    format!(r"\\.\pipe\icu-computeruse-{safe}")
}

/// `--socket` or `SKY_CUA_SERVICE_NATIVE_PIPE_PATH` as a pipe path: a full `\\.\pipe\...` path is
/// used as it is, a bare name gets the prefix. A name with a separator in it is refused (a file
/// path is not a pipe, and `\\.\pipe\a\b` is not a name Windows accepts here).
pub fn pipe_path(raw: &str) -> Result<String, String> {
    let prefix = r"\\.\pipe\";
    let name = if raw.len() >= prefix.len() && raw.is_char_boundary(prefix.len()) && raw[..prefix.len()].eq_ignore_ascii_case(prefix) { &raw[prefix.len()..] } else { raw };
    if name.is_empty() || name.len() > 200 || name.contains(['\\', '/', ':', '\0']) {
        return Err(format!("{raw:?} is not a pipe name: use a bare name or {prefix}<name>"));
    }
    Ok(format!("{prefix}{name}"))
}

/// The DACL of the pipe: full access for the user and for SYSTEM, nothing for anyone else (an
/// administrator who is not this user included).
pub fn pipe_sddl(user_sid: &str) -> String { format!("D:P(A;;GA;;;{user_sid})(A;;GA;;;SY)") }

#[cfg(windows)]
pub use imp::{bind_pipe, serve_pipe, user_name, PipeInstance};

#[cfg(windows)]
mod imp {
    use super::*;
    use crate::skypipe::{serve_connection, SkyPipe};
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
    use windows_sys::Win32::Security::{GetTokenInformation, TokenUser, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER};
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
    use windows_sys::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// One listening instance of the pipe; `accept` blocks until a client connects.
    pub struct PipeInstance(std::fs::File);

    fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() }

    pub fn user_name() -> String { std::env::var("USERNAME").unwrap_or_default() }

    /// This process's user as an SID string (`S-1-5-21-...`).
    fn current_user_sid() -> std::io::Result<String> {
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 { return Err(std::io::Error::last_os_error()); }
            let mut need = 0u32;
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut need);
            let mut buf = vec![0u8; need as usize];
            let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), need, &mut need);
            let err = std::io::Error::last_os_error();
            CloseHandle(token);
            if ok == 0 { return Err(err); }
            let user = &*(buf.as_ptr() as *const TOKEN_USER);
            let mut text: *mut u16 = std::ptr::null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 { return Err(std::io::Error::last_os_error()); }
            let len = (0..).take_while(|i| *text.add(*i) != 0).count();
            let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
            LocalFree(text.cast());
            Ok(sid)
        }
    }

    /// Create one instance of `path` (a `\\.\pipe\...` path). `first` asks for the name to be new:
    /// a pipe that already exists under it, ours or not, is `AddrInUse`.
    pub fn bind_pipe(path: &str, first: bool) -> std::io::Result<PipeInstance> {
        let sddl = wide(&pipe_sddl(&current_user_sid()?));
        let mut descriptor = std::ptr::null_mut();
        if unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut descriptor, std::ptr::null_mut()) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES { nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: descriptor, bInheritHandle: 0 };
        let open_mode = PIPE_ACCESS_DUPLEX | if first { FILE_FLAG_FIRST_PIPE_INSTANCE } else { 0 };
        let handle = unsafe { CreateNamedPipeW(wide(path).as_ptr(), open_mode, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES, 64 * 1024, 64 * 1024, 0, &attributes) };
        let error = std::io::Error::last_os_error();
        unsafe { LocalFree(descriptor.cast()); }
        if handle == INVALID_HANDLE_VALUE {
            return Err(match error.raw_os_error().map(|c| c as u32) {
                Some(ERROR_ACCESS_DENIED | ERROR_PIPE_BUSY) if first => std::io::Error::new(std::io::ErrorKind::AddrInUse, format!("{path} is already being served")),
                _ => error,
            });
        }
        Ok(PipeInstance(unsafe { std::fs::File::from_raw_handle(handle as _) }))
    }

    impl PipeInstance {
        /// Wait for a client; the instance is then that client's stream.
        pub fn accept(self) -> std::io::Result<std::fs::File> {
            let ok = unsafe { ConnectNamedPipe(self.0.as_raw_handle() as HANDLE, std::ptr::null_mut()) };
            let error = std::io::Error::last_os_error();
            if ok == 0 && error.raw_os_error().map(|c| c as u32) != Some(ERROR_PIPE_CONNECTED) { return Err(error); }
            Ok(self.0)
        }
    }

    /// One thread per pipe instance: it waits for a client, makes the instance that replaces it, then serves
    /// the connection. Two instances are waiting from the start, so a client that arrives while another is
    /// being handed off still finds one. (The first form made the next instance only after the current one
    /// connected, and a second client in that window got `ERROR_PIPE_BUSY`: "All pipe instances are busy".)
    fn waiter(pipe: std::sync::Arc<SkyPipe>, path: String, instance: PipeInstance) {
        std::thread::spawn(move || {
            let stream = instance.accept();
            loop {
                match bind_pipe(&path, false) {
                    Ok(next) => { waiter(pipe.clone(), path.clone(), next); break; }
                    Err(e) => { eprintln!("[icu] sky-pipe: could not make the next pipe instance: {e}"); std::thread::sleep(std::time::Duration::from_millis(200)); }
                }
            }
            if let Ok(stream) = stream { serve_connection(&pipe, stream); }
        });
    }

    /// Serve `path` until the process ends.
    pub fn serve_pipe(pipe: std::sync::Arc<SkyPipe>, path: &str) -> std::io::Result<()> {
        let first = bind_pipe(path, true)?;
        let second = bind_pipe(path, false)?;
        waiter(pipe.clone(), path.to_owned(), first);
        waiter(pipe, path.to_owned(), second);
        loop { std::thread::park(); }
    }
}
