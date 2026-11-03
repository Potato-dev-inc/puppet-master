//! Watches the local pointer for the hub: every move of the mouse or trackpad on this Mac is reported to a sink, so the
//! hub can see the pointer push through an edge the moment it happens (the old DevHop rule), without polling.
//!
//! A listen-only Quartz event tap on a thread of its own, with its own run loop. Listen-only means it can never drop,
//! delay or change an event: the pointer always moves as the person moves it. The callback only converts the event and
//! hands it to the sink, which must never block (the hub's sink just queues).
//!
//! Events this process posted itself (the injection half of `MacosRemote`, which moves the pointer for the other
//! computer) are skipped, so the hub never reacts to its own moves. Missing Accessibility or Input Monitoring is
//! reported as an error, and the hub then falls back to polling.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use arc_swap::ArcSwapOption;

use super::capture::ACCESS_NEEDED;
use super::ffi::{self, CFMachPortRef, CFTypeRef, CGEventRef, CGEventTapProxy};
use crate::desk::link::pointer_event::to_move;
use crate::desk::link::remote::PointerSink;

struct Ctx {
    sink: PointerSink,
    /// This process's id: events it posted carry it as their source process.
    pid: i64,
    tap: AtomicUsize,
    stop: AtomicBool,
}

static ACTIVE: ArcSwapOption<Ctx> = ArcSwapOption::const_empty();

/// A running watch. Dropping it without `stop` would leave the tap on; the owner always calls `stop`.
pub struct PointerWatch {
    join: JoinHandle<()>,
    ctx: Arc<Ctx>,
}

/// Starts the tap. Returns once the tap is on (or has failed), so an error means nothing is watched.
pub fn start(sink: PointerSink) -> Result<PointerWatch, String> {
    // SAFETY: plain call.
    if !unsafe { ffi::AXIsProcessTrusted() } {
        return Err(ACCESS_NEEDED.into());
    }
    if ACTIVE.load().is_some() {
        return Err("the pointer is already watched".into());
    }
    let ctx = Arc::new(Ctx {
        sink,
        // SAFETY: plain call.
        pid: i64::from(unsafe { ffi::getpid() }),
        tap: AtomicUsize::new(0),
        stop: AtomicBool::new(false),
    });
    ACTIVE.store(Some(ctx.clone()));
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let ctx_thread = ctx.clone();
    let spawned = thread::Builder::new().name("desk-pointer-watch".into()).spawn(move || run_tap(&ctx_thread, &ready_tx));
    let join = match spawned {
        Ok(j) => j,
        Err(e) => {
            ACTIVE.store(None);
            return Err(e.to_string());
        }
    };
    match ready_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(())) => Ok(PointerWatch { join, ctx }),
        Ok(Err(e)) => {
            ACTIVE.store(None);
            let _ = join.join();
            Err(e)
        }
        Err(_) => {
            ACTIVE.store(None);
            ctx.stop.store(true, Ordering::SeqCst);
            Err("the pointer watch did not start".into())
        }
    }
}

/// The watch thread: create the tap, put it on this thread's run loop, run the loop until `stop` asks it to leave,
/// then invalidate and release the tap here, on the thread that made it.
fn run_tap(ctx: &Ctx, ready: &mpsc::Sender<Result<(), String>>) {
    // SAFETY: the callback only reads `ACTIVE`, and the tap lives until it is invalidated below.
    unsafe {
        let tap = ffi::CGEventTapCreate(
            ffi::CG_SESSION_EVENT_TAP,
            ffi::CG_HEAD_INSERT_EVENT_TAP,
            ffi::CG_EVENT_TAP_OPTION_LISTEN_ONLY,
            ffi::MOVE_EVENTS_MASK,
            tap_proc,
            std::ptr::null_mut(),
        );
        if tap.is_null() {
            // A listen-only tap for moves is refused without Input Monitoring (or Accessibility): same plain words.
            let _ = ready.send(Err(ACCESS_NEEDED.into()));
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

impl PointerWatch {
    /// Ends the tap and waits for its thread to leave. After this no sink call can happen.
    pub fn stop(self) {
        ACTIVE.store(None);
        self.ctx.stop.store(true, Ordering::SeqCst);
        let _ = self.join.join();
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
    let at = ffi::CGEventGetLocation(event);
    let (dx, dy) = (field(event, ffi::CG_MOUSE_EVENT_DELTA_X), field(event, ffi::CG_MOUSE_EVENT_DELTA_Y));
    (ctx.sink)(to_move(at.x, at.y, dx, dy));
    // Listen-only: the event is never changed or dropped, whatever the sink does with it.
    event
}
