//! Running other programs for the worktree features (setup steps, `gh`, `git push`): argv only
//! (never a shell), no console window on Windows, a hard time limit, and capped output.
//!
//! Programs are looked up on `PATH` (and `PATHEXT` on Windows) by hand, because Rust's own lookup
//! on Windows only finds `.exe`, and `npm` is `npm.cmd`.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How a tool is looked up and what extra environment it gets. The default is the app's own
/// environment; tests point `path` at a folder holding a fake `gh`.
#[derive(Debug, Clone, Default)]
pub struct ToolEnv {
    /// Replaces `PATH` for both the lookup and the child.
    pub path: Option<OsString>,
    pub extra: Vec<(String, String)>,
}

impl ToolEnv {
    /// `dir` first, then the real `PATH`.
    pub fn with_dir_first(dir: &Path) -> Self {
        let mut paths: Vec<PathBuf> = vec![dir.to_path_buf()];
        if let Some(real) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&real));
        }
        Self { path: std::env::join_paths(paths).ok(), extra: Vec::new() }
    }

    pub fn with_env(mut self, key: &str, value: &str) -> Self {
        self.extra.push((key.to_string(), value.to_string()));
        self
    }

    fn search_path(&self) -> Option<OsString> {
        self.path.clone().or_else(|| std::env::var_os("PATH"))
    }

    /// The program `name` resolves to, if any.
    pub fn resolve(&self, name: &str) -> Option<PathBuf> {
        let as_path = Path::new(name);
        if as_path.components().count() > 1 || as_path.is_absolute() {
            return as_path.is_file().then(|| as_path.to_path_buf());
        }
        let paths = self.search_path()?;
        let extensions: Vec<String> = if cfg!(windows) {
            std::env::var("PATHEXT")
                .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
                .split(';')
                .filter(|ext| !ext.is_empty())
                .map(|ext| ext.to_ascii_lowercase())
                .collect()
        } else {
            vec![String::new()]
        };
        for dir in std::env::split_paths(&paths) {
            for ext in &extensions {
                let candidate = dir.join(format!("{name}{ext}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        None
    }
}

/// What a finished (or timed-out) child left behind.
#[derive(Debug, Clone)]
pub struct Ran {
    /// `None` when it was killed (time limit) or could not report a code.
    pub code: Option<i32>,
    pub timed_out: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Ran {
    pub fn ok(&self) -> bool {
        !self.timed_out && self.code == Some(0)
    }
}

/// Output kept per stream; the rest is dropped (a runaway tool must not fill memory).
const MAX_CAPTURE: usize = 256 * 1024;

fn capture<R: Read + Send + 'static>(mut stream: R) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut kept: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let room = MAX_CAPTURE.saturating_sub(kept.len());
                    kept.extend_from_slice(&chunk[..n.min(room)]);
                }
            }
        }
        String::from_utf8_lossy(&kept).into_owned()
    })
}

pub(super) fn base_command(program: &Path, args: &[String], dir: &Path, env: &ToolEnv) -> Command {
    let mut cmd = crate::proc_util::hidden_command(program);
    cmd.args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1");
    if let Some(path) = &env.path {
        cmd.env("PATH", path);
    }
    for (key, value) in &env.extra {
        cmd.env(key, value);
    }
    cmd
}

/// Kill `child` and everything it started (`npm` spawns `node`; a plain kill leaves the tree).
pub(super) fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let _ = crate::proc_util::hidden_command("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Run `program args` in `dir` with a time limit. A failure to start is `Err` (plain words).
pub fn run(
    program: &str,
    args: &[String],
    dir: &Path,
    env: &ToolEnv,
    limit: Duration,
) -> Result<Ran, String> {
    let resolved = env
        .resolve(program)
        .ok_or_else(|| format!("{program} was not found on this PC"))?;
    let mut child = base_command(&resolved, args, dir, env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not start {program}: {error}"))?;
    let out = child.stdout.take().map(capture);
    let err = child.stderr.take().map(capture);
    let deadline = Instant::now() + limit;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Ok(None) => {
                timed_out = true;
                kill_tree(&mut child);
                break None;
            }
            Err(_) => break None,
        }
    };
    let join = |handle: Option<std::thread::JoinHandle<String>>| {
        handle.and_then(|h| h.join().ok()).unwrap_or_default()
    };
    Ok(Ran {
        code: status.and_then(|s| s.code()),
        timed_out,
        stdout: join(out),
        stderr: join(err),
    })
}

/// `run` for string-literal arguments.
pub fn run_str(
    program: &str,
    args: &[&str],
    dir: &Path,
    env: &ToolEnv,
    limit: Duration,
) -> Result<Ran, String> {
    let args: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
    run(program, &args, dir, env, limit)
}

/// The last `max_lines` lines of `text`, trimmed.
pub fn tail_lines(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.len().saturating_sub(max_lines);
    lines[from..].join("\n").trim().to_string()
}
