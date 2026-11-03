// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! Capture while the pointer is on another computer: low-level mouse and keyboard hooks that swallow what the person
//! does and report it, on a thread of their own with their own message loop. Installed only between `start` and
//! `stop`, never while the pointer is home.
//!
//! The pointer is held still at the park point near the edge it left by; motion is reported as the distance the
//! hardware moved it from there. The PC pointer is also clipped to a box around the park point and hidden after the
//! first forwarded motion (`cursor_lock.rs`, DevHop's behaviour). Clip and hide are undone on every exit: stop, the
//! thread's end, the watchdog and the panic hook.
//!
//! Fixed against DevHop: a key that was already down when the pointer left, and whose release was swallowed here,
//! is released again on `stop`, so the local keyboard is never left with a key stuck down. Callbacks never wait: a
//! busy lock lets the event through, and the sink only queues.
//!
//! Two modes share this file and never mix. Pass-through (the watch, `start_watch`): one low-level mouse hook reports
//! every local move and lets it through. Remote (`start`): the mouse and keyboard are swallowed and the pointer is
//! parked. `REMOTE_MODE` says which one is on; the watch reports nothing while remote mode is on, and starts its
//! deltas afresh after every change of mode (`REMOTE_EPOCH`), so the first local move after a release is never a
//! jump from the park point.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use arc_swap::ArcSwapOption;
use desk_proto::vk_to_hid;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, KillTimer, PostThreadMessageW, SetTimer, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_QUIT, WM_RBUTTONDOWN,
    WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER,
};

use super::cursor_lock;
use super::dpi::DpiScope;
use crate::desk::link::remote::{Capture, CaptureSink, PointerMove, PointerSink};

/// How often the capture thread checks that the pointer is not left clipped or hidden without remote mode.
const WATCHDOG_MS: u32 = 250;

const LLMHF_INJECTED: u32 = 1;
/// Remote-mode motion in the first milliseconds after the pointer is parked is not forwarded: it is the park warp and
/// motion queued before it (DevHop's REMOTE_SETTLE_MS).
const REMOTE_SETTLE_MS: u64 = 10;
/// A remote-mode position may be this far beyond the park span and still count as real motion (DevHop's slack).
const REMOTE_MOTION_SLACK: i32 = 2;

/// Whether a mouse move at `(x, y)` is real hardware motion while the pointer is parked at `(park_x, park_y)`: not in
/// the settle window, and not farther than the span (plus slack) from the park point. A position beyond that was
/// queued before the pointer was parked, or is the warp itself, and is dropped.
pub(super) fn should_forward_remote_motion(
    x: i32,
    y: i32,
    park_x: i32,
    park_y: i32,
    span: i32,
    ms_since_remote_enabled: Option<u64>,
) -> bool {
    if ms_since_remote_enabled.is_some_and(|ms| ms < REMOTE_SETTLE_MS) {
        return false;
    }
    let limit = span + REMOTE_MOTION_SLACK;
    (x - park_x).abs() <= limit && (y - park_y).abs() <= limit
}

/// The delta a remote-mode mouse move at `pt` reports: its distance from the park point. Nothing when there is no
/// distance (the warp back to the park point) or the move is not real motion.
pub(super) fn remote_motion_delta(pt: (i32, i32), park: (i32, i32), span: i32, ms_since_remote_enabled: Option<u64>) -> Option<(i32, i32)> {
    if !should_forward_remote_motion(pt.0, pt.1, park.0, park.1, span, ms_since_remote_enabled) {
        return None;
    }
    let (dx, dy) = (pt.0 - park.0, pt.1 - park.1);
    (dx != 0 || dy != 0).then_some((dx, dy))
}

struct Ctx {
    sink: CaptureSink,
    park: (i32, i32),
    /// The park span: the clip box is this far each way from the park point.
    span: i32,
    /// When the pointer was parked; the settle window counts from here.
    parked_at: Instant,
    /// Virtual keys down when the pointer left.
    down_at_start: BTreeSet<u16>,
    /// Of those, the ones whose release was swallowed.
    swallowed_ups: Mutex<BTreeSet<u16>>,
}

static ACTIVE: ArcSwapOption<Ctx> = ArcSwapOption::const_empty();

/// True from the start of a capture until its stop: the mouse is parked and swallowed.
static REMOTE_MODE: AtomicBool = AtomicBool::new(false);
/// Counts every change of remote mode. The watch starts its deltas afresh when it changes.
static REMOTE_EPOCH: AtomicU32 = AtomicU32::new(0);

fn set_remote_mode(on: bool) {
    REMOTE_MODE.store(on, Ordering::SeqCst);
    REMOTE_EPOCH.fetch_add(1, Ordering::SeqCst);
}

pub struct CaptureHook {
    thread_id: u32,
    join: JoinHandle<()>,
    ctx: Arc<Ctx>,
}

fn key_down(vk: u16) -> bool {
    // SAFETY: plain call.
    unsafe { GetAsyncKeyState(i32::from(vk)) < 0 }
}

/// Starts remote mode: the hooks are installed and the mouse is swallowed and parked from now on. Remote mode is on
/// before the caller warps the pointer to `park`, so the warp itself is already inside the hook.
pub fn start(sink: CaptureSink, park: (i32, i32), span: i32) -> Result<CaptureHook, String> {
    if ACTIVE.load().is_some() {
        return Err("the mouse is already taken".into());
    }
    set_remote_mode(true);
    let result = start_hooks(sink, park, span);
    if result.is_err() {
        set_remote_mode(false);
    }
    result
}

fn start_hooks(sink: CaptureSink, park: (i32, i32), span: i32) -> Result<CaptureHook, String> {
    let down_at_start: BTreeSet<u16> = (0x08u16..=0xFE).filter(|vk| key_down(*vk)).collect();
    let ctx = Arc::new(Ctx { sink, park, span, parked_at: Instant::now(), down_at_start, swallowed_ups: Mutex::new(BTreeSet::new()) });
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
                    let timer = SetTimer(None, 0, WATCHDOG_MS, None);
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                        if msg.message == WM_TIMER {
                            watchdog();
                            continue;
                        }
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    if timer != 0 {
                        let _ = KillTimer(None, timer);
                    }
                    // However the loop ended, the pointer comes back: clip and hide go, with the hooks.
                    cursor_lock::release();
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
        set_remote_mode(false);
        ACTIVE.store(None);
        // SAFETY: plain call to the capture thread's id.
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        let _ = self.join.join();
        self.ctx.swallowed_ups.lock().map(|s| s.iter().copied().collect()).unwrap_or_default()
    }
}

/// The pass-through watch's state: the last point it reported, and the remote epoch that point belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WatchTrack {
    last: Option<(i32, i32)>,
    epoch: u32,
}

impl WatchTrack {
    const FRESH: Self = Self { last: None, epoch: 0 };
}

/// One non-injected mouse move at `pt`, seen by the watch. Returns the move to report (none while remote mode is on)
/// and the new bookkeeping. The first move after a start or after any change of mode reports a delta of zero.
pub(super) fn watch_step(track: WatchTrack, epoch: u32, remote: bool, pt: (i32, i32)) -> (Option<PointerMove>, WatchTrack) {
    if remote {
        return (None, WatchTrack { last: None, epoch });
    }
    let last = if track.epoch == epoch { track.last } else { None };
    let (dx, dy) = match last {
        Some((x, y)) => (pt.0 - x, pt.1 - y),
        None => (0, 0),
    };
    (Some(PointerMove { x: pt.0, y: pt.1, dx, dy }), WatchTrack { last: Some(pt), epoch })
}

struct WatchCtx {
    sink: PointerSink,
}

static WATCH: ArcSwapOption<WatchCtx> = ArcSwapOption::const_empty();
/// Only the watch's own hook thread reads or writes this, from its callback (try_lock) or from `start_watch`.
static WATCH_TRACK: Mutex<WatchTrack> = Mutex::new(WatchTrack::FRESH);

/// A running pass-through watch: its own thread and its own message loop.
pub struct WatchHook {
    thread_id: u32,
    join: JoinHandle<()>,
}

pub fn start_watch(sink: PointerSink) -> Result<WatchHook, String> {
    if WATCH.load().is_some() {
        return Err("the pointer is already watched".into());
    }
    *WATCH_TRACK.lock().unwrap_or_else(|p| p.into_inner()) = WatchTrack::FRESH;
    WATCH.store(Some(Arc::new(WatchCtx { sink })));
    let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();
    let spawned = thread::Builder::new().name("desk-watch".into()).spawn(move || {
        let _dpi = DpiScope::new();
        // SAFETY: the hook procedure is a plain function; this thread runs the message loop it needs.
        unsafe {
            match SetWindowsHookExW(WH_MOUSE_LL, Some(watch_proc), None, 0) {
                Ok(hook) => {
                    let _ = ready_tx.send(Ok(GetCurrentThreadId()));
                    pump_messages();
                    let _ = UnhookWindowsHookEx(hook);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("could not install the pointer watch: {e}")));
                }
            }
        }
    });
    let join = match spawned {
        Ok(j) => j,
        Err(e) => {
            WATCH.store(None);
            return Err(e.to_string());
        }
    };
    match ready_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(thread_id)) => Ok(WatchHook { thread_id, join }),
        Ok(Err(e)) => {
            WATCH.store(None);
            let _ = join.join();
            Err(e)
        }
        Err(_) => {
            WATCH.store(None);
            Err("the pointer watch did not start".into())
        }
    }
}

impl WatchHook {
    /// Removes the hook and waits for its thread. Reports nothing after it returns.
    pub fn stop(self) {
        WATCH.store(None);
        // SAFETY: plain call to the watch thread's id.
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        let _ = self.join.join();
    }
}

fn pump_messages() {
    let mut msg = MSG::default();
    // SAFETY: a plain message loop on the thread that installed the hook.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// The capture thread's watchdog: remote mode is off but the pointer is still clipped or hidden, so give it back. Runs
/// on the capture thread between events, never inside a hook callback.
fn watchdog() {
    if !REMOTE_MODE.load(Ordering::SeqCst) && cursor_lock::is_engaged() {
        cursor_lock::release();
    }
}

/// Called from the watch callback: reports the move (if any) and lets nothing else happen. Never waits.
fn report_local_move(pt: (i32, i32)) {
    let Some(ctx) = WATCH.load_full() else {
        return;
    };
    let Ok(mut track) = WATCH_TRACK.try_lock() else {
        return;
    };
    let (next, now) = watch_step(*track, REMOTE_EPOCH.load(Ordering::SeqCst), REMOTE_MODE.load(Ordering::SeqCst), pt);
    *track = now;
    drop(track);
    if let Some(m) = next {
        (ctx.sink)(m);
    }
}

unsafe extern "system" fn watch_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
    if wparam.0 as u32 == WM_MOUSEMOVE && info.flags & LLMHF_INJECTED == 0 {
        report_local_move((info.pt.x, info.pt.y));
    }
    // The pass-through watch never swallows: the person's pointer moves as usual.
    CallNextHookEx(None, code, wparam, lparam)
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
            let since = ctx.parked_at.elapsed().as_millis() as u64;
            remote_motion_delta((info.pt.x, info.pt.y), ctx.park, ctx.span, Some(since)).map(|(dx, dy)| Capture::Motion { dx, dy })
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
        let motion = matches!(e, Capture::Motion { .. });
        (ctx.sink)(e);
        // The PC pointer is hidden only once a remote motion has really gone out: a crossing that never moves leaves
        // it visible.
        if motion {
            cursor_lock::hide_after_first_motion();
        }
    }
    // Back to the park point after every mouse event, so the next position is again the distance moved since this
    // one. Swallowing the event does not keep the position from drifting: without this the distances grow until
    // they hit the edge of the screen and the other computer's pointer is thrown into a corner.
    let _ = super::cursor::warp(ctx.park.0, ctx.park.1);
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

#[cfg(test)]
mod tests {
    use super::*;

    const SPAN: i32 = crate::desk::link::park::PARK_SPAN;

    #[test]
    fn stale_pre_warp_position_is_dropped() {
        assert!(!should_forward_remote_motion(100, 540, 1760, 540, SPAN, Some(50)));
    }

    #[test]
    fn normal_delta_inside_clip_is_kept() {
        assert!(should_forward_remote_motion(1761, 541, 1760, 540, SPAN, Some(50)));
    }

    #[test]
    fn settling_window_drops_motion() {
        assert!(!should_forward_remote_motion(1760, 540, 1760, 540, SPAN, Some(5)));
    }

    #[test]
    fn boundary_at_span_plus_slack() {
        let park = (1000, 500);
        let limit = SPAN + REMOTE_MOTION_SLACK;
        assert!(should_forward_remote_motion(park.0 + limit, park.1, park.0, park.1, SPAN, Some(20)));
        assert!(!should_forward_remote_motion(park.0 + limit + 1, park.1, park.0, park.1, SPAN, Some(20)));
    }

    #[test]
    fn custom_span_limits_motion() {
        assert!(should_forward_remote_motion(1881, 300, 1880, 300, 40, Some(20)));
        assert!(should_forward_remote_motion(1922, 300, 1880, 300, 40, Some(20)));
        assert!(!should_forward_remote_motion(1923, 300, 1880, 300, 40, Some(20)));
    }

    #[test]
    fn motion_is_the_distance_from_the_park_point() {
        assert_eq!(remote_motion_delta((1003, 497), (1000, 500), SPAN, Some(50)), Some((3, -3)));
    }

    #[test]
    fn a_zero_delta_is_not_forwarded() {
        assert_eq!(remote_motion_delta((1000, 500), (1000, 500), SPAN, Some(50)), None);
    }

    #[test]
    fn nothing_in_the_settle_window_is_forwarded() {
        assert_eq!(remote_motion_delta((1003, 497), (1000, 500), SPAN, Some(9)), None);
    }

    fn fresh() -> WatchTrack {
        WatchTrack::FRESH
    }

    #[test]
    fn the_first_local_move_reports_no_delta() {
        let (m, track) = watch_step(fresh(), 0, false, (500, 300));
        assert_eq!(m, Some(PointerMove { x: 500, y: 300, dx: 0, dy: 0 }));
        assert_eq!(track.last, Some((500, 300)));
    }

    #[test]
    fn later_moves_report_the_distance_from_the_last_point() {
        let (_, t1) = watch_step(fresh(), 0, false, (500, 300));
        let (m, t2) = watch_step(t1, 0, false, (497, 310));
        assert_eq!(m, Some(PointerMove { x: 497, y: 310, dx: -3, dy: 10 }));
        let (m, _) = watch_step(t2, 0, false, (497, 310));
        assert_eq!(m, Some(PointerMove { x: 497, y: 310, dx: 0, dy: 0 }));
    }

    #[test]
    fn nothing_is_reported_while_remote_mode_is_on() {
        let (_, t1) = watch_step(fresh(), 0, false, (500, 300));
        let (m, t2) = watch_step(t1, 0, true, (1760, 540));
        assert_eq!(m, None);
        assert_eq!(t2.last, None);
    }

    #[test]
    fn the_first_move_after_a_release_starts_fresh_and_never_jumps() {
        // Local pointer last seen at (500, 300). A capture starts (epoch 1) and ends (epoch 2): the pointer is
        // back at a far-away point. That first move must report no delta, not a jump from the park point or from
        // the stale point before the capture.
        let (_, t1) = watch_step(fresh(), 0, false, (500, 300));
        let (_, t2) = watch_step(t1, 1, true, (1760, 540));
        let (m, _) = watch_step(t2, 2, false, (1700, 540));
        assert_eq!(m, Some(PointerMove { x: 1700, y: 540, dx: 0, dy: 0 }));
    }

    #[test]
    fn a_stale_track_from_an_earlier_epoch_is_never_used() {
        let stale = WatchTrack { last: Some((10, 10)), epoch: 1 };
        let (m, _) = watch_step(stale, 2, false, (900, 900));
        assert_eq!(m, Some(PointerMove { x: 900, y: 900, dx: 0, dy: 0 }));
    }
}
