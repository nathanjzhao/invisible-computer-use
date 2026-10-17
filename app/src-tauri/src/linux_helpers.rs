//! Current-user setup, called only by explicit History setup or approved scope changes.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const EXTENSION: &[u8] = include_bytes!("../linux-helpers/gnome/winrects@cua/extension.js");
const METADATA: &[u8] = include_bytes!("../linux-helpers/gnome/winrects@cua/metadata.json");
const HISTORY_POINTER: &[u8] =
    include_bytes!("../linux-helpers/gnome/winrects@cua/history-pointer.js");
const LICENSE: &[u8] = include_bytes!("../linux-helpers/LICENSE");

fn desktop() -> String {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase()
}
pub fn notice() -> Option<&'static str> {
    if !computer_history::linux::is_wayland() {
        return None;
    }
    let desktop = desktop();
    if desktop.contains("gnome") {
        Some("Setting up desktop capture installs a GNOME helper for your account. Sign out and sign back in to load a new helper. Your selected capture scope stays unchanged.")
    } else if desktop.contains("kde") {
        Some("Setting up desktop capture enables the packaged KWin helper. It must match your Plasma version. Your selected capture scope stays unchanged.")
    } else {
        None
    }
}

// Do not follow symlinks or replace another extension. The known bundle bytes
// are compiled into the app, so setup does not execute a user-writable script.
fn owned_directory(path: &Path) -> Result<(), String> {
    let m = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o022 != 0 {
        return Err(format!(
            "Helper folder is not private to your account: {}",
            path.display()
        ));
    }
    Ok(())
}

// Only exact previously shipped bundles are eligible for replacement.
// Version labels alone cannot establish that the directory belongs to us.
const PREVIOUS_GNOME: [(&str, &str); 3] = [
    (
        "extension.js",
        "f5d08a8f72417a6440c08c3e727f677e1f73cff89de279c9023408022462be02",
    ),
    (
        "metadata.json",
        "3f3a4041b0bc6b8ab0e1aac4bc734ea29d703a4f6ca4936e65008ea38c8ed829",
    ),
    (
        "LICENSE",
        "c0779290c1d4783169aa3dbfb55feb505e563ef8a004bbf55298ceffcfbda8d9",
    ),
];
const PREVIOUS_GNOME_V10: [(&str, &str); 4] = [
    ("extension.js", "358032236a1837715664d5069853760daaccbe6b8790be9be0e165fcbedc5209"),
    ("metadata.json", "4b39f4be999474e3fd823af27f5f118fe997fe7f9ecca51d5322fb4f4ec211a1"),
    ("history-pointer.js", "9b50bd9a06deddee8a8b4c632c983735914bf095ebdf2791733b4a10a4706e2f"),
    ("LICENSE", "c0779290c1d4783169aa3dbfb55feb505e563ef8a004bbf55298ceffcfbda8d9"),
];

#[derive(Debug, PartialEq)]
struct HelperSnapshot {
    device: u64,
    inode: u64,
    hashes: BTreeMap<String, String>,
}

fn helper_snapshot(path: &Path) -> Result<HelperSnapshot, String> {
    owned_directory(path)?;
    let directory = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    let mut hashes = BTreeMap::new();
    for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "Unknown helper file")?;
        if ![
            "extension.js",
            "metadata.json",
            "history-pointer.js",
            "LICENSE",
        ]
        .contains(&name.as_str())
        {
            return Err(
                "The GNOME helper contains an unknown file. Its contents were preserved.".into(),
            );
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(entry.path())
            .map_err(|e| e.to_string())?;
        let m = file.metadata().map_err(|e| e.to_string())?;
        if !m.is_file()
            || m.uid() != unsafe { libc::geteuid() }
            || m.mode() & 0o022 != 0
            || m.nlink() != 1
            || m.len() > 65536
        {
            return Err(
                "The GNOME helper contains an unsafe file. Its contents were preserved.".into(),
            );
        }
        let mut bytes = Vec::new();
        file.take(65537)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 65536 {
            return Err("GNOME helper file is too large".into());
        }
        hashes.insert(name, format!("{:x}", Sha256::digest(&bytes)));
    }
    Ok(HelperSnapshot {
        device: directory.dev(),
        inode: directory.ino(),
        hashes,
    })
}

fn rename_helper(from: &Path, to: &Path, flags: u32) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    let from = std::ffi::CString::new(from.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let to = std::ffi::CString::new(to.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    if unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            flags,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

fn install_gnome(home: &Path) -> Result<(), String> {
    owned_directory(home)?;
    let mut parent = home.to_path_buf();
    for part in [".local", "share", "gnome-shell", "extensions"] {
        parent.push(part);
        match fs::DirBuilder::new().mode(0o700).create(&parent) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.to_string()),
        }
        owned_directory(&parent)?;
    }
    let dest = parent.join("winrects@cua");
    let files = [
        ("extension.js", EXTENSION),
        ("metadata.json", METADATA),
        ("history-pointer.js", HISTORY_POINTER),
        ("LICENSE", LICENSE),
    ];
    let previous = if dest.symlink_metadata().is_ok() {
        let snapshot = helper_snapshot(&dest)?;
        let current: BTreeMap<_, _> = files
            .iter()
            .map(|(n, b)| (n.to_string(), format!("{:x}", Sha256::digest(b))))
            .collect();
        if snapshot.hashes == current {
            return Ok(());
        }
        let known = [PREVIOUS_GNOME.as_slice(), PREVIOUS_GNOME_V10.as_slice()]
            .iter().any(|files| snapshot.hashes == files.iter()
                .map(|(n, h)| (n.to_string(), h.to_string())).collect());
        if !known {
            return Err("A different GNOME helper is installed. Its contents were preserved; review it before approving this scope again.".into());
        }
        Some(snapshot)
    } else {
        None
    };
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let staging = parent.join(format!(".winrects-copilot-{}-{nonce}", std::process::id()));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&staging)
        .map_err(|e| e.to_string())?;
    // Once exchanged, this directory holds the previous bundle. Keep it as a
    // recoverable backup, including on a verification or sync error.
    let mut preserve_staging = false;
    let result = (|| {
        for (name, bytes) in files {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(staging.join(name))
                .map_err(|e| e.to_string())?;
            file.write_all(bytes).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
        }
        fs::File::open(&staging)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        let staged = helper_snapshot(&staging)?;
        if let Some(previous) = &previous {
            if helper_snapshot(&dest)? != *previous {
                return Err(
                    "GNOME helper changed during setup. Try again after reviewing it.".into(),
                );
            }
            rename_helper(&staging, &dest, libc::RENAME_EXCHANGE)?;
            preserve_staging = true;
            // Detect another directory arriving between validation and rename.
            // Both complete directories survive; never delete an unknown one.
            if helper_snapshot(&staging).as_ref() != Ok(previous) {
                if helper_snapshot(&dest).as_ref() != Ok(&staged) {
                    return Err("GNOME helper changed during setup. Both folders were preserved for review.".into());
                }
                rename_helper(&staging, &dest, libc::RENAME_EXCHANGE)?;
                preserve_staging = helper_snapshot(&staging).as_ref() != Ok(&staged);
                return Err(
                    "GNOME helper changed during setup. The previous folder was restored.".into(),
                );
            }
        } else {
            rename_helper(&staging, &dest, libc::RENAME_NOREPLACE)?;
        }
        fs::File::open(&parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() && !preserve_staging {
        let _ = fs::remove_dir_all(staging);
    }
    result
}
fn remember_gnome_approval(settings: &gio::Settings) -> Result<(), String> {
    use gio::prelude::*;
    const UUID: &str = "winrects@cua";
    // A freshly installed extension is not discovered until the next login.
    // EnableExtension then returns false and does not remember the approval.
    // Persist only this helper's choice, preserving other extensions and the
    // user's global extensions switch. GNOME loads it after discovery.
    let mut enabled: Vec<String> = settings
        .strv("enabled-extensions")
        .iter()
        .map(|s| s.to_string())
        .collect();
    if !enabled.iter().any(|s| s == UUID) {
        enabled.push(UUID.into());
        settings
            .set_strv("enabled-extensions", enabled.as_slice())
            .map_err(|e| e.to_string())?;
    }
    let disabled: Vec<String> = settings
        .strv("disabled-extensions")
        .iter()
        .filter(|s| s.as_str() != UUID)
        .map(|s| s.to_string())
        .collect();
    settings
        .set_strv("disabled-extensions", disabled.as_slice())
        .map_err(|e| e.to_string())?;
    gio::Settings::sync();
    Ok(())
}

fn enable_gnome_with(
    mut desktop_enable: impl FnMut() -> bool,
    remember_default_approval: impl FnOnce() -> Result<(), String>,
    custom_settings: bool,
) -> Result<String, String> {
    const READY: &str =
        "GNOME helper enabled. If capture is unavailable, sign out and sign back in.";
    // Let GNOME update its own configuration first. An app-local dconf read
    // cache can differ from the desktop database that receives its writes.
    if desktop_enable() {
        return Ok(READY.into());
    }
    if custom_settings {
        return Ok(
            "GNOME helper installed. Sign out and back in, then set up desktop capture again."
                .into(),
        );
    }
    remember_default_approval()?;
    Ok(if desktop_enable() {
        READY.into()
    } else {
        "GNOME helper approved for your account. Sign out and sign back in to load it.".into()
    })
}

fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            if !status.success() {
                return Err(format!("{program} could not enable the helper"));
            }
            let output = child.wait_with_output().map_err(|e| e.to_string())?;
            return Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Desktop helper setup timed out".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn remember_kwin_with(
    desktop_remember: impl FnOnce() -> bool,
    remember_default_approval: impl FnOnce() -> Result<(), String>,
    custom_settings: bool,
) -> Result<String, String> {
    if desktop_remember() {
        return Ok("KWin helper enabled for your desktop account.".into());
    }
    if custom_settings || remember_default_approval().is_err() {
        return Ok("KWin helper enabled for this session. Enable it in Desktop Effects to keep it enabled.".into());
    }
    Ok("KWin helper enabled for your desktop account.".into())
}
pub fn enable() -> Result<String, String> {
    if !computer_history::linux::is_wayland() {
        return Ok(String::new());
    }
    if unsafe { libc::geteuid() } == 0 {
        return Err("Enable History from your desktop account.".into());
    }
    let desktop = desktop();
    if desktop.contains("gnome") {
        let home = std::env::var_os("HOME").ok_or("Your home folder is unavailable")?;
        install_gnome(Path::new(&home))?;
        // The CLI can use the same app-local dconf cache. Call the Shell's
        // service directly so it merges the choice in its own configuration.
        let desktop_enable = || {
            run(
                "gdbus",
                &[
                    "call",
                    "--session",
                    "--dest",
                    "org.gnome.Shell",
                    "--object-path",
                    "/org/gnome/Shell",
                    "--method",
                    "org.gnome.Shell.Extensions.EnableExtension",
                    "winrects@cua",
                ],
            )
            .is_ok_and(|s| s == "(true,)")
        };
        let custom_settings = ["XDG_CONFIG_HOME", "GSETTINGS_BACKEND", "DCONF_PROFILE"]
            .iter()
            .any(|name| std::env::var_os(name).is_some());
        return enable_gnome_with(
            desktop_enable,
            || {
                let schema = gio::SettingsSchemaSource::default()
                    .and_then(|s| s.lookup("org.gnome.shell", true))
                    .ok_or("GNOME extension settings are unavailable")?;
                let settings =
                    gio::Settings::new_full(&schema, None::<&gio::SettingsBackend>, None);
                remember_gnome_approval(&settings)
            },
            custom_settings,
        );
    }
    if desktop.contains("kde") {
        // Upstream supplies an ABI-specific effect, not a KWin JS script. The
        // distribution package installs it into KWin's native plugin directory.
        let already_loaded = run(
            "gdbus",
            &[
                "call",
                "--session",
                "--dest",
                "org.kde.KWin",
                "--object-path",
                "/Effects",
                "--method",
                "org.kde.kwin.Effects.isEffectLoaded",
                "cua_kwin_target_helper",
            ],
        )
        .is_ok_and(|s| s == "(true,)");
        let loaded = if already_loaded {
            "(true,)".into()
        } else {
            run(
                "gdbus",
                &[
                    "call",
                    "--session",
                    "--dest",
                    "org.kde.KWin",
                    "--object-path",
                    "/Effects",
                    "--method",
                    "org.kde.kwin.Effects.loadEffect",
                    "cua_kwin_target_helper",
                ],
            )?
        };
        if loaded != "(true,)" {
            return Err(
                "KWin helper could not load. Install the helper package for your Plasma version."
                    .into(),
            );
        }
        let desktop_remember = || {
            run(
                "gdbus",
                &[
                    "call",
                    "--session",
                    "--dest",
                    "org.cua.KWinTarget",
                    "--object-path",
                    "/org/cua/KWinTarget",
                    "--method",
                    "org.cua.KWinTarget.RememberHistoryApproval",
                ],
            )
            .is_ok_and(|value| value == "(true,)")
        };
        return remember_kwin_with(
            desktop_remember,
            || {
                run(
                    "kwriteconfig6",
                    &[
                        "--file",
                        "kwinrc",
                        "--group",
                        "Plugins",
                        "--key",
                        "cua_kwin_target_helperEnabled",
                        "true",
                    ],
                )
                .map(|_| ())
            },
            std::env::var_os("XDG_CONFIG_HOME").is_some(),
        );
    }
    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kwin_approval_is_persisted_by_the_actual_desktop() {
        let notice = remember_kwin_with(
            || true,
            || panic!("app-local config must not be used"),
            true,
        )
        .unwrap();
        assert!(notice.contains("desktop account"));
    }
    #[test]
    fn older_kwin_helper_with_private_config_reports_session_only() {
        let notice = remember_kwin_with(
            || false,
            || panic!("isolated config would lose persistence"),
            true,
        )
        .unwrap();
        assert!(notice.contains("for this session"));
    }
    #[test]
    fn default_kwin_configuration_keeps_compatible_setup_and_reports_errors() {
        assert!(remember_kwin_with(|| false, || Ok(()), false)
            .unwrap()
            .contains("desktop account"));
        assert!(
            remember_kwin_with(|| false, || Err("write failed".into()), false)
                .unwrap()
                .contains("for this session")
        );
    }
    fn previous_install(label: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let home = std::env::temp_dir().join(format!("icu-helper-{label}-{}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&home).unwrap();
        install_gnome(&home).unwrap();
        let dest = home.join(".local/share/gnome-shell/extensions/winrects@cua");
        fs::write(
            dest.join("extension.js"),
            include_bytes!("../test-fixtures/gnome-v9/extension.js"),
        )
        .unwrap();
        fs::write(
            dest.join("metadata.json"),
            include_bytes!("../test-fixtures/gnome-v9/metadata.json"),
        )
        .unwrap();
        fs::remove_file(dest.join("history-pointer.js")).unwrap();
        (home, dest)
    }
    #[test]
    fn known_previous_bundle_upgrades_atomically_and_keeps_backup() {
        let (home, dest) = previous_install("upgrade");
        let previous = helper_snapshot(&dest).unwrap();
        install_gnome(&home).unwrap();
        assert_eq!(
            fs::read(dest.join("history-pointer.js")).unwrap(),
            HISTORY_POINTER
        );
        assert_eq!(fs::read(dest.join("extension.js")).unwrap(), EXTENSION);
        let backups: Vec<_> = fs::read_dir(dest.parent().unwrap())
            .unwrap()
            .map(|p| p.unwrap().path())
            .filter(|p| p != &dest)
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(helper_snapshot(&backups[0]).unwrap(), previous);
        install_gnome(&home).unwrap();
        assert_eq!(fs::read_dir(dest.parent().unwrap()).unwrap().count(), 2);
        fs::remove_dir_all(home).unwrap();
    }
    fn previous_v10_install(label: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let (home, dest) = previous_install(label);
        fs::write(dest.join("extension.js"), include_bytes!("../test-fixtures/gnome-v10/extension.js")).unwrap();
        fs::write(dest.join("metadata.json"), include_bytes!("../test-fixtures/gnome-v10/metadata.json")).unwrap();
        fs::write(dest.join("history-pointer.js"), include_bytes!("../test-fixtures/gnome-v10/history-pointer.js")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dest.join("history-pointer.js"), fs::Permissions::from_mode(0o600)).unwrap();
        (home, dest)
    }
    #[test]
    fn pointer_helper_upgrades_without_losing_previous_bytes() {
        let (home, dest) = previous_v10_install("upgrade-v10");
        let previous = helper_snapshot(&dest).unwrap();
        install_gnome(&home).unwrap();
        assert_eq!(fs::read(dest.join("history-pointer.js")).unwrap(), HISTORY_POINTER);
        let backups: Vec<_> = fs::read_dir(dest.parent().unwrap()).unwrap()
            .map(|p| p.unwrap().path()).filter(|p| p != &dest).collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(helper_snapshot(&backups[0]).unwrap(), previous);
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn modified_pointer_helper_is_not_replaced() {
        let (home, dest) = previous_v10_install("modified-v10");
        fs::write(dest.join("history-pointer.js"), b"different pointer code").unwrap();
        let previous = helper_snapshot(&dest).unwrap();
        assert!(install_gnome(&home).is_err());
        assert_eq!(helper_snapshot(&dest).unwrap(), previous);
        assert_eq!(fs::read_dir(dest.parent().unwrap()).unwrap().count(), 1);
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn previous_version_label_cannot_authorize_modified_bytes() {
        let (home, dest) = previous_install("modified");
        fs::write(dest.join("extension.js"), b"different code, same version").unwrap();
        let before = helper_snapshot(&dest).unwrap();
        assert!(install_gnome(&home).is_err());
        assert_eq!(helper_snapshot(&dest).unwrap(), before);
        assert_eq!(fs::read_dir(dest.parent().unwrap()).unwrap().count(), 1);
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn previous_bundle_with_additions_is_preserved() {
        let (home, dest) = previous_install("extra");
        fs::write(dest.join("user-notes.txt"), b"keep this file").unwrap();
        assert!(install_gnome(&home).is_err());
        assert_eq!(
            fs::read(dest.join("user-notes.txt")).unwrap(),
            b"keep this file"
        );
        assert_eq!(
            fs::read(dest.join("extension.js")).unwrap(),
            include_bytes!("../test-fixtures/gnome-v9/extension.js")
        );
        assert_eq!(fs::read_dir(dest.parent().unwrap()).unwrap().count(), 1);
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn previous_bundle_with_linked_file_is_preserved() {
        let (home, dest) = previous_install("linked");
        fs::hard_link(dest.join("extension.js"), home.join("linked.js")).unwrap();
        assert!(install_gnome(&home).is_err());
        assert_eq!(
            fs::read(home.join("linked.js")).unwrap(),
            include_bytes!("../test-fixtures/gnome-v9/extension.js")
        );
        assert_eq!(fs::read_dir(dest.parent().unwrap()).unwrap().count(), 1);
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn private_app_cache_cannot_replace_desktop_extension_choices() {
        use std::cell::RefCell;
        let desktop = RefCell::new(vec!["unrelated@owner", "icu-input-test@owner"]);
        let private_app_cache = vec!["winrects@cua"];
        enable_gnome_with(
            || {
                desktop.borrow_mut().push("winrects@cua");
                true
            },
            || {
                *desktop.borrow_mut() = private_app_cache;
                Ok(())
            },
            true,
        )
        .unwrap();
        assert_eq!(
            *desktop.borrow(),
            ["unrelated@owner", "icu-input-test@owner", "winrects@cua"]
        );
    }
    #[test]
    fn undiscovered_helper_never_writes_from_a_private_config() {
        use std::cell::RefCell;
        let desktop = RefCell::new(vec!["unrelated@owner"]);
        let notice = enable_gnome_with(
            || false,
            || {
                *desktop.borrow_mut() = vec!["winrects@cua"];
                Ok(())
            },
            true,
        )
        .unwrap();
        assert_eq!(*desktop.borrow(), ["unrelated@owner"]);
        assert!(notice.contains("set up desktop capture again"));
    }
    #[test]
    fn approval_survives_discovery_and_preserves_other_extension_choices() {
        use gio::prelude::*;
        let dir = std::env::temp_dir().join(format!("icu-gnome-schema-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        fs::write(
            dir.join("test.gschema.xml"),
            r#"<schemalist>
          <schema id="org.gnome.shell" path="/org/gnome/shell/">
            <key name="enabled-extensions" type="as"><default>[]</default></key>
            <key name="disabled-extensions" type="as"><default>[]</default></key>
            <key name="disable-user-extensions" type="b"><default>false</default></key>
          </schema></schemalist>"#,
        )
        .unwrap();
        assert!(Command::new("glib-compile-schemas")
            .arg(&dir)
            .status()
            .unwrap()
            .success());
        let schema = gio::SettingsSchemaSource::from_directory(&dir, None, false)
            .unwrap()
            .lookup("org.gnome.shell", false)
            .unwrap();
        let backend = gio::memory_settings_backend_new();
        let settings = gio::Settings::new_full(&schema, Some(&backend), None);
        settings
            .set_strv("enabled-extensions", ["unrelated@owner"])
            .unwrap();
        settings
            .set_strv("disabled-extensions", ["disabled@owner", "winrects@cua"])
            .unwrap();
        settings
            .set_boolean("disable-user-extensions", true)
            .unwrap();
        remember_gnome_approval(&settings).unwrap();
        remember_gnome_approval(&settings).unwrap();
        let reopened = gio::Settings::new_full(&schema, Some(&backend), None);
        assert_eq!(
            reopened.strv("enabled-extensions").as_slice(),
            ["unrelated@owner", "winrects@cua"]
        );
        assert_eq!(
            reopened.strv("disabled-extensions").as_slice(),
            ["disabled@owner"]
        );
        assert!(reopened.boolean("disable-user-extensions"));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn installation_preserves_unknown_files_and_refuses_links() {
        use std::os::unix::fs::symlink;
        let home = std::env::temp_dir().join(format!("icu-helper-test-{}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&home).unwrap();
        install_gnome(&home).unwrap();
        install_gnome(&home).unwrap();
        let extension = home.join(".local/share/gnome-shell/extensions/winrects@cua/extension.js");
        fs::write(&extension, b"different extension").unwrap();
        assert!(install_gnome(&home).is_err());
        assert_eq!(fs::read(&extension).unwrap(), b"different extension");
        fs::remove_file(&extension).unwrap();
        let outside = home.join("outside");
        fs::write(&outside, EXTENSION).unwrap();
        symlink(&outside, &extension).unwrap();
        assert!(install_gnome(&home).is_err());
        fs::remove_dir_all(home).unwrap();
    }
}
