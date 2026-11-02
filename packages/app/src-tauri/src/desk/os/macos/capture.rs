// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! Capture while the pointer is on another computer: an active Quartz event tap on its own thread that swallows
//! what the person does and reports it. Installed only between `start` and `stop`.
//!
//! Unlike the Windows side the pointer is not parked: a swallowed move never reaches the screen, so the pointer
//! stays where it left, and the distance moved comes from the hardware delta in the event itself.
//!
//! A key that was already down when the pointer left, and whose release was swallowed here, is reported by `stop`
//! so the caller can release it, and the local keyboard is never left with a key stuck down. Callbacks never wait.
//! Events this process posted itself (the injection half of `MacosRemote`) pass through untouched.

use std::collections::BTreeSet;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use arc_swap::ArcSwapOption;
use desk_proto::mac_to_hid;

use super::ffi::{self, CFMachPortRef, CFTypeRef, CGEventRef, CGEventTapProxy};
use super::hook::modifier_down;
use crate::desk::link::remote::{Capture, CaptureSink};

/// A jump bigger than this in one event is not a mouse move; it is dropped.
const MAX_STEP: i64 = 2000;
/// Wheel units the other side uses (a Windows notch is 120) per scrolled pixel here. `remote.rs` divides by it.
pub(super) const WHEEL_UNITS_PER_PIXEL: i32 = 4;

pub(super) const ACCESS_NEEDED: &str = "Puppet Master needs Accessibility access to control this Mac. Turn it on in System Settings > Privacy & Security > Accessibility, then restart Puppet Master. Started from a terminal (npm run tauri dev)? Turn it on for that terminal app instead.";
const TAP_REFUSED: &str = "macOS would not let Puppet Master take the mouse and keyboard. Check Accessibility access in System Settings > Privacy & Security.";

struct Ctx {
    sink: CaptureSink,
    pid: i64,
    /// Mac key codes down when the pointer left.
    down_at_start: BTreeSet<u16>,
    /// Of those, the ones whose release was swallowed.
    swallowed_ups: Mutex<BTreeSet<u16>>,
    tap: AtomicUsize,
    stop: AtomicBool,
}

static ACTIVE: ArcSwapOption<Ctx> = ArcSwapOption::const_empty();

pub struct CaptureHook {
    join: JoinHandle<()>,
    ctx: Arc<Ctx>,
}

fn key_down(code: u16) -> bool {
    // SAFETY: plain call; reads the current state of one key.
    unsafe { ffi::CGEventSourceKeyState(ffi::CG_EVENT_SOURCE_STATE_COMBINED_SESSION, code) }
}

pub fn start(sink: CaptureSink) -> Result<CaptureHook, String> {
    // SAFETY: plain call.
    if !unsafe { ffi::AXIsProcessTrusted() } {
        return Err(ACCESS_NEEDED.into());
    }
    if ACTIVE.load().is_some() {
        return Err("the mouse is already taken".into());
    }
    let down_at_start: BTreeSet<u16> = (0u16..=0x7F).filter(|c| mac_to_hid(*c).is_some() && key_down(*c)).collect();
    let ctx = Arc::new(Ctx {
        sink,
        // SAFETY: plain call.
        pid: i64::from(unsafe { ffi::getpid() }),
        down_at_start,
        swallowed_ups: Mutex::new(BTreeSet::new()),
        tap: AtomicUsize::new(0),
        stop: AtomicBool::new(false),
    });
    ACTIVE.store(Some(ctx.clone()));
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let ctx_thread = ctx.clone();
    let spawned = thread::Builder::new().name("desk-capture".into()).spawn(move || run_tap(&ctx_thread, &ready_tx));
    let join = match spawned {
        Ok(j) => j,
        Err(e) => {
            ACTIVE.store(None);
            return Err(e.to_string());
        }
    };
    match ready_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(())) => Ok(CaptureHook { join, ctx }),
        Ok(Err(e)) => {
            ACTIVE.store(None);
            let _ = join.join();
            Err(e)
        }
        Err(_) => {
            ACTIVE.store(None);
            ctx.stop.store(true, Ordering::SeqCst);
            Err("the capture thread did not start".into())
        }
    }
}

/// The capture thread: create the tap, put it on this thread's run loop, run the loop until `stop` asks it to leave.
fn run_tap(ctx: &Ctx, ready: &mpsc::Sender<Result<(), String>>) {
    // SAFETY: the callback only reads `ACTIVE`, and the tap lives until it is invalidated below.
    unsafe {
        let tap = ffi::CGEventTapCreate(
            ffi::CG_SESSION_EVENT_TAP,
            ffi::CG_HEAD_INSERT_EVENT_TAP,
            ffi::CG_EVENT_TAP_OPTION_DEFAULT,
            ffi::KEY_EVENTS_MASK | ffi::POINTER_EVENTS_MASK,
            tap_proc,
            std::ptr::null_mut(),
        );
        if tap.is_null() {
            let _ = ready.send(Err(TAP_REFUSED.into()));
            return;
        }
        let source = ffi::CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
        ffi::CFRunLoopAddSource(ffi::CFRunLoopGetCurrent(), source, ffi::kCFRunLoopCommonModes);
        ctx.tap.store(tap as usize, Ordering::SeqCst);
        ffi::CGEventTapEnable(tap, true);
        let _ = ready.send(Ok(()));
        while !ctx.stop.load(Ordering::SeqCst) {
            ffi::CFRunLoopRunInMode(ffi::kCFRunLoopDefaultMode, 0.25, 0);
        }
        ffi::CFMachPortInvalidate(tap);
        ffi::CFRelease(source as CFTypeRef);
        ffi::CFRelease(tap as CFTypeRef);
    }
}

impl CaptureHook {
    /// Ends the tap and waits for the thread. Returns the Mac key codes that were down when the pointer left and
    /// were let go meanwhile: their release never reached this computer and must be sent now.
    pub fn stop(self) -> Vec<u16> {
        ACTIVE.store(None);
        self.ctx.stop.store(true, Ordering::SeqCst);
        let _ = self.join.join();
        self.ctx.swallowed_ups.lock().map(|s| s.iter().copied().collect()).unwrap_or_default()
    }
}

unsafe fn field(event: CGEventRef, f: u32) -> i64 {
    ffi::CGEventGetIntegerValueField(event, f)
}

unsafe extern "C" fn tap_proc(_proxy: CGEventTapProxy, kind: u32, event: CGEventRef, _info: *mut c_void) -> CGEventRef {
    let Some(ctx) = ACTIVE.load_full() else {
        return event;
    };
    if kind == ffi::CG_EVENT_TAP_DISABLED_BY_TIMEOUT || kind == ffi::CG_EVENT_TAP_DISABLED_BY_USER_INPUT {
        // macOS switched the tap off (a slow callback, or secure input). Switch it back on.
        ffi::CGEventTapEnable(ctx.tap.load(Ordering::SeqCst) as CFMachPortRef, true);
        return event;
    }
    if field(event, ffi::CG_EVENT_SOURCE_UNIX_PROCESS_ID) == ctx.pid {
        return event;
    }
    let reported = match kind {
        ffi::CG_EVENT_MOUSE_MOVED
        | ffi::CG_EVENT_LEFT_MOUSE_DRAGGED
        | ffi::CG_EVENT_RIGHT_MOUSE_DRAGGED
        | ffi::CG_EVENT_OTHER_MOUSE_DRAGGED => {
            let (dx, dy) = (field(event, ffi::CG_MOUSE_EVENT_DELTA_X), field(event, ffi::CG_MOUSE_EVENT_DELTA_Y));
            if (dx != 0 || dy != 0) && dx.abs() < MAX_STEP && dy.abs() < MAX_STEP {
                Some(Capture::Motion { dx: dx as i32, dy: dy as i32 })
            } else {
                None
            }
        }
        ffi::CG_EVENT_LEFT_MOUSE_DOWN => Some(Capture::Button { button: 0, down: true }),
        ffi::CG_EVENT_LEFT_MOUSE_UP => Some(Capture::Button { button: 0, down: false }),
        ffi::CG_EVENT_RIGHT_MOUSE_DOWN => Some(Capture::Button { button: 1, down: true }),
        ffi::CG_EVENT_RIGHT_MOUSE_UP => Some(Capture::Button { button: 1, down: false }),
        ffi::CG_EVENT_OTHER_MOUSE_DOWN => Some(Capture::Button { button: 2, down: true }),
        ffi::CG_EVENT_OTHER_MOUSE_UP => Some(Capture::Button { button: 2, down: false }),
        ffi::CG_EVENT_SCROLL_WHEEL => {
            let dy = field(event, ffi::CG_SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1) as i32;
            // Positive is left on a Mac and right on Windows.
            let dx = -(field(event, ffi::CG_SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_2) as i32);
            Some(Capture::Wheel {
                dx: dx.saturating_mul(WHEEL_UNITS_PER_PIXEL),
                dy: dy.saturating_mul(WHEEL_UNITS_PER_PIXEL),
            })
        }
        ffi::CG_EVENT_KEY_DOWN | ffi::CG_EVENT_KEY_UP | ffi::CG_EVENT_FLAGS_CHANGED => {
            let code = field(event, ffi::CG_KEYBOARD_EVENT_KEYCODE) as u16;
            let down = match kind {
                ffi::CG_EVENT_KEY_DOWN => Some(true),
                ffi::CG_EVENT_KEY_UP => Some(false),
                _ => modifier_down(code, ffi::CGEventGetFlags(event)),
            };
            match down {
                Some(down) => {
                    if !down && ctx.down_at_start.contains(&code) {
                        if let Ok(mut s) = ctx.swallowed_ups.try_lock() {
                            s.insert(code);
                        }
                    }
                    mac_to_hid(code).map(|hid| Capture::Key { hid, down })
                }
                None => None,
            }
        }
        _ => None,
    };
    if let Some(e) = reported {
        (ctx.sink)(e);
    }
    // Everything the person does stays off this computer while the pointer is away.
    std::ptr::null_mut()
}
