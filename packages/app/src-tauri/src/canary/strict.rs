//! Strict mode: the set of agents Puppet Master must not guess for.
//!
//! The set is replaced as a whole whenever a version probe or a check changes the picture
//! (`canary::refresh_strict`). Reads are cheap (a read lock on a small set) because the gate
//! check runs on every poll of a delivery.
//!
//! What strict mode does to a gate: the profile still RECOGNISES it, so a run parks and shows it,
//! but the gate no longer carries `dismiss` keys, so nothing presses them for the person.

use crate::tui::{StartupGate, TuiProfile};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::collections::HashSet;

static STRICT: Lazy<RwLock<HashSet<String>>> = Lazy::new(Default::default);

fn key(agent: &str) -> String {
    agent.to_ascii_lowercase()
}

pub fn is_strict(agent: &str) -> bool {
    let set = STRICT.read();
    !set.is_empty() && set.contains(&key(agent))
}

/// Replace the whole set; returns the agents that just entered strict mode.
pub fn replace(agents: impl IntoIterator<Item = String>) -> Vec<String> {
    let next: HashSet<String> = agents.into_iter().map(|a| key(&a)).collect();
    let mut set = STRICT.write();
    let entered = next.difference(&set).cloned().collect();
    *set = next;
    entered
}

pub fn list() -> Vec<String> {
    let mut agents: Vec<String> = STRICT.read().iter().cloned().collect();
    agents.sort();
    agents
}

/// `profile.startup_gate(screen)` as the live dismissal paths must see it: in strict mode a
/// notice that would be closed for the person is handed back without its keys, so the run parks
/// on it instead. The check runs here, not in each profile, so no agent can forget it.
pub fn startup_gate(profile: &dyn TuiProfile, screen: &str) -> Option<StartupGate> {
    let gate = profile.startup_gate(screen)?;
    if gate.dismiss.is_some() && is_strict(profile.id()) {
        return Some(StartupGate { dismiss: None, ..gate });
    }
    Some(gate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::{Features, TuiProfile};

    /// A profile with a dismissible notice, under an id no other test uses.
    struct Noisy(&'static str);
    impl TuiProfile for Noisy {
        fn id(&self) -> &'static str {
            self.0
        }
        fn features(&self) -> Features {
            Features::CLI
        }
        fn startup_gate(&self, screen: &str) -> Option<StartupGate> {
            screen.contains("update").then_some(StartupGate { id: "update_available", summary: "an update", dismiss: Some("3") })
        }
    }

    #[test]
    fn outside_strict_mode_the_gate_keeps_its_dismiss_keys() {
        let _lock = crate::canary::test_lock();
        replace(Vec::<String>::new());
        let gate = startup_gate(&Noisy("strict_test_free"), "an update is out").unwrap();
        assert_eq!(gate.dismiss, Some("3"));
    }

    #[test]
    fn in_strict_mode_the_gate_is_still_seen_but_has_no_keys() {
        let _lock = crate::canary::test_lock();
        replace(["Strict_Test_Held".to_string()]);
        let gate = startup_gate(&Noisy("strict_test_held"), "an update is out").unwrap();
        assert_eq!(gate.id, "update_available", "the gate is still recognised");
        assert_eq!(gate.dismiss, None, "but nothing may close it for the person");
        assert!(startup_gate(&Noisy("strict_test_held"), "all quiet").is_none());
        // Another agent is unaffected.
        assert_eq!(startup_gate(&Noisy("strict_test_other"), "an update").unwrap().dismiss, Some("3"));
        replace(Vec::<String>::new());
    }

    #[test]
    fn replace_reports_who_just_went_strict() {
        let _lock = crate::canary::test_lock();
        replace(Vec::<String>::new());
        let entered = replace(["a_strict_test".to_string()]);
        assert!(entered.contains(&"a_strict_test".to_string()));
        let again = replace(["a_strict_test".to_string(), "b_strict_test".to_string()]);
        assert_eq!(again, vec!["b_strict_test".to_string()]);
        replace(Vec::<String>::new());
    }
}
