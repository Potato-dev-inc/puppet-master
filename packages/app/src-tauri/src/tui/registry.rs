//! Runtime agent registry (SPEC REG-1..3): an `ArcSwap` snapshot of every profile.
//!
//! `profile_for`, `profiles()` and the other lookups in `tui` read this. A reader takes the current
//! snapshot without locking; a reload swaps in a whole new one.

use super::*;
use arc_swap::ArcSwap;
use std::sync::{Arc, OnceLock};

/// Ordered profiles (code profiles first, in `read_only_backends` order) behind an atomic snapshot.
pub struct Registry {
    snapshot: ArcSwap<Vec<Arc<dyn TuiProfile>>>,
    fallback: Arc<dyn TuiProfile>,
}

impl Registry {
    /// The nine code profiles; this order is what `read_only_backends` reports, keep it stable.
    pub fn seeded() -> Self {
        let code: Vec<Arc<dyn TuiProfile>> = vec![
            Arc::new(claude::Claude),
            Arc::new(codex::Codex),
            Arc::new(opencode::OpenCode),
            Arc::new(opencode::OpenCodeNative),
            Arc::new(cursor::CursorAgent),
            Arc::new(cursor::CursorIde),
            Arc::new(shell::Cmd),
            Arc::new(shell::Powershell),
            Arc::new(shell::Bash),
        ];
        Self {
            snapshot: ArcSwap::from_pointee(code),
            fallback: Arc::new(generic::Unknown),
        }
    }

    /// The current snapshot; stays valid (and unchanged) for the caller across later swaps.
    pub fn snapshot(&self) -> Arc<Vec<Arc<dyn TuiProfile>>> {
        self.snapshot.load_full()
    }

    /// Profiles in registry order.
    #[cfg_attr(not(test), allow(dead_code))] // only `tui::profiles()` (test-only) reads it so far
    pub fn ordered(&self) -> Vec<Arc<dyn TuiProfile>> {
        self.snapshot.load().iter().cloned().collect()
    }

    /// Profile for an `agent_type`, case-insensitively; unregistered types get generic behaviour.
    pub fn profile_for_arc(&self, agent_type: &str) -> Arc<dyn TuiProfile> {
        self.snapshot
            .load()
            .iter()
            .find(|profile| profile.id().eq_ignore_ascii_case(agent_type))
            .cloned()
            .unwrap_or_else(|| self.fallback.clone())
    }

    /// Replace the whole snapshot atomically (SPEC REG-2).
    #[cfg_attr(not(test), allow(dead_code))] // the reload path (later phase) calls it
    pub fn swap(&self, profiles: Vec<Arc<dyn TuiProfile>>) {
        self.snapshot.store(Arc::new(profiles));
    }
}

/// The process-wide registry, seeded with the code profiles on first use.
pub fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::seeded)
}

/// Current snapshot of the process-wide registry.
pub fn snapshot() -> Arc<Vec<Arc<dyn TuiProfile>>> {
    registry().snapshot()
}

/// `profile_for` against the process-wide registry, returning an owned handle.
pub fn profile_for_arc(agent_type: &str) -> Arc<dyn TuiProfile> {
    registry().profile_for_arc(agent_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[test]
    fn seeded_in_code_order() {
        let registry = Registry::seeded();
        let ids: Vec<&str> = registry.ordered().iter().map(|p| p.id()).collect();
        let expected: Vec<&str> = profiles().iter().map(|p| p.id()).collect();
        assert_eq!(ids, expected);
        assert_eq!(registry.snapshot().len(), 9);
        assert_eq!(registry.profile_for_arc("CLAUDE").id(), "claude");
        assert_eq!(registry.profile_for_arc("not-a-real-agent").id(), "unknown");
    }

    /// The hot path hands out the same `Arc` on every call until a swap replaces the snapshot.
    #[test]
    fn profile_for_returns_same_arc_until_swap() {
        let first = crate::tui::profile_for("claude");
        let again = crate::tui::profile_for("CLAUDE");
        assert!(Arc::ptr_eq(&first, &again));

        let registry = Registry::seeded();
        let before = registry.profile_for_arc("codex");
        assert!(Arc::ptr_eq(&before, &registry.profile_for_arc("codex")));
        let unknown = registry.profile_for_arc("not-a-real-agent");
        assert!(Arc::ptr_eq(&unknown, &registry.profile_for_arc("also-not-real")));
        registry.swap(vec![Arc::new(codex::Codex)]);
        let after = registry.profile_for_arc("codex");
        assert!(!Arc::ptr_eq(&before, &after));
        assert!(Arc::ptr_eq(&after, &registry.profile_for_arc("codex")));
    }

    /// Profile whose id carries the swap generation that published it.
    struct Marker(&'static str);

    impl TuiProfile for Marker {
        fn id(&self) -> &'static str {
            self.0
        }
    }

    fn generation_of(snapshot: &[Arc<dyn TuiProfile>]) -> usize {
        snapshot
            .iter()
            .find_map(|p| p.id().strip_prefix("marker_"))
            .map_or(0, |n| n.parse().unwrap())
    }

    /// 10k swaps while 8 threads read: no panic, code profiles always resolve, and a reader never
    /// sees a snapshot older than the last swap published before its read began.
    #[test]
    fn stress_swap_while_reading() {
        const SWAPS: usize = 10_000;
        let registry = Arc::new(Registry::seeded());
        let published = Arc::new(AtomicUsize::new(0));
        let done = Arc::new(AtomicBool::new(false));
        let readers: Vec<_> = (0..8)
            .map(|_| {
                let (registry, published, done) =
                    (registry.clone(), published.clone(), done.clone());
                std::thread::spawn(move || {
                    let mut reads = 0usize;
                    while !done.load(Ordering::SeqCst) || reads == 0 {
                        let floor = published.load(Ordering::SeqCst);
                        let seen = generation_of(&registry.snapshot());
                        assert!(seen >= floor, "stale snapshot {seen} after swap {floor}");
                        assert_eq!(registry.profile_for_arc("claude").id(), "claude");
                        assert_eq!(registry.profile_for_arc("bash").id(), "bash");
                        reads += 1;
                    }
                    reads
                })
            })
            .collect();
        let code: Vec<Arc<dyn TuiProfile>> = registry.ordered();
        for generation in 1..=SWAPS {
            let id: &'static str = Box::leak(format!("marker_{generation}").into_boxed_str());
            let mut next = code.clone();
            next.push(Arc::new(Marker(id)));
            registry.swap(next);
            published.store(generation, Ordering::SeqCst);
        }
        done.store(true, Ordering::SeqCst);
        for reader in readers {
            assert!(reader.join().expect("reader panicked") > 0);
        }
        assert_eq!(generation_of(&registry.snapshot()), SWAPS);
        assert_eq!(registry.profile_for_arc("marker_10000").id(), "marker_10000");
    }
}
