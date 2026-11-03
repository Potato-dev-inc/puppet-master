// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! The PC pointer while another computer holds it: clipped to a box `2 * span` wide around the park point, and hidden
//! once the first remote motion has been forwarded. Release undoes both, and is safe to call any number of times.
//!
//! This is the only file in the desk that calls `ClipCursor` or `ShowCursor` (a guard test says so). A crash must not
//! leave the person without a visible pointer, so the release runs from `stop_capture`, from the capture thread's
//! exit, from the capture thread's watchdog (remote mode off but still engaged), from a panic hook (chained to the
//! previous hook, and it acts only while engaged) and from `Drop`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, Once, TryLockError};

use windows::Win32::Foundation::RECT;
use windows::Win32::UI::WindowsAndMessaging::{ClipCursor, ShowCursor};

/// The most ShowCursor calls made in one hide or one show, as DevHop capped them.
pub const MAX_SHOW_CALLS: u32 = 16;

/// The two system calls, behind a trait so the counting can be tested without touching the real pointer.
pub trait CursorOps {
    /// `Some((left, top, right, bottom))` with right and bottom exclusive, or `None` to release the clip.
    fn clip(&mut self, rect: Option<(i32, i32, i32, i32)>);
    /// ShowCursor(show). Returns the display counter after the call: the pointer is shown while it is 0 or more.
    fn show(&mut self, show: bool) -> i32;
}

/// The clip box for a park point: `span` each way from it, exclusive on the far side, so `2 * span` in all.
pub fn clip_box(park: (i32, i32), span: i32) -> (i32, i32, i32, i32) {
    (park.0 - span, park.1 - span, park.0 + span, park.1 + span)
}

/// The clip and the hide for one capture. `engage` clips; `hide` (after the first forwarded motion) hides once;
/// `release` clips nothing and shows the pointer again only if this lock hid it.
pub struct CursorLock<O: CursorOps> {
    ops: O,
    engaged: bool,
    hidden: bool,
}

impl<O: CursorOps> CursorLock<O> {
    pub const fn new(ops: O) -> Self {
        Self { ops, engaged: false, hidden: false }
    }

    pub fn is_engaged(&self) -> bool {
        self.engaged
    }

    /// Clips the pointer to the box around `park`. Does nothing when already engaged.
    pub fn engage(&mut self, park: (i32, i32), span: i32) {
        if self.engaged {
            return;
        }
        self.ops.clip(Some(clip_box(park, span)));
        self.engaged = true;
    }

    /// Hides the pointer, once, while engaged. Returns whether it hid the pointer just now.
    pub fn hide(&mut self) -> bool {
        if !self.engaged || self.hidden {
            return false;
        }
        // ShowCursor(FALSE) until the counter is below zero (hidden), at most MAX_SHOW_CALLS times.
        let mut calls = 0;
        loop {
            let counter = self.ops.show(false);
            calls += 1;
            if counter < 0 || calls >= MAX_SHOW_CALLS {
                break;
            }
        }
        self.hidden = true;
        true
    }

    /// Undoes everything: the clip always, and the hide only if this lock made it. Safe to call twice.
    pub fn release(&mut self) {
        self.ops.clip(None);
        if self.hidden {
            // ShowCursor(TRUE) until the counter is back to 0 or more, at most MAX_SHOW_CALLS times. One hide, one show.
            let mut calls = 0;
            loop {
                let counter = self.ops.show(true);
                calls += 1;
                if counter >= 0 || calls >= MAX_SHOW_CALLS {
                    break;
                }
            }
        }
        self.hidden = false;
        self.engaged = false;
    }
}

impl<O: CursorOps> Drop for CursorLock<O> {
    fn drop(&mut self) {
        self.release();
    }
}

/// The real system calls.
pub struct WinCursor;

impl CursorOps for WinCursor {
    fn clip(&mut self, rect: Option<(i32, i32, i32, i32)>) {
        // SAFETY: the rectangle lives for the call; ClipCursor copies it.
        unsafe {
            match rect {
                Some((left, top, right, bottom)) => {
                    let r = RECT { left, top, right, bottom };
                    let _ = ClipCursor(Some(&r as *const RECT));
                }
                None => {
                    let _ = ClipCursor(None);
                }
            }
        }
    }

    fn show(&mut self, show: bool) -> i32 {
        // SAFETY: plain call.
        unsafe { ShowCursor(show) }
    }
}

static LOCK: Mutex<CursorLock<WinCursor>> = Mutex::new(CursorLock::new(WinCursor));
/// Mirrors `engaged` so the hook and the panic hook can look without waiting for the lock.
static ENGAGED: AtomicBool = AtomicBool::new(false);
static PANIC_HOOK: Once = Once::new();

fn lock() -> MutexGuard<'static, CursorLock<WinCursor>> {
    LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

/// Clips the PC pointer around the park point. Installs the panic hook the first time.
pub fn engage(park: (i32, i32), span: i32) {
    install_panic_hook();
    let mut l = lock();
    l.engage(park, span);
    ENGAGED.store(l.is_engaged(), Ordering::SeqCst);
}

/// Hides the PC pointer. Called from the hook on every forwarded motion, so a capture that never moves stays visible.
/// Never waits: if the lock is busy it tries again on the next motion.
pub fn hide_after_first_motion() {
    if !ENGAGED.load(Ordering::SeqCst) {
        return;
    }
    if let Ok(mut l) = LOCK.try_lock() {
        l.hide();
    }
}

/// Gives the pointer back. Safe to call any number of times, and from any thread.
pub fn release() {
    let mut l = lock();
    l.release();
    ENGAGED.store(false, Ordering::SeqCst);
}

/// Whether the clip (and maybe the hide) is still on. Read by the capture thread's watchdog.
pub fn is_engaged() -> bool {
    ENGAGED.load(Ordering::SeqCst)
}

fn install_panic_hook() {
    PANIC_HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            panic_release();
            previous(info);
        }));
    });
}

/// From the panic hook: gives the pointer back whatever state the lock is in. Acts only while engaged. If the lock
/// is held by the thread that panicked, it does not wait; it makes the pointer visible and free directly.
fn panic_release() {
    if !ENGAGED.swap(false, Ordering::SeqCst) {
        return;
    }
    match LOCK.try_lock() {
        Ok(mut l) => l.release(),
        Err(TryLockError::Poisoned(p)) => p.into_inner().release(),
        Err(TryLockError::WouldBlock) => {
            let mut ops = WinCursor;
            ops.clip(None);
            let mut calls = 0;
            loop {
                let counter = ops.show(true);
                calls += 1;
                if counter >= 0 || calls >= MAX_SHOW_CALLS {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        Clip(Option<(i32, i32, i32, i32)>),
        Show(bool),
    }

    /// A pretend pointer: records every call and keeps a ShowCursor counter that starts at 0, like the system's.
    struct Fake {
        log: Rc<RefCell<Vec<Call>>>,
        counter: i32,
    }

    impl CursorOps for Fake {
        fn clip(&mut self, rect: Option<(i32, i32, i32, i32)>) {
            self.log.borrow_mut().push(Call::Clip(rect));
        }
        fn show(&mut self, show: bool) -> i32 {
            self.log.borrow_mut().push(Call::Show(show));
            self.counter += if show { 1 } else { -1 };
            self.counter
        }
    }

    fn fake() -> (CursorLock<Fake>, Rc<RefCell<Vec<Call>>>) {
        let log = Rc::new(RefCell::new(Vec::new()));
        (CursorLock::new(Fake { log: log.clone(), counter: 0 }), log)
    }

    #[test]
    fn the_clip_box_is_two_spans_wide_around_the_park_point() {
        assert_eq!(clip_box((1760, 540), 160), (1600, 380, 1920, 700));
    }

    #[test]
    fn engage_clips_and_hide_hides_once_and_release_undoes_both() {
        let (mut lock, log) = fake();
        lock.engage((1760, 540), 160);
        assert!(lock.hide());
        lock.release();
        assert_eq!(
            *log.borrow(),
            vec![Call::Clip(Some((1600, 380, 1920, 700))), Call::Show(false), Call::Clip(None), Call::Show(true)]
        );
        assert!(!lock.is_engaged());
    }

    #[test]
    fn engage_twice_clips_once() {
        let (mut lock, log) = fake();
        lock.engage((100, 100), 10);
        lock.engage((900, 900), 10);
        assert_eq!(log.borrow().iter().filter(|c| matches!(c, Call::Clip(Some(_)))).count(), 1);
    }

    #[test]
    fn hide_twice_hides_once_so_show_and_hide_stay_balanced() {
        let (mut lock, log) = fake();
        lock.engage((100, 100), 10);
        assert!(lock.hide());
        assert!(!lock.hide());
        lock.release();
        let hides = log.borrow().iter().filter(|c| **c == Call::Show(false)).count();
        let shows = log.borrow().iter().filter(|c| **c == Call::Show(true)).count();
        assert_eq!((hides, shows), (1, 1));
    }

    #[test]
    fn hide_before_engage_does_nothing() {
        let (mut lock, log) = fake();
        assert!(!lock.hide());
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn release_twice_is_safe_and_shows_only_once() {
        let (mut lock, log) = fake();
        lock.engage((100, 100), 10);
        lock.hide();
        lock.release();
        lock.release();
        let shows = log.borrow().iter().filter(|c| **c == Call::Show(true)).count();
        assert_eq!(shows, 1);
        assert_eq!(lock.ops.counter, 0, "the show count is back to 0");
    }

    #[test]
    fn release_without_a_hide_never_shows() {
        let (mut lock, log) = fake();
        lock.engage((100, 100), 10);
        lock.release();
        assert_eq!(*log.borrow(), vec![Call::Clip(Some((90, 90, 110, 110))), Call::Clip(None)]);
    }

    #[test]
    fn a_show_loop_is_capped() {
        // A counter that never crosses zero on its own: each loop stops at MAX_SHOW_CALLS, never runs away.
        struct Stuck {
            calls: u32,
        }
        impl CursorOps for Stuck {
            fn clip(&mut self, _rect: Option<(i32, i32, i32, i32)>) {}
            fn show(&mut self, show: bool) -> i32 {
                self.calls += 1;
                if show {
                    -1
                } else {
                    0
                }
            }
        }
        let mut lock = CursorLock::new(Stuck { calls: 0 });
        lock.engage((0, 0), 5);
        lock.hide();
        assert_eq!(lock.ops.calls, MAX_SHOW_CALLS);
        lock.release();
        assert_eq!(lock.ops.calls, 2 * MAX_SHOW_CALLS);
    }

    #[test]
    fn dropping_a_held_lock_gives_the_pointer_back() {
        let log = Rc::new(RefCell::new(Vec::new()));
        {
            let mut lock = CursorLock::new(Fake { log: log.clone(), counter: 0 });
            lock.engage((100, 100), 10);
            lock.hide();
        }
        assert_eq!(log.borrow().last(), Some(&Call::Show(true)));
        assert!(log.borrow().contains(&Call::Clip(None)));
    }
}
