//! An update of a `.deb` or `.rpm` install, checked before the plugin installs it.
//!
//! `tauri-plugin-updater` 2.12 installs those with `pkexec dpkg -i` or `pkexec rpm -U` and keeps only the exit status
//! (`PackageInstallFailed`), so a refusal showed as `Failed to install package` whatever the cause. Two causes matter:
//!
//! * an unmet dependency. `rpm -U` refuses it (`Failed dependencies`) and changes nothing. `dpkg -i` does not: it unpacks the
//!   new version over the installed one and leaves it "unpacked, not configured" (measured on Ubuntu 24.04: the binary of the old
//!   version was already replaced by the new one and `dpkg --audit` lists the package), then fails. Neither tells the user which
//!   package is missing in the app.
//! * everything else rpm refuses (a signature policy, a conflict): the generic text stays, with rpm's own reason when it has one.
//!
//! The check needs no privilege and runs before the plugin's install, so a dependency the machine lacks leaves the running app and
//! the installed version as they were and the toast says what is missing. rpm answers it exactly (`rpm -U --test`, which
//! also handles versions, rich dependencies and file dependencies); dpkg has no dry run that checks dependencies, so the new
//! deb's `Depends` and `Pre-Depends` are read with `dpkg-deb -f` and matched against the installed packages in
//! `/var/lib/dpkg/status` the way `dpkg -i` does (installed packages only, `Provides` included, versions through
//! `dpkg --compare-versions`; apt's resolution is not used because `apt-get -s` would fetch what `dpkg -i` cannot).
//! Everything that decides is a pure function and tested on any OS; only running the tools is Linux code.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind { Deb, Rpm }

/// The package format of downloaded bytes: an rpm starts with the lead magic `ED AB EE DB`, a deb is an `ar` archive.
pub fn kind_of(bytes: &[u8]) -> Option<Kind> {
    if bytes.starts_with(&[0xED, 0xAB, 0xEE, 0xDB]) { Some(Kind::Rpm) }
    else if bytes.starts_with(b"!<arch>\n") { Some(Kind::Deb) }
    else { None }
}

/// The kind this install was packaged as (the marker `tauri build` patches into the binary), on Linux.
#[cfg(target_os = "linux")]
pub fn this_install_kind() -> Option<Kind> {
    use tauri::utils::{config::BundleType, platform::bundle_type};
    match bundle_type() { Some(BundleType::Deb) => Some(Kind::Deb), Some(BundleType::Rpm) => Some(Kind::Rpm), _ => None }
}
#[cfg(not(target_os = "linux"))]
pub fn this_install_kind() -> Option<Kind> { None }

/// What rpm lists under `Failed dependencies:` in `rpm -U --test` (stderr): the requirement text, one per line, e.g.
/// `libwebkit2gtk-4.1.so.0()(64bit)`, `icu-nonexistent-dep`, `bash >= 99`, `(a or b)`.
pub fn rpm_unmet(stderr: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in stderr.lines() {
        if line.trim_start_matches("error: ").trim() == "Failed dependencies:" { inside = true; continue; }
        if !inside { continue; }
        if !line.starts_with(char::is_whitespace) { break; }
        if let Some((req, _)) = line.trim().split_once(" is needed by ") {
            let req = req.trim();
            if !req.is_empty() && !out.iter().any(|r| r == req) { out.push(req.to_string()); }
        }
    }
    out
}

/// One alternative of a dpkg dependency: `name[:arch] [(op version)]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alt { pub name: String, pub constraint: Option<(String, String)> }

impl Alt {
    fn text(&self) -> String {
        match &self.constraint { Some((op, v)) => format!("{} ({} {})", self.name, op, v), None => self.name.clone() }
    }
}

/// `a (>= 1), b | c` as a list of groups, each group a list of alternatives. Empty entries are dropped.
pub fn parse_depends(field: &str) -> Vec<Vec<Alt>> {
    field.split(',').filter_map(|group| {
        let alts: Vec<Alt> = group.split('|').filter_map(|alt| {
            let alt = alt.trim();
            if alt.is_empty() { return None; }
            let (head, constraint) = match alt.split_once('(') {
                Some((h, rest)) => {
                    let inner = rest.split(')').next().unwrap_or("").trim();
                    let mut it = inner.splitn(2, char::is_whitespace);
                    let op = it.next().unwrap_or("").trim();
                    let ver = it.next().unwrap_or("").trim();
                    (h.trim(), if op.is_empty() || ver.is_empty() { None } else { Some((op.to_string(), ver.to_string())) })
                }
                None => (alt, None),
            };
            let name = head.split(':').next().unwrap_or(head).split_whitespace().next().unwrap_or("").to_string();
            if name.is_empty() { None } else { Some(Alt { name, constraint }) }
        }).collect();
        if alts.is_empty() { None } else { Some(alts) }
    }).collect()
}

/// An installed package as `/var/lib/dpkg/status` lists it, with the names it provides (and the version, if it gives one).
#[derive(Debug, Default)]
pub struct Installed {
    pub versions: HashMap<String, String>,
    pub provides: Vec<(String, String, Option<String>)>,
}

/// The installed packages of a dpkg status file: only stanzas whose status is `install ok installed`.
pub fn parse_status(status: &str) -> Installed {
    let mut db = Installed::default();
    for stanza in status.split("\n\n") {
        let (mut name, mut version, mut ok, mut provides) = (None, None, false, String::new());
        let mut field = "";
        for line in stanza.lines() {
            if line.starts_with(char::is_whitespace) {
                if field == "Provides" { provides.push_str(line.trim()); }
                continue;
            }
            let Some((k, v)) = line.split_once(':') else { continue };
            field = match k { "Provides" => "Provides", _ => "" };
            let v = v.trim();
            match k {
                "Package" => name = Some(v.to_string()),
                "Version" => version = Some(v.to_string()),
                "Status" => ok = v.ends_with(" installed") && v.starts_with("install "),
                "Provides" => provides = v.to_string(),
                _ => {}
            }
        }
        let (Some(name), true) = (name, ok) else { continue };
        let version = version.unwrap_or_default();
        for p in provides.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (pn, pv) = match p.split_once('(') {
                Some((n, rest)) => (n.trim().split(':').next().unwrap_or("").to_string(),
                                    rest.split(')').next().and_then(|i| i.trim().strip_prefix('=')).map(|v| v.trim().to_string())),
                None => (p.split(':').next().unwrap_or(p).to_string(), None),
            };
            db.provides.push((pn, name.clone(), pv));
        }
        db.versions.insert(name, version);
    }
    db
}

/// The `Depends` and `Pre-Depends` groups of the new deb that nothing installed satisfies, as `a (>= 1) | b` text.
/// `compare(have, op, want)` is `dpkg --compare-versions`.
pub fn deb_unmet(depends: &str, db: &Installed, compare: &dyn Fn(&str, &str, &str) -> bool) -> Vec<String> {
    parse_depends(depends).into_iter().filter(|group| {
        !group.iter().any(|alt| {
            let by_name = db.versions.get(&alt.name).map(|have| match &alt.constraint {
                Some((op, want)) => compare(have, op, want),
                None => true,
            }).unwrap_or(false);
            // a provided name satisfies an unversioned dependency, or a versioned one only when the provider states a version
            let by_provides = db.provides.iter().any(|(pn, _, pv)| *pn == alt.name && match (&alt.constraint, pv) {
                (None, _) => true,
                (Some((op, want)), Some(have)) => compare(have, op, want),
                (Some(_), None) => false,
            });
            by_name || by_provides
        })
    }).map(|g| g.iter().map(Alt::text).collect::<Vec<_>>().join(" | ")).collect()
}

/// The toast text for a package whose dependencies the machine lacks: what is missing, one way to install it, and that nothing changed.
pub fn needs_message(kind: Kind, unmet: &[String]) -> String {
    let list = unmet.join(", ");
    let how = match kind {
        Kind::Rpm => format!("sudo dnf install {}", unmet.iter().map(|u| format!("'{u}'")).collect::<Vec<_>>().join(" ")),
        Kind::Deb => format!("sudo apt install {}", unmet.iter().map(|u| u.split([' ', '|']).next().unwrap_or(u).to_string()).collect::<Vec<_>>().join(" ")),
    };
    format!("This update needs {list}, which this computer does not have. Install it ({how}) and update again. Copilot was not changed and is still running.")
}

/// rpm's own words for a refusal that is not a dependency (a signature policy, a conflict), for the failed-install message.
pub fn first_error_lines(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().map(str::trim).filter(|l| !l.is_empty()).take(3).collect();
    lines.join(" ").chars().take(240).collect()
}

/// What running a tool came to.
#[derive(Debug, Clone)]
pub struct Ran { pub ok: bool, pub stdout: String, pub stderr: String }

/// The tools the check runs, so a test can answer for them.
pub trait Tools {
    fn run(&self, program: &str, args: &[&str]) -> Option<Ran>;
    fn read(&self, path: &str) -> Option<String>;
}

pub struct System;
impl Tools for System {
    fn run(&self, program: &str, args: &[&str]) -> Option<Ran> {
        let out = std::process::Command::new(program).args(args).output().ok()?;
        Some(Ran { ok: out.status.success(), stdout: String::from_utf8_lossy(&out.stdout).into_owned(), stderr: String::from_utf8_lossy(&out.stderr).into_owned() })
    }
    fn read(&self, path: &str) -> Option<String> {
        std::fs::read(path).ok().map(|b| String::from_utf8_lossy(&b).into_owned())
    }
}

/// `Some(message)` when the package at `path` needs something this machine lacks (nothing is run with privilege, nothing changes),
/// `None` when the check found nothing wrong or could not run (the install then goes on as the plugin does it).
pub fn preflight(kind: Kind, path: &str, tools: &dyn Tools) -> Option<String> {
    let unmet = match kind {
        Kind::Rpm => {
            let r = tools.run("rpm", &["-U", "--test", path])?;
            if r.ok { return None; }
            rpm_unmet(&r.stderr)
        }
        Kind::Deb => {
            let depends = |field: &str| tools.run("dpkg-deb", &["-f", path, field]).filter(|r| r.ok).map(|r| r.stdout.trim().to_string()).unwrap_or_default();
            let joined = [depends("Pre-Depends"), depends("Depends")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");
            if joined.is_empty() { return None; }
            let db = parse_status(&tools.read("/var/lib/dpkg/status")?);
            let compare = |have: &str, op: &str, want: &str| {
                let op = match op { "<<" => "lt", "<=" | "<" => "le", "=" => "eq", ">=" | ">" => "ge", ">>" => "gt", _ => return true };
                tools.run("dpkg", &["--compare-versions", have, op, want]).map(|r| r.ok).unwrap_or(true)
            };
            deb_unmet(&joined, &db, &compare)
        }
    };
    if unmet.is_empty() { None } else { Some(needs_message(kind, &unmet)) }
}

/// After the plugin's install failed: rpm's own reason when `rpm -U --test` has one (it checks signatures and conflicts too),
/// else nothing. A dpkg failure has no such second opinion.
pub fn why_install_failed(kind: Kind, path: &str, tools: &dyn Tools) -> Option<String> {
    if kind != Kind::Rpm { return None; }
    let r = tools.run("rpm", &["-U", "--test", path])?;
    if r.ok { return None; }
    let unmet = rpm_unmet(&r.stderr);
    if !unmet.is_empty() { return Some(needs_message(kind, &unmet)); }
    let text = first_error_lines(&r.stderr);
    if text.is_empty() { None } else { Some(format!("rpm refused the update: {text}")) }
}

/// What the install command answers the front end (`status` is the tag): the plugin installed the package and the app restarts
/// itself, or nothing changed and `message` goes in the toast.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    Relaunch,
    Kept { message: String },
}

/// Check the downloaded package, then let `install` (the plugin's `pkexec dpkg -i` or `pkexec rpm -U`) run it. A package that needs
/// something this machine lacks is not installed at all. A failed install is an `Err` carrying rpm's own reason when it has one.
pub fn install_checked(kind: Kind, bytes: &[u8], tools: &dyn Tools, install: impl FnOnce(&[u8]) -> Result<(), String>) -> Result<Outcome, String> {
    if kind_of(bytes) != Some(kind) {
        return Err("the update is not the package format this install is updated with".into());
    }
    let dir = std::env::temp_dir().join(format!("copilot-update-pkg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(if kind == Kind::Rpm { "package.rpm" } else { "package.deb" });
    let written = std::fs::write(&path, bytes).map_err(|e| e.to_string());
    let outcome = written.and_then(|_| {
        let p = path.to_string_lossy().to_string();
        if let Some(message) = preflight(kind, &p, tools) { return Ok(Outcome::Kept { message }); }
        match install(bytes) {
            Ok(()) => Ok(Outcome::Relaunch),
            Err(e) => Err(match why_install_failed(kind, &p, tools) { Some(why) => format!("{why} ({e})"), None => e }),
        }
    });
    let _ = std::fs::remove_dir_all(&dir);
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    const RPM_FAIL: &str = "error: Failed dependencies:\n\ticu-nonexistent-dep is needed by copilot-0:0.3.0-1.x86_64\n\tlibwebkit2gtk-4.1.so.0()(64bit) is needed by copilot-0:0.3.0-1.x86_64\n\ticu-nonexistent-dep is needed by copilot-0:0.3.0-1.x86_64\n";

    #[test]
    fn rpm_lists_each_missing_requirement_once() {
        assert_eq!(rpm_unmet(RPM_FAIL), vec!["icu-nonexistent-dep", "libwebkit2gtk-4.1.so.0()(64bit)"]);
        assert_eq!(rpm_unmet("error: Failed dependencies:\n\tbash >= 99 is needed by x-1.0-1.noarch\n\t(icu-aaa or icu-bbb) is needed by x-1.0-1.noarch\n"), vec!["bash >= 99", "(icu-aaa or icu-bbb)"]);
        // other refusals have no dependency section
        assert!(rpm_unmet("error: copilot-0.2.0-1.x86_64: does not verify: no signature\n").is_empty());
        assert!(rpm_unmet("").is_empty());
    }

    #[test]
    fn dependency_text_is_split_into_groups_and_alternatives() {
        let g = parse_depends("bash (>= 4), icu-aaa | libfoo:any (<< 2.0) ,, at-spi2-core");
        assert_eq!(g.len(), 3);
        assert_eq!(g[0], vec![Alt { name: "bash".into(), constraint: Some((">=".into(), "4".into())) }]);
        assert_eq!(g[1][1], Alt { name: "libfoo".into(), constraint: Some(("<<".into(), "2.0".into())) });
        assert_eq!(g[2][0].name, "at-spi2-core");
    }

    const STATUS: &str = "Package: bash\nStatus: install ok installed\nVersion: 5.2.21-2\n\nPackage: libc6\nStatus: install ok installed\nVersion: 2.39-0\nProvides: libc6-udeb (= 2.39-0),\n  libc-bin-virtual\n\nPackage: postfix\nStatus: deinstall ok config-files\nVersion: 3.8\nProvides: mail-transport-agent\n\nPackage: half\nStatus: install ok unpacked\nVersion: 1\n\nPackage: exim\nStatus: install ok installed\nVersion: 4\nProvides: mail-transport-agent\n";

    fn cmp(have: &str, op: &str, want: &str) -> bool {
        // numeric-prefix comparison is enough for the fixture versions
        let n = |s: &str| s.split(|c: char| !c.is_ascii_digit()).next().unwrap_or("0").parse::<u64>().unwrap_or(0);
        let (a, b) = (n(have), n(want));
        match op { ">=" => a >= b, ">>" => a > b, "<<" => a < b, "<=" => a <= b, "=" => a == b, _ => true }
    }

    #[test]
    fn a_dependency_counts_as_met_only_by_an_installed_package_or_what_it_provides() {
        let db = parse_status(STATUS);
        assert!(deb_unmet("bash, libc6 (>= 2.31)", &db, &cmp).is_empty());
        assert_eq!(deb_unmet("bash, icu-nonexistent-dep", &db, &cmp), vec!["icu-nonexistent-dep"]);
        assert_eq!(deb_unmet("bash (>= 99)", &db, &cmp), vec!["bash (>= 99)"]);
        // an alternative that is installed satisfies the group
        assert!(deb_unmet("icu-aaa | bash", &db, &cmp).is_empty());
        assert_eq!(deb_unmet("icu-aaa | icu-bbb", &db, &cmp), vec!["icu-aaa | icu-bbb"]);
        // a package that is removed (config files left) or only unpacked is not installed
        assert_eq!(deb_unmet("half", &db, &cmp), vec!["half"]);
        // a virtual package is met by an installed provider, and the removed postfix does not count
        assert!(deb_unmet("mail-transport-agent", &db, &cmp).is_empty());
        let only_postfix = parse_status("Package: postfix\nStatus: deinstall ok config-files\nVersion: 3.8\nProvides: mail-transport-agent\n");
        assert_eq!(deb_unmet("mail-transport-agent", &only_postfix, &cmp), vec!["mail-transport-agent"]);
        // a versioned dependency is met by a provider only when the provider states a matching version
        assert!(deb_unmet("libc6-udeb (>= 2.30)", &db, &cmp).is_empty());
        assert_eq!(deb_unmet("libc-bin-virtual (>= 1)", &db, &cmp), vec!["libc-bin-virtual (>= 1)"]);
    }

    #[test]
    fn the_message_says_what_is_missing_how_to_get_it_and_that_nothing_changed() {
        let m = needs_message(Kind::Rpm, &["libwebkit2gtk-4.1.so.0()(64bit)".to_string(), "bash >= 99".to_string()]);
        assert!(m.contains("needs libwebkit2gtk-4.1.so.0()(64bit), bash >= 99"), "{m}");
        assert!(m.contains("sudo dnf install 'libwebkit2gtk-4.1.so.0()(64bit)' 'bash >= 99'"), "{m}");
        assert!(m.contains("still running"), "{m}");
        let d = needs_message(Kind::Deb, &["icu-aaa | icu-bbb".to_string(), "libfoo (>= 2)".to_string()]);
        assert!(d.contains("needs icu-aaa | icu-bbb, libfoo (>= 2)"), "{d}");
        assert!(d.contains("sudo apt install icu-aaa libfoo"), "{d}");
    }

    #[test]
    fn a_package_that_needs_something_missing_is_never_handed_to_the_installer() {
        let mut t = fake();
        t.rpm = Some(Ran { ok: false, stdout: String::new(), stderr: RPM_FAIL.into() });
        let mut installed = false;
        let out = install_checked(Kind::Rpm, &[0xED, 0xAB, 0xEE, 0xDB, 0, 0], &t, |_| { installed = true; Ok(()) }).unwrap();
        assert!(matches!(&out, Outcome::Kept { message } if message.contains("icu-nonexistent-dep")), "{out:?}");
        assert!(!installed, "the installer ran for a package with an unmet dependency");
        // a clean package is installed and the app restarts
        t.rpm = Some(Ran { ok: true, stdout: String::new(), stderr: String::new() });
        assert_eq!(install_checked(Kind::Rpm, &[0xED, 0xAB, 0xEE, 0xDB, 0, 0], &t, |_| Ok(())).unwrap(), Outcome::Relaunch);
        // an install that fails reports the plugin's error, with rpm's reason when rpm has one
        let err = install_checked(Kind::Rpm, &[0xED, 0xAB, 0xEE, 0xDB, 0, 0], &t, |_| Err("Failed to install package".into())).unwrap_err();
        assert_eq!(err, "Failed to install package");
        t.rpm = Some(Ran { ok: false, stdout: String::new(), stderr: "error: x: does not verify: no signature\n".into() });
        let err = install_checked(Kind::Rpm, &[0xED, 0xAB, 0xEE, 0xDB, 0, 0], &t, |_| Err("Failed to install package".into())).unwrap_err();
        assert!(err.contains("rpm refused the update: error: x: does not verify: no signature") && err.contains("Failed to install package"), "{err}");
        // bytes of the other format, or of neither, are not handed to this install at all
        let mut ran = false;
        assert!(install_checked(Kind::Rpm, b"!<arch>\nx", &t, |_| { ran = true; Ok(()) }).is_err());
        assert!(!ran);
    }

    /// The real tools on a Debian machine: synthetic debs built with `dpkg-deb`, checked against the machine's own dpkg database.
    /// Skipped on Linux hosts without dpkg; requires Unix file permissions.
    #[cfg(target_os = "linux")]
    #[test]
    fn real_dpkg_refuses_a_synthetic_deb_with_an_unmet_dependency_and_accepts_a_met_one() {
        if std::process::Command::new("dpkg-deb").arg("--version").output().is_err() || !std::path::Path::new("/var/lib/dpkg/status").exists() {
            eprintln!("skipped: no dpkg on this machine");
            return;
        }
        let dir = std::env::temp_dir().join(format!("icu-pkgcheck-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let build = |name: &str, depends: &str| {
            let root = dir.join(name);
            std::fs::create_dir_all(root.join("DEBIAN")).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(root.join("DEBIAN"), std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::write(root.join("DEBIAN/control"), format!("Package: icu-pkgcheck\nVersion: 9.0\nArchitecture: all\nMaintainer: t <t@example.invalid>\nDescription: synthetic\nDepends: {depends}\n")).unwrap();
            let out = dir.join(format!("{name}.deb"));
            let built = std::process::Command::new("dpkg-deb").args(["--build", "--root-owner-group"]).arg(&root).arg(&out).output().unwrap();
            assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
            out.to_string_lossy().to_string()
        };
        let missing = build("missing", "bash, icu-nonexistent-dep");
        let met = build("met", "bash, dpkg (>= 1.0)");
        let versioned = build("versioned", "dpkg (>= 99:1)");
        let m = preflight(Kind::Deb, &missing, &System).expect("the unmet dependency was not seen");
        assert!(m.contains("icu-nonexistent-dep") && !m.contains("bash"), "{m}");
        assert_eq!(preflight(Kind::Deb, &met, &System), None);
        assert!(preflight(Kind::Deb, &versioned, &System).expect("the unmet version was not seen").contains("dpkg (>= 99:1)"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_package_format_is_read_from_its_magic() {
        assert_eq!(kind_of(&[0xED, 0xAB, 0xEE, 0xDB, 3, 0]), Some(Kind::Rpm));
        assert_eq!(kind_of(b"!<arch>\ndebian-binary   "), Some(Kind::Deb));
        assert_eq!(kind_of(b"\x1f\x8b\x08 an appimage archive"), None);
    }

    /// Answers for the tools from a table.
    struct Fake { rpm: Option<Ran>, depends: String, status: Option<String>, compare_ok: bool, calls: std::sync::Mutex<Vec<String>> }
    impl Tools for Fake {
        fn run(&self, program: &str, args: &[&str]) -> Option<Ran> {
            self.calls.lock().unwrap().push(format!("{program} {}", args.join(" ")));
            match (program, args.first().copied()) {
                ("rpm", _) => self.rpm.clone(),
                ("dpkg-deb", _) => Some(Ran { ok: true, stdout: if args.last() == Some(&"Depends") { self.depends.clone() } else { String::new() }, stderr: String::new() }),
                ("dpkg", Some("--compare-versions")) => Some(Ran { ok: self.compare_ok, stdout: String::new(), stderr: String::new() }),
                _ => None,
            }
        }
        fn read(&self, _path: &str) -> Option<String> { self.status.clone() }
    }
    fn fake() -> Fake { Fake { rpm: None, depends: String::new(), status: None, compare_ok: true, calls: Default::default() } }

    #[test]
    fn an_rpm_with_an_unmet_dependency_is_refused_with_the_name_and_a_clean_one_is_not() {
        let mut t = fake();
        t.rpm = Some(Ran { ok: false, stdout: String::new(), stderr: RPM_FAIL.into() });
        let m = preflight(Kind::Rpm, "/tmp/p.rpm", &t).expect("an unmet dependency must be refused");
        assert!(m.contains("icu-nonexistent-dep") && m.contains("libwebkit2gtk-4.1.so.0()(64bit)"), "{m}");
        t.rpm = Some(Ran { ok: true, stdout: String::new(), stderr: String::new() });
        assert_eq!(preflight(Kind::Rpm, "/tmp/p.rpm", &t), None);
        // rpm refusing for another reason is not ours to block on (the install runs and the reason is shown after it)
        t.rpm = Some(Ran { ok: false, stdout: String::new(), stderr: "error: x: does not verify: no signature\n".into() });
        assert_eq!(preflight(Kind::Rpm, "/tmp/p.rpm", &t), None);
        // no rpm tool at all: the plugin goes on
        t.rpm = None;
        assert_eq!(preflight(Kind::Rpm, "/tmp/p.rpm", &t), None);
    }

    #[test]
    fn a_deb_with_an_unmet_dependency_is_refused_before_dpkg_unpacks_anything() {
        let mut t = fake();
        t.depends = "bash, icu-nonexistent-dep".into();
        t.status = Some(STATUS.into());
        let m = preflight(Kind::Deb, "/tmp/p.deb", &t).expect("an unmet dependency must be refused");
        assert!(m.contains("needs icu-nonexistent-dep,") && m.contains("sudo apt install icu-nonexistent-dep"), "{m}");
        assert!(!t.calls.lock().unwrap().iter().any(|c| c.starts_with("dpkg -i")), "nothing is installed by the check");
        t.depends = "bash, exim".into();
        assert_eq!(preflight(Kind::Deb, "/tmp/p.deb", &t), None);
        // a versioned dependency goes through dpkg --compare-versions
        t.depends = "bash (>= 99)".into();
        t.compare_ok = false;
        assert!(preflight(Kind::Deb, "/tmp/p.deb", &t).is_some());
        t.compare_ok = true;
        assert_eq!(preflight(Kind::Deb, "/tmp/p.deb", &t), None);
        // no status file or no Depends: nothing to say
        t.status = None;
        assert_eq!(preflight(Kind::Deb, "/tmp/p.deb", &t), None);
    }

    #[test]
    fn a_failed_rpm_install_names_rpms_own_reason() {
        let mut t = fake();
        t.rpm = Some(Ran { ok: false, stdout: String::new(), stderr: "error: copilot-0:0.2.0-1.x86_64: does not verify: no signature\n".into() });
        let m = why_install_failed(Kind::Rpm, "/tmp/p.rpm", &t).unwrap();
        assert_eq!(m, "rpm refused the update: error: copilot-0:0.2.0-1.x86_64: does not verify: no signature");
        t.rpm = Some(Ran { ok: false, stdout: String::new(), stderr: RPM_FAIL.into() });
        assert!(why_install_failed(Kind::Rpm, "/tmp/p.rpm", &t).unwrap().contains("needs icu-nonexistent-dep"));
        t.rpm = Some(Ran { ok: true, stdout: String::new(), stderr: String::new() });
        assert_eq!(why_install_failed(Kind::Rpm, "/tmp/p.rpm", &t), None);
        assert_eq!(why_install_failed(Kind::Deb, "/tmp/p.deb", &t), None);
    }
}
