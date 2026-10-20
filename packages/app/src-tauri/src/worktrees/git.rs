//! The one place that spawns `git` for worktrees: no console window on Windows, no prompts, a
//! stable locale, and path quoting off.

use crate::operations::OperationError;
use std::ffi::OsStr;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug)]
pub(super) struct GitOut {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

fn command(dir: &Path, args: &[&str], env: &[(&str, &OsStr)]) -> Command {
    let mut cmd = crate::proc_util::hidden_command("git");
    cmd.arg("-c")
        .arg("core.quotepath=false")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null());
    for (key, value) in env {
        cmd.env(key, value);
    }
    cmd
}

fn unavailable(error: std::io::Error) -> OperationError {
    OperationError::new(
        "GIT_UNAVAILABLE",
        format!("could not run git: {error}. Install git and make sure it is on PATH."),
        false,
    )
}

/// Run git; a non-zero exit is data (`ok: false`), only a failure to spawn is an error.
pub(super) fn git_raw(
    dir: &Path,
    args: &[&str],
    env: &[(&str, &OsStr)],
) -> Result<GitOut, OperationError> {
    let output = command(dir, args, env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(unavailable)?;
    Ok(GitOut {
        ok: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Trimmed stdout, or an error carrying git's message under `code`.
pub(super) fn git_ok(dir: &Path, args: &[&str], code: &str) -> Result<String, OperationError> {
    let out = git_raw(dir, args, &[])?;
    if out.ok {
        Ok(out.stdout.trim().to_string())
    } else {
        Err(OperationError::new(
            code,
            format!("git {} failed: {}", args.first().unwrap_or(&""), out.stderr.trim()),
            false,
        ))
    }
}

/// stdout capped at `max` bytes (cut on a char boundary); the child is killed once the cap is
/// hit, so a huge diff never lands in memory.
pub(super) fn git_limited(
    dir: &Path,
    args: &[&str],
    max: usize,
) -> Result<(String, bool), OperationError> {
    let mut child = command(dir, args, &[])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(unavailable)?;
    let mut buf = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        let _ = stdout.take(max as u64 + 1).read_to_end(&mut buf);
    }
    let truncated = buf.len() > max;
    if truncated {
        let _ = child.kill();
        buf.truncate(max);
    }
    let _ = child.wait();
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if truncated {
        // lossy decoding may have ended on a replacement char from a cut sequence
        while text.ends_with('\u{FFFD}') {
            text.pop();
        }
    }
    Ok((text, truncated))
}
