//! Scoped native action metadata. Printable keystrokes never enter this queue;
//! approved UIA observations supply text only after secure-control checks.
use crate::{Error, Result};
use std::{
    cell::RefCell,
    ptr,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::POINT,
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
        },
        WindowsAndMessaging::*,
    },
};

#[derive(Clone, Debug)]
pub enum Input {
    Click {
        x: i32,
        y: i32,
        right: bool,
        target: i64,
    },
    Key {
        vk: u32,
        ctrl: bool,
        alt: bool,
        shift: bool,
        win: bool,
        focus: i64,
    },
}
#[derive(Clone, Debug)]
pub struct Event {
    pub timestamp_ms: u64,
    pub process_id: i64,
    pub window_id: i64,
    pub input: Input,
}
struct HookState {
    pid: u32,
    window: usize,
    send: SyncSender<Event>,
    lost: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
}
thread_local! {static HOOK:RefCell<Option<HookState>>=const{RefCell::new(None)};}
fn publish(input: Input) {
    HOOK.with(|slot| {
        let state = slot.borrow();
        let Some(s) = state.as_ref() else { return };
        if !s.active.load(Ordering::SeqCst) {
            return;
        }
        unsafe {
            let hwnd = GetForegroundWindow();
            let mut pid = 0;
            if hwnd as usize != s.window
                || GetWindowThreadProcessId(hwnd, &mut pid) == 0
                || pid != s.pid
            {
                return;
            }
            let event = Event {
                timestamp_ms: crate::now_ms(),
                process_id: i64::from(pid),
                window_id: hwnd as usize as i64,
                input,
            };
            if s.send.try_send(event).is_err() {
                s.lost.store(true, Ordering::SeqCst)
            }
        }
    });
}
unsafe extern "system" fn keyboard(code: i32, message: usize, data: isize) -> isize {
    if code == HC_ACTION as i32 && (message as u32 == WM_KEYDOWN || message as u32 == WM_SYSKEYDOWN)
    {
        let key = unsafe { &*(data as *const KBDLLHOOKSTRUCT) };
        let pressed = |vk: u16| unsafe { GetAsyncKeyState(i32::from(vk)) < 0 };
        let ctrl = pressed(VK_CONTROL);
        let alt = pressed(VK_MENU);
        let shift = pressed(VK_SHIFT);
        let win = pressed(VK_LWIN) || pressed(VK_RWIN);
        // No ordinary letters, digits or password-length metadata are retained.
        if ctrl || alt || win || matches!(key.vkCode, 0x0D | 0x09 | 0x1B) {
            let mut info: GUITHREADINFO = unsafe { std::mem::zeroed() };
            info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
            if unsafe { GetGUIThreadInfo(0, &mut info) } != 0 {
                publish(Input::Key {
                    vk: key.vkCode,
                    ctrl,
                    alt,
                    shift,
                    win,
                    focus: info.hwndFocus as usize as i64,
                });
            }
        }
    }
    unsafe { CallNextHookEx(ptr::null_mut(), code, message, data) }
}
unsafe extern "system" fn mouse(code: i32, message: usize, data: isize) -> isize {
    if code == HC_ACTION as i32 && matches!(message as u32, WM_LBUTTONDOWN | WM_RBUTTONDOWN) {
        let event = unsafe { &*(data as *const MSLLHOOKSTRUCT) };
        let target = HOOK.with(|slot| {
            let state = slot.borrow();
            let Some(s) = state.as_ref() else {
                return None;
            };
            unsafe {
                let hwnd = WindowFromPoint(POINT {
                    x: event.pt.x,
                    y: event.pt.y,
                });
                let mut pid = 0;
                (GetWindowThreadProcessId(hwnd, &mut pid) != 0
                    && pid == s.pid
                    && GetAncestor(hwnd, GA_ROOT) as usize == s.window)
                    .then_some(hwnd as usize as i64)
            }
        });
        if let Some(target) = target {
            publish(Input::Click {
                x: event.pt.x,
                y: event.pt.y,
                right: message as u32 == WM_RBUTTONDOWN,
                target,
            })
        }
    }
    unsafe { CallNextHookEx(ptr::null_mut(), code, message, data) }
}

pub struct InputSource {
    events: Receiver<Event>,
    lost: Arc<AtomicBool>,
    thread_id: u32,
    worker: Option<JoinHandle<()>>,
    active: Arc<AtomicBool>,
}
impl InputSource {
    pub fn start(process_id: i64, window_id: i64) -> Result<Self> {
        let pid = u32::try_from(process_id)
            .map_err(|_| Error::invalid("Invalid event source process"))?;
        if pid == 0 || window_id == 0 {
            return Err(Error::invalid("Native event identity required"));
        }
        let (send, events) = mpsc::sync_channel(256);
        let lost = Arc::new(AtomicBool::new(false));
        let flag = lost.clone();
        let active = Arc::new(AtomicBool::new(true));
        let running = active.clone();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || unsafe {
            let id = GetCurrentThreadId();
            let mut message: MSG = std::mem::zeroed();
            PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_NOREMOVE);
            HOOK.with(|slot| {
                *slot.borrow_mut() = Some(HookState {
                    pid,
                    window: window_id as usize,
                    send,
                    lost: flag,
                    active: running.clone(),
                })
            });
            let module = GetModuleHandleW(ptr::null());
            let keys = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard), module, 0);
            let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse), module, 0);
            if keys.is_null() || mouse.is_null() {
                if !keys.is_null() {
                    UnhookWindowsHookEx(keys);
                }
                if !mouse.is_null() {
                    UnhookWindowsHookEx(mouse);
                }
                let _ = ready_tx.send(None);
                return;
            }
            if !running.load(Ordering::SeqCst) || ready_tx.send(Some(id)).is_err() {
                UnhookWindowsHookEx(keys);
                UnhookWindowsHookEx(mouse);
                HOOK.with(|slot| *slot.borrow_mut() = None);
                return;
            }
            while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            UnhookWindowsHookEx(keys);
            UnhookWindowsHookEx(mouse);
            HOOK.with(|slot| *slot.borrow_mut() = None);
        });
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Some(thread_id)) => Ok(Self {
                events,
                lost,
                thread_id,
                worker: Some(worker),
                active,
            }),
            _ => {
                active.store(false, Ordering::SeqCst);
                if worker.is_finished() {
                    let _ = worker.join();
                }
                Err(Error::new(
                    "computerHistoryInputUnavailable",
                    "Native Windows input observation did not start",
                ))
            }
        }
    }
    pub fn next(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }
    pub fn take_lost(&self) -> bool {
        self.lost.swap(false, Ordering::SeqCst)
    }
}
impl Drop for InputSource {
    fn drop(&mut self) {
        self.active.store(false, Ordering::SeqCst);
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        };
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
