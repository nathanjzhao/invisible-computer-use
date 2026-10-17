//! Caller-bound, transient GNOME pointer events consumed by the History owner.
use crate::{
    linux::LinuxSource,
    linux_atspi as ax,
    linux_input::{Event, FrozenTarget, Input, InputSource},
    policy::Context,
    Error, Result,
};
use serde::Deserialize;
use std::time::{Duration, Instant};

const PATH: &str = "/org/cua/WinRects";
const INTERFACE: &str = "org.cua.WinRects";
const MAX_AGE: Duration = Duration::from_millis(100);
const MAX_SEQUENCE: u64 = (1u64 << 53) - 1;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Rect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}
impl Rect {
    fn valid(self) -> bool {
        (1..=32768).contains(&self.width)
            && (1..=32768).contains(&self.height)
            && self.x.checked_add(self.width).is_some()
            && self.y.checked_add(self.height).is_some()
    }
    fn contains(self, x: f64, y: f64) -> bool {
        self.valid()
            && x.is_finite()
            && y.is_finite()
            && x >= f64::from(self.x)
            && y >= f64::from(self.y)
            && x < f64::from(self.x) + f64::from(self.width)
            && y < f64::from(self.y) + f64::from(self.height)
    }
    fn tuple(self) -> (i32, i32, i32, i32) {
        (self.x, self.y, self.width, self.height)
    }
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireEvent {
    sequence: u64,
    time: u32,
    timestamp_ms: u64,
    observed_monotonic_us: u64,
    button: u32,
    x: f64,
    y: f64,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    ctrl: bool,
    #[serde(default)]
    alt: bool,
    #[serde(default)]
    shift: bool,
    #[serde(default)]
    win: bool,
    frame: Rect,
    buffer: Rect,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    token: String,
    #[serde(default)]
    sequence: Option<u64>,
    id: u64,
    pid: u32,
    frame: Rect,
    buffer: Rect,
    monotonic_us: u64,
    #[serde(default)]
    events: Vec<WireEvent>,
    #[serde(default)]
    lost: bool,
}
fn unavailable() -> Error {
    Error::new(
        "computerHistoryInputScopeChanged",
        "Native pointer capture needs to reconnect to its current window",
    )
}
fn monotonic_us() -> Result<u64> {
    let mut now: libc::timespec = unsafe { std::mem::zeroed() };
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) } != 0
        || now.tv_sec < 0
        || now.tv_nsec < 0
    {
        return Err(unavailable());
    }
    Ok((now.tv_sec as u64).saturating_mul(1_000_000) + now.tv_nsec as u64 / 1000)
}
fn valid_reply(reply: &Reply, token: &str, id: u64, pid: u32, now: u64) -> bool {
    reply.token == token
        && reply.id == id
        && reply.pid == pid
        && reply.frame.valid()
        && reply.buffer.valid()
        && now
            .checked_sub(reply.monotonic_us)
            .is_some_and(|age| age <= 200_000)
        && reply.events.len() <= 64
        && reply.sequence.is_none_or(|value| value <= MAX_SEQUENCE)
}
fn event_age(event: &WireEvent, state: &Reply, previous: u64, now: u64) -> Option<Duration> {
    if event.sequence != previous.checked_add(1)?
        || event.sequence > state.sequence?
        || input_metadata(event).is_none()
        || event.timestamp_ms > MAX_SEQUENCE
        || event.frame != state.frame
        || event.buffer != state.buffer
        || (event.key.is_none()
            && (!event.frame.contains(event.x, event.y)
                || !event.buffer.contains(event.x, event.y)))
    {
        return None;
    }
    let age = Duration::from_micros(now.checked_sub(event.observed_monotonic_us)?);
    (age <= MAX_AGE).then_some(age)
}

fn input_metadata(event: &WireEvent) -> Option<Input> {
    if let Some(key) = event.key.as_deref() {
        let name = match key {
            "return" => "return",
            "tab" => "tab",
            "escape" => "escape",
            _ => return None,
        };
        if event.button != 0 || event.x != 0.0 || event.y != 0.0 {
            return None;
        }
        Some(Input::Key {
            name,
            ctrl: event.ctrl,
            alt: event.alt,
            shift: event.shift,
            win: event.win,
        })
    } else {
        (matches!(event.button, 1 | 3) && !event.ctrl && !event.alt && !event.shift && !event.win)
            .then_some(Input::Click {
                right: event.button == 3,
            })
    }
}

pub struct GnomePointerSource {
    input: InputSource,
    keyboard: bool,
}
impl GnomePointerSource {
    pub fn start(source: &mut LinuxSource, context: &Context) -> Result<Option<Self>> {
        let Some(mut reader) = GnomePointerReader::start(source, context)? else {
            return Ok(None);
        };
        Ok(Some(Self {
            keyboard: reader.version >= 2,
            input: InputSource::from_producer(move || reader.drain())?,
        }))
    }
    pub fn drain(&mut self) -> Result<(Vec<Event>, bool)> {
        self.input.drain()
    }
    pub fn keyboard_refusal(&self) -> Option<Error> {
        (!self.keyboard).then(|| {
            Error::new(
                "computerHistoryKeyboardUnsupported",
                "GNOME pointer capture does not include physical keyboard events",
            )
        })
    }
}
struct GnomePointerReader {
    version: u32,
    connection: zbus::Connection,
    owner: String,
    id: u64,
    pid: u32,
    token: String,
    atspi: ax::Atspi,
    frame: ax::ObjectKey,
    sequence: u64,
}
impl GnomePointerReader {
    pub fn start(source: &mut LinuxSource, context: &Context) -> Result<Option<Self>> {
        let Some((connection, owner, id, atspi, frame)) = source.gnome_pointer_scope(context)?
        else {
            return Ok(None);
        };
        let pid = u32::try_from(context.process_id).map_err(|_| unavailable())?;
        let (version, raw): (u32, String) = ax::run(async {
            let proxy = ax::call(zbus::Proxy::new(
                &connection,
                owner.as_str(),
                PATH,
                INTERFACE,
            ))
            .await?;
            let version: u32 = ax::call(proxy.call("GetHistoryPointerVersion", &())).await?;
            if !matches!(version, 1 | 2) {
                return Err(unavailable());
            }
            let raw = ax::call(proxy.call("StartHistoryPointer", &(id, pid))).await?;
            Ok((version, raw))
        })?;
        let reply: Reply = serde_json::from_str(&raw).map_err(|_| unavailable())?;
        if reply.token.len() != 36
            || !reply
                .token
                .bytes()
                .all(|b| b.is_ascii_hexdigit() || b == b'-')
            || reply.sequence != Some(0)
            || !reply.events.is_empty()
            || reply.lost
            || !valid_reply(&reply, &reply.token, id, pid, monotonic_us()?)
        {
            return Err(unavailable());
        }
        Ok(Some(Self {
            version,
            connection,
            owner,
            id,
            pid,
            token: reply.token,
            atspi,
            frame,
            sequence: 0,
        }))
    }
    fn rpc(&self, method: &str) -> Result<String> {
        ax::run(async {
            let proxy = ax::call(zbus::Proxy::new(
                &self.connection,
                self.owner.as_str(),
                PATH,
                INTERFACE,
            ))
            .await?;
            ax::call(proxy.call(method, &(self.token.as_str(),))).await
        })
    }
    pub fn drain(&mut self) -> Result<(Vec<Event>, bool)> {
        let raw = self.rpc("ReadHistoryPointer")?;
        let reply: Reply = serde_json::from_str(&raw).map_err(|_| unavailable())?;
        let now = monotonic_us()?;
        if !valid_reply(&reply, &self.token, self.id, self.pid, now) {
            return Err(unavailable());
        }
        let sequence = reply.sequence.ok_or_else(unavailable)?;
        if sequence < self.sequence {
            return Err(unavailable());
        }
        if reply.lost {
            self.sequence = sequence;
            return Ok((Vec::new(), true));
        }
        let mut events = Vec::new();
        let mut lost = false;
        let mut previous = self.sequence;
        for row in &reply.events {
            if self.version == 1 && row.key.is_some() {
                lost = true;
                continue;
            }
            let Some(_age) = event_age(row, &reply, previous, monotonic_us()?) else {
                lost = true;
                continue;
            };
            previous = row.sequence;
            // Only exact frame/buffer geometry translations compete. Multiple
            // different accessible targets are refused, never selected by name.
            let mut hits = Vec::new();
            let mut rectangles = vec![row.buffer];
            if row.frame != row.buffer {
                rectangles.push(row.frame);
            }
            if row.key.is_some() {
                match ax::run(self.atspi.focused(&self.frame, self.pid)) {
                    Ok(Some(object)) => hits.push(object),
                    _ => {
                        lost = true;
                    }
                }
            }
            for rect in rectangles.into_iter().filter(|_| row.key.is_none()) {
                let result = ax::run(async {
                    ax::call(self.atspi.hit_wayland(
                        &self.frame,
                        self.pid,
                        row.x.floor() as i32,
                        row.y.floor() as i32,
                        rect.tuple(),
                    ))
                    .await
                });
                match result {
                    Ok(Some(key)) if !hits.contains(&key) => hits.push(key),
                    Ok(_) => (),
                    Err(_) => {
                        lost = true;
                        hits.clear();
                        break;
                    }
                }
            }
            if hits.len() != 1
                || monotonic_us()?.saturating_sub(row.observed_monotonic_us)
                    > MAX_AGE.as_micros() as u64
            {
                lost = true;
                continue;
            }
            let object = hits.remove(0);
            let content = ax::run(async {
                let age = Duration::from_micros(
                    monotonic_us()?.saturating_sub(row.observed_monotonic_us),
                );
                tokio::time::timeout(
                    MAX_AGE.saturating_sub(age),
                    ax::read_checked(
                        &self.atspi,
                        &object,
                        &self.frame,
                        self.pid,
                        false,
                        row.key.is_some(),
                        false,
                    ),
                )
                .await
                .map_err(|_| unavailable())?
            });
            let Ok(Some(content)) = content else {
                lost = true;
                continue;
            };
            let state: Reply = serde_json::from_str(&self.rpc("CheckHistoryPointer")?)
                .map_err(|_| unavailable())?;
            if !valid_reply(&state, &self.token, self.id, self.pid, monotonic_us()?)
                || state.frame != row.frame
                || state.buffer != row.buffer
                || state.sequence.is_some()
                || !state.events.is_empty()
                || state.lost
            {
                return Err(unavailable());
            }
            let age = Duration::from_micros(
                monotonic_us()?
                    .checked_sub(row.observed_monotonic_us)
                    .ok_or_else(unavailable)?,
            );
            if age > MAX_AGE {
                lost = true;
                continue;
            }
            if row.key.is_some()
                && !ax::run(self.atspi.verify_focus(&object, &self.frame, self.pid))
                    .unwrap_or(false)
            {
                lost = true;
                continue;
            }
            // Native event time is validated by the compositor at capture.
            // Wall time comes from that callback, not this later polling step.
            let _native_time = row.time;
            events.push(Event {
                timestamp_ms: row.timestamp_ms,
                object,
                input: input_metadata(row).ok_or_else(unavailable)?,
                received: Instant::now() - age,
                frozen: Some(FrozenTarget {
                    frame: self.frame.clone(),
                    content,
                }),
            });
        }
        if previous != sequence {
            lost = true;
        }
        self.sequence = sequence;
        if lost {
            events.clear();
        }
        Ok((events, lost))
    }
}
impl Drop for GnomePointerReader {
    fn drop(&mut self) {
        let _ = self.rpc("StopHistoryPointer");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> (Reply, WireEvent) {
        let rect = Rect {
            x: 100,
            y: 200,
            width: 640,
            height: 480,
        };
        (
            Reply {
                token: "scope".into(),
                sequence: Some(1),
                id: 7,
                pid: 42,
                frame: rect,
                buffer: rect,
                monotonic_us: 1_050_000,
                events: vec![],
                lost: false,
            },
            WireEvent {
                sequence: 1,
                time: 1000,
                timestamp_ms: 1_700_000_000_000,
                observed_monotonic_us: 1_000_000,
                button: 1,
                x: 300.5,
                y: 400.5,
                key: None,
                ctrl: false,
                alt: false,
                shift: false,
                win: false,
                frame: rect,
                buffer: rect,
            },
        )
    }
    #[test]
    fn changed_peer_scope_and_future_clock_are_not_accepted() {
        let (reply, _) = sample();
        assert!(valid_reply(&reply, "scope", 7, 42, 1_060_000));
        assert!(!valid_reply(&reply, "other", 7, 42, 1_060_000));
        assert!(!valid_reply(&reply, "scope", 8, 42, 1_060_000));
        assert!(!valid_reply(&reply, "scope", 7, 43, 1_060_000));
        assert!(!valid_reply(&reply, "scope", 7, 42, 1_000_000));
    }
    #[test]
    fn stale_replayed_moved_and_outside_pointer_events_are_refused() {
        let (reply, mut event) = sample();
        assert_eq!(
            event_age(&event, &reply, 0, 1_050_000),
            Some(Duration::from_millis(50))
        );
        assert!(event_age(&event, &reply, 1, 1_050_000).is_none());
        assert!(event_age(&event, &reply, 0, 1_200_000).is_none());
        event.x = 99.9;
        assert!(event_age(&event, &reply, 0, 1_050_000).is_none());
        event.x = 300.5;
        event.buffer.x += 1;
        assert!(event_age(&event, &reply, 0, 1_050_000).is_none());
        event.buffer = reply.buffer;
        event.button = 8;
        assert!(event_age(&event, &reply, 0, 1_050_000).is_none());
    }
    #[test]
    fn keys_are_non_text_metadata_and_keep_scope_and_age_checks() {
        let (reply, mut event) = sample();
        event.button = 0;
        event.x = 0.0;
        event.y = 0.0;
        event.key = Some("return".into());
        event.ctrl = true;
        assert!(matches!(
            input_metadata(&event),
            Some(Input::Key {
                name: "return",
                ctrl: true,
                ..
            })
        ));
        assert!(event_age(&event, &reply, 0, 1_050_000).is_some());
        assert!(event_age(&event, &reply, 0, 1_200_000).is_none());
        assert!(event_age(&event, &reply, 1, 1_050_000).is_none());
        event.key = Some("a".into());
        assert!(input_metadata(&event).is_none());
        event.key = Some("tab".into());
        event.button = 1;
        assert!(input_metadata(&event).is_none());
        event.button = 0;
        event.x = f64::NAN;
        assert!(input_metadata(&event).is_none());
        event.x = 0.0;
        event.frame.x += 1;
        assert!(event_age(&event, &reply, 0, 1_050_000).is_none());
    }
}
