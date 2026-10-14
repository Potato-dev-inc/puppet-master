//! "Merge when green" (COWORK-PLAN §5.1, founder decision Q8 → b): one click, after a confirm
//! sheet, arms **GitHub's own** auto-merge with `gh pr merge <n> --auto --<method>`. GitHub then
//! merges when the required checks pass and branch protection allows it; Puppet Master never merges
//! by itself, never schedules a merge, and never runs this without the person's click.
//!
//! What the sheet shows comes from GitHub, never from a card (security review M2):
//! - only cards this app made itself from the person's own `gh` (`source: github`) offer it; a
//!   teammate's card can name any repository and number with any title;
//! - [`plan`] fetches the pull request (`repos/{repo}/pulls/{n}`) and builds the sheet from GitHub's
//!   answer: repository, number, title, author, head branch and **head commit**;
//! - [`arm`] passes `--match-head-commit <that commit>`, so GitHub refuses if anything was pushed
//!   after the person looked, and [`check_shown`] refuses if the plan changed between the sheet
//!   and the click.

use super::event::{EventSource, TeamEvent};
use super::sources::github::{owner_repo, Gh};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    Merge,
    Squash,
    Rebase,
}

impl Method {
    pub fn flag(self) -> &'static str {
        match self {
            Self::Merge => "--merge",
            Self::Squash => "--squash",
            Self::Rebase => "--rebase",
        }
    }

    /// For the confirm sheet.
    pub fn words(self) -> &'static str {
        match self {
            Self::Merge => "with a merge commit",
            Self::Squash => "squashed into one commit",
            Self::Rebase => "rebased onto the base branch",
        }
    }
}

/// What the confirm sheet shows. Every field but the method comes from GitHub's answer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergePlan {
    /// `owner/repo`.
    pub repo: String,
    pub number: u64,
    pub title: String,
    /// GitHub login of the pull request's author.
    pub author: String,
    /// The pull request's branch and the commit it points at now.
    pub head_ref: String,
    pub head_sha: String,
    pub base_ref: String,
    pub method: Method,
    pub method_words: &'static str,
    /// GitHub says auto-merge is off for this repository (only known with push access).
    pub auto_merge_off: bool,
}

/// The repository's preferred method: merge commit if allowed (it never rewrites history), else
/// squash, else rebase. Without push access GitHub hides these settings: merge commit, GitHub's
/// default.
pub fn pick_method(repo_json: &str) -> (Method, bool) {
    let v: serde_json::Value = serde_json::from_str(repo_json).unwrap_or_default();
    let allowed = |key: &str| v.get(key).and_then(|b| b.as_bool());
    let auto_off = allowed("allow_auto_merge") == Some(false);
    let method = match (allowed("allow_merge_commit"), allowed("allow_squash_merge"), allowed("allow_rebase_merge")) {
        (Some(false), Some(true), _) => Method::Squash,
        (Some(false), Some(false), Some(true)) => Method::Rebase,
        _ => Method::Merge,
    };
    (method, auto_off)
}

#[derive(Debug, Deserialize)]
struct ApiPull {
    number: u64,
    #[serde(default)]
    state: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    merged: bool,
    user: Option<ApiUser>,
    head: ApiRef,
    base: ApiRef,
}

#[derive(Debug, Deserialize)]
struct ApiUser {
    login: String,
}

#[derive(Debug, Deserialize)]
struct ApiRef {
    #[serde(rename = "ref", default)]
    name: String,
    #[serde(default)]
    sha: String,
    repo: Option<ApiRepo>,
}

#[derive(Debug, Deserialize)]
struct ApiRepo {
    full_name: String,
}

fn is_full_sha(sha: &str) -> bool {
    matches!(sha.len(), 40 | 64) && sha.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The card offers the button at all: a fact this app learned from GitHub itself, about an open
/// pull request on github.com.
pub fn offered(event: &TeamEvent) -> bool {
    event.source == EventSource::Github
        && owner_repo(&event.project.key).is_some()
        && event.subject.pr.as_ref().is_some_and(|pr| pr.state == "open")
}

/// Check the card, ask GitHub about the pull request, and work out the plan. Reads only.
pub fn plan(gh: &dyn Gh, event: &TeamEvent) -> Result<MergePlan, String> {
    if event.source != EventSource::Github {
        return Err("Merge when green is offered on GitHub's own cards only, not on a teammate's message.".into());
    }
    let repo = owner_repo(&event.project.key).ok_or("Merge when green works for GitHub pull requests only.")?;
    let number = event.subject.pr.as_ref().ok_or("This card isn't about a pull request.")?.number;
    let answer = gh
        .api(&format!("repos/{repo}/pulls/{number}"), None)
        .map_err(|e| format!("Couldn't ask GitHub about #{number}: {}", super::event::clean_text(&e, 200)))?;
    if answer.status != 200 {
        return Err(format!("GitHub doesn't show #{number} in {repo} to you (it said {}).", answer.status));
    }
    let pr: ApiPull = serde_json::from_str(&answer.body).map_err(|_| "GitHub's answer about the pull request was unreadable.".to_string())?;
    if pr.number != number || pr.base.repo.as_ref().is_some_and(|r| !r.full_name.eq_ignore_ascii_case(&repo)) {
        return Err("GitHub's answer is about another pull request.".into());
    }
    if pr.merged {
        return Err(format!("#{number} is already merged."));
    }
    if pr.state != "open" {
        return Err(format!("#{number} is closed."));
    }
    if pr.draft {
        return Err(format!("#{number} is a draft. It can be armed once it's ready for review."));
    }
    if !is_full_sha(&pr.head.sha) {
        return Err("GitHub didn't say which commit the pull request is at.".into());
    }
    let repo_json = gh.api(&format!("repos/{repo}"), None).map(|r| r.body).unwrap_or_default();
    let (method, auto_merge_off) = pick_method(&repo_json);
    Ok(MergePlan {
        repo,
        number,
        title: super::event::clean_text(&pr.title, 200),
        author: super::event::clean_text(&pr.user.map(|u| u.login).unwrap_or_default(), 40),
        head_ref: super::event::clean_text(&pr.head.name, 200),
        head_sha: pr.head.sha.to_ascii_lowercase(),
        base_ref: super::event::clean_text(&pr.base.name, 200),
        method,
        method_words: method.words(),
        auto_merge_off,
    })
}

/// The plan is still what the person saw: the same pull request at the same commit.
pub fn check_shown(plan: &MergePlan, shown_sha: &str) -> Result<(), String> {
    if !is_full_sha(shown_sha) {
        return Err("Open the sheet again: it didn't say which commit you checked.".into());
    }
    if !plan.head_sha.eq_ignore_ascii_case(shown_sha) {
        return Err(format!(
            "#{} changed since you looked (it's now at {}). Nothing was armed. Check it again.",
            plan.number,
            &plan.head_sha[..7]
        ));
    }
    Ok(())
}

/// The exact `gh` arguments: always `--auto`, never a direct merge, and only at the commit the
/// person saw.
pub fn arm_args(plan: &MergePlan) -> Vec<String> {
    vec![
        "pr".into(),
        "merge".into(),
        plan.number.to_string(),
        "--auto".into(),
        plan.method.flag().into(),
        "--match-head-commit".into(),
        plan.head_sha.clone(),
        "--repo".into(),
        plan.repo.clone(),
    ]
}

/// Arm GitHub auto-merge. The person confirmed `plan` on the sheet.
pub fn arm(gh: &dyn Gh, plan: &MergePlan) -> Result<String, String> {
    let args = arm_args(plan);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    match gh.run(&refs) {
        Ok(_) => Ok(format!("GitHub will merge #{} when its checks pass. Nothing was merged yet.", plan.number)),
        Err(error) if error.to_ascii_lowercase().contains("auto merge is not allowed")
            || error.to_ascii_lowercase().contains("auto-merge is not allowed")
            || error.contains("enablePullRequestAutoMerge") =>
        {
            Err("Auto-merge is off for this repository. An admin can turn it on in the repository's GitHub settings.".into())
        }
        Err(error) if error.to_ascii_lowercase().contains("head") && error.to_ascii_lowercase().contains("commit") => Err(format!(
            "#{} got new commits since you looked, so GitHub didn't arm it. Check it again.",
            plan.number
        )),
        Err(error) if error.trim().is_empty() => Err("GitHub didn't accept it.".into()),
        Err(error) => Err(format!("GitHub didn't accept it: {}", super::event::clean_text(&error, 200))),
    }
}
