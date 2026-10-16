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
    /// The compiled-in profiles, always first in every snapshot `set_declarative` publishes.
    code: Vec<Arc<dyn TuiProfile>>,
    fallback: Arc<dyn TuiProfile>,
    /// Profiles a wizard pane runs under while the Agent Wizard owns it (`add_transient`). Found by the
    /// id lookups below, never part of `snapshot()`, so they are not listed, not reloaded and not shown
    /// in spawn menus.
    transient: ArcSwap<Vec<Arc<dyn TuiProfile>>>,
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
            snapshot: ArcSwap::from_pointee(code.clone()),
            code,
            fallback: Arc::new(generic::Unknown),
            transient: ArcSwap::from_pointee(Vec::new()),
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
        self.find(agent_type).unwrap_or_else(|| self.fallback.clone())
    }

    /// A registered profile, or a transient one (a pane the Agent Wizard owns), by id.
    pub fn find(&self, agent_type: &str) -> Option<Arc<dyn TuiProfile>> {
        self.snapshot
            .load()
            .iter()
            .find(|profile| profile.id().eq_ignore_ascii_case(agent_type))
            .cloned()
            .or_else(|| {
                self.transient.load().iter().find(|profile| profile.id().eq_ignore_ascii_case(agent_type)).cloned()
            })
    }

    /// Make `profile` findable by id until `remove_transient`. Not listed and not reloaded. An id that a
    /// registered profile already uses is refused.
    pub fn add_transient(&self, profile: Arc<dyn TuiProfile>) -> Result<(), String> {
        let id = profile.id();
        if self.snapshot.load().iter().any(|p| p.id().eq_ignore_ascii_case(id)) {
            return Err(format!("{id} is already a registered agent"));
        }
        self.transient.rcu(|current| {
            let mut next: Vec<Arc<dyn TuiProfile>> =
                current.iter().filter(|p| !p.id().eq_ignore_ascii_case(id)).cloned().collect();
            next.push(profile.clone());
            next
        });
        Ok(())
    }

    pub fn remove_transient(&self, agent_type: &str) {
        self.transient.rcu(|current| {
            current.iter().filter(|p| !p.id().eq_ignore_ascii_case(agent_type)).cloned().collect::<Vec<_>>()
        });
    }

    /// Ids of the compiled-in profiles (reserved for definitions, REG-7).
    pub fn code_ids(&self) -> Vec<&'static str> {
        self.code.iter().map(|profile| profile.id()).collect()
    }

    /// Publish the code profiles (unchanged, first, in their fixed order) followed by `extra`
    /// (declarative profiles). An extra whose id collides with an earlier entry is dropped, so a
    /// definition can never replace a code profile (REG-7). One atomic swap (REG-2); readers
    /// holding the previous snapshot keep it.
    pub fn set_declarative(&self, extra: Vec<Arc<dyn TuiProfile>>) {
        let mut next = self.code.clone();
        for profile in extra {
            if !next.iter().any(|p| p.id().eq_ignore_ascii_case(profile.id())) {
                next.push(profile);
            }
        }
        self.swap(next);
    }

    /// Replace the whole snapshot atomically (SPEC REG-2).
    #[cfg_attr(not(test), allow(dead_code))] // the reload path (later phase) calls it
    pub fn swap(&self, profiles: Vec<Arc<dyn TuiProfile>>) {
        self.snapshot.store(Arc::new(profiles));
    }
}

/// Spawn-time gate for `agent_type` (APR-4, SEC-4): the reference to record on the run, or the
/// refusal (`AGENT_DEF_APPROVAL_STALE`, `AGENT_DEF_UNAPPROVED`, `AGENT_DEF_BINARY_CHANGED`,
/// `AGENT_NOT_INSTALLED`). Code profiles pass with no reference.
pub fn admit(
    agent_type: &str,
    project: &str,
) -> Result<Option<DefinitionRef>, crate::operations::OperationError> {
    profile_for_arc(agent_type)
        .spawn_gate(project)
        .map_err(|e| crate::operations::OperationError::new(e.code, e.message, false))
}

/// Why `agent_type` is not registered although a definition for it exists on disk: the specific
/// SPEC 7.4 refusal (`AGENT_DEF_APPROVAL_STALE`, `AGENT_DEF_UNAPPROVED`, `AGENT_DEF_BINARY_CHANGED`)
/// instead of the generic "cannot run delegated work". `None` when the id is registered or truly
/// unknown (those keep their own errors).
pub fn unavailable(agent_type: &str) -> Option<crate::operations::OperationError> {
    use super::declarative::loader::DefState;
    if crate::tui::is_registered(agent_type) {
        return None;
    }
    let entry = super::declarative::loader::last_entry_for(agent_type)?;
    let id = entry.id.clone().unwrap_or_else(|| agent_type.to_string());
    let (code, message) = match entry.state {
        DefState::ApprovalStale => (
            "AGENT_DEF_APPROVAL_STALE",
            format!("the definition of {id} changed after it was approved ({}); review and approve it again in Settings > Agents before starting new runs. Runs already started are not affected.", entry.path.display()),
        ),
        DefState::NeedsApproval => (
            "AGENT_DEF_UNAPPROVED",
            format!("the definition of {id} ({}) has not been approved; approve it in Settings > Agents before running it.", entry.path.display()),
        ),
        _ if entry.binary_changed => (
            "AGENT_DEF_BINARY_CHANGED",
            format!("the program for {id} now resolves to a different file than the one approved; approve it again in Settings > Agents."),
        ),
        _ => return None,
    };
    Some(crate::operations::OperationError::new(code, message, false))
}

/// Serializes tests that swap profiles into the process-wide registry with tests that assert on
/// its exact contents. Hold the guard for the whole swap window / assertion.
///
/// Re-entrant: `tui::profiles()` (the test helper that reads the process-wide registry) takes it
/// too, so a test that already holds the guard can still call `profiles()`, while one that does
/// not can never read the registry in the middle of another test's swap window.
#[cfg(test)]
pub(crate) fn test_serial() -> parking_lot::ReentrantMutexGuard<'static, ()> {
    static LOCK: OnceLock<parking_lot::ReentrantMutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| parking_lot::ReentrantMutex::new(())).lock()
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

    fn entry(id: &str, state: super::super::declarative::loader::DefState, binary_changed: bool) -> super::super::declarative::loader::Entry {
        super::super::declarative::loader::Entry {
            path: std::path::PathBuf::from(format!("C:/agents/{id}.toml")),
            id: Some(id.into()),
            source: "user",
            state,
            approval: None,
            fingerprint: None,
            errors: Vec::new(),
            shadowed_by: None,
            meta: None,
            binary_changed,
        }
    }

    /// A stale/unapproved/binary-changed definition is refused with its own code, not the generic
    /// "cannot run delegated work"; unknown ids and registered ids are not this function's business.
    #[test]
    fn unavailable_names_the_specific_refusal() {
        use super::super::declarative::loader::{last_entries, store_last, DefState};
        let _serial = test_serial();
        let saved = last_entries();
        store_last(vec![
            entry("zz_stale", DefState::ApprovalStale, false),
            entry("zz_new", DefState::NeedsApproval, false),
            entry("zz_bin", DefState::Invalid, true),
        ]);
        let stale = unavailable("ZZ_STALE").expect("stale is refused");
        assert_eq!(stale.code, "AGENT_DEF_APPROVAL_STALE");
        assert!(stale.message.contains("approve it again") && !stale.message.contains("cannot run delegated work"), "{}", stale.message);
        assert_eq!(unavailable("zz_new").unwrap().code, "AGENT_DEF_UNAPPROVED");
        assert_eq!(unavailable("zz_bin").unwrap().code, "AGENT_DEF_BINARY_CHANGED");
        assert!(unavailable("zz_nothing").is_none());
        assert!(unavailable("claude").is_none());
        store_last(saved);
    }

    /// `seeded_in_code_order` compared a fresh registry with the process-wide one and failed once
    /// at 64 test threads: another test (`bridge::dispatch`, `pty::registry`) had its probe
    /// profile swapped into the process-wide registry at that moment. The helper that reads the
    /// registry must wait out a swap window instead of seeing the half-way state.
    #[test]
    fn reading_the_process_wide_registry_waits_out_another_tests_swap_window() {
        /// Puts the registry back even when an assertion below fails.
        struct Restore(Arc<Vec<Arc<dyn TuiProfile>>>);
        impl Drop for Restore {
            fn drop(&mut self) {
                registry().swap(self.0.iter().cloned().collect());
            }
        }
        let serial = test_serial();
        let before = snapshot();
        let restore = Restore(before.clone());
        let mut with_probe: Vec<Arc<dyn TuiProfile>> = before.iter().cloned().collect();
        with_probe.push(declarative("zz_probe_window"));
        registry().swap(with_probe);

        let (sender, receiver) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let ids: Vec<String> = profiles().iter().map(|p| p.id().to_string()).collect();
            sender.send(ids).unwrap();
        });
        // While the window is open the reader must not get an answer.
        assert!(
            receiver.recv_timeout(std::time::Duration::from_millis(300)).is_err(),
            "profiles() returned while a swap window was open"
        );
        drop(restore);
        drop(serial);
        let ids = receiver.recv_timeout(std::time::Duration::from_secs(10)).expect("reader finishes after the window");
        reader.join().unwrap();
        assert!(!ids.iter().any(|id| id == "zz_probe_window"), "{ids:?}");
        assert_eq!(ids.len(), 9);
    }

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

    fn declarative(id: &str) -> Arc<dyn TuiProfile> {
        let text = format!(
            "schema_version = 1\n[agent]\nid = \"{id}\"\nlabel = \"T\"\n[binary]\nnames = [\"t\"]\n[headless]\nargv = []\n"
        );
        let def = crate::tui::declarative::parse_definition(&text).unwrap();
        Arc::new(crate::tui::declarative::DeclarativeProfile::new(def).unwrap())
    }

    #[test]
    fn reload_adds_and_removes_declarative() {
        let registry = Registry::seeded();
        let code: Vec<&str> = registry.ordered().iter().map(|p| p.id()).collect();
        let before = registry.profile_for_arc("claude");
        assert_eq!(registry.code_ids(), code);

        registry.set_declarative(vec![declarative("alpha_x"), declarative("beta_x")]);
        let ids: Vec<&str> = registry.ordered().iter().map(|p| p.id()).collect();
        assert_eq!(&ids[..9], &code[..], "code profiles stay first, in order");
        assert_eq!(&ids[9..], ["alpha_x", "beta_x"]);
        assert!(Arc::ptr_eq(&before, &registry.profile_for_arc("claude")), "code profiles are the same Arcs");
        assert_eq!(registry.profile_for_arc("ALPHA_X").id(), "alpha_x");

        let held = registry.snapshot();
        registry.set_declarative(vec![declarative("beta_x")]);
        assert_eq!(registry.profile_for_arc("alpha_x").id(), "unknown");
        assert_eq!(held.len(), 11, "an earlier snapshot is unchanged");
        assert_eq!(registry.snapshot().len(), 10);

        registry.set_declarative(Vec::new());
        assert_eq!(registry.snapshot().len(), 9);
    }

    #[test]
    fn a_declarative_profile_cannot_replace_a_code_profile() {
        let registry = Registry::seeded();
        registry.set_declarative(vec![declarative("claude"), declarative("dup_x"), declarative("dup_x")]);
        let ids: Vec<&str> = registry.ordered().iter().map(|p| p.id()).collect();
        assert_eq!(ids.iter().filter(|id| **id == "claude").count(), 1);
        assert_eq!(ids.iter().filter(|id| **id == "dup_x").count(), 1);
        assert!(registry.profile_for_arc("claude").features().read_only_headless);
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
