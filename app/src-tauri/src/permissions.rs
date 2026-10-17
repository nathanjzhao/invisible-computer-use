//! macOS privacy grants for the app-owned driver.
//!
//! The bundled driver runs as a child of Copilot, so macOS attributes its
//! Accessibility and Screen Recording use to Copilot. In that embedded mode
//! the driver never raises a prompt of its own; the host app must. Without
//! this, a clean install shows "driver connected" while every observation
//! and input silently fails.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    /// False where the OS has no such grants (the fields are then true).
    pub supported: bool,
    pub accessibility: bool,
    pub screen_recording: bool,
    /// Windows only: why the UIAccess helper cannot start from where Copilot is installed.
    pub uia_note: Option<String>,
    /// The identity whose grants were checked, never the app for an external endpoint.
    pub source: &'static str,
    /// A passive status probe failed. False grants must not be presented as verified denial.
    pub error: Option<String>,
}

/// Windows grants UIAccess only to a signed binary under `\Program Files\`
/// (or a machine policy an admin set). A per-user install in `%LOCALAPPDATA%`
/// is outside it, so its helper never reaches UIAccess integrity.
pub fn uia_path_ok(exe: &str, program_files: &[&str]) -> bool {
    let exe = exe.replace('/', "\\").to_lowercase();
    program_files.iter().filter(|p| !p.is_empty()).any(|p| {
        let p = p.replace('/', "\\").trim_end_matches('\\').to_lowercase();
        exe.starts_with(&format!("{p}\\"))
    })
}

/// The warning for a Windows install the UIAccess helper cannot start from; None elsewhere or when the path is fine.
pub fn uia_note_for(exe: &str, program_files: &[&str]) -> Option<String> {
    if uia_path_ok(exe, program_files) { return None; }
    Some("Copilot is installed for this user only, so Windows will not start its UIAccess helper from here. \
Keystrokes to Calculator, Settings and other Store apps may be dropped, and background clicks into some windows can fail. \
Reinstall for all users (run the installer as an administrator and choose all users, or pass /AllUsers) to fix this.".into())
}

/// The scope switch the NSIS installer is run with when the updater starts it. The installer's `both` mode
/// installs per user unless told otherwise, so an update of an all-users install (under Program Files, where the
/// UIAccess helper can start) used to land per user and leave the old copy in place (measured on Windows Server 2022).
/// The install keeps its scope: all users from Program Files, the current user from anywhere else. An MSI install has
/// no `uninstall.exe` beside it and takes no NSIS switches, so it gets none.
pub fn updater_scope_arg(exe: &str, program_files: &[&str], nsis_install: bool) -> Option<&'static str> {
    if !nsis_install { return None; }
    Some(if uia_path_ok(exe, program_files) { "/AllUsers" } else { "/CurrentUser" })
}

/// `updater_scope_arg` for this process (Windows only; other platforms take no installer switches).
#[cfg(windows)]
pub fn updater_installer_arg() -> Option<&'static str> {
    let exe = std::env::current_exe().ok()?;
    let nsis = exe.parent().map(|d| d.join("uninstall.exe").is_file()).unwrap_or(false);
    let pf = [std::env::var("ProgramFiles").unwrap_or_default(), std::env::var("ProgramW6432").unwrap_or_default(),
              std::env::var("ProgramFiles(x86)").unwrap_or_default()];
    updater_scope_arg(&exe.to_string_lossy(), &pf.iter().map(String::as_str).collect::<Vec<_>>(), nsis)
}
#[cfg(not(windows))]
pub fn updater_installer_arg() -> Option<&'static str> { None }

/// Why this copy cannot be updated by the person using it, or None when it can. An install under Program Files is
/// all users: its installer needs an administrator to replace it. When the signed-in user cannot elevate at all (a
/// standard user), the installer's elevation fails and it falls back to a per-user copy next to the old one, after the
/// app has already quit (measured on Windows Server 2022, UAC standard-deny). So the update is refused up front,
/// before anything is downloaded, with a message that says what to do. A per-user install never needs elevation.
pub fn update_blocked_reason(exe: &str, program_files: &[&str], can_elevate: bool) -> Option<String> {
    if can_elevate || !uia_path_ok(exe, program_files) { return None; }
    Some("Copilot is installed for all users, and this account is not an administrator, so it cannot install the update. \
Ask an administrator to run the update (or install the new version for all users).".into())
}

/// Whether the signed-in user can approve an elevation prompt: an administrator (a filtered token that UAC can
/// elevate, or one that is already elevated). A standard user cannot, whatever credentials they might know.
#[cfg(windows)]
fn current_user_can_elevate() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        CheckTokenMembership, CreateWellKnownSid, GetTokenInformation, TokenElevationType, WinBuiltinAdministratorsSid,
        TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 { return false; }
        let mut kind: i32 = 0;
        let mut returned = 0u32;
        let ok = GetTokenInformation(token, TokenElevationType, &mut kind as *mut i32 as *mut _, 4, &mut returned) != 0;
        let can = if !ok {
            false
        } else if kind == 2 || kind == 3 {
            // TokenElevationTypeFull: already elevated. Limited: the admin's filtered token; UAC can elevate it.
            true
        } else {
            // Default: no split token. An administrator with UAC off is elevated already; anyone else is a standard user.
            let mut sid = [0u8; 68];
            let mut len = sid.len() as u32;
            let mut member = 0i32;
            CreateWellKnownSid(WinBuiltinAdministratorsSid, std::ptr::null_mut(), sid.as_mut_ptr() as *mut _, &mut len) != 0
                && CheckTokenMembership(std::ptr::null_mut(), sid.as_mut_ptr() as *mut _, &mut member) != 0
                && member != 0
        };
        CloseHandle(token);
        can
    }
}

/// `update_blocked_reason` for this process. Off Windows an update never needs elevation here.
#[cfg(windows)]
pub fn updater_blocked() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let pf = [std::env::var("ProgramFiles").unwrap_or_default(), std::env::var("ProgramW6432").unwrap_or_default(),
              std::env::var("ProgramFiles(x86)").unwrap_or_default()];
    update_blocked_reason(&exe.to_string_lossy(), &pf.iter().map(String::as_str).collect::<Vec<_>>(), current_user_can_elevate())
}
#[cfg(not(windows))]
pub fn updater_blocked() -> Option<String> { None }

#[cfg(windows)]
fn uia_note() -> Option<String> {
    let exe = std::env::current_exe().ok()?.to_string_lossy().to_string();
    let pf = [std::env::var("ProgramFiles").unwrap_or_default(), std::env::var("ProgramW6432").unwrap_or_default(),
              std::env::var("ProgramFiles(x86)").unwrap_or_default()];
    uia_note_for(&exe, &pf.iter().map(String::as_str).collect::<Vec<_>>())
}
#[cfg(not(windows))]
fn uia_note() -> Option<String> { None }

#[cfg(target_os = "macos")]
mod native {
    use std::ffi::c_void;
    type CFTypeRef = *const c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> bool;
        static kAXTrustedCheckOptionPrompt: CFTypeRef;
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFBooleanTrue: CFTypeRef;
        static kCFTypeDictionaryKeyCallBacks: c_void;
        static kCFTypeDictionaryValueCallBacks: c_void;
        fn CFDictionaryCreate(allocator: CFTypeRef, keys: *const CFTypeRef, values: *const CFTypeRef, count: isize,
                              key_callbacks: *const c_void, value_callbacks: *const c_void) -> CFTypeRef;
        fn CFRelease(value: CFTypeRef);
    }

    pub fn status() -> super::Status {
        // Both calls only read the current grant; neither shows UI.
        unsafe { super::Status { supported: true, accessibility: AXIsProcessTrusted(), screen_recording: CGPreflightScreenCaptureAccess(), uia_note: None, source: "app", error: None } }
    }

    /// Ask macOS to list Copilot in both privacy panes and show its prompts.
    pub fn request() -> super::Status {
        let current = status();
        unsafe {
            if !current.accessibility {
                let keys = [kAXTrustedCheckOptionPrompt];
                let values = [kCFBooleanTrue];
                let options = CFDictionaryCreate(std::ptr::null(), keys.as_ptr(), values.as_ptr(), 1,
                    &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
                AXIsProcessTrustedWithOptions(options);
                if !options.is_null() { CFRelease(options); }
            }
            if !current.screen_recording { CGRequestScreenCaptureAccess(); }
        }
        status()
    }
}

pub fn status() -> Status {
    if !crate::cua::uses_host_owned_driver() {
        let mut status=driver_status(crate::cua::passive_permission_probe);
        if status.error.is_none() { status.error=crate::cua::capture_permission_error(); }
        return status;
    }
    let mut status = io_status();
    status.error = crate::cua::capture_permission_error();
    status
}

fn host_status() -> Status {
    #[cfg(target_os = "macos")]
    { native::status() }
    #[cfg(not(target_os = "macos"))]
    { Status { supported: false, accessibility: true, screen_recording: true, uia_note: uia_note(), source: "app", error: None } }
}

/// Query only the selected endpoint. A read-only status request must not set
/// up cursor state, discover windows, probe live capture, or start a driver.
fn driver_status(mut probe: impl FnMut(&str, &serde_json::Value) -> Result<serde_json::Value, String>) -> Status {
    use serde_json::json;
    let direct = probe("check_permissions", &json!({"prompt":false,"probe_direct_capture":false}))
        .and_then(|value| parse_driver_permissions(&value));
    match direct {
        Ok(status) => status,
        Err(first) => match probe("health_report", &json!({"include":["tcc_accessibility","tcc_screen_recording"]}))
            .and_then(|value| parse_driver_health(&value)) {
            Ok(status) => status,
            Err(second) => Status { supported:true,accessibility:false,screen_recording:false,
                uia_note:None,source:"driver",error:Some(format!("Could not check the selected driver's access: {first}; {second}")) },
        },
    }
}

fn driver_grants(supported: bool, accessibility: bool, screen_recording: bool) -> Status {
    Status { supported, accessibility, screen_recording, uia_note:None, source:"driver", error:None }
}

fn passive_result(value: &serde_json::Value) -> Result<&serde_json::Value, String> {
    let fields = value.get("structuredContent").unwrap_or(value);
    if value.get("isError").and_then(serde_json::Value::as_bool) == Some(true)
        || fields.get("isError").and_then(serde_json::Value::as_bool) == Some(true)
        || fields.get("error").is_some_and(|error| !error.is_null() && error != &serde_json::Value::Bool(false)) {
        return Err("permission status was refused".into());
    }
    Ok(fields)
}

fn parse_driver_permissions(value: &serde_json::Value) -> Result<Status,String> {
    let fields = passive_result(value)?;
    let accessibility = fields.get("accessibility").and_then(serde_json::Value::as_bool)
        .ok_or("driver did not report Accessibility status")?;
    let screen_recording = fields.get("screen_recording").and_then(serde_json::Value::as_bool)
        .ok_or("driver did not report Screen Recording status")?;
    // Deliberately ignore direct_capture_verification: it is historical evidence,
    // not a current permission grant or proof that a future capture is silent.
    Ok(driver_grants(true, accessibility, screen_recording))
}

fn parse_driver_health(value: &serde_json::Value) -> Result<Status,String> {
    let fields = passive_result(value)?;
    match fields.get("platform").and_then(serde_json::Value::as_str) {
        Some("win32" | "windows" | "linux") => return Ok(driver_grants(false,true,true)),
        Some("darwin" | "macos") => {},
        _ => return Err("driver did not report a recognized platform".into()),
    }
    let checks = fields.get("checks").and_then(serde_json::Value::as_array).ok_or("driver health checks are missing")?;
    let grant = |name: &str| match checks.iter().find(|check| check["name"] == name)
        .and_then(|check|check["status"].as_str()) {
        Some("pass") => Ok(true), Some("fail") => Ok(false),
        _ => Err(format!("driver did not check {name}")),
    };
    Ok(driver_grants(true,grant("tcc_accessibility")?,grant("tcc_screen_recording")?))
}

/// Silent request preflight. Fixture processes inject grants explicitly so
/// tests never depend on or request the developer machine's privacy grants.
pub(crate) fn io_status() -> Status {
    #[cfg(test)]
    if let Ok(grants) = std::env::var("ICU_TEST_IO_PERMISSIONS") {
        return Status { supported: true, accessibility: matches!(grants.as_str(), "granted" | "ax_only"),
            screen_recording: matches!(grants.as_str(), "granted" | "screen_only"), uia_note: None, source: "app", error: None };
    }
    host_status()
}

pub(crate) fn missing_io_permission(status: &Status, accessibility: bool, capture: bool) -> Option<&'static str> {
    if !status.supported { return None; }
    if accessibility && !status.accessibility {
        Some("Copilot needs Accessibility access. Choose Grant access in Copilot to request it.")
    } else if capture && !status.screen_recording {
        Some("Copilot needs Screen Recording access. Choose Grant access in Copilot to request it.")
    } else { None }
}

/// Shows system UI on macOS. Only call from an explicit user action.
pub fn request() -> Status {
    // A selected external endpoint owns its own permission flow. Never ask for
    // this app's grants, even if the UI invokes its request command directly.
    if !crate::cua::uses_host_owned_driver() {
        let mut current=driver_status(crate::cua::passive_permission_probe);
        crate::cua::retry_capture_permission_after_user_action(current.error.is_none() && current.screen_recording);
        if current.error.is_none() { current.error=crate::cua::capture_permission_error(); }
        return current;
    }
    #[cfg(target_os = "macos")]
    let current = native::request();
    #[cfg(not(target_os = "macos"))]
    let current = io_status();
    crate::cua::retry_capture_permission_after_user_action(current.screen_recording);
    status()
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyPane {
    Accessibility,
    ScreenRecording,
}

/// An explicit recovery control after macOS stops repeating a denied prompt.
/// This opens the normal OS pane; it never changes a privacy grant.
pub fn open_settings(pane: PrivacyPane) -> Result<(), String> {
    if !crate::cua::uses_host_owned_driver() {
        return Err("Configure access in the application that owns the selected driver.".into());
    }
    #[cfg(target_os = "macos")]
    {
        let url = match pane {
            PrivacyPane::Accessibility => "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
            PrivacyPane::ScreenRecording => "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
        };
        let result = std::process::Command::new("/usr/bin/open").arg(url).status()
            .map_err(|error| format!("Could not open System Settings: {error}"))?;
        if result.success() { Ok(()) } else { Err("System Settings could not be opened".into()) }
    }
    #[cfg(not(target_os = "macos"))]
    { let _ = pane; Err("These privacy settings are available on macOS".into()) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn external_status_ignores_historical_capture_evidence_and_rejects_partial_grants() {
        use serde_json::json;
        let current=super::parse_driver_permissions(&json!({"accessibility":false,"screen_recording":false,
            "screen_recording_capturable":true,"direct_capture_verification":{"status":"ready"}})).unwrap();
        assert!(!current.accessibility && !current.screen_recording);
        assert_eq!(current.source,"driver");
        assert!(super::parse_driver_permissions(&json!({"accessibility":true})).is_err());
        assert!(super::parse_driver_permissions(&json!({"isError":true,"accessibility":true,"screen_recording":true})).is_err());
        assert!(super::parse_driver_health(&json!({"platform":"darwin","checks":[
            {"name":"tcc_accessibility","status":"skip"},{"name":"tcc_screen_recording","status":"pass"}]})).is_err());
        let windows=super::parse_driver_health(&json!({"platform":"win32","checks":[]})).unwrap();
        assert!(!windows.supported && windows.accessibility && windows.screen_recording);
        assert_eq!(windows.source,"driver");
        assert!(super::parse_driver_health(&json!({"platform":"unknown","checks":[]})).is_err());
    }

    #[test]
    fn failed_external_status_is_additive_and_never_becomes_verified_denial() {
        use serde_json::json;
        let mut calls=Vec::new();
        let status=super::driver_status(|tool,args|{calls.push((tool.to_owned(),args.clone()));Err("offline".into())});
        assert_eq!(calls,vec![("check_permissions".into(),json!({"prompt":false,"probe_direct_capture":false})),
            ("health_report".into(),json!({"include":["tcc_accessibility","tcc_screen_recording"]}))]);
        let fields=serde_json::to_value(status).unwrap();
        for field in ["supported","accessibility","screen_recording","uia_note","source","error"] {assert!(fields.get(field).is_some());}
        assert_eq!(fields["source"],"driver");
        assert!(fields["error"].as_str().is_some_and(|error|error.contains("offline")));
    }
    #[test]
    fn silent_io_preflight_only_requires_the_capability_used() {
        let state = |ax, screen| super::Status {supported:true,accessibility:ax,screen_recording:screen,uia_note:None,source:"app",error:None};
        assert!(super::missing_io_permission(&state(false,false),false,false).is_none());
        assert!(super::missing_io_permission(&state(true,false),true,false).is_none(), "AX-only observations remain usable");
        assert!(super::missing_io_permission(&state(false,true),false,true).is_none(), "image-only observations do not need AX");
        assert!(super::missing_io_permission(&state(false,true),true,false).unwrap().contains("Accessibility"));
        assert!(super::missing_io_permission(&state(true,false),false,true).unwrap().contains("Screen Recording"));
    }
    #[test]
    fn status_reports_platform_support_without_prompting() {
        let status = super::status();
        assert_eq!(status.supported, cfg!(target_os = "macos"));
        if !status.supported { assert!(status.accessibility && status.screen_recording); }
    }

    #[test]
    fn an_update_keeps_the_scope_of_the_install_it_replaces() {
        let pf = ["C:\\Program Files", "C:\\Program Files (x86)"];
        assert_eq!(super::updater_scope_arg("C:\\Program Files\\Copilot\\app.exe", &pf, true), Some("/AllUsers"));
        assert_eq!(super::updater_scope_arg("C:\\Users\\n\\AppData\\Local\\Programs\\Copilot\\app.exe", &pf, true), Some("/CurrentUser"));
        // not under Program Files just because the name starts the same way
        assert_eq!(super::updater_scope_arg("C:\\Program Files Evil\\app.exe", &pf, true), Some("/CurrentUser"));
        // an MSI install takes no NSIS switches: msiexec would reject them
        assert_eq!(super::updater_scope_arg("C:\\Program Files\\Copilot\\app.exe", &pf, false), None);
        // nothing known about Program Files: the choice that needs no elevation
        assert_eq!(super::updater_scope_arg("C:\\Program Files\\Copilot\\app.exe", &[], true), Some("/CurrentUser"));
        // off Windows there is nothing to pass
        if !cfg!(windows) { assert_eq!(super::updater_installer_arg(), None); }
    }

    #[test]
    fn a_standard_user_is_not_offered_an_update_of_an_all_users_install() {
        let pf = ["C:\\Program Files", "C:\\Program Files (x86)"];
        let all_users = "C:\\Program Files\\Copilot\\app.exe";
        let reason = super::update_blocked_reason(all_users, &pf, false).expect("a standard user must be refused");
        // the message has to tell the person what to do, not only what is wrong
        assert!(reason.contains("Ask an administrator") && reason.contains("not an administrator") && !reason.contains('\u{2014}'), "{reason}");
        // an administrator (filtered or elevated token) gets the installer's own elevation prompt
        assert_eq!(super::update_blocked_reason(all_users, &pf, true), None);
        // a per-user install never needs elevation, whoever runs it
        assert_eq!(super::update_blocked_reason("C:\\Users\\n\\AppData\\Local\\Programs\\Copilot\\app.exe", &pf, false), None);
        // a folder that only starts like Program Files is per user
        assert_eq!(super::update_blocked_reason("C:\\Program Files Evil\\app.exe", &pf, false), None);
        // nothing known about Program Files: the install is not shown to need elevation
        assert_eq!(super::update_blocked_reason(all_users, &[], false), None);
        if !cfg!(windows) { assert_eq!(super::updater_blocked(), None); }
    }

    /// Run on a Windows host as each kind of user with `ICU_EXPECT_CAN_ELEVATE` set to 0 (a standard
    /// user) or 1 (an administrator, filtered or elevated): the check must agree with who is running.
    /// Without the variable it checks nothing.
    #[cfg(windows)]
    #[test]
    fn the_signed_in_users_ability_to_elevate_is_read_from_the_token() {
        let Ok(expected) = std::env::var("ICU_EXPECT_CAN_ELEVATE") else { return };
        assert_eq!(super::current_user_can_elevate(), expected == "1", "ICU_EXPECT_CAN_ELEVATE={expected}");
    }

    #[test]
    fn a_per_user_install_gets_the_uiaccess_warning() {
        let pf = ["C:\\Program Files", "C:\\Program Files (x86)"];
        assert!(super::uia_note_for("C:\\Program Files\\Copilot\\app.exe", &pf).is_none());
        assert!(super::uia_note_for("c:\\program files (x86)\\Copilot\\app.exe", &pf).is_none());
        let note = super::uia_note_for("C:\\Users\\n\\AppData\\Local\\Programs\\Copilot\\app.exe", &pf).unwrap();
        assert!(note.contains("UIAccess") && note.contains("all users") && !note.contains('\u{2014}'), "{note}");
        // A folder that only starts with the same letters is not under Program Files.
        assert!(super::uia_note_for("C:\\Program Files Evil\\app.exe", &pf).is_some());
        // With no Program Files known, it cannot be shown to be fine.
        assert!(super::uia_note_for("C:\\Program Files\\Copilot\\app.exe", &[]).is_some());
    }
}
