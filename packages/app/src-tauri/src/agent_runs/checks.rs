//! State-based acceptance checks evaluated against the real filesystem.
//!
//! File/reply predicates are read-only. `command_exits` is the one check that executes something:
//! an argv (never a shell string) run in the project directory with a timeout, compared by exit
//! code. Callers can already run commands through `shell_exec`, so it grants nothing new.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const MAX_CHECKS: usize = 20;
pub const MAX_READ_BYTES: u64 = 1024 * 1024;
pub const DEFAULT_COMMAND_TIMEOUT_MS: u64 = 120_000;
pub const MAX_COMMAND_TIMEOUT_MS: u64 = 600_000;
const OUTPUT_TAIL_BYTES: usize = 600;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Check {
    FileExists { path: String },
    FileContains { path: String, text: String },
    FileAbsent { path: String },
    /// The run's final reply text contains `text` (case-sensitive).
    ResultContains { text: String },
    /// Run `command` (argv, no shell) in the project directory; passes when the exit code equals
    /// `expect_exit` (default 0). Killed after `timeout_ms`.
    CommandExits {
        command: Vec<String>,
        #[serde(default)]
        expect_exit: i32,
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
}

impl Check {
    fn path(&self) -> Option<&str> {
        match self {
            Check::FileExists { path }
            | Check::FileContains { path, .. }
            | Check::FileAbsent { path } => Some(path),
            Check::ResultContains { .. } | Check::CommandExits { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    pub check: Check,
    pub passed: bool,
    pub detail: String,
}

/// Resolve `rel` under `project`, rejecting absolute paths, `..`, and symlink escapes.
fn resolve(project: &Path, rel: &str) -> Result<PathBuf, String> {
    if rel.trim().is_empty() {
        return Err("path must be non-empty".into());
    }
    if Path::new(rel).is_absolute() {
        return Err(format!("path `{rel}` must be relative to the project"));
    }
    let mut joined = PathBuf::new();
    for component in Path::new(rel).components() {
        match component {
            Component::Normal(part) => joined.push(part),
            Component::CurDir => {}
            _ => return Err(format!("path `{rel}` must stay inside the project")),
        }
    }
    if joined.as_os_str().is_empty() {
        return Err("path must name a file inside the project".into());
    }
    let root = project
        .canonicalize()
        .map_err(|error| format!("project path is not accessible: {error}"))?;
    let target = root.join(&joined);
    // Canonicalize the deepest existing ancestor so symlinks cannot escape the project.
    let mut probe = target.as_path();
    loop {
        if let Ok(real) = probe.canonicalize() {
            if !real.starts_with(&root) {
                return Err(format!("path `{rel}` escapes the project"));
            }
            break;
        }
        match probe.parent() {
            Some(parent) => probe = parent,
            None => return Err(format!("path `{rel}` escapes the project")),
        }
    }
    Ok(target)
}

/// Validate checks at request time. Returns an error message suitable for INVALID_ARGUMENT.
pub fn validate(project: &Path, checks: &[Check]) -> Result<(), String> {
    if checks.len() > MAX_CHECKS {
        return Err(format!("at most {MAX_CHECKS} checks are allowed"));
    }
    for check in checks {
        if let Some(path) = check.path() {
            resolve(project, path)?;
        }
        match check {
            Check::FileContains { text, .. } if text.is_empty() => {
                return Err("file_contains requires non-empty text".into());
            }
            Check::ResultContains { text } if text.is_empty() => {
                return Err("result_contains requires non-empty text".into());
            }
            Check::CommandExits { command, timeout_ms, .. } => {
                if command.first().map_or(true, |program| program.trim().is_empty()) {
                    return Err("command_exits requires a non-empty command argv".into());
                }
                if timeout_ms.is_some_and(|ms| ms == 0 || ms > MAX_COMMAND_TIMEOUT_MS) {
                    return Err(format!(
                        "command_exits timeout_ms must be 1..={MAX_COMMAND_TIMEOUT_MS}"
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn tail(text: &str) -> String {
    let text = text.trim();
    if text.len() <= OUTPUT_TAIL_BYTES {
        return text.to_string();
    }
    let mut start = text.len() - OUTPUT_TAIL_BYTES;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    format!("...{}", &text[start..])
}

fn run_command(
    project: &Path,
    command: &[String],
    expect_exit: i32,
    timeout_ms: Option<u64>,
) -> (bool, String) {
    let timeout = Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_COMMAND_TIMEOUT_MS));
    let label = command.join(" ");
    let mut child = match Command::new(&command[0])
        .args(&command[1..])
        .current_dir(project)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return (false, format!("`{label}` could not start: {error}")),
    };
    // Drain pipes on threads so a chatty child cannot block on a full pipe.
    fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
            let mut buffer = Vec::new();
            if let Some(pipe) = pipe {
                let _ = pipe.take(MAX_READ_BYTES).read_to_end(&mut buffer);
            }
            String::from_utf8_lossy(&buffer).into_owned()
        })
    }
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => return (false, format!("`{label}` failed to run: {error}")),
        }
    };
    let out = stdout.join().unwrap_or_default();
    let err = stderr.join().unwrap_or_default();
    let output = tail(&format!("{out}\n{err}"));
    match status {
        None => (
            false,
            format!("`{label}` timed out after {} ms. output: {output}", timeout.as_millis()),
        ),
        Some(status) => {
            let code = status.code();
            if code == Some(expect_exit) {
                (true, format!("`{label}` exited {expect_exit}"))
            } else {
                (
                    false,
                    format!(
                        "`{label}` exited {} (expected {expect_exit}). output: {output}",
                        code.map_or("by signal".into(), |c| c.to_string())
                    ),
                )
            }
        }
    }
}

/// Run every `command_exits` check, keyed by its index. Call this outside any shared lock: it can
/// take as long as the commands' timeouts.
pub fn run_command_checks(project: &Path, checks: &[Check]) -> HashMap<usize, (bool, String)> {
    checks
        .iter()
        .enumerate()
        .filter_map(|(index, check)| match check {
            Check::CommandExits { command, expect_exit, timeout_ms } => {
                Some((index, run_command(project, command, *expect_exit, *timeout_ms)))
            }
            _ => None,
        })
        .collect()
}

fn evaluate_one(project: &Path, check: &Check, result: Option<&str>) -> (bool, String) {
    if let Check::CommandExits { .. } = check {
        return (false, "command check was not executed".into());
    }
    if let Check::ResultContains { text } = check {
        return match result {
            Some(reply) if reply.contains(text.as_str()) => {
                (true, "result contains the expected text".into())
            }
            Some(_) => (false, "result does not contain the expected text".into()),
            None => (false, "run produced no result text to check".into()),
        };
    }
    let target = match check.path().map(|path| resolve(project, path)) {
        Some(Ok(target)) => target,
        Some(Err(error)) => return (false, error),
        None => return (false, "check has no path".into()),
    };
    match check {
        Check::FileExists { path } => {
            if target.exists() {
                (true, format!("{path} exists"))
            } else {
                (false, format!("{path} does not exist"))
            }
        }
        Check::FileAbsent { path } => {
            if target.exists() || target.symlink_metadata().is_ok() {
                (false, format!("{path} exists but should be absent"))
            } else {
                (true, format!("{path} is absent"))
            }
        }
        Check::ResultContains { .. } | Check::CommandExits { .. } => unreachable!("handled above"),
        Check::FileContains { path, text } => {
            let file = match std::fs::File::open(&target) {
                Ok(file) => file,
                Err(error) => return (false, format!("cannot read {path}: {error}")),
            };
            if !file.metadata().map(|meta| meta.is_file()).unwrap_or(false) {
                return (false, format!("{path} is not a regular file"));
            }
            let mut buffer = Vec::new();
            if let Err(error) = file.take(MAX_READ_BYTES).read_to_end(&mut buffer) {
                return (false, format!("cannot read {path}: {error}"));
            }
            if String::from_utf8_lossy(&buffer).contains(text.as_str()) {
                (true, format!("{path} contains the expected text"))
            } else {
                (false, format!("{path} does not contain the expected text"))
            }
        }
    }
}

pub fn evaluate(project: &Path, checks: &[Check], result: Option<&str>) -> Vec<CheckResult> {
    evaluate_with_commands(project, checks, result, &run_command_checks(project, checks))
}

/// Like `evaluate`, using command results already produced by `run_command_checks`.
pub fn evaluate_with_commands(
    project: &Path,
    checks: &[Check],
    result: Option<&str>,
    commands: &HashMap<usize, (bool, String)>,
) -> Vec<CheckResult> {
    checks
        .iter()
        .enumerate()
        .map(|(index, check)| {
            let (passed, detail) = match commands.get(&index) {
                Some(done) => done.clone(),
                None => evaluate_one(project, check, result),
            };
            CheckResult {
                check: check.clone(),
                passed,
                detail,
            }
        })
        .collect()
}

/// Human-readable list of failing checks, or None when all passed.
pub fn failure_summary(results: &[CheckResult]) -> Option<String> {
    let failed: Vec<&str> = results
        .iter()
        .filter(|result| !result.passed)
        .map(|result| result.detail.as_str())
        .collect();
    (!failed.is_empty()).then(|| format!("acceptance checks failed: {}", failed.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(code: i32) -> Vec<String> {
        if cfg!(windows) {
            vec!["cmd".into(), "/C".into(), format!("exit {code}")]
        } else {
            vec!["sh".into(), "-c".into(), format!("exit {code}")]
        }
    }

    #[test]
    fn command_exits_compares_exit_code_and_reports_output() {
        let root = dir("cmd");
        let ok = Check::CommandExits { command: probe(0), expect_exit: 0, timeout_ms: None };
        let bad = Check::CommandExits { command: probe(3), expect_exit: 0, timeout_ms: None };
        let expected_fail =
            Check::CommandExits { command: probe(3), expect_exit: 3, timeout_ms: None };
        let missing = Check::CommandExits {
            command: vec!["definitely-not-a-real-binary-pm".into()],
            expect_exit: 0,
            timeout_ms: None,
        };
        let results = evaluate(&root, &[ok, bad, expected_fail, missing], None);
        let passed: Vec<bool> = results.iter().map(|r| r.passed).collect();
        assert_eq!(passed, vec![true, false, true, false]);
        assert!(results[1].detail.contains("exited 3"));
        assert!(results[3].detail.contains("could not start"));
    }

    #[test]
    fn command_exits_validation_rejects_empty_argv_and_bad_timeout() {
        let root = dir("cmdval");
        let empty = Check::CommandExits { command: vec![], expect_exit: 0, timeout_ms: None };
        assert!(validate(&root, &[empty]).is_err());
        let slow = Check::CommandExits { command: probe(0), expect_exit: 0, timeout_ms: Some(0) };
        assert!(validate(&root, &[slow]).is_err());
    }

    fn dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("pm-checks-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn exists(path: &str) -> Check {
        Check::FileExists { path: path.into() }
    }

    #[test]
    fn each_check_type_passes_and_fails() {
        let root = dir("types");
        std::fs::write(root.join("a.txt"), "hello world").unwrap();
        let checks = vec![
            exists("a.txt"),
            exists("missing.txt"),
            Check::FileContains { path: "a.txt".into(), text: "hello".into() },
            Check::FileContains { path: "a.txt".into(), text: "nope".into() },
            Check::FileAbsent { path: "missing.txt".into() },
            Check::FileAbsent { path: "a.txt".into() },
        ];
        let passed: Vec<bool> = evaluate(&root, &checks, None).iter().map(|r| r.passed).collect();
        assert_eq!(passed, vec![true, false, true, false, true, false]);
    }

    #[test]
    fn rejects_absolute_and_escaping_paths() {
        let root = dir("escape");
        let abs = if cfg!(windows) { "C:\\Windows\\win.ini" } else { "/etc/passwd" };
        assert!(validate(&root, &[exists(abs)]).is_err());
        assert!(validate(&root, &[exists("../x")]).is_err());
        assert!(validate(&root, &[exists("a/../../x")]).is_err());
        assert!(validate(&root, &[exists("")]).is_err());
        assert!(validate(&root, &[exists("sub/ok.txt")]).is_ok());
        let results = evaluate(&root, &[exists("../x")], None);
        assert!(!results[0].passed);
    }

    #[test]
    fn caps_check_count_and_requires_text() {
        let root = dir("cap");
        let many: Vec<Check> = (0..21).map(|i| exists(&format!("f{i}"))).collect();
        assert!(validate(&root, &many).is_err());
        assert!(validate(&root, &many[..20]).is_ok());
        let empty = Check::FileContains { path: "a".into(), text: String::new() };
        assert!(validate(&root, &[empty]).is_err());
    }

    #[test]
    fn parses_tagged_json_and_rejects_unknown_type() {
        let ok: Check = serde_json::from_str(r#"{"type":"file_exists","path":"a"}"#).unwrap();
        assert_eq!(ok, exists("a"));
        assert!(serde_json::from_str::<Check>(r#"{"type":"command","cmd":"ls"}"#).is_err());
    }

    #[test]
    fn failure_summary_lists_failures_only() {
        let root = dir("summary");
        let results = evaluate(&root, &[exists("nope")], None);
        assert!(failure_summary(&results).unwrap().contains("nope"));
        std::fs::write(root.join("yes"), "x").unwrap();
        assert!(failure_summary(&evaluate(&root, &[exists("yes")], None)).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        let root = dir("link");
        let outside = dir("outside");
        std::os::unix::fs::symlink(&outside, root.join("out")).unwrap();
        assert!(validate(&root, &[exists("out/x")]).is_err());
    }

    #[test]
    fn result_contains_checks_reply_text() {
        let root = dir("result");
        let check = Check::ResultContains { text: "PONG".into() };
        let pass = evaluate(&root, std::slice::from_ref(&check), Some("reply: PONG"));
        let miss = evaluate(&root, std::slice::from_ref(&check), Some("nope"));
        let none = evaluate(&root, std::slice::from_ref(&check), None);
        assert!(pass[0].passed && !miss[0].passed && !none[0].passed);
        assert!(validate(&root, &[Check::ResultContains { text: String::new() }]).is_err());
        assert!(validate(&root, &[check]).is_ok());
    }
}
