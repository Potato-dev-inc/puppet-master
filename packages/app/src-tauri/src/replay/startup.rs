//! The startup-gate decision, stepped screen by screen.
//!
//! `bridge::approvals::wait_for_startup_permission_resolution` is the real decision: it parks a run
//! on a startup dialog (folder trust, sign-in) and says what ended the wait. It is a blocking loop
//! over the operation store and real time, so it cannot be driven by a virtual clock. This module
//! repeats its order of checks one screen at a time, calling the same functions it calls:
//!
//! 1. remember the gate when the screen shows one (`pane_prompt::startup_gate`);
//! 2. the pane's process is gone (`error`) while a gate was seen: declined;
//! 3. a gate was seen and the screen is now the host shell's bare prompt
//!    (`tui::agent_gone_to_host_shell`): declined, the agent exited and the shell stayed;
//! 4. a prompt or startup screen is still up (`pane_prompt::detect_in_screen`,
//!    `TuiProfile::startup_gate`): hold;
//! 5. otherwise the run is released.
//!
//! `ReplayPane`'s `typing` lines come from `pane_prompt::dispatch_blocker`, the last check before a
//! task is typed, so a cast also shows whether the task could have reached the pane.

use super::pane::ReplayPane;
use crate::pty::status::PaneStatus;
use crate::tui::profile_for;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// A dialog is up; the run waits for the user.
    Hold(String),
    /// The dialog was declined: the run ends with this error code.
    Declined(&'static str),
    /// The pane died for a reason that is not a declined dialog.
    ReadinessFailed,
    /// Nothing blocks any more; the task may be typed.
    Released,
}

impl Verdict {
    pub fn describe(&self) -> String {
        match self {
            Verdict::Hold(gate) => format!("hold ({gate})"),
            Verdict::Declined(code) => format!("run ends: {code}"),
            Verdict::ReadinessFailed => "run ends: PANE_READINESS_FAILED".to_string(),
            Verdict::Released => "released".to_string(),
        }
    }
}

pub struct StartupWatch {
    agent: String,
    gate: Option<Value>,
}

impl StartupWatch {
    pub fn new(agent: &str) -> Self {
        Self { agent: agent.to_string(), gate: None }
    }

    pub fn step(&mut self, screen: &str, status: PaneStatus) -> Verdict {
        if let Some(seen) = crate::pane_prompt::startup_gate(&self.agent, screen) {
            self.gate = Some(seen);
        }
        let gate_name = || {
            self.gate
                .as_ref()
                .and_then(|g| g.get("kind").and_then(Value::as_str))
                .unwrap_or("gate")
                .to_string()
        };
        if status == PaneStatus::Error {
            return if self.gate.is_some() { Verdict::Declined("WORKSPACE_TRUST_DECLINED") } else { Verdict::ReadinessFailed };
        }
        if self.gate.is_some() && crate::tui::agent_gone_to_host_shell(&self.agent, screen) {
            return Verdict::Declined("WORKSPACE_TRUST_DECLINED");
        }
        let blocked = crate::pane_prompt::detect_in_screen(&self.agent, screen, false).is_some()
            || profile_for(&self.agent).startup_gate(screen).is_some();
        if blocked {
            Verdict::Hold(gate_name())
        } else {
            Verdict::Released
        }
    }
}

/// Plays `pane` forward in `step`s from `from` (when the run starts waiting) up to `until`, logging
/// the startup verdict whenever it changes. Returns the last one.
pub fn replay_startup(pane: &mut ReplayPane, from: Duration, step: Duration, until: Duration) -> Verdict {
    let agent = pane.cast().meta.agent.clone();
    let mut watch = StartupWatch::new(&agent);
    let mut last: Option<Verdict> = None;
    let mut at = from;
    while at <= until {
        pane.run_to(at);
        let verdict = watch.step(&pane.screen_text(), pane.status());
        if last.as_ref() != Some(&verdict) {
            pane.note("startup", verdict.describe());
            last = Some(verdict);
        }
        at += step;
    }
    last.expect("at least one step")
}
