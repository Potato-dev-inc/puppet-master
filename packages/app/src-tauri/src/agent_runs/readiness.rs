//! The typing permit: Puppet Master types run input into a pane only while holding a permit.
//!
//! A run's task text (with its clear sequence and Enter) is written only with a [`TypingPermit`], and
//! a startup screen's dismiss keys only with a [`GatePermit`]. Both are issued here and nowhere else:
//! their fields are private and they have no public constructor, so no other module can build one
//! (the `compile_fail` examples below are run by `cargo test --doc`). A permit exists only for:
//!
//! - a [`Readiness::Ready`] verdict (typing) or a [`Readiness::Gate`] that declares its own dismiss
//!   keys (gate keys), computed by [`assess`] from one [`Observation`] of the pane;
//! - with the agent process not known to be gone (`pty::agent_proc`);
//! - and only while that observation is still the latest: the pane's write path refuses a permit's
//!   first write when output arrived after the screen the verdict was computed from (`admit`), so a
//!   menu that draws between the check and the keystroke is never typed into.
//!
//! Everything else is not a permit: an unknown screen keeps being looked at and then parks the run
//! with the screen and a plain explanation; it never types and never silently falls back.
//!
//! ```compile_fail,E0451
//! // A permit cannot be built from its parts outside this module.
//! let permit = puppet_master_app_lib::agent_runs::readiness::TypingPermit {
//!     pane_id: String::new(),
//!     output_pos: None,
//!     issued_at: std::time::Instant::now(),
//!     used: std::cell::Cell::new(false),
//! };
//! ```
//!
//! ```compile_fail,E0451
//! // Nor a gate permit (it would let any code press a gate's keys).
//! let permit = puppet_master_app_lib::agent_runs::readiness::GatePermit {
//!     pane_id: String::new(),
//!     keys: "\u{1b}",
//!     output_pos: None,
//!     used: std::cell::Cell::new(false),
//! };
//! ```
//!
//! ```compile_fail,E0451
//! // Nor a verdict that would hand one out.
//! use puppet_master_app_lib::agent_runs::readiness::{Basis, Readiness, Verdict};
//! let verdict = Verdict {
//!     readiness: Readiness::Ready,
//!     basis: Basis::Recognised,
//!     composer: puppet_master_app_lib::agent_runs::readiness::ComposerState::Empty,
//!     pane_id: String::new(),
//!     output_pos: None,
//!     screen: String::new(),
//!     why: "",
//! };
//! ```
//!
//! The types themselves are public (this compiles), so the examples above fail only because of the
//! private fields:
//!
//! ```
//! fn takes(_typing: &puppet_master_app_lib::agent_runs::readiness::TypingPermit,
//!          _gate: &puppet_master_app_lib::agent_runs::readiness::GatePermit,
//!          _verdict: &puppet_master_app_lib::agent_runs::readiness::Verdict) {}
//! ```

pub use crate::pty::agent_proc::AgentProcess;
pub use crate::tui::ComposerState;
use crate::tui::{Kind, StartupGate, TuiProfile};
use std::cell::Cell;
use std::time::{Duration, Instant};

/// What the pane's screen and process say about typing a task into it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    /// The agent's input box is there and takes a task.
    Ready,
    /// A turn is running.
    Busy,
    /// A startup screen or a dialog is up. One with `dismiss` keys may be closed with a
    /// [`GatePermit`]; any other waits for the user.
    Gate(StartupGate),
    /// The agent is not running in the pane; its host shell is (or will be) at a prompt.
    HostShell(HostShell),
    /// Nothing recognised: blank, a splash, a menu half drawn, a screen no profile rule knows.
    Unknown,
}

/// Why the pane is at its host shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostShell {
    /// The agent ran and exited (declined dialog, crash, `/exit`, killed).
    AgentExited,
    /// The host shell could not find the agent's command (not installed or not on PATH).
    NotInstalled(String),
}

/// How a `Ready` verdict was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// The profile's readiness recogniser saw its input box (`TuiProfile::ready_recogniser`).
    Recognised,
    /// The profile has no recogniser: something is drawn, with no gate, dialog, shell prompt or
    /// working marker on it (`READINESS_INFERRED`).
    Inferred,
}

/// The facts one look at a pane gives: its screen, how much output the screen includes (the pane's
/// stream position, `pty::stream_pos`, read under the same lock as the screen) and the process
/// side. Fakes in tests may leave the position and the process `None`/`Unknown`.
#[derive(Debug, Clone)]
pub struct Observation {
    pub pane_id: String,
    pub screen: String,
    pub output_pos: Option<u64>,
    pub agent: AgentProcess,
}

impl Observation {
    /// A screen and nothing else (no stream position, no process facts).
    pub fn screen_only(screen: String) -> Self {
        Self {
            pane_id: String::new(),
            screen,
            output_pos: None,
            agent: AgentProcess::Unknown,
        }
    }
}

/// A readiness verdict. Only [`assess`] makes one; permits are issued from it.
#[derive(Debug, Clone)]
pub struct Verdict {
    readiness: Readiness,
    basis: Basis,
    composer: ComposerState,
    pane_id: String,
    output_pos: Option<u64>,
    screen: String,
    why: &'static str,
}

impl Verdict {
    pub fn readiness(&self) -> &Readiness {
        &self.readiness
    }

    pub fn basis(&self) -> Basis {
        self.basis
    }

    /// What the profile read in the input box (`Unknown` for a profile without a recogniser).
    pub fn composer(&self) -> ComposerState {
        self.composer
    }

    pub fn screen(&self) -> &str {
        &self.screen
    }

    /// One plain sentence for why the pane is not ready (empty when it is).
    pub fn why(&self) -> &'static str {
        self.why
    }

    pub fn is_ready(&self) -> bool {
        self.readiness == Readiness::Ready
    }

    /// The gate on screen, if the verdict is one.
    pub fn gate(&self) -> Option<StartupGate> {
        match self.readiness {
            Readiness::Gate(gate) => Some(gate),
            _ => None,
        }
    }

    /// A permit to type run input, for a `Ready` verdict only.
    pub fn typing_permit(&self) -> Option<TypingPermit> {
        self.is_ready().then(|| TypingPermit {
            pane_id: self.pane_id.clone(),
            output_pos: self.output_pos,
            issued_at: Instant::now(),
            used: Cell::new(false),
        })
    }

    /// A permit to press a gate's own dismiss keys, for a gate that declares them only.
    pub fn gate_permit(&self) -> Option<GatePermit> {
        let Readiness::Gate(gate) = self.readiness else { return None };
        Some(GatePermit {
            pane_id: self.pane_id.clone(),
            keys: gate.dismiss?,
            output_pos: self.output_pos,
            used: Cell::new(false),
        })
    }
}

/// Permission to write one delivery's run input (the clear sequence, the task text, its Enter) into
/// one pane. Its first write must find the pane's output exactly where the verdict saw it; later
/// writes of the same delivery (Enter after the verified text) ride on it for [`PERMIT_MAX_AGE`].
#[derive(Debug)]
pub struct TypingPermit {
    pane_id: String,
    output_pos: Option<u64>,
    issued_at: Instant,
    used: Cell<bool>,
}

/// Permission to write one gate's declared dismiss keys, once.
#[derive(Debug)]
pub struct GatePermit {
    pane_id: String,
    keys: &'static str,
    output_pos: Option<u64>,
    used: Cell<bool>,
}

/// How long one typing permit covers a delivery (type, verify, Enter, one more Enter).
pub const PERMIT_MAX_AGE: Duration = Duration::from_secs(30);
/// Prefix of the write error for a permit whose screen is no longer the latest: look again.
pub const PERMIT_STALE: &str = "PERMIT_STALE";
/// Prefix of every other permit refusal.
pub const PERMIT_REFUSED: &str = "PERMIT_REFUSED";

impl TypingPermit {
    /// The pane write path's check (`pty::registry::write_input`): right pane, not too old, the
    /// agent not gone, and for the first write no output since the verdict.
    pub(crate) fn admit(
        &self,
        pane_id: &str,
        emitted_now: Option<u64>,
        agent: impl FnOnce() -> AgentProcess,
    ) -> Result<(), String> {
        if !self.pane_id.is_empty() && self.pane_id != pane_id {
            return Err(format!("{PERMIT_REFUSED}: the permit was issued for another pane"));
        }
        if self.issued_at.elapsed() > PERMIT_MAX_AGE {
            return Err(format!("{PERMIT_REFUSED}: the permit expired; the pane must be checked again"));
        }
        if !self.used.get() {
            if let (Some(seen), Some(now)) = (self.output_pos, emitted_now) {
                if now != seen {
                    return Err(format!(
                        "{PERMIT_STALE}: the pane printed {} more byte(s) after the readiness check",
                        now.saturating_sub(seen)
                    ));
                }
            }
        }
        // On every write, not only the first: a later Enter must never land in the shell that is
        // left when the agent died after its text went in.
        if agent() == AgentProcess::Exited {
            return Err(format!("{PERMIT_REFUSED}: the agent process has exited"));
        }
        self.used.set(true);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn pane_id(&self) -> &str {
        &self.pane_id
    }
}

impl GatePermit {
    /// The write path's check: right pane, exactly the declared keys, no Enter, once, and no output
    /// since the gate was seen.
    pub(crate) fn admit(&self, pane_id: &str, text: &str, append_newline: bool, emitted_now: Option<u64>) -> Result<(), String> {
        if !self.pane_id.is_empty() && self.pane_id != pane_id {
            return Err(format!("{PERMIT_REFUSED}: the gate permit was issued for another pane"));
        }
        if text != self.keys || append_newline {
            return Err(format!("{PERMIT_REFUSED}: a gate permit only covers the gate's own dismiss keys"));
        }
        if self.used.replace(true) {
            return Err(format!("{PERMIT_REFUSED}: a gate permit is used once"));
        }
        if let (Some(seen), Some(now)) = (self.output_pos, emitted_now) {
            if now != seen {
                return Err(format!("{PERMIT_STALE}: the screen changed after the gate was seen"));
            }
        }
        Ok(())
    }

    pub fn keys(&self) -> &'static str {
        self.keys
    }
}

/// The pseudo-gate for a dialog or question the profile detects (`detect_prompt`) that is not a
/// startup screen: only the user answers it.
pub const PROMPT_GATE: StartupGate = StartupGate {
    id: "prompt",
    summary: "The agent is asking a question in the pane",
    dismiss: None,
};

/// The readiness verdict for one observation of a pane running `profile`. Order matters, and every
/// step only ever moves AWAY from `Ready`:
///
/// 1. the process side: an agent known to have exited vetoes everything (`HostShell`);
/// 2. a blank screen is `Unknown`;
/// 3. a bare host-shell prompt where the agent was is `HostShell` (not installed when the shell
///    says it could not find the command);
/// 4. a startup screen (`startup_gate`), then a dialog (`detect_prompt`) is `Gate`;
/// 5. an agent the host shell has not started yet is `Unknown`;
/// 6. the profile's recogniser decides (`Empty` = `Ready`; text in the box is `Ready` only when the
///    profile can clear it; `Working` = `Busy`; anything else `Unknown`);
/// 7. a profile without a recogniser: `Busy` when its busy marker says so, else `Ready` (`Inferred`).
pub fn assess(profile: &dyn TuiProfile, observation: Observation) -> Verdict {
    let Observation { pane_id, screen, output_pos, agent } = observation;
    let verdict = |readiness, basis, composer, why| Verdict {
        readiness,
        basis,
        composer,
        pane_id: pane_id.clone(),
        output_pos,
        screen: screen.clone(),
        why,
    };
    let agent_pane = matches!(profile.features().kind, Kind::Cli | Kind::Unknown);
    if agent_pane {
        if let Some(command) = shell_not_found(&screen) {
            return verdict(
                Readiness::HostShell(HostShell::NotInstalled(command)),
                Basis::Recognised,
                ComposerState::Unknown,
                "the terminal could not find the agent's command",
            );
        }
    }
    if agent == AgentProcess::Exited {
        return verdict(
            Readiness::HostShell(HostShell::AgentExited),
            Basis::Recognised,
            ComposerState::Unknown,
            "the agent's process has exited",
        );
    }
    if screen.trim().is_empty() {
        return verdict(Readiness::Unknown, Basis::Recognised, ComposerState::Unknown, "the pane is blank");
    }
    if agent_pane && crate::tui::shell::host_prompt_visible(&screen) {
        return verdict(
            Readiness::HostShell(HostShell::AgentExited),
            Basis::Recognised,
            ComposerState::Unknown,
            "the terminal shows a shell prompt where the agent was",
        );
    }
    if let Some(gate) = profile.startup_gate(&screen) {
        return verdict(Readiness::Gate(gate), Basis::Recognised, ComposerState::Unknown, gate.summary);
    }
    if crate::pane_prompt::detect_in_screen(profile.id(), &screen, false).is_some() {
        return verdict(Readiness::Gate(PROMPT_GATE), Basis::Recognised, ComposerState::Unknown, PROMPT_GATE.summary);
    }
    if agent == AgentProcess::Starting {
        return verdict(Readiness::Unknown, Basis::Recognised, ComposerState::Unknown, "the agent has not started yet");
    }
    match profile.ready_recogniser(&screen) {
        Some(ComposerState::Empty) => verdict(Readiness::Ready, Basis::Recognised, ComposerState::Empty, ""),
        Some(ComposerState::HasText) if profile.clear_input_sequence().is_some() => {
            verdict(Readiness::Ready, Basis::Recognised, ComposerState::HasText, "")
        }
        Some(ComposerState::HasText) => verdict(
            Readiness::Unknown,
            Basis::Recognised,
            ComposerState::HasText,
            "the agent's input box already holds text that Puppet Master did not type",
        ),
        Some(ComposerState::Working) => {
            verdict(Readiness::Busy, Basis::Recognised, ComposerState::Working, "the agent is still working on a turn")
        }
        Some(ComposerState::Unknown) => verdict(
            Readiness::Unknown,
            Basis::Recognised,
            ComposerState::Unknown,
            "the screen does not show the agent's input box",
        ),
        None if profile.busy_marker(&screen) == Some(true) => {
            verdict(Readiness::Busy, Basis::Inferred, ComposerState::Unknown, "the agent is still working on a turn")
        }
        None => verdict(Readiness::Ready, Basis::Inferred, ComposerState::Unknown, ""),
    }
}

/// The command a host shell reported missing, when the screen ends at that shell's prompt.
fn shell_not_found(screen: &str) -> Option<String> {
    if !crate::tui::shell::host_prompt_visible(screen) {
        return None;
    }
    crate::tui::shell::command_not_found(screen)
}

/// Whether the profile reads readiness positively (it has a recogniser), as asked of a screen that
/// any recogniser answers (a blank one): the receipt stamp `READINESS_INFERRED` is for the others.
pub fn has_recogniser(profile: &dyn TuiProfile) -> bool {
    profile.ready_recogniser("").is_some()
}

/// The run error for a pane whose agent is not running, in plain words. `AGENT_EXITED` keeps the code
/// and shape the rest of the app uses (`pane_prompt::dispatch_blocker`).
pub fn host_shell_error(
    shell: &HostShell,
    profile: &dyn TuiProfile,
    pane_id: &str,
) -> crate::operations::OperationError {
    match shell {
        HostShell::AgentExited => {
            let mut error = crate::operations::OperationError::new(
                "AGENT_EXITED",
                "the agent in this pane has exited and the terminal shows a shell prompt; the task was not typed into the shell. Start a new run",
                false,
            );
            error.context = serde_json::json!({"pane_id": pane_id, "typed_bytes": 0});
            error
        }
        HostShell::NotInstalled(command) => {
            // The vendor's own installer first, then the profile's fallback route (npm), as the doctor shows them.
            let routes: Vec<String> = [profile.install_hint(), profile.install_fallback_hint()].into_iter().flatten().collect();
            let hint = (!routes.is_empty())
                .then(|| routes.join(", or "))
                .map(|hint| format!(" Install it ({hint}), or check that it is on PATH, then start the run again."))
                .unwrap_or_else(|| " Install it or check that it is on PATH, then start the run again.".into());
            let mut error = crate::operations::OperationError::new(
                "AGENT_NOT_INSTALLED",
                format!("The agent's command `{command}` was not found in this pane's terminal, so nothing was typed.{hint}"),
                false,
            );
            error.context = serde_json::json!({"pane_id": pane_id, "command": command, "typed_bytes": 0});
            error
        }
    }
}

/// How a parked run's wait ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParkEnd {
    /// The pane reads `Ready`, or shows a startup screen delivery closes itself: deliver again.
    Resume,
    /// The agent is gone; nothing is typed.
    Gone(HostShell),
    /// The user declined a startup dialog on this pane.
    Declined,
    /// The run is no longer parked (cancelled, timed out by its supervisor, answered elsewhere).
    Cancelled,
    /// The park ran out with the pane still not ready.
    TimedOut { why: String, screen: String },
}

/// The wait of a run parked on a screen that is not ready: one verdict per look until something
/// decides. A gate only the user may pass (a dialog) keeps the run waiting; nothing is typed here.
pub fn wait_while_parked(
    profile: &dyn TuiProfile,
    mut observe: impl FnMut() -> Observation,
    mut cancelled: impl FnMut() -> bool,
    mut declined: impl FnMut() -> bool,
    mut expired: impl FnMut() -> bool,
    mut pause: impl FnMut(),
) -> ParkEnd {
    loop {
        if cancelled() {
            return ParkEnd::Cancelled;
        }
        if declined() {
            return ParkEnd::Declined;
        }
        let verdict = assess(profile, observe());
        match verdict.readiness() {
            Readiness::Ready => return ParkEnd::Resume,
            Readiness::Gate(gate) if gate.dismiss.is_some() => return ParkEnd::Resume,
            Readiness::HostShell(shell) => return ParkEnd::Gone(shell.clone()),
            Readiness::Gate(_) | Readiness::Busy | Readiness::Unknown => {}
        }
        if expired() {
            return ParkEnd::TimedOut { why: verdict.why().to_string(), screen: verdict.screen().to_string() };
        }
        pause();
    }
}

/// The last rows of a screen, for a parked run's explanation.
pub fn screen_tail(screen: &str, rows: usize) -> String {
    let lines: Vec<&str> = screen.lines().map(str::trim_end).filter(|line| !line.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(rows)..].join("\n")
}

/// Code of a run parked and then ended because its pane never showed a recognised input box.
pub const SCREEN_NOT_READY: &str = "SCREEN_NOT_READY";

/// The run error for a screen that never became ready (after the park ran out).
pub fn not_ready_error(why: &str, screen: &str, pane_id: &str) -> crate::operations::OperationError {
    let mut error = crate::operations::OperationError::new(
        SCREEN_NOT_READY,
        format!(
            "Puppet Master did not type the task: {why}. Nothing was typed into the pane and the run did not switch to a background run. Get the agent to its input box, then start the run again."
        ),
        true,
    );
    error.context = serde_json::json!({"pane_id": pane_id, "screen": screen_tail(screen, 12), "typed_bytes": 0});
    error
}

#[cfg(test)]
#[path = "readiness_tests.rs"]
mod tests;
