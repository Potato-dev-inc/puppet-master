//! Phone-sized panes: a paired phone that types into a pane's tab leases the PTY size, so the agent
//! redraws for the phone's screen instead of the phone shrinking a picture of the PC's grid.
//!
//! Viewing alone never takes the size: only input from the phone claims it (the phone then renews
//! the lease while it keeps typing). The size is sticky: when the phone leaves the tab, hides, or
//! stops renewing (asleep), the pane keeps the phone size. The desktop pane window takes the size
//! back with `reclaim` (a click or focus in its window), which restores the desktop size.
//!
//! Several phones may hold one pane (each with its own `viewer` id): the pane takes the smallest
//! of their sizes (like tmux with several clients), so it fits every one of them.
//! While an entry exists, resizes from the desktop pane window are remembered, not applied.

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use super::PaneRegistry;

/// A lease the phone does not renew within this time expires (the phone renews every ~5 s).
pub const LEASE_TTL_MS: u64 = 15_000;
const SWEEP_EVERY_MS: u64 = 3_000;
const MIN_COLS: u16 = 20;
const MAX_COLS: u16 = 400;
const MIN_ROWS: u16 = 6;
const MAX_ROWS: u16 = 200;

type Size = (u16, u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Viewer {
    size: Size,
    expires_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PaneLease {
    /// Phones currently holding the size. Empty when the size is sticky (phone gone, size kept).
    viewers: HashMap<String, Viewer>,
    /// The size the desktop wants (pane size before the first lease, or its latest resize since).
    desktop: Size,
}

impl PaneLease {
    /// Smallest cols and smallest rows over the viewers: a grid every one of them can show.
    fn effective(&self) -> Option<Size> {
        let cols = self.viewers.values().map(|v| v.size.0).min()?;
        let rows = self.viewers.values().map(|v| v.size.1).min()?;
        Some((cols, rows))
    }
}

#[derive(Debug, Default)]
pub struct Leases {
    by_pane: HashMap<String, PaneLease>,
}

pub fn clamp(cols: u16, rows: u16) -> Size {
    (cols.clamp(MIN_COLS, MAX_COLS), rows.clamp(MIN_ROWS, MAX_ROWS))
}

impl Leases {
    /// `viewer` sends input to the pane at `cols`x`rows`: start or renew its lease. Returns the size
    /// to apply to the PTY (the smallest over all viewers). `current` is the pane's size now
    /// (remembered as the desktop size when this is the first lease).
    pub fn claim(&mut self, pane_id: &str, viewer: &str, cols: u16, rows: u16, current: Size, now_ms: u64) -> Size {
        let lease = self
            .by_pane
            .entry(pane_id.to_string())
            .or_insert_with(|| PaneLease { viewers: HashMap::new(), desktop: current });
        lease
            .viewers
            .insert(viewer.to_string(), Viewer { size: clamp(cols, rows), expires_at_ms: now_ms + LEASE_TTL_MS });
        lease.effective().unwrap_or(current)
    }

    /// Extends `viewer`'s lease only if it exists. Never starts a lease, never changes a size.
    /// True when `viewer` holds a lease on the pane after the call.
    pub fn renew(&mut self, pane_id: &str, viewer: &str, now_ms: u64) -> bool {
        match self.by_pane.get_mut(pane_id).and_then(|lease| lease.viewers.get_mut(viewer)) {
            Some(held) => {
                held.expires_at_ms = now_ms + LEASE_TTL_MS;
                true
            }
            None => false,
        }
    }

    /// `viewer` stopped viewing. Returns the size to apply now: the smallest over the viewers left.
    /// None when no viewer is left (the size stays sticky, nothing resizes) or it held no lease.
    pub fn release(&mut self, pane_id: &str, viewer: &str) -> Option<Size> {
        let lease = self.by_pane.get_mut(pane_id)?;
        lease.viewers.remove(viewer)?;
        lease.effective()
    }

    /// The desktop pane window takes its size back: removes the entry and returns the desktop size
    /// to apply. None when no entry existed.
    pub fn reclaim(&mut self, pane_id: &str) -> Option<Size> {
        self.by_pane.remove(pane_id).map(|lease| lease.desktop)
    }

    /// The desktop window resized the pane. True when a phone holds it: the size is remembered
    /// for later and must not be applied now.
    pub fn desktop_resize(&mut self, pane_id: &str, cols: u16, rows: u16) -> bool {
        match self.by_pane.get_mut(pane_id) {
            Some(lease) => {
                lease.desktop = (cols, rows);
                true
            }
            None => false,
        }
    }

    /// Drops viewers that stopped renewing. Returns, per pane whose live viewers changed and that
    /// still has one, the size to apply now (smallest over the viewers left). A pane left with no
    /// viewers stays sticky at its current size: it is not returned.
    pub fn take_expired(&mut self, now_ms: u64) -> Vec<(String, Size)> {
        let mut changed = Vec::new();
        for (pane_id, lease) in self.by_pane.iter_mut() {
            let before = lease.viewers.len();
            lease.viewers.retain(|_, viewer| viewer.expires_at_ms > now_ms);
            if lease.viewers.len() != before {
                if let Some(size) = lease.effective() {
                    changed.push((pane_id.clone(), size));
                }
            }
        }
        changed.sort();
        changed
    }

    /// A phone holds the pane's size (live or sticky): the desktop must not take it silently.
    pub fn holds(&self, pane_id: &str) -> bool {
        self.by_pane.contains_key(pane_id)
    }

    pub fn is_empty(&self) -> bool {
        self.by_pane.is_empty()
    }
}

static LEASES: Lazy<Mutex<Leases>> = Lazy::new(|| Mutex::new(Leases::default()));
static SWEEPER: once_cell::sync::OnceCell<()> = once_cell::sync::OnceCell::new();

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Resize the PTY and tell the phone and the desktop window. Errors (pane gone) are ignored.
fn apply(registry: &Mutex<PaneRegistry>, pane_id: &str, (cols, rows): Size) {
    if let Ok(true) = super::registry_resize(registry, pane_id, cols, rows) {
        crate::bridge::notify_pane_resized(pane_id, cols, rows);
        crate::bridge::notify_pty_resized_by_lease(pane_id, cols, rows);
    }
}

/// `POST /panes/{id}/viewport` (input from the phone): lease the pane at this phone's size.
pub fn claim(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    viewer: &str,
    cols: u16,
    rows: u16,
) -> Result<Size, String> {
    let current = {
        let reg = registry.lock();
        let pane = reg.get(pane_id).ok_or_else(|| format!("unknown pane: {pane_id}"))?;
        (pane.info.cols, pane.info.rows)
    };
    let size = LEASES.lock().claim(pane_id, viewer, cols, rows, current, now_ms());
    if size != current {
        tracing::info!(pane = %pane_id, from = ?current, to = ?size, "phone size lease: resizing pane for the phone");
    } else {
        tracing::debug!(pane = %pane_id, size = ?size, "phone size lease renewed");
    }
    apply(registry, pane_id, size);
    ensure_sweeper(registry.clone());
    Ok(size)
}

/// `POST /panes/{id}/viewport` with `renew_only`: extend this phone's lease if it has one.
/// True when the phone still holds the pane. Never resizes.
pub fn renew(pane_id: &str, viewer: &str) -> bool {
    LEASES.lock().renew(pane_id, viewer, now_ms())
}

/// `POST /panes/{id}/viewport` with `active: false`: this phone stops viewing. The pane takes the
/// size of the phones left; with none left it keeps its size (sticky).
pub fn release(registry: &Mutex<PaneRegistry>, pane_id: &str, viewer: &str) -> Option<Size> {
    let size = LEASES.lock().release(pane_id, viewer)?;
    tracing::info!(pane = %pane_id, to = ?size, "phone size lease released");
    apply(registry, pane_id, size);
    Some(size)
}

/// The desktop pane window takes the pane back from the phone: restores the desktop size.
/// Returns the size applied, or None when no phone held the pane.
pub fn reclaim(registry: &Mutex<PaneRegistry>, pane_id: &str) -> Option<Size> {
    let desktop = LEASES.lock().reclaim(pane_id)?;
    tracing::info!(pane = %pane_id, to = ?desktop, "phone size lease reclaimed by the desktop");
    apply(registry, pane_id, desktop);
    // Also when the size did not change: the desktop windows must drop their "sized for phone" notice.
    crate::bridge::notify_panes_changed();
    Some(desktop)
}

/// A phone holds this pane's size (live or sticky).
pub fn is_held(pane_id: &str) -> bool {
    LEASES.lock().holds(pane_id)
}

/// The desktop pane window asks for a size: true when a phone holds the pane (remembered only).
pub fn desktop_resize(pane_id: &str, cols: u16, rows: u16) -> bool {
    LEASES.lock().desktop_resize(pane_id, cols, rows)
}

/// One thread for the app's life, started by the first lease: drops phones that went quiet. A pane
/// whose phones all went quiet keeps the size they left (sticky).
fn ensure_sweeper(registry: Arc<Mutex<PaneRegistry>>) {
    SWEEPER.get_or_init(|| {
        let spawned = std::thread::Builder::new()
            .name("pm-viewer-size-sweeper".into())
            .spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(SWEEP_EVERY_MS));
                // Sticky entries outlive their phones: forget the ones whose pane was closed.
                {
                    let reg = registry.lock();
                    LEASES.lock().by_pane.retain(|pane_id, _| reg.get(pane_id).is_some());
                }
                let expired = LEASES.lock().take_expired(now_ms());
                for (pane_id, size) in expired {
                    tracing::info!(pane = %pane_id, cols = size.0, rows = size.1, "phone size lease expired");
                    apply(&registry, &pane_id, size);
                }
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "viewer size sweeper failed to start");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lease_is_sticky_after_the_phone_leaves_and_the_desktop_takes_it_back() {
        let mut leases = Leases::default();
        assert_eq!(leases.claim("p", "phone", 48, 38, (121, 36), 0), (48, 38));
        // Renewal changes the phone size but not the remembered desktop size.
        assert_eq!(leases.claim("p", "phone", 80, 20, (48, 38), 1_000), (80, 20));
        // The phone leaves: nothing to apply, the entry stays (sticky).
        assert_eq!(leases.release("p", "phone"), None);
        assert!(leases.holds("p"));
        assert!(!leases.is_empty());
        // The desktop takes it back: the desktop size, and the entry is gone.
        assert_eq!(leases.reclaim("p"), Some((121, 36)));
        assert!(!leases.holds("p"));
        assert_eq!(leases.reclaim("p"), None);
        assert!(leases.is_empty());
    }

    #[test]
    fn renew_only_extends_an_existing_lease_and_never_starts_one() {
        let mut leases = Leases::default();
        assert!(!leases.renew("p", "phone", 0), "no lease: renewal does not start one");
        assert!(!leases.holds("p"));
        leases.claim("p", "phone", 48, 38, (121, 36), 0);
        assert!(leases.renew("p", "phone", LEASE_TTL_MS - 1));
        // Renewed just before the old expiry: still alive after it.
        assert!(leases.take_expired(LEASE_TTL_MS).is_empty());
        assert!(!leases.renew("p", "other", LEASE_TTL_MS), "another viewer does not hold it");
    }

    #[test]
    fn renew_after_the_desktop_took_it_back_does_not_retake_it() {
        let mut leases = Leases::default();
        leases.claim("p", "phone", 48, 38, (121, 36), 0);
        leases.release("p", "phone");
        assert_eq!(leases.reclaim("p"), Some((121, 36)));
        assert!(!leases.renew("p", "phone", 100));
        assert!(!leases.holds("p"));
    }

    #[test]
    fn an_expired_only_phone_leaves_the_pane_sticky_with_no_resize() {
        let mut leases = Leases::default();
        leases.claim("p", "phone", 48, 38, (121, 36), 0);
        assert!(leases.take_expired(LEASE_TTL_MS).is_empty(), "no live viewer left: no resize");
        assert!(leases.holds("p"));
        assert!(leases.take_expired(LEASE_TTL_MS + 1).is_empty());
        assert_eq!(leases.reclaim("p"), Some((121, 36)));
    }

    #[test]
    fn two_phones_share_the_smallest_grid_instead_of_taking_turns() {
        let mut leases = Leases::default();
        assert_eq!(leases.claim("p", "emulator", 56, 56, (121, 36), 0), (56, 56));
        assert_eq!(leases.claim("p", "phone", 48, 21, (56, 56), 100), (48, 21));
        // Renewals in any order keep the same answer: no flipping.
        assert_eq!(leases.claim("p", "emulator", 56, 56, (48, 21), 5_000), (48, 21));
        assert_eq!(leases.claim("p", "phone", 48, 21, (48, 21), 5_100), (48, 21));
        // The phone closes its keyboard: rows grow for both, cols stay the smaller.
        assert_eq!(leases.claim("p", "phone", 48, 40, (48, 21), 6_000), (48, 40));
        // One leaves: the other keeps its size; the last leaves: sticky until the desktop reclaims.
        assert_eq!(leases.release("p", "phone"), Some((56, 56)));
        assert_eq!(leases.release("p", "emulator"), None);
        assert_eq!(leases.reclaim("p"), Some((121, 36)));
        assert!(leases.is_empty());
    }

    #[test]
    fn desktop_resizes_during_a_lease_are_remembered_not_applied() {
        let mut leases = Leases::default();
        assert!(!leases.desktop_resize("p", 100, 30), "no lease: the desktop resize applies");
        leases.claim("p", "phone", 48, 38, (121, 36), 0);
        assert!(leases.desktop_resize("p", 140, 40));
        assert_eq!(leases.release("p", "phone"), None);
        assert_eq!(leases.reclaim("p"), Some((140, 40)));
    }

    #[test]
    fn quiet_phones_drop_out_and_the_rest_keep_their_size() {
        let mut leases = Leases::default();
        leases.claim("a", "phone", 48, 38, (121, 36), 0);
        leases.claim("b", "phone", 48, 38, (90, 30), 10_000);
        assert!(leases.take_expired(LEASE_TTL_MS - 1).is_empty());
        // "a" went quiet: no live viewer left on it, so no resize; the entry stays sticky.
        assert!(leases.take_expired(LEASE_TTL_MS).is_empty());
        assert!(leases.holds("a"));
        // "b" renewed at 10 s, so it is still live at 15 s and holds the pane.
        assert!(leases.take_expired(10_000 + LEASE_TTL_MS).is_empty());
        assert!(leases.holds("b"));
    }

    #[test]
    fn a_quiet_phone_leaves_the_others_their_size() {
        let mut leases = Leases::default();
        leases.claim("p", "asleep", 40, 20, (121, 36), 0);
        leases.claim("p", "awake", 56, 50, (40, 20), 0);
        leases.claim("p", "awake", 56, 50, (40, 20), 10_000);
        assert_eq!(leases.take_expired(LEASE_TTL_MS), vec![("p".to_string(), (56, 50))]);
        assert!(leases.holds("p"));
    }

    #[test]
    fn sizes_are_clamped_to_something_a_tui_can_draw() {
        assert_eq!(clamp(1, 1), (MIN_COLS, MIN_ROWS));
        assert_eq!(clamp(9_999, 9_999), (MAX_COLS, MAX_ROWS));
        assert_eq!(clamp(48, 38), (48, 38));
    }
}
