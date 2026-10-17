//! Foreground identity and separately approved native UI Automation content.
use crate::{
    capture::{Content, Source},
    policy::{browser, Context, Website},
    store::CaptureMode,
    Error, Result,
};
use std::{mem::size_of, ptr};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, WAIT_TIMEOUT},
    System::{
        RemoteDesktop::{
            ProcessIdToSessionId, WTSActive, WTSFreeMemory, WTSGetActiveConsoleSessionId,
            WTSQuerySessionInformationW, WTSSessionInfoEx, WTSINFOEXW, WTS_CURRENT_SERVER_HANDLE,
            WTS_SESSIONSTATE_UNLOCK,
        },
        StationsAndDesktops::{
            CloseDesktop, GetUserObjectInformationW, OpenInputDesktop, DESKTOP_READOBJECTS,
            UOI_NAME,
        },
        Threading::{
            GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        },
    },
    UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    },
};

struct Process(HANDLE);
impl Drop for Process {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn process_identity(process: &Process) -> Option<String> {
    if unsafe { WaitForSingleObject(process.0, 0) } != WAIT_TIMEOUT {
        return None;
    }
    let mut path = vec![0u16; 32768];
    let mut length = path.len() as u32;
    if unsafe { QueryFullProcessImageNameW(process.0, 0, path.as_mut_ptr(), &mut length) } == 0
        || unsafe { WaitForSingleObject(process.0, 0) } != WAIT_TIMEOUT
    {
        return None;
    }
    path.get(..length as usize).and_then(application_name)
}

/// The same live executable identity used by History, without exporting its path.
/// The process handle pins this query even if the numeric PID is later reused.
pub fn application_identity(pid: u32) -> Option<String> {
    if pid == 0 { return None; }
    let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid) };
    if raw.is_null() { return None; }
    process_identity(&Process(raw))
}

fn session_allowed(mine: u32, console: u32, active: bool, unlocked: bool, desktop: &str) -> bool {
    mine != 0
        && console != u32::MAX
        && mine == console
        && active
        && unlocked
        && desktop == "Default"
}

fn unlocked_console() -> bool {
    unsafe {
        let mut mine = 0;
        if ProcessIdToSessionId(GetCurrentProcessId(), &mut mine) == 0 {
            return false;
        }
        let console = WTSGetActiveConsoleSessionId();
        let mut buffer = ptr::null_mut();
        let mut length = 0;
        if WTSQuerySessionInformationW(
            WTS_CURRENT_SERVER_HANDLE,
            mine,
            WTSSessionInfoEx,
            &mut buffer,
            &mut length,
        ) == 0
        {
            return false;
        }
        let (active, unlocked) = if !buffer.is_null() && length as usize >= size_of::<WTSINFOEXW>()
        {
            let info = &*buffer.cast::<WTSINFOEXW>();
            if info.Level == 1 {
                let state = &info.Data.WTSInfoExLevel1;
                (
                    state.SessionId == mine && state.SessionState == WTSActive,
                    state.SessionFlags as u32 == WTS_SESSIONSTATE_UNLOCK,
                )
            } else {
                (false, false)
            }
        } else {
            (false, false)
        };
        if !buffer.is_null() {
            WTSFreeMemory(buffer.cast());
        }
        let desktop = OpenInputDesktop(0, 0, DESKTOP_READOBJECTS);
        if desktop.is_null() {
            return false;
        }
        let mut name = [0u16; 128];
        let mut needed = 0;
        let ok = GetUserObjectInformationW(
            desktop,
            UOI_NAME,
            name.as_mut_ptr().cast(),
            std::mem::size_of_val(&name) as u32,
            &mut needed,
        );
        CloseDesktop(desktop);
        if ok == 0 || needed as usize > std::mem::size_of_val(&name) {
            return false;
        }
        let end = name.iter().position(|v| *v == 0).unwrap_or(name.len());
        let Ok(name) = String::from_utf16(&name[..end]) else {
            return false;
        };
        session_allowed(mine, console, active, unlocked, &name)
    }
}

pub(crate) fn foreground() -> Option<(u32, i64)> {
    if !unlocked_console() {
        return None;
    }
    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() || IsWindowVisible(window) == 0 || IsIconic(window) != 0 {
            return None;
        }
        let mut pid = 0;
        if GetWindowThreadProcessId(window, &mut pid) == 0 || pid == 0 {
            return None;
        }
        let mut session = 0;
        if ProcessIdToSessionId(pid, &mut session) == 0 || session != WTSGetActiveConsoleSessionId()
        {
            return None;
        }
        Some((pid, window as usize as i64))
    }
}

pub use crate::windows_uia::Invocations;

fn application_name(path: &[u16]) -> Option<String> {
    let path = String::from_utf16(path).ok()?;
    let name = path.rsplit(['\\', '/']).next()?;
    (!name.is_empty() && name.len() <= 512 && !name.chars().any(char::is_control))
        .then(|| name.to_owned())
}

pub(crate) fn terminal_application(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "windowsterminal.exe"
            | "openconsole.exe"
            | "conhost.exe"
            | "cmd.exe"
            | "powershell.exe"
            | "pwsh.exe"
            | "wsl.exe"
            | "mintty.exe"
            | "alacritty.exe"
            | "wezterm-gui.exe"
    )
}

#[derive(Default)]
pub struct WindowsSource {
    observed: Option<(Context, Process)>,
    uia: Option<crate::windows_uia::Uia>,
    read_focus: bool,
}
impl WindowsSource {
    /// Window-wide observations survive ordinary focus changes between controls.
    /// Focused values additionally use `verify` to bind their exact UIA element.
    pub fn verify_window(&self, context: &Context) -> Result<bool> {
        let Some((observed, process)) = &self.observed else {
            return Ok(false);
        };
        Ok(observed == context
            && unsafe { WaitForSingleObject(process.0, 0) } == WAIT_TIMEOUT
            && foreground() == Some((context.process_id as u32, context.window_id)))
    }
    fn ensure_uia(&mut self) -> Result<()> {
        if self.uia.is_none() {
            self.uia = Some(crate::windows_uia::Uia::new()?)
        }
        Ok(())
    }
    pub fn focus_target(&mut self, context: &Context) -> Result<Option<(Content, i64)>> {
        if context.website != Website::NotWeb || !self.verify_window(context)? {
            return Ok(None);
        }
        self.ensure_uia()?;
        let value = self
            .uia
            .as_mut()
            .unwrap()
            .focus_target(context, !terminal_application(&context.application))?;
        self.read_focus = value.is_some();
        if !self.verify(context)? {
            return Ok(None);
        }
        Ok(value)
    }
    pub fn target_at(
        &mut self,
        context: &Context,
        x: i32,
        y: i32,
        expected: i64,
    ) -> Result<Option<Content>> {
        if context.website != Website::NotWeb || !self.verify_window(context)? {
            return Ok(None);
        }
        self.ensure_uia()?;
        let value = self
            .uia
            .as_ref()
            .unwrap()
            .target_at(context, x, y, expected)?;
        if !self.verify_window(context)? {
            return Ok(None);
        }
        Ok(value)
    }
    pub fn terminal_text(&mut self, context: &Context) -> Result<Option<(String, bool)>> {
        if !terminal_application(&context.application)
            || context.website != Website::NotWeb
            || !self.verify_window(context)?
        {
            return Ok(None);
        }
        self.ensure_uia()?;
        let value = self.uia.as_mut().unwrap().terminal_text(context)?;
        self.read_focus = value.is_some();
        if !self.verify(context)? {
            return Ok(None);
        }
        Ok(value)
    }
    /// Used only by the interaction collector after its window-level consent.
    pub fn native_window_text(&mut self, context: &Context) -> Result<Option<Content>> {
        if terminal_application(&context.application) {
            return Err(Error::new(
                "computerHistoryEventSourceRequired",
                "Terminal text requires stateful redaction",
            ));
        }
        if context.website != Website::NotWeb || !self.verify_window(context)? {
            return Ok(None);
        }
        if self.uia.is_none() {
            self.uia = Some(crate::windows_uia::Uia::new()?)
        }
        let value = self.uia.as_ref().unwrap().window_text(context)?;
        if !self.verify_window(context)? {
            return Ok(None);
        }
        Ok(value)
    }
}
impl Source for WindowsSource {
    fn identify(&mut self) -> Result<Option<Context>> {
        self.observed = None;
        self.read_focus = false;
        let Some((pid, window_id)) = foreground() else {
            return Ok(None);
        };
        let raw = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if raw.is_null() {
            return Ok(None);
        }
        let process = Process(raw);
        let Some(application) = process_identity(&process) else {
            return Ok(None);
        };
        // Resolve only identity. Full paths, titles and control values never enter a record.
        let website = if browser(&application) {
            Website::Unknown
        } else {
            Website::NotWeb
        };
        let context = Context {
            application,
            process_id: pid as i64,
            window_id,
            website,
        };
        if foreground() != Some((pid, window_id)) {
            return Ok(None);
        }
        self.observed = Some((context.clone(), process));
        Ok(Some(context))
    }
    fn read(&mut self, context: &Context, mode: CaptureMode) -> Result<Option<Content>> {
        if mode == CaptureMode::ApplicationMetadata {
            return Ok(None);
        }
        if terminal_application(&context.application) {
            return Err(Error::new(
                "computerHistoryEventSourceRequired",
                "Terminal content requires the stateful interaction recorder",
            ));
        }
        if context.website != Website::NotWeb || !self.verify_window(context)? {
            return Ok(None);
        }
        if self.uia.is_none() {
            self.uia = Some(crate::windows_uia::Uia::new()?)
        }
        let result = self.uia.as_mut().unwrap().read(context, mode);
        self.read_focus = result.as_ref().is_ok_and(|value| value.is_some());
        if result.is_err() {
            self.uia = None
        }
        result
    }
    fn verify(&mut self, context: &Context) -> Result<bool> {
        Ok(self.verify_window(context)?
            && (!self.read_focus
                || self
                    .uia
                    .as_ref()
                    .is_some_and(|uia| uia.verify_focus(context))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_process_identity_uses_the_recorded_executable_scope() {
        let expected = std::env::current_exe().unwrap().file_name().unwrap().to_str().unwrap().to_owned();
        assert_eq!(application_identity(std::process::id()), Some(expected));
        assert!(application_identity(0).is_none());
        assert!(application_identity(u32::MAX).is_none());
    }
    #[test]
    fn unknown_locked_off_console_and_secure_desktops_are_excluded() {
        assert!(session_allowed(1, 1, true, true, "Default"));
        for (mine, console, active, unlocked, desktop) in [
            (0, 0, true, true, "Default"),
            (1, u32::MAX, true, true, "Default"),
            (2, 1, true, true, "Default"),
            (1, 1, false, true, "Default"),
            (1, 1, true, false, "Default"),
            (1, 1, true, true, "Winlogon"),
            (1, 1, true, true, ""),
        ] {
            assert!(!session_allowed(mine, console, active, unlocked, desktop));
        }
    }
    #[test]
    fn application_identity_omits_user_paths_and_preserves_browser_classification() {
        assert_eq!(
            application_name(
                &r"C:\Users\Private Name\apps\Fixture.exe"
                    .encode_utf16()
                    .collect::<Vec<_>>()
            ),
            Some("Fixture.exe".into())
        );
        let chrome = application_name(
            &r"C:\Program Files\Chrome\chrome.exe"
                .encode_utf16()
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(browser(&chrome));
        assert!(application_name(&[]).is_none());
        assert!(application_name(&[0xd800]).is_none());
        assert!(application_name(&"bad\n.exe".encode_utf16().collect::<Vec<_>>()).is_none());
    }
}
