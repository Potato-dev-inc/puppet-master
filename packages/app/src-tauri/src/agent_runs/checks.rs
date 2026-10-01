//! State-based acceptance checks evaluated against the real filesystem.
//!
//! Deliberately limited to read-only file predicates: no command execution.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

pub const MAX_CHECKS: usize = 20;
pub const MAX_READ_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Check {
    FileExists { path: String },
    FileContains { path: String, text: String },
    FileAbsent { path: String },
}

impl Check {
    fn path(&self) -> &str {
        match self {
            Check::FileExists { path }
            | Check::FileContains { path, .. }
            | Check::FileAbsent { path } => path,
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
        resolve(project, check.path())?;
        if let Check::FileContains { text, .. } = check {
            if text.is_empty() {
                return Err("file_contains requires non-empty text".into());
            }
        }
    }
    Ok(())
}

fn evaluate_one(project: &Path, check: &Check) -> (bool, String) {
    let target = match resolve(project, check.path()) {
        Ok(target) => target,
        Err(error) => return (false, error),
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

pub fn evaluate(project: &Path, checks: &[Check]) -> Vec<CheckResult> {
    checks
        .iter()
        .map(|check| {
            let (passed, detail) = evaluate_one(project, check);
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
        let passed: Vec<bool> = evaluate(&root, &checks).iter().map(|r| r.passed).collect();
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
        let results = evaluate(&root, &[exists("../x")]);
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
        let results = evaluate(&root, &[exists("nope")]);
        assert!(failure_summary(&results).unwrap().contains("nope"));
        std::fs::write(root.join("yes"), "x").unwrap();
        assert!(failure_summary(&evaluate(&root, &[exists("yes")])).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        let root = dir("link");
        let outside = dir("outside");
        std::os::unix::fs::symlink(&outside, root.join("out")).unwrap();
        assert!(validate(&root, &[exists("out/x")]).is_err());
    }
}
