//! Data model for "Assign an issue" (SPEC n32 section 3.2): the settings, the watched repos,
//! the runs and the error codes. Pure data plus the validators. No I/O here (see `store`).
//!
//! Defaults are the safe ones: the feature is off, every repo asks before a run and before a
//! push, and the budgets are always set. A missing "ask" switch in a saved file reads as ON.

use serde::{Deserialize, Serialize};

pub const DEFAULT_LABEL: &str = "puppet";
pub const TICK_MS: u64 = 15_000;
pub const POLL_MIN_S: u64 = 60;
pub const BACKOFF_MAX_S: u64 = 900;
pub const BODY_MAX_BYTES: usize = 16 * 1024;
pub const TITLE_MAX_CHARS: usize = 200;
pub const REPOS_CAP: usize = 20;
pub const ALLOWED_CAP: usize = 50;
pub const RECORDS_CAP: usize = 500;
pub const TURNS_DEFAULT: u32 = 30;
pub const TURNS_MAX: u32 = 200;
pub const MINUTES_DEFAULT: u32 = 45;
pub const MINUTES_MIN: u32 = 5;
pub const MINUTES_MAX: u32 = 240;
pub const RUNS_PER_DAY_DEFAULT: u32 = 5;
pub const RUNS_PER_DAY_MAX: u32 = 50;
pub const PARALLEL_DEFAULT: u32 = 1;
pub const PARALLEL_MAX: u32 = 3;
pub const CARD_TIMEOUT_S: u64 = 24 * 3600;
pub const AUTO_PUSH_MAX_FILES: usize = 100;
pub const CARD_KIND_START: &str = "issue_start";
pub const CARD_KIND_PUSH: &str = "issue_push";
pub const CHOICE_START: &str = "start";
pub const CHOICE_NOT_NOW: &str = "not_now";
pub const CHOICE_IGNORE: &str = "ignore";
pub const CHOICE_PUSH: &str = "push";
pub const CHOICE_LOOK: &str = "look";
pub const CHOICE_THROW: &str = "throw_away";

// Error codes (plain English messages are written where they are raised).
pub const ERR_GH_MISSING: &str = "ISSUE_GH_MISSING";
pub const ERR_GH_SIGNED_OUT: &str = "ISSUE_GH_SIGNED_OUT";
pub const ERR_REPO_MISMATCH: &str = "ISSUE_REPO_MISMATCH";
pub const ERR_NOT_ALLOWED: &str = "ISSUE_NOT_ALLOWED";
pub const ERR_DAY_CAP: &str = "ISSUE_DAY_CAP";
pub const ERR_LABEL_GONE: &str = "ISSUE_LABEL_GONE";
pub const ERR_CLOSED: &str = "ISSUE_CLOSED";
pub const ERR_AGENT_NOT_READY: &str = "ISSUE_AGENT_NOT_READY";
pub const ERR_START_FAILED: &str = "ISSUE_START_FAILED";
pub const ERR_NO_CHANGES: &str = "ISSUE_NO_CHANGES";
pub const ERR_GIT_CHANGED: &str = "ISSUE_GIT_CHANGED";
pub const ERR_REMOTE_BRANCH_EXISTS: &str = "ISSUE_REMOTE_BRANCH_EXISTS";
pub const ERR_PUSH_HELD: &str = "ISSUE_PUSH_HELD";
pub const ERR_PUSH_FAILED: &str = "ISSUE_PUSH_FAILED";

/// `agent` or `agent@account`. Both parts use `[A-Za-z0-9_.-]{1,64}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentPick {
    pub agent_type: String,
    pub account: Option<String>,
}

impl AgentPick {
    /// Parse `agent` or `agent@account`. Anything else (two `@`, empty parts, odd characters) is None.
    pub fn parse(s: &str) -> Option<Self> {
        let (agent, account) = match s.split_once('@') {
            Some((agent, account)) => (agent, Some(account)),
            None => (s, None),
        };
        if !valid_token(agent) {
            return None;
        }
        if let Some(account) = account {
            if !valid_token(account) {
                return None;
            }
        }
        Some(Self { agent_type: agent.to_string(), account: account.map(str::to_string) })
    }

    /// The text form again: `agent` or `agent@account`.
    pub fn key(&self) -> String {
        match &self.account {
            Some(account) => format!("{}@{account}", self.agent_type),
            None => self.agent_type.clone(),
        }
    }
}

fn valid_token(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

/// `owner/repo`. Owner `[A-Za-z0-9-]{1,39}`, repo `[A-Za-z0-9_.-]{1,100}` and not starting with `.`.
pub fn valid_owner_repo(s: &str) -> bool {
    let Some((owner, repo)) = s.split_once('/') else {
        return false;
    };
    let owner_ok = (1..=39).contains(&owner.len()) && owner.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    let repo_ok = (1..=100).contains(&repo.len())
        && !repo.starts_with('.')
        && repo.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'));
    owner_ok && repo_ok && !repo.contains('/')
}

/// A GitHub label name, trimmed by the caller: `[A-Za-z0-9 _.:-]{1,50}` after trimming.
pub fn valid_label(s: &str) -> bool {
    let t = s.trim();
    (1..=50).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'_' | b'.' | b':' | b'-'))
}

/// A GitHub login `[A-Za-z0-9-]{1,39}`, or `<name>[bot]` for a bot listed exactly.
pub fn valid_login(s: &str) -> bool {
    let name = s.strip_suffix("[bot]").unwrap_or(s);
    (1..=39).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// `ir-` + 12 lowercase hex characters.
pub fn valid_run_id(id: &str) -> bool {
    valid_hex_id(id, "ir-", 12)
}

/// `rw-` + 8 lowercase hex characters.
pub fn valid_repo_id(id: &str) -> bool {
    valid_hex_id(id, "rw-", 8)
}

fn valid_hex_id(id: &str, prefix: &str, hex_len: usize) -> bool {
    match id.strip_prefix(prefix) {
        Some(hex) => hex.len() == hex_len && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        None => false,
    }
}

/// One repo the person watched. `folder` is the git top level of a clone whose origin is `owner_repo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoWatch {
    pub id: String,
    pub owner_repo: String,
    pub folder: String,
    pub label: String,
    pub agent: AgentPick,
    pub turns: u32,
    pub minutes: u32,
    pub runs_per_day: u32,
    #[serde(default = "default_true")]
    pub ask_before_start: bool,
    #[serde(default = "default_true")]
    pub ask_before_push: bool,
    /// Only labels added after this time count (the baseline).
    pub added_at_ms: u64,
    #[serde(default)]
    pub paused: bool,
}

impl Default for RepoWatch {
    fn default() -> Self {
        Self {
            id: String::new(),
            owner_repo: String::new(),
            folder: String::new(),
            label: DEFAULT_LABEL.to_string(),
            agent: AgentPick { agent_type: String::new(), account: None },
            turns: TURNS_DEFAULT,
            minutes: MINUTES_DEFAULT,
            runs_per_day: RUNS_PER_DAY_DEFAULT,
            ask_before_start: true,
            ask_before_push: true,
            added_at_ms: 0,
            paused: false,
        }
    }
}

/// The whole feature's settings (`config.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub enabled: bool,
    /// Extra GitHub logins allowed to trigger runs, lowercase.
    #[serde(default)]
    pub allowed: Vec<String>,
    #[serde(default)]
    pub write_access_counts: bool,
    #[serde(default = "default_parallel")]
    pub parallel: u32,
    #[serde(default)]
    pub repos: Vec<RepoWatch>,
    /// (owner_repo, issue number) pairs the person chose to ignore.
    #[serde(default)]
    pub ignored: Vec<(String, u64)>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed: Vec::new(),
            write_access_counts: false,
            parallel: PARALLEL_DEFAULT,
            repos: Vec::new(),
            ignored: Vec::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_parallel() -> u32 {
    PARALLEL_DEFAULT
}

/// Counts of what the text cleaner removed from an issue.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanNotes {
    pub html_comments: u32,
    pub invisible_chars: u32,
    pub cut: bool,
}

/// The issue as the person saw it when they said yes. The agent gets exactly this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueSnapshot {
    pub owner_repo: String,
    pub number: u64,
    /// Must start with `https://github.com/`.
    pub url: String,
    pub title: String,
    pub body: String,
    pub author: String,
    pub author_association: String,
    pub labeled_by: String,
    pub labeled_by_bot: bool,
    pub label_event_id: u64,
    pub labeled_at_ms: u64,
    pub notes: CleanNotes,
    /// `sha256(title + "\n" + body)[..8]`.
    pub sha8: String,
}

/// Why a labeler was let in. Refused labelers never reach a card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trigger {
    Allowed { why: &'static str },
    Refused { why: String },
}

/// Where one issue run is. Serialized with a `state` tag in snake_case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RunState {
    /// The labeler is not allowed, or the label came before the repo was added.
    Skipped { why: String },
    Asking { question_id: String },
    /// "Not now". The person can ask again from Settings.
    Declined,
    Ignored,
    /// The card timed out.
    Expired,
    /// Approved; waiting for a free slot or the day cap.
    Queued { why: String },
    Starting,
    Running { handle: String, operation_id: String, pane_id: Option<String> },
    Checking,
    AskPush { question_id: String, facts: PushFacts },
    Pushing,
    PrOpen { number: u64, url: String },
    NoChanges { message: String },
    /// A guard stopped the push. The person decides in the app.
    Held { code: String, message: String },
    ThrownAway,
    Failed { code: String, message: String },
}

/// What the push card and the push check know about a finished run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushFacts {
    pub files: u32,
    pub additions: u32,
    pub deletions: u32,
    pub ci_files: Vec<String>,
    pub secret_files: Vec<String>,
    /// (path, line)
    pub secret_lines: Vec<(String, u32)>,
    pub home_path_lines: Vec<(String, u32)>,
    /// Files under `.puppet-master/`.
    pub internal_files: Vec<String>,
    pub git_changed: bool,
    pub remote_branch_exists: bool,
    /// The issue on GitHub is not what the person said yes to (S10): the text changed, it was
    /// closed, or the label came off. A card is always asked; the push is not held.
    #[serde(default)]
    pub issue_changed: bool,
    /// "completed" and so on.
    pub run_status: String,
    pub turns: u32,
    pub minutes: u32,
}

/// One run made from one labeled issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueRun {
    /// `ir-` + 12 lowercase hex.
    pub id: String,
    pub repo_id: String,
    pub issue: IssueSnapshot,
    pub agent: AgentPick,
    pub agent_label: String,
    pub at_ms: u64,
    pub started_at_ms: Option<u64>,
    /// Why the start card could not be skipped.
    #[serde(default)]
    pub forced_ask: Vec<String>,
    /// "desktop" | "phone" | "auto".
    pub approved_by: Option<String>,
    /// `issue-<n>-<4 hex>`, also the worktree handle.
    pub handle: Option<String>,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub git_fingerprint: Option<String>,
    pub state: RunState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_pick_parses_agent_and_account() {
        let plain = AgentPick::parse("codex").unwrap();
        assert_eq!((plain.agent_type.as_str(), plain.account.as_deref()), ("codex", None));
        let with = AgentPick::parse("codex@work_1.a").unwrap();
        assert_eq!(with.account.as_deref(), Some("work_1.a"));
        assert_eq!(with.key(), "codex@work_1.a");
        assert_eq!(AgentPick::parse(&plain.key()).unwrap(), plain);
    }

    #[test]
    fn agent_pick_refuses_bad_forms() {
        let long = "a".repeat(65);
        for bad in ["", "@acct", "codex@", "a@b@c", "co dex", "x/y", long.as_str()] {
            assert!(AgentPick::parse(bad).is_none(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn owner_repo_rules() {
        assert!(valid_owner_repo("acme/billing"));
        assert!(valid_owner_repo("acme/billing.js"));
        for bad in ["../x", "a/b/c", ".hidden", "acme/.x", "acme/", "/x", "acme", "acme/x y"] {
            assert!(!valid_owner_repo(bad), "{bad:?} must be refused");
        }
        assert!(valid_owner_repo(&format!("{}/x", "a".repeat(39))));
        assert!(!valid_owner_repo(&format!("{}/x", "a".repeat(40))));
    }

    #[test]
    fn label_rules() {
        assert!(valid_label("puppet"));
        assert!(valid_label("  puppet: ready  "));
        assert!(!valid_label("a,b"));
        assert!(!valid_label("   "));
        assert!(!valid_label(&"a".repeat(51)));
    }

    #[test]
    fn login_rules() {
        assert!(valid_login("someone"));
        assert!(valid_login("x[bot]"));
        assert!(!valid_login("a_b"));
        assert!(!valid_login("x[bot]y"));
        assert!(!valid_login(""));
    }

    #[test]
    fn id_shapes() {
        assert!(valid_run_id("ir-0123456789ab"));
        assert!(!valid_run_id("ir-0123456789AB"));
        assert!(!valid_run_id("rw-0123456789ab"));
        assert!(valid_repo_id("rw-0123abcd"));
        assert!(!valid_repo_id("rw-../../x"));
    }

    #[test]
    fn defaults_are_the_safe_ones() {
        let repo = RepoWatch::default();
        assert!(repo.ask_before_start && repo.ask_before_push);
        assert_eq!((repo.turns, repo.minutes, repo.runs_per_day), (30, 45, 5));
        let cfg = Config::default();
        assert!(!cfg.enabled && !cfg.write_access_counts);
        assert_eq!(cfg.parallel, 1);
    }

    #[test]
    fn missing_ask_switches_read_as_on() {
        let json = r#"{"id":"rw-01234567","owner_repo":"acme/billing","folder":"C:/x","label":"puppet",
            "agent":{"agent_type":"codex","account":null},"turns":30,"minutes":45,"runs_per_day":5,"added_at_ms":1}"#;
        let repo: RepoWatch = serde_json::from_str(json).unwrap();
        assert!(repo.ask_before_start && repo.ask_before_push && !repo.paused);
    }

    #[test]
    fn run_state_tags_are_stable() {
        let cases = [
            (RunState::Declined, r#"{"state":"declined"}"#),
            (RunState::Asking { question_id: "q1".into() }, r#"{"state":"asking","question_id":"q1"}"#),
            (RunState::Skipped { why: "no".into() }, r#"{"state":"skipped","why":"no"}"#),
            (
                RunState::Running { handle: "issue-42-a1b2".into(), operation_id: "op".into(), pane_id: None },
                r#"{"state":"running","handle":"issue-42-a1b2","operation_id":"op","pane_id":null}"#,
            ),
            (
                RunState::PrOpen { number: 57, url: "https://github.com/acme/billing/pull/57".into() },
                r#"{"state":"pr_open","number":57,"url":"https://github.com/acme/billing/pull/57"}"#,
            ),
            (
                RunState::Held { code: "ISSUE_GIT_CHANGED".into(), message: "m".into() },
                r#"{"state":"held","code":"ISSUE_GIT_CHANGED","message":"m"}"#,
            ),
        ];
        for (state, golden) in cases {
            assert_eq!(serde_json::to_string(&state).unwrap(), golden);
            let back: RunState = serde_json::from_str(golden).unwrap();
            assert_eq!(back, state);
        }
    }
}
