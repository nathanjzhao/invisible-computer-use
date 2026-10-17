//! Bounded ownership probes tolerate the pipe server's accept/recreate gap.
use std::os::windows::io::AsRawHandle;
use std::time::{Duration, Instant};
use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;

pub fn listener_pid(path: &str, budget: Duration) -> Option<u32> {
    let deadline = Instant::now() + budget;
    loop {
        match std::fs::OpenOptions::new().read(true).write(true).open(path) {
            Ok(pipe) => {
                let mut pid = 0;
                return unsafe {
                    (GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) != 0 && pid != 0).then_some(pid)
                };
            }
            // ERROR_PIPE_BUSY and ERROR_FILE_NOT_FOUND are normal while a
            // named-pipe instance is accepted or replaced. Never infer owner
            // identity from these errors, and never replay an input request.
            Err(error) if matches!(error.raw_os_error(), Some(231 | 2)) => {}
            Err(_) => return None,
        }
        let remaining = deadline.checked_duration_since(Instant::now())?;
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use std::sync::atomic::{AtomicU32, Ordering};
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows_sys::Win32::System::Pipes::CreateNamedPipeW;

    fn name() -> String {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        format!(r"\\.\pipe\icu-owner-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
    }

    fn server(path: &str) -> OwnedHandle {
        let name: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        let raw = unsafe { CreateNamedPipeW(name.as_ptr(), PIPE_ACCESS_DUPLEX, 0, 1, 4096, 4096, 0, std::ptr::null()) };
        assert_ne!(raw, INVALID_HANDLE_VALUE, "{}", std::io::Error::last_os_error());
        unsafe { OwnedHandle::from_raw_handle(raw) }
    }

    #[test]
    fn busy_pipe_recovery_preserves_the_kernel_owner_check() {
        let path = name();
        let first = server(&path);
        let client = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
        assert_eq!(std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap_err().raw_os_error(), Some(231));
        let next_path = path.clone();
        let worker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(75));
            drop(client); drop(first);
            server(&next_path)
        });
        let owner = listener_pid(&path, Duration::from_millis(250));
        let _replacement = worker.join().unwrap();
        assert_eq!(owner, Some(std::process::id()));
        assert_ne!(owner, Some(std::process::id() + 1));
    }

    #[test]
    fn permanently_busy_pipe_has_a_bounded_failure() {
        let path = name();
        let _server = server(&path);
        let _client = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
        let start = Instant::now();
        assert_eq!(listener_pid(&path, Duration::from_millis(80)), None);
        assert!(start.elapsed() >= Duration::from_millis(70));
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
