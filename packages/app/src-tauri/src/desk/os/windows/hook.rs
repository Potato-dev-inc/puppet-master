// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! The keyboard hook, on its own thread with its own message loop. The callback only decides (the pure
//! `HotkeyMatcher`) and `try_send`s; it never waits. Windows removes a hook whose callback is slow, so
//! the engine's watchdog asks [`Hook::health`] and installs a new one.
//!
//! Only the keyboard is hooked. Local hops do not need the mouse, so the mouse hook DevHop installed for
//! remote control is not installed here.
//!
//! Fixed against DevHop: each install gets its own receiver (DevHop kept the first one in a `OnceLock`
//! forever, so a stop and start delivered into a dead channel), modifiers come from the hook's own events,
//! the key-up of a swallowed hotkey is swallowed, and the callback never waits on a full channel.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use arc_swap::ArcSwapOption;
use desk_proto::vk_to_hid;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::desk::engine::hotkeys::{HotkeyMatcher, Table, Verdict};
use crate::desk::host::{hook_looks_dropped, HookHealth, HostEvent, HostSink, DROPPED_AFTER_MS};

struct Ctx {
    matcher: Mutex<HotkeyMatcher>,
    sink: HostSink,
    last_callback_ms: AtomicU64,
}

/// What the callback reads. A plain function cannot carry state, so the live install is here.
static ACTIVE: ArcSwapOption<Ctx> = ArcSwapOption::const_empty();

pub struct Hook {
    thread_id: u32,
    join: JoinHandle<()>,
    alive: Arc<AtomicBool>,
    ctx: Arc<Ctx>,
}

/// (virtual key, HID usage) for the eight modifier keys.
const MODIFIERS: [(u16, u16); 8] = [
    (0xA2, 0xE0),
    (0xA0, 0xE1),
    (0xA4, 0xE2),
    (0x5B, 0xE3),
    (0xA3, 0xE4),
    (0xA1, 0xE5),
    (0xA5, 0xE6),
    (0x5C, 0xE7),
];

fn now_ms() -> u64 {
    // SAFETY: plain call.
    unsafe { GetTickCount64() }
}

fn key_down(vk: u16) -> bool {
    // SAFETY: plain call.
    unsafe { GetAsyncKeyState(i32::from(vk)) < 0 }
}

pub fn install(table: Table, sink: HostSink) -> Result<Hook, String> {
    if ACTIVE.load().is_some() {
        return Err("a keyboard hook is already installed".into());
    }
    let mut matcher = HotkeyMatcher::new(table);
    for (vk, hid) in MODIFIERS {
        if key_down(vk) {
            matcher.seed_modifier(hid, true);
        }
    }
    let ctx = Arc::new(Ctx { matcher: Mutex::new(matcher), sink, last_callback_ms: AtomicU64::new(now_ms()) });
    ACTIVE.store(Some(ctx.clone()));
    let alive = Arc::new(AtomicBool::new(true));
    let alive_thread = alive.clone();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();
    let spawned = thread::Builder::new().name("desk-hook".into()).spawn(move || {
        // SAFETY: the hook procedure is a plain function; this thread runs the message loop the hook needs.
        unsafe {
            match SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_proc), None, 0) {
                Ok(hook) => {
                    let _ = ready_tx.send(Ok(GetCurrentThreadId()));
                    let mut msg = MSG::default();
                    loop {
                        let r = GetMessageW(&mut msg, None, 0, 0).0;
                        if r <= 0 {
                            break;
                        }
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    let _ = UnhookWindowsHookEx(hook);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("could not install the keyboard hook: {e}")));
                }
            }
        }
        alive_thread.store(false, Ordering::SeqCst);
    });
    let join = match spawned {
        Ok(j) => j,
        Err(e) => {
            ACTIVE.store(None);
            return Err(e.to_string());
        }
    };
    match ready_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(thread_id)) => Ok(Hook { thread_id, join, alive, ctx }),
        Ok(Err(e)) => {
            ACTIVE.store(None);
            let _ = join.join();
            Err(e)
        }
        Err(_) => {
            ACTIVE.store(None);
            Err("the keyboard hook thread did not start".into())
        }
    }
}

impl Hook {
    pub fn health(&self) -> HookHealth {
        if !self.alive.load(Ordering::SeqCst) {
            return HookHealth::Dropped;
        }
        let age = now_ms().saturating_sub(self.ctx.last_callback_ms.load(Ordering::SeqCst));
        // Only look for a held key when the hook has been quiet a while.
        let held = age > DROPPED_AFTER_MS && normal_key_held();
        if hook_looks_dropped(age, held) {
            HookHealth::Dropped
        } else {
            HookHealth::Healthy
        }
    }

    /// Stop the thread (its message loop gets WM_QUIT, then it unhooks) and wait for it.
    pub fn remove(self) {
        ACTIVE.store(None);
        if self.alive.load(Ordering::SeqCst) {
            // SAFETY: plain call to the hook thread's id.
            unsafe {
                let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        let _ = self.join.join();
    }

    /// Forget half-pressed state (a swallowed key whose release will never come).
    pub fn reset(&self) {
        if let Ok(mut m) = self.ctx.matcher.lock() {
            m.reset();
        }
    }
}

fn is_modifier_vk(vk: u16) -> bool {
    matches!(vk, 0x10..=0x12 | 0x5B | 0x5C | 0xA0..=0xA5)
}

fn normal_key_held() -> bool {
    (0x08u16..=0xFE).any(|vk| !is_modifier_vk(vk) && key_down(vk))
}

unsafe extern "system" fn key_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let Some(ctx) = ACTIVE.load_full() else {
        return CallNextHookEx(None, code, wparam, lparam);
    };
    ctx.last_callback_ms.store(now_ms(), Ordering::Relaxed);
    let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
    let message = wparam.0 as u32;
    let down = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
    let up = matches!(message, WM_KEYUP | WM_SYSKEYUP);
    let Some(hid) = vk_to_hid(info.vkCode as u16) else {
        return CallNextHookEx(None, code, wparam, lparam);
    };
    if !down && !up {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let injected = info.flags.0 & LLKHF_INJECTED.0 != 0;
    // Never wait for a lock inside the hook: if it is busy, the key goes through.
    let verdict = match ctx.matcher.try_lock() {
        Ok(mut m) => m.on_key(hid, down, injected),
        Err(_) => Verdict::Pass,
    };
    match verdict {
        Verdict::Pass => CallNextHookEx(None, code, wparam, lparam),
        Verdict::Swallow => LRESULT(1),
        Verdict::Fire(action) => {
            (ctx.sink)(HostEvent::Hotkey(action));
            LRESULT(1)
        }
    }
}
