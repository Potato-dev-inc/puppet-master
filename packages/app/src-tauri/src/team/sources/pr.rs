//! "…and opened a PR": after a push, ask GitHub (through the person's own `gh` login, so Puppet
//! Master holds no GitHub secret) whether the pushed branch has an open pull request (plan §6.1
//! step 2). `gh pr create` often follows the push by a few seconds, so a miss is retried at
//! [`RETRY_AFTER_MS`]. Without `gh`, or for a remote that is not on GitHub, it quietly finds nothing.

use crate::team::event::{is_valid_branch, PrRef};
use std::path::Path;
use std::process::Stdio;

/// Delays after a push at which the lookup is retried.
pub const RETRY_AFTER_MS: [u64; 2] = [30_000, 120_000];

/// Looks up the open pull request for a branch (a fake in tests).
pub trait PrLookup: Send + Sync {
    fn open_pr_for(&self, repo: &Path, branch: &str) -> Option<PrRef>;
}

/// `gh pr list --head <branch> --state open --json …` in the repository.
pub struct GhCli;

impl PrLookup for GhCli {
    fn open_pr_for(&self, repo: &Path, branch: &str) -> Option<PrRef> {
        if !is_valid_branch(branch) {
            return None;
        }
        let mut cmd = crate::proc_util::hidden_command("gh");
        cmd.args([
            "pr", "list", "--head", branch, "--state", "open", "--limit", "1",
            "--json", "number,title,url,isDraft,state,changedFiles,additions,deletions",
        ])
        .current_dir(repo)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
        let out = cmd.output().ok()?;
        if !out.status.success() {
            return None;
        }
        parse_pr_list(&String::from_utf8_lossy(&out.stdout))
    }
}

/// The first entry of `gh pr list --json …` output.
pub fn parse_pr_list(json: &str) -> Option<PrRef> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let first = value.as_array()?.first()?;
    let number = first.get("number")?.as_u64().filter(|n| *n > 0)?;
    let url = first.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let as_u32 = |key: &str| first.get(key).and_then(|v| v.as_u64()).map(|n| n.min(u64::from(u32::MAX)) as u32);
    Some(PrRef {
        number,
        title: first.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        url: if url.starts_with("https://") { url } else { String::new() },
        state: first.get("state").and_then(|v| v.as_str()).unwrap_or("OPEN").to_ascii_lowercase(),
        draft: first.get("isDraft").and_then(|v| v.as_bool()).unwrap_or(false),
        files: as_u32("changedFiles"),
        additions: as_u32("additions"),
        deletions: as_u32("deletions"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_first_open_pr() {
        let json = r#"[{"number":142,"title":"Add OAuth token refresh","url":"https://github.com/acme/billing/pull/142","isDraft":false,"state":"OPEN","changedFiles":6,"additions":212,"deletions":34}]"#;
        let pr = parse_pr_list(json).unwrap();
        assert_eq!(pr.number, 142);
        assert_eq!(pr.title, "Add OAuth token refresh");
        assert_eq!(pr.files, Some(6));
        assert_eq!(pr.state, "open");
        assert!(!pr.draft);
    }

    #[test]
    fn no_pr_and_bad_output_find_nothing() {
        assert_eq!(parse_pr_list("[]"), None);
        assert_eq!(parse_pr_list("not json"), None);
        assert_eq!(parse_pr_list(r#"[{"number":0}]"#), None);
        let odd = parse_pr_list(r#"[{"number":3,"url":"javascript:alert(1)"}]"#).unwrap();
        assert_eq!(odd.url, "", "only https links are kept");
    }
}
