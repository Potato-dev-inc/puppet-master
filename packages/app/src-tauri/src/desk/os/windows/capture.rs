// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! Capture while the pointer is on another computer: low-level mouse and keyboard hooks that swallow what the person
//! does and report it, on a thread of their own with their own message loop. Installed only between `start` and
//! `stop`, never while the pointer is home.
//!
//! The pointer is held still in the middle of the screen it left from; motion is reported as the distance the
//! hardware moved it from there. It is not hidden: hiding it means replacing the system cursors, and a crash would
//! leave the person without one. (DevHop did hide it.)
//!
//! Fixed against DevHop: a key that was already down when the pointer left, and whose release was swallowed here,
//! is released again on `stop`, so the local keyboard is never left with a key stuck down. Callbacks never wait: a
//! busy lock lets the event through, and the sink only queues.

use std::collections::BTreeSet;
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use arc_swap::ArcSwapOption;
use desk_proto::vk_to_hid;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use super::dpi::DpiScope;
use crate::desk::link::remote::{Capture, CaptureSink};

const LLMHF_INJECTED: u32 = 1;
/// A jump bigger than this in one event is not a mouse move (the pointer was warped); it is dropped.
const MAX_STEP: i32 = 2000;

struct Ctx {
    sink: CaptureSink,
    park: (i32, i32),
    /// Virtual keys down when the pointer left.
    down_at_start: BTreeSet<u16>,
    /// Of those, the ones whose release was swallowed.
    swallowed_ups: Mutex<BTreeSet<u16>>,
}

static ACTIVE: ArcSwapOption<Ctx> = ArcSwapOption::const_empty();

pub struct CaptureHook {
    thread_id: u32,
    join: JoinHandle<()>,
    ctx: Arc<Ctx>,
}

fn key_down(vk: u16) -> bool {
    // SAFETY: plain call.
    unsafe { GetAsyncKeyState(i32::from(vk)) < 0 }
}

pub fn start(sink: CaptureSink, park: (i32, i32)) -> Result<CaptureHook, String> {
    if ACTIVE.load().is_some() {
        return Err("the mouse is already taken".into());
    }
    let down_at_start: BTreeSet<u16> = (0x08u16..=0xFE).filter(|vk| key_down(*vk)).collect();
    let ctx = Arc::new(Ctx { sink, park, down_at_start, swallowed_ups: Mutex::new(BTreeSet::new()) });
    ACTIVE.store(Some(ctx.clone()));
    let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();
    let spawned = thread::Builder::new().name("desk-capture".into()).spawn(move || {
        let _dpi = DpiScope::new();
        // SAFETY: the hook procedures are plain functions; this thread runs the message loop they need.
        unsafe {
            let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0);
            let keys = SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_proc), None, 0);
            match (mouse, keys) {
                (Ok(m), Ok(k)) => {
                    let _ = ready_tx.send(Ok(GetCurrentThreadId()));
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    let _ = UnhookWindowsHookEx(m);
                    let _ = UnhookWindowsHookEx(k);
                }
                (m, k) => {
                    if let Ok(m) = m {
                        let _ = UnhookWindowsHookEx(m);
                    }
                    if let Ok(k) = k {
                        let _ = UnhookWindowsHookEx(k);
                    }
                    let _ = ready_tx.send(Err("could not install the mouse and keyboard hooks".into()));
                }
            }
        }
    });
    let join = match spawned {
        Ok(j) => j,
        Err(e) => {
            ACTIVE.store(None);
            return Err(e.to_string());
        }
    };
    match ready_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(thread_id)) => Ok(CaptureHook { thread_id, join, ctx }),
        Ok(Err(e)) => {
            ACTIVE.store(None);
            let _ = join.join();
            Err(e)
        }
        Err(_) => {
            ACTIVE.store(None);
            Err("the capture thread did not start".into())
        }
    }
}

impl CaptureHook {
    /// Ends the hooks and waits for the thread. Returns the virtual keys that were down when the pointer left and
    /// were let go meanwhile: their release never reached this computer and must be sent now.
    pub fn stop(self) -> Vec<u16> {
        ACTIVE.store(None);
        // SAFETY: plain call to the capture thread's id.
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        let _ = self.join.join();
        self.ctx.swallowed_ups.lock().map(|s| s.iter().copied().collect()).unwrap_or_default()
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let Some(ctx) = ACTIVE.load_full() else {
        return CallNextHookEx(None, code, wparam, lparam);
    };
    let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
    if info.flags & LLMHF_INJECTED != 0 {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let event = match wparam.0 as u32 {
        WM_MOUSEMOVE => {
            let (dx, dy) = (info.pt.x - ctx.park.0, info.pt.y - ctx.park.1);
            if (dx != 0 || dy != 0) && dx.abs() < MAX_STEP && dy.abs() < MAX_STEP {
                Some(Capture::Motion { dx, dy })
            } else {
                None
            }
        }
        WM_LBUTTONDOWN => Some(Capture::Button { button: 0, down: true }),
        WM_LBUTTONUP => Some(Capture::Button { button: 0, down: false }),
        WM_RBUTTONDOWN => Some(Capture::Button { button: 1, down: true }),
        WM_RBUTTONUP => Some(Capture::Button { button: 1, down: false }),
        WM_MBUTTONDOWN => Some(Capture::Button { button: 2, down: true }),
        WM_MBUTTONUP => Some(Capture::Button { button: 2, down: false }),
        WM_MOUSEWHEEL => Some(Capture::Wheel { dx: 0, dy: i32::from((info.mouseData >> 16) as i16) }),
        WM_MOUSEHWHEEL => Some(Capture::Wheel { dx: i32::from((info.mouseData >> 16) as i16), dy: 0 }),
        _ => None,
    };
    if let Some(e) = event {
        (ctx.sink)(e);
    }
    // Everything the person does with the mouse stays off this computer while the pointer is away.
    LRESULT(1)
}

unsafe extern "system" fn key_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let Some(ctx) = ACTIVE.load_full() else {
        return CallNextHookEx(None, code, wparam, lparam);
    };
    let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
    if info.flags.0 & LLKHF_INJECTED.0 != 0 {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let message = wparam.0 as u32;
    let down = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
    let up = matches!(message, WM_KEYUP | WM_SYSKEYUP);
    if !down && !up {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let vk = info.vkCode as u16;
    if up && ctx.down_at_start.contains(&vk) {
        if let Ok(mut s) = ctx.swallowed_ups.try_lock() {
            s.insert(vk);
        }
    }
    if let Some(hid) = vk_to_hid(vk) {
        (ctx.sink)(Capture::Key { hid, down });
    }
    LRESULT(1)
}
