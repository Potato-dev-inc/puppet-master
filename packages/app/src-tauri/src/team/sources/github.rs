//! GitHub facts (COWORK-PLAN §2.2 source 2): pull requests opened, merged or closed, and CI going
//! red or green, learned by each member's own app from GitHub through their own `gh` login. Puppet
//! Master holds no GitHub secret, and a member sees only what their account can see.
//!
//! - **Conditional requests:** every poll sends `If-None-Match` with the last ETag. GitHub answers
//!   304 for "nothing changed", which does not count against the rate limit; `X-Poll-Interval` is
//!   honoured.
//! - **Facts never travel through the relay.** Every member learns them from GitHub; they become
//!   local cards (`secure::Verified::local_fact`) with the same card UI as a teammate's events, and
//!   a teammate's envelope that claims to be a GitHub fact is refused.
//! - **Baseline first:** the first look at a repository records its state and announces nothing.
//! - The mapping (`diff_pulls`, `ci_state`, `fact_event`) is pure and tested with recorded `gh`
//!   output (`team/fixtures/gh/`).

use crate::team::event::{self, Actor, CiRef, EventSource, PrRef, ProjectRef, Subject, TeamEvent, TeamEventKind, Tier};
use crate::team::roster::Roster;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::process::{Command, Stdio};

/// Default seconds between polls of one repository (GitHub may ask for more).
pub const DEFAULT_POLL_S: u64 = 60;

/// One `gh api --include` answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhResponse {
    pub status: u16,
    pub etag: Option<String>,
    pub poll_interval_s: Option<u64>,
    pub body: String,
}

/// Parse `gh api --include` output: the status line, headers, a blank line, the body. `gh` exits
/// non-zero on a 304, so the exit code is not used.
pub fn parse_include(output: &str) -> Option<GhResponse> {
    let text = output.replace("\r\n", "\n");
    let (head, body) = text.split_once("\n\n").unwrap_or((text.as_str(), ""));
    let mut lines = head.lines();
    let status_line = lines.next()?;
    let status: u16 = status_line.strip_prefix("HTTP/")?.split_whitespace().nth(1)?.parse().ok()?;
    let mut etag = None;
    let mut poll = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        match name.trim().to_ascii_lowercase().as_str() {
            "etag" => etag = Some(value.trim().to_string()),
            "x-poll-interval" => poll = value.trim().parse().ok(),
            _ => {}
        }
    }
    Some(GhResponse { status, etag, poll_interval_s: poll, body: body.to_string() })
}

/// Runs `gh` (a fake in tests). Every call is a read except `pr merge --auto`, which only the
/// person's confirmed click reaches (`merge_green`).
pub trait Gh: Send + Sync {
    /// `gh api --include [-H If-None-Match: etag] <path>`.
    fn api(&self, path: &str, etag: Option<&str>) -> Result<GhResponse, String>;
    /// Any other `gh` command; returns stdout, or stderr as the error.
    fn run(&self, args: &[&str]) -> Result<String, String>;
}

/// The real `gh` on PATH (`PUPPET_MASTER_GH` overrides it, for a demo fake).
pub struct GhCli;

pub(crate) fn gh_command() -> Command {
    let program = std::env::var("PUPPET_MASTER_GH").ok().filter(|p| !p.trim().is_empty()).unwrap_or_else(|| "gh".into());
    let mut cmd = crate::proc_util::hidden_command(program);
    cmd.env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

impl Gh for GhCli {
    fn api(&self, path: &str, etag: Option<&str>) -> Result<GhResponse, String> {
        let mut cmd = gh_command();
        cmd.args(["api", "--include"]);
        if let Some(etag) = etag {
            cmd.args(["-H", &format!("If-None-Match: {etag}")]);
        }
        cmd.arg(path);
        let out = cmd.output().map_err(|e| format!("gh isn't available: {e}"))?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        parse_include(&text).ok_or_else(|| {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if err.is_empty() { "gh gave no answer".into() } else { err }
        })
    }

    fn run(&self, args: &[&str]) -> Result<String, String> {
        let out = gh_command().args(args).output().map_err(|e| format!("gh isn't available: {e}"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
}

// ----------------------------------------------------------------------------- pull requests

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct ApiUser {
    login: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct ApiRef {
    #[serde(rename = "ref")]
    name: String,
    #[serde(default)]
    sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct ApiPull {
    number: u64,
    state: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    merged_at: Option<String>,
    user: Option<ApiUser>,
    head: ApiRef,
    base: ApiRef,
}

/// What the app remembers about one pull request between polls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrSnapshot {
    pub number: u64,
    pub open: bool,
    pub merged: bool,
    pub draft: bool,
    pub title: String,
    pub url: String,
    pub author: String,
    pub head: String,
    pub head_sha: String,
    pub base: String,
}

/// `GET /repos/{o}/{r}/pulls?state=all` → snapshots. Malformed entries are skipped.
pub fn parse_pulls(json: &str) -> Vec<PrSnapshot> {
    let Ok(list) = serde_json::from_str::<Vec<serde_json::Value>>(json) else { return Vec::new() };
    list.into_iter()
        .filter_map(|v| serde_json::from_value::<ApiPull>(v).ok())
        .filter(|p| p.number > 0 && event::is_valid_branch(&p.head.name) && event::is_valid_branch(&p.base.name))
        .map(|p| PrSnapshot {
            number: p.number,
            open: p.state == "open",
            merged: p.merged_at.is_some(),
            draft: p.draft,
            title: p.title,
            url: if p.html_url.starts_with("https://") { p.html_url } else { String::new() },
            author: p.user.map(|u| u.login).unwrap_or_default(),
            head_sha: if event::is_valid_commit(&p.head.sha) { p.head.sha } else { String::new() },
            head: p.head.name,
            base: p.base.name,
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fact {
    PrOpened(PrSnapshot),
    PrMerged(PrSnapshot),
    PrClosed(PrSnapshot),
    CiFailed { branch: String, pr: Option<PrSnapshot>, ci: CiRef },
    CiPassed { branch: String, pr: Option<PrSnapshot>, ci: CiRef },
}

/// What changed between two looks at the pull request list. The first look (`prev` = `None`) is
/// the baseline and yields nothing.
pub fn diff_pulls(prev: Option<&BTreeMap<u64, PrSnapshot>>, now: &[PrSnapshot]) -> Vec<Fact> {
    let Some(prev) = prev else { return Vec::new() };
    let mut facts = Vec::new();
    for pr in now {
        match prev.get(&pr.number) {
            None if pr.open => facts.push(Fact::PrOpened(pr.clone())),
            // Opened and merged between two polls.
            None if pr.merged => facts.push(Fact::PrMerged(pr.clone())),
            None => {}
            Some(before) if before.open && !pr.open && pr.merged => facts.push(Fact::PrMerged(pr.clone())),
            Some(before) if before.open && !pr.open => facts.push(Fact::PrClosed(pr.clone())),
            Some(before) if !before.open && pr.open => facts.push(Fact::PrOpened(pr.clone())), // reopened
            Some(_) => {}
        }
    }
    facts
}

// ------------------------------------------------------------------------------------------ CI

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiState {
    /// No checks at all.
    None,
    Pending,
    Passed,
    Failed,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiRun {
    name: String,
    status: String,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    html_url: String,
}

/// `GET /repos/{o}/{r}/commits/{sha}/check-runs` → the overall state and the failed job names.
pub fn ci_state(json: &str) -> (CiState, CiRef) {
    #[derive(Deserialize)]
    struct Runs {
        #[serde(default)]
        check_runs: Vec<ApiRun>,
    }
    let runs = serde_json::from_str::<Runs>(json).map(|r| r.check_runs).unwrap_or_default();
    let failed: Vec<&ApiRun> = runs
        .iter()
        .filter(|r| matches!(r.conclusion.as_deref(), Some("failure" | "timed_out" | "cancelled" | "action_required" | "startup_failure")))
        .collect();
    let state = if runs.is_empty() {
        CiState::None
    } else if !failed.is_empty() {
        CiState::Failed
    } else if runs.iter().any(|r| r.status != "completed") {
        CiState::Pending
    } else {
        CiState::Passed
    };
    let mut names: Vec<String> = failed.iter().map(|r| event::clean_text(&r.name, 80)).collect();
    names.dedup();
    names.truncate(10);
    let url = failed.first().map(|r| r.html_url.clone()).filter(|u| u.starts_with("https://")).unwrap_or_default();
    let conclusion = match state {
        CiState::Failed => "failure",
        CiState::Passed => "success",
        CiState::Pending => "pending",
        CiState::None => "none",
    };
    (state, CiRef { conclusion: conclusion.into(), failed_jobs: names, url })
}

/// A CI fact for a change of state (red after anything else; green after red or pending).
pub fn ci_fact(before: Option<CiState>, now: CiState, branch: &str, pr: Option<PrSnapshot>, ci: CiRef) -> Option<Fact> {
    let before = before?; // baseline
    match (before, now) {
        (b, CiState::Failed) if b != CiState::Failed => Some(Fact::CiFailed { branch: branch.into(), pr, ci }),
        (CiState::Failed | CiState::Pending, CiState::Passed) => Some(Fact::CiPassed { branch: branch.into(), pr, ci }),
        _ => None,
    }
}

// --------------------------------------------------------------------------- facts → events

fn pr_ref(pr: &PrSnapshot, state: &str) -> PrRef {
    PrRef {
        number: pr.number,
        title: pr.title.clone(),
        url: pr.url.clone(),
        state: state.into(),
        draft: pr.draft,
        files: None,
        additions: None,
        deletions: None,
    }
}

/// The card event for a fact. The actor is the teammate whose GitHub login it is (from the
/// roster), or the plain login for someone not on the team (they still count, plan Q6).
pub fn fact_event(project_key: &str, fact: &Fact, roster: Option<&Roster>, now_ms: u64) -> TeamEvent {
    let (kind, author, subject, tier) = match fact {
        Fact::PrOpened(pr) => (
            TeamEventKind::PrOpened,
            pr.author.clone(),
            Subject { branch: Some(pr.head.clone()), base: Some(pr.base.clone()), pr: Some(pr_ref(pr, "open")), ..Subject::default() },
            Tier::Ambient,
        ),
        Fact::PrMerged(pr) => (
            TeamEventKind::PrMerged,
            pr.author.clone(),
            Subject { branch: Some(pr.head.clone()), base: Some(pr.base.clone()), pr: Some(pr_ref(pr, "merged")), ..Subject::default() },
            Tier::Ambient,
        ),
        Fact::PrClosed(pr) => (
            TeamEventKind::PrClosed,
            pr.author.clone(),
            Subject { branch: Some(pr.head.clone()), base: Some(pr.base.clone()), pr: Some(pr_ref(pr, "closed")), ..Subject::default() },
            Tier::Digest,
        ),
        Fact::CiFailed { branch, pr, ci } | Fact::CiPassed { branch, pr, ci } => {
            let failed = matches!(fact, Fact::CiFailed { .. });
            (
                if failed { TeamEventKind::CiFailed } else { TeamEventKind::CiPassed },
                pr.as_ref().map(|p| p.author.clone()).unwrap_or_default(),
                Subject {
                    branch: Some(branch.clone()),
                    pr: pr.as_ref().map(|p| pr_ref(p, "open")),
                    ci: Some(ci.clone()),
                    ..Subject::default()
                },
                if failed { Tier::Interrupt } else { Tier::Digest },
            )
        }
    };
    let member = roster.and_then(|r| r.by_github(&author));
    let actor = Actor {
        member: member.map(|m| m.id.clone()).unwrap_or_else(|| format!("github:{author}")),
        device: String::new(),
        display: member.map(|m| m.display.clone()).unwrap_or_else(|| if author.is_empty() { "GitHub".into() } else { author.clone() }),
        agent: None,
    };
    TeamEvent {
        v: event::SCHEMA_VERSION,
        id: event::new_event_id(now_ms),
        team: roster.map(|r| r.team.clone()).unwrap_or_default(),
        project: ProjectRef { key: project_key.into(), name: crate::team::project_key::display_name(project_key) },
        actor,
        kind,
        at_ms: now_ms,
        dedupe_key: event::dedupe_key(project_key, kind, &subject),
        subject,
        note: None,
        to: Vec::new(),
        tier_hint: tier,
        expires_at_ms: now_ms + event::DEFAULT_TTL_MS,
        source: EventSource::Github,
    }
}

/// `github.com/acme/billing` → `acme/billing`.
pub fn owner_repo(project_key: &str) -> Option<String> {
    let rest = project_key.strip_prefix("github.com/")?;
    let mut parts = rest.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    (ok(owner) && ok(repo) && parts.next().is_none() && !repo.starts_with('.')).then(|| format!("{owner}/{repo}"))
}

// ------------------------------------------------------------------------------- the poller

/// Per-repository memory of the poller (in memory only; a restart takes a new baseline).
#[derive(Debug, Default)]
pub struct RepoPoll {
    pub pulls_etag: Option<String>,
    pub pulls: Option<BTreeMap<u64, PrSnapshot>>,
    /// head sha → (etag, last state)
    pub checks: BTreeMap<String, (Option<String>, CiState)>,
    pub next_at_ms: u64,
}

/// One poll of one repository: the pull request list, then CI of the open pull requests' heads.
pub fn poll_repo(gh: &dyn Gh, owner_repo: &str, state: &mut RepoPoll, now_ms: u64) -> Result<Vec<Fact>, String> {
    let path = format!("repos/{owner_repo}/pulls?state=all&sort=updated&direction=desc&per_page=30");
    let response = gh.api(&path, state.pulls_etag.as_deref())?;
    let interval = response.poll_interval_s.unwrap_or(DEFAULT_POLL_S).max(DEFAULT_POLL_S);
    state.next_at_ms = now_ms + interval * 1000;
    let mut facts = Vec::new();
    let mut open_prs: Vec<PrSnapshot> = Vec::new();
    match response.status {
        200 => {
            let now = parse_pulls(&response.body);
            facts.extend(diff_pulls(state.pulls.as_ref(), &now));
            let mut map = state.pulls.take().unwrap_or_default();
            for pr in now {
                map.insert(pr.number, pr);
            }
            state.pulls = Some(map);
            state.pulls_etag = response.etag;
        }
        304 => {}
        code => return Err(format!("GitHub answered {code}")),
    }
    if let Some(map) = &state.pulls {
        open_prs = map.values().filter(|p| p.open && !p.head_sha.is_empty()).cloned().collect();
    }
    open_prs.truncate(10);
    for pr in open_prs {
        let (etag, before) = state.checks.get(&pr.head_sha).cloned().map(|(e, s)| (e, Some(s))).unwrap_or((None, None));
        let response = gh.api(&format!("repos/{owner_repo}/commits/{}/check-runs?per_page=50", pr.head_sha), etag.as_deref())?;
        if response.status == 304 {
            continue;
        }
        if response.status != 200 {
            continue;
        }
        let (now_state, ci) = ci_state(&response.body);
        if let Some(fact) = ci_fact(before, now_state, &pr.head, Some(pr.clone()), ci) {
            facts.push(fact);
        }
        state.checks.insert(pr.head_sha.clone(), (response.etag, now_state));
    }
    Ok(facts)
}
