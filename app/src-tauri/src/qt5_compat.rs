//! Explicit process-launch mode. It never starts a recorder or alters system libraries.
pub fn entry() -> Option<i32> {
    let mut args = std::env::args_os();
    args.next();
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--qt5-accessibility")) { return None; }
    #[cfg(target_os = "linux")]
    let result = launch(args.collect());
    #[cfg(not(target_os = "linux"))]
    let result: Result<(), String> = Err("Qt5 accessibility compatibility requires Linux Wayland.".into());
    if let Err(error) = result { eprintln!("{error}"); return Some(2); }
    Some(0)
}

#[cfg(target_os = "linux")]
fn launch(mut args: Vec<std::ffi::OsString>) -> Result<(), String> {
    use std::{fs::File, io::Write, os::{fd::{FromRawFd, AsRawFd}, unix::{fs::MetadataExt, process::CommandExt}}, process::Command};
    if args.first().is_some_and(|s| s == "--") { args.remove(0); }
    let program = args.first().ok_or("Usage: app --qt5-accessibility -- /path/to/qt5-app [arguments]")?;
    if std::env::var("XDG_SESSION_TYPE").as_deref() != Ok("wayland")
        || std::env::var_os("WAYLAND_DISPLAY").is_none()
        || std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        return Err("Run Qt5 accessibility compatibility from a Wayland desktop session.".into());
    }
    if std::env::var_os("LD_PRELOAD").is_some() {
        return Err("Qt5 accessibility compatibility cannot be combined with an existing preload override.".into());
    }
    if unsafe { libc::getuid() == 0 || libc::geteuid() != libc::getuid() || libc::getegid() != libc::getgid() } {
        return Err("Qt5 accessibility compatibility requires an ordinary user process.".into());
    }
    let path = std::path::Path::new(program).canonicalize().map_err(|e| format!("Cannot open the selected application: {e}"))?;
    let metadata = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.mode() & 0o111 == 0 || metadata.mode() & 0o6000 != 0 {
        return Err("Select an ordinary executable application without set-user or set-group privileges.".into());
    }
    let target = File::open(&path).map_err(|e| e.to_string())?;
    let capabilities = unsafe { libc::fgetxattr(target.as_raw_fd(), c"security.capability".as_ptr(), std::ptr::null_mut(), 0) };
    if capabilities > 0 { return Err("Qt5 accessibility compatibility does not launch applications with file capabilities.".into()); }
    if capabilities < 0 && !matches!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ENODATA | libc::ENOTSUP)) {
        return Err("The selected application's capabilities could not be checked.".into());
    }
    let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/libcopilot-qt5.so"));
    // A sealed, inherited descriptor avoids writable helper caches and path replacement.
    let mut fd = unsafe { libc::memfd_create(c"copilot-qt5-compat".as_ptr(), libc::MFD_ALLOW_SEALING) };
    if fd < 0 { return Err(format!("Cannot prepare Qt5 accessibility compatibility: {}", std::io::Error::last_os_error())); }
    if fd < 3 {
        let copied = unsafe { libc::fcntl(fd, libc::F_DUPFD, 3) };
        unsafe { libc::close(fd); }
        if copied < 0 { return Err(format!("Cannot retain Qt5 accessibility compatibility: {}", std::io::Error::last_os_error())); }
        fd = copied;
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(bytes).map_err(|e| e.to_string())?;
    let seals = libc::F_SEAL_WRITE | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_SEAL;
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, seals) } < 0 {
        return Err(format!("Cannot seal Qt5 accessibility compatibility: {}", std::io::Error::last_os_error()));
    }
    let mut command = Command::new(path);
    command.args(&args[1..]);
    // App-private settings and model credentials do not belong to a launched app.
    for (name, _) in std::env::vars_os() {
        let name_text = name.to_string_lossy();
        if name_text.starts_with("ICU_") || matches!(name_text.as_ref(), "OPENAI_API_KEY" | "ANTHROPIC_API_KEY" | "CODEX_HOME" | "CLAUDE_CONFIG_DIR") {
            command.env_remove(name);
        }
    }
    command.env("LD_PRELOAD", format!("/proc/self/fd/{}", file.as_raw_fd()))
        .env("COPILOT_QT5_ACCESSIBILITY_COMPAT", "1")
        .env("COPILOT_QT5_COMPAT_FD", file.as_raw_fd().to_string())
        .env("QT_QPA_PLATFORM", "wayland");
    Err(format!("Could not launch the selected application: {}", command.exec()))
}
