//! Data the canary keeps and shows: one record per agent, the result of its last check, and the
//! plain-language view the UI renders. Serde only; the rules live in `state.rs`.

use super::version::Standing;
use serde::{Deserialize, Serialize};

/// How a canary run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The CLI reached its input box through the profile's own gate rules; nothing was sent to it.
    Ready,
    /// The CLI stopped at a screen the profile recognises but only the person may pass (folder
    /// trust, sign-in). The screens are understood; the check cannot go further without them.
    AsksYou,
    /// The screen held still and the profile recognised neither a gate nor its ready screen.
    /// This is what "the CLI changed its screens" looks like.
    Unrecognised,
    /// Nothing was drawn within the time allowed. A slow machine or a hung CLI, not proof of a change.
    NeverDrew,
    /// The CLI quit by itself before reaching its input box.
    Exited,
    /// The check could not start the CLI at all.
    StartFailed,
    /// This agent has no ready screen the check can read, so it cannot be judged.
    Inconclusive,
}

impl Outcome {
    /// The screens were understood (or at least no sign they changed).
    pub fn passed(self) -> bool {
        matches!(self, Outcome::Ready | Outcome::AsksYou)
    }

    /// Evidence that the CLI's screens no longer match its profile. Only this puts an agent in
    /// strict mode: the other non-passing outcomes say nothing about the screens.
    pub fn changed_screens(self) -> bool {
        matches!(self, Outcome::Unrecognised)
    }

    /// Not proof of a change alone, but the same failure twice in a row on one version is.
    /// `StartFailed` and `Inconclusive` say nothing about the screens, so they never count.
    pub fn counts_when_repeated(self) -> bool {
        matches!(self, Outcome::NeverDrew | Outcome::Exited)
    }
}

/// One screen the check saw, already redacted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shot {
    /// When it settled, in plain words ("after 3s").
    pub label: String,
    pub screen: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanaryResult {
    pub agent: String,
    /// The CLI version the check ran against (`0.162.1`), when it could be read.
    pub version: Option<String>,
    pub at_ms: i64,
    pub duration_ms: u64,
    pub outcome: Outcome,
    pub passed: bool,
    /// One plain sentence.
    pub summary: String,
    /// Id of the gate the run stopped at (`AsksYou`) or got stuck on.
    pub gate: Option<String>,
    /// Gates closed through the profile's own dismiss keys, in order.
    pub dismissed: Vec<String>,
    /// Every byte string written to the CLI. Only gate dismiss keys can appear here.
    pub keys_sent: Vec<String>,
    /// Prompts typed into the CLI. Always 0: the check never sends a task, so it spends no tokens.
    pub prompts_sent: u32,
    pub screens: Vec<Shot>,
}

/// What an Inbox card says about a CLI update.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    /// The screens changed; Puppet Master asks before acting. (Every card from before this field.)
    #[default]
    Changed,
    /// The update was checked on this computer and everything read fine.
    Good,
}

/// An info card for the Inbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub id: String,
    pub agent: String,
    pub text: String,
    pub at_ms: i64,
    #[serde(default)]
    pub dismissed: bool,
    #[serde(default)]
    pub kind: NoticeKind,
}

/// Something seen while the app ran that hints a CLI's screens changed (`canary::note_signal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// A live pane stayed on a screen its profile cannot read.
    UnreadableScreen,
    /// A run could not type its task: the input box never showed.
    NotReady,
    /// A turn went quiet and the screen did not say whether it finished.
    AmbiguousIdle,
}

impl Signal {
    pub fn as_str(self) -> &'static str {
        match self {
            Signal::UnreadableScreen => "unreadable_screen",
            Signal::NotReady => "not_ready",
            Signal::AmbiguousIdle => "ambiguous_idle",
        }
    }
}

/// Checks that failed the same way, one after another, on one version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failing {
    pub version: String,
    pub outcome: Outcome,
    pub count: u32,
}

/// Every field after `notice` is newer than the first `canary.json`; each has a default, so an old
/// file still loads.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentRecord {
    /// The version the last probe read (the first line of `--version`, trimmed).
    pub version: Option<String>,
    pub probed_ms: i64,
    pub last_result: Option<CanaryResult>,
    pub notice: Option<Notice>,
    /// The version before the last change (`2.3.0`), so the UI can say "2.3.0 to 2.4.0".
    #[serde(default)]
    pub previous_version: Option<String>,
    /// When that change was seen.
    #[serde(default)]
    pub changed_ms: i64,
    /// Versions a check passed on, on this computer (exact labels, `0.171.0`).
    #[serde(default)]
    pub verified: Vec<String>,
    /// The version the screens were found changed on (one check, or two failures alike).
    #[serde(default)]
    pub screens_changed: Option<String>,
    /// Failed checks of one kind in a row (`Outcome::counts_when_repeated`).
    #[serde(default)]
    pub failing: Option<Failing>,
    /// The version a runtime signal already started a check for: one per agent and version.
    #[serde(default)]
    pub signal_checked: Option<String>,
    /// Path, size and modified time of the program at the last probe.
    #[serde(default)]
    pub binary: Option<String>,
}

fn yes() -> bool {
    true
}

/// What is written to `canary.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Persisted {
    /// Check an agent by itself when its version changes (Settings can turn it off).
    #[serde(default = "yes")]
    pub auto_check: bool,
    #[serde(default)]
    pub agents: std::collections::BTreeMap<String, AgentRecord>,
}

impl Default for Persisted {
    fn default() -> Self {
        Self { auto_check: true, agents: Default::default() }
    }
}

/// Why an agent is in strict mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StrictReason {
    /// The installed version is outside the profile's tested range.
    UntestedVersion,
    /// The last check on this version found screens the profile does not recognise.
    ChangedScreens,
}

/// What the profile and the machine say about one agent right now.
#[derive(Debug, Clone)]
pub struct Facts {
    pub id: String,
    pub label: String,
    pub installed: bool,
    pub tested: Option<String>,
}

/// One agent as the UI shows it (Settings > Agents, the pane header, the Inbox).
#[derive(Debug, Clone, Serialize)]
pub struct AgentView {
    pub agent: String,
    pub label: String,
    pub installed: bool,
    pub version: Option<String>,
    pub tested: Option<String>,
    pub standing: Standing,
    pub strict: bool,
    pub strict_reason: Option<StrictReason>,
    /// The chip text on the pane header and in Settings; `None` when there is nothing to flag.
    pub chip: Option<String>,
    /// Plain sentence for the Settings row.
    pub plain: String,
    pub checking: bool,
    pub last: Option<ResultView>,
    /// Screens saved for this agent that Puppet Master did not recognise.
    pub captures: usize,
    /// A check passed on exactly this version, on this computer.
    pub verified: bool,
    /// The last version change and what became of it; `None` before the first change.
    pub change: Option<ChangeView>,
}

/// "Claude Code 2.3.0 -> 2.4.0: checked, all good."
#[derive(Debug, Clone, Serialize)]
pub struct ChangeView {
    pub from: String,
    pub to: String,
    pub at_ms: i64,
    pub verdict: Verdict,
    /// One short sentence.
    pub text: String,
}

/// How the newest version got on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// A check passed on this version.
    Good,
    /// The screens changed; Puppet Master asks before acting.
    Changed,
    /// No check has judged it (not run yet, switched off, or not possible for this agent).
    Unchecked,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResultView {
    pub outcome: Outcome,
    pub passed: bool,
    pub summary: String,
    pub at_ms: i64,
    pub version: Option<String>,
    pub screens: Vec<Shot>,
}

/// The whole picture for the UI.
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub agents: Vec<AgentView>,
    pub auto_check: bool,
    /// Info cards for the Inbox that were not dismissed.
    pub notices: Vec<Notice>,
    /// Total unrecognised screens saved, all agents.
    pub captures: usize,
}
