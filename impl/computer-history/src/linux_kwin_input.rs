//! The verified KWin helper observes only leased, focused-window metadata.
//! No printable keys are transported. Content uses the normal AT-SPI policy.
use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    valid: bool,
    lost: bool,
    events: Vec<Metadata>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    kind: String,
    at: u64,
    bounds: (i32, i32, i32, i32),
    x: Option<f64>,
    y: Option<f64>,
    #[serde(default)]
    right: bool,
    key: Option<String>,
    #[serde(default)]
    ctrl: bool,
    #[serde(default)]
    alt: bool,
    #[serde(default)]
    shift: bool,
    #[serde(default)]
    win: bool,
}
fn monotonic_us() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) } != 0 {
        return 0;
    }
    ts.tv_sec as u64 * 1_000_000 + ts.tv_nsec as u64 / 1000
}
fn decode(item: &Metadata, now: u64) -> Option<(Input, Option<(i32, i32)>, Duration)> {
    let age = Duration::from_micros(now.checked_sub(item.at)?);
    if age > MAX_AGE
        || !(1..=32768).contains(&item.bounds.2)
        || !(1..=32768).contains(&item.bounds.3)
        || item.bounds.0.checked_add(item.bounds.2).is_none()
        || item.bounds.1.checked_add(item.bounds.3).is_none()
    {
        return None;
    }
    let (input, point) = match item.kind.as_str() {
        "click" => {
            let (x, y) = (item.x?, item.y?);
            let (bx, by, w, h) = item.bounds;
            if !x.is_finite()
                || !y.is_finite()
                || x < f64::from(bx)
                || y < f64::from(by)
                || x >= f64::from(bx) + f64::from(w)
                || y >= f64::from(by) + f64::from(h)
            {
                return None;
            }
            (
                Input::Click { right: item.right },
                Some((x.floor() as i32, y.floor() as i32)),
            )
        }
        "key" => {
            let name = match item.key.as_deref()? {
                "return" => "return",
                "tab" => "tab",
                "escape" => "escape",
                _ => return None,
            };
            (
                Input::Key {
                    name,
                    ctrl: item.ctrl,
                    alt: item.alt,
                    shift: item.shift,
                    win: item.win,
                },
                None,
            )
        }
        _ => return None,
    };
    Some((input, point, age))
}
impl InputSource {
    pub fn start_kwin(source: &mut LinuxSource, context: &Context) -> Result<Self> {
        let (atspi, frame) = source.input_scope(context)?;
        let (connection, owner, id) = source.kwin_input_helper(context)?;
        let proxy = ax::run(async {
            let proxy = ax::call(zbus::Proxy::new_owned(
                connection,
                owner,
                "/org/cua/KWinTarget".to_owned(),
                "org.cua.KWinTarget".to_owned(),
            ))
            .await?;
            let version: u32 = ax::call(proxy.call("GetHistoryInputVersion", &())).await.map_err(|_| Error::new(
                "computerHistoryInputUnsupported", "The verified compositor helper does not offer focused physical History input; native content observation remains available"))?;
            if version != 1 {
                return Err(ax::unavailable());
            }
            let accepted: bool =
                ax::call(proxy.call("BeginHistoryInput", &(id, context.process_id as u32))).await?;
            if !accepted {
                return Err(scope_refusal());
            }
            Ok(proxy)
        })?;
        let (send, events) = mpsc::sync_channel(128);
        let active = Arc::new(AtomicBool::new(true));
        let lost = Arc::new(AtomicBool::new(false));
        let topology_changed = Arc::new(AtomicBool::new(false));
        let running = active.clone();
        let flag = lost.clone();
        let changed = topology_changed.clone();
        let pid = context.process_id as u32;
        let worker = std::thread::Builder::new()
            .name("history-wayland-input".into())
            .spawn(move || {
                while running.load(Ordering::SeqCst) {
                    let result = ax::run(async {
                        let raw: String = ax::call(proxy.call("DrainHistoryInput", &())).await?;
                        if raw.len() > 16384 {
                            return Err(ax::unavailable());
                        }
                        let batch: Batch =
                            serde_json::from_str(&raw).map_err(|_| ax::unavailable())?;
                        if !batch.valid || batch.events.len() > 32 {
                            return Err(scope_refusal());
                        }
                        if batch.lost {
                            flag.store(true, Ordering::SeqCst);
                        }
                        for item in batch.events {
                            let Some((input, point, age)) = decode(&item, monotonic_us()) else {
                                flag.store(true, Ordering::SeqCst);
                                continue;
                            };
                            let received = Instant::now() - age;
                            let captured =
                                tokio::time::timeout(MAX_AGE.saturating_sub(age), async {
                                    let object = if let Some((x, y)) = point {
                                        atspi.hit(&frame, pid, x, y, item.bounds).await
                                    } else {
                                        atspi.focused(&frame, pid).await
                                    }?;
                                    let Some(object) = object else {
                                        return Ok(None);
                                    };
                                    let content = ax::read_checked(
                                        &atspi, &object, &frame, pid, false, false, false,
                                    )
                                    .await?;
                                    Ok(content.map(|content| (object, content)))
                                })
                                .await
                                .map_err(|_| ax::unavailable())??;
                            let valid: bool =
                                ax::call(proxy.call("CheckHistoryInput", &())).await?;
                            if !valid {
                                return Err(scope_refusal());
                            }
                            if let Some((object, content)) = captured {
                                if fresh(received) && running.load(Ordering::SeqCst) {
                                    if send
                                        .try_send(Event {
                                            timestamp_ms: crate::now_ms().saturating_sub(
                                                received.elapsed().as_millis() as u64,
                                            ),
                                            object,
                                            input,
                                            received,
                                            frozen: Some(FrozenTarget {
                                                frame: frame.clone(),
                                                content,
                                            }),
                                        })
                                        .is_err()
                                    {
                                        flag.store(true, Ordering::SeqCst);
                                    }
                                }
                            }
                        }
                        Ok(())
                    });
                    if result.is_err() {
                        changed.store(true, Ordering::SeqCst);
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                // Drop, focus loss, lock or caller disconnect clears the compositor
                // subscription. The helper also expires an abandoned lease itself.
                let _ = ax::run(async {
                    ax::call(proxy.call::<_, _, ()>("EndHistoryInput", &())).await
                });
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
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wayland_metadata_rejects_text_stale_time_and_foreign_coordinates() {
        let mut item: Metadata =
            serde_json::from_value(serde_json::json!({"kind":"key", "at":1000000,
            "bounds":[10,20,100,100], "key":"return"}))
            .unwrap();
        assert!(decode(&item, 1000100).is_some());
        assert!(decode(&item, 999999).is_none());
        assert!(decode(&item, 1100001).is_none());
        item.key = Some("a".into());
        assert!(decode(&item, 1000100).is_none());
        item.kind = "click".into();
        item.x = Some(25.0);
        item.y = Some(35.0);
        assert!(decode(&item, 1000100).is_some());
        item.x = Some(110.0);
        assert!(decode(&item, 1000100).is_none());
        item.x = Some(f64::NAN);
        assert!(decode(&item, 1000100).is_none());
    }
    #[test]
    fn negative_coordinates_keep_the_native_pixel_and_overflow_refuses() {
        let mut item: Metadata = serde_json::from_value(serde_json::json!({
            "kind":"click", "at":1000000, "bounds":[-10,-20,100,100], "x":-9.5,"y":-19.5
        }))
        .unwrap();
        assert_eq!(decode(&item, 1000100).unwrap().1, Some((-10, -20)));
        item.bounds = (i32::MAX, 0, 100, 100);
        assert!(decode(&item, 1000100).is_none());
        item.bounds = (0, 0, 32769, 100);
        assert!(decode(&item, 1000100).is_none());
    }
}
