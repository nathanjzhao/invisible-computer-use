//! Start suspended, contain the process tree, then allow the command to run.
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, THREADENTRY32, TH32CS_SNAPTHREAD,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    GetProcessIdOfThread, OpenThread, ResumeThread, CREATE_NO_WINDOW, CREATE_SUSPENDED,
    THREAD_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
};

pub struct Job(OwnedHandle);

fn error(operation: &str) -> String {
    format!("subprocess {operation}: {}", std::io::Error::last_os_error())
}

impl Job {
    fn new() -> Result<Self, String> {
        unsafe {
            let raw = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if raw.is_null() { return Err(error("create job")); }
            let job = Self(OwnedHandle::from_raw_handle(raw));
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(raw, JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32) == 0 {
                return Err(error("configure job"));
            }
            Ok(job)
        }
    }

    pub fn finish(&self) -> Result<(), String> {
        // Closing the handle also kills the tree, including on host death.
        // Explicit termination lets us verify exit before returning to callers.
        unsafe {
            if TerminateJobObject(self.0.as_raw_handle(), 1) == 0 {
                return Err(error("terminate job"));
            }
        }
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let mut accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
            unsafe {
                if QueryInformationJobObject(self.0.as_raw_handle(), JobObjectBasicAccountingInformation,
                    (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                    std::mem::size_of_val(&accounting) as u32, std::ptr::null_mut()) == 0 {
                    return Err(error("query job cleanup"));
                }
            }
            if accounting.ActiveProcesses == 0 { return Ok(()); }
            if Instant::now() >= deadline { return Err("subprocess tree cleanup exceeded one second".into()); }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn resume(child: &Child) -> Result<(), String> {
    // Stable Rust does not expose Child's primary thread handle. The process
    // has never run, so its suspended initial thread can be found by owner PID.
    unsafe {
        let raw = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if raw == INVALID_HANDLE_VALUE { return Err(error("snapshot suspended thread")); }
        let snapshot = OwnedHandle::from_raw_handle(raw);
        let mut entry: THREADENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of_val(&entry) as u32;
        let mut found = Thread32First(snapshot.as_raw_handle(), &mut entry);
        while found != 0 {
            if entry.th32OwnerProcessID == child.id() {
                let raw = OpenThread(THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION, 0, entry.th32ThreadID);
                if raw.is_null() { return Err(error("open suspended thread")); }
                let thread = OwnedHandle::from_raw_handle(raw);
                if GetProcessIdOfThread(thread.as_raw_handle()) != child.id() {
                    return Err("subprocess thread ownership changed".into());
                }
                if ResumeThread(thread.as_raw_handle()) != 1 {
                    return Err("subprocess initial thread was not suspended exactly once".into());
                }
                return Ok(());
            }
            found = Thread32Next(snapshot.as_raw_handle(), &mut entry);
        }
        Err("subprocess initial thread was not found".into())
    }
}

pub fn spawn(command: &mut Command) -> Result<(Child, Job), String> {
    let job = Job::new()?;
    // Assignment after an ordinary spawn would let fast children escape before
    // containment. Failure at any setup stage kills the still-owned child.
    let mut child = command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED)
        .spawn().map_err(|e| format!("spawn: {e}"))?;
    let setup = unsafe {
        if AssignProcessToJobObject(job.0.as_raw_handle(), child.as_raw_handle()) == 0 {
            Err(error("assign job"))
        } else { resume(&child) }
    };
    if let Err(error) = setup {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    Ok((child, job))
}
