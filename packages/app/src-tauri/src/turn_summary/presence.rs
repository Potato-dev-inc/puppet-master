//! Is the user at the desk? Windows idle time (`GetLastInputInfo`) plus whether the cockpit window
//! has focus. "Away" = no input for `threshold_ms` (default 5 minutes). When the user is active
//! again and the cockpit is focused (or opened), the machine says "show the digest for
//! `since_ms..now`". A pure state machine: the clock and the idle reading are passed in.

/// Default: 5 minutes without keyboard or mouse.
pub const DEFAULT_AWAY_MS: u64 = 5 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Return {
    /// When the user stopped (the start of the digest window).
    pub since_ms: u64,
}

#[derive(Debug)]
pub struct Presence {
    /// 0 turns the feature off.
    threshold_ms: u64,
    /// When input stopped, while the user is (or was and has not yet seen the digest) away.
    away_since: Option<u64>,
    /// Input resumed after being away; the digest waits for the cockpit to be in front.
    active_again: bool,
    focused: bool,
}

impl Presence {
    pub fn new(threshold_ms: u64) -> Self {
        Self { threshold_ms, away_since: None, active_again: false, focused: false }
    }

    pub fn set_threshold(&mut self, threshold_ms: u64) {
        self.threshold_ms = threshold_ms;
        if threshold_ms == 0 {
            self.away_since = None;
            self.active_again = false;
        }
    }

    /// One poll of the idle clock. `idle_ms` = time since the last keyboard or mouse input.
    pub fn tick(&mut self, now_ms: u64, idle_ms: u64) -> Option<Return> {
        if self.threshold_ms == 0 {
            return None;
        }
        if idle_ms >= self.threshold_ms {
            // Still away (or just went away). The earliest start is kept if a return was not yet shown.
            if self.away_since.is_none() {
                self.away_since = Some(now_ms.saturating_sub(idle_ms));
            }
            self.active_again = false;
            return None;
        }
        if self.away_since.is_some() {
            self.active_again = true;
        }
        self.ready()
    }

    /// The cockpit window gained or lost focus (also called with `true` when it is opened).
    pub fn set_focus(&mut self, focused: bool) -> Option<Return> {
        self.focused = focused;
        self.ready()
    }

    fn ready(&mut self) -> Option<Return> {
        if self.active_again && self.focused {
            let since_ms = self.away_since.take()?;
            self.active_again = false;
            return Some(Return { since_ms });
        }
        None
    }
}

/// Milliseconds since the last keyboard or mouse input on this desktop, when the OS can tell.
#[cfg(windows)]
pub fn system_idle_ms() -> Option<u64> {
    #[repr(C)]
    struct LastInputInfo {
        cb_size: u32,
        dw_time: u32,
    }
    #[link(name = "user32")]
    extern "system" {
        fn GetLastInputInfo(plii: *mut LastInputInfo) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetTickCount() -> u32;
    }
    let mut info = LastInputInfo { cb_size: std::mem::size_of::<LastInputInfo>() as u32, dw_time: 0 };
    // SAFETY: `info` is a valid, correctly sized LASTINPUTINFO that lives for the call; both
    // functions only read OS state.
    let ok = unsafe { GetLastInputInfo(&mut info) };
    if ok == 0 {
        return None;
    }
    // SAFETY: no arguments, no pointers.
    let now = unsafe { GetTickCount() };
    Some(u64::from(now.wrapping_sub(info.dw_time)))
}

/// Other platforms: no reading, so the user is never considered away (no false digests).
#[cfg(not(windows))]
pub fn system_idle_ms() -> Option<u64> {
    None
}
