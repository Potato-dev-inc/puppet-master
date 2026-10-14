//! The zero-token check: drive a freshly started CLI to its input box using only what its profile
//! declares, and write down what it saw. Pure over [`CanaryIo`], so the whole sequence is tested
//! against recorded screens without a PTY or a clock.
//!
//! What it may send: the `dismiss` keys of a gate the profile recognises (a "skip" for a notice),
//! exactly as a run would. What it never sends: a task, a prompt, Enter after text, or an answer
//! to a trust or sign-in screen. So it cannot start a turn and spends no tokens. Every byte it
//! writes is listed in the result (`keys_sent`).
//!
//! Agent-agnostic: "ready" is the profile's own `standby_screen` / `composer_state`, and gates are
//! the profile's `startup_gate`. The caller decides which profile to pass.

use super::model::{CanaryResult, Outcome, Shot};
use crate::tui::{ComposerState, StandbyScreen, StartupGate, TuiProfile};
use std::time::Duration;

/// The CLI under test, as the check sees it.
pub trait CanaryIo {
    /// The visible screen as text.
    fn screen(&mut self) -> String;
    /// Raw bytes to the CLI's input.
    fn write(&mut self, keys: &str) -> Result<(), String>;
    /// Whether the CLI process is still running.
    fn alive(&mut self) -> bool;
    fn pause(&mut self, duration: Duration);
    /// Time since the CLI started.
    fn elapsed(&mut self) -> Duration;
}

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub poll: Duration,
    /// A screen must hold still this long before it is judged (a menu may draw a moment after the
    /// first paint; live, Codex 0.159.3 drew its update menu about a second in).
    pub settle: Duration,
    /// A ready screen must hold still this long before it counts.
    pub ready_still: Duration,
    /// A settled screen that is neither a gate nor ready for this long is "not recognised".
    pub stuck_after: Duration,
    /// Give up after this long in total.
    pub budget: Duration,
    /// How long a dismissed gate gets to change the screen.
    pub change_wait: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            poll: Duration::from_millis(250),
            settle: Duration::from_millis(1500),
            ready_still: Duration::from_millis(3000),
            stuck_after: Duration::from_secs(10),
            budget: Duration::from_secs(60),
            change_wait: Duration::from_secs(6),
        }
    }
}

/// More gates in a row than this is a loop, not a startup sequence.
const MAX_GATES: usize = 4;
const MAX_SHOTS: usize = 6;

/// What `drive` found, before it is dressed as a [`CanaryResult`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drive {
    pub outcome: Outcome,
    pub gate: Option<&'static str>,
    pub dismissed: Vec<&'static str>,
    pub keys_sent: Vec<String>,
    /// Raw (unredacted) screens in order of appearance; the caller redacts before keeping them.
    pub screens: Vec<(String, String)>,
    pub elapsed: Duration,
}

/// Can this profile's ready screen be read at all? A check that cannot recognise "ready" would
/// call every healthy start a failure, so such an agent is "not checkable" instead.
pub fn checkable(profile: &dyn TuiProfile) -> bool {
    let support = profile.standby_support();
    profile.reads_composer() || (support.visible && !support.experimental)
}

/// The profile positively reads this screen as an empty input box with nothing in front of it.
pub fn is_ready(profile: &dyn TuiProfile, screen: &str) -> bool {
    if profile.startup_gate(screen).is_some() {
        return false;
    }
    if profile.reads_composer() {
        return profile.composer_state(screen) == ComposerState::Empty;
    }
    profile.standby_screen(screen) == StandbyScreen::Ready
}

pub fn drive(io: &mut dyn CanaryIo, profile: &dyn TuiProfile, timing: &Timing) -> Drive {
    let mut out = Drive {
        outcome: Outcome::NeverDrew,
        gate: None,
        dismissed: Vec::new(),
        keys_sent: Vec::new(),
        screens: Vec::new(),
        elapsed: Duration::ZERO,
    };
    let mut last = String::new();
    let mut changed_at = Duration::ZERO;
    let mut rounds = 0usize;

    loop {
        let now = io.elapsed();
        out.elapsed = now;
        if now >= timing.budget {
            out.outcome = if last.trim().is_empty() { Outcome::NeverDrew } else { Outcome::Unrecognised };
            return out;
        }
        let screen = io.screen();
        if screen != last {
            if !screen.trim().is_empty() && out.screens.len() < MAX_SHOTS {
                out.screens.push((format!("after {}s", now.as_secs()), screen.clone()));
            }
            last = screen.clone();
            changed_at = now;
        }
        if !io.alive() {
            // Whatever it drew last is kept; a clean read of "ready" before it quit still counts.
            out.outcome = Outcome::Exited;
            return out;
        }
        if screen.trim().is_empty() {
            io.pause(timing.poll);
            continue;
        }
        let still = now.saturating_sub(changed_at);
        if still < timing.settle {
            io.pause(timing.poll);
            continue;
        }
        if crate::tui::agent_gone_to_host_shell(profile.id(), &screen) {
            // The agent ended and the terminal fell back to the shell that launched it.
            out.outcome = Outcome::Exited;
            return out;
        }
        match profile.startup_gate(&screen) {
            Some(gate) => {
                match gate.dismiss {
                    None => {
                        out.outcome = Outcome::AsksYou;
                        out.gate = Some(gate.id);
                        return out;
                    }
                    Some(keys) => {
                        if rounds >= MAX_GATES {
                            out.outcome = Outcome::Unrecognised;
                            out.gate = Some(gate.id);
                            return out;
                        }
                        rounds += 1;
                        if io.write(keys).is_err() {
                            out.outcome = Outcome::Exited;
                            out.gate = Some(gate.id);
                            return out;
                        }
                        out.keys_sent.push(format!("{}: {}", gate.id, printable(keys)));
                        out.dismissed.push(gate.id);
                        let waited_from = io.elapsed();
                        let mut moved = false;
                        while io.elapsed().saturating_sub(waited_from) < timing.change_wait {
                            io.pause(timing.poll);
                            if io.screen() != screen {
                                moved = true;
                                break;
                            }
                        }
                        if !moved {
                            // The keys did nothing: the gate is not what the profile thinks it is.
                            out.outcome = Outcome::Unrecognised;
                            out.gate = Some(gate.id);
                            return out;
                        }
                        continue;
                    }
                }
            }
            None => {
                if is_ready(profile, &screen) {
                    if still >= timing.ready_still {
                        out.outcome = Outcome::Ready;
                        return out;
                    }
                } else if still >= timing.stuck_after {
                    out.outcome = Outcome::Unrecognised;
                    return out;
                }
            }
        }
        io.pause(timing.poll);
    }
}

/// Keys as the person would read them in the result (`\r` -> `<Enter>`).
fn printable(keys: &str) -> String {
    keys.chars()
        .map(|c| match c {
            '\r' => "<Enter>".to_string(),
            '\n' => "<LF>".to_string(),
            '\x1b' => "<Esc>".to_string(),
            '\t' => "<Tab>".to_string(),
            c if c.is_control() => format!("<0x{:02x}>", c as u32),
            c => c.to_string(),
        })
        .collect()
}

/// Plain sentence for an outcome. `gate` is the recognised screen's one-line summary.
pub fn summary(outcome: Outcome, gate: Option<&StartupGate>, label: &str) -> String {
    match outcome {
        Outcome::Ready => format!("{label} started and reached its input box. Nothing was sent to it."),
        Outcome::AsksYou => match gate {
            Some(gate) => format!("{label} stopped at a question only you can answer: {}. Its screens still read fine.", gate.summary),
            None => format!("{label} stopped at a question only you can answer. Its screens still read fine."),
        },
        Outcome::Unrecognised => format!("{label} showed a screen Puppet Master does not recognise."),
        Outcome::NeverDrew => format!("{label} did not draw anything in time. Try again."),
        Outcome::Exited => format!("{label} closed by itself while starting."),
        Outcome::StartFailed => format!("Could not start {label} for the check."),
        Outcome::Inconclusive => format!("{label} has no ready screen Puppet Master can check."),
    }
}

/// A [`Drive`] as the stored result. `redact` runs on every screen before it is kept.
pub fn into_result(
    drive: Drive,
    agent: &str,
    label: &str,
    version: Option<String>,
    at_ms: i64,
    profile: &dyn TuiProfile,
    redact: &dyn Fn(&str) -> String,
) -> CanaryResult {
    let gate = drive.gate.and_then(|id| {
        // The summary of the gate that stopped the run, read back from the last screen it saw.
        drive.screens.last().and_then(|(_, screen)| profile.startup_gate(screen)).filter(|g| g.id == id)
    });
    CanaryResult {
        agent: agent.to_string(),
        version,
        at_ms,
        duration_ms: drive.elapsed.as_millis() as u64,
        outcome: drive.outcome,
        passed: drive.outcome.passed(),
        summary: summary(drive.outcome, gate.as_ref(), label),
        gate: drive.gate.map(str::to_string),
        dismissed: drive.dismissed.iter().map(|id| id.to_string()).collect(),
        keys_sent: drive.keys_sent,
        prompts_sent: 0,
        screens: drive
            .screens
            .into_iter()
            .map(|(label, screen)| Shot { label, screen: redact(&screen) })
            .collect(),
    }
}
