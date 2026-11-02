// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! The keyboard hook, on its own thread with its own run loop. A Quartz event tap sees each key press before the
//! apps do. Its callback makes the same decision as the Windows hook (the pure `HotkeyMatcher`) and never waits.
//! macOS turns a tap off when a callback is slow or secure input is on, so the callback turns it back on.
//!
//! A tap that can drop keys needs Accessibility access. Without it macOS will not create the tap, and `install`
//! says so, so the engine shows the person what to switch on.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use arc_swap::ArcSwapOption;
use desk_proto::mac_to_hid;

use super::ffi::{self, CFMachPortRef, CFTypeRef, CGEventRef, CGEventTapProxy};
use crate::desk::engine::hotkeys::{HotkeyMatcher, Table, Verdict};
use crate::desk::host::{hook_looks_dropped, HookHealth, HostEvent, HostSink, DROPPED_AFTER_MS};

const ACCESS_NEEDED: &str = "Puppet Master needs Accessibility access to read keys. Turn it on in System Settings > Privacy & Security > Accessibility, then restart Puppet Master.";
const TAP_REFUSED: &str =
    "macOS would not start the keyboard hook. Check Accessibility access in System Settings > Privacy & Security.";

struct Ctx {
    matcher: Mutex<HotkeyMatcher>,
    sink: HostSink,
    last_callback_ms: AtomicU64,
    /// The tap, as an address (raw pointers are not `Send`). Set by the hook thread before it reports ready.
    tap: AtomicUsize,
    /// Set by `remove`; the hook thread leaves its run loop once it sees this.
    stop: AtomicBool,
}

/// What the callback reads. A C callback cannot carry state, so the live install is here.
static ACTIVE: ArcSwapOption<Ctx> = ArcSwapOption::const_empty();
static START: OnceLock<Instant> = OnceLock::new();

pub struct Hook {
    join: JoinHandle<()>,
    alive: Arc<AtomicBool>,
    ctx: Arc<Ctx>,
}

/// Each modifier key's mac keycode and the bit that says it is down in the event flags (the device bits from
/// IOKit's NX_DEVICE*KEYMASK), so the left and right keys can be told apart. Caps lock and fn are left out: caps
/// lock's flag is a toggle that stays set while the lock is on, and `desk-proto` has no HID code for fn.
pub(super) const MODIFIER_BITS: [(u16, u64); 8] = [
    (0x37, 0x0000_0008), // left command
    (0x36, 0x0000_0010), // right command
    (0x38, 0x0000_0002), // left shift
    (0x3C, 0x0000_0004), // right shift
    (0x3B, 0x0000_0001), // left control
    (0x3E, 0x0000_2000), // right control
    (0x3A, 0x0000_0020), // left option
    (0x3D, 0x0000_0040), // right option
];

fn now_ms() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Whether a modifier key is down, from the event flags. `None` for keys that are not modifiers.
pub(super) fn modifier_down(keycode: u16, flags: u64) -> Option<bool> {
    MODIFIER_BITS
        .iter()
        .find(|(code, _)| *code == keycode)
        .map(|(_, bit)| flags & bit != 0)
}

pub(super) fn is_modifier(keycode: u16) -> bool {
    MODIFIER_BITS.iter().any(|(code, _)| *code == keycode)
}

fn normal_key_held() -> bool {
    (0u16..=0x7F).any(|code| {
        // SAFETY: plain call; reads the current state of one key.
        mac_to_hid(code).is_some()
            && !is_modifier(code)
            && unsafe { ffi::CGEventSourceKeyState(ffi::CG_EVENT_SOURCE_STATE_COMBINED_SESSION, code) }
    })
}

pub fn install(table: Table, sink: HostSink) -> Result<Hook, String> {
    // SAFETY: plain call.
    if !unsafe { ffi::AXIsProcessTrusted() } {
        return Err(ACCESS_NEEDED.into());
    }
    if ACTIVE.load().is_some() {
        return Err("a keyboard hook is already installed".into());
    }
    let mut matcher = HotkeyMatcher::new(table);
    // SAFETY: plain call.
    let flags = unsafe { ffi::CGEventSourceFlagsState(ffi::CG_EVENT_SOURCE_STATE_COMBINED_SESSION) };
    for (code, bit) in MODIFIER_BITS {
        if flags & bit != 0 {
            if let Some(hid) = mac_to_hid(code) {
                matcher.seed_modifier(hid, true);
            }
        }
    }
    let ctx = Arc::new(Ctx {
        matcher: Mutex::new(matcher),
        sink,
        last_callback_ms: AtomicU64::new(now_ms()),
        tap: AtomicUsize::new(0),
        stop: AtomicBool::new(false),
    });
    ACTIVE.store(Some(ctx.clone()));
    let alive = Arc::new(AtomicBool::new(true));
    let alive_thread = alive.clone();
    let ctx_thread = ctx.clone();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let spawned = thread::Builder::new().name("desk-hook".into()).spawn(move || {
        run_tap(&ctx_thread, &ready_tx);
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
        Ok(Ok(())) => Ok(Hook { join, alive, ctx }),
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

/// The hook thread: create the tap, put it on this thread's run loop, and run the loop until `remove` asks it to
/// stop. The tap and its source are released here, on the thread that made them.
fn run_tap(ctx: &Ctx, ready: &mpsc::Sender<Result<(), String>>) {
    // SAFETY: the callback only reads `ACTIVE`, and the tap lives until it is invalidated below.
    unsafe {
        let tap = ffi::CGEventTapCreate(
            ffi::CG_SESSION_EVENT_TAP,
            ffi::CG_HEAD_INSERT_EVENT_TAP,
            ffi::CG_EVENT_TAP_OPTION_DEFAULT,
            ffi::KEY_EVENTS_MASK,
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

    /// Stop the thread (it leaves its run loop within a quarter second, then unhooks) and wait for it.
    pub fn remove(self) {
        ACTIVE.store(None);
        self.ctx.stop.store(true, Ordering::SeqCst);
        let _ = self.join.join();
    }

    /// Forget half-pressed state (a swallowed key whose release will never come).
    pub fn reset(&self) {
        if let Ok(mut m) = self.ctx.matcher.lock() {
            m.reset();
        }
    }
}

/// The event-tap callback. It runs on the hook thread and never waits: a busy matcher lets the key through.
unsafe extern "C" fn tap_proc(_proxy: CGEventTapProxy, kind: u32, event: CGEventRef, _info: *mut c_void) -> CGEventRef {
    let Some(ctx) = ACTIVE.load_full() else {
        return event;
    };
    if kind == ffi::CG_EVENT_TAP_DISABLED_BY_TIMEOUT || kind == ffi::CG_EVENT_TAP_DISABLED_BY_USER_INPUT {
        // macOS switched the tap off (a slow callback, or secure input). Switch it back on.
        ffi::CGEventTapEnable(ctx.tap.load(Ordering::SeqCst) as CFMachPortRef, true);
        return event;
    }
    ctx.last_callback_ms.store(now_ms(), Ordering::Relaxed);
    // Keys this computer sent itself (the remote host's injection) are not the person typing a hotkey.
    if ffi::CGEventGetIntegerValueField(event, ffi::CG_EVENT_SOURCE_UNIX_PROCESS_ID) == i64::from(ffi::getpid()) {
        return event;
    }
    let keycode = ffi::CGEventGetIntegerValueField(event, ffi::CG_KEYBOARD_EVENT_KEYCODE) as u16;
    let Some(hid) = mac_to_hid(keycode) else {
        return event;
    };
    let down = match kind {
        ffi::CG_EVENT_KEY_DOWN => true,
        ffi::CG_EVENT_KEY_UP => false,
        ffi::CG_EVENT_FLAGS_CHANGED => {
            let flags = ffi::CGEventGetFlags(event);
            match modifier_down(keycode, flags) {
                Some(down) => down,
                None => return event,
            }
        }
        _ => return event,
    };
    let verdict = match ctx.matcher.try_lock() {
        Ok(mut m) => m.on_key(hid, down, false),
        Err(_) => Verdict::Pass,
    };
    match verdict {
        Verdict::Pass => event,
        Verdict::Swallow => std::ptr::null_mut(),
        Verdict::Fire(action) => {
            (ctx.sink)(HostEvent::Hotkey(action));
            std::ptr::null_mut()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_modifier_key_has_a_hid_code() {
        for (code, _) in MODIFIER_BITS {
            assert!(mac_to_hid(code).is_some(), "keycode {code:#x} has no HID code");
        }
    }

    #[test]
    fn left_and_right_modifiers_read_their_own_bits() {
        assert_eq!(modifier_down(0x37, 0x08), Some(true));
        assert_eq!(modifier_down(0x36, 0x08), Some(false));
        assert_eq!(modifier_down(0x38, 0), Some(false));
        assert_eq!(modifier_down(0x00, u64::MAX), None);
    }
}
