//! Bounded XInput2 device-event metadata with event-time coordinates and modifiers.
use crate::{
    capture::Content,
    linux::{LinuxSource, ObjectKey},
    linux_atspi as ax,
    policy::Context,
    Error, Result,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::{
        xinput::{self, ConnectionExt as _},
        xkb::{self, ConnectionExt as _},
        xproto::ConnectionExt as _,
        Event as XEvent,
    },
};
#[derive(Clone, Debug)]
pub enum Input {
    Click {
        right: bool,
    },
    Key {
        name: &'static str,
        ctrl: bool,
        alt: bool,
        shift: bool,
        win: bool,
    },
}
#[derive(Clone, Debug)]
pub struct Event {
    pub timestamp_ms: u64,
    pub object: ObjectKey,
    pub input: Input,
    pub(crate) received: Instant,
    pub(crate) frozen: Option<FrozenTarget>,
}
#[derive(Clone, Debug)]
pub(crate) struct FrozenTarget {
    pub(crate) frame: ObjectKey,
    pub(crate) content: Content,
}
impl Event {
    fn delivery_fresh(&self) -> bool {
        self.received.elapsed()
            <= if self.frozen.is_some() {
                MAX_DELIVERY_AGE
            } else {
                MAX_AGE
            }
    }
    fn frozen_content(&self, frame: &ObjectKey) -> Option<Content> {
        let captured = self.frozen.as_ref()?;
        (self.delivery_fresh() && captured.frame == *frame).then(|| captured.content.clone())
    }
    /// X11 resolves and reads the safe label at intake. A slow snapshot must
    /// not discard that bounded result or replace its label with later text.
    /// The caller still rechecks consent, policy and the native window before
    /// committing. Unresolved backends retain their original fresh-read path.
    pub fn content(&self, source: &mut LinuxSource, context: &Context) -> Result<Option<Content>> {
        if self.frozen.is_none() {
            return source.object_content(context, &self.object);
        }
        if !self.delivery_fresh() {
            return Ok(None);
        }
        let (_, frame) = source.input_scope(context)?;
        if !source.verify_window(context)? {
            return Ok(None);
        }
        Ok(self.frozen_content(&frame))
    }
}
pub struct InputSource {
    events: Receiver<Event>,
    lost: Arc<AtomicBool>,
    topology_changed: Arc<AtomicBool>,
    core_keyboard: bool,
    active: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
fn scope_changed(event: &XEvent) -> bool {
    matches!(
        event,
        XEvent::CreateNotify(_) | XEvent::DestroyNotify(_) | XEvent::ReparentNotify(_)
    )
}
fn scope_refusal() -> Error {
    Error::new(
        "computerHistoryInputScopeChanged",
        "The admitted native input scope changed; observation must reconnect before recording",
    )
}
fn keyboard_mask(core_keyboard: bool) -> xinput::XIEventMask {
    // XI2 device KeyPress selection can suppress delivery to clients using core
    // X11 keyboard events (including Qt). Preserve the application's input and
    // use a nonexclusive core subscription for that client's keyboard instead.
    if core_keyboard {
        xinput::XIEventMask::BUTTON_PRESS
    } else {
        xinput::XIEventMask::BUTTON_PRESS | xinput::XIEventMask::KEY_PRESS
    }
}
fn core_mask(enabled: bool) -> x11rb::protocol::xproto::EventMask {
    use x11rb::protocol::xproto::EventMask;
    if enabled {
        EventMask::SUBSTRUCTURE_NOTIFY | EventMask::KEY_PRESS | EventMask::KEY_RELEASE
    } else {
        EventMask::SUBSTRUCTURE_NOTIFY
    }
}
fn enable_core_keyboard(conn: &x11rb::rust_connection::RustConnection) -> Result<bool> {
    if !conn
        .xkb_use_extension(1, 0)
        .map_err(|_| ax::unavailable())?
        .reply()
        .map_err(|_| ax::unavailable())?
        .supported
    {
        return Ok(false);
    }
    let flag = xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT;
    let reply = conn
        .xkb_per_client_flags(
            xkb::ID::USE_CORE_KBD.into(),
            flag,
            flag,
            0u32.into(),
            0u32.into(),
            0u32.into(),
        )
        .map_err(|_| ax::unavailable())?
        .reply()
        .map_err(|_| ax::unavailable())?;
    Ok(reply.supported.contains(flag) && reply.value.contains(flag))
}
fn core_key_once(
    event: &XEvent,
    keys: &std::collections::HashMap<u32, &'static str>,
    down: &mut std::collections::HashSet<u8>,
) -> bool {
    match event {
        XEvent::KeyPress(key) => {
            key.response_type & 128 == 0
                && keys.contains_key(&u32::from(key.detail))
                && down.insert(key.detail)
        }
        XEvent::KeyRelease(key) => {
            if key.response_type & 128 == 0 {
                down.remove(&key.detail);
            }
            false
        }
        _ => true,
    }
}
fn key_name(symbol: u32) -> Option<&'static str> {
    match symbol {
        0xff0d | 0xff8d => Some("return"),
        0xff09 | 0xfe20 => Some("tab"),
        0xff1b => Some("escape"),
        _ => None,
    }
}
const MAX_AGE: Duration = Duration::from_millis(100);
// This budget applies only after the target and safe label were captured
// within MAX_AGE. It never permits delayed coordinate or focus resolution.
const MAX_DELIVERY_AGE: Duration = Duration::from_secs(2);
fn fresh(received: Instant) -> bool {
    received.elapsed() <= MAX_AGE
}
fn event_age(server_now: u32, event_time: u32) -> Option<Duration> {
    let age = Duration::from_millis(u64::from(server_now.wrapping_sub(event_time)));
    (age <= MAX_AGE).then_some(age)
}
struct PendingInput {
    input: Input,
    point: Option<(i32, i32)>,
    received: Instant,
    timestamp_ms: u64,
}
fn input_queue() -> (mpsc::SyncSender<PendingInput>, Receiver<PendingInput>) {
    mpsc::sync_channel(32)
}
fn input_metadata(
    event: XEvent,
    keys: &std::collections::HashMap<u32, &'static str>,
) -> Option<(Input, Option<(i32, i32)>, u32)> {
    Some(match event {
        XEvent::KeyPress(key) if key.response_type & 128 == 0 => {
            let name = keys.get(&u32::from(key.detail))?;
            let mask = u16::from(key.state);
            (
                Input::Key {
                    name,
                    ctrl: mask & 4 != 0,
                    alt: mask & 8 != 0,
                    shift: mask & 1 != 0,
                    win: mask & 64 != 0,
                },
                None,
                key.time,
            )
        }
        XEvent::XinputButtonPress(e) if e.response_type & 128 == 0 && matches!(e.detail, 1 | 3) => {
            (
                Input::Click {
                    right: e.detail == 3,
                },
                Some((e.root_x >> 16, e.root_y >> 16)),
                e.time,
            )
        }
        XEvent::XinputKeyPress(e)
            if e.response_type & 128 == 0
                && u32::from(e.flags) & u32::from(xinput::KeyEventFlags::KEY_REPEAT) == 0 =>
        {
            let name = keys.get(&e.detail)?;
            let mask = e.mods.effective;
            (
                Input::Key {
                    name,
                    ctrl: mask & 4 != 0,
                    alt: mask & 8 != 0,
                    shift: mask & 1 != 0,
                    win: mask & 64 != 0,
                },
                None,
                e.time,
            )
        }
        _ => return None,
    })
}
fn refresh_keys(
    keys: &mut std::collections::HashMap<u32, &'static str>,
    refreshed: std::collections::HashMap<u32, &'static str>,
) -> bool {
    if *keys == refreshed {
        return false;
    }
    *keys = refreshed;
    true
}
fn mapping_current(event_time: u32, mapped_at: u32) -> bool {
    event_time.wrapping_sub(mapped_at) as i32 >= 0
}
fn allowed_keys(
    conn: &x11rb::rust_connection::RustConnection,
) -> Result<std::collections::HashMap<u32, &'static str>> {
    let setup = conn.setup();
    let min = setup.min_keycode;
    let mapping = conn
        .get_keyboard_mapping(min, setup.max_keycode - min + 1)
        .map_err(|_| ax::unavailable())?
        .reply()
        .map_err(|_| ax::unavailable())?;
    Ok(mapping
        .keysyms
        .chunks(usize::from(mapping.keysyms_per_keycode).max(1))
        .enumerate()
        .filter_map(|(i, row)| {
            row.first()
                .and_then(|s| key_name(*s))
                .map(|name| (u32::from(min) + i as u32, name))
        })
        .collect())
}
impl InputSource {
    pub(crate) fn from_producer(
        mut producer: impl FnMut() -> Result<(Vec<Event>, bool)> + Send + 'static,
    ) -> Result<Self> {
        struct ExitFlag(Arc<AtomicBool>);
        impl Drop for ExitFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let (send, events) = mpsc::sync_channel(128);
        let active = Arc::new(AtomicBool::new(true));
        let lost = Arc::new(AtomicBool::new(false));
        let topology_changed = Arc::new(AtomicBool::new(false));
        let running = active.clone();
        let flag = lost.clone();
        let changed = topology_changed.clone();
        let worker = std::thread::Builder::new()
            .name("history-native-input".into())
            .spawn(move || {
                let _exit = ExitFlag(changed.clone());
                while running.load(Ordering::SeqCst) {
                    match producer() {
                        Ok((events, dropped)) => {
                            if dropped {
                                flag.store(true, Ordering::SeqCst);
                            }
                            for event in events {
                                if !running.load(Ordering::SeqCst) {
                                    break;
                                }
                                if send.try_send(event).is_err() {
                                    flag.store(true, Ordering::SeqCst);
                                }
                            }
                        }
                        Err(_) => {
                            changed.store(true, Ordering::SeqCst);
                            break;
                        }
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                // Dropping the producer ends its compositor lease on this thread.
            })
            .map_err(|_| ax::unavailable())?;
        Ok(Self {
            events,
            lost,
            topology_changed,
            core_keyboard: false,
            active,
            worker: Some(worker),
        })
    }
    pub fn start(source: &mut LinuxSource, context: &Context) -> Result<Self> {
        if crate::linux::is_wayland() {
            return Err(Error::new(
                "computerHistoryInputUnsupported",
                "Wayland has no global input observation",
            ));
        }
        let (atspi, frame) = source.input_scope(context)?;
        let (conn, screen) = x11rb::connect(None).map_err(|_| ax::unavailable())?;
        let root = conn.setup().roots[screen].root;
        conn.xinput_xi_query_version(2, 0)
            .map_err(|_| ax::unavailable())?
            .reply()
            .map_err(|_| ax::unavailable())?;
        // Device events carry event-time root coordinates/modifiers. Raw XI2 events
        // have no screen position and cannot safely be paired with QueryPointer.
        let mut pending_windows = vec![context.window_id as u32];
        let mut selected = 0;
        let mut windows = Vec::new();
        let mut core_keyboard = false;
        while let Some(window) = pending_windows.pop() {
            selected += 1;
            if selected > 256 {
                return Err(ax::unavailable());
            }
            // Watch each admitted parent before enumerating it. A topology change
            // invalidates the complete subscription, including already queued input.
            conn.change_window_attributes(
                window,
                &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
                    .event_mask(x11rb::protocol::xproto::EventMask::SUBSTRUCTURE_NOTIFY),
            )
            .map_err(|_| ax::unavailable())?
            .check()
            .map_err(|_| ax::unavailable())?;
            let attributes = conn
                .get_window_attributes(window)
                .map_err(|_| ax::unavailable())?
                .reply()
                .map_err(|_| ax::unavailable())?;
            core_keyboard |= attributes
                .all_event_masks
                .contains(x11rb::protocol::xproto::EventMask::KEY_PRESS);
            windows.push(window);
            pending_windows.extend(
                conn.query_tree(window)
                    .map_err(|_| ax::unavailable())?
                    .reply()
                    .map_err(|_| ax::unavailable())?
                    .children,
            );
        }
        let core_enabled = core_keyboard && enable_core_keyboard(&conn).unwrap_or(false);
        for window in windows {
            if core_enabled {
                conn.change_window_attributes(
                    window,
                    &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
                        .event_mask(core_mask(true)),
                )
                .map_err(|_| ax::unavailable())?
                .check()
                .map_err(|_| ax::unavailable())?;
            }
            conn.xinput_xi_select_events(
                window,
                &[xinput::EventMask {
                    deviceid: xinput::Device::ALL_MASTER.into(),
                    mask: vec![keyboard_mask(core_keyboard)],
                }],
            )
            .map_err(|_| ax::unavailable())?
            .check()
            .map_err(|_| ax::unavailable())?;
        }
        // Mapping replies use a separate connection with no input subscription.
        let (mapping_conn, _) = x11rb::connect(None).map_err(|_| ax::unavailable())?;
        let mut keys = allowed_keys(&mapping_conn)?;
        // Anchor the wrapping X server clock without reading any application data.
        use x11rb::protocol::xproto::{
            AtomEnum, CreateWindowAux, EventMask, PropMode, WindowClass,
        };
        use x11rb::wrapper::ConnectionExt as _;
        let clock_window = conn.generate_id().map_err(|_| ax::unavailable())?;
        conn.create_window(
            0,
            clock_window,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )
        .map_err(|_| ax::unavailable())?;
        let clock_at = Instant::now();
        conn.change_property8(
            PropMode::REPLACE,
            clock_window,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            b"clock",
        )
        .map_err(|_| ax::unavailable())?;
        conn.flush().map_err(|_| ax::unavailable())?;
        let deadline = Instant::now() + Duration::from_millis(150);
        let server_time = loop {
            match conn.poll_for_event().map_err(|_| ax::unavailable())? {
                Some(XEvent::PropertyNotify(e)) if e.window == clock_window => break e.time,
                Some(ref event) if scope_changed(event) => return Err(scope_refusal()),
                _ if Instant::now() >= deadline => return Err(ax::unavailable()),
                _ => std::thread::sleep(Duration::from_millis(1)),
            }
        };
        let (send, events) = mpsc::sync_channel(128);
        let (pending_tx, pending_rx) = input_queue();
        let active = Arc::new(AtomicBool::new(true));
        let lost = Arc::new(AtomicBool::new(false));
        let topology_changed = Arc::new(AtomicBool::new(false));
        let changed = topology_changed.clone();
        let running = active.clone();
        let flag = lost.clone();
        let context = context.clone();
        // The subscribed connection performs no synchronous requests during intake.
        // Mapping refresh uses the separate connection; accessibility resolution is
        // on another thread. x11rb never queues input waiting on a reply. Both
        // handoff queues are bounded, and expired work is discarded before resolution.
        let reader_running = active.clone();
        let reader_lost = lost.clone();
        let reader = std::thread::Builder::new()
            .name("history-xinput-intake".into())
            .spawn(move || {
                let mut mapped_at = server_time;
                let mut core_down = std::collections::HashSet::new();
                while reader_running.load(Ordering::SeqCst) {
                    let event = match conn.poll_for_event() {
                        Ok(Some(e)) => e,
                        Ok(None) => {
                            std::thread::sleep(Duration::from_millis(2));
                            continue;
                        }
                        Err(_) => {
                            reader_lost.store(true, Ordering::SeqCst);
                            break;
                        }
                    };
                    if scope_changed(&event) {
                        changed.store(true, Ordering::SeqCst);
                        reader_lost.store(true, Ordering::SeqCst);
                        break;
                    }
                    if let XEvent::MappingNotify(ref e) = event {
                        if e.request == x11rb::protocol::xproto::Mapping::KEYBOARD
                            && refresh_keys(&mut keys, allowed_keys(&mapping_conn).unwrap_or_default()) {
                            reader_lost.store(true, Ordering::SeqCst);
                            core_down.clear();
                            mapped_at = server_time.wrapping_add(clock_at.elapsed().as_millis() as u32).wrapping_add(1);
                        }
                        continue;
                    }
                    // Keys queued during a mapping change have ambiguous meaning.
                    if matches!(&event, XEvent::XinputKeyPress(e) if !mapping_current(e.time, mapped_at)) { continue; }
                    if matches!(&event, XEvent::KeyPress(key) if !mapping_current(key.time, mapped_at)) { continue; }
                    if !core_key_once(&event, &keys, &mut core_down) { continue; }
                    let Some((input, point, time)) = input_metadata(event, &keys) else {
                        continue;
                    };
                    let now = server_time.wrapping_add(clock_at.elapsed().as_millis() as u32);
                    let Some(age) = event_age(now, time) else {
                        reader_lost.store(true, Ordering::SeqCst);
                        continue;
                    };
                    if pending_tx
                        .try_send(PendingInput {
                            input,
                            point,
                            received: Instant::now() - age,
                            timestamp_ms: crate::now_ms().saturating_sub(age.as_millis() as u64),
                        })
                        .is_err()
                    {
                        reader_lost.store(true, Ordering::SeqCst);
                    }
                }
            })
            .map_err(|_| ax::unavailable())?;
        let worker = std::thread::Builder::new()
            .name("history-xinput".into())
            .spawn(move || {
                while running.load(Ordering::SeqCst) {
                    let pending = match pending_rx.recv_timeout(Duration::from_millis(20)) {
                        Ok(pending) => pending,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    if !fresh(pending.received) {
                        flag.store(true, Ordering::SeqCst);
                        continue;
                    }
                    if crate::linux::x11_foreground()
                        != Some((context.process_id as u32, context.window_id))
                    {
                        continue;
                    }
                    let captured = ax::run(async {
                        tokio::time::timeout(
                            MAX_AGE.saturating_sub(pending.received.elapsed()),
                            async {
                                let (object, bounds) = if let Some((x, y)) = pending.point {
                                    let Some(bounds) = crate::linux::x11_bounds(context.window_id)
                                    else {
                                        return Ok(None);
                                    };
                                    let object = atspi
                                        .hit(&frame, context.process_id as u32, x, y, bounds)
                                        .await?;
                                    // A move/resize during accessibility resolution invalidates
                                    // the root-to-window coordinate translation.
                                    (object, Some(bounds))
                                } else {
                                    (
                                        atspi.focused(&frame, context.process_id as u32).await?,
                                        None,
                                    )
                                };
                                let Some(object) = object else {
                                    return Ok(None);
                                };
                                // Do not read values. The same secure/web/ancestry
                                // checks run before and after this label read.
                                let content = ax::read_checked(
                                    &atspi,
                                    &object,
                                    &frame,
                                    context.process_id as u32,
                                    false,
                                    false,
                                    false,
                                )
                                .await?;
                                if crate::linux::x11_foreground()
                                    != Some((context.process_id as u32, context.window_id))
                                    || bounds.is_some_and(|bounds| {
                                        crate::linux::x11_bounds(context.window_id) != Some(bounds)
                                    })
                                {
                                    return Ok(None);
                                }
                                Ok(content.map(|content| (object, content)))
                            },
                        )
                        .await
                        .map_err(|_| ax::unavailable())?
                    });
                    if let Ok(Some((object, content))) = captured {
                        if fresh(pending.received) && running.load(Ordering::SeqCst) {
                            if send
                                .try_send(Event {
                                    timestamp_ms: pending.timestamp_ms,
                                    object,
                                    input: pending.input,
                                    received: pending.received,
                                    frozen: Some(FrozenTarget {
                                        frame: frame.clone(),
                                        content,
                                    }),
                                })
                                .is_err()
                            {
                                flag.store(true, Ordering::SeqCst);
                            }
                        } else {
                            flag.store(true, Ordering::SeqCst);
                        }
                    } else {
                        flag.store(true, Ordering::SeqCst);
                    }
                }
                let _ = reader.join();
            })
            .map_err(|_| {
                active.store(false, Ordering::SeqCst);
                ax::unavailable()
            })?;
        Ok(Self {
            events,
            lost,
            topology_changed,
            core_keyboard: core_keyboard && !core_enabled,
            active,
            worker: Some(worker),
        })
    }
    pub fn keyboard_refusal(&self) -> Option<Error> {
        self.core_keyboard.then(|| Error::new(
            "computerHistoryKeyboardUnsupported",
            "Physical keyboard metadata is unavailable for this core X11 input client; native content and pointer observation remain active",
        ))
    }
    pub fn drain(&self) -> Result<(Vec<Event>, bool)> {
        if self.topology_changed.load(Ordering::SeqCst) {
            return Err(scope_refusal());
        }
        let lost = self.lost.swap(false, Ordering::SeqCst);
        let events: Vec<_> = self.events.try_iter().collect();
        if lost {
            return Ok((vec![], true));
        }
        let mut partial = false;
        let events = events
            .into_iter()
            .filter(|e| {
                let fresh = e.delivery_fresh();
                partial |= !fresh;
                fresh
            })
            .collect();
        if self.topology_changed.load(Ordering::SeqCst) {
            return Err(scope_refusal());
        }
        Ok((events, partial))
    }
}
impl Drop for InputSource {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
        if let Some(t) = self.worker.take() {
            let _ = t.join();
        }
    }
}
#[path = "linux_kwin_input.rs"]
mod kwin;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn background_input_failure_invalidates_queued_results() {
        let mut first = true;
        let source = InputSource::from_producer(move || {
            if first {
                first = false;
                Ok((vec![captured_click(Duration::ZERO)], false))
            } else {
                Err(ax::unavailable())
            }
        })
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while !source.topology_changed.load(Ordering::SeqCst) {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            source.drain().unwrap_err().code,
            "computerHistoryInputScopeChanged"
        );
    }
    #[test]
    fn stopping_background_input_releases_the_producer_before_returning() {
        struct Lease(Arc<AtomicBool>);
        impl Drop for Lease {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let stopped = Arc::new(AtomicBool::new(false));
        let lease = Lease(stopped.clone());
        let source = InputSource::from_producer(move || {
            let _ = &lease;
            Ok((Vec::new(), false))
        })
        .unwrap();
        assert!(!stopped.load(Ordering::SeqCst));
        drop(source);
        assert!(stopped.load(Ordering::SeqCst));
    }
    fn captured_click(age: Duration) -> Event {
        Event {
            timestamp_ms: 1234,
            object: ObjectKey {
                bus: ":1.2".into(),
                path: "/button".into(),
            },
            input: Input::Click { right: false },
            received: Instant::now() - age,
            frozen: Some(FrozenTarget {
                frame: ObjectKey {
                    bus: ":1.2".into(),
                    path: "/frame".into(),
                },
                content: Content {
                    role: "AXButton".into(),
                    label: Some("Export note".into()),
                    ..Default::default()
                },
            }),
        }
    }
    fn queued(event: Event) -> InputSource {
        let (send, events) = mpsc::sync_channel(2);
        send.send(event).unwrap();
        InputSource {
            events,
            lost: Arc::new(AtomicBool::new(false)),
            topology_changed: Arc::new(AtomicBool::new(false)),
            core_keyboard: false,
            active: Arc::new(AtomicBool::new(false)),
            worker: None,
        }
    }
    #[test]
    fn resolved_click_survives_a_slow_collector_without_rereading_its_label() {
        let source = queued(captured_click(Duration::from_millis(500)));
        let (events, lost) = source.drain().unwrap();
        assert!(!lost);
        assert_eq!(events.len(), 1);
        let content = events[0]
            .frozen_content(&ObjectKey {
                bus: ":1.2".into(),
                path: "/frame".into(),
            })
            .unwrap();
        assert_eq!(content.label.as_deref(), Some("Export note"));
        assert_eq!(content.value, None);
        assert_eq!(events[0].timestamp_ms, 1234);
    }
    #[test]
    fn resolved_click_cannot_cross_a_frame_or_provider_change() {
        let event = captured_click(Duration::from_millis(500));
        assert!(event
            .frozen_content(&ObjectKey {
                bus: ":1.3".into(),
                path: "/frame".into()
            })
            .is_none());
        assert!(event
            .frozen_content(&ObjectKey {
                bus: ":1.2".into(),
                path: "/other-frame".into()
            })
            .is_none());
    }
    #[test]
    fn resolved_click_still_expires_and_lost_batches_are_discarded() {
        let source = queued(captured_click(Duration::from_millis(2001)));
        let (events, lost) = source.drain().unwrap();
        assert!(lost && events.is_empty());
        let source = queued(captured_click(Duration::from_millis(500)));
        source.lost.store(true, Ordering::SeqCst);
        let (events, lost) = source.drain().unwrap();
        assert!(lost && events.is_empty());
        let source = queued(captured_click(Duration::from_millis(500)));
        source.topology_changed.store(true, Ordering::SeqCst);
        assert_eq!(
            source.drain().unwrap_err().code,
            "computerHistoryInputScopeChanged"
        );
    }
    #[test]
    fn core_keyboard_delivery_is_not_intercepted() {
        assert_eq!(keyboard_mask(true), xinput::XIEventMask::BUTTON_PRESS);
        assert_eq!(
            keyboard_mask(false),
            xinput::XIEventMask::BUTTON_PRESS | xinput::XIEventMask::KEY_PRESS
        );
        let (_, events) = mpsc::sync_channel(1);
        let source = InputSource {
            events,
            lost: Arc::new(AtomicBool::new(false)),
            topology_changed: Arc::new(AtomicBool::new(false)),
            core_keyboard: true,
            active: Arc::new(AtomicBool::new(false)),
            worker: None,
        };
        assert_eq!(
            source.keyboard_refusal().unwrap().code,
            "computerHistoryKeyboardUnsupported"
        );
    }
    #[test]
    fn core_keys_preserve_modifiers_and_refuse_repeats_text_and_send_event() {
        use x11rb::protocol::xproto::{EventMask, KeyPressEvent};
        assert_eq!(
            core_mask(true),
            EventMask::SUBSTRUCTURE_NOTIFY | EventMask::KEY_PRESS | EventMask::KEY_RELEASE
        );
        assert_eq!(core_mask(false), EventMask::SUBSTRUCTURE_NOTIFY);
        let keys = std::collections::HashMap::from([(36, "return")]);
        let mut down = std::collections::HashSet::new();
        let key = KeyPressEvent {
            response_type: 2,
            detail: 36,
            time: 1234,
            state: 13u16.into(),
            ..Default::default()
        };
        assert!(core_key_once(&XEvent::KeyPress(key), &keys, &mut down));
        let (input, point, time) = input_metadata(XEvent::KeyPress(key), &keys).unwrap();
        assert!(matches!(
            input,
            Input::Key {
                name: "return",
                ctrl: true,
                alt: true,
                shift: true,
                win: false
            }
        ));
        assert_eq!((point, time), (None, 1234));
        assert!(!core_key_once(&XEvent::KeyPress(key), &keys, &mut down));
        assert!(!core_key_once(&XEvent::KeyRelease(key), &keys, &mut down));
        assert!(core_key_once(&XEvent::KeyPress(key), &keys, &mut down));
        let text = KeyPressEvent { detail: 38, ..key };
        assert!(!core_key_once(&XEvent::KeyPress(text), &keys, &mut down));
        assert!(input_metadata(XEvent::KeyPress(text), &keys).is_none());
        down.clear();
        let injected = KeyPressEvent {
            response_type: 130,
            ..key
        };
        assert!(!core_key_once(
            &XEvent::KeyPress(injected),
            &keys,
            &mut down
        ));
        assert!(input_metadata(XEvent::KeyPress(injected), &keys).is_none());
    }
    #[test]
    #[ignore = "requires an isolated X11 display with XTEST"]
    fn core_subscription_preserves_receiver_delivery_and_window_scope() {
        use x11rb::protocol::xproto::{CreateWindowAux, EventMask, InputFocus, WindowClass};
        let (receiver, screen) = x11rb::connect(None).unwrap();
        let root = receiver.setup().roots[screen].root;
        let window = receiver.generate_id().unwrap();
        receiver
            .create_window(
                0,
                window,
                root,
                0,
                0,
                200,
                100,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &CreateWindowAux::new().event_mask(EventMask::KEY_PRESS),
            )
            .unwrap()
            .check()
            .unwrap();
        receiver.map_window(window).unwrap().check().unwrap();
        receiver
            .set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME)
            .unwrap()
            .check()
            .unwrap();
        let (observer, _) = x11rb::connect(None).unwrap();
        assert!(enable_core_keyboard(&observer).unwrap());
        observer
            .change_window_attributes(
                window,
                &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
                    .event_mask(core_mask(true)),
            )
            .unwrap()
            .check()
            .unwrap();
        let keys = allowed_keys(&observer).unwrap();
        let return_code = *keys.iter().find(|(_, name)| **name == "return").unwrap().0;
        let inject = |code: u32| {
            assert!(std::process::Command::new("python3").args(["-c", &format!(
                "import ctypes,time; x=ctypes.CDLL('libX11.so.6'); t=ctypes.CDLL('libXtst.so.6'); x.XOpenDisplay.restype=ctypes.c_void_p; d=x.XOpenDisplay(None); t.XTestFakeKeyEvent.argtypes=[ctypes.c_void_p,ctypes.c_uint,ctypes.c_int,ctypes.c_ulong]; x.XSync.argtypes=[ctypes.c_void_p,ctypes.c_int]; t.XTestFakeKeyEvent(d,{code},1,0); t.XTestFakeKeyEvent(d,{code},0,0); x.XSync(d,0)")]).status().unwrap().success());
        };
        inject(return_code);
        let server_keys: Vec<_> = std::iter::from_fn(|| receiver.poll_for_event().unwrap())
            .filter(|event| matches!(event, XEvent::KeyPress(_)))
            .collect();
        assert_eq!(server_keys.len(), 1);
        let mut down = std::collections::HashSet::new();
        let observed: Vec<_> = std::iter::from_fn(|| observer.poll_for_event().unwrap())
            .filter(|event| core_key_once(event, &keys, &mut down))
            .filter_map(|event| input_metadata(event, &keys))
            .collect();
        assert_eq!(observed.len(), 1);
        assert!(matches!(observed[0].0, Input::Key { name: "return", .. }));
        let foreign = receiver.generate_id().unwrap();
        receiver
            .create_window(
                0,
                foreign,
                root,
                220,
                0,
                200,
                100,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &CreateWindowAux::new().event_mask(EventMask::KEY_PRESS),
            )
            .unwrap()
            .check()
            .unwrap();
        receiver.map_window(foreign).unwrap().check().unwrap();
        receiver
            .set_input_focus(InputFocus::PARENT, foreign, x11rb::CURRENT_TIME)
            .unwrap()
            .check()
            .unwrap();
        inject(return_code);
        assert!(std::iter::from_fn(|| observer.poll_for_event().unwrap())
            .all(|event| input_metadata(event, &keys).is_none()));
    }
    fn button(detail: u32) -> xinput::ButtonPressEvent {
        xinput::ButtonPressEvent {
            response_type: 35,
            extension: 0,
            sequence: 0,
            length: 0,
            event_type: 4,
            deviceid: 2,
            time: 1234,
            detail,
            root: 1,
            event: 2,
            child: 0,
            root_x: 321 << 16,
            root_y: 456 << 16,
            event_x: 0,
            event_y: 0,
            sourceid: 2,
            flags: 0u32.into(),
            mods: Default::default(),
            group: Default::default(),
            button_mask: vec![],
            valuator_mask: vec![],
            axisvalues: vec![],
        }
    }
    #[test]
    fn click_uses_device_event_coordinates_and_timestamp() {
        let keys = Default::default();
        let (_, point, time) = input_metadata(XEvent::XinputButtonPress(button(1)), &keys).unwrap();
        assert_eq!(point, Some((321, 456)));
        assert_eq!(time, 1234);
        assert!(input_metadata(XEvent::XinputButtonPress(button(4)), &keys).is_none());
        let mut synthetic = button(1);
        synthetic.response_type |= 128;
        assert!(input_metadata(XEvent::XinputButtonPress(synthetic), &keys).is_none());
    }
    #[test]
    fn key_filter_precedes_identity_and_drops_repeats() {
        let b = button(36);
        let mut key = xinput::KeyPressEvent {
            response_type: b.response_type,
            extension: 0,
            sequence: 0,
            length: 0,
            event_type: 2,
            deviceid: 2,
            time: 1234,
            detail: 36,
            root: 1,
            event: 2,
            child: 0,
            root_x: 0,
            root_y: 0,
            event_x: 0,
            event_y: 0,
            sourceid: 2,
            flags: 0u32.into(),
            mods: Default::default(),
            group: Default::default(),
            button_mask: vec![],
            valuator_mask: vec![],
            axisvalues: vec![],
        };
        let keys = std::collections::HashMap::from([(36, "return")]);
        assert!(input_metadata(XEvent::XinputKeyPress(key.clone()), &keys).is_some());
        key.response_type |= 128;
        assert!(input_metadata(XEvent::XinputKeyPress(key.clone()), &keys).is_none());
        key.response_type &= 127;
        key.flags = xinput::KeyEventFlags::KEY_REPEAT;
        assert!(input_metadata(XEvent::XinputKeyPress(key.clone()), &keys).is_none());
        key.flags = 0u32.into();
        key.detail = 38;
        assert!(input_metadata(XEvent::XinputKeyPress(key), &keys).is_none());
    }
    #[test]
    fn unchanged_mapping_preserves_pending_input() {
        let mut keys = std::collections::HashMap::from([(36, "return")]);
        assert!(!refresh_keys(
            &mut keys,
            std::collections::HashMap::from([(36, "return")])
        ));
        assert!(refresh_keys(
            &mut keys,
            std::collections::HashMap::from([(40, "return")])
        ));
        assert!(!keys.contains_key(&36));
        assert_eq!(keys.get(&40), Some(&"return"));
    }
    #[test]
    fn mapping_changes_refuse_queued_keys() {
        assert!(!mapping_current(999, 1000));
        assert!(mapping_current(1001, 1000));
        assert!(mapping_current(10, u32::MAX - 9));
        assert!(!mapping_current(u32::MAX - 9, 10));
    }
    #[test]
    fn stalled_resolution_has_a_bounded_nonblocking_backlog() {
        let (send, receive) = input_queue();
        let mut accepted = 0;
        for _ in 0..10000 {
            if send
                .try_send(PendingInput {
                    input: Input::Click { right: false },
                    point: Some((1, 2)),
                    received: Instant::now(),
                    timestamp_ms: 0,
                })
                .is_ok()
            {
                accepted += 1;
            }
        }
        assert_eq!(accepted, 32);
        assert_eq!(receive.try_iter().count(), 32);
    }
    #[test]
    fn old_input_is_discarded_including_server_queue_delay() {
        assert_eq!(event_age(1000, 950), Some(Duration::from_millis(50)));
        assert!(event_age(1000, 899).is_none());
        assert!(event_age(1000, 1001).is_none());
        assert_eq!(event_age(10, u32::MAX - 9), Some(Duration::from_millis(20)));
        assert!(!fresh(Instant::now() - Duration::from_millis(101)));
        let (send, events) = mpsc::sync_channel(2);
        send.send(Event {
            timestamp_ms: 0,
            received: Instant::now() - Duration::from_millis(101),
            object: ObjectKey {
                bus: ":1.2".into(),
                path: "/entry".into(),
            },
            input: Input::Click { right: false },
            frozen: None,
        })
        .unwrap();
        let source = InputSource {
            events,
            lost: Arc::new(AtomicBool::new(false)),
            topology_changed: Arc::new(AtomicBool::new(false)),
            core_keyboard: false,
            active: Arc::new(AtomicBool::new(false)),
            worker: None,
        };
        let (events, lost) = source.drain().unwrap();
        assert!(lost);
        assert!(events.is_empty());
    }
    #[test]
    fn new_child_tree_explicitly_refuses_pending_input() {
        let create = x11rb::protocol::xproto::CreateNotifyEvent {
            response_type: 16,
            sequence: 0,
            parent: 1,
            window: 2,
            x: 0,
            y: 0,
            width: 10,
            height: 10,
            border_width: 0,
            override_redirect: false,
        };
        assert!(scope_changed(&XEvent::CreateNotify(create)));
        assert!(scope_changed(&XEvent::DestroyNotify(
            x11rb::protocol::xproto::DestroyNotifyEvent {
                response_type: 17,
                sequence: 0,
                event: 1,
                window: 2
            }
        )));
        assert!(scope_changed(&XEvent::ReparentNotify(
            x11rb::protocol::xproto::ReparentNotifyEvent {
                response_type: 21,
                sequence: 0,
                event: 1,
                window: 2,
                parent: 3,
                x: 0,
                y: 0,
                override_redirect: false
            }
        )));
        assert!(!scope_changed(&XEvent::XinputButtonPress(button(1))));
        let (send, events) = mpsc::sync_channel(2);
        send.send(Event {
            timestamp_ms: 0,
            received: Instant::now(),
            object: ObjectKey {
                bus: ":1.2".into(),
                path: "/entry".into(),
            },
            input: Input::Click { right: false },
            frozen: None,
        })
        .unwrap();
        let source = InputSource {
            events,
            lost: Arc::new(AtomicBool::new(false)),
            topology_changed: Arc::new(AtomicBool::new(true)),
            core_keyboard: false,
            active: Arc::new(AtomicBool::new(false)),
            worker: None,
        };
        assert_eq!(
            source.drain().unwrap_err().code,
            "computerHistoryInputScopeChanged"
        );
        assert!(source.drain().is_err()); // sticky until a new subscription
    }
    #[test]
    fn printable_keys_never_enter_queue() {
        for k in 0x20..=0x7e {
            assert_eq!(key_name(k), None)
        }
        assert_eq!(key_name(0xff0d), Some("return"));
        assert_eq!(key_name(0xff09), Some("tab"));
        assert_eq!(key_name(0xff1b), Some("escape"));
    }
}
