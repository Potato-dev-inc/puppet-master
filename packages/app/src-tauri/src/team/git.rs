//! The one place `team/` spawns git: no console window on Windows, never a credential prompt
//! (`GIT_TERMINAL_PROMPT=0`), a stable locale. Arguments are always an argv list, never a shell
//! string, and every ref that came from a teammate is checked by `event::is_valid_branch` /
//! `is_valid_commit` before it gets here, so it can never be read as an option.

use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub struct GitOut {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Every git invocation `team/` makes, for the safety tests (`never pushes`).
#[cfg(test)]
pub(crate) static TRACE: std::sync::Mutex<Vec<Vec<String>>> = std::sync::Mutex::new(Vec::new());

pub fn raw(dir: &Path, args: &[&str]) -> Result<GitOut, String> {
    #[cfg(test)]
    if let Ok(mut trace) = TRACE.lock() {
        trace.push(args.iter().map(|a| a.to_string()).collect());
    }
    let mut cmd = Command::new("git");
    cmd.arg("-c")
        .arg("core.quotepath=false")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().map_err(|e| format!("could not run git: {e}"))?;
    Ok(GitOut {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// Trimmed stdout, or git's error text.
pub fn run(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = raw(dir, args)?;
    if out.ok {
        Ok(out.stdout.trim().to_string())
    } else {
        Err(out.stderr.trim().to_string())
    }
}

/// `-c user.name/-c user.email` when the repo has no identity configured, then `args`.
pub fn with_identity<'a>(dir: &Path, args: &[&'a str]) -> Vec<&'a str> {
    let has = |key: &str| run(dir, &["config", "--get", key]).map(|v| !v.is_empty()).unwrap_or(false);
    let mut full = Vec::new();
    if !has("user.name") {
        full.extend(["-c", "user.name=Puppet Master"]);
    }
    if !has("user.email") {
        full.extend(["-c", "user.email=puppet-master@localhost"]);
    }
    full.extend_from_slice(args);
    full
}

/// The current branch, or `None` on a detached HEAD.
pub fn current_branch(dir: &Path) -> Option<String> {
    run(dir, &["symbolic-ref", "-q", "--short", "HEAD"]).ok().filter(|b| !b.is_empty())
}

/// Uncommitted changes, ignoring Puppet Master's own folder.
pub fn is_dirty(dir: &Path) -> bool {
    match run(dir, &["status", "--porcelain=v1", "--untracked-files=normal"]) {
        Ok(text) => text.lines().any(|line| {
            let path = line.get(3..).unwrap_or("").trim_matches('"');
            !(path == ".puppet-master" || path.starts_with(".puppet-master/"))
        }),
        Err(_) => true,
    }
}

/// `ancestor` is an ancestor of (or equal to) `descendant`.
pub fn is_ancestor(dir: &Path, ancestor: &str, descendant: &str) -> bool {
    raw(dir, &["merge-base", "--is-ancestor", ancestor, descendant]).map(|o| o.ok).unwrap_or(false)
}

pub fn rev_parse(dir: &Path, rev: &str) -> Option<String> {
    run(dir, &["rev-parse", "--verify", "-q", &format!("{rev}^{{commit}}")]).ok().filter(|s| !s.is_empty())
}

/// The repository's shared git directory (the main `.git`, also for a worktree).
pub fn common_dir(dir: &Path) -> Option<std::path::PathBuf> {
    let text = run(dir, &["rev-parse", "--git-common-dir"]).ok()?;
    let path = std::path::PathBuf::from(text);
    Some(if path.is_absolute() { path } else { dir.join(path) })
}

/// The repository's top-level folder.
pub fn toplevel(dir: &Path) -> Option<std::path::PathBuf> {
    run(dir, &["rev-parse", "--show-toplevel"]).ok().filter(|s| !s.is_empty()).map(std::path::PathBuf::from)
}
