//! Bounded UI Automation reads after the History policy admits a native window.
//! Labels and values are never queried before password, ancestry and owner checks.
use crate::{capture::Content, policy::Context, store::CaptureMode, Error, Result};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    time::{Duration, Instant},
};
use windows::{
    core::{implement, AgileReference, Interface, Ref},
    Win32::{
        Foundation::HWND,
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_MULTITHREADED,
        },
        UI::Accessibility::*,
    },
};

#[path = "windows_values.rs"]
mod values;
pub use crate::value_buffer::ValueChange;

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

struct PendingInvocation {
    sender: AgileReference<IUIAutomationElement>,
    received: Instant,
    timestamp_ms: u64,
}

#[implement(IUIAutomationEventHandler)]
struct InvocationHandler {
    send: SyncSender<PendingInvocation>,
    active: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
    context: Context,
}

impl IUIAutomationEventHandler_Impl for InvocationHandler_Impl {
    fn HandleAutomationEvent(
        &self,
        sender: Ref<'_, IUIAutomationElement>,
        eventid: UIA_EVENT_ID,
    ) -> windows::core::Result<()> {
        // This callback is a provider notification, not a low-level input hook.
        // Keep an apartment-safe reference to the actual sender. No Name, Value
        // or other content is read until the collector rechecks its consent.
        if eventid != UIA_Invoke_InvokedEventId
            || !self.active.load(Ordering::SeqCst)
            || crate::windows::foreground()
                != Some((self.context.process_id as u32, self.context.window_id))
        {
            return Ok(());
        }
        let received = Instant::now();
        let timestamp_ms = crate::now_ms();
        if let Some(sender) = sender.as_ref() {
            match AgileReference::new(sender) {
                Ok(sender) => {
                    if self.active.load(Ordering::SeqCst)
                        && self
                            .send
                            .try_send(PendingInvocation {
                                sender,
                                received,
                                timestamp_ms,
                            })
                            .is_err()
                    {
                        self.lost.store(true, Ordering::SeqCst);
                    }
                }
                Err(_) => self.lost.store(true, Ordering::SeqCst),
            }
        }
        Ok(())
    }
}

/// Provider-confirmed virtual button actions under the single History collector.
/// They are invocations, not inferred physical clicks or shortcut keystrokes.
pub struct Invocations {
    root: IUIAutomationElement,
    handler: IUIAutomationEventHandler,
    events: Receiver<PendingInvocation>,
    active: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
    context: Context,
    values: Option<values::Subscription>,
    // Keep the apartment alive until every interface above has been released.
    uia: Uia,
}

impl Invocations {
    pub fn start(context: &Context) -> Result<Self> {
        let uia = Uia::new()?;
        let active = Arc::new(AtomicBool::new(true));
        let lost = Arc::new(AtomicBool::new(false));
        let (send, events) = mpsc::sync_channel(128);
        let handler: IUIAutomationEventHandler = InvocationHandler {
            send,
            active: active.clone(),
            lost: lost.clone(),
            context: context.clone(),
        }
        .into();
        unsafe {
            let root = uia
                .automation
                .ElementFromHandle(HWND(context.window_id as usize as *mut _))
                .map_err(|_| unavailable())?;
            uia.automation
                .AddAutomationEventHandler(
                    UIA_Invoke_InvokedEventId,
                    &root,
                    TreeScope_Subtree,
                    &uia.metadata,
                    &handler,
                )
                .map_err(|_| unavailable())?;
            Ok(Self {
                root,
                handler,
                events,
                active,
                lost,
                context: context.clone(),
                values: None,
                uia,
            })
        }
    }

    /// Attach value notifications to this same collector-owned subscription.
    /// No separate recorder, consent source or store is created.
    pub fn start_with_values(context: &Context, store: &crate::store::Store,
        expected: (&str, u64)) -> Result<Self> {
        let mut own = Self::start(context)?;
        own.values = Some(values::Subscription::start(&own.uia, &own.root,
            context, store, expected, own.active.clone(), own.lost.clone())?);
        Ok(own)
    }

    pub fn drain_values(&mut self, boundary_ms: u64) -> (Vec<ValueChange>, bool) {
        self.values.as_mut().map(|values| values.drain(boundary_ms))
            .unwrap_or_default()
    }

    #[cfg(debug_assertions)]
    pub fn value_diagnostics(&self)->Vec<usize> {self.values.as_ref().map(|v|v.counts()).unwrap_or_default()}

    /// Revalidate consent before and after each provider read. The callback must
    /// bind the current policy revision and generation to this subscription.
    pub fn drain(&self, authorized: impl Fn() -> bool) -> (Vec<(u64, Content)>, bool) {
        let mut output = Vec::new();
        let mut partial = self.lost.swap(false, Ordering::SeqCst);
        for _ in 0..64 {
            let Ok(event) = self.events.try_recv() else {
                break;
            };
            if !authorized()
                || event.received.elapsed() > Duration::from_secs(2)
                || crate::windows::foreground()
                    != Some((self.context.process_id as u32, self.context.window_id))
            {
                partial = true;
                continue;
            }
            let content = event.sender.resolve().ok().and_then(|sender| {
                self.uia
                    .invoked_content(&sender, &self.context)
                    .ok()
                    .flatten()
            });
            if let Some(content) = content {
                if event.received.elapsed() <= Duration::from_secs(2)
                    && authorized()
                    && crate::windows::foreground()
                        == Some((self.context.process_id as u32, self.context.window_id))
                {
                    output.push((event.timestamp_ms, content));
                } else {
                    partial = true;
                }
            }
        }
        (output, partial)
    }
}

impl Drop for Invocations {
    fn drop(&mut self) {
        // Disable callbacks before removing the subscription and dropping the
        // queue. An in-flight callback cannot publish into a later session.
        self.active.store(false, Ordering::SeqCst);
        unsafe {
            if let Some(values) = &self.values { values.remove(&self.uia, &self.root); }
            let _ = self.uia.automation.RemoveAutomationEventHandler(
                UIA_Invoke_InvokedEventId,
                &self.root,
                &self.handler,
            );
        }
    }
}

pub(crate) struct Uia {
    automation: IUIAutomation,
    metadata: IUIAutomationCacheRequest,
    focused: Option<IUIAutomationElement>,
    // Interfaces above are released before their apartment is uninitialized.
    _apartment: Apartment,
}

fn bounded(value: windows::core::BSTR, max_chars: usize) -> String {
    char::decode_utf16(value.iter().copied())
        .take(max_chars)
        .map(|value| value.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

fn unavailable() -> Error {
    Error::new(
        "computerHistoryAccessibilityUnavailable",
        "Windows accessibility is unavailable or exceeded its deadline",
    )
}
// UIA represents the end of a tree as S_OK plus a null interface. Preserve
// that distinction from provider failure before trusting a node as a leaf.
fn first_child(
    walker: &IUIAutomationTreeWalker,
    element: &IUIAutomationElement,
) -> Result<Option<IUIAutomationElement>> {
    unsafe {
        let mut raw = std::ptr::null_mut();
        (Interface::vtable(walker).GetFirstChildElement)(
            Interface::as_raw(walker),
            Interface::as_raw(element),
            &mut raw,
        )
        .ok()
        .map_err(|_| unavailable())?;
        Ok((!raw.is_null()).then(|| IUIAutomationElement::from_raw(raw)))
    }
}
fn next_sibling(
    walker: &IUIAutomationTreeWalker,
    element: &IUIAutomationElement,
) -> Result<Option<IUIAutomationElement>> {
    unsafe {
        let mut raw = std::ptr::null_mut();
        (Interface::vtable(walker).GetNextSiblingElement)(
            Interface::as_raw(walker),
            Interface::as_raw(element),
            &mut raw,
        )
        .ok()
        .map_err(|_| unavailable())?;
        Ok((!raw.is_null()).then(|| IUIAutomationElement::from_raw(raw)))
    }
}
fn web_class(class: &str, framework: &str, control: UIA_CONTROLTYPE_ID) -> bool {
    let class = class.to_ascii_lowercase();
    let framework = framework.to_ascii_lowercase();
    framework == "chrome"
        || framework == "msaahtml"
        || framework == "mozilla"
        || class.contains("chrome_")
        || class.contains("webview")
        || class.contains("internet explorer_server")
        || class.contains("internetexplorer_server")
        || control == UIA_DocumentControlTypeId
            && !matches!(framework.as_str(), "win32" | "winform" | "wpf" | "xaml")
}
#[allow(non_upper_case_globals)]
fn role(control: UIA_CONTROLTYPE_ID) -> Option<&'static str> {
    match control {
        UIA_EditControlTypeId => Some("AXTextField"),
        UIA_DocumentControlTypeId => Some("AXTextArea"),
        UIA_TextControlTypeId => Some("AXStaticText"),
        UIA_ButtonControlTypeId => Some("AXButton"),
        UIA_CheckBoxControlTypeId => Some("AXCheckBox"),
        UIA_RadioButtonControlTypeId => Some("AXRadioButton"),
        UIA_ComboBoxControlTypeId => Some("AXComboBox"),
        UIA_SliderControlTypeId => Some("AXSlider"),
        _ => None,
    }
}

impl Uia {
    // Plain WPF text boxes expose scroll decorations as raw children. They are
    // still one editable value, unlike arbitrary containers that aggregate
    // descendant text. Inspect every decoration before reading that value.
    #[allow(non_upper_case_globals)]
    fn content_structure(
        &self,
        element: &IUIAutomationElement,
        context: &Context,
    ) -> Result<Option<Vec<IUIAutomationElement>>> {
        unsafe {
            let walker = self.automation.RawViewWalker().map_err(|_| unavailable())?;
            let Some(child) = first_child(&walker, element)? else {
                return Ok(Some(Vec::new()));
            };
            if element.CurrentControlType().map_err(|_| unavailable())? != UIA_EditControlTypeId
                || bounded(
                    element.CurrentFrameworkId().map_err(|_| unavailable())?,
                    128,
                ) != "WPF"
                || bounded(element.CurrentClassName().map_err(|_| unavailable())?, 128) != "TextBox"
            {
                return Ok(None);
            }
            let deadline = Instant::now() + Duration::from_millis(750);
            let mut pending = vec![(child, 0usize)];
            let mut structure = Vec::new();
            while let Some((node, depth)) = pending.pop() {
                if structure.len() >= 32
                    || depth >= 12
                    || Instant::now() >= deadline
                    || !self.eligible(&node, context)
                    || bounded(node.CurrentFrameworkId().map_err(|_| unavailable())?, 128) != "WPF"
                {
                    return Ok(None);
                }
                let control = node.CurrentControlType().map_err(|_| unavailable())?;
                let safe = match control {
                    UIA_PaneControlTypeId => matches!(
                        bounded(node.CurrentClassName().map_err(|_| unavailable())?, 128).as_str(),
                        "ScrollViewer" | "ScrollContentPresenter"
                    ),
                    UIA_ScrollBarControlTypeId
                    | UIA_ThumbControlTypeId
                    | UIA_ButtonControlTypeId
                    | UIA_TextControlTypeId => true,
                    _ => false,
                };
                if !safe {
                    return Ok(None);
                }
                if let Some(sibling) = next_sibling(&walker, &node)? {
                    pending.push((sibling, depth));
                }
                if let Some(child) = first_child(&walker, &node)? {
                    pending.push((child, depth + 1));
                }
                structure.push(node);
            }
            Ok(Some(structure))
        }
    }
    fn content_unchanged(
        &self,
        element: &IUIAutomationElement,
        context: &Context,
        before: &[IUIAutomationElement],
    ) -> bool {
        if !self.eligible(element, context) {
            return false;
        }
        let Ok(Some(after)) = self.content_structure(element, context) else {
            return false;
        };
        before.len() == after.len()
            && before.iter().zip(&after).all(|(a, b)| unsafe {
                self.automation
                    .CompareElements(a, b)
                    .is_ok_and(|same| same.as_bool())
            })
    }
    #[allow(non_upper_case_globals)]
    fn invoked_content(
        &self,
        element: &IUIAutomationElement,
        context: &Context,
    ) -> Result<Option<Content>> {
        unsafe {
            // Native-child actions already have hook attribution. Subscribe only
            // to virtual buttons here, so one action never becomes two records.
            if !self.eligible(element, context)
                || element
                    .CurrentNativeWindowHandle()
                    .map_err(|_| unavailable())?
                    .0 as usize
                    != 0
                || element.CurrentControlType().map_err(|_| unavailable())?
                    != UIA_ButtonControlTypeId
                || element.CurrentIsOffscreen().map_or(true, |v| v.as_bool())
            {
                return Ok(None);
            }
            // WPF button templates contain text/image children. Inspect their
            // safety metadata before reading an aggregate button name. Editable,
            // web, protected and unknown child structures are deliberately refused.
            let Some(children) = self.invocation_children(element, context)? else {
                return Ok(None);
            };
            let label = bounded(element.CurrentName().map_err(|_| unavailable())?, 513);
            let identifier = bounded(
                element.CurrentAutomationId().map_err(|_| unavailable())?,
                257,
            );
            if !self.eligible(element, context)
                || element.CurrentIsOffscreen().map_or(true, |v| v.as_bool())
            {
                return Ok(None);
            }
            let Some(current) = self.invocation_children(element, context)? else {
                return Ok(None);
            };
            if current.len() != children.len()
                || !current.iter().zip(&children).all(|(a, b)| {
                    self.automation
                        .CompareElements(a, b)
                        .is_ok_and(|same| same.as_bool())
                })
            {
                return Ok(None);
            }
            Ok(Some(Content {
                role: "AXButton".into(),
                label: (!label.is_empty()).then_some(label),
                identifier: (!identifier.is_empty()).then_some(identifier),
                value: None,
            }))
        }
    }
    #[allow(non_upper_case_globals)]
    fn invocation_children(
        &self,
        element: &IUIAutomationElement,
        context: &Context,
    ) -> Result<Option<Vec<IUIAutomationElement>>> {
        unsafe {
            let walker = self.automation.RawViewWalker().map_err(|_| unavailable())?;
            let deadline = Instant::now() + Duration::from_millis(750);
            let mut child = first_child(&walker, element)?;
            let mut children = Vec::new();
            while let Some(current) = child {
                if children.len() >= 32
                    || Instant::now() >= deadline
                    || !self.eligible(&current, context)
                    || !matches!(
                        current.CurrentControlType().map_err(|_| unavailable())?,
                        UIA_TextControlTypeId | UIA_ImageControlTypeId
                    )
                    || first_child(&walker, &current)?.is_some()
                {
                    return Ok(None);
                }
                child = next_sibling(&walker, &current)?;
                children.push(current);
            }
            Ok(Some(children))
        }
    }
    pub(crate) fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(|_| unavailable())?;
            let apartment = Apartment;
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)
                    .map_err(|_| unavailable())?;
            let limits: IUIAutomation2 = automation.cast().map_err(|_| unavailable())?;
            limits
                .SetConnectionTimeout(300)
                .map_err(|_| unavailable())?;
            limits
                .SetTransactionTimeout(300)
                .map_err(|_| unavailable())?;
            let metadata = automation.CreateCacheRequest().map_err(|_| unavailable())?;
            metadata
                .SetTreeScope(TreeScope_Element)
                .map_err(|_| unavailable())?;
            // Batch only structural/safety properties. A cache containing Name
            // or Value would read a protected field before deciding its scope.
            for property in [
                UIA_ProcessIdPropertyId,
                UIA_IsPasswordPropertyId,
                UIA_ControlTypePropertyId,
                UIA_ClassNamePropertyId,
                UIA_FrameworkIdPropertyId,
                UIA_NativeWindowHandlePropertyId,
            ] {
                metadata.AddProperty(property).map_err(|_| unavailable())?;
            }
            Ok(Self {
                automation,
                metadata,
                focused: None,
                _apartment: apartment,
            })
        }
    }
    // Structure and safety properties only. Returning true requires reaching the
    // exact admitted top-level HWND through a bounded, same-process ancestry.
    pub(crate) fn eligible(&self, element: &IUIAutomationElement, context: &Context) -> bool {
        unsafe {
            let Ok(walker) = self.automation.RawViewWalker() else {
                return false;
            };
            let deadline = Instant::now() + Duration::from_millis(750);
            let Ok(root) = self
                .automation
                .ElementFromHandle(HWND(context.window_id as usize as *mut _))
            else {
                return false;
            };
            let Ok(mut current) = element.BuildUpdatedCache(&self.metadata) else {
                return false;
            };
            for _ in 0..48 {
                if Instant::now() >= deadline {
                    return false;
                }
                if current.CachedProcessId().ok().map(i64::from) != Some(context.process_id)
                    || current.CachedIsPassword().map_or(true, |v| v.as_bool())
                {
                    return false;
                }
                let (Ok(control), Ok(class), Ok(framework)) = (
                    current.CachedControlType(),
                    current.CachedClassName(),
                    current.CachedFrameworkId(),
                ) else {
                    return false;
                };
                if web_class(&bounded(class, 128), &bounded(framework, 128), control) {
                    return false;
                }
                let Ok(hwnd) = current.CachedNativeWindowHandle() else {
                    return false;
                };
                if hwnd.0 as usize as i64 == context.window_id
                    && self
                        .automation
                        .CompareElements(&current, &root)
                        .is_ok_and(|same| same.as_bool())
                {
                    return true;
                }
                let Ok(parent) = walker.GetParentElementBuildCache(&current, &self.metadata) else {
                    return false;
                };
                if self
                    .automation
                    .CompareElements(&current, &parent)
                    .map_or(true, |v| v.as_bool())
                {
                    return false;
                }
                current = parent;
            }
            false
        }
    }
    pub(crate) fn read(&mut self, context: &Context, mode: CaptureMode) -> Result<Option<Content>> {
        self.focused = None;
        unsafe {
            let element = self
                .automation
                .GetFocusedElement()
                .map_err(|_| unavailable())?;
            if !self.eligible(&element, context)
                || element
                    .CurrentHasKeyboardFocus()
                    .map_or(true, |v| !v.as_bool())
                || element.CurrentIsOffscreen().map_or(true, |v| v.as_bool())
            {
                return Ok(None);
            }
            let Some(role) = element.CurrentControlType().ok().and_then(role) else {
                return Ok(None);
            };
            let Some(structure) = self.content_structure(&element, context)? else {
                return Ok(None);
            };
            let label = bounded(element.CurrentName().map_err(|_| unavailable())?, 513);
            let identifier = bounded(
                element.CurrentAutomationId().map_err(|_| unavailable())?,
                257,
            );
            let value = if mode == CaptureMode::Values {
                if let Ok(pattern) =
                    element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                {
                    Some(bounded(
                        pattern.CurrentValue().map_err(|_| unavailable())?,
                        2049,
                    ))
                } else if let Ok(pattern) =
                    element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                {
                    Some(bounded(
                        pattern
                            .DocumentRange()
                            .and_then(|range| range.GetText(4098))
                            .map_err(|_| unavailable())?,
                        2049,
                    ))
                } else {
                    None
                }
            } else {
                None
            };
            if !self.content_unchanged(&element, context, &structure) {
                return Ok(None);
            }
            self.focused = Some(element);
            Ok(Some(Content {
                role: role.into(),
                label: (!label.is_empty()).then_some(label),
                identifier: (!identifier.is_empty()).then_some(identifier),
                value,
            }))
        }
    }
    pub(crate) fn focus_target(
        &mut self,
        context: &Context,
        labels: bool,
    ) -> Result<Option<(Content, i64)>> {
        self.focused = None;
        unsafe {
            let element = self
                .automation
                .GetFocusedElement()
                .map_err(|_| unavailable())?;
            if !self.eligible(&element, context)
                || element
                    .CurrentHasKeyboardFocus()
                    .map_or(true, |v| !v.as_bool())
            {
                return Ok(None);
            }
            let kind = element.CurrentControlType().ok().and_then(role);
            let leaf = if labels {
                let walker = self.automation.RawViewWalker().map_err(|_| unavailable())?;
                first_child(&walker, &element)?.is_none()
            } else {
                true
            };
            let readable =
                kind.is_some() && leaf && element.CurrentIsOffscreen().is_ok_and(|v| !v.as_bool());
            // A blank native window or container can have focus between
            // controls. Keep window observation alive without reading its
            // aggregate name or attributing keys to an unknown target.
            let handle = if readable {
                element
                    .CurrentNativeWindowHandle()
                    .map_err(|_| unavailable())?
                    .0 as usize as i64
            } else {
                0
            };
            let label = if labels && readable {
                Some(bounded(
                    element.CurrentName().map_err(|_| unavailable())?,
                    513,
                ))
            } else {
                None
            };
            if !self.eligible(&element, context) {
                return Ok(None);
            }
            self.focused = Some(element);
            Ok(Some((
                Content {
                    role: kind.unwrap_or("AXUnknown").into(),
                    label,
                    identifier: None,
                    value: None,
                },
                handle,
            )))
        }
    }
    pub(crate) fn target_at(
        &self,
        context: &Context,
        x: i32,
        y: i32,
        expected: i64,
    ) -> Result<Option<Content>> {
        unsafe {
            let element = self
                .automation
                .ElementFromPoint(windows::Win32::Foundation::POINT { x, y })
                .map_err(|_| unavailable())?;
            if !self.eligible(&element, context) {
                return Ok(None);
            }
            // Bind the delayed semantic read to the actual native child under
            // the pointer at hook time. Virtual or replaced targets are omitted.
            let handle = element
                .CurrentNativeWindowHandle()
                .map_err(|_| unavailable())?
                .0 as usize as i64;
            if handle == 0 || handle != expected {
                return Ok(None);
            }
            let Some(kind) = element.CurrentControlType().ok().and_then(role) else {
                return Ok(None);
            };
            let walker = self.automation.RawViewWalker().map_err(|_| unavailable())?;
            if first_child(&walker, &element)?.is_some() {
                return Ok(None);
            }
            let label = bounded(element.CurrentName().map_err(|_| unavailable())?, 513);
            if !self.eligible(&element, context) {
                return Ok(None);
            }
            Ok(Some(Content {
                role: kind.into(),
                label: Some(label),
                identifier: None,
                value: None,
            }))
        }
    }
    pub(crate) fn terminal_text(&mut self, context: &Context) -> Result<Option<(String, bool)>> {
        unsafe {
            let element = self
                .automation
                .GetFocusedElement()
                .map_err(|_| unavailable())?;
            if !self.eligible(&element, context) {
                return Ok(None);
            }
            let (value, partial) = if let Ok(pattern) =
                element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
            {
                let range = pattern.DocumentRange().map_err(|_| unavailable())?;
                range
                    .MoveEndpointByRange(
                        TextPatternRangeEndpoint_Start,
                        &range,
                        TextPatternRangeEndpoint_End,
                    )
                    .map_err(|_| unavailable())?;
                let moved = range
                    .MoveEndpointByUnit(TextPatternRangeEndpoint_Start, TextUnit_Character, -8192)
                    .map_err(|_| unavailable())?;
                (
                    bounded(range.GetText(16386).map_err(|_| unavailable())?, 8193),
                    moved <= -8192,
                )
            } else if let Ok(pattern) =
                element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            {
                let value = pattern.CurrentValue().map_err(|_| unavailable())?;
                let start = value.len().saturating_sub(16384);
                (String::from_utf16_lossy(&value[start..]), start > 0)
            } else {
                return Ok(None);
            };
            if !self.eligible(&element, context) {
                return Ok(None);
            }
            self.focused = Some(element);
            Ok(Some((value, partial)))
        }
    }

    pub(crate) fn window_text(&self, context: &Context) -> Result<Option<Content>> {
        unsafe {
            let root = self
                .automation
                .ElementFromHandle(HWND(context.window_id as usize as *mut _))
                .map_err(|_| unavailable())?;
            if !self.eligible(&root, context) {
                return Ok(None);
            }
            let walker = self.automation.RawViewWalker().map_err(|_| unavailable())?;
            let deadline = Instant::now() + Duration::from_millis(900);
            let mut pending = vec![(root, 0usize)];
            let mut lines = Vec::new();
            let mut visited = 0;
            let mut partial = false;
            while let Some((element, depth)) = pending.pop() {
                if visited >= 128 || Instant::now() >= deadline {
                    partial = true;
                    break;
                }
                visited += 1;
                // Excluded subtrees are never traversed for text.
                if !self.eligible(&element, context)
                    || element.CurrentIsOffscreen().map_or(true, |v| v.as_bool())
                {
                    continue;
                }
                let control = element.CurrentControlType().map_err(|_| unavailable())?;
                let child = first_child(&walker, &element)?;
                // Arbitrary containers can aggregate protected descendants.
                // Only leaves and structurally checked plain editors expose
                // content here; other containers are traversed separately.
                let structure = if role(control).is_some() {
                    self.content_structure(&element, context)?
                } else {
                    None
                };
                if let Some(ref structure) = structure {
                    if let Some(kind) = role(control) {
                        let name = bounded(element.CurrentName().map_err(|_| unavailable())?, 257);
                        let mut value = if let Ok(pattern) = element
                            .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                        {
                            bounded(pattern.CurrentValue().map_err(|_| unavailable())?, 2049)
                        } else if control == UIA_TextControlTypeId {
                            name.clone()
                        } else if let Ok(pattern) = element
                            .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                        {
                            bounded(
                                pattern
                                    .DocumentRange()
                                    .and_then(|range| range.GetText(4098))
                                    .map_err(|_| unavailable())?,
                                2049,
                            )
                        } else {
                            String::new()
                        };
                        if !self.content_unchanged(&element, context, structure) {
                            continue;
                        }
                        partial |= value.chars().count() > 2048 || name.chars().count() > 256;
                        value = value.chars().take(2048).collect();
                        if !name.is_empty() || !value.is_empty() {
                            let label = if control == UIA_TextControlTypeId {
                                String::new()
                            } else {
                                name.chars().take(256).collect()
                            };
                            lines.push(format!("{kind} {label:?}: {value:?}"));
                        }
                    }
                }
                if structure.is_some() && child.is_some() {
                    // A checked editor's scroll decorations are not a second
                    // projection of the same editable value.
                    continue;
                }
                if depth >= 16 {
                    partial = true;
                    continue;
                }
                let mut children = Vec::new();
                if let Some(mut child) = child {
                    loop {
                        if children.len() >= 128 || Instant::now() >= deadline {
                            partial = true;
                            break;
                        }
                        children.push(child.clone());
                        let Ok(next) = walker.GetNextSiblingElement(&child) else {
                            break;
                        };
                        if self
                            .automation
                            .CompareElements(&child, &next)
                            .map_or(true, |v| v.as_bool())
                        {
                            partial = true;
                            break;
                        }
                        child = next;
                    }
                }
                pending.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
            }
            if partial {
                lines.push("[Partial accessible window snapshot]".into())
            }
            if lines.is_empty() {
                return Ok(None);
            }
            Ok(Some(Content {
                role: "AXWindowSnapshot".into(),
                label: Some("Native window context".into()),
                identifier: None,
                value: Some(lines.join("\n")),
            }))
        }
    }

    pub(crate) fn verify_focus(&self, context: &Context) -> bool {
        let Some(prior) = &self.focused else {
            return false;
        };
        unsafe {
            let Ok(current) = self.automation.GetFocusedElement() else {
                return false;
            };
            self.eligible(&current, context)
                && self
                    .automation
                    .CompareElements(prior, &current)
                    .is_ok_and(|same| same.as_bool())
        }
    }
}
