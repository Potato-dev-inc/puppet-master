//! Issue intake: the push check (spec n32 sections 3.3 and 3.6 C, threats S6 and S7).
//!
//! Plain rules over the files, the diff and the git fingerprint of a finished run. Nothing here
//! changes the copy. [`reasons`] turns the facts into the sentences the push card shows and says
//! whether the push is held (no push button at all). The diff reads go through
//! `worktrees::harden::diff`, which runs git with the hardened env and with diff drivers off.
//!
//! Matching is case-insensitive and treats `\` and `/` the same. Name rules match at any depth, so
//! a copy cannot hide a workflow or a key in a subfolder. The rules err on the side of a card.

use super::model::{PushFacts, AUTO_PUSH_MAX_FILES};
use crate::operations::OperationError;
use crate::worktrees::harden;
use sha2::{Digest, Sha256};
use std::path::Path;

/// The most patch text the check reads. Over this, the check could not read every added line, so
/// the caller must force a card (see [`DiffText::over_cap`]).
pub const DIFF_MAX_BYTES: usize = 256 * 1024;
/// How many example lines or files a fact keeps. The rest are dropped.
const LIST_CAP: usize = 20;
/// How many names a sentence shows before "and N more".
const SHOWN: usize = 3;

const CI_FILE_NAMES: &[&str] =
    &[".gitlab-ci.yml", "azure-pipelines.yml", "jenkinsfile", "bitbucket-pipelines.yml", ".travis.yml"];
const CI_FOLDERS: &[&str] = &[".github", ".circleci", ".buildkite"];
const KEY_EXTENSIONS: &[&str] = &[".pem", ".key", ".p12", ".pfx"];
const INTERNAL_FOLDER: &str = ".puppet-master";

/// The folder parts of a path: `\` read as `/`, lower case, no empty parts and no `.` parts.
fn parts_of(path: &str) -> Vec<String> {
    path.replace('\\', "/")
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .map(|part| part.to_ascii_lowercase())
        .collect()
}

/// Files that define CI or a workflow: `.github/**`, `.circleci/**`, `.buildkite/**` and the
/// single-file CI configs.
pub fn ci_paths(files: &[String]) -> Vec<String> {
    files.iter().filter(|path| is_ci(path)).cloned().collect()
}

fn is_ci(path: &str) -> bool {
    let parts = parts_of(path);
    let Some((name, folders)) = parts.split_last() else {
        return false;
    };
    CI_FILE_NAMES.contains(&name.as_str()) || folders.iter().any(|part| CI_FOLDERS.contains(&part.as_str()))
}

/// Files that look like they hold secrets, by file name: `.env` and `.env.*`, keys and
/// certificates, `id_rsa*`, `id_ed25519*`, `.npmrc`, `.pypirc` and `credentials*.json`.
/// `.env.example` counts too: a card is cheap, a leak is not.
pub fn secret_paths(files: &[String]) -> Vec<String> {
    files.iter().filter(|path| is_secret(path)).cloned().collect()
}

fn is_secret(path: &str) -> bool {
    let Some(name) = parts_of(path).pop() else {
        return false;
    };
    name == ".env"
        || name.starts_with(".env.")
        || KEY_EXTENSIONS.iter().any(|ext| name.ends_with(ext))
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
        || name == ".npmrc"
        || name == ".pypirc"
        || (name.starts_with("credentials") && name.ends_with(".json"))
}

/// Puppet Master's own files in the copy (`.puppet-master/**`). They are never meant to be pushed.
pub fn internal_paths(files: &[String]) -> Vec<String> {
    files.iter().filter(|path| parts_of(path).iter().any(|part| part == INTERNAL_FOLDER)).cloned().collect()
}

/// One changed line of a unified diff.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Change {
    Added,
    Removed,
}

/// Walks a unified diff and calls `visit(change, path, new_line, text)` for every line inside a
/// hunk. Hunk sizes come from the `@@` header, so a `+++` or `---` line inside a hunk is content,
/// not a header. Line numbers are in the new file. `text` has no leading `+` or `-`.
fn walk(diff: &str, mut visit: impl FnMut(Change, &str, u32, &str)) {
    let mut path = String::new();
    let mut new_line = 0u32;
    let (mut old_left, mut new_left) = (0u32, 0u32);
    for line in diff.lines() {
        if old_left == 0 && new_left == 0 {
            if let Some(rest) = line.strip_prefix("+++ ") {
                path = file_of(rest);
            } else if line.starts_with("diff --git ") {
                path.clear();
            } else if let Some(header) = line.strip_prefix("@@ ") {
                let (old, new, start) = hunk_counts(header);
                (old_left, new_left) = (old, new);
                new_line = start.saturating_sub(1);
            }
            continue;
        }
        match line.as_bytes().first() {
            Some(b'+') => {
                new_left = new_left.saturating_sub(1);
                new_line += 1;
                visit(Change::Added, &path, new_line, &line[1..]);
            }
            Some(b'-') => {
                old_left = old_left.saturating_sub(1);
                visit(Change::Removed, &path, new_line, &line[1..]);
            }
            Some(b'\\') => {}
            _ => {
                old_left = old_left.saturating_sub(1);
                new_left = new_left.saturating_sub(1);
                new_line += 1;
            }
        }
    }
}

/// The new file name of a `+++ b/<name>` header. `/dev/null` (a deleted file) gives an empty name.
fn file_of(rest: &str) -> String {
    let name = rest.trim_end_matches('\t').trim_matches('"');
    if name == "/dev/null" {
        return String::new();
    }
    name.strip_prefix("b/").unwrap_or(name).to_string()
}

/// The text after `@@ `: `-a,b +c,d @@ ...`. Returns (old count, new count, new start). A missing
/// count is 1, as git writes it.
fn hunk_counts(header: &str) -> (u32, u32, u32) {
    let mut parts = header.split_whitespace();
    let (_, old_count) = range_part(parts.next().unwrap_or("-0"));
    let (new_start, new_count) = range_part(parts.next().unwrap_or("+0"));
    (old_count, new_count, new_start)
}

/// `-1,3` or `+5` as (start, count).
fn range_part(part: &str) -> (u32, u32) {
    let body = part.trim_start_matches(|c: char| c == '-' || c == '+');
    let mut bits = body.split(',');
    let start = bits.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let count = bits.next().and_then(|v| v.parse().ok()).unwrap_or(1);
    (start, count)
}

/// Lower case with `/` separators, for matching a home folder inside a line.
fn norm_text(text: &str) -> String {
    text.replace('\\', "/").to_ascii_lowercase()
}

/// The `+` lines of a unified diff that the `hit` test flags (secret-looking), and the `+` lines that
/// contain the home folder path, each as (file, new line number). Removed and context lines are
/// not scanned. Each list keeps at most 20 entries.
pub fn scan_added(
    unified_diff: &str,
    hit: &dyn Fn(&str) -> bool,
    home: Option<&str>,
) -> (Vec<(String, u32)>, Vec<(String, u32)>) {
    let home = home
        .map(|h| norm_text(h.trim_end_matches(|c: char| c == '/' || c == '\\')))
        .filter(|h| h.len() >= 3);
    let mut secrets = Vec::new();
    let mut homes = Vec::new();
    walk(unified_diff, |change, path, line, text| {
        if change != Change::Added {
            return;
        }
        if secrets.len() < LIST_CAP && hit(text) {
            secrets.push((path.to_string(), line));
        }
        if homes.len() < LIST_CAP {
            if let Some(home) = &home {
                if norm_text(text).contains(home.as_str()) {
                    homes.push((path.to_string(), line));
                }
            }
        }
    });
    (secrets, homes)
}

/// Added and removed line counts of a unified diff, as `(additions, deletions)`.
pub fn diff_totals(unified_diff: &str) -> (u32, u32) {
    let (mut additions, mut deletions) = (0, 0);
    walk(unified_diff, |change, _, _, _| match change {
        Change::Added => additions += 1,
        Change::Removed => deletions += 1,
    });
    (additions, deletions)
}

/// A digest of what git would run for a copy: the config lines whose scope is `local` or `worktree`
/// (the repo's own settings, which the agent can write) and the hook files as `(path, sha256)`.
/// `config_list` is `git config --list --show-origin --show-scope` output. Line order does not
/// matter, and global or system settings are left out.
pub fn git_fingerprint(config_list: &str, hooks: &[(String, String)]) -> String {
    let mut config: Vec<&str> = config_list
        .lines()
        .filter(|line| matches!(line.split('\t').next(), Some("local" | "worktree")))
        .collect();
    config.sort_unstable();
    let mut hook_rows: Vec<String> = hooks.iter().map(|(path, digest)| format!("{path}\t{digest}")).collect();
    hook_rows.sort_unstable();
    let mut hasher = Sha256::new();
    for line in &config {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    hasher.update(b"hooks\n");
    for row in &hook_rows {
        hasher.update(row.as_bytes());
        hasher.update(b"\n");
    }
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The fingerprint of a `harden::config_snapshot`. Take it when the run starts and again before the
/// push card and right before the push: any difference means no push.
pub fn fingerprint_of(snapshot: &(String, Vec<(String, String)>)) -> String {
    git_fingerprint(&snapshot.0, &snapshot.1)
}

/// The sentences for the push card, and whether the push is held. An empty list means the push may
/// go on without a card. `held` means no push button at all: only "Look first" and "Throw away".
/// A held push ends its list with a line saying so.
pub fn reasons(facts: &PushFacts, ask_before_push: bool) -> (Vec<String>, bool) {
    let held = facts.git_changed || facts.remote_branch_exists || !facts.internal_files.is_empty();
    let mut out = Vec::new();
    if !facts.ci_files.is_empty() {
        out.push(format!("Changes CI or workflow files: {}.", names(&facts.ci_files)));
    }
    if !facts.secret_files.is_empty() {
        out.push(format!("Adds files that look like secrets: {}.", names(&facts.secret_files)));
    }
    if !facts.secret_lines.is_empty() {
        out.push(format!("Adds lines that look like a key or token: {}.", places(&facts.secret_lines)));
    }
    if !facts.home_path_lines.is_empty() {
        out.push(format!("Adds lines with your home folder path: {}.", places(&facts.home_path_lines)));
    }
    if facts.files as usize > AUTO_PUSH_MAX_FILES {
        out.push(format!("Changes {} files, more than {}.", facts.files, AUTO_PUSH_MAX_FILES));
    }
    if facts.run_status != "completed" {
        out.push(format!("The run did not finish (status: {}).", facts.run_status));
    }
    if !facts.internal_files.is_empty() {
        out.push(format!("Includes Puppet Master's own files: {}.", names(&facts.internal_files)));
    }
    if facts.git_changed {
        out.push("The git settings or hooks changed while the agent worked.".to_string());
    }
    if facts.remote_branch_exists {
        out.push("A branch with this name already exists on GitHub.".to_string());
    }
    if ask_before_push {
        out.push("You asked Puppet Master to check each push first.".to_string());
    }
    if held {
        out.push("Puppet Master won't push this. Check the copy yourself.".to_string());
    }
    (out, held)
}

/// Up to three names, then "and N more".
fn names(items: &[String]) -> String {
    tail(items.iter().take(SHOWN).cloned().collect(), items.len())
}

/// Up to three `path line N` places, then "and N more".
fn places(items: &[(String, u32)]) -> String {
    let shown: Vec<String> = items.iter().take(SHOWN).map(|(path, line)| format!("{path} line {line}")).collect();
    tail(shown, items.len())
}

fn tail(shown: Vec<String>, total: usize) -> String {
    let more = total.saturating_sub(SHOWN);
    if more == 0 {
        shown.join(", ")
    } else {
        format!("{} and {} more", shown.join(", "), more)
    }
}

/// The patch from a base commit to HEAD on a copy, cut at [`DIFF_MAX_BYTES`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffText {
    pub text: String,
    /// True when the patch was longer than the cap. The caller must then force a card.
    pub over_cap: bool,
}

/// A base must be a commit id (hex only). Anything else is refused before git sees it.
fn range_of(base: &str) -> Result<String, OperationError> {
    let plain = (7..=64).contains(&base.len()) && base.chars().all(|c| c.is_ascii_hexdigit());
    if plain {
        Ok(format!("{base}..HEAD"))
    } else {
        Err(OperationError::new("INVALID_ARGUMENT", "not a commit id", false))
    }
}

/// The paths changed between `base` and HEAD on a copy. `-z` keeps odd names exact.
pub fn changed_files(copy: &Path, base: &str, env: &[(String, String)]) -> Result<Vec<String>, OperationError> {
    let range = range_of(base)?;
    let out = harden::diff(copy, &["--name-only", "-z", &range], env)?;
    Ok(out.split('\0').filter(|name| !name.is_empty()).map(str::to_string).collect())
}

/// The patch from `base` to HEAD on a copy, cut at [`DIFF_MAX_BYTES`] on a character boundary.
/// `env` must be the hardened env (`harden::git_env`).
pub fn push_diff(copy: &Path, base: &str, env: &[(String, String)]) -> Result<DiffText, OperationError> {
    let range = range_of(base)?;
    let mut text = harden::diff(copy, &[&range], env)?;
    let over_cap = text.len() > DIFF_MAX_BYTES;
    if over_cap {
        let mut end = DIFF_MAX_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    Ok(DiffText { text, over_cap })
}
