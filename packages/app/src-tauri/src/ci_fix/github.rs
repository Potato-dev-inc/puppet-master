//! Red to Green: GitHub reads with the `gh` login (SPEC n33 section 3.3). Parses the JSON that
//! `gh api` returns for a pull request and its check runs, builds the read paths, and reads both
//! through the existing `Gh` client. Read only: every call is a GET with no fields, and no path
//! built here names a write (`WRITE_PATHS_NEVER`). The parsers do no I/O; the tests read the
//! recorded responses in `fixtures/gh/`, never GitHub.
//!
//! Everything here is data from GitHub, so it is untrusted. A missing or odd field fails closed:
//! `None`, or an error. Check names and links are cleaned before anything uses them.

use super::model::{valid_sha, FailedCheck};
use crate::issue_intake::model::valid_owner_repo;
use crate::team::event::{clean_text, is_valid_branch};
use crate::team::sources::github::Gh;
use serde::Deserialize;

/// A check name on a card: one line, at most this many characters.
const NAME_MAX_CHARS: usize = 80;
/// Shown when a check has no usable name.
const UNNAMED: &str = "Unnamed check";
/// The only link prefix Puppet Master keeps.
const GITHUB_PREFIX: &str = "https://github.com/";
/// The app that runs GitHub Actions jobs. Only its checks have a job log.
const ACTIONS_APP: &str = "github-actions";
/// Conclusions that count as a failure. Every other conclusion is not one.
const FAILED_CONCLUSIONS: &[&str] = &["failure", "timed_out", "startup_failure"];
/// Shown when GitHub waits for a person to approve a run.
const WAITING_APPROVAL: &str = "Waiting for approval on GitHub";

/// Words no path built here may contain (SPEC S9): reruns, dispatches, approvals, deployment
/// approvals, merges, reviews, comments and labels all write to GitHub. Puppet Master only reads.
pub const WRITE_PATHS_NEVER: &[&str] =
    &["rerun", "rerun-failed-jobs", "dispatches", "approve", "pending_deployments", "/merge", "/reviews", "/comments", "/labels"];

/// A pull request as GitHub describes it, read through the person's `gh` login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiPull {
    pub number: u64,
    /// "open" | "closed".
    pub state: String,
    pub merged: bool,
    /// `user.login`. Empty when GitHub does not say (ownership then refuses it).
    pub author: String,
    pub head_ref: String,
    /// The head commit, 40 lowercase hex.
    pub head_sha: String,
    /// `head.repo.full_name`. None when the fork was deleted.
    pub head_repo: Option<String>,
    pub base_ref: String,
    pub base_repo: String,
    /// Starts with `https://github.com/`, else empty.
    pub html_url: String,
}

#[derive(Deserialize)]
struct RawUser {
    login: String,
}

#[derive(Deserialize)]
struct RawRepo {
    full_name: String,
}

#[derive(Deserialize)]
struct RawSide {
    #[serde(rename = "ref")]
    name: String,
    #[serde(default)]
    sha: String,
    #[serde(default)]
    repo: Option<RawRepo>,
}

#[derive(Deserialize)]
struct RawPull {
    number: u64,
    state: String,
    #[serde(default)]
    merged: Option<bool>,
    #[serde(default)]
    merged_at: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    user: Option<RawUser>,
    head: RawSide,
    base: RawSide,
}

/// One pull request answer. None when it is unreadable, or a name or commit is not valid.
pub fn parse_pull(json: &str) -> Option<ApiPull> {
    let raw: RawPull = serde_json::from_str(json).ok()?;
    if raw.number == 0 || !matches!(raw.state.as_str(), "open" | "closed") {
        return None;
    }
    if !is_valid_branch(&raw.head.name) || !is_valid_branch(&raw.base.name) || !valid_sha(&raw.head.sha) {
        return None;
    }
    let base_repo = raw.base.repo.map(|r| r.full_name)?;
    if !valid_owner_repo(&base_repo) {
        return None;
    }
    let head_repo = match raw.head.repo {
        Some(repo) if valid_owner_repo(&repo.full_name) => Some(repo.full_name),
        Some(_) => return None,
        None => None,
    };
    Some(ApiPull {
        number: raw.number,
        state: raw.state,
        merged: raw.merged.unwrap_or(false) || raw.merged_at.is_some(),
        author: raw.user.map(|u| u.login).unwrap_or_default(),
        head_ref: raw.head.name,
        head_sha: raw.head.sha,
        head_repo,
        base_ref: raw.base.name,
        base_repo,
        html_url: github_link(raw.html_url.as_deref().unwrap_or("")),
    })
}

/// One check run on a commit, as GitHub describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiCheck {
    pub id: u64,
    pub name: String,
    /// "queued" | "in_progress" | "completed" (and whatever GitHub adds later).
    pub status: String,
    /// None while the run has not finished.
    pub conclusion: Option<String>,
    /// `app.slug`. Empty when GitHub does not say.
    pub app_slug: String,
    pub html_url: String,
    pub details_url: String,
}

#[derive(Deserialize)]
struct RawApp {
    #[serde(default)]
    slug: Option<String>,
}

#[derive(Deserialize)]
struct RawCheck {
    id: u64,
    name: String,
    status: String,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    app: Option<RawApp>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    details_url: Option<String>,
}

#[derive(Deserialize)]
struct RawChecks {
    check_runs: Vec<RawCheck>,
}

/// `GET .../check-runs` answer. None = unreadable (fail closed): one bad entry makes the whole
/// answer unreadable, so a half-read list never raises a card.
pub fn parse_check_runs(json: &str) -> Option<Vec<ApiCheck>> {
    let raw: RawChecks = serde_json::from_str(json).ok()?;
    Some(
        raw.check_runs
            .into_iter()
            .map(|c| ApiCheck {
                id: c.id,
                name: c.name,
                status: c.status,
                conclusion: c.conclusion,
                app_slug: c.app.and_then(|a| a.slug).unwrap_or_default(),
                html_url: c.html_url.unwrap_or_default(),
                details_url: c.details_url.unwrap_or_default(),
            })
            .collect(),
    )
}

/// What the checks of one commit say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CiRead {
    /// No checks on the commit (yet).
    NoChecks,
    /// Checks are still running and none has failed.
    Running,
    /// Every check passed, or was skipped, cancelled or neutral.
    Passed,
    /// At least one check failed. `still_running`: another check has not finished yet.
    Failed { checks: Vec<FailedCheck>, still_running: bool },
    /// GitHub waits for a person to approve a run. Puppet Master never approves.
    Waiting { why: String },
}

/// Reads the checks of one commit. Only `failure`, `timed_out` and `startup_failure` are
/// failures; `action_required` waits for a person; the rest pass through.
pub fn read_ci(checks: &[ApiCheck]) -> CiRead {
    if checks.is_empty() {
        return CiRead::NoChecks;
    }
    let still_running = checks.iter().any(|c| c.status != "completed");
    let failed: Vec<FailedCheck> = checks.iter().filter(|c| is_failed(c)).map(failed_check).collect();
    if !failed.is_empty() {
        return CiRead::Failed { checks: failed, still_running };
    }
    if checks.iter().any(|c| c.conclusion.as_deref() == Some("action_required")) {
        return CiRead::Waiting { why: WAITING_APPROVAL.to_string() };
    }
    if still_running {
        CiRead::Running
    } else {
        CiRead::Passed
    }
}

fn is_failed(check: &ApiCheck) -> bool {
    check.conclusion.as_deref().is_some_and(|c| FAILED_CONCLUSIONS.contains(&c))
}

fn failed_check(check: &ApiCheck) -> FailedCheck {
    let url = [check.html_url.as_str(), check.details_url.as_str()]
        .into_iter()
        .map(github_link)
        .find(|u| !u.is_empty())
        .unwrap_or_default();
    FailedCheck {
        name: check_name(&check.name),
        conclusion: check.conclusion.clone().unwrap_or_default(),
        job_id: job_id(check),
        url,
    }
}

/// A check name for a card: plain text, one line, at most 80 characters.
fn check_name(raw: &str) -> String {
    let name = clean_text(raw, NAME_MAX_CHARS);
    if name.is_empty() {
        UNNAMED.to_string()
    } else {
        name
    }
}

/// A link Puppet Master keeps: `https://github.com/...` only, with no spaces or control characters.
/// Anything else becomes empty.
fn github_link(url: &str) -> String {
    let ok = url.starts_with(GITHUB_PREFIX)
        && url.len() <= 500
        && !url.chars().any(|c| c.is_control() || c.is_whitespace());
    if ok {
        url.to_string()
    } else {
        String::new()
    }
}

/// The job id for the log read. Only a GitHub Actions check has one; any other app has no log here.
pub fn job_id(check: &ApiCheck) -> Option<u64> {
    (check.app_slug == ACTIONS_APP).then_some(check.id)
}

/// `repos/{owner}/{repo}/pulls/{n}`.
pub fn pull_path(owner_repo: &str, n: u64) -> String {
    format!("repos/{owner_repo}/pulls/{n}")
}

/// `repos/{owner}/{repo}/commits/{sha}/check-runs`, newest run of each check. None unless the
/// commit is 40 lowercase hex and the repository name is valid.
pub fn checks_path(owner_repo: &str, sha: &str) -> Option<String> {
    (valid_owner_repo(owner_repo) && valid_sha(sha))
        .then(|| format!("repos/{owner_repo}/commits/{sha}/check-runs?per_page=100&filter=latest"))
}

/// `repos/{owner}/{repo}/actions/jobs/{id}/logs`.
pub fn job_log_path(owner_repo: &str, job: u64) -> String {
    format!("repos/{owner_repo}/actions/jobs/{job}/logs")
}

/// Reads one pull request through `gh` (a GET). A bad repository name is refused before any call.
pub fn read_pull(gh: &dyn Gh, owner_repo: &str, number: u64) -> Result<ApiPull, String> {
    if !valid_owner_repo(owner_repo) || number == 0 {
        return Err("Puppet Master can't tell which pull request this is".to_string());
    }
    let response = gh.api(&pull_path(owner_repo, number), None)?;
    if response.status != 200 {
        return Err(format!("GitHub answered {}", response.status));
    }
    parse_pull(&response.body).ok_or_else(|| "GitHub's pull request answer couldn't be read".to_string())
}

/// Reads the check runs of one commit through `gh` (a GET). A bad sha is refused before any call.
pub fn read_checks(gh: &dyn Gh, owner_repo: &str, sha: &str) -> Result<Vec<ApiCheck>, String> {
    let path = checks_path(owner_repo, sha).ok_or_else(|| "Puppet Master can't tell which commit this is".to_string())?;
    let response = gh.api(&path, None)?;
    if response.status != 200 {
        return Err(format!("GitHub answered {}", response.status));
    }
    parse_check_runs(&response.body).ok_or_else(|| "GitHub's check answer couldn't be read".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::team::sources::github::GhResponse;
    use std::sync::Mutex;

    const PULL_OPEN: &str = include_str!("fixtures/gh/pull_open.json");
    const PULL_FORK: &str = include_str!("fixtures/gh/pull_fork.json");
    const PULL_OTHER_AUTHOR: &str = include_str!("fixtures/gh/pull_other_author.json");
    const PULL_MERGED: &str = include_str!("fixtures/gh/pull_merged.json");
    const CHECKS_FAILED: &str = include_str!("fixtures/gh/checks_failed.json");
    const CHECKS_RUNNING: &str = include_str!("fixtures/gh/checks_running.json");
    const CHECKS_PASSED: &str = include_str!("fixtures/gh/checks_passed.json");
    const CHECKS_ACTION_REQUIRED: &str = include_str!("fixtures/gh/checks_action_required.json");
    const CHECKS_EXTERNAL_FAIL: &str = include_str!("fixtures/gh/checks_external_fail.json");
    const CHECKS_NONE: &str = include_str!("fixtures/gh/checks_none.json");

    const HEAD: &str = "1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d";

    /// A fake `gh`: answers every GET with one fixed response and records each path asked for.
    struct FakeGh {
        status: u16,
        body: String,
        asked: Mutex<Vec<String>>,
        runs: Mutex<u32>,
    }

    impl FakeGh {
        fn answering(status: u16, body: &str) -> Self {
            Self { status, body: body.to_string(), asked: Mutex::new(Vec::new()), runs: Mutex::new(0) }
        }
    }

    impl Gh for FakeGh {
        fn api(&self, path: &str, etag: Option<&str>) -> Result<GhResponse, String> {
            assert!(etag.is_none(), "a one-shot read sends no ETag");
            self.asked.lock().unwrap().push(path.to_string());
            Ok(GhResponse { status: self.status, etag: None, poll_interval_s: None, body: self.body.clone() })
        }

        fn run(&self, _args: &[&str]) -> Result<String, String> {
            *self.runs.lock().unwrap() += 1;
            Err("a read must never run a gh command".to_string())
        }
    }

    fn check(name: &str, status: &str, conclusion: Option<&str>) -> ApiCheck {
        ApiCheck {
            id: 7,
            name: name.to_string(),
            status: status.to_string(),
            conclusion: conclusion.map(str::to_string),
            app_slug: ACTIONS_APP.to_string(),
            html_url: "https://github.com/acme/billing/runs/7".to_string(),
            details_url: String::new(),
        }
    }

    fn failed_of(read: CiRead) -> (Vec<FailedCheck>, bool) {
        match read {
            CiRead::Failed { checks, still_running } => (checks, still_running),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn open_pull_is_read_with_its_owner_and_branches() {
        let pull = parse_pull(PULL_OPEN).expect("the open fixture reads");
        assert_eq!(pull.number, 61);
        assert_eq!(pull.state, "open");
        assert!(!pull.merged);
        assert_eq!(pull.author, "you");
        assert_eq!(pull.head_ref, "pm/issue-42-a1b2");
        assert_eq!(pull.head_sha, HEAD);
        assert_eq!(pull.head_repo.as_deref(), Some("acme/billing"));
        assert_eq!(pull.base_ref, "main");
        assert_eq!(pull.base_repo, "acme/billing");
        assert_eq!(pull.html_url, "https://github.com/acme/billing/pull/61");
    }

    #[test]
    fn fork_pull_has_another_head_repo() {
        let pull = parse_pull(PULL_FORK).expect("the fork fixture reads");
        assert_eq!(pull.head_repo.as_deref(), Some("someone/billing"));
        assert_eq!(pull.base_repo, "acme/billing");
    }

    #[test]
    fn other_author_is_read_as_is() {
        let pull = parse_pull(PULL_OTHER_AUTHOR).expect("the other-author fixture reads");
        assert_eq!(pull.author, "someone");
        assert_eq!(pull.head_ref, "pm/issue-43-c3d4");
    }

    #[test]
    fn merged_pull_is_closed_and_merged() {
        let pull = parse_pull(PULL_MERGED).expect("the merged fixture reads");
        assert_eq!(pull.state, "closed");
        assert!(pull.merged);
    }

    #[test]
    fn deleted_fork_gives_no_head_repo() {
        let json = r#"{"number":64,"state":"open","merged":false,"html_url":"https://github.com/acme/billing/pull/64",
            "user":{"login":"you"},
            "head":{"ref":"pm/issue-44-f7a8","sha":"4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f70","repo":null},
            "base":{"ref":"main","sha":"9f8e7d6c5b4a39281716151413121110f0e0d0c0","repo":{"full_name":"acme/billing"}}}"#;
        let pull = parse_pull(json).expect("a deleted fork still reads");
        assert_eq!(pull.head_repo, None);
        assert_eq!(pull.base_repo, "acme/billing");
    }

    #[test]
    fn unreadable_or_odd_pull_is_none() {
        assert!(parse_pull("not json").is_none());
        assert!(parse_pull(&PULL_OPEN.replace("1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d", "ABC")).is_none());
        assert!(parse_pull(&PULL_OPEN.replace("pm/issue-42-a1b2", "../escape")).is_none());
        assert!(parse_pull(&PULL_OPEN.replace("\"number\": 61", "\"number\": 0")).is_none());
        // No base repository: the watch can't tell where the pull request goes.
        let no_base_repo = PULL_OPEN.replace("\"repo\": { \"full_name\": \"acme/billing\", \"fork\": false }\n  }\n}", "\"repo\": null\n  }\n}");
        assert!(parse_pull(&no_base_repo).is_none());
    }

    #[test]
    fn links_outside_github_are_dropped() {
        let pull = parse_pull(&PULL_OPEN.replace("https://github.com/acme/billing/pull/61", "https://evil.example/pull/61"))
            .expect("the pull still reads");
        assert_eq!(pull.html_url, "");
        let checks = parse_check_runs(CHECKS_EXTERNAL_FAIL).unwrap();
        let (failed, _) = failed_of(read_ci(&checks));
        assert_eq!(failed[0].url, "", "neither http nor non-GitHub https links are kept");
    }

    #[test]
    fn read_ci_fails_on_failure_timed_out_and_startup_failure() {
        for conclusion in ["failure", "timed_out", "startup_failure"] {
            let (failed, still_running) = failed_of(read_ci(&[check("test", "completed", Some(conclusion))]));
            assert!(!still_running, "{conclusion}");
            assert_eq!(failed.len(), 1, "{conclusion}");
            assert_eq!(failed[0].conclusion, conclusion);
        }
    }

    #[test]
    fn read_ci_passes_cancelled_neutral_and_skipped_through() {
        for conclusion in ["cancelled", "neutral", "skipped", "stale", "success"] {
            assert_eq!(read_ci(&[check("docs", "completed", Some(conclusion))]), CiRead::Passed, "{conclusion}");
        }
        let checks = parse_check_runs(CHECKS_PASSED).unwrap();
        assert_eq!(read_ci(&checks), CiRead::Passed);
    }

    #[test]
    fn read_ci_waits_when_github_needs_an_approval() {
        let checks = parse_check_runs(CHECKS_ACTION_REQUIRED).unwrap();
        assert_eq!(read_ci(&checks), CiRead::Waiting { why: WAITING_APPROVAL.to_string() });
    }

    #[test]
    fn running_and_failed_is_failed_still_running() {
        let mut both = parse_check_runs(CHECKS_RUNNING).unwrap();
        both.extend(parse_check_runs(CHECKS_FAILED).unwrap());
        let (failed, still_running) = failed_of(read_ci(&both));
        assert!(still_running);
        assert_eq!(failed.len(), 2);
    }

    #[test]
    fn running_alone_is_running_and_no_checks_is_no_checks() {
        let running = parse_check_runs(CHECKS_RUNNING).unwrap();
        assert_eq!(read_ci(&running), CiRead::Running);
        let none = parse_check_runs(CHECKS_NONE).unwrap();
        assert!(none.is_empty());
        assert_eq!(read_ci(&none), CiRead::NoChecks);
    }

    #[test]
    fn failed_checks_keep_their_job_ids_and_links() {
        let checks = parse_check_runs(CHECKS_FAILED).unwrap();
        let (failed, still_running) = failed_of(read_ci(&checks));
        assert!(!still_running);
        assert_eq!(
            failed,
            vec![
                FailedCheck {
                    name: "lint".to_string(),
                    conclusion: "failure".to_string(),
                    job_id: Some(1001),
                    url: "https://github.com/acme/billing/runs/1001".to_string(),
                },
                FailedCheck {
                    name: "test".to_string(),
                    conclusion: "failure".to_string(),
                    job_id: Some(1002),
                    url: "https://github.com/acme/billing/runs/1002".to_string(),
                },
            ]
        );
    }

    #[test]
    fn job_id_only_for_github_actions() {
        let actions = check("test", "completed", Some("failure"));
        assert_eq!(job_id(&actions), Some(7));
        let mut other = actions.clone();
        other.app_slug = "vendor-ci".to_string();
        assert_eq!(job_id(&other), None);
        other.app_slug = String::new();
        assert_eq!(job_id(&other), None);
    }

    #[test]
    fn unreadable_check_answer_is_none() {
        assert!(parse_check_runs("{}").is_none());
        assert!(parse_check_runs("nope").is_none());
        // One entry without an id makes the whole answer unreadable.
        assert!(parse_check_runs(r#"{"check_runs":[{"name":"x","status":"completed"}]}"#).is_none());
    }

    #[test]
    fn checks_path_refuses_a_non_40_hex_sha() {
        assert_eq!(
            checks_path("acme/billing", HEAD).as_deref(),
            Some("repos/acme/billing/commits/1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d/check-runs?per_page=100&filter=latest")
        );
        assert_eq!(checks_path("acme/billing", ""), None);
        assert_eq!(checks_path("acme/billing", &HEAD[..39]), None);
        assert_eq!(checks_path("acme/billing", &format!("{HEAD}0")), None);
        assert_eq!(checks_path("acme/billing", &HEAD.replace('a', "g")), None);
        assert_eq!(checks_path("acme/billing", &HEAD.to_uppercase()), None);
        assert_eq!(checks_path("acme/billing/../x", HEAD), None);
    }

    #[test]
    fn check_names_are_one_line_and_80_chars() {
        let raw = format!("lint\n\tstep\u{202E}{}", "x".repeat(120));
        let (failed, _) = failed_of(read_ci(&[check(&raw, "completed", Some("failure"))]));
        let name = &failed[0].name;
        assert!(name.starts_with("lint step"), "{name}");
        assert!(!name.contains('\n') && !name.contains('\u{202E}'));
        assert_eq!(name.chars().count(), NAME_MAX_CHARS);
        let (blank, _) = failed_of(read_ci(&[check("  \n ", "completed", Some("failure"))]));
        assert_eq!(blank[0].name, UNNAMED);
    }

    #[test]
    fn write_words_are_absent_from_every_path_builder_output() {
        let paths = [
            pull_path("acme/billing", 61),
            checks_path("acme/billing", HEAD).unwrap(),
            job_log_path("acme/billing", 1001),
        ];
        for path in &paths {
            for word in WRITE_PATHS_NEVER {
                assert!(!path.contains(word), "{path} contains {word}");
            }
        }
    }

    #[test]
    fn read_pull_asks_one_get_and_never_runs_gh() {
        let gh = FakeGh::answering(200, PULL_OPEN);
        let pull = read_pull(&gh, "acme/billing", 61).expect("the read works");
        assert_eq!(pull.number, 61);
        assert_eq!(*gh.asked.lock().unwrap(), vec!["repos/acme/billing/pulls/61".to_string()]);
        assert_eq!(*gh.runs.lock().unwrap(), 0);
    }

    #[test]
    fn read_checks_asks_the_checks_path_only() {
        let gh = FakeGh::answering(200, CHECKS_FAILED);
        let checks = read_checks(&gh, "acme/billing", HEAD).expect("the read works");
        assert_eq!(checks.len(), 3);
        assert_eq!(*gh.asked.lock().unwrap(), vec![checks_path("acme/billing", HEAD).unwrap()]);
        assert_eq!(*gh.runs.lock().unwrap(), 0);
    }

    #[test]
    fn reads_refuse_bad_input_before_any_call_and_report_other_statuses() {
        let gh = FakeGh::answering(200, PULL_OPEN);
        assert!(read_pull(&gh, "acme/billing/../x", 61).is_err());
        assert!(read_pull(&gh, "acme/billing", 0).is_err());
        assert!(read_checks(&gh, "acme/billing", "short").is_err());
        assert!(gh.asked.lock().unwrap().is_empty());
        let not_found = FakeGh::answering(404, "{}");
        assert_eq!(read_pull(&not_found, "acme/billing", 61).unwrap_err(), "GitHub answered 404");
        let merged = FakeGh::answering(200, PULL_MERGED);
        assert!(read_pull(&merged, "acme/billing", 58).unwrap().merged);
    }
}
