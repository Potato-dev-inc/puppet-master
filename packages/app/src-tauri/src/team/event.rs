//! The `TeamEvent` schema (COWORK-PLAN §2.3): one thing that happened, as it travels between
//! teammates' Puppet Masters. Agent-agnostic and forge-agnostic.
//!
//! The sender fills in facts; the recipient renders the copy. A teammate's event is UNTRUSTED
//! data: [`validate_inbound`] checks every field before anything is stored, and the only free text
//! (`note`) is always shown quoted. Nothing in an event can make this app do anything; it can only
//! become a card.

use serde::{Deserialize, Serialize};

/// The schema version this build writes and accepts.
pub const SCHEMA_VERSION: u8 = 1;
/// Longest note a teammate may attach (characters).
pub const MAX_NOTE_CHARS: usize = 280;
/// Largest serialized event a transport accepts (bytes).
pub const MAX_EVENT_BYTES: usize = 16 * 1024;
/// Events dated further in the future than this are refused (clock skew allowance).
pub const MAX_FUTURE_SKEW_MS: u64 = 10 * 60 * 1000;
/// Default lifetime of an event.
pub const DEFAULT_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;
const MAX_PATHS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamEvent {
    /// Schema version ([`SCHEMA_VERSION`]).
    pub v: u8,
    /// ULID; the replay guard and the card id.
    pub id: String,
    /// Team id. v0: the name in `team.json`; v1: a hash of the team's root public key.
    pub team: String,
    pub project: ProjectRef,
    pub actor: Actor,
    pub kind: TeamEventKind,
    pub at_ms: u64,
    #[serde(default)]
    pub subject: Subject,
    /// At most [`MAX_NOTE_CHARS`]; human or agent text, always rendered quoted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Member ids (or, in v0, display names); empty = everyone on the project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub to: Vec<String>,
    /// The recipient may demote it, never promote it.
    pub tier_hint: Tier,
    /// `{project}#{pr}:{kind}` or `{project}@{branch}:{kind}`: cards with the same key merge.
    pub dedupe_key: String,
    pub expires_at_ms: u64,
    pub source: EventSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRef {
    /// The normalised remote URL (`github.com/acme/billing-service`); see `project_key`.
    pub key: String,
    /// Short name for display (`billing-service`).
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor {
    /// Member id (v1: public key fingerprint).
    pub member: String,
    /// Device id within the member.
    pub device: String,
    /// "Maya".
    pub display: String,
    /// The agent that did it, from the sender's registry; `None` = the person themselves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentTag>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTag {
    /// "Claude Code", "Codex": a display label, never an account or login.
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamEventKind {
    PrOpened,
    PrReady,
    PrUpdated,
    PrMerged,
    PrClosed,
    ReviewRequested,
    /// `subject.branch` + `subject.commit` (+ `subject.forced`).
    Pushed,
    CiFailed,
    CiPassed,
    /// Never sent: computed locally, same shape for the UI.
    ConflictForecast,
    QuestionForwarded,
    Answer,
    PleaseReview,
    Claim,
    Release,
    TurnFinished,
    Presence,
    Note,
    Ack,
    /// A kind from a newer build. Parsed so the rest of the batch still reads; never shown.
    #[serde(other)]
    Unknown,
}

impl TeamEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PrOpened => "pr_opened",
            Self::PrReady => "pr_ready",
            Self::PrUpdated => "pr_updated",
            Self::PrMerged => "pr_merged",
            Self::PrClosed => "pr_closed",
            Self::ReviewRequested => "review_requested",
            Self::Pushed => "pushed",
            Self::CiFailed => "ci_failed",
            Self::CiPassed => "ci_passed",
            Self::ConflictForecast => "conflict_forecast",
            Self::QuestionForwarded => "question_forwarded",
            Self::Answer => "answer",
            Self::PleaseReview => "please_review",
            Self::Claim => "claim",
            Self::Release => "release",
            Self::TurnFinished => "turn_finished",
            Self::Presence => "presence",
            Self::Note => "note",
            Self::Ack => "ack",
            Self::Unknown => "unknown",
        }
    }

    /// Kinds a v0 recipient turns into a card. Everything else is dropped on arrival: v1 adds them
    /// with their own safety rules (an `Answer` needs the approve role, and so on).
    pub fn shown_in_v0(self) -> bool {
        matches!(self, Self::Pushed | Self::PrOpened | Self::PrReady | Self::PrUpdated)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Interrupt,
    Ambient,
    Digest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    Pm,
    Git,
    Github,
    Gitlab,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subject {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// The push rewrote history (the old tip is not an ancestor of the new one).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub forced: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr: Option<PrRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ci: Option<CiRef>,
    /// Repo-relative paths (a team may turn paths off, plan §11 Q10).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// Files changed and lines added/removed by a push.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<DiffStat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<ForwardedQuestion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sentence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffStat {
    pub files: u32,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrRef {
    pub number: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CiRef {
    pub conclusion: String,
    #[serde(default)]
    pub failed_jobs: Vec<String>,
    #[serde(default)]
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardedQuestion {
    pub text: String,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default)]
    pub risk: String,
    #[serde(default)]
    pub expires_at_ms: u64,
}

// ------------------------------------------------------------------------------------------ ids

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A ULID (48-bit milliseconds + 80 random bits, Crockford base32): sortable by time, unique.
pub fn new_event_id(now_ms: u64) -> String {
    use rand::RngCore;
    let mut random = [0u8; 10];
    rand::thread_rng().fill_bytes(&mut random);
    ulid_from(now_ms, random)
}

pub(crate) fn ulid_from(ms: u64, random: [u8; 10]) -> String {
    let mut value: u128 = u128::from(ms & 0xFFFF_FFFF_FFFF) << 80;
    for (i, byte) in random.iter().enumerate() {
        value |= u128::from(*byte) << (72 - 8 * i);
    }
    let mut out = [0u8; 26];
    for i in (0..26).rev() {
        out[i] = CROCKFORD[(value & 0x1F) as usize];
        value >>= 5;
    }
    String::from_utf8(out.to_vec()).unwrap_or_default()
}

pub fn is_event_id(id: &str) -> bool {
    id.len() == 26 && id.bytes().all(|b| CROCKFORD.contains(&b))
}

// ---------------------------------------------------------------------------------- validation

/// A git branch name that is safe to show and to pass to git as a ref (never an option).
/// Conservative subset of `git check-ref-format --branch`.
pub fn is_valid_branch(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('-')
        && !name.starts_with('/')
        && !name.ends_with('/')
        && !name.ends_with('.')
        && !name.ends_with(".lock")
        && !name.contains("..")
        && !name.contains("//")
        && !name.contains("@{")
        && name != "@"
        && name.split('/').all(|part| !part.is_empty() && !part.starts_with('.'))
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.' | '+'))
}

/// A full or abbreviated commit id: 7..=64 lowercase hex characters.
pub fn is_valid_commit(sha: &str) -> bool {
    (7..=64).contains(&sha.len()) && sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A repo-relative path that is safe to show: not absolute, no `..`, no drive letter, no NUL.
pub fn is_valid_repo_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 300
        && !path.starts_with('/')
        && !path.starts_with('\\')
        && !path.contains(':')
        && !path.split(['/', '\\']).any(|seg| seg == "..")
        && !path.chars().any(char::is_control)
}

/// Text from a teammate, made safe to render as plain text: control characters and the bidi
/// overrides that can disguise text are removed, whitespace runs collapse, and it is cut to `max`
/// characters.
pub fn clean_text(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut last_space = false;
    for ch in text.chars() {
        let bidi = matches!(ch, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}');
        if bidi || (ch.is_control() && !ch.is_whitespace()) {
            continue;
        }
        if ch.is_whitespace() {
            if !last_space && !out.is_empty() {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    out.trim_end().chars().take(max).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected(pub &'static str);

impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// Check an event that arrived from a teammate. Returns the event with every display string
/// cleaned, or why it was refused. Refused events are dropped without a card.
pub fn validate_inbound(mut event: TeamEvent, now_ms: u64) -> Result<TeamEvent, Rejected> {
    if event.v != SCHEMA_VERSION {
        return Err(Rejected("unsupported schema version"));
    }
    if !is_event_id(&event.id) {
        return Err(Rejected("bad event id"));
    }
    if event.kind == TeamEventKind::Unknown || event.kind == TeamEventKind::ConflictForecast {
        return Err(Rejected("kind is not sent between teammates"));
    }
    if event.at_ms > now_ms.saturating_add(MAX_FUTURE_SKEW_MS) {
        return Err(Rejected("dated in the future"));
    }
    if event.expires_at_ms <= now_ms {
        return Err(Rejected("expired"));
    }
    if event.team.trim().is_empty() || event.team.len() > 64 {
        return Err(Rejected("bad team id"));
    }
    if event.project.key.trim().is_empty() || event.project.key.len() > 300 {
        return Err(Rejected("bad project key"));
    }
    if event.actor.member.trim().is_empty() || event.actor.member.len() > 100 || event.actor.device.len() > 100 {
        return Err(Rejected("bad actor"));
    }
    if let Some(note) = &event.note {
        if note.chars().count() > MAX_NOTE_CHARS {
            return Err(Rejected("note too long"));
        }
    }
    let subject = &event.subject;
    if let Some(branch) = &subject.branch {
        if !is_valid_branch(branch) {
            return Err(Rejected("bad branch name"));
        }
    }
    if let Some(base) = &subject.base {
        if !is_valid_branch(base) {
            return Err(Rejected("bad base branch name"));
        }
    }
    if let Some(commit) = &subject.commit {
        if !is_valid_commit(commit) {
            return Err(Rejected("bad commit id"));
        }
    }
    if subject.paths.len() > MAX_PATHS || !subject.paths.iter().all(|p| is_valid_repo_path(p)) {
        return Err(Rejected("bad paths"));
    }
    if event.kind == TeamEventKind::Pushed && (subject.branch.is_none() || subject.commit.is_none()) {
        return Err(Rejected("a push names its branch and commit"));
    }
    if let Some(pr) = &subject.pr {
        if pr.number == 0 || !(pr.url.is_empty() || pr.url.starts_with("https://")) {
            return Err(Rejected("bad pull request"));
        }
    }
    // Display strings: cleaned, never rejected for being odd.
    event.project.name = clean_text(&event.project.name, 80);
    event.actor.display = clean_text(&event.actor.display, 40);
    if event.actor.display.is_empty() {
        event.actor.display = "A teammate".into();
    }
    if let Some(agent) = &mut event.actor.agent {
        agent.label = clean_text(&agent.label, 40);
        agent.role = agent.role.as_deref().map(|r| clean_text(r, 40));
    }
    event.note = event.note.as_deref().map(|n| clean_text(n, MAX_NOTE_CHARS)).filter(|n| !n.is_empty());
    if let Some(pr) = &mut event.subject.pr {
        pr.title = clean_text(&pr.title, 200);
        pr.state = clean_text(&pr.state, 20);
    }
    event.subject.sentence = event.subject.sentence.as_deref().map(|s| clean_text(s, 280));
    event.dedupe_key = clean_text(&event.dedupe_key, 300);
    Ok(event)
}

/// `{project}@{branch}:{kind}` for pushes, `{project}#{pr}:{kind}` for PRs.
pub fn dedupe_key(project_key: &str, kind: TeamEventKind, subject: &Subject) -> String {
    match (&subject.pr, &subject.branch) {
        (Some(pr), _) => format!("{project_key}#{}:{}", pr.number, kind.as_str()),
        (None, Some(branch)) => format!("{project_key}@{branch}:{}", kind.as_str()),
        (None, None) => format!("{project_key}:{}", kind.as_str()),
    }
}
