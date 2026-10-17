//! Updates the plugin would run for us, run by the app itself so it can tell what happened.
//!
//! Three kinds of Windows install, three ways to run their update (`InstallKind`):
//! all users (NSIS under Program Files): elevated with `runas`, waited for, a cancelled prompt handled (below);
//! per user (NSIS under the profile): not elevated at all. The installer's manifest asks for the highest level the user
//! has, so an administrator gets a UAC prompt for a per-user install that needs none, and the plugin exits the app before
//! the prompt is answered; started with `__COMPAT_LAYER=RunAsInvoker` it runs as the user, with no prompt, and is waited for;
//! MSI (per machine): `msiexec` elevated with `runas`, inside one elevated `cmd` that starts the app again through the user's
//! own shell afterwards (the MSI's own relaunch would start it at High integrity); once the prompt is accepted the app exits,
//! as the plugin does, so the installer can replace the files; a cancelled prompt keeps the app.
//!
//! The all-users case, in detail:
//!
//! `tauri-plugin-updater` 2.12 runs the NSIS installer with `ShellExecuteW(.., "open", ..)` and then calls
//! `std::process::exit(0)` itself (updater.rs, `install_inner`). The installer is `installMode: both`, so it starts
//! without elevation and asks for it only when it needs it. If the elevation is cancelled or refused it falls back to a
//! per-user install next to the all-users one (measured on Windows Server 2022, gaps-windows-2), and the app has
//! already exited, so it never learns the answer. For an all-users install this module does the install step itself:
//! the plugin still downloads the archive and checks its signature, then the installer is started through
//! `ShellExecuteExW` with the `runas` verb, so the elevation prompt is ours, and we wait for the process. A cancelled
//! or refused prompt (ERROR_CANCELLED 1223, ERROR_ACCESS_DENIED 5) or an installer that exits nonzero leaves the
//! running app and the installed copy untouched. Everything that decides what happens is pure and tested on any OS;
//! only the launch itself is Windows code.

use serde::Serialize;

/// `ShellExecuteExW` reports a prompt the user cancelled as this error.
pub const ERROR_CANCELLED: u32 = 1223;
pub const ERROR_ACCESS_DENIED: u32 = 5;
/// The update could not be started because a path cannot be passed on a command line safely.
pub const ERROR_BAD_PATHNAME: u32 = 161;

/// How this install is updated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallKind {
    /// NSIS under Program Files.
    AllUsersNsis,
    /// NSIS anywhere else (the user's profile).
    PerUserNsis,
    /// The MSI bundle: per machine only.
    Msi,
    /// Not an installed copy (a dev build, a portable copy): the plugin's own path.
    Unknown,
}

/// The install kind from what the process can see: its path, whether `uninstall.exe` is beside it (NSIS) and whether its
/// bundle marker says MSI.
pub fn install_kind(exe: &str, program_files: &[&str], nsis_install: bool, msi_bundle: bool) -> InstallKind {
    if msi_bundle {
        return InstallKind::Msi;
    }
    match crate::permissions::updater_scope_arg(exe, program_files, nsis_install) {
        Some("/AllUsers") => InstallKind::AllUsersNsis,
        Some(_) => InstallKind::PerUserNsis,
        None => InstallKind::Unknown,
    }
}

/// Whether the installer of this kind has to be started elevated. A per-user install writes only under the user's
/// profile and HKCU, so it never does.
pub fn needs_elevation(kind: InstallKind) -> bool {
    matches!(kind, InstallKind::AllUsersNsis | InstallKind::Msi)
}

/// The environment variable that makes Windows start a program without the elevation its manifest asks for
/// (`RunAsInvoker`): the NSIS installer's `highestAvailable` manifest otherwise gives an administrator a UAC prompt
/// for an install that needs none.
pub const RUN_AS_INVOKER: (&str, &str) = ("__COMPAT_LAYER", "RunAsInvoker");

/// A downloaded installer is what this install kind expects: an NSIS installer is a PE (`MZ`), an MSI an OLE compound file.
pub fn package_matches(kind: InstallKind, bytes: &[u8]) -> bool {
    match kind {
        InstallKind::AllUsersNsis | InstallKind::PerUserNsis => bytes.starts_with(b"MZ"),
        InstallKind::Msi => bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]),
        InstallKind::Unknown => false,
    }
}

/// What starting and waiting for the elevated installer came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    /// The installer ran and exited with this code.
    Finished(u32),
    /// `ShellExecuteExW` did not start it: this Win32 error.
    NotStarted(u32),
    /// Started elevated and not waited for (the MSI: the app must exit for it to replace the files).
    HandedOff,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    /// The installer finished with exit code 0. It restarts the app itself (`/R`), so the caller does not.
    Installed,
    /// The installer is running and the app has to exit for it to finish (the MSI). The caller exits the app.
    HandedOff,
    /// Nothing changed and the app keeps running. `message` says why, for the toast.
    Kept { message: String },
}

pub fn classify(launch: Launch) -> Outcome {
    match launch {
        Launch::HandedOff => Outcome::HandedOff,
        Launch::Finished(0) => Outcome::Installed,
        Launch::Finished(code) => Outcome::Kept {
            message: format!("The installer stopped (exit code {code}). Copilot was not updated and is still running."),
        },
        Launch::NotStarted(ERROR_CANCELLED) => Outcome::Kept {
            message: "Update cancelled: administrator approval was not given. Copilot was not changed.".into(),
        },
        Launch::NotStarted(ERROR_ACCESS_DENIED) => Outcome::Kept {
            message: "Windows refused administrator approval for the update. Copilot was not changed.".into(),
        },
        Launch::NotStarted(ERROR_BAD_PATHNAME) => Outcome::Kept {
            message: "The update could not be started: the folder it was downloaded to has a name the installer cannot be given. Copilot was not changed.".into(),
        },
        Launch::NotStarted(code) => Outcome::Kept {
            message: format!("The installer could not be started (Windows error {code}). Copilot was not changed."),
        },
    }
}

/// The installer's command line: the plugin's own for `installMode: both` (passive, update mode, restart after) with the
/// all-users scope this module exists for.
pub fn installer_parameters() -> &'static str {
    "/P /UPDATE /R /AllUsers"
}

/// The same for a per-user install: the current-user scope, no elevation.
pub fn per_user_installer_parameters() -> &'static str {
    "/P /UPDATE /R /CurrentUser"
}

/// `msiexec`'s arguments for the update: the plugin's own for `installMode: passive` (a basic progress bar, a restart prompt
/// only when one is needed), without `AUTOLAUNCHAPP`. That property makes the MSI start the app from the elevated installer, so
/// the app comes back at High integrity (measured: 0x3000, where the NSIS installer's restart is at the user's 0x2000).
pub fn msi_parameters(msi: &str) -> String {
    format!("/i \"{msi}\" /passive /promptrestart")
}

/// The command line of the one elevated process an MSI update runs: `cmd` runs `msiexec`, and then, whatever its outcome (the old
/// app comes back when the install failed too), asks the user's own shell to start the app (`explorer.exe <app>`), which starts it at
/// the user's integrity, not the elevated one. `None` when a path holds a character that cannot be put on a `cmd` line safely.
pub fn msi_elevated_command(msiexec: &str, msi: &str, app: &str, explorer: &str) -> Option<String> {
    let unsafe_char = |s: &str| s.chars().any(|c| matches!(c, '"' | '&' | '%' | '^' | '!' | '|' | '<' | '>' | '\r' | '\n'));
    if [msiexec, msi, app, explorer].iter().any(|p| unsafe_char(p) || p.is_empty()) {
        return None;
    }
    Some(format!("/d /s /c \"\"{msiexec}\" {} & \"{explorer}\" \"{app}\"\"", msi_parameters(msi)))
}

/// `SHELLEXECUTEINFOW.fMask` for the launch. `SEE_MASK_NOCLOSEPROCESS` (0x40) gives the process handle to wait on.
/// `SEE_MASK_FLAG_NO_UI` (0x400) keeps the shell from putting its own error box up when the elevation fails: measured on
/// Windows Server 2022, a failed `runas` (the consent prompt ended without an answer) otherwise showed a modal "Restrictions:
/// This operation has been cancelled due to restrictions in effect on this computer" dialog, and `ShellExecuteExW` did not
/// return until someone pressed OK, so the app sat on "Restarting..." and never said the update was not installed.
pub const SEE_MASK_NOCLOSEPROCESS_BITS: u32 = 0x0000_0040;
pub const SEE_MASK_FLAG_NO_UI_BITS: u32 = 0x0000_0400;
pub fn launch_mask() -> u32 {
    SEE_MASK_NOCLOSEPROCESS_BITS | SEE_MASK_FLAG_NO_UI_BITS
}

/// Whether this install's update is run by this module: every installed kind (all users, per user, MSI). Only a copy that is
/// not an installed one (a dev build) keeps the plugin's own path.
pub fn owns_install(kind: InstallKind) -> bool {
    kind != InstallKind::Unknown
}

#[cfg(windows)]
pub fn this_install_kind() -> InstallKind {
    let Ok(exe) = std::env::current_exe() else { return InstallKind::Unknown };
    let nsis = exe.parent().map(|d| d.join("uninstall.exe").is_file()).unwrap_or(false);
    let pf = [std::env::var("ProgramFiles").unwrap_or_default(), std::env::var("ProgramW6432").unwrap_or_default(),
              std::env::var("ProgramFiles(x86)").unwrap_or_default()];
    let msi = matches!(tauri::utils::platform::bundle_type(), Some(tauri::utils::config::BundleType::Msi));
    install_kind(&exe.to_string_lossy(), &pf.iter().map(String::as_str).collect::<Vec<_>>(), nsis, msi)
}
#[cfg(not(windows))]
pub fn this_install_kind() -> InstallKind { InstallKind::Unknown }

pub fn owns_this_install() -> bool { owns_install(this_install_kind()) }

/// Start `installer` elevated (`runas`) with `parameters` and wait for it.
#[cfg(windows)]
pub fn run_elevated(installer: &std::path::Path, parameters: &str) -> Launch {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW;
    let wide = |s: &std::ffi::OsStr| -> Vec<u16> { s.encode_wide().chain(std::iter::once(0)).collect() };
    let verb = wide(std::ffi::OsStr::new("runas"));
    let file = wide(installer.as_os_str());
    let params = wide(std::ffi::OsStr::new(parameters));
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = launch_mask();
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_SHOW;
        if ShellExecuteExW(&mut info) == 0 {
            return Launch::NotStarted(GetLastError());
        }
        if info.hProcess.is_null() {
            return Launch::NotStarted(0);
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code = 0u32;
        let got = GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        if got == 0 { Launch::NotStarted(GetLastError()) } else { Launch::Finished(code) }
    }
}
#[cfg(not(windows))]
pub fn run_elevated(_installer: &std::path::Path, _parameters: &str) -> Launch { Launch::NotStarted(ERROR_ACCESS_DENIED) }

/// Start `program` elevated (`runas`) with `parameters` and do not wait: the answer to the prompt is all that is known.
#[cfg(windows)]
pub fn start_elevated(program: &std::path::Path, parameters: &str) -> Launch {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW;
    let wide = |s: &std::ffi::OsStr| -> Vec<u16> { s.encode_wide().chain(std::iter::once(0)).collect() };
    let verb = wide(std::ffi::OsStr::new("runas"));
    let file = wide(program.as_os_str());
    let params = wide(std::ffi::OsStr::new(parameters));
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = launch_mask();
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_SHOW;
        if ShellExecuteExW(&mut info) == 0 {
            return Launch::NotStarted(GetLastError());
        }
        if !info.hProcess.is_null() {
            CloseHandle(info.hProcess);
        }
        Launch::HandedOff
    }
}
#[cfg(not(windows))]
pub fn start_elevated(_program: &std::path::Path, _parameters: &str) -> Launch { Launch::NotStarted(ERROR_ACCESS_DENIED) }

/// Start `installer` with `parameters` as the user who runs this app, with no elevation and no prompt, and wait for it.
/// `parameters` is passed through as written (the installer parses its own command line).
#[cfg(windows)]
pub fn run_unelevated(installer: &std::path::Path, parameters: &str) -> Launch {
    use std::os::windows::process::CommandExt;
    let mut command = std::process::Command::new(installer);
    command.raw_arg(parameters).env(RUN_AS_INVOKER.0, RUN_AS_INVOKER.1);
    match command.spawn() {
        Ok(mut child) => match child.wait() {
            Ok(status) => Launch::Finished(status.code().map(|c| c as u32).unwrap_or(1)),
            Err(e) => Launch::NotStarted(e.raw_os_error().unwrap_or(0) as u32),
        },
        Err(e) => Launch::NotStarted(e.raw_os_error().unwrap_or(0) as u32),
    }
}
#[cfg(not(windows))]
pub fn run_unelevated(_installer: &std::path::Path, _parameters: &str) -> Launch { Launch::NotStarted(ERROR_ACCESS_DENIED) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cancelled_or_refused_prompt_keeps_the_app_and_says_so() {
        for code in [ERROR_CANCELLED, ERROR_ACCESS_DENIED, ERROR_BAD_PATHNAME, 2, 1155] {
            match classify(Launch::NotStarted(code)) {
                Outcome::Kept { message } => {
                    assert!(message.contains("Copilot was not changed"), "{code}: {message}");
                    assert!(!message.contains('\u{2014}'), "{message}");
                }
                other => panic!("error {code} must keep the app, got {other:?}"),
            }
        }
        // the two answers a person can give are told apart
        let cancelled = format!("{:?}", classify(Launch::NotStarted(ERROR_CANCELLED)));
        let refused = format!("{:?}", classify(Launch::NotStarted(ERROR_ACCESS_DENIED)));
        assert!(cancelled.contains("cancelled") && refused.contains("refused"), "{cancelled} / {refused}");
    }

    #[test]
    fn only_an_installer_that_exited_zero_counts_as_installed() {
        assert_eq!(classify(Launch::Finished(0)), Outcome::Installed);
        for code in [1, 2, 1602, 1223] {
            assert!(matches!(classify(Launch::Finished(code)), Outcome::Kept { .. }), "exit {code}");
        }
    }

    #[test]
    fn the_installer_is_asked_for_all_users_and_the_plugin_switches() {
        let p = installer_parameters();
        for needed in ["/P", "/UPDATE", "/R", "/AllUsers"] {
            assert!(p.split(' ').any(|a| a == needed), "{needed} missing from {p}");
        }
        assert!(!p.contains("/CurrentUser"));
    }

    #[test]
    fn the_shell_may_not_put_up_its_own_error_box_over_a_failed_elevation() {
        // 0x40 gives the process handle to wait on, 0x400 is SEE_MASK_FLAG_NO_UI (windows-sys' own constants are these bits)
        let m = launch_mask();
        assert_eq!(m & 0x40, 0x40, "no process handle to wait on");
        assert_eq!(m & 0x400, 0x400, "the shell may show a modal error dialog and block the launch");
        #[cfg(windows)]
        {
            assert_eq!(SEE_MASK_NOCLOSEPROCESS_BITS, windows_sys::Win32::UI::Shell::SEE_MASK_NOCLOSEPROCESS);
            assert_eq!(SEE_MASK_FLAG_NO_UI_BITS, windows_sys::Win32::UI::Shell::SEE_MASK_FLAG_NO_UI);
        }
    }

    #[test]
    fn every_installed_kind_is_told_apart_and_run_by_us_and_a_dev_build_is_not() {
        let pf = ["C:\\Program Files"];
        let all = "C:\\Program Files\\Copilot\\app.exe";
        let user = "C:\\Users\\n\\AppData\\Local\\Programs\\Copilot\\app.exe";
        assert_eq!(install_kind(all, &pf, true, false), InstallKind::AllUsersNsis);
        assert_eq!(install_kind(user, &pf, true, false), InstallKind::PerUserNsis);
        assert_eq!(install_kind("C:\\Program Files Evil\\app.exe", &pf, true, false), InstallKind::PerUserNsis);
        assert_eq!(install_kind(all, &pf, false, true), InstallKind::Msi, "the bundle marker says MSI");
        assert_eq!(install_kind(all, &pf, false, false), InstallKind::Unknown, "no uninstall.exe and no MSI marker: not an installed copy");
        assert_eq!(install_kind(all, &[], true, false), InstallKind::PerUserNsis);
        for kind in [InstallKind::AllUsersNsis, InstallKind::PerUserNsis, InstallKind::Msi] { assert!(owns_install(kind), "{kind:?}"); }
        assert!(!owns_install(InstallKind::Unknown));
        if !cfg!(windows) { assert!(!owns_this_install()); }
    }

    #[test]
    fn only_the_installs_that_write_outside_the_profile_are_started_elevated() {
        assert!(needs_elevation(InstallKind::AllUsersNsis));
        assert!(needs_elevation(InstallKind::Msi));
        assert!(!needs_elevation(InstallKind::PerUserNsis), "a per-user install needs no administrator");
        assert_eq!(RUN_AS_INVOKER, ("__COMPAT_LAYER", "RunAsInvoker"));
    }

    #[test]
    fn a_per_user_update_is_run_for_the_current_user() {
        let p = per_user_installer_parameters();
        for needed in ["/P", "/UPDATE", "/R", "/CurrentUser"] { assert!(p.split(' ').any(|a| a == needed), "{needed} missing from {p}"); }
        assert!(!p.contains("/AllUsers"));
    }

    #[test]
    fn an_msi_update_does_not_start_the_app_from_the_elevated_installer() {
        let m = msi_parameters("C:\\Temp\\Copilot 0.2.0.msi");
        assert_eq!(m, "/i \"C:\\Temp\\Copilot 0.2.0.msi\" /passive /promptrestart");
        assert!(!m.contains("AUTOLAUNCHAPP"), "the MSI would start the app at High integrity");
        let line = msi_elevated_command("C:\\Windows\\System32\\msiexec.exe", "C:\\Temp\\Copilot 0.2.0.msi",
            "C:\\Program Files\\Copilot\\app.exe", "C:\\Windows\\explorer.exe").expect("a plain command");
        assert_eq!(line, "/d /s /c \"\"C:\\Windows\\System32\\msiexec.exe\" /i \"C:\\Temp\\Copilot 0.2.0.msi\" /passive /promptrestart & \"C:\\Windows\\explorer.exe\" \"C:\\Program Files\\Copilot\\app.exe\"\"");
        // msiexec first, then the user's shell starts the app, whatever msiexec's outcome (a single & runs it either way)
        assert!(line.find("msiexec").unwrap() < line.find(" & ").unwrap() && line.find(" & ").unwrap() < line.find("explorer").unwrap());
        assert!(!line.contains("&&"), "the app must also come back after a failed install");
    }

    #[test]
    fn a_path_that_cannot_be_put_on_a_cmd_line_safely_is_refused() {
        let ok = ("C:\\Windows\\System32\\msiexec.exe", "C:\\Temp\\a.msi", "C:\\Program Files\\Copilot\\app.exe", "C:\\Windows\\explorer.exe");
        assert!(msi_elevated_command(ok.0, ok.1, ok.2, ok.3).is_some());
        for bad in ["C:\\a&calc\\x.msi", "C:\\a\"b\\x.msi", "C:\\%TEMP%\\x.msi", "C:\\a|b\\x.msi", "C:\\a^b\\x.msi", "C:\\a!b\\x.msi", "C:\\a>b\\x.msi", ""] {
            assert!(msi_elevated_command(ok.0, bad, ok.2, ok.3).is_none(), "{bad}");
            assert!(msi_elevated_command(ok.0, ok.1, bad, ok.3).is_none(), "{bad}");
        }
    }

    #[test]
    fn a_package_that_is_not_the_installs_own_kind_is_not_run() {
        let nsis = b"MZ\x90\x00rest";
        let msi = [0xD0u8, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0];
        assert!(package_matches(InstallKind::AllUsersNsis, nsis) && package_matches(InstallKind::PerUserNsis, nsis));
        assert!(!package_matches(InstallKind::Msi, nsis), "an NSIS installer is not an MSI update");
        assert!(package_matches(InstallKind::Msi, &msi));
        assert!(!package_matches(InstallKind::AllUsersNsis, &msi) && !package_matches(InstallKind::PerUserNsis, &msi));
        assert!(!package_matches(InstallKind::Unknown, nsis));
        assert!(!package_matches(InstallKind::Msi, b"PK\x03\x04"), "a zip");
    }

    #[test]
    fn an_msi_handoff_and_the_installer_exit_codes_are_classified() {
        assert_eq!(classify(Launch::HandedOff), Outcome::HandedOff);
        assert_eq!(classify(Launch::Finished(0)), Outcome::Installed);
        assert!(matches!(classify(Launch::NotStarted(ERROR_CANCELLED)), Outcome::Kept { .. }));
    }

    /// Windows only: an installer started unelevated is started with the variable that switches its manifest's elevation
    /// off, and its exit code comes back. `cmd` stands in for the installer.
    #[cfg(windows)]
    #[test]
    fn an_unelevated_start_carries_runasinvoker_and_returns_the_exit_code() {
        let cmd = std::path::Path::new("C:\\Windows\\System32\\cmd.exe");
        let seen = run_unelevated(cmd, "/d /c if \"%__COMPAT_LAYER%\"==\"RunAsInvoker\" (exit 42) else (exit 1)");
        assert_eq!(seen, Launch::Finished(42), "the child must see __COMPAT_LAYER=RunAsInvoker");
        assert_eq!(run_unelevated(cmd, "/d /c exit 7"), Launch::Finished(7));
        assert!(matches!(run_unelevated(std::path::Path::new("C:\\no\\such\\installer.exe"), ""), Launch::NotStarted(_)));
    }
}
