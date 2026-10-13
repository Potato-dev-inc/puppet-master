//! Phone-sized panes: a paired phone looking at a pane's tab leases the PTY size, so the agent
//! redraws for the phone's screen instead of the phone shrinking a picture of the PC's grid.
//!
//! Several phones may view one pane (each with its own `viewer` id): the pane takes the smallest
//! of their sizes (like tmux with several clients), so it fits every one of them and they never
//! take turns resizing it. While any lease is live, resizes from the desktop pane window are
//! remembered, not applied. When the last phone releases (left the tab, app hidden) or stops
//! renewing (phone asleep, connection lost: `LEASE_TTL_MS`), the pane goes back to the desktop size.

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use super::PaneRegistry;

/// A lease the phone does not renew within this time ends (the phone heartbeats every ~5 s).
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
    /// `viewer` sees the pane at `cols`x`rows`: start or renew its lease. Returns the size to apply
    /// to the PTY (the smallest over all viewers). `current` is the pane's size now (remembered as
    /// the desktop size when this is the first lease).
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

    /// `viewer` left the pane. Returns the size to apply now: the smallest over the viewers left,
    /// or the desktop size when it was the last one. None when it held no lease.
    pub fn release(&mut self, pane_id: &str, viewer: &str) -> Option<Size> {
        let lease = self.by_pane.get_mut(pane_id)?;
        lease.viewers.remove(viewer)?;
        match lease.effective() {
            Some(size) => Some(size),
            None => self.by_pane.remove(pane_id).map(|lease| lease.desktop),
        }
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

    /// Drops viewers that stopped renewing. Returns, per pane that changed, the size to apply now
    /// (smallest over the viewers left, or the desktop size when none are left).
    pub fn take_expired(&mut self, now_ms: u64) -> Vec<(String, Size)> {
        let mut changed = Vec::new();
        for (pane_id, lease) in self.by_pane.iter_mut() {
            let before = lease.viewers.len();
            lease.viewers.retain(|_, viewer| viewer.expires_at_ms > now_ms);
            if lease.viewers.len() != before {
                changed.push((pane_id.clone(), lease.effective().unwrap_or(lease.desktop)));
            }
        }
        self.by_pane.retain(|_, lease| !lease.viewers.is_empty());
        changed.sort();
        changed
    }

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

/// `POST /panes/{id}/viewport` with `active: true`: lease the pane at this phone's size.
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

/// `POST /panes/{id}/viewport` with `active: false`: this phone stops viewing; the pane takes the
/// size of the phones left, or goes back to the desktop size.
pub fn release(registry: &Mutex<PaneRegistry>, pane_id: &str, viewer: &str) -> Option<Size> {
    let size = LEASES.lock().release(pane_id, viewer)?;
    tracing::info!(pane = %pane_id, to = ?size, "phone size lease released");
    apply(registry, pane_id, size);
    Some(size)
}

/// A phone currently holds this pane's size.
pub fn is_held(pane_id: &str) -> bool {
    LEASES.lock().holds(pane_id)
}

/// The desktop pane window asks for a size: true when a phone holds the pane (remembered only).
pub fn desktop_resize(pane_id: &str, cols: u16, rows: u16) -> bool {
    LEASES.lock().desktop_resize(pane_id, cols, rows)
}

/// One thread for the app's life, started by the first lease: restores panes whose phone went
/// quiet. Leases of panes that no longer exist just end.
fn ensure_sweeper(registry: Arc<Mutex<PaneRegistry>>) {
    SWEEPER.get_or_init(|| {
        let spawned = std::thread::Builder::new()
            .name("pm-viewer-size-sweeper".into())
            .spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(SWEEP_EVERY_MS));
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
    fn a_lease_keeps_the_desktop_size_and_gives_it_back() {
        let mut leases = Leases::default();
        assert_eq!(leases.claim("p", "phone", 48, 38, (121, 36), 0), (48, 38));
        // Renewal changes the phone size but not the remembered desktop size.
        assert_eq!(leases.claim("p", "phone", 80, 20, (48, 38), 1_000), (80, 20));
        assert_eq!(leases.release("p", "phone"), Some((121, 36)));
        assert_eq!(leases.release("p", "phone"), None);
        assert!(leases.is_empty());
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
        // One leaves: the other gets its own size; the last leaves: desktop size.
        assert_eq!(leases.release("p", "phone"), Some((56, 56)));
        assert_eq!(leases.release("p", "emulator"), Some((121, 36)));
        assert!(leases.is_empty());
    }

    #[test]
    fn desktop_resizes_during_a_lease_are_remembered_not_applied() {
        let mut leases = Leases::default();
        assert!(!leases.desktop_resize("p", 100, 30), "no lease: the desktop resize applies");
        leases.claim("p", "phone", 48, 38, (121, 36), 0);
        assert!(leases.desktop_resize("p", 140, 40));
        assert_eq!(leases.release("p", "phone"), Some((140, 40)));
    }

    #[test]
    fn quiet_phones_lose_the_lease() {
        let mut leases = Leases::default();
        leases.claim("a", "phone", 48, 38, (121, 36), 0);
        leases.claim("b", "phone", 48, 38, (90, 30), 10_000);
        assert!(leases.take_expired(LEASE_TTL_MS - 1).is_empty());
        assert_eq!(leases.take_expired(LEASE_TTL_MS), vec![("a".to_string(), (121, 36))]);
        assert_eq!(leases.take_expired(10_000 + LEASE_TTL_MS), vec![("b".to_string(), (90, 30))]);
        assert!(leases.is_empty());
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
