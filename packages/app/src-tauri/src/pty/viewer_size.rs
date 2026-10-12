//! Phone-sized panes: a paired phone looking at a pane's tab leases the PTY size, so the agent
//! redraws for the phone's screen instead of the phone shrinking a picture of the PC's grid.
//!
//! While a lease is live, resizes from the desktop pane window are remembered, not applied. When
//! the phone releases the lease (left the tab, app hidden) or stops renewing it (phone asleep,
//! connection lost: `LEASE_TTL_MS`), the pane goes back to the last desktop size.

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
struct Lease {
    size: Size,
    expires_at_ms: u64,
    /// The size the desktop wants (pane size before the lease, or its latest resize since).
    desktop: Size,
}

#[derive(Debug, Default)]
pub struct Leases {
    by_pane: HashMap<String, Lease>,
}

pub fn clamp(cols: u16, rows: u16) -> Size {
    (cols.clamp(MIN_COLS, MAX_COLS), rows.clamp(MIN_ROWS, MAX_ROWS))
}

impl Leases {
    /// The phone views the pane at `cols`x`rows`: start or renew the lease. Returns the size to
    /// apply to the PTY. `current` is the pane's size now (remembered as the desktop size).
    pub fn claim(&mut self, pane_id: &str, cols: u16, rows: u16, current: Size, now_ms: u64) -> Size {
        let size = clamp(cols, rows);
        let expires_at_ms = now_ms + LEASE_TTL_MS;
        self.by_pane
            .entry(pane_id.to_string())
            .and_modify(|lease| {
                lease.size = size;
                lease.expires_at_ms = expires_at_ms;
            })
            .or_insert(Lease { size, expires_at_ms, desktop: current });
        size
    }

    /// The phone left the pane. Returns the desktop size to restore, if it held a lease.
    pub fn release(&mut self, pane_id: &str) -> Option<Size> {
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

    /// Leases that ran out, removed, with the desktop size to restore.
    pub fn take_expired(&mut self, now_ms: u64) -> Vec<(String, Size)> {
        let expired: Vec<String> = self
            .by_pane
            .iter()
            .filter(|(_, lease)| lease.expires_at_ms <= now_ms)
            .map(|(id, _)| id.clone())
            .collect();
        expired
            .into_iter()
            .filter_map(|id| self.by_pane.remove(&id).map(|lease| (id, lease.desktop)))
            .collect()
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
    }
}

/// `POST /panes/{id}/viewport` with `active: true`: lease the pane at the phone's size.
pub fn claim(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str, cols: u16, rows: u16) -> Result<Size, String> {
    let current = {
        let reg = registry.lock();
        let pane = reg.get(pane_id).ok_or_else(|| format!("unknown pane: {pane_id}"))?;
        (pane.info.cols, pane.info.rows)
    };
    let size = LEASES.lock().claim(pane_id, cols, rows, current, now_ms());
    if size != current {
        tracing::info!(pane = %pane_id, from = ?current, to = ?size, "phone size lease: resizing pane for the phone");
    } else {
        tracing::debug!(pane = %pane_id, size = ?size, "phone size lease renewed");
    }
    apply(registry, pane_id, size);
    ensure_sweeper(registry.clone());
    Ok(size)
}

/// `POST /panes/{id}/viewport` with `active: false`: give the pane back to the desktop size.
pub fn release(registry: &Mutex<PaneRegistry>, pane_id: &str) -> Option<Size> {
    let desktop = LEASES.lock().release(pane_id)?;
    tracing::info!(pane = %pane_id, to = ?desktop, "phone size lease released; restoring desktop size");
    apply(registry, pane_id, desktop);
    Some(desktop)
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
                for (pane_id, desktop) in expired {
                    tracing::info!(pane = %pane_id, cols = desktop.0, rows = desktop.1, "phone size lease expired; restoring desktop size");
                    apply(&registry, &pane_id, desktop);
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
        assert_eq!(leases.claim("p", 48, 38, (121, 36), 0), (48, 38));
        // Renewal changes the phone size but not the remembered desktop size.
        assert_eq!(leases.claim("p", 80, 20, (48, 38), 1_000), (80, 20));
        assert_eq!(leases.release("p"), Some((121, 36)));
        assert_eq!(leases.release("p"), None);
        assert!(leases.is_empty());
    }

    #[test]
    fn desktop_resizes_during_a_lease_are_remembered_not_applied() {
        let mut leases = Leases::default();
        assert!(!leases.desktop_resize("p", 100, 30), "no lease: the desktop resize applies");
        leases.claim("p", 48, 38, (121, 36), 0);
        assert!(leases.desktop_resize("p", 140, 40));
        assert_eq!(leases.release("p"), Some((140, 40)));
    }

    #[test]
    fn quiet_phones_lose_the_lease() {
        let mut leases = Leases::default();
        leases.claim("a", 48, 38, (121, 36), 0);
        leases.claim("b", 48, 38, (90, 30), 10_000);
        assert!(leases.take_expired(LEASE_TTL_MS - 1).is_empty());
        assert_eq!(leases.take_expired(LEASE_TTL_MS), vec![("a".to_string(), (121, 36))]);
        assert_eq!(leases.take_expired(10_000 + LEASE_TTL_MS), vec![("b".to_string(), (90, 30))]);
        assert!(leases.is_empty());
    }

    #[test]
    fn sizes_are_clamped_to_something_a_tui_can_draw() {
        assert_eq!(clamp(1, 1), (MIN_COLS, MIN_ROWS));
        assert_eq!(clamp(9_999, 9_999), (MAX_COLS, MAX_ROWS));
        assert_eq!(clamp(48, 38), (48, 38));
    }
}
