//! "Merge when green" (COWORK-PLAN §5.1, founder decision Q8 → b): one click, after a confirm
//! sheet, arms **GitHub's own** auto-merge with `gh pr merge <n> --auto --<method>`. GitHub then
//! merges when the required checks pass and branch protection allows it; Puppet Master never merges
//! by itself, never schedules a merge, and never runs this without the person's click.
//!
//! Only for an open GitHub pull request on a card. A teammate's event cannot reach this: it is a
//! local Tauri command, and the card only offers the button.

use super::event::TeamEvent;
use super::sources::github::{owner_repo, Gh};

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

/// What the confirm sheet shows.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergePlan {
    pub repo: String,
    pub number: u64,
    pub title: String,
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

/// Check the card and work out the plan. Reads only.
pub fn plan(gh: &dyn Gh, event: &TeamEvent) -> Result<MergePlan, String> {
    let repo = owner_repo(&event.project.key).ok_or("Merge when green works for GitHub pull requests only.")?;
    let pr = event.subject.pr.as_ref().ok_or("This card isn't about a pull request.")?;
    if matches!(pr.state.as_str(), "merged" | "closed") {
        return Err(format!("#{} is already {}.", pr.number, pr.state));
    }
    let repo_json = gh.api(&format!("repos/{repo}"), None).map(|r| r.body).unwrap_or_default();
    let (method, auto_merge_off) = pick_method(&repo_json);
    Ok(MergePlan { repo, number: pr.number, title: pr.title.clone(), method, method_words: method.words(), auto_merge_off })
}

/// The exact `gh` arguments: always `--auto`, never a direct merge.
pub fn arm_args(plan: &MergePlan) -> Vec<String> {
    vec![
        "pr".into(),
        "merge".into(),
        plan.number.to_string(),
        "--auto".into(),
        plan.method.flag().into(),
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
        Err(error) if error.trim().is_empty() => Err("GitHub didn't accept it.".into()),
        Err(error) => Err(format!("GitHub didn't accept it: {}", super::event::clean_text(&error, 200))),
    }
}
