//! Source-bound value notifications for the existing Windows collector.
//! Retain only bounded, policy-approved field snapshots. Intermediate typing
//! stays private until a quiet interval or a subsequent native action boundary.
use super::{bounded, unavailable, web_class, Uia};
use crate::{
    capture::Content,
    policy::{Context, Website},
    store::{CaptureMode, Store},
    value_buffer::{ValueBuffer, ValueChange},
    Result,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    time::{Duration, Instant},
};
use windows::{
    core::{implement, Ref},
    Win32::{
        System::{
            Com::SAFEARRAY,
            Ole::*,
            Variant::{VARIANT, VT_BSTR, VT_EMPTY, VT_I4},
        },
        UI::Accessibility::*,
    },
};

struct Counts {
    #[cfg(debug_assertions)]
    values: [std::sync::atomic::AtomicUsize; 42],
}
impl Default for Counts {
    fn default() -> Self {
        Self {
            #[cfg(debug_assertions)]
            values: std::array::from_fn(|_| std::sync::atomic::AtomicUsize::new(0)),
        }
    }
}
impl Counts {
    fn hit(&self, index: usize) {
        #[cfg(debug_assertions)]
        {
            self.values[index].fetch_add(1, Ordering::Relaxed);
        }
        #[cfg(not(debug_assertions))]
        let _ = index;
    }
}

#[implement(IUIAutomationPropertyChangedEventHandler)]
struct Handler {
    store: Store,
    context: Context,
    generation: String,
    revision: u64,
    active: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
    send: SyncSender<(Vec<i32>, ValueChange)>,
    counts: Arc<Counts>,
}
impl IUIAutomationPropertyChangedEventHandler_Impl for Handler_Impl {
    fn HandlePropertyChangedEvent(
        &self,
        sender: Ref<'_, IUIAutomationElement>,
        property: UIA_PROPERTY_ID,
        new_value: &VARIANT,
    ) -> windows::core::Result<()> {
        self.counts.hit(0);
        if property != UIA_ValueValuePropertyId
            || !self.active.load(Ordering::SeqCst)
            || crate::windows::foreground()
                != Some((self.context.process_id as u32, self.context.window_id))
        {
            return Ok(());
        }
        let Some(sender) = sender.as_ref() else {
            return Ok(());
        };
        self.counts.hit(1);
        let received = Instant::now();
        let capture = || -> Result<Option<(Vec<i32>, ValueChange)>> {
            let Some(observation) = self.store.observation()? else {
                return Ok(None);
            };
            if observation.mode != CaptureMode::InteractionEvents
                || !observation.matches_revision(&self.generation, self.revision)
            {
                return Ok(None);
            }
            let Some(mut ticket) = self.store.bind(observation, self.context.clone())? else {
                return Ok(None);
            };
            self.counts.hit(2);
            // A pause or rule acknowledgement waits for this native read. Keep
            // the same ticket for the later collector commit, which rechecks it.
            let sampled = unsafe { new_value.Anonymous.Anonymous.vt == VT_EMPTY };
            let read = || {
                if !self.active.load(Ordering::SeqCst) {
                    return Ok(None);
                }
                let uia = Uia::new()?;
                property_content(&uia, sender, &self.context, new_value, &self.counts)
                    .map(|content| content.map(|value| (value, Instant::now())))
            };
            let content = if sampled {
                self.store
                    .sample_with_ticket(&mut ticket, &self.context, read)?
            } else {
                self.store.read_with_ticket(&ticket, &self.context, read)?
            }
            .flatten();
            let Some(((identity, content), sample_order)) = content else {
                return Ok(None);
            };
            let source_order = if sampled { sample_order } else { received };
            if !self.active.load(Ordering::SeqCst)
                || received.elapsed() > Duration::from_secs(2)
                || crate::windows::foreground()
                    != Some((self.context.process_id as u32, self.context.window_id))
            {
                return Ok(None);
            }
            Ok(Some((
                identity,
                ValueChange::new(ticket, content, received, source_order),
            )))
        };
        match capture() {
            Ok(Some(event)) => {
                if self.send.try_send(event).is_err() {
                    self.lost.store(true, Ordering::SeqCst);
                } else {
                    self.counts.hit(8);
                }
            }
            Ok(None) => {}
            Err(_) => {
                self.counts.hit(9);
                self.lost.store(true, Ordering::SeqCst);
            }
        }
        Ok(())
    }
}

// The runtime identity is metadata, not a reusable action token. Its bounded
// array is owned by this call and released on every success/failure path.
struct Array(*mut SAFEARRAY);
impl Drop for Array {
    fn drop(&mut self) {
        unsafe {
            let _ = SafeArrayDestroy(self.0);
        }
    }
}
fn runtime_id(element: &IUIAutomationElement) -> Option<Vec<i32>> {
    unsafe {
        let array = Array(element.GetRuntimeId().ok()?);
        if array.0.is_null()
            || SafeArrayGetDim(array.0) != 1
            || SafeArrayGetElemsize(array.0) != 4
            || SafeArrayGetVartype(array.0).ok()? != VT_I4
        {
            return None;
        }
        let first = SafeArrayGetLBound(array.0, 1).ok()?;
        let last = SafeArrayGetUBound(array.0, 1).ok()?;
        let count = last.checked_sub(first)?.checked_add(1)?;
        if !(1..=64).contains(&count) {
            return None;
        }
        let mut data = std::ptr::null_mut();
        SafeArrayAccessData(array.0, &mut data).ok()?;
        let result = if data.is_null() {
            None
        } else {
            Some(std::slice::from_raw_parts(data.cast::<i32>(), count as usize).to_vec())
        };
        let unlocked = SafeArrayUnaccessData(array.0).is_ok();
        if unlocked {
            result
        } else {
            None
        }
    }
}
fn property_content(
    uia: &Uia,
    sender: &IUIAutomationElement,
    context: &Context,
    value: &VARIANT,
    counts: &Counts,
) -> Result<Option<(Vec<i32>, Content)>> {
    unsafe {
        // Check both the event's structural cache and the current ancestry.
        // Neither cache requests nor these gates read a Name or Value.
        if sender.CachedProcessId().ok().map(i64::from) != Some(context.process_id)
            || sender.CachedIsPassword().map_or(true, |v| v.as_bool())
            || sender.CachedIsOffscreen().map_or(true, |v| v.as_bool())
        {
            return Ok(None);
        }
        counts.hit(3);
        let (Ok(control), Ok(class), Ok(framework)) = (
            sender.CachedControlType(),
            sender.CachedClassName(),
            sender.CachedFrameworkId(),
        ) else {
            return Ok(None);
        };
        if control != UIA_EditControlTypeId
            || web_class(&bounded(class, 128), &bounded(framework, 128), control)
            || !uia.eligible(sender, context)
            || sender.CurrentControlType().ok() != Some(UIA_EditControlTypeId)
            || sender.CurrentIsOffscreen().map_or(true, |v| v.as_bool())
        {
            return Ok(None);
        }
        let Some(structure) = uia.content_structure(sender, context)? else {
            return Ok(None);
        };
        counts.hit(4);
        let Some(identity) = runtime_id(sender) else {
            return Err(unavailable());
        };
        counts.hit(5);
        let label = bounded(sender.CurrentName().map_err(|_| unavailable())?, 513);
        let identifier = bounded(
            sender.CurrentAutomationId().map_err(|_| unavailable())?,
            257,
        );
        // No conversion, cloning or allocation of the supplied value occurs
        // until all policy, owner, password and embedded-web checks above pass.
        counts.hit(10 + usize::from(value.Anonymous.Anonymous.vt.0).min(31));
        let text = match value.Anonymous.Anonymous.vt {
            VT_BSTR => {
                char::decode_utf16(value.Anonymous.Anonymous.Anonymous.bstrVal.iter().copied())
                    .take(2049)
                    .map(|ch| ch.unwrap_or(char::REPLACEMENT_CHARACTER))
                    .collect::<String>()
            }
            // Classic Win32 providers notify without supplying the new value.
            // Read the exact sender only after the same policy and structural
            // gates. The caller records actual sample completion time.
            VT_EMPTY => {
                let pattern = sender
                    .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                    .map_err(|_| unavailable())?;
                bounded(pattern.CurrentValue().map_err(|_| unavailable())?, 2049)
            }
            _ => return Ok(None),
        };
        counts.hit(6);
        if !uia.content_unchanged(sender, context, &structure)
            || sender.CurrentIsOffscreen().map_or(true, |v| v.as_bool())
            || runtime_id(sender).as_ref() != Some(&identity)
        {
            return Ok(None);
        }
        counts.hit(7);
        Ok(Some((
            identity,
            Content {
                role: "AXTextField".into(),
                label: (!label.is_empty()).then_some(label),
                identifier: (!identifier.is_empty()).then_some(identifier),
                value: Some(text),
            },
        )))
    }
}

pub(super) struct Subscription {
    handler: IUIAutomationPropertyChangedEventHandler,
    _cache: IUIAutomationCacheRequest,
    events: Receiver<(Vec<i32>, ValueChange)>,
    buffer: ValueBuffer,
    counts: Arc<Counts>,
    lost: Arc<AtomicBool>,
}
impl Subscription {
    pub fn start(
        uia: &Uia,
        root: &IUIAutomationElement,
        context: &Context,
        store: &Store,
        expected: (&str, u64),
        active: Arc<AtomicBool>,
        lost: Arc<AtomicBool>,
    ) -> Result<Self> {
        if context.website != Website::NotWeb
            || crate::windows::terminal_application(&context.application)
        {
            return Err(unavailable());
        }
        let (send, events) = mpsc::sync_channel(128);
        let counts = Arc::new(Counts::default());
        let handler: IUIAutomationPropertyChangedEventHandler = Handler {
            store: store.clone(),
            context: context.clone(),
            generation: expected.0.to_owned(),
            revision: expected.1,
            active,
            lost: lost.clone(),
            send,
            counts: counts.clone(),
        }
        .into();
        unsafe {
            let cache = uia.metadata.Clone().map_err(|_| unavailable())?;
            cache
                .AddProperty(UIA_IsOffscreenPropertyId)
                .map_err(|_| unavailable())?;
            uia.automation
                .AddPropertyChangedEventHandlerNativeArray(
                    root,
                    TreeScope_Subtree,
                    &cache,
                    &handler,
                    &[UIA_ValueValuePropertyId],
                )
                .map_err(|_| unavailable())?;
            Ok(Self {
                handler,
                _cache: cache,
                events,
                buffer: ValueBuffer::default(),
                lost,
                counts,
            })
        }
    }
    pub fn drain(&mut self, boundary_ms: u64) -> (Vec<ValueChange>, bool) {
        // The channel pull stays here (it owns the COM-fed receiver); the
        // ordering and action-boundary publication policy lives in the
        // cross-platform, unit-tested ValueBuffer.
        let mut partial = self.lost.swap(false, Ordering::SeqCst);
        for _ in 0..128 {
            let Ok((identity, value)) = self.events.try_recv() else {
                break;
            };
            if self.buffer.ingest(identity, value) {
                partial = true;
            }
        }
        let (ready, drained_partial) = self.buffer.drain(boundary_ms);
        (ready, partial || drained_partial)
    }
    #[cfg(debug_assertions)]
    pub fn counts(&self) -> Vec<usize> {
        self.counts
            .values
            .iter()
            .map(|n| n.load(Ordering::Relaxed))
            .collect()
    }
    pub fn remove(&self, uia: &Uia, root: &IUIAutomationElement) {
        unsafe {
            let _ = uia
                .automation
                .RemovePropertyChangedEventHandler(root, &self.handler);
        }
    }
}
