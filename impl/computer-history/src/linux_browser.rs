//! Native browser accessibility. All page context is transient and read only
//! after the host's separate browser consent and application rule.
use crate::{
    browser::document_url,
    capture::Content,
    linux_atspi::{self as ax, Atspi, Metadata, ObjectKey, Tree},
    Result,
};
use atspi::{Role, State};
use std::{
    collections::{HashMap, HashSet},
    os::unix::fs::MetadataExt,
    path::Path,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Chrome,
    Chromium,
    Firefox,
}
pub(super) fn kind(application: &str) -> Option<Kind> {
    match application {
        "chrome" | "google-chrome" | "google-chrome-stable" => Some(Kind::Chrome),
        "chromium" | "chromium-browser" => Some(Kind::Chromium),
        "firefox" | "firefox-bin" | "firefox-esr" => Some(Kind::Firefox),
        _ => None,
    }
}

// A basename alone cannot establish which browser implements these semantics.
// Resolve the running executable, check every directory, and refuse sandbox
// package layouts whose broker/accessibility process identity differs.
pub(super) fn executable(pid: u32, kind: Kind) -> bool {
    let check = || -> Option<bool> {
        let proc = format!("/proc/{pid}/exe");
        let path = std::fs::read_link(&proc).ok()?;
        let allowed = match kind {
            Kind::Chrome => path == Path::new("/opt/google/chrome/chrome"),
            Kind::Chromium => [
                "/usr/lib/chromium/chromium",
                "/usr/lib64/chromium-browser/chromium-browser",
                "/usr/lib/chromium-browser/chromium-browser",
            ]
            .iter()
            .any(|p| path == Path::new(p)),
            Kind::Firefox => [
                "/usr/lib/firefox/firefox",
                "/usr/lib64/firefox/firefox",
                "/usr/lib/firefox-esr/firefox-esr",
            ]
            .iter()
            .any(|p| path == Path::new(p)),
        };
        if !allowed {
            return Some(false);
        }
        match std::fs::metadata(format!("/proc/{pid}/root/.flatpak-info")) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Some(false),
        }
        let running = std::fs::metadata(&proc).ok()?;
        let installed = std::fs::symlink_metadata(&path).ok()?;
        if !installed.is_file()
            || running.dev() != installed.dev()
            || running.ino() != installed.ino()
        {
            return Some(false);
        }
        for component in path.ancestors() {
            let m = std::fs::symlink_metadata(component).ok()?;
            if m.uid() != 0 || m.mode() & 0o022 != 0 || (component != path && !m.is_dir()) {
                return Some(false);
            }
        }
        Some(true)
    };
    check() == Some(true)
}

pub(crate) trait BrowserTree: Tree {
    async fn attributes(&self, key: &ObjectKey) -> Result<HashMap<String, String>>;
    async fn name(&self, key: &ObjectKey) -> Result<String>;
    async fn locale(&self, key: &ObjectKey) -> Result<String>;
    async fn document_attribute(&self, key: &ObjectKey, attribute: &str) -> Result<String>;
}
impl BrowserTree for Atspi {
    async fn attributes(&self, key: &ObjectKey) -> Result<HashMap<String, String>> {
        let proxy = self.proxy(key, "org.a11y.atspi.Accessible").await?;
        ax::call(proxy.call("GetAttributes", &())).await
    }
    async fn name(&self, key: &ObjectKey) -> Result<String> {
        let proxy = self.proxy(key, "org.a11y.atspi.Accessible").await?;
        let value: String = ax::call(proxy.get_property("Name")).await?;
        if value.len() > 16384 {
            return Err(ax::unavailable());
        }
        Ok(value)
    }
    async fn locale(&self, key: &ObjectKey) -> Result<String> {
        let proxy = self.proxy(key, "org.a11y.atspi.Accessible").await?;
        ax::call(proxy.get_property("Locale")).await
    }
    async fn document_attribute(&self, key: &ObjectKey, attribute: &str) -> Result<String> {
        let proxy = self.proxy(key, "org.a11y.atspi.Document").await?;
        let value: String = ax::call(proxy.call("GetAttributeValue", &(attribute,))).await?;
        if value.len() > 16384 {
            return Err(ax::unavailable());
        }
        Ok(value)
    }
}

fn healthy(meta: &Metadata, pid: u32) -> bool {
    meta.pid == pid
        && meta.children >= 0
        && meta.children <= 512
        && ![State::Defunct, State::Invalid, State::Stale, State::Busy]
            .iter()
            .any(|s| meta.states.contains(*s))
        && !matches!(meta.role, Role::Invalid | Role::Unknown)
}
fn document(role: Role) -> bool {
    matches!(
        role,
        Role::DocumentWeb | Role::DocumentFrame | Role::Embedded | Role::DocumentEmail
    )
}
fn frame(role: Role) -> bool {
    matches!(role, Role::Frame | Role::Window | Role::Dialog)
}
fn english(locale: &str) -> bool {
    matches!(locale, "en-US" | "en_US" | "en_US.UTF-8" | "C" | "C.UTF-8")
}

// Chromium adds its URL status bubble as a second Frame under BrowserRootView.
// Admit only the passive three-node native shape and never read its URL label.
// Other popups, documents, focusable nodes and incomplete structures refuse.
async fn passive_status(t: &impl BrowserTree, root: &ObjectKey, pid: u32) -> Result<bool> {
    let mut key = root.clone();
    for (role, class, children) in [
        (Role::Frame, "RootView", 1),
        (Role::Panel, "StatusView", 1),
        (Role::Static, "Label", 0),
    ] {
        let m = t.metadata(&key).await?;
        if !healthy(&m, pid)
            || m.role != role
            || m.children != children
            || [State::Active, State::Focused, State::Focusable]
                .iter()
                .any(|s| m.states.contains(*s))
            || t.attributes(&key).await?.get("class").map(String::as_str) != Some(class)
        {
            return Ok(false);
        }
        let next = t.children(&key).await?;
        if next.len() != children as usize {
            return Ok(false);
        }
        if let Some(child) = next.first() {
            if t.parent(child).await? != key {
                return Ok(false);
            }
            key = child.clone();
        }
    }
    Ok(true)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Page {
    pub document: ObjectKey,
    pub url: String,
    pub focused: Option<ObjectKey>,
    // Every document on the focused control ancestry, outermost first.
    documents: Vec<(ObjectKey, String)>,
    url_attribute: &'static str,
    // The root name is a transient browser-owned mode signal, never a payload.
    title: String,
    chrome: Vec<(ObjectKey, String)>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Snapshot {
    Public(Page),
    Private,
    Unknown,
}

pub(crate) async fn native_frame(
    t: &impl BrowserTree,
    root: &ObjectKey,
    kind: Kind,
) -> Result<bool> {
    Ok(match kind {
        Kind::Chrome | Kind::Chromium => {
            t.attributes(root).await?.get("class").map(String::as_str) == Some("BrowserRootView")
        }
        Kind::Firefox => {
            t.document_attribute(root, "DocURL").await? == "chrome://browser/content/browser.xhtml"
        }
    })
}

// Only reviewed English browser chrome is currently recognized. Unknown
// locales/profile badges/title layouts are refusals, never inferred normal.
fn title_mode(kind: Kind, title: &str, locale: &str) -> Option<bool> {
    if !english(locale) {
        return None;
    }
    let brand = match kind {
        Kind::Chrome => "Google Chrome",
        Kind::Chromium => "Chromium",
        Kind::Firefox => "Mozilla Firefox",
    };
    let private = if kind == Kind::Firefox {
        format!("{brand} Private Browsing")
    } else {
        format!("{brand} (Incognito)")
    };
    if title.ends_with(&private) {
        return Some(true);
    }
    let normal = if kind == Kind::Firefox {
        format!(" — {brand}")
    } else {
        format!(" - {brand}")
    };
    (title.ends_with(&normal) || (kind == Kind::Firefox && title == brand)).then_some(false)
}

pub(super) async fn snapshot(
    t: &impl BrowserTree,
    root: &ObjectKey,
    pid: u32,
    kind: Kind,
) -> Result<Snapshot> {
    let root_meta = t.metadata(root).await?;
    if !healthy(&root_meta, pid)
        || !frame(root_meta.role)
        || !root_meta.states.contains(State::Active)
        || !root_meta.states.contains(State::Showing)
        || root_meta.states.contains(State::Iconified)
        || !native_frame(t, root, kind).await?
    {
        return Ok(Snapshot::Unknown);
    }
    let title = t.name(root).await?;
    match title_mode(kind, &title, &t.locale(root).await?) {
        Some(true) => return Ok(Snapshot::Private),
        Some(false) => {}
        None => return Ok(Snapshot::Unknown),
    }
    // Firefox's permanent-private mode can present ordinary window chrome.
    // Until the provider exposes a positive normal-mode authority, its title
    // and absence of a private indicator cannot authorize even metadata.
    if kind == Kind::Firefox {
        return Ok(Snapshot::Unknown);
    }
    // Inspect browser-owned controls without descending into any page until
    // mode is established. HTML attributes/names cannot serve as chrome proof.
    let mut pending = vec![(root.clone(), 0)];
    let mut seen = HashSet::new();
    let mut documents = Vec::new();
    let mut chrome = Vec::new();
    let mut private = false;
    while let Some((key, depth)) = pending.pop() {
        if seen.len() >= 512 || depth > 32 || !seen.insert(key.clone()) {
            return Err(ax::unavailable());
        }
        let m = t.metadata(&key).await?;
        if !healthy(&m, pid) {
            return Ok(Snapshot::Unknown);
        }
        if !m.states.contains(State::Showing) {
            continue;
        }
        if key != *root && frame(m.role) {
            if matches!(kind, Kind::Chrome | Kind::Chromium) && passive_status(t, &key, pid).await?
            {
                continue;
            }
            return Ok(Snapshot::Unknown);
        }
        if document(m.role) {
            if m.role != Role::DocumentWeb {
                return Ok(Snapshot::Unknown);
            }
            documents.push(key);
            continue;
        }
        // Omnibox/menu/toolbar focus is outside the page, even at metadata
        // scope. Do not infer page focus from the selected tab alone.
        if key != *root && m.states.contains(State::Focused) {
            return Ok(Snapshot::Unknown);
        }
        let attrs = t.attributes(&key).await?;
        if kind != Kind::Firefox
            && attrs.get("class").map(String::as_str) == Some("AvatarToolbarButton")
            && m.role == Role::Button
        {
            let name = t.name(&key).await?;
            private |= name == "Incognito" || name == "Guest";
            chrome.push((key.clone(), name));
        }
        if kind == Kind::Firefox {
            private |= attrs.get("class").is_some_and(|s| {
                s.split_whitespace()
                    .any(|v| v == "private-browsing-indicator-button")
            }) || attrs.get("id").map(String::as_str)
                == Some("reset-pbm-toolbar-button");
        }
        let children = t.children(&key).await?;
        if children.len() != m.children as usize {
            return Ok(Snapshot::Unknown);
        }
        pending.extend(children.into_iter().rev().map(|k| (k, depth + 1)));
    }
    if private {
        return Ok(Snapshot::Private);
    }
    if kind != Kind::Firefox
        && (chrome.len() != 1
            || !matches!(
                chrome[0].1.as_str(),
                "You" | "Sign in to Chrome?" | "Sign in to Chromium?"
            ))
    {
        return Ok(Snapshot::Unknown);
    }
    if documents.len() != 1 {
        return Ok(Snapshot::Unknown);
    }
    let document = documents.pop().unwrap();
    let attribute = if kind == Kind::Firefox {
        "DocURL"
    } else {
        "URI"
    };
    let Some(url) = document_url(&t.document_attribute(&document, attribute).await?) else {
        return Ok(Snapshot::Unknown);
    };
    let (focused, focused_documents) = focus(t, &document, &url, attribute, pid).await?;
    if t.metadata(root).await? != root_meta || t.name(root).await? != title {
        return Ok(Snapshot::Unknown);
    }
    Ok(Snapshot::Public(Page {
        document,
        url,
        focused,
        documents: focused_documents,
        url_attribute: attribute,
        title,
        chrome,
    }))
}

async fn focus(
    t: &impl BrowserTree, document_key: &ObjectKey, page_url: &str,
    attribute: &'static str, pid: u32,
) -> Result<(Option<ObjectKey>, Vec<(ObjectKey, String)>)> {
    let Some(origin) = url::Url::parse(page_url).ok().map(|url| url.origin()) else {
        return Ok((None, vec![]));
    };
    let mut pending = vec![(document_key.clone(), 0, vec![(document_key.clone(), page_url.to_owned())])];
    let mut seen = HashSet::new();
    let mut focused = None;
    let mut documents = Vec::new();
    while let Some((key, depth, mut ancestry)) = pending.pop() {
        if seen.len() >= 512 || depth > 32 || !seen.insert(key.clone()) {
            return Err(ax::unavailable());
        }
        let m = t.metadata(&key).await?;
        if !healthy(&m, pid) || frame(m.role) {
            return Ok((None, vec![]));
        }
        if !m.states.contains(State::Showing) || m.role == Role::PasswordText {
            continue;
        }
        // An embedded document needs its own browser-reported URI. Never use
        // an iframe element's inherited URI, a page label or a parent fallback.
        if key != *document_key && document(m.role) {
            if m.role != Role::DocumentWeb { continue; }
            let Some(uri) = document_url(&t.document_attribute(&key, attribute).await?) else { continue; };
            if url::Url::parse(&uri).ok().map(|url| url.origin()).as_ref() != Some(&origin) { continue; }
            ancestry.push((key.clone(), uri));
        }
        if m.states.contains(State::Focused) && !document(m.role) {
            if focused.is_some() { return Ok((None, vec![])); }
            focused = Some(key.clone());
            documents = ancestry.clone();
        }
        let children = t.children(&key).await?;
        if children.len() != m.children as usize { return Ok((None, vec![])); }
        pending.extend(children.into_iter().rev().map(|key| (key, depth + 1, ancestry.clone())));
    }
    Ok((focused, documents))
}

async fn ancestry(
    t: &impl BrowserTree,
    key: &ObjectKey,
    root: &ObjectKey,
    page: &Page,
    pid: u32,
) -> Result<Option<Vec<(ObjectKey, Metadata)>>> {
    let mut at = key.clone();
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    let mut documents = 0;
    for _ in 0..48 {
        if !seen.insert(at.clone()) {
            return Ok(None);
        }
        let m = t.metadata(&at).await?;
        if !healthy(&m, pid) || m.role == Role::PasswordText {
            return Ok(None);
        }
        if document(m.role) {
            let Some((expected, uri)) = page.documents.iter().rev().nth(documents) else { return Ok(None); };
            if at != *expected || m.role != Role::DocumentWeb
                || document_url(&t.document_attribute(&at, page.url_attribute).await?).as_ref() != Some(uri) {
                return Ok(None);
            }
            documents += 1;
        }
        if frame(m.role) && at != *root {
            return Ok(None);
        }
        chain.push((at.clone(), m.clone()));
        if at == *root {
            return Ok((documents == page.documents.len() && documents > 0
                && m.states.contains(State::Active)
                && m.states.contains(State::Showing)
                && !m.states.contains(State::Iconified))
            .then_some(chain));
        }
        at = t.parent(&at).await?;
    }
    Ok(None)
}

pub(super) async fn read(
    t: &impl BrowserTree,
    root: &ObjectKey,
    pid: u32,
    page: &Page,
    values: bool,
) -> Result<Option<Content>> {
    let Some(key) = &page.focused else {
        return Ok(None);
    };
    let Some(before) = ancestry(t, key, root, page, pid).await? else {
        return Ok(None);
    };
    let m = &before[0].1;
    if !m.states.contains(State::Focused)
        || !m.states.contains(State::Showing)
        || (m.children != 0 && m.role != Role::Link)
        || !matches!(
            m.role,
            Role::Entry
                | Role::Text
                | Role::Button
                | Role::CheckBox
                | Role::RadioButton
                | Role::Link
        )
    {
        return Ok(None);
    }
    let content = if m.role == Role::Link {
        let children = t.children(key).await?;
        if children.len() != m.children as usize {
            return Ok(None);
        }
        for child in children {
            let meta = t.metadata(&child).await?;
            if !healthy(&meta, pid)
                || meta.children != 0
                || !matches!(meta.role, Role::Static | Role::Text)
                || meta.states.contains(State::Focusable)
                || t.parent(&child).await? != *key
            {
                return Ok(None);
            }
        }
        Content {
            role: "AXLink".into(),
            label: Some(t.name(key).await?),
            ..Default::default()
        }
    } else {
        let mut content = t.content(key, m.role, values, 2049).await?;
        // Accessible IDs are page-controlled and may themselves be URLs.
        content.identifier = None;
        content
    };
    Ok((ancestry(t, key, root, page, pid).await?.as_ref() == Some(&before)).then_some(content))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    fn key(n: usize) -> ObjectKey {
        ObjectKey {
            bus: ":1.7".into(),
            path: format!("/node/{n}"),
        }
    }
    struct Fixture {
        nodes: RefCell<Vec<(Metadata, usize, Vec<usize>)>>,
        title: String,
        locale: String,
        badge: String,
        uri: String,
        document_uris: RefCell<HashMap<usize, String>>,
        swap_uri_on_read: Option<usize>,
        url_reads: Cell<usize>,
        content_reads: Cell<usize>,
        reparent: bool,
    }
    impl Fixture {
        fn new() -> Self {
            let node = |role, parent, children: Vec<usize>, states: &[State]| {
                (
                    Metadata {
                        pid: 42,
                        role,
                        children: children.len() as i32,
                        states: states.iter().copied().collect(),
                    },
                    parent,
                    children,
                )
            };
            Self {
                nodes: RefCell::new(vec![
                    node(Role::Frame, 0, vec![1, 2], &[State::Active, State::Showing]),
                    node(Role::Button, 0, vec![], &[State::Showing]),
                    node(Role::DocumentWeb, 0, vec![3], &[State::Showing]),
                    node(Role::Entry, 2, vec![], &[State::Showing, State::Focused]),
                ]),
                title: "Fixture - Google Chrome".into(),
                locale: "C.UTF-8".into(),
                badge: "Sign in to Chrome?".into(),
                uri: "https://public.example/path?token=NEVER_STORE#fragment".into(),
                document_uris: RefCell::new(HashMap::new()),
                swap_uri_on_read: None,
                url_reads: Cell::new(0),
                content_reads: Cell::new(0),
                reparent: false,
            }
        }
        fn index(key: &ObjectKey) -> usize {
            key.path.rsplit('/').next().unwrap().parse().unwrap()
        }
        fn set_children(&self, n: usize, children: Vec<usize>) {
            let mut nodes = self.nodes.borrow_mut();
            nodes[n].0.children = children.len() as i32;
            nodes[n].2 = children;
        }
    }
    impl Tree for Fixture {
        async fn metadata(&self, k: &ObjectKey) -> Result<Metadata> {
            Ok(self.nodes.borrow()[Self::index(k)].0.clone())
        }
        async fn parent(&self, k: &ObjectKey) -> Result<ObjectKey> {
            Ok(key(self.nodes.borrow()[Self::index(k)].1))
        }
        async fn children(&self, k: &ObjectKey) -> Result<Vec<ObjectKey>> {
            Ok(self.nodes.borrow()[Self::index(k)]
                .2
                .iter()
                .map(|n| key(*n))
                .collect())
        }
        async fn content(&self, _: &ObjectKey, _: Role, values: bool, _: i32) -> Result<Content> {
            self.content_reads.set(self.content_reads.get() + 1);
            if let Some(document) = self.swap_uri_on_read {
                self.document_uris.borrow_mut().insert(document, "https://foreign.example/changed".into());
            }
            if self.reparent {
                self.nodes.borrow_mut()[3].1 = 0;
            }
            Ok(Content {
                role: "AXTextField".into(),
                label: Some("Approved field".into()),
                value: values.then(|| "Approved value".into()),
                ..Default::default()
            })
        }
    }
    impl BrowserTree for Fixture {
        async fn attributes(&self, k: &ObjectKey) -> Result<HashMap<String, String>> {
            Ok(match Self::index(k) {
                0 => [("class".into(), "BrowserRootView".into())].into(),
                1 => [("class".into(), "AvatarToolbarButton".into())].into(),
                4 => [("class".into(), "RootView".into())].into(),
                5 => [("class".into(), "StatusView".into())].into(),
                6 => [("class".into(), "Label".into())].into(),
                _ => HashMap::new(),
            })
        }
        async fn name(&self, k: &ObjectKey) -> Result<String> {
            Ok(if Self::index(k) == 0 {
                self.title.clone()
            } else {
                self.badge.clone()
            })
        }
        async fn locale(&self, _: &ObjectKey) -> Result<String> {
            Ok(self.locale.clone())
        }
        async fn document_attribute(&self, k: &ObjectKey, _: &str) -> Result<String> {
            if Self::index(k) == 0 {
                return Ok("chrome://browser/content/browser.xhtml".into());
            }
            self.url_reads.set(self.url_reads.get() + 1);
            Ok(self.document_uris.borrow().get(&Self::index(k)).cloned().unwrap_or_else(|| self.uri.clone()))
        }
    }
    fn snapshot_of(f: &Fixture) -> Snapshot {
        ax::run(snapshot(f, &key(0), 42, Kind::Chrome)).unwrap()
    }
    fn page(f: &Fixture) -> Page {
        match snapshot_of(f) {
            Snapshot::Public(p) => p,
            other => panic!("{other:?}"),
        }
    }
    #[test]
    fn approved_page_reads_only_focused_scope() {
        let f = Fixture::new();
        let p = page(&f);
        assert_eq!(p.document, key(2));
        assert_eq!(p.focused, Some(key(3)));
        assert_eq!(f.content_reads.get(), 0);
        let text = ax::run(read(&f, &key(0), 42, &p, true)).unwrap().unwrap();
        assert_eq!(text.value.as_deref(), Some("Approved value"));
        let semantic = ax::run(read(&f, &key(0), 42, &p, false)).unwrap().unwrap();
        assert_eq!(semantic.value, None);
        assert_eq!(
            crate::policy::web_host(&p.url).as_deref(),
            Some("public.example")
        );
    }
    #[test]
    fn private_and_unknown_chrome_never_read_page_url_or_content() {
        for (title, locale, badge, expected) in [
            (
                "Fixture - Google Chrome (Incognito)",
                "C.UTF-8",
                "Incognito",
                Snapshot::Private,
            ),
            (
                "Fixture - Google Chrome",
                "C.UTF-8",
                "Incognito",
                Snapshot::Private,
            ),
            (
                "Fixture - Google Chrome (Guest)",
                "C.UTF-8",
                "Guest",
                Snapshot::Unknown,
            ),
            (
                "Fixture - Google Chrome",
                "fr",
                "Sign in to Chrome?",
                Snapshot::Unknown,
            ),
            (
                "Fixture - Google Chrome",
                "C.UTF-8",
                "Unrecognized profile",
                Snapshot::Unknown,
            ),
            (
                "Unknown browser title",
                "C.UTF-8",
                "Sign in to Chrome?",
                Snapshot::Unknown,
            ),
        ] {
            let mut f = Fixture::new();
            f.title = title.into();
            f.locale = locale.into();
            f.badge = badge.into();
            assert_eq!(snapshot_of(&f), expected);
            assert_eq!(f.url_reads.get(), 0);
            assert_eq!(f.content_reads.get(), 0);
        }
    }
    #[test]
    fn firefox_normal_looking_chrome_is_not_a_normal_mode_authority() {
        let mut f = Fixture::new();
        f.title = "Fixture — Mozilla Firefox".into();
        f.locale = "en-US".into();
        assert_eq!(
            ax::run(snapshot(&f, &key(0), 42, Kind::Firefox)).unwrap(),
            Snapshot::Unknown
        );
        assert_eq!(f.url_reads.get(), 0);
    }
    #[test]
    fn title_injection_cannot_hide_private_suffix() {
        for title in [
            "Fixture - Google Chrome - Google Chrome (Incognito)",
            "Fixture — Mozilla Firefox — Mozilla Firefox Private Browsing",
        ] {
            let k = if title.contains("Firefox") {
                Kind::Firefox
            } else {
                Kind::Chrome
            };
            assert_eq!(title_mode(k, title, "en-US"), Some(true));
        }
    }
    fn embedded_fixture(uri: &str) -> Fixture {
        let f = Fixture::new();
        {
            let mut nodes = f.nodes.borrow_mut();
            nodes[3].0.states.remove(State::Focused);
            let mut doc = nodes[2].0.clone(); doc.children = 1;
            nodes.push((doc, 2, vec![5]));
            let mut entry = nodes[3].0.clone(); entry.states.insert(State::Focused);
            nodes.push((entry, 4, vec![]));
        }
        f.set_children(2, vec![3, 4]);
        f.document_uris.borrow_mut().insert(4, uri.into());
        f
    }
    #[test]
    fn same_origin_embedded_document_has_its_own_uri_binding() {
        let f = embedded_fixture("https://public.example/embedded");
        let p = page(&f);
        assert_eq!(p.focused, Some(key(5)));
        assert_eq!(p.documents.len(), 2);
        assert_eq!(ax::run(read(&f, &key(0), 42, &p, true)).unwrap().unwrap().value.as_deref(), Some("Approved value"));
    }
    #[test]
    fn foreign_or_unknown_embedded_origins_never_inherit_parent_approval() {
        for uri in ["https://foreign.example/embedded", "http://public.example/embedded", "https://public.example:8443/embedded", "", "about:blank"] {
            let f = embedded_fixture(uri); let p = page(&f);
            assert!(p.focused.is_none(), "{uri}");
            assert!(ax::run(read(&f, &key(0), 42, &p, true)).unwrap().is_none());
            assert_eq!(f.content_reads.get(), 0);
        }
    }
    #[test]
    fn replaced_embedded_document_uri_refuses_before_content_read() {
        let f = embedded_fixture("https://public.example/embedded"); let p = page(&f);
        f.document_uris.borrow_mut().insert(4, "https://foreign.example/replaced".into());
        assert!(ax::run(read(&f, &key(0), 42, &p, true)).unwrap().is_none());
        assert_eq!(f.content_reads.get(), 0);
    }
    #[test]
    fn embedded_navigation_during_content_read_discards_the_value() {
        let mut f = embedded_fixture("https://public.example/embedded"); f.swap_uri_on_read = Some(4);
        let p = page(&f);
        assert!(ax::run(read(&f, &key(0), 42, &p, true)).unwrap().is_none());
        assert_eq!(f.content_reads.get(), 1);
    }
    #[test]
    fn secure_nested_document_and_toolbar_focus_are_excluded() {
        for role in [
            Role::PasswordText,
            Role::DocumentWeb,
            Role::DocumentFrame,
            Role::Embedded,
        ] {
            let f = Fixture::new();
            f.nodes.borrow_mut()[3].0.role = role;
            let p = page(&f);
            assert!(p.focused.is_none());
            assert!(ax::run(read(&f, &key(0), 42, &p, true)).unwrap().is_none());
            assert_eq!(f.content_reads.get(), 0);
        }
        let f = Fixture::new();
        f.nodes.borrow_mut()[3].0.states.remove(State::Focused);
        f.nodes.borrow_mut()[1].0.states.insert(State::Focused);
        assert_eq!(snapshot_of(&f), Snapshot::Unknown);
    }
    #[test]
    fn wrong_pid_and_ambiguous_documents_refuse() {
        let f = Fixture::new();
        f.nodes.borrow_mut()[0].0.pid = 43;
        assert_eq!(snapshot_of(&f), Snapshot::Unknown);
        let f = Fixture::new();
        f.nodes.borrow_mut()[1].0.role = Role::DocumentWeb;
        assert_eq!(snapshot_of(&f), Snapshot::Unknown);
        assert_eq!(f.url_reads.get(), 0);
        let f = Fixture::new();
        f.nodes.borrow_mut()[3].0.pid = 43;
        assert!(page(&f).focused.is_none());
    }
    #[test]
    fn malformed_and_nonweb_urls_refuse() {
        for uri in [
            "",
            "about:blank",
            "file:///tmp/test",
            "javascript:alert(1)",
            "public.example",
        ] {
            let mut f = Fixture::new();
            f.uri = uri.into();
            assert_eq!(snapshot_of(&f), Snapshot::Unknown);
        }
    }
    #[test]
    fn navigation_mode_and_focus_changes_invalidate_snapshot() {
        let mut f = Fixture::new();
        let original = page(&f);
        f.uri = "https://blocked.example/".into();
        assert_ne!(page(&f), original);
        f.uri = original.url.clone();
        f.nodes.borrow_mut()[3].0.states.remove(State::Focused);
        assert_ne!(page(&f), original);
        f.badge = "Incognito".into();
        assert_eq!(snapshot_of(&f), Snapshot::Private);
    }
    #[test]
    fn reparenting_during_read_discards_content() {
        let mut f = Fixture::new();
        f.reparent = true;
        let p = page(&f);
        assert!(ax::run(read(&f, &key(0), 42, &p, true)).unwrap().is_none());
    }
    #[test]
    fn cycles_and_child_count_mismatch_refuse() {
        let f = Fixture::new();
        f.set_children(1, vec![0]);
        assert!(ax::run(snapshot(&f, &key(0), 42, Kind::Chrome)).is_err());
        let f = Fixture::new();
        f.nodes.borrow_mut()[0].0.children = 3;
        assert_eq!(snapshot_of(&f), Snapshot::Unknown);
    }
    #[test]
    fn unsupported_browsers_and_our_process_do_not_gain_browser_authority() {
        for app in [
            "brave",
            "chromium (deleted)",
            "python3",
            "google-chrome-beta",
        ] {
            assert!(kind(app).is_none());
        }
        for k in [Kind::Chrome, Kind::Chromium, Kind::Firefox] {
            assert!(!executable(std::process::id(), k));
        }
    }
    #[test]
    fn only_a_passive_native_status_bubble_can_be_ignored() {
        let f = Fixture::new();
        f.nodes.borrow_mut().extend([
            (
                Metadata {
                    pid: 42,
                    role: Role::Frame,
                    children: 1,
                    states: [State::Showing].into_iter().collect(),
                },
                0,
                vec![5],
            ),
            (
                Metadata {
                    pid: 42,
                    role: Role::Panel,
                    children: 1,
                    states: [State::Showing].into_iter().collect(),
                },
                4,
                vec![6],
            ),
            (
                Metadata {
                    pid: 42,
                    role: Role::Static,
                    children: 0,
                    states: [State::Showing].into_iter().collect(),
                },
                5,
                vec![],
            ),
        ]);
        f.set_children(0, vec![1, 2, 4]);
        assert!(matches!(snapshot_of(&f), Snapshot::Public(_)));
        for state in [State::Active, State::Focused, State::Focusable] {
            f.nodes.borrow_mut()[5].0.states.insert(state);
            assert_eq!(snapshot_of(&f), Snapshot::Unknown);
            f.nodes.borrow_mut()[5].0.states.remove(state);
        }
        f.nodes.borrow_mut()[6].0.role = Role::DocumentWeb;
        assert_eq!(snapshot_of(&f), Snapshot::Unknown);
        assert_eq!(f.content_reads.get(), 0);
    }
}
