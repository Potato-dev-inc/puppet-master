//! Hardened git for Puppet Master's own calls on an agent's copy (spec n32 §3.5, threat S7).
//!
//! The agent runs as the person and can write `.git/config` and `.git/hooks`. A worktree shares
//! both with its origin repository, so anything the agent plants there would run inside PM's next
//! git call. Every PM git call on a copy after the agent starts therefore gets [`git_env`]: an
//! empty hooks folder and fsmonitor off, through `GIT_CONFIG_COUNT`, which beats the repo's own
//! config. Filter drivers that the repo's local config defines are blanked too (see
//! [`with_filters_off`]), because a clean filter runs during `git add` and no hook setting stops it.
//!
//! [`config_snapshot`] records what git would run (the config and the hook files). Callers keep the
//! snapshot taken when the run starts and compare it before the push card and again right before the
//! push: any difference means no push.
//!
//! [`read`] is the read-only path for a copy that an agent may be working in (spec n33 CA-2): a
//! fixed list of commands, refused options, a size cap and a deadline, and never `index.lock`.

use crate::operations::OperationError;
use super::git::{git_capped_env, git_raw};
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Hook files recorded per snapshot. More than this is cut and marked, so a huge folder cannot
/// make the snapshot unbounded.
const MAX_HOOK_FILES: usize = 500;
/// How deep the hook folders are walked.
const MAX_HOOK_DEPTH: u8 = 3;

/// The environment for PM's own git calls on a copy: no hooks (`core.hooksPath` = `empty_hooks`,
/// which should be an empty private folder), no fsmonitor, no terminal prompts.
pub fn git_env(empty_hooks: &Path) -> Vec<(String, String)> {
    let hooks = empty_hooks.to_string_lossy().into_owned();
    config_env(&[
        ("core.hooksPath".to_string(), hooks),
        ("core.fsmonitor".to_string(), "false".to_string()),
    ])
}

/// `GIT_CONFIG_COUNT` plus one key/value pair per entry, and the prompt switch.
fn config_env(pairs: &[(String, String)]) -> Vec<(String, String)> {
    let mut env = vec![("GIT_CONFIG_COUNT".to_string(), pairs.len().to_string())];
    for (index, (key, value)) in pairs.iter().enumerate() {
        env.push((format!("GIT_CONFIG_KEY_{index}"), key.clone()));
        env.push((format!("GIT_CONFIG_VALUE_{index}"), value.clone()));
    }
    env.push(("GIT_TERMINAL_PROMPT".to_string(), "0".to_string()));
    env
}

/// The config pairs carried by an env built with [`config_env`].
fn pairs_of(env: &[(String, String)]) -> Vec<(String, String)> {
    let count = env
        .iter()
        .find(|(key, _)| key == "GIT_CONFIG_COUNT")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let find = |name: String| env.iter().find(|(key, _)| *key == name).map(|(_, value)| value.clone());
    (0..count)
        .filter_map(|index| {
            let key = find(format!("GIT_CONFIG_KEY_{index}"))?;
            let value = find(format!("GIT_CONFIG_VALUE_{index}")).unwrap_or_default();
            Some((key, value))
        })
        .collect()
}

/// Borrow an env for `git_raw`.
pub(super) fn refs(env: &[(String, String)]) -> Vec<(&str, &OsStr)> {
    env.iter().map(|(key, value)| (key.as_str(), OsStr::new(value.as_str()))).collect()
}

/// The hardened env plus blank filter drivers. Only the repo's own `local` and `worktree` scopes
/// are blanked: the agent can write those, while the person's global and system settings (for
/// example Git LFS) stay as they are. An empty `env` (the plain path) comes back empty, so plain
/// calls keep their old behaviour.
pub fn with_filters_off(dir: &Path, env: &[(String, String)]) -> Result<Vec<(String, String)>, OperationError> {
    if env.is_empty() {
        return Ok(Vec::new());
    }
    let mut pairs = pairs_of(env);
    pairs.extend(local_filter_overrides(dir)?);
    let mut out: Vec<(String, String)> =
        env.iter().filter(|(key, _)| !key.starts_with("GIT_CONFIG_")).cloned().collect();
    out.extend(config_env(&pairs));
    Ok(out)
}

/// `filter.<name>.<key>` settings from the repo's own scopes, each set to a blank value. A blank
/// clean/smudge/process command runs nothing; `required` is set to false so git does not refuse.
fn local_filter_overrides(dir: &Path) -> Result<Vec<(String, String)>, OperationError> {
    // Exit 1 means "no such key": no filter drivers at all.
    let out = git_raw(dir, &["config", "--show-scope", "--name-only", "--get-regexp", r"^filter\."], &[])?;
    if !out.ok {
        return Ok(Vec::new());
    }
    Ok(filter_overrides_from(&out.stdout))
}

/// The overrides for a `scope<TAB>key` listing (`git config --show-scope --name-only`): every
/// `local` or `worktree` filter key, blank (`required` false). Other scopes are left alone.
pub(super) fn filter_overrides_from(listing: &str) -> Vec<(String, String)> {
    let mut overrides = Vec::new();
    for line in listing.lines() {
        let Some((scope, key)) = line.split_once('\t') else { continue };
        if !matches!(scope, "local" | "worktree") || !key.starts_with("filter.") {
            continue;
        }
        let value = if key.ends_with(".required") { "false" } else { "" };
        overrides.push((key.to_string(), value.to_string()));
    }
    overrides
}

/// A remote, branch or start point name we pass to git: plain ref characters only, and never an
/// option (`-x`). Refusing it up front keeps argv injection out.
pub(super) fn check_name(text: &str) -> Result<(), OperationError> {
    let plain = !text.is_empty()
        && text.len() <= 200
        && !text.starts_with('-')
        && !text.starts_with('/')
        && !text.contains("..")
        && text.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'));
    if plain {
        Ok(())
    } else {
        Err(OperationError::new("INVALID_ARGUMENT", format!("not a valid git name: {text}"), false))
    }
}

fn run(dir: &Path, args: &[&str], env: &[(String, String)]) -> Result<String, OperationError> {
    let out = git_raw(dir, args, &refs(env))?;
    if out.ok {
        Ok(out.stdout.trim().to_string())
    } else {
        Err(OperationError::new(
            "GIT_FAILED",
            format!("git {} failed: {}", args.first().unwrap_or(&""), out.stderr.trim()),
            false,
        ))
    }
}

/// Fetch `branch` from `remote` into `refs/remotes/<remote>/<branch>` and return its commit.
/// Used at the start of an issue run, from the person's own folder, with [`git_env`].
pub fn fetch_branch(
    project: &str,
    remote: &str,
    branch: &str,
    env: &[(String, String)],
) -> Result<String, OperationError> {
    check_name(remote)?;
    check_name(branch)?;
    let dir = Path::new(project);
    let tracking = format!("refs/remotes/{remote}/{branch}");
    let refspec = format!("+refs/heads/{branch}:{tracking}");
    run(dir, &["fetch", "--no-tags", "--quiet", remote, &refspec], env)?;
    run(dir, &["rev-parse", "--verify", "-q", &format!("{tracking}^{{commit}}")], &[])
}

/// What git would run for this folder: the full config list with its origins and scopes, and the
/// hook files (the hooks folder git uses, plus `.husky/`) as `(path, sha256)`, sorted. Callers
/// keep the snapshot from the start of a run and compare later. The config text is returned whole;
/// `pushcheck::git_fingerprint` keeps the local and worktree lines.
pub fn config_snapshot(dir: &Path) -> Result<(String, Vec<(String, String)>), OperationError> {
    let config = run(dir, &["config", "--list", "--show-origin", "--show-scope"], &[])?;
    let mut hooks = Vec::new();
    for folder in hook_folders(dir)? {
        collect_hooks(&folder, 0, &mut hooks);
    }
    hooks.sort();
    Ok((config, hooks))
}

/// The folders git looks in for hooks (honouring `core.hooksPath`), plus the husky folder.
fn hook_folders(dir: &Path) -> Result<Vec<PathBuf>, OperationError> {
    let configured = git_raw(dir, &["config", "--get", "core.hooksPath"], &[])?;
    let hooks = if configured.ok && !configured.stdout.trim().is_empty() {
        PathBuf::from(configured.stdout.trim())
    } else {
        PathBuf::from(run(dir, &["rev-parse", "--git-path", "hooks"], &[])?)
    };
    let hooks = if hooks.is_absolute() { hooks } else { dir.join(hooks) };
    Ok(vec![hooks, dir.join(".husky")])
}

fn collect_hooks(folder: &Path, depth: u8, out: &mut Vec<(String, String)>) {
    let Ok(read) = std::fs::read_dir(folder) else {
        return;
    };
    let mut paths: Vec<PathBuf> = read.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        if out.len() >= MAX_HOOK_FILES {
            out.push(("(more hook files not listed)".to_string(), String::new()));
            return;
        }
        let name = path.to_string_lossy().replace('\\', "/");
        if path.is_dir() {
            if depth < MAX_HOOK_DEPTH {
                collect_hooks(&path, depth + 1, out);
            }
        } else if path.is_file() {
            let digest = match std::fs::read(&path) {
                Ok(bytes) => sha256_hex(&bytes),
                Err(_) => "unreadable".to_string(),
            };
            out.push((name, digest));
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Whether `remote` has `refs/heads/<branch>` (`git ls-remote --heads`). Only an exact ref name
/// counts: a branch with the same tail (`x/pm-1`) does not.
pub fn remote_has_branch(
    dir: &Path,
    remote: &str,
    branch: &str,
    env: &[(String, String)],
) -> Result<bool, OperationError> {
    check_name(remote)?;
    check_name(branch)?;
    let wanted = format!("refs/heads/{branch}");
    let listed = run(dir, &["ls-remote", "--heads", remote, &wanted], env)?;
    Ok(listed.lines().any(|line| line.split_whitespace().nth(1) == Some(wanted.as_str())))
}

/// `git diff <args>` on a copy, for the push check (spec n32 section 3.6 C, threats S6 and S7).
/// Always adds `--no-textconv --no-ext-diff`: a diff driver (`diff.<name>.textconv` or
/// `diff.<name>.command`) is a program the agent could plant, and a diff would run it. The env must
/// be a hardened one from [`git_env`]; an empty env is refused, so the plain path cannot be used by
/// mistake. Output is returned as it is (no trim), so `-z` names and diff lines stay exact.
pub fn diff(dir: &Path, args: &[&str], env: &[(String, String)]) -> Result<String, OperationError> {
    if env.is_empty() {
        return Err(OperationError::new("INVALID_ARGUMENT", "a diff needs the hardened git env", false));
    }
    let filters = with_filters_off(dir, env)?;
    let mut all = vec!["diff", "--no-textconv", "--no-ext-diff", "--no-color", "--no-renames"];
    all.extend_from_slice(args);
    let out = git_raw(dir, &all, &refs(&filters))?;
    if out.ok {
        Ok(out.stdout)
    } else {
        Err(OperationError::new(
            "GIT_FAILED",
            format!("git diff failed: {}", out.stderr.trim()),
            false,
        ))
    }
}

/// What [`read`] returns: the stdout, and whether the output cap cut it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadOut {
    pub text: String,
    pub truncated: bool,
}

/// The only git commands [`read`] runs: three plumbing reads, and `diff` (always with textconv
/// and external diff off). None of them takes `.git/index.lock` once `GIT_OPTIONAL_LOCKS=0`.
const READ_COMMANDS: [&str; 4] = ["diff-index", "ls-files", "rev-parse", "diff"];

/// A read of a copy that PM runs while an agent may be working in it (spec n33 CA-2, threats C1
/// and C2). The command must be one of [`READ_COMMANDS`]; the caller's arguments are checked by
/// [`read_argv`]. The env must be a hardened one from [`git_env`]. For `diff-index` and `diff`
/// the repo's own filter drivers are blanked too (see [`with_filters_off`]), because a clean
/// filter runs while git hashes a changed file. `GIT_OPTIONAL_LOCKS=0` is always added, so the
/// read never takes `index.lock` or rewrites the index. Stdout is capped at `max_bytes`, and the
/// git process is killed at `timeout_ms`.
pub fn read(
    dir: &Path,
    args: &[&str],
    env: &[(String, String)],
    max_bytes: usize,
    timeout_ms: u64,
) -> Result<ReadOut, OperationError> {
    if env.is_empty() {
        return Err(refuse("a read needs the hardened git env".to_string()));
    }
    let argv = read_argv(args)?;
    let command = argv[0].clone();
    let mut hardened = if matches!(command.as_str(), "diff-index" | "diff") {
        with_filters_off(dir, env)?
    } else {
        env.to_vec()
    };
    hardened.push(("GIT_OPTIONAL_LOCKS".to_string(), "0".to_string()));
    let words: Vec<&str> = argv.iter().map(String::as_str).collect();
    let (text, truncated, timed_out) = git_capped_env(dir, &words, &refs(&hardened), max_bytes, timeout_ms)
        .map_err(|error| match error.code.as_str() {
            "GIT_FAILED" => OperationError::new("GIT_FAILED", format!("git {command} failed"), false),
            _ => error,
        })?;
    if timed_out {
        return Err(OperationError::new(
            "GIT_TIMEOUT",
            format!("git {command} took longer than {timeout_ms} ms"),
            true,
        ));
    }
    Ok(ReadOut { text, truncated })
}

/// The argv for a [`read`]: the command, the flags PM always adds, then the caller's arguments.
/// Refuses an unknown command, a NUL byte, and the options that write a file, leave the repo or
/// run a program. Pure, so the rules are tested without git.
fn read_argv(args: &[&str]) -> Result<Vec<String>, OperationError> {
    let Some((&command, rest)) = args.split_first() else {
        return Err(refuse("a read needs a git command".to_string()));
    };
    if !READ_COMMANDS.contains(&command) {
        return Err(refuse(format!("git {command} is not a read Puppet Master makes")));
    }
    if let Some(arg) = args.iter().find(|arg| arg.contains('\0')) {
        return Err(refuse(format!("a git argument has a NUL byte: {arg:?}")));
    }
    if let Some(arg) = rest.iter().find(|arg| refused_option(arg)) {
        return Err(refuse(format!("git {command} does not take {arg}")));
    }
    let mut argv = vec![command.to_string()];
    argv.extend(forced_flags(command).iter().map(|flag| flag.to_string()));
    argv.extend(rest.iter().map(|arg| arg.to_string()));
    Ok(argv)
}

/// Options a [`read`] refuses from its caller: `--no-index` (reads outside the repo), `--output*`
/// and `-O` (write a file or read an order file), `--ext-diff` and `--textconv` (run a program),
/// and a pager.
fn refused_option(arg: &str) -> bool {
    matches!(arg, "--no-index" | "--ext-diff" | "--textconv" | "--open-files-in-pager")
        || arg.starts_with("--open-files-in-pager=")
        || arg.starts_with("--output")
        || arg.starts_with("-O")
}

/// The flags [`read`] puts right after the command. Textconv is always off for both diffs; the
/// external diff, colour and rename flags are fixed for `diff`, so its output is stable to parse.
fn forced_flags(command: &str) -> &'static [&'static str] {
    match command {
        "diff-index" => &["--no-textconv"],
        "diff" => &["--no-textconv", "--no-ext-diff", "--no-color", "--no-renames"],
        _ => &[],
    }
}

fn refuse(message: String) -> OperationError {
    OperationError::new("INVALID_ARGUMENT", message, false)
}

#[cfg(test)]
mod tests;
