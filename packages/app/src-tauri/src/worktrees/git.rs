//! The one place that spawns `git` for worktrees: no console window on Windows, no prompts, a
//! stable locale, and path quoting off.

use crate::operations::OperationError;
use std::ffi::OsStr;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

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

/// `git <args>` in `dir` with extra env, stdout capped at `max` bytes and a deadline of
/// `timeout_ms`. Returns `(text, truncated, timed_out)`; see [`run_capped`].
pub(super) fn git_capped_env(
    dir: &Path,
    args: &[&str],
    env: &[(&str, &OsStr)],
    max: usize,
    timeout_ms: u64,
) -> Result<(String, bool, bool), OperationError> {
    run_capped(command(dir, args, env), max, timeout_ms)
}

/// Run `cmd` with its stdout piped (stderr dropped) and capped at `max` bytes, and wait at most
/// `timeout_ms`. Stdout is read on its own thread, so a child that goes quiet cannot block the
/// wait. At the cap the child is killed and the text is cut (`truncated`). At the deadline the
/// child is killed and nothing is returned (`timed_out`). A child that exits non-zero on its own
/// is `GIT_FAILED`. Taking the command as a parameter is the test seam for the kill path.
pub(super) fn run_capped(
    mut cmd: Command,
    max: usize,
    timeout_ms: u64,
) -> Result<(String, bool, bool), OperationError> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(unavailable)?;
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let stdout = child.stdout.take();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(out) = stdout {
            // one byte past the cap, so the reader can tell "exactly at the cap" from "over it"
            let _ = out.take(max as u64 + 1).read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut got: Option<Vec<u8>> = None;
    loop {
        if got.is_none() {
            if let Ok(buf) = rx.try_recv() {
                got = Some(buf);
            }
        }
        if got.as_ref().is_some_and(|buf| buf.len() > max) {
            let _ = child.kill();
            let _ = child.wait();
            let (text, truncated) = finish(got.take().unwrap_or_default(), max);
            return Ok((text, truncated, false));
        }
        if let Some(status) = child.try_wait().map_err(unavailable)? {
            if got.is_none() {
                let left = deadline.saturating_duration_since(Instant::now());
                match rx.recv_timeout(left) {
                    Ok(buf) => got = Some(buf),
                    Err(RecvTimeoutError::Timeout) => return Ok((String::new(), false, true)),
                    Err(RecvTimeoutError::Disconnected) => got = Some(Vec::new()),
                }
            }
            let (text, truncated) = finish(got.take().unwrap_or_default(), max);
            if truncated || status.success() {
                return Ok((text, truncated, false));
            }
            return Err(OperationError::new("GIT_FAILED", "git stopped with an error", false));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok((String::new(), false, true));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The captured bytes as text, cut at `max` bytes on a char boundary, and whether they were cut.
fn finish(mut buf: Vec<u8>, max: usize) -> (String, bool) {
    let truncated = buf.len() > max;
    buf.truncate(max);
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if truncated {
        // lossy decoding may have ended on a replacement char from a cut sequence
        while text.ends_with('\u{FFFD}') {
            text.pop();
        }
    }
    (text, truncated)
}
