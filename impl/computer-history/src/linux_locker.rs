//! Desktop locker interfaces supplement, never replace, the live logind gate.
use super::{ax, process_start, refusal, Result};
use std::{os::unix::fs::MetadataExt, time::Duration};
use zbus::{fdo::DBusProxy, zvariant::OwnedObjectPath, Connection, Proxy};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Desktop {
    Gnome,
    Kde,
    Other,
}
impl Desktop {
    fn parse(value: &str) -> Result<Self> {
        let names: Vec<_> = value.split(':').map(str::to_ascii_lowercase).collect();
        let gnome = names.iter().any(|s| s == "gnome");
        let kde = names.iter().any(|s| matches!(s.as_str(), "kde" | "plasma"));
        match (gnome, kde) {
            (true, false) => Ok(Self::Gnome),
            (false, true) => Ok(Self::Kde),
            (false, false) => Ok(Self::Other),
            _ => Err(unknown()),
        }
    }
    fn authority(self) -> Option<&'static str> {
        match self {
            Self::Gnome => Some("org.gnome.ScreenSaver"),
            Self::Kde => Some("org.freedesktop.ScreenSaver"),
            Self::Other => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reply {
    Active(bool),
    NotSupported,
    Unknown,
}
fn unknown() -> super::Error {
    refusal(
        "computerHistorySessionUnknown",
        "The desktop locker state or owner could not be verified",
    )
}
fn response(result: std::result::Result<bool, zbus::Error>) -> Reply {
    match result {
        Ok(active) => Reply::Active(active),
        Err(zbus::Error::MethodError(name, _, _))
            if name.as_str() == "org.freedesktop.DBus.Error.NotSupported" =>
        {
            Reply::NotSupported
        }
        _ => Reply::Unknown,
    }
}
fn evaluate(desktop: Desktop, replies: &[(&str, Reply)]) -> Result<bool> {
    // A positive lock signal wins even if another interface is broken.
    if replies.iter().any(|(_, r)| *r == Reply::Active(true)) {
        return Ok(true);
    }
    if let Some(authority) = desktop.authority() {
        if !replies.contains(&(authority, Reply::Active(false))) {
            return Err(unknown());
        }
    }
    for (name, reply) in replies {
        match reply {
            Reply::Active(false) => {}
            Reply::NotSupported
                if desktop == Desktop::Gnome && *name == "org.freedesktop.ScreenSaver" => {}
            _ => return Err(unknown()),
        }
    }
    Ok(false)
}
fn executable_allowed(name: &str, exe: &str) -> bool {
    match name {
        "org.gnome.ScreenSaver" => exe == "gnome-shell",
        "org.freedesktop.ScreenSaver" => matches!(
            exe,
            "gnome-shell" | "gsd-screensaver-proxy" | "ksmserver" | "kwin_x11" | "kwin_wayland"
        ),
        "org.mate.ScreenSaver" => exe == "mate-screensaver",
        "org.cinnamon.ScreenSaver" => exe == "cinnamon-screensaver",
        "org.xfce.ScreenSaver" => exe == "xfce4-screensaver",
        _ => false,
    }
}
fn root_file(uid: u32, mode: u32, file: bool) -> bool {
    file && uid == 0 && mode & 0o022 == 0
}
fn root_owned(path: impl AsRef<std::path::Path>) -> bool {
    std::fs::metadata(path).is_ok_and(|m| root_file(m.uid(), m.mode(), m.is_file()))
}
fn gnome_proxy(exe: &str, args: &[&str]) -> bool {
    matches!(exe, "gjs-console" | "gjs")
        && args.len() == 3
        && args[1] == "-m"
        && args[2] == "/usr/share/gnome-shell/org.gnome.ScreenSaver"
}
fn trusted_executable(pid: u32, name: &str) -> bool {
    let proc = format!("/proc/{pid}/exe");
    let Ok(path) = std::fs::read_link(&proc) else {
        return false;
    };
    let Some(exe) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    if !root_owned(&proc) {
        return false;
    }
    if executable_allowed(name, exe) {
        return true;
    }
    if name != "org.gnome.ScreenSaver" {
        return false;
    }
    let Ok(raw) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    if raw.len() > 4096 {
        return false;
    }
    let Ok(raw) = std::str::from_utf8(&raw) else {
        return false;
    };
    let args: Vec<_> = raw.trim_end_matches('\0').split('\0').collect();
    gnome_proxy(exe, &args) && root_owned(args[2])
}
fn gnome_delegate(shell: &str, shield: &str, exe: Option<&str>) -> bool {
    shell == shield && exe == Some("gnome-shell")
}
fn stable_owner(owner: &str, current: &str, start: &str, current_start: Option<&str>) -> bool {
    owner == current && current_start == Some(start)
}
fn owner_matches(uid: u32, ours: u32, session: &str, mine: &str, trusted: bool) -> bool {
    uid == ours && session == mine && trusted
}

fn requires_kwin(desktop: Desktop, wayland: bool, name: &str) -> bool {
    desktop == Desktop::Kde && wayland && name == "org.freedesktop.ScreenSaver"
}

fn kwin_owner_matches(
    owner: &str,
    expected: &str,
    pid: Option<u32>,
    uid: Option<u32>,
    server: u32,
) -> bool {
    owner.starts_with(':')
        && owner == expected
        && pid == Some(server)
        && uid == Some(unsafe { libc::geteuid() })
}

// Keep the complete Wayland identity (including both pidfds) alive across
// GetActive. A known executable name or a same-UID bus owner is not a fallback
// for this branch: the locker must be the verified KWin reply server itself.
struct KwinLocker {
    identity: super::kwin::Identity,
    desktop_owner: String,
}
impl KwinLocker {
    async fn verify(&self, bus: &DBusProxy<'_>, owner: &str) -> Result<()> {
        for (name, expected) in [
            ("org.freedesktop.ScreenSaver", owner),
            ("org.kde.KWin", self.desktop_owner.as_str()),
        ] {
            let current = ax::call(bus.get_name_owner(name.try_into().unwrap())).await?;
            let credentials = ax::call(
                bus.get_connection_credentials(expected.try_into().map_err(|_| unknown())?),
            )
            .await?;
            if !kwin_owner_matches(
                current.as_str(),
                expected,
                credentials.process_id(),
                credentials.unix_user_id(),
                self.identity.pid(),
            ) {
                return Err(unknown());
            }
        }
        if !self.identity.unchanged() {
            return Err(unknown());
        }
        Ok(())
    }
}

async fn checked_active<F: std::future::Future<Output = Result<()>>>(
    mut verify: impl FnMut() -> F,
    call: impl std::future::Future<Output = std::result::Result<bool, zbus::Error>>,
) -> Result<Reply> {
    verify().await?;
    let reply = match tokio::time::timeout(Duration::from_millis(150), call).await {
        Ok(result) => response(result),
        Err(_) => Reply::Unknown,
    };
    verify().await?;
    Ok(reply)
}

fn user_service_scope(
    cgroup: &str,
    uid: u32,
    process_uid: u32,
    display: &str,
    sessions: &[(String, String)],
) -> bool {
    let prefix = format!("0::/user.slice/user-{uid}.slice/user@{uid}.service/");
    // Newer systemd versions register the user manager as its own virtual
    // session. Only those explicitly noninteractive classes can be excluded;
    // another login (including a background/unknown class) remains ambiguous.
    let mut logins = sessions
        .iter()
        .filter(|(_, class)| !matches!(class.as_str(), "manager" | "manager-early"));
    let single_display = logins.next().is_some_and(|(path, class)| {
        path == display
            && matches!(
                class.as_str(),
                "user" | "user-early" | "user-light" | "user-early-light"
            )
    }) && logins.next().is_none();
    cgroup.lines().any(|line| line.starts_with(&prefix)) && uid == process_uid && single_display
}
fn no_pid_session(error: &zbus::Error) -> bool {
    matches!(error, zbus::Error::MethodError(name, _, _) if name.as_str() == "org.freedesktop.login1.NoSessionForPID")
}
pub(super) async fn process_session(manager: &Proxy<'_>, pid: u32) -> Result<OwnedObjectPath> {
    let session = tokio::time::timeout(
        Duration::from_millis(150),
        manager.call::<_, _, OwnedObjectPath>("GetSessionByPID", &(pid,)),
    )
    .await
    .map_err(|_| unknown())?;
    match session {
        Ok(session) => return Ok(session),
        Err(error) if no_pid_session(&error) => {}
        Err(_) => return Err(unknown()),
    }
    let uid = unsafe { libc::geteuid() };
    // GNOME starts desktop applications, its shell and services in the per-user
    // manager. They have no PID session. Accept that explicit cgroup only when
    // logind associates the user with exactly this single login display session.
    let user: OwnedObjectPath = ax::call(manager.call("GetUser", &(uid,))).await?;
    let proxy = ax::call(
        zbus::proxy::Builder::<Proxy>::new(manager.connection())
            .destination("org.freedesktop.login1")
            .unwrap()
            .path(user)
            .unwrap()
            .interface("org.freedesktop.login1.User")
            .unwrap()
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build(),
    )
    .await?;
    let (_, display): (String, OwnedObjectPath) = ax::call(proxy.get_property("Display")).await?;
    let sessions: Vec<(String, OwnedObjectPath)> = ax::call(proxy.get_property("Sessions")).await?;
    let mut classified = Vec::with_capacity(sessions.len());
    for (_, path) in sessions {
        let session = ax::call(
            zbus::proxy::Builder::<Proxy>::new(manager.connection())
                .destination("org.freedesktop.login1")
                .unwrap()
                .path(path.clone())
                .unwrap()
                .interface("org.freedesktop.login1.Session")
                .unwrap()
                .cache_properties(zbus::proxy::CacheProperties::No)
                .build(),
        )
        .await?;
        // A failed/absent Class is unknown, never an ignorable virtual session.
        let class: String = ax::call(session.get_property("Class")).await?;
        classified.push((path.to_string(), class));
    }
    let cgroup = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).map_err(|_| unknown())?;
    let process_uid = std::fs::metadata(format!("/proc/{pid}"))
        .map_err(|_| unknown())?
        .uid();
    if !user_service_scope(&cgroup, uid, process_uid, display.as_str(), &classified) {
        return Err(unknown());
    }
    Ok(display)
}
pub(super) async fn active(
    connection: &Connection,
    manager: &Proxy<'_>,
    mine: &OwnedObjectPath,
) -> Result<bool> {
    let desktop = Desktop::parse(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default())?;
    let bus = ax::call(DBusProxy::new(connection)).await?;
    let mut replies = Vec::new();
    let mut delegates = Vec::new();
    for (name, path) in super::LOCKERS {
        if !ax::call(bus.name_has_owner((*name).try_into().unwrap())).await? {
            if desktop.authority() != Some(*name) {
                continue;
            }
            // GNOME's standard proxy is D-Bus activated. Activation is not admission:
            // credentials, executable, delegate and live state must still be checked.
            ax::call(bus.start_service_by_name((*name).try_into().unwrap(), 0)).await?;
        }
        let owner = ax::call(bus.get_name_owner((*name).try_into().unwrap())).await?;
        let uid =
            ax::call(bus.get_connection_unix_user(owner.as_str().try_into().unwrap())).await?;
        let pid = ax::call(bus.get_connection_unix_process_id(owner.as_str().try_into().unwrap()))
            .await?;
        let start = process_start(pid).ok_or_else(unknown)?;
        let kwin = if requires_kwin(desktop, super::is_wayland(), name) {
            let identity = super::kwin::Identity::connect().ok_or_else(unknown)?;
            if pid != identity.pid()
                || process_session(manager, identity.listener_pid()).await? != *mine
            {
                return Err(unknown());
            }
            let desktop_owner =
                ax::call(bus.get_name_owner("org.kde.KWin".try_into().unwrap())).await?;
            Some(KwinLocker {
                identity,
                desktop_owner: desktop_owner.to_string(),
            })
        } else {
            None
        };
        if uid != unsafe { libc::geteuid() } || (kwin.is_none() && !trusted_executable(pid, name)) {
            return Err(unknown());
        }
        if *name == "org.gnome.ScreenSaver" {
            // Recent GNOME exposes the public API from a system-installed GJS
            // proxy. Its ScreenShield delegate must be the actual session shell.
            let shell = ax::call(bus.get_name_owner("org.gnome.Shell".try_into().unwrap())).await?;
            let shield =
                ax::call(bus.get_name_owner("org.gnome.Shell.ScreenShield".try_into().unwrap()))
                    .await?;
            let shell_pid =
                ax::call(bus.get_connection_unix_process_id(shell.as_str().try_into().unwrap()))
                    .await?;
            let shell_uid =
                ax::call(bus.get_connection_unix_user(shell.as_str().try_into().unwrap())).await?;
            let shell_start = process_start(shell_pid).ok_or_else(unknown)?;
            let shell_path =
                std::fs::read_link(format!("/proc/{shell_pid}/exe")).map_err(|_| unknown())?;
            if !gnome_delegate(
                shell.as_str(),
                shield.as_str(),
                shell_path.file_name().and_then(|s| s.to_str()),
            ) {
                return Err(unknown());
            }
            delegates.push((
                "org.gnome.Shell",
                shell.to_string(),
                shell_pid,
                shell_start.clone(),
            ));
            delegates.push((
                "org.gnome.Shell.ScreenShield",
                shield.to_string(),
                shell_pid,
                shell_start,
            ));
            let shell_session = process_session(manager, shell_pid).await?;
            if !owner_matches(
                shell_uid,
                uid,
                shell_session.as_str(),
                mine.as_str(),
                trusted_executable(shell_pid, name),
            ) {
                return Err(unknown());
            }
        } else if process_session(manager, pid).await? != *mine {
            return Err(unknown());
        }
        // Call the checked immutable owner, not a replaceable well-known name.
        let proxy = ax::call(Proxy::new(connection, owner.as_str(), *path, *name)).await?;
        let reply = checked_active(
            || async {
                if let Some(kwin) = &kwin {
                    kwin.verify(&bus, owner.as_str()).await?;
                }
                Ok(())
            },
            proxy.call::<_, _, bool>("GetActive", &()),
        )
        .await?;
        if let Some(kwin) = &kwin {
            if process_session(manager, pid).await? != *mine
                || process_session(manager, kwin.identity.listener_pid()).await? != *mine
            {
                return Err(unknown());
            }
        }
        let current = ax::call(bus.get_name_owner((*name).try_into().unwrap())).await?;
        if !stable_owner(
            owner.as_str(),
            current.as_str(),
            &start,
            process_start(pid).as_deref(),
        ) {
            return Err(unknown());
        }
        replies.push((*name, reply));
    }
    // The standard proxy forwards to a well-known Shell name. Recheck its
    // delegate after the call so a replaced connection cannot inherit admission.
    for (name, owner, pid, start) in delegates {
        let current = ax::call(bus.get_name_owner(name.try_into().unwrap())).await?;
        if !stable_owner(
            &owner,
            current.as_str(),
            &start,
            process_start(pid).as_deref(),
        ) {
            return Err(unknown());
        }
    }
    evaluate(desktop, &replies)
}

#[cfg(test)]
mod tests {
    use super::*;
    const GNOME: &str = "org.gnome.ScreenSaver";
    const FDO: &str = "org.freedesktop.ScreenSaver";
    #[test]
    fn locker_identity_is_rechecked_around_get_active() {
        use std::cell::Cell;
        for failed_check in [None, Some(0), Some(1)] {
            let checks = Cell::new(0);
            let called = Cell::new(false);
            let reply = ax::run(checked_active(
                || {
                    let index = checks.get();
                    checks.set(index + 1);
                    std::future::ready(if failed_check == Some(index) {
                        Err(unknown())
                    } else {
                        Ok(())
                    })
                },
                async {
                    called.set(true);
                    Ok(false)
                },
            ));
            if failed_check.is_none() {
                assert_eq!(reply.unwrap(), Reply::Active(false));
                assert_eq!(checks.get(), 2);
            } else {
                assert!(reply.is_err());
            }
            assert_eq!(called.get(), failed_check != Some(0));
        }
    }
    #[test]
    fn kwin_identity_is_required_only_for_kde_wayland_standard_locker() {
        assert!(requires_kwin(Desktop::Kde, true, FDO));
        for (desktop, wayland, name) in [
            (Desktop::Kde, false, FDO),
            (Desktop::Gnome, true, FDO),
            (Desktop::Other, true, FDO),
            (Desktop::Kde, true, GNOME),
            (Desktop::Kde, true, "org.mate.ScreenSaver"),
        ] {
            assert!(!requires_kwin(desktop, wayland, name));
        }
    }
    #[test]
    fn kwin_locker_requires_exact_server_credentials() {
        let uid = Some(unsafe { libc::geteuid() });
        assert!(kwin_owner_matches(":1.2", ":1.2", Some(42), uid, 42));
        // A different process of the same user cannot inherit locker authority,
        // even when it owns the well-known name before our first observation.
        assert!(!kwin_owner_matches(":1.2", ":1.2", Some(43), uid, 42));
        assert!(!kwin_owner_matches(":1.2", ":1.2", None, uid, 42));
        assert!(!kwin_owner_matches(":1.2", ":1.2", Some(42), None, 42));
        assert!(!kwin_owner_matches(
            ":1.2",
            ":1.2",
            Some(42),
            Some(uid.unwrap() + 1),
            42
        ));
    }
    #[test]
    fn kwin_locker_replaced_connection_refuses_even_with_same_pid() {
        let uid = Some(unsafe { libc::geteuid() });
        assert!(!kwin_owner_matches(":1.3", ":1.2", Some(42), uid, 42));
        assert!(!kwin_owner_matches(FDO, FDO, Some(42), uid, 42));
    }
    #[test]
    fn gnome_requires_authority_before_ignoring_unsupported_compatibility() {
        assert_eq!(
            evaluate(
                Desktop::Gnome,
                &[(GNOME, Reply::Active(false)), (FDO, Reply::NotSupported)]
            )
            .unwrap(),
            false
        );
        for replies in [
            vec![],
            vec![(FDO, Reply::NotSupported)],
            vec![(GNOME, Reply::Unknown)],
            vec![(GNOME, Reply::NotSupported)],
            vec![(FDO, Reply::Active(false))],
        ] {
            assert!(evaluate(Desktop::Gnome, &replies).is_err());
        }
    }
    #[test]
    fn kde_requires_freedesktop_answer() {
        assert!(!evaluate(Desktop::Kde, &[(FDO, Reply::Active(false))]).unwrap());
        for replies in [
            vec![],
            vec![(FDO, Reply::NotSupported)],
            vec![(FDO, Reply::Unknown)],
            vec![(GNOME, Reply::Active(false))],
        ] {
            assert!(evaluate(Desktop::Kde, &replies).is_err());
        }
    }
    #[test]
    fn all_other_unknowns_refuse() {
        for desktop in [Desktop::Gnome, Desktop::Kde, Desktop::Other] {
            let mut replies = vec![(GNOME, Reply::Active(false)), (FDO, Reply::Active(false))];
            for reply in [Reply::Unknown, Reply::NotSupported] {
                replies.push(("org.mate.ScreenSaver", reply));
                assert!(evaluate(desktop, &replies).is_err());
                replies.pop();
            }
        }
        assert!(evaluate(Desktop::Other, &[(FDO, Reply::NotSupported)]).is_err());
        assert!(!evaluate(Desktop::Other, &[]).unwrap());
    }
    #[test]
    fn any_active_locker_wins() {
        for desktop in [Desktop::Gnome, Desktop::Kde, Desktop::Other] {
            for name in [GNOME, FDO, "org.mate.ScreenSaver"] {
                assert!(evaluate(
                    desktop,
                    &[(name, Reply::Active(true)), ("other", Reply::Unknown)]
                )
                .unwrap());
            }
        }
    }
    #[test]
    fn desktop_selection_is_unambiguous() {
        assert_eq!(Desktop::parse("ubuntu:GNOME").unwrap(), Desktop::Gnome);
        assert_eq!(Desktop::parse("KDE").unwrap(), Desktop::Kde);
        assert_eq!(Desktop::parse("sway").unwrap(), Desktop::Other);
        assert!(Desktop::parse("GNOME:KDE").is_err());
    }
    #[test]
    fn locker_code_must_be_a_root_owned_nonwritable_file() {
        assert!(root_file(0, 0o755, true));
        assert!(!root_file(1000, 0o755, true));
        assert!(!root_file(0, 0o775, true));
        assert!(!root_file(0, 0o757, true));
        assert!(!root_file(0, 0o755, false));
    }
    #[test]
    fn only_missing_pid_session_allows_the_user_manager_check() {
        let message = zbus::Message::method_call("/test", "GetSessionByPID")
            .unwrap()
            .build(&())
            .unwrap();
        for (name, expected) in [
            ("org.freedesktop.login1.NoSessionForPID", true),
            ("org.freedesktop.DBus.Error.AccessDenied", false),
            ("org.freedesktop.DBus.Error.Timeout", false),
        ] {
            assert_eq!(
                no_pid_session(&zbus::Error::MethodError(
                    name.try_into().unwrap(),
                    None,
                    message.clone()
                )),
                expected
            );
        }
        assert!(!no_pid_session(&zbus::Error::Failure(
            "disconnected".into()
        )));
    }
    #[test]
    fn user_manager_locker_requires_one_unambiguous_logind_display() {
        let group="0::/user.slice/user-1000.slice/user@1000.service/session.slice/org.gnome.Shell@wayland.service\n";
        let mine = "/org/freedesktop/login1/session/_31";
        assert!(user_service_scope(
            group,
            1000,
            1000,
            mine,
            &[(mine.into(), "user".into())]
        ));
        assert!(!user_service_scope(
            group,
            1001,
            1001,
            mine,
            &[(mine.into(), "user".into())]
        ));
        assert!(!user_service_scope(
            group,
            1000,
            1001,
            mine,
            &[(mine.into(), "user".into())]
        ));
        assert!(!user_service_scope(
            "0::/unrelated/",
            1000,
            1000,
            mine,
            &[(mine.into(), "user".into())]
        ));
        assert!(!user_service_scope(
            group,
            1000,
            1000,
            "/other",
            &[(mine.into(), "user".into())]
        ));
        assert!(!user_service_scope(group, 1000, 1000, mine, &[]));
        assert!(!user_service_scope(
            group,
            1000,
            1000,
            mine,
            &[
                (mine.into(), "user".into()),
                ("/other".into(), "user".into())
            ]
        ));
    }
    #[test]
    fn only_verified_manager_classes_are_excluded_from_login_ambiguity() {
        let group = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/test.scope";
        let mine = "/display";
        for class in ["manager", "manager-early"] {
            let sessions = [
                ("/manager".into(), class.into()),
                (mine.into(), "user".into()),
            ];
            assert!(user_service_scope(group, 1000, 1000, mine, &sessions));
            assert!(!user_service_scope(
                group, 1000, 1000, "/manager", &sessions
            ));
        }
        for class in ["user", "background", "greeter", "", "future-class"] {
            let sessions = [
                (mine.into(), "user".into()),
                ("/other".into(), class.into()),
            ];
            assert!(!user_service_scope(group, 1000, 1000, mine, &sessions));
        }
        for class in ["manager", "manager-early", "background", "", "future-class"] {
            assert!(!user_service_scope(
                group,
                1000,
                1000,
                mine,
                &[(mine.into(), class.into())]
            ));
        }
        for class in ["user", "user-early", "user-light", "user-early-light"] {
            assert!(user_service_scope(
                group,
                1000,
                1000,
                mine,
                &[(mine.into(), class.into())]
            ));
        }
    }
    #[test]
    fn gnome_proxy_is_the_fixed_system_script() {
        let args = [
            "/usr/bin/gjs",
            "-m",
            "/usr/share/gnome-shell/org.gnome.ScreenSaver",
        ];
        assert!(gnome_proxy("gjs-console", &args));
        assert!(!gnome_proxy("python", &args));
        assert!(!gnome_proxy("gjs-console", &args[..2]));
        assert!(!gnome_proxy("gjs-console", &[args[0], "-c", args[2]]));
        assert!(!gnome_proxy(
            "gjs-console",
            &[args[0], args[1], "/tmp/org.gnome.ScreenSaver"]
        ));
        assert!(!gnome_proxy(
            "gjs-console",
            &[args[0], args[1], args[2], "extra"]
        ));
    }
    #[test]
    fn gnome_delegate_must_be_the_actual_shell_connection() {
        assert!(gnome_delegate(":1.1", ":1.1", Some("gnome-shell")));
        assert!(!gnome_delegate(":1.1", ":1.2", Some("gnome-shell")));
        assert!(!gnome_delegate(":1.1", ":1.1", Some("gjs-console")));
        assert!(!gnome_delegate(":1.1", ":1.1", None));
    }
    #[test]
    fn owner_replacement_and_pid_reuse_refuse() {
        assert!(stable_owner(":1.1", ":1.1", "100", Some("100")));
        assert!(!stable_owner(":1.1", ":1.2", "100", Some("100")));
        assert!(!stable_owner(":1.1", ":1.1", "100", Some("101")));
        assert!(!stable_owner(":1.1", ":1.1", "100", None));
    }
    #[test]
    fn only_the_exact_not_supported_error_is_eligible() {
        let message = zbus::Message::method_call("/test", "GetActive")
            .unwrap()
            .build(&())
            .unwrap();
        assert_eq!(
            response(Err(zbus::Error::MethodError(
                "org.freedesktop.DBus.Error.NotSupported"
                    .try_into()
                    .unwrap(),
                None,
                message.clone()
            ))),
            Reply::NotSupported
        );
        assert_eq!(
            response(Err(zbus::Error::MethodError(
                "org.freedesktop.DBus.Error.UnknownMethod"
                    .try_into()
                    .unwrap(),
                None,
                message
            ))),
            Reply::Unknown
        );
        assert_eq!(
            response(Err(zbus::Error::Failure("unavailable".into()))),
            Reply::Unknown
        );
        assert_eq!(response(Ok(false)), Reply::Active(false));
        assert_eq!(response(Ok(true)), Reply::Active(true));
    }
    #[test]
    fn locker_owners_require_same_user_session_and_trusted_process() {
        assert!(owner_matches(1000, 1000, "session-a", "session-a", true));
        assert!(!owner_matches(1001, 1000, "session-a", "session-a", true));
        assert!(!owner_matches(1000, 1000, "session-b", "session-a", true));
        assert!(!owner_matches(1000, 1000, "session-a", "session-a", false));
        for (name, exe) in [
            (GNOME, "gnome-shell"),
            (FDO, "ksmserver"),
            ("org.mate.ScreenSaver", "mate-screensaver"),
            ("org.cinnamon.ScreenSaver", "cinnamon-screensaver"),
            ("org.xfce.ScreenSaver", "xfce4-screensaver"),
        ] {
            assert!(executable_allowed(name, exe));
            assert!(!executable_allowed(name, "fixture"));
            assert!(!executable_allowed(name, &format!("{exe} (deleted)")));
        }
        assert!(!trusted_executable(std::process::id(), GNOME));
    }
}
