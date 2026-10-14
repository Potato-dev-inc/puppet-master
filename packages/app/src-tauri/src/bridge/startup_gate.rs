//! Startup gates: screens a freshly started TUI shows before it takes a prompt (an update notice,
//! a trust question, a login page; see `TuiProfile::startup_gate`).
//!
//! Before a task is typed, a gate that its profile says is safe to close (a "skip" for a notice)
//! is closed with the keys the profile declares; the user is not asked about an update notice.
//! A gate without such keys is never answered here: the caller parks the run and shows it.
//!
//! The pane I/O is behind [`GateIo`] so the sequence is unit-tested without a PTY.
//!
//! Keys are pressed only with a `GatePermit` (`agent_runs::readiness`): issued for a gate the profile
//! declares, recognised on the latest screen, for exactly that gate's keys.
//!
//! Agent-agnostic: nothing here names an agent. Per-agent screens live in `tui/<agent>/screen.rs`.

use std::time::Duration;

use crate::agent_runs::readiness::{self, GatePermit, Observation, Readiness};
use crate::tui::{StartupGate, TuiProfile};

/// More than this many gates in a row is a loop, not a startup sequence.
const MAX_GATES: usize = 4;
/// How long a dismissed screen may take to change, as polls of [`POLL`].
const CHANGE_POLLS: u32 = 40;
const POLL: Duration = Duration::from_millis(100);
/// The screen must hold still this long after it changed before the next step.
const SETTLE: Duration = Duration::from_millis(300);

pub(crate) trait GateIo {
    fn screen(&mut self) -> String;
    /// The screen with the pane's output position and agent process, for a readiness verdict.
    fn observe(&mut self) -> Observation {
        Observation::screen_only(self.screen())
    }
    /// Presses the permit's keys (`GatePermit::keys`).
    fn write(&mut self, permit: &GatePermit) -> Result<(), String>;
    fn pause(&mut self, duration: Duration);
}

/// What the dismissal pass found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GateReport {
    /// Gates that were closed, in order (`StartupGate::id`).
    pub dismissed: Vec<&'static str>,
    /// The gate still on screen afterwards: one only the user may move past, or one that did not
    /// react to its dismiss keys.
    pub blocked: Option<StartupGate>,
}

/// Closes every dismissible gate on screen, one at a time, waiting for the screen to change after
/// each. Types nothing when no gate is showing.
pub(crate) fn dismiss_startup_gates(io: &mut dyn GateIo, profile: &dyn TuiProfile) -> GateReport {
    let mut report = GateReport {
        dismissed: Vec::new(),
        blocked: None,
    };
    for _ in 0..MAX_GATES {
        let verdict = readiness::assess(profile, io.observe());
        // Only startup screens are closed here; a dialog (the prompt pseudo-gate), a shell prompt or
        // an input box is somebody else's business.
        let gate = match verdict.readiness() {
            Readiness::Gate(gate) if gate.id != readiness::PROMPT_GATE.id => *gate,
            _ => return report,
        };
        let before = verdict.screen().to_string();
        let Some(permit) = verdict.gate_permit() else {
            report.blocked = Some(gate);
            return report;
        };
        match io.write(&permit) {
            Ok(()) => {}
            // The screen moved between the look and the keys: look again (counts as a round).
            Err(error) if error.starts_with(readiness::PERMIT_STALE) => continue,
            Err(_) => {
                report.blocked = Some(gate);
                return report;
            }
        }
        let mut changed = false;
        for _ in 0..CHANGE_POLLS {
            io.pause(POLL);
            if io.screen() != before {
                changed = true;
                break;
            }
        }
        if !changed {
            report.blocked = Some(gate);
            return report;
        }
        io.pause(SETTLE);
        report.dismissed.push(gate.id);
    }
    // Still a gate after the allowed number of rounds.
    let last = io.screen();
    report.blocked = profile.startup_gate(&last);
    report
}

struct PaneGateIo<'a> {
    registry: &'a std::sync::Arc<parking_lot::Mutex<crate::pty::PaneRegistry>>,
    app: &'a tauri::AppHandle,
    pane_id: &'a str,
}

impl GateIo for PaneGateIo<'_> {
    fn screen(&mut self) -> String {
        crate::pty::registry_read_snapshot(self.registry, self.pane_id).unwrap_or_default()
    }

    fn observe(&mut self) -> Observation {
        crate::agent_runs::delivery::observe_pane(self.registry, self.pane_id)
    }

    fn write(&mut self, permit: &GatePermit) -> Result<(), String> {
        crate::pty::registry_write_input(
            self.registry,
            self.app,
            self.pane_id,
            permit.keys(),
            false,
            false,
            None,
            crate::pty::WriteOrigin::Gate(permit),
        )
    }

    fn pause(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// [`dismiss_startup_gates`] on a live pane, recording what it closed in the operation transcript.
pub(super) fn dismiss_for_operation(
    operation: &crate::operations::OperationSnapshot,
    registry: &std::sync::Arc<parking_lot::Mutex<crate::pty::PaneRegistry>>,
    app: &tauri::AppHandle,
    pane_id: &str,
    agent_type: &str,
) -> GateReport {
    let profile = crate::tui::profile_for(agent_type);
    let mut io = PaneGateIo {
        registry,
        app,
        pane_id,
    };
    let report = dismiss_startup_gates(&mut io, profile.as_ref());
    if !report.dismissed.is_empty() {
        crate::agent_runs::transcript::append(
            &operation.project_path,
            &operation.operation_id,
            "startup",
            &format!(
                "closed startup screen before the task was typed: {}",
                report.dismissed.join(", ")
            ),
        );
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pane that shows `screens[0]`, then the next one after each accepted write.
    struct FakePane {
        screens: Vec<&'static str>,
        at: usize,
        written: Vec<String>,
        ignore_keys: bool,
    }

    impl GateIo for FakePane {
        fn screen(&mut self) -> String {
            self.screens[self.at].to_string()
        }
        fn write(&mut self, permit: &GatePermit) -> Result<(), String> {
            permit.admit("", permit.keys(), false, None)?;
            self.written.push(permit.keys().to_string());
            if !self.ignore_keys && self.at + 1 < self.screens.len() {
                self.at += 1;
            }
            Ok(())
        }
        fn pause(&mut self, _duration: Duration) {}
    }

    /// Profile for the tests: "NOTICE" is skippable with `s`, "TRUST" needs the user.
    struct Probe;
    impl TuiProfile for Probe {
        fn id(&self) -> &'static str {
            "gate_probe"
        }
        fn startup_gate(&self, screen: &str) -> Option<StartupGate> {
            if screen.contains("NOTICE") {
                Some(StartupGate {
                    id: "notice",
                    summary: "a notice",
                    dismiss: Some("s"),
                })
            } else if screen.contains("TRUST") {
                Some(StartupGate {
                    id: "trust",
                    summary: "trust",
                    dismiss: None,
                })
            } else {
                None
            }
        }
    }

    fn pane(screens: Vec<&'static str>) -> FakePane {
        FakePane {
            screens,
            at: 0,
            written: Vec::new(),
            ignore_keys: false,
        }
    }

    #[test]
    fn a_clear_screen_gets_no_keystrokes() {
        let mut pane = pane(vec!["READY"]);
        let report = dismiss_startup_gates(&mut pane, &Probe);
        assert_eq!(
            report,
            GateReport {
                dismissed: vec![],
                blocked: None
            }
        );
        assert!(pane.written.is_empty());
    }

    #[test]
    fn a_notice_is_closed_with_its_declared_keys_and_the_next_gate_is_still_checked() {
        let mut pane = pane(vec!["NOTICE one", "NOTICE two", "READY"]);
        let report = dismiss_startup_gates(&mut pane, &Probe);
        assert_eq!(report.dismissed, vec!["notice", "notice"]);
        assert_eq!(report.blocked, None);
        assert_eq!(pane.written, vec!["s", "s"]);
    }

    #[test]
    fn a_trust_question_is_never_answered() {
        let mut pane = pane(vec!["NOTICE", "TRUST dialog", "READY"]);
        let report = dismiss_startup_gates(&mut pane, &Probe);
        assert_eq!(report.dismissed, vec!["notice"]);
        assert_eq!(report.blocked.map(|gate| gate.id), Some("trust"));
        assert_eq!(pane.written, vec!["s"], "only the notice's own keys were written");
    }

    #[test]
    fn a_notice_that_ignores_its_keys_is_reported_as_blocking() {
        let mut pane = pane(vec!["NOTICE", "READY"]);
        pane.ignore_keys = true;
        let report = dismiss_startup_gates(&mut pane, &Probe);
        assert!(report.dismissed.is_empty());
        assert_eq!(report.blocked.map(|gate| gate.id), Some("notice"));
        assert_eq!(pane.written.len(), 1, "one attempt, not a key-mashing loop");
    }

    #[test]
    fn a_gate_that_keeps_reappearing_stops_after_a_few_rounds() {
        let mut pane = pane(vec![
            "NOTICE 1", "NOTICE 2", "NOTICE 3", "NOTICE 4", "NOTICE 5", "READY",
        ]);
        let report = dismiss_startup_gates(&mut pane, &Probe);
        assert_eq!(report.dismissed.len(), MAX_GATES);
        assert!(report.blocked.is_some());
    }

    #[test]
    fn the_default_profile_has_no_gates() {
        struct Plain;
        impl TuiProfile for Plain {
            fn id(&self) -> &'static str {
                "plain"
            }
        }
        let mut pane = pane(vec!["NOTICE"]);
        assert_eq!(dismiss_startup_gates(&mut pane, &Plain).blocked, None);
        assert!(pane.written.is_empty());
    }
}
