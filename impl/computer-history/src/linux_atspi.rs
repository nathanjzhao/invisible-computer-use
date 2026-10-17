//! Content is fetched only after metadata admission. Event queues contain references,
//! never signal text, names, values, titles or a printable-key history.
use crate::{capture::Content, Error, Result};
use atspi::{connection::AccessibilityConnection, Role, State, StateSet};
use futures_util::StreamExt;
use std::{
    collections::HashSet,
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use zbus::{proxy::CacheProperties, zvariant::OwnedObjectPath};

pub(crate) const BUDGET: Duration = Duration::from_millis(750);
pub(crate) fn unavailable() -> Error {
    Error::new(
        "computerHistoryAccessibilityUnavailable",
        "AT-SPI metadata is unavailable or exceeded its deadline",
    )
}
pub(crate) fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("History runtime")
    })
}
pub(crate) async fn call<T, E>(f: impl Future<Output = std::result::Result<T, E>>) -> Result<T> {
    tokio::time::timeout(Duration::from_millis(150), f)
        .await
        .map_err(|_| unavailable())?
        .map_err(|_| unavailable())
}
pub(crate) fn run<T>(f: impl Future<Output = Result<T>>) -> Result<T> {
    runtime().block_on(async {
        tokio::time::timeout(BUDGET, f)
            .await
            .map_err(|_| unavailable())?
    })
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ObjectKey {
    pub(crate) bus: String,
    pub(crate) path: String,
}
impl ObjectKey {
    fn from_wire((bus, path): (String, OwnedObjectPath)) -> Self {
        Self {
            bus,
            path: path.to_string(),
        }
    }
    fn web(&self) -> bool {
        let bus = self.bus.to_ascii_lowercase();
        bus.contains("webkit") || bus.contains("webprocess") || !self.bus.starts_with(':')
    }
    pub(crate) fn window_id(&self) -> i64 {
        // Stable positive ID. Full references, not hashes, are used for every safety comparison.
        let mut hash = 0x811c9dc5u32;
        for b in self.bus.bytes().chain([0]).chain(self.path.bytes()) {
            hash = (hash ^ u32::from(b)).wrapping_mul(0x01000193);
        }
        i64::from(hash.max(1))
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Metadata {
    pub pid: u32,
    pub role: Role,
    pub states: StateSet,
    pub children: i32,
}
fn safe_role(role: Role) -> bool {
    !matches!(
        role,
        Role::Invalid
            | Role::Unknown
            | Role::PasswordText
            | Role::DocumentWeb
            | Role::DocumentFrame
            | Role::Embedded
            | Role::DocumentEmail
    )
}
fn safe(meta: &Metadata, pid: u32) -> bool {
    meta.pid == pid
        && safe_role(meta.role)
        && !meta.states.contains(State::Defunct)
        && !meta.states.contains(State::Invalid)
        && !meta.states.contains(State::Stale)
}
fn frame(role: Role) -> bool {
    matches!(role, Role::Frame | Role::Window | Role::Dialog)
}
fn content_role(role: Role) -> Option<&'static str> {
    Some(match role {
        Role::Entry => "AXTextField",
        Role::Text => "AXTextArea",
        Role::Label | Role::Static => "AXStaticText",
        Role::Button | Role::ToggleButton => "AXButton",
        Role::CheckBox => "AXCheckBox",
        Role::RadioButton => "AXRadioButton",
        Role::Slider | Role::SpinButton => "AXSlider",
        Role::Terminal => "AXTerminal",
        _ => return None,
    })
}
pub(crate) trait Tree {
    async fn metadata(&self, key: &ObjectKey) -> Result<Metadata>;
    async fn parent(&self, key: &ObjectKey) -> Result<ObjectKey>;
    async fn children(&self, key: &ObjectKey) -> Result<Vec<ObjectKey>>;
    async fn content(
        &self,
        key: &ObjectKey,
        role: Role,
        values: bool,
        limit: i32,
    ) -> Result<Content>;
}
// The entire admission/read/recheck operation also has a 750 ms cancellation deadline.
async fn ancestry(
    t: &impl Tree,
    key: &ObjectKey,
    root: &ObjectKey,
    pid: u32,
) -> Result<Option<Vec<(ObjectKey, Metadata)>>> {
    let mut current = key.clone();
    let mut seen = HashSet::new();
    let mut chain = Vec::new();
    for _ in 0..48 {
        if current.web() || !seen.insert(current.clone()) {
            return Ok(None);
        }
        let meta = t.metadata(&current).await?;
        if !safe(&meta, pid) {
            return Ok(None);
        }
        let reached = current == *root;
        if frame(meta.role) && !reached {
            return Ok(None);
        }
        chain.push((current.clone(), meta.clone()));
        if reached {
            return Ok((frame(meta.role)
                && meta.states.contains(State::Active)
                && !meta.states.contains(State::Iconified))
            .then_some(chain));
        }
        current = t.parent(&current).await?;
    }
    Ok(None)
}
async fn focused_reference(
    t: &impl Tree,
    key: &ObjectKey,
    root: &ObjectKey,
    pid: u32,
) -> Result<bool> {
    Ok(ancestry(t, key, root, pid).await?.is_some_and(|chain| {
        chain[0].1.states.contains(State::Focused) && chain[0].1.states.contains(State::Showing)
    }))
}
pub(crate) async fn read_checked(
    t: &impl Tree,
    key: &ObjectKey,
    root: &ObjectKey,
    pid: u32,
    values: bool,
    focused: bool,
    terminal: bool,
) -> Result<Option<Content>> {
    let Some(before) = ancestry(t, key, root, pid).await? else {
        return Ok(None);
    };
    let meta = &before[0].1;
    if !meta.states.contains(State::Showing)
        || (focused && !meta.states.contains(State::Focused))
        || meta.children != 0
        || content_role(meta.role).is_none()
        || (meta.role == Role::Terminal) != terminal
    {
        return Ok(None);
    }
    let content = t
        .content(key, meta.role, values, if terminal { 32769 } else { 2049 })
        .await?;
    let Some(after) = ancestry(t, key, root, pid).await? else {
        return Ok(None);
    };
    // Recheck the complete chain and safety state, including focus and child count.
    Ok((before == after).then_some(content))
}
fn contains_point(rect: (i32, i32, i32, i32), x: i32, y: i32) -> bool {
    rect.2 > 0
        && rect.3 > 0
        && x >= rect.0
        && y >= rect.1
        && i64::from(x) < i64::from(rect.0) + i64::from(rect.2)
        && i64::from(y) < i64::from(rect.1) + i64::from(rect.3)
}
fn hit_coordinates(
    accessible: (i32, i32, i32, i32),
    native: (i32, i32, i32, i32),
    x: i32,
    y: i32,
) -> Option<(i32, i32, u32)> {
    if !contains_point(native, x, y) {
        return None;
    }
    // GTK4's X11 provider returns a zero origin for DESKTOP_COORDS, including
    // every child. WINDOW_COORDS retains the actual individual widget offsets.
    // Translate only an exact client-size match for the already admitted XID.
    if accessible.0 == 0
        && accessible.1 == 0
        && (accessible.2, accessible.3) == (native.2, native.3)
    {
        Some((x.checked_sub(native.0)?, y.checked_sub(native.1)?, 1))
    } else if rectangles_match(accessible, native) {
        Some((x, y, 0))
    } else {
        None
    }
}
fn unique_point(mut candidates: Vec<ObjectKey>) -> Option<ObjectKey> {
    if candidates.len() != 1 {
        return None;
    }
    candidates.pop()
}
// Decoration differences are bounded, and *every* same-process frame competes.
// Matching geometry is never sufficient if a second frame could be the native XID.
fn rectangles_match(a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)) -> bool {
    a.2 > 0
        && a.3 > 0
        && b.2 > 0
        && b.3 > 0
        && [a.0, a.1, a.2, a.3]
            .into_iter()
            .zip([b.0, b.1, b.2, b.3])
            .all(|(x, y)| i64::from(x).abs_diff(i64::from(y)) <= 64)
}
fn select_x11_frame(mut candidates: Vec<(ObjectKey, bool)>) -> Result<ObjectKey> {
    if candidates.len() != 1 || !candidates[0].1 {
        return Err(Error::new(
            "computerHistoryAmbiguousFrame",
            "X11 geometry and active AT-SPI frame do not identify exactly one window",
        ));
    }
    Ok(candidates.remove(0).0)
}
pub(crate) struct Inspection {
    pub native: bool,
    pub focused: Option<ObjectKey>,
}
fn web_role(role: Role) -> bool {
    matches!(
        role,
        Role::DocumentWeb | Role::DocumentFrame | Role::Embedded | Role::DocumentEmail
    )
}
// A window is native only if the complete bounded metadata walk proves it. A
// browser's toolbar is a sibling of its document, so ancestry alone cannot admit it.
async fn inspect_frame(t: &impl Tree, root: &ObjectKey, pid: u32) -> Result<Inspection> {
    let mut pending = vec![root.clone()];
    let mut seen = HashSet::new();
    let mut focused = None;
    while let Some(key) = pending.pop() {
        if seen.len() >= 512 || !seen.insert(key.clone()) {
            return Err(Error::new(
                "computerHistoryTreeBudget",
                "Accessible window exceeds the metadata scan budget",
            ));
        }
        if key.web() {
            return Ok(Inspection {
                native: false,
                focused: None,
            });
        }
        let m = t.metadata(&key).await?;
        if m.pid != pid || web_role(m.role) {
            return Ok(Inspection {
                native: false,
                focused: None,
            });
        }
        if m.states.contains(State::Focused) {
            if focused.is_some() {
                return Err(Error::new(
                    "computerHistoryAmbiguousFocus",
                    "Multiple objects claim focus",
                ));
            }
            focused = Some(key.clone());
        }
        if m.children > 512 {
            return Err(Error::new(
                "computerHistoryTreeBudget",
                "Accessible child count exceeds the metadata scan budget",
            ));
        }
        pending.extend(t.children(&key).await?.into_iter().rev());
    }
    Ok(Inspection {
        native: true,
        focused,
    })
}
#[derive(Clone)]
pub(crate) struct Atspi {
    pub connection: AccessibilityConnection,
    owners: Arc<std::sync::Mutex<std::collections::HashMap<String, u32>>>,
}
impl Atspi {
    pub fn new() -> Result<Self> {
        run(async {
            Ok(Self {
                connection: call(AccessibilityConnection::new()).await?,
                owners: Default::default(),
            })
        })
    }
    pub(crate) async fn proxy<'a>(
        &'a self,
        key: &ObjectKey,
        interface: &'static str,
    ) -> Result<zbus::Proxy<'a>> {
        call(
            zbus::proxy::Builder::<zbus::Proxy>::new(self.connection.connection())
                .destination(key.bus.clone())
                .map_err(|_| unavailable())?
                .path(key.path.clone())
                .map_err(|_| unavailable())?
                .interface(interface)
                .map_err(|_| unavailable())?
                .cache_properties(CacheProperties::No)
                .build(),
        )
        .await
    }
    pub async fn pid(&self, key: &ObjectKey) -> Result<u32> {
        // Unique names cannot change owner during this connection's lifetime.
        if !key.bus.starts_with(':') {
            return Err(unavailable());
        }
        if let Some(pid) = self
            .owners
            .lock()
            .map_err(|_| unavailable())?
            .get(&key.bus)
            .copied()
        {
            return Ok(pid);
        }
        let bus = call(zbus::fdo::DBusProxy::new(self.connection.connection())).await?;
        let pid = call(bus.get_connection_unix_process_id(
            key.bus.as_str().try_into().map_err(|_| unavailable())?,
        ))
        .await?;
        let mut owners = self.owners.lock().map_err(|_| unavailable())?;
        if owners.len() >= 128 {
            owners.clear();
        }
        owners.insert(key.bus.clone(), pid);
        Ok(pid)
    }
    async fn frames(&self, pid: Option<u32>) -> Result<Vec<(ObjectKey, Metadata)>> {
        // Registry enumeration reads only object references and numeric metadata.
        let root = call(self.connection.root_accessible_on_registry()).await?;
        let apps = call(root.get_children()).await?;
        if apps.len() > 128 {
            return Err(unavailable());
        }
        let mut found = Vec::new();
        for app in apps {
            let Some(bus) = app.name_as_str() else {
                return Err(unavailable());
            };
            let app = ObjectKey {
                bus: bus.into(),
                path: app.path_as_str().into(),
            };
            if app.web() {
                continue;
            }
            let owner = self.pid(&app).await?;
            if pid.is_some_and(|p| p != owner) {
                continue;
            }
            let frames = self.children(&app).await?;
            if frames.len() > 128 {
                return Err(unavailable());
            }
            for key in frames {
                if key.web() {
                    continue;
                }
                let m = self.metadata(&key).await?;
                if frame(m.role) && safe(&m, owner) && !m.states.contains(State::Iconified) {
                    found.push((key, m));
                }
            }
        }
        Ok(found)
    }
    pub async fn active_frame(&self, pid: Option<u32>) -> Result<(ObjectKey, u32)> {
        let frames = self.frames(pid).await?;
        let active: Vec<_> = frames
            .iter()
            .filter(|(_, m)| m.states.contains(State::Active))
            .collect();
        if active.len() != 1
            || frames
                .iter()
                .filter(|(_, m)| m.pid == active[0].1.pid)
                .count()
                != 1
        {
            return Err(Error::new(
                "computerHistoryAmbiguousFrame",
                "Wayland needs one active AT-SPI frame and one native frame for its process",
            ));
        }
        Ok((active[0].0.clone(), active[0].1.pid))
    }
    pub(crate) async fn active_browser_frame(&self, pid: u32, kind: crate::linux::browser_native::Kind) -> Result<(ObjectKey, u32)> {
        let mut candidates = Vec::new();
        for (key, m) in self.frames(Some(pid)).await? {
            if crate::linux::browser_native::native_frame(self, &key, kind).await? {
                candidates.push((key, m));
            } else if m.states.contains(State::Active) {
                return Err(unavailable());
            }
        }
        if candidates.len() != 1 || !candidates[0].1.states.contains(State::Active) {
            return Err(unavailable());
        }
        Ok((candidates.remove(0).0, pid))
    }
    pub async fn x11_frame(&self, pid: u32, bounds: (i32, i32, i32, i32)) -> Result<ObjectKey> {
        let mut candidates = Vec::new();
        for (key, metadata) in self.frames(Some(pid)).await? {
            let p = self.proxy(&key, "org.a11y.atspi.Component").await?;
            let rect: (i32, i32, i32, i32) = call(p.call("GetExtents", &(0u32,))).await?;
            if rectangles_match(rect, bounds) {
                candidates.push((key, metadata.states.contains(State::Active)));
            }
        }
        select_x11_frame(candidates)
    }
    pub async fn focused(&self, root: &ObjectKey, pid: u32) -> Result<Option<ObjectKey>> {
        Ok(inspect_frame(self, root, pid).await?.focused)
    }
    pub async fn inspect(&self, root: &ObjectKey, pid: u32) -> Result<Inspection> {
        inspect_frame(self, root, pid).await
    }
    pub async fn verify_focus(&self, key: &ObjectKey, root: &ObjectKey, pid: u32) -> Result<bool> {
        focused_reference(self, key, root, pid).await
    }
    pub async fn hit(
        &self,
        root: &ObjectKey,
        pid: u32,
        x: i32,
        y: i32,
        native: (i32, i32, i32, i32),
    ) -> Result<Option<ObjectKey>> {
        let component = self.proxy(root, "org.a11y.atspi.Component").await?;
        let rect = call(component.call("GetExtents", &(0u32,))).await?;
        let Some((x, y, coordinates)) = hit_coordinates(rect, native, x, y) else {
            return Ok(None);
        };
        self.hit_at(root,pid,x,y,coordinates).await
    }
    pub(crate) async fn hit_wayland(
        &self, root: &ObjectKey, pid: u32, x: i32, y: i32, native: (i32,i32,i32,i32),
    ) -> Result<Option<ObjectKey>> {
        // The compositor already bound this exact accessible frame. Wayland
        // does not expose usable desktop coordinates through every toolkit.
        let component=self.proxy(root,"org.a11y.atspi.Component").await?;
        let rect:(i32,i32,i32,i32)=call(component.call("GetExtents",&(1u32,))).await?;
        if rect.0!=0 || rect.1!=0 || (rect.2,rect.3)!=(native.2,native.3) || !contains_point(native,x,y) {
            return Ok(None);
        }
        let Some(x)=x.checked_sub(native.0) else {return Ok(None);};
        let Some(y)=y.checked_sub(native.1) else {return Ok(None);};
        self.hit_at(root,pid,x,y,1).await
    }
    async fn hit_at(&self,root:&ObjectKey,pid:u32,x:i32,y:i32,coordinates:u32)->Result<Option<ObjectKey>> {
        if let Ok(Some(key)) = self.direct_hit(root, pid, x, y, coordinates).await {
            return Ok(Some(key));
        }
        let mut pending = vec![root.clone()];
        let mut seen = HashSet::new();
        let mut found = Vec::new();
        while let Some(key) = pending.pop() {
            if seen.len() >= 256 || !seen.insert(key.clone()) || key.web() {
                return Ok(None);
            }
            let m = self.metadata(&key).await?;
            if !m.states.contains(State::Showing) {
                continue;
            }
            let p = self.proxy(&key, "org.a11y.atspi.Component").await?;
            let rect: (i32, i32, i32, i32) = call(p.call("GetExtents", &(coordinates,))).await?;
            if !contains_point(rect, x, y) {
                continue;
            }
            if !safe(&m, pid) || (key != *root && frame(m.role)) {
                return Ok(None);
            }
            if m.children == 0 {
                found.push(key)
            } else {
                pending.extend(self.children(&key).await?);
            }
        }
        Ok(unique_point(found))
    }
    async fn direct_hit(
        &self,
        root: &ObjectKey,
        pid: u32,
        x: i32,
        y: i32,
        coordinates: u32,
    ) -> Result<Option<ObjectKey>> {
        let mut key = root.clone();
        let mut seen = HashSet::new();
        for _ in 0..48 {
            if key.web() || !seen.insert(key.clone()) {
                return Ok(None);
            }
            let m = self.metadata(&key).await?;
            if !safe(&m, pid) {
                return Ok(None);
            }
            if m.children == 0 {
                return Ok(Some(key));
            }
            let proxy = self.proxy(&key, "org.a11y.atspi.Component").await?;
            let raw: (String, OwnedObjectPath) =
                call(proxy.call("GetAccessibleAtPoint", &(x, y, coordinates))).await?;
            let child = ObjectKey::from_wire(raw);
            if child == key {
                return Ok(None);
            }
            key = child;
        }
        Ok(None)
    }
    pub async fn snapshot(
        &self,
        root: &ObjectKey,
        pid: u32,
        terminal: bool,
    ) -> Result<Option<(String, bool)>> {
        let mut pending = vec![root.clone()];
        let mut seen = HashSet::new();
        let mut rows = Vec::new();
        let mut partial = false;
        while let Some(key) = pending.pop() {
            if seen.len() >= 256 || !seen.insert(key.clone()) {
                partial = true;
                break;
            }
            if key.web() {
                partial = true;
                continue;
            }
            let m = self.metadata(&key).await?;
            if !safe(&m, pid) || (key != *root && frame(m.role)) {
                partial = true;
                continue;
            }
            if m.children == 0
                && content_role(m.role).is_some()
                && (m.role == Role::Terminal) == terminal
            {
                if let Some(c) = read_checked(self, &key, root, pid, true, false, terminal).await? {
                    if terminal {
                        rows.push(c.value.unwrap_or_default());
                    } else {
                        rows.push(format!(
                            "{} {}: {}",
                            c.role,
                            serde_json::to_string(&c.label.unwrap_or_default()).unwrap(),
                            serde_json::to_string(&c.value.unwrap_or_default()).unwrap()
                        ));
                    }
                } else {
                    partial = true
                }
            } else if m.role != Role::Terminal {
                pending.extend(self.children(&key).await?.into_iter().rev())
            }
            if rows.iter().map(String::len).sum::<usize>() > 32768 {
                partial = true;
                break;
            }
        }
        if rows.is_empty() {
            return Ok(None);
        }
        if terminal && rows.len() != 1 {
            return Ok(None);
        }
        if partial && !terminal {
            rows.push("[Partial accessible window snapshot]".into())
        }
        Ok(Some((rows.join("\n"), partial)))
    }
}
impl Tree for Atspi {
    async fn metadata(&self, key: &ObjectKey) -> Result<Metadata> {
        // Never build a property cache: GetAll could fetch Name before admission (and crashes Qt5).
        let p = self.proxy(key, "org.a11y.atspi.Accessible").await?;
        let pid = self.pid(key).await?;
        let role = call(p.call("GetRole", &())).await?;
        let states = call(p.call("GetState", &())).await?;
        let children = call(p.get_property("ChildCount")).await?;
        Ok(Metadata {
            pid,
            role,
            states,
            children,
        })
    }
    async fn parent(&self, key: &ObjectKey) -> Result<ObjectKey> {
        let p = self.proxy(key, "org.a11y.atspi.Accessible").await?;
        Ok(ObjectKey::from_wire(call(p.get_property("Parent")).await?))
    }
    async fn children(&self, key: &ObjectKey) -> Result<Vec<ObjectKey>> {
        let p = self.proxy(key, "org.a11y.atspi.Accessible").await?;
        let refs: Vec<(String, OwnedObjectPath)> = call(p.call("GetChildren", &())).await?;
        if refs.len() > 512 {
            return Err(unavailable());
        }
        Ok(refs.into_iter().map(ObjectKey::from_wire).collect())
    }
    async fn content(
        &self,
        key: &ObjectKey,
        role: Role,
        values: bool,
        limit: i32,
    ) -> Result<Content> {
        let p = self.proxy(key, "org.a11y.atspi.Accessible").await?;
        let name: String = if role == Role::Terminal {
            String::new()
        } else {
            call(p.get_property("Name")).await?
        };
        // AccessibleId is optional on older bridges. It is never synthesized from text.
        let id: String = call(p.get_property("AccessibleId"))
            .await
            .unwrap_or_default();
        let mut value = None;
        if values {
            let interfaces: Vec<String> = call(p.call("GetInterfaces", &())).await?;
            if interfaces.iter().any(|i| i == "org.a11y.atspi.Value") {
                let v = self.proxy(key, "org.a11y.atspi.Value").await?;
                let n: f64 = call(v.get_property("CurrentValue")).await?;
                if n.is_finite() {
                    value = Some(n.to_string())
                }
            } else if interfaces.iter().any(|i| i == "org.a11y.atspi.Text") {
                let t = self.proxy(key, "org.a11y.atspi.Text").await?;
                let count: i32 = call(t.get_property("CharacterCount")).await?;
                if count < 0 {
                    return Err(unavailable());
                }
                value = Some(
                    call(t.call::<_, _, String>("GetText", &(0i32, count.min(limit))))
                        .await?
                        .chars()
                        .take(limit as usize)
                        .collect(),
                );
            }
        }
        Ok(Content {
            role: content_role(role).unwrap_or_default().into(),
            label: (!name.is_empty()).then(|| name.chars().take(513).collect()),
            identifier: (!id.is_empty()).then(|| id.chars().take(257).collect()),
            value,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    Focus,
    Value,
    Invoke,
    Window,
}
#[derive(Clone, Debug)]
pub struct PendingEvent {
    pub timestamp_ms: u64,
    pub(crate) received: Instant,
    pub object: ObjectKey,
    pub kind: EventKind,
}
fn notification(message: &zbus::Message, root: &ObjectKey) -> Result<Option<PendingEvent>> {
    use atspi::{Event, ObjectEvents};
    // Filter by immutable sender before reading metadata. Text/value/window
    // signal bodies may contain protected content and are never decoded.
    let header = message.header();
    let Some(bus) = header.sender() else {
        return Ok(None);
    };
    if bus.as_str() != root.bus {
        return Ok(None);
    }
    let Some(path) = header.path() else {
        return Ok(None);
    };
    let interface = header.interface().map(|s| s.as_str()).unwrap_or("");
    let member = header.member().map(|s| s.as_str()).unwrap_or("");
    let kind = match (interface, member) {
        ("org.a11y.atspi.Event.Window", "Activate" | "Deactivate") => EventKind::Window,
        ("org.a11y.atspi.Event.Object", "TextChanged" | "PropertyChange") => EventKind::Value,
        ("org.a11y.atspi.Event.Object", "StateChanged") => {
            match Event::try_from(message).map_err(|_| unavailable())? {
                Event::Object(ObjectEvents::StateChanged(e)) if e.state == State::Focused => {
                    EventKind::Focus
                }
                Event::Object(ObjectEvents::StateChanged(e))
                    if (e.enabled && matches!(e.state, State::Pressed | State::Selected))
                        || e.state == State::Checked =>
                {
                    EventKind::Invoke
                }
                _ => return Ok(None),
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(PendingEvent {
        object: ObjectKey {
            bus: bus.to_string(),
            path: path.to_string(),
        },
        kind,
        timestamp_ms: crate::now_ms(),
        received: Instant::now(),
    }))
}
fn event_match(root: &ObjectKey) -> zbus::MatchRule<'_> {
    zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(root.bus.as_str())
        .expect("admitted unique bus name")
        .build()
}
pub struct Events {
    events: Receiver<PendingEvent>,
    lost: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Events {
    pub(crate) fn start(atspi: Atspi, root: ObjectKey) -> Result<Self> {
        let (send, events) = mpsc::sync_channel(128);
        let lost = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicBool::new(true));
        let running = active.clone();
        let flag = lost.clone();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("history-atspi-events".into())
            .spawn(move || {
                runtime().block_on(async move {
                    use atspi::{ObjectEvents, WindowEvents};
                    let c = &atspi.connection;
                    let register = async {
                        call(c.add_registry_event::<ObjectEvents>()).await?;
                        call(c.add_registry_event::<WindowEvents>()).await
                    };
                    if run_async(register).await.is_err() {
                        let _ = ready_tx.send(false);
                        return;
                    }
                    // add_registry_event calls RegisterEvent only; unlike register_event
                    // it does NOT install the crate's unfiltered ObjectEvents match.
                    let rule = event_match(&root);
                    let Ok(mut stream) = call(zbus::MessageStream::for_match_rule(
                        rule,
                        c.connection(),
                        Some(128),
                    ))
                    .await
                    else {
                        let _ = ready_tx.send(false);
                        return;
                    };
                    let _ = ready_tx.send(true);
                    while running.load(Ordering::SeqCst) {
                        let event =
                            match tokio::time::timeout(Duration::from_millis(50), stream.next())
                                .await
                            {
                                Err(_) => continue,
                                Ok(Some(Ok(e))) => e,
                                _ => {
                                    flag.store(true, Ordering::SeqCst);
                                    break;
                                }
                            };
                        let event = match notification(&event, &root) {
                            Ok(Some(event)) => event,
                            Ok(None) => continue,
                            Err(_) => {
                                flag.store(true, Ordering::SeqCst);
                                continue;
                            }
                        };
                        if running.load(Ordering::SeqCst) && send.try_send(event).is_err() {
                            flag.store(true, Ordering::SeqCst)
                        }
                    }
                    let _ = call(c.remove_registry_event::<ObjectEvents>()).await;
                    let _ = call(c.remove_registry_event::<WindowEvents>()).await;
                })
            })
            .map_err(|_| unavailable())?;
        if ready_rx.recv_timeout(Duration::from_secs(2)) != Ok(true) {
            active.store(false, Ordering::SeqCst);
            let _ = worker.join();
            return Err(unavailable());
        }
        Ok(Self {
            events,
            lost,
            active,
            worker: Some(worker),
        })
    }
    pub fn drain(&self) -> (Vec<PendingEvent>, bool) {
        let lost = self.lost.swap(false, Ordering::SeqCst);
        let events: Vec<_> = self.events.try_iter().collect();
        // Overflow invalidates the whole backlog, including invocation ordering.
        if lost {
            return (vec![], true);
        }
        let mut partial = false;
        let events = events
            .into_iter()
            .filter(|e| {
                let fresh = e.received.elapsed() <= Duration::from_secs(2);
                partial |= !fresh;
                fresh
            })
            .collect();
        (events, partial)
    }
}
async fn run_async<T>(f: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(BUDGET, f)
        .await
        .map_err(|_| unavailable())?
}
impl Drop for Events {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
        if let Some(t) = self.worker.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, collections::HashMap};
    fn key(n: usize) -> ObjectKey {
        ObjectKey {
            bus: ":1.23".into(),
            path: format!("/object/{n}"),
        }
    }
    struct Fake {
        nodes: HashMap<ObjectKey, (Metadata, Option<ObjectKey>)>,
        reads: Cell<usize>,
        change: bool,
        delay: bool,
        change_state: bool,
        metadata_reads: Cell<usize>,
    }
    impl Fake {
        fn new() -> Self {
            let leaf = Metadata {
                pid: 42,
                role: Role::Entry,
                states: StateSet::from(State::Showing) | StateSet::from(State::Focused),
                children: 0,
            };
            let root = Metadata {
                pid: 42,
                role: Role::Frame,
                states: StateSet::from(State::Active),
                children: 1,
            };
            Self {
                nodes: HashMap::from([(key(0), (leaf, Some(key(1)))), (key(1), (root, None))]),
                reads: Cell::new(0),
                change: false,
                delay: false,
                change_state: false,
                metadata_reads: Cell::new(0),
            }
        }
        fn m(&mut self, n: usize) -> &mut Metadata {
            &mut self.nodes.get_mut(&key(n)).unwrap().0
        }
        fn read(&self) -> Result<Option<Content>> {
            run(read_checked(self, &key(0), &key(1), 42, true, true, false))
        }
        fn refused(&self) {
            assert!(self.read().unwrap().is_none());
            assert_eq!(self.reads.get(), 0, "content was queried before admission")
        }
    }
    impl Tree for Fake {
        async fn metadata(&self, k: &ObjectKey) -> Result<Metadata> {
            self.metadata_reads.set(self.metadata_reads.get() + 1);
            if self.delay {
                tokio::time::sleep(Duration::from_secs(1)).await
            }
            let mut m = self.nodes.get(k).ok_or_else(unavailable)?.0.clone();
            if self.change && self.reads.get() > 0 {
                m.role = Role::PasswordText
            }
            if self.change_state && self.reads.get() > 0 {
                m.states.insert(State::Enabled);
            }
            Ok(m)
        }
        async fn parent(&self, k: &ObjectKey) -> Result<ObjectKey> {
            self.nodes
                .get(k)
                .and_then(|n| n.1.clone())
                .ok_or_else(unavailable)
        }
        async fn children(&self, k: &ObjectKey) -> Result<Vec<ObjectKey>> {
            Ok(self
                .nodes
                .iter()
                .filter(|(_, v)| v.1.as_ref() == Some(k))
                .map(|(k, _)| k.clone())
                .collect())
        }
        async fn content(
            &self,
            _: &ObjectKey,
            _: Role,
            values: bool,
            limit: i32,
        ) -> Result<Content> {
            self.reads.set(self.reads.get() + 1);
            assert_eq!(limit, 2049);
            Ok(Content {
                role: "AXTextField".into(),
                label: None,
                identifier: Some("draft".into()),
                value: values.then(|| "Cedar note".into()),
            })
        }
    }
    #[test]
    fn sibling_web_document_refuses_browser_chrome() {
        for role in [
            Role::DocumentWeb,
            Role::DocumentFrame,
            Role::Embedded,
            Role::DocumentEmail,
        ] {
            let mut f = Fake::new();
            let mut document = f.m(0).clone();
            document.role = role;
            document.states = StateSet::empty();
            f.nodes.insert(key(2), (document, Some(key(1))));
            f.m(1).children = 2;
            assert!(!run(inspect_frame(&f, &key(1), 42)).unwrap().native);
            assert_eq!(f.reads.get(), 0);
        }
    }
    #[test]
    fn large_tree_refuses_and_small_tree_recovers_without_content_read() {
        let mut f = Fake::new();
        let mut m = f.m(0).clone();
        m.role = Role::Panel;
        m.states = StateSet::empty();
        for i in 2..2202 {
            f.nodes.insert(key(i), (m.clone(), Some(key(1))));
        }
        f.m(1).children = 2201;
        assert_eq!(
            run(inspect_frame(&f, &key(1), 42)).err().unwrap().code,
            "computerHistoryTreeBudget"
        );
        assert_eq!(
            f.metadata_reads.get(),
            1,
            "oversized children were traversed"
        );
        // Also exercise the total-node bound independently of the child-count bound.
        f.m(1).children = 2;
        assert_eq!(
            run(inspect_frame(&f, &key(1), 42)).err().unwrap().code,
            "computerHistoryTreeBudget"
        );
        assert_eq!(f.reads.get(), 0);
        f.nodes.retain(|k, _| *k == key(0) || *k == key(1));
        f.m(1).children = 1;
        let scan = run(inspect_frame(&f, &key(1), 42)).unwrap();
        assert!(scan.native);
        assert_eq!(scan.focused, Some(key(0)));
        assert!(f.read().unwrap().is_some());
    }
    #[test]
    fn cached_focus_checks_ancestry_without_walking_siblings() {
        let mut f = Fake::new();
        for i in 2..2202 {
            f.nodes
                .insert(key(i), (f.nodes[&key(0)].0.clone(), Some(key(1))));
        }
        assert!(run(focused_reference(&f, &key(0), &key(1), 42)).unwrap());
        assert_eq!(f.metadata_reads.get(), 2);
        f.m(0).states.remove(State::Focused);
        assert!(!run(focused_reference(&f, &key(0), &key(1), 42)).unwrap());
        assert_eq!(f.reads.get(), 0);
    }
    #[test]
    fn event_subscription_is_sender_filtered() {
        let rule = event_match(&key(1)).to_string();
        assert!(rule.contains("sender=':1.23'"), "{rule}");
        assert!(rule.contains("type='signal'"), "{rule}");
    }
    #[test]
    fn happy_path_keeps_value_separate_from_label() {
        let f = Fake::new();
        let c = f.read().unwrap().unwrap();
        assert_eq!(c.value.as_deref(), Some("Cedar note"));
        assert_eq!(c.label, None);
        assert_eq!(f.reads.get(), 1)
    }
    #[test]
    fn semantic_scope_never_requests_value() {
        let f = Fake::new();
        let c = run(read_checked(&f, &key(0), &key(1), 42, false, true, false))
            .unwrap()
            .unwrap();
        assert_eq!(c.value, None)
    }
    #[test]
    fn password_role_refused() {
        let mut f = Fake::new();
        f.m(0).role = Role::PasswordText;
        assert!(!safe(f.m(0), 42));
        f.refused()
    }
    #[test]
    fn unknown_role_refused() {
        let mut f = Fake::new();
        f.m(0).role = Role::Unknown;
        assert!(!safe(f.m(0), 42));
        f.refused()
    }
    #[test]
    fn invalid_role_refused() {
        let mut f = Fake::new();
        f.m(0).role = Role::Invalid;
        assert!(!safe(f.m(0), 42));
        f.refused()
    }
    #[test]
    fn web_document_refused() {
        for role in [
            Role::DocumentWeb,
            Role::DocumentFrame,
            Role::Embedded,
            Role::DocumentEmail,
        ] {
            let mut f = Fake::new();
            f.m(1).role = role;
            assert!(!safe(f.m(1), 42));
            f.refused()
        }
    }
    #[test]
    fn web_bus_refused() {
        let f = Fake::new();
        let mut web = key(0);
        web.bus = "org.webkit.WebProcess.fixture".into();
        assert!(run(read_checked(&f, &web, &key(1), 42, true, true, false))
            .unwrap()
            .is_none());
        assert_eq!(f.reads.get(), 0)
    }
    #[test]
    fn other_pid_refused() {
        let mut f = Fake::new();
        f.m(0).pid = 43;
        f.refused()
    }
    #[test]
    fn other_pid_parent_refused() {
        let mut f = Fake::new();
        f.m(1).pid = 43;
        f.refused()
    }
    #[test]
    fn defunct_refused() {
        let mut f = Fake::new();
        f.m(0).states.insert(State::Defunct);
        f.refused()
    }
    #[test]
    fn stale_refused() {
        let mut f = Fake::new();
        f.m(0).states.insert(State::Stale);
        f.refused()
    }
    #[test]
    fn invalid_state_refused() {
        let mut f = Fake::new();
        f.m(0).states.insert(State::Invalid);
        f.refused()
    }
    #[test]
    fn hidden_refused() {
        let mut f = Fake::new();
        f.m(0).states.remove(State::Showing);
        f.refused()
    }
    #[test]
    fn unfocused_refused() {
        let mut f = Fake::new();
        f.m(0).states.remove(State::Focused);
        f.refused()
    }
    #[test]
    fn container_content_refused() {
        let mut f = Fake::new();
        f.m(0).children = 1;
        f.refused()
    }
    #[test]
    fn inactive_frame_refused() {
        let mut f = Fake::new();
        f.m(1).states.remove(State::Active);
        f.refused()
    }
    #[test]
    fn iconic_frame_refused() {
        let mut f = Fake::new();
        f.m(1).states.insert(State::Iconified);
        f.refused()
    }
    #[test]
    fn sibling_frame_refused() {
        let mut f = Fake::new();
        f.nodes.insert(key(2), f.nodes[&key(1)].clone());
        f.nodes.get_mut(&key(0)).unwrap().1 = Some(key(2));
        f.refused()
    }
    #[test]
    fn cycle_refused() {
        let mut f = Fake::new();
        f.nodes.get_mut(&key(0)).unwrap().1 = Some(key(0));
        f.refused();
        assert!(f.metadata_reads.get() <= 2)
    }
    #[test]
    fn depth_bound_refused() {
        let mut f = Fake::new();
        let meta = Metadata {
            pid: 42,
            role: Role::Panel,
            states: StateSet::empty(),
            children: 1,
        };
        f.nodes.get_mut(&key(0)).unwrap().1 = Some(key(2));
        for i in 2..51 {
            f.nodes.insert(
                key(i),
                (
                    meta.clone(),
                    Some(if i == 50 { key(1) } else { key(i + 1) }),
                ),
            );
        }
        f.refused()
    }
    #[test]
    fn deadline_refused() {
        let mut f = Fake::new();
        f.delay = true;
        let start = Instant::now();
        assert!(f.read().is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
        assert_eq!(f.reads.get(), 0)
    }
    #[test]
    fn changed_role_after_read_refused() {
        let mut f = Fake::new();
        f.change = true;
        assert!(f.read().unwrap().is_none());
        assert_eq!(f.reads.get(), 1)
    }
    #[test]
    fn changed_state_after_read_refused() {
        let mut f = Fake::new();
        f.change_state = true;
        assert!(f.read().unwrap().is_none());
        assert_eq!(f.reads.get(), 1);
    }
    #[test]
    fn terminal_value_requires_dedicated_redactor() {
        let mut f = Fake::new();
        f.m(0).role = Role::Terminal;
        f.refused()
    }
    #[test]
    fn stable_positive_window_ids() {
        assert_eq!(key(0).window_id(), key(0).window_id());
        assert!(key(0).window_id() > 0);
        assert_ne!(key(0).window_id(), key(1).window_id())
    }
    #[test]
    fn lost_events_discard_backlog() {
        let (send, events) = mpsc::sync_channel(1);
        send.send(PendingEvent {
            timestamp_ms: 1,
            received: Instant::now(),
            object: key(0),
            kind: EventKind::Invoke,
        })
        .unwrap();
        let queue = Events {
            events,
            lost: Arc::new(AtomicBool::new(true)),
            active: Arc::new(AtomicBool::new(true)),
            worker: None,
        };
        let (events, lost) = queue.drain();
        assert!(lost);
        assert!(events.is_empty());
        assert_eq!(queue.drain().0.len(), 0)
    }
    #[test]
    fn old_events_refused() {
        let (send, events) = mpsc::sync_channel(1);
        send.send(PendingEvent {
            timestamp_ms: 1,
            received: Instant::now() - Duration::from_secs(3),
            object: key(0),
            kind: EventKind::Invoke,
        })
        .unwrap();
        let queue = Events {
            events,
            lost: Arc::new(AtomicBool::new(false)),
            active: Arc::new(AtomicBool::new(true)),
            worker: None,
        };
        let (events, partial) = queue.drain();
        assert!(partial);
        assert!(events.is_empty())
    }
    #[test]
    fn same_bounds_frames_refused() {
        assert!(select_x11_frame(vec![(key(0), true), (key(1), false)]).is_err());
    }
    #[test]
    fn different_native_frame_refused() {
        assert!(select_x11_frame(vec![(key(0), false)]).is_err());
    }
    #[test]
    fn unique_active_native_frame_allowed() {
        assert_eq!(select_x11_frame(vec![(key(0), true)]).unwrap(), key(0));
    }
    #[test]
    fn native_geometry_must_match() {
        assert!(rectangles_match((5, 20, 700, 600), (5, 20, 700, 600)));
        assert!(!rectangles_match((5, 20, 700, 600), (800, 20, 700, 600)));
        assert!(!rectangles_match((5, 20, 0, 600), (5, 20, 700, 600)));
    }
    #[test]
    fn gtk4_hit_uses_client_offsets_and_window_coordinates() {
        assert_eq!(
            hit_coordinates((0, 0, 800, 550), (0, 32, 800, 550), 400, 530),
            Some((400, 498, 1))
        );
        assert_eq!(
            hit_coordinates((0, 0, 800, 550), (240, 80, 800, 550), 640, 578),
            Some((400, 498, 1))
        );
        assert_eq!(
            hit_coordinates((0, 0, 800, 550), (0, 0, 800, 550), 400, 498),
            Some((400, 498, 1))
        );
    }
    #[test]
    fn normal_hit_preserves_desktop_coordinates() {
        assert_eq!(
            hit_coordinates((240, 80, 800, 550), (240, 80, 800, 550), 640, 578),
            Some((640, 578, 0))
        );
        // A zero-origin frame of a different size cannot authorize translation.
        assert_eq!(
            hit_coordinates((0, 0, 790, 550), (240, 80, 800, 550), 640, 578),
            None
        );
    }
    #[test]
    fn translated_hit_stays_in_exact_native_client() {
        for (x, y) in [(240, 79), (239, 80), (1040, 90), (250, 630)] {
            assert_eq!(
                hit_coordinates((0, 0, 800, 550), (240, 80, 800, 550), x, y),
                None
            );
        }
        assert_eq!(hit_coordinates((0, 0, 0, 550), (0, 0, 0, 550), 0, 0), None);
    }
    #[test]
    fn point_ambiguity_refused() {
        assert!(unique_point(vec![key(0), key(1)]).is_none());
        assert_eq!(unique_point(vec![key(0)]), Some(key(0)));
    }
    #[test]
    fn point_outside_refused() {
        assert!(contains_point((0, 0, 10, 10), 5, 5));
        assert!(!contains_point((0, 0, 10, 10), 10, 5));
        assert!(!contains_point((0, 0, 10, 10), 5, -1));
    }
    fn signal(sender: &str, interface: &str, member: &str) -> zbus::Message {
        zbus::Message::signal("/object/0", interface, member)
            .unwrap()
            .sender(sender)
            .unwrap()
            .build(&"PROTECTED SIGNAL CONTENT")
            .unwrap()
    }
    #[test]
    fn content_notifications_only_keep_reference() {
        for member in ["TextChanged", "PropertyChange"] {
            // This body cannot deserialize as an AT-SPI text/value event.
            // Classification must still work without accessing that content.
            let event = notification(
                &signal(":1.23", "org.a11y.atspi.Event.Object", member),
                &key(1),
            )
            .unwrap()
            .unwrap();
            assert_eq!(event.object, key(0));
            assert_eq!(event.kind, EventKind::Value);
        }
    }
    #[test]
    fn other_bus_notifications_ignored() {
        assert!(notification(
            &signal(":1.24", "org.a11y.atspi.Event.Object", "TextChanged"),
            &key(1)
        )
        .unwrap()
        .is_none());
    }
    #[test]
    fn unknown_state_notification_refused() {
        assert!(notification(
            &signal(":1.23", "org.a11y.atspi.Event.Object", "StateChanged"),
            &key(1)
        )
        .is_err());
    }
    #[test]
    fn unrelated_window_signal_ignored() {
        assert!(notification(
            &signal(":1.23", "org.a11y.atspi.Event.Window", "PropertyChange"),
            &key(1)
        )
        .unwrap()
        .is_none());
    }
}
