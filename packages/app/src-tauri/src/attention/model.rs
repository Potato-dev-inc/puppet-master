//! The attention vocabulary: what can need a person, which lane it sits in, how it is ordered and
//! what the wire snapshot looks like. Pure data, no I/O. See `docs/features/attention.md`.

use serde::{Deserialize, Serialize};

/// What kind of thing needs (or will soon need) the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// An agent wants to control a pane outside its lineage, or answer above low risk.
    Consent,
    /// A pane is stopped at a permission prompt.
    Approval,
    /// A pane is stopped at a startup gate only the person may pass (folder trust, sign-in).
    TrustGate,
    /// An agent asked something through `ask_user`.
    Question,
    Tamper,
    Budget,
    Limit,
    Setup,
    /// A pane's process hit an error.
    Error,
    /// The loop guard thinks a pane is stuck.
    Stuck,
    /// A finished mission, a separate copy ready to merge, failed PR checks.
    Review,
    /// A run finished that nobody has looked at.
    Done,
    /// An Ask-bar answer that arrived while the bar was hidden.
    Answer,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Consent => "consent",
            Self::Approval => "approval",
            Self::TrustGate => "trust_gate",
            Self::Question => "question",
            Self::Tamper => "tamper",
            Self::Budget => "budget",
            Self::Limit => "limit",
            Self::Setup => "setup",
            Self::Error => "error",
            Self::Stuck => "stuck",
            Self::Review => "review",
            Self::Done => "done",
            Self::Answer => "answer",
        }
    }

    /// Kinds that describe the state of one pane (a pane shows one of them at a time).
    pub fn is_pane_state(self) -> bool {
        matches!(self, Self::TrustGate | Self::Approval | Self::Error | Self::Stuck)
    }
}

/// `blocking`: an agent cannot go on until a person acts, and only these are counted.
/// `ready`: something waits for you but nothing is blocked; no number is shown for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    Blocking,
    Ready,
}

/// The one place that says which lane a kind belongs to.
pub fn lane_of(kind: Kind) -> Lane {
    match kind {
        Kind::Review | Kind::Done | Kind::Answer => Lane::Ready,
        _ => Lane::Blocking,
    }
}

/// How worried the prompt reading is. `None` on an item means the profile read no risk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Low,
    Medium,
    High,
}

impl Level {
    /// `low | medium | high`, exactly the three levels `deriveNeedsYou` accepts.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// The words every surface uses: the desktop card, the phone card and the push.
    pub fn word(self) -> &'static str {
        match self {
            Self::Low => "Low risk",
            Self::Medium => "Check first",
            Self::High => "High risk",
        }
    }
}

impl From<crate::prompt_risk::RiskLevel> for Level {
    fn from(level: crate::prompt_risk::RiskLevel) -> Self {
        match level {
            crate::prompt_risk::RiskLevel::Low => Self::Low,
            crate::prompt_risk::RiskLevel::Medium => Self::Medium,
            crate::prompt_risk::RiskLevel::High => Self::High,
        }
    }
}

/// Lower comes first. High risk and consent first, then calmer approvals, questions, then the rest;
/// ties are broken by age (oldest first) and id.
pub fn rank_of(kind: Kind, risk: Option<Level>) -> u8 {
    match kind {
        Kind::Consent | Kind::TrustGate => 0,
        Kind::Approval => match risk {
            Some(Level::High) => 0,
            Some(Level::Low) => 2,
            // Medium, or no risk read at all: look before allowing.
            _ => 1,
        },
        Kind::Question => 2,
        Kind::Tamper | Kind::Budget | Kind::Limit | Kind::Setup => 3,
        Kind::Error | Kind::Stuck => 4,
        Kind::Review => 10,
        Kind::Done => 11,
        Kind::Answer => 12,
    }
}

/// Tombstones stay in the snapshot this long, so a card that is on screen can say what happened.
pub const TOMBSTONE_MS: u64 = 15_000;
/// A finished run nobody opened stops being offered after a day.
pub const DONE_TTL_MS: u64 = 86_400_000;
/// A consent request nobody answered is withdrawn after ten minutes.
pub const CONSENT_TTL_MS: u64 = 600_000;
/// Bursts of screen updates produce one snapshot.
pub const SNAPSHOT_DEBOUNCE_MS: u64 = 120;
/// The snapshot never lists more than this; the oldest ready items go first, blocking never.
pub const MAX_ITEMS: usize = 200;

// ---------------------------------------------------------------------------------------------
// Inputs

/// Everything `derive` looks at. The shape (camelCase) is the shape `AttentionInput` has in
/// `lib/attention.ts`, so the shared parity corpus can feed both implementations.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    pub panes: Vec<PaneFact>,
    /// Detected prompt per pane. Supplies the risk of an approval.
    #[serde(default)]
    pub prompts: std::collections::HashMap<String, Option<PromptFact>>,
    /// Live (pending, not expired) questions.
    #[serde(default)]
    pub questions: Vec<QuestionFact>,
    /// Panes the loop guard flagged. Unknown panes are ignored.
    #[serde(default)]
    pub stuck_pane_ids: Vec<String>,
    /// Ask-bar answers nobody saw yet.
    #[serde(default)]
    pub notices: Vec<NoticeFact>,
    /// When each pane entered its current status.
    #[serde(default)]
    pub status_since: std::collections::HashMap<String, u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneFact {
    pub id: String,
    pub status: String,
    /// The person's name for the pane (or its auto title), else the numbered agent label.
    #[serde(default)]
    pub heading: Option<String>,
    #[serde(default)]
    pub agent_label: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptFact {
    #[serde(default)]
    pub risk: Option<RiskFact>,
    /// What the pane shows: `prompt_id`, `kind`, `summary`, and how many times this pane asked.
    #[serde(default)]
    pub prompt_id: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub occurrence: Option<u32>,
    #[serde(default)]
    pub headline: Option<String>,
    #[serde(default)]
    pub choices: Vec<ChoiceFact>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RiskFact {
    #[serde(default)]
    pub level: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChoiceFact {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub broad: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct QuestionFact {
    pub id: String,
    #[serde(default)]
    pub pane_id: Option<String>,
    #[serde(default)]
    pub created_at_ms: Option<u64>,
    #[serde(default)]
    pub expires_at_ms: Option<u64>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    /// Set when this "question" is a consent card (an agent wants to control or answer in a pane it
    /// did not start). It is then a `consent` item, not a `question`.
    #[serde(default)]
    pub consent: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NoticeFact {
    pub id: String,
    #[serde(default)]
    pub at_ms: Option<u64>,
    #[serde(default)]
    pub text: Option<String>,
}

/// One entry `derive` produces: the same five fields `NeedsYouItem` has in `lib/attention.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Derived {
    /// `<kind>:<pane or question or notice id>`.
    pub id: String,
    pub kind: Kind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<Level>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<u64>,
}

// ---------------------------------------------------------------------------------------------
// The wire snapshot

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub v: u32,
    pub rev: u64,
    pub counts: Counts,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Counts {
    pub blocking: u32,
    pub ready: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Open,
    /// Left the list less than `TOMBSTONE_MS` ago. Never counted.
    Resolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    /// Opaque and URL-safe; stable for one occurrence. Never contains a pane id.
    pub id: String,
    /// `<kind>:<subject>`; what `deriveNeedsYou` calls the id. Clients may match on it, never render it.
    pub key: String,
    pub kind: Kind,
    pub lane: Lane,
    pub rank: u8,
    pub state: State,
    pub since_ms: Option<u64>,
    pub expires_at_ms: Option<u64>,
    pub pane: Option<PaneRef>,
    pub title: String,
    pub sentence: Option<String>,
    pub risk: Option<RiskView>,
    pub prompt: Option<PromptView>,
    pub actions: Vec<Action>,
    pub resolution: Option<Resolution>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PaneRef {
    /// Clients need it to open the terminal. Never rendered.
    pub id: String,
    pub heading: String,
    pub agent: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RiskView {
    pub level: Level,
    pub word: &'static str,
    pub headline: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptView {
    pub prompt_id: String,
    /// How many times this pane has asked; the same command asked again is a new occurrence.
    pub occurrence: u32,
    pub kind: String,
    pub summary: Option<String>,
    pub choices: Vec<ChoiceFact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Action {
    pub id: String,
    pub label: String,
    pub style: String,
    /// The device capability the button needs (`answer`, `control`, `type`).
    pub needs: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Resolution {
    /// `answered | gone | expired | withdrawn`.
    pub state: String,
    pub by: Option<String>,
    pub choice_label: Option<String>,
    pub reason: Option<String>,
    pub at_ms: u64,
}

/// What a producer (budget, limit, consent, tamper, setup, review, done) hands to `raise`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raised {
    /// The producer's own key (`budget:<run>`, `consent:<request>`). Raising it again updates in place.
    pub key: String,
    pub kind: Kind,
    pub pane_id: Option<String>,
    pub title: String,
    pub sentence: Option<String>,
    pub expires_at_ms: Option<u64>,
    pub actions: Vec<Action>,
}
