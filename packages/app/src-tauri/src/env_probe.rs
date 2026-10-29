//! Tool presence/version probe reported under `bridge_health.env`, so agents stop installing pip
//! or pytest just to find out whether they exist. PATH lookups only, each `--version` call is
//! bounded by a short timeout, and the whole snapshot is cached for a minute.

use parking_lot::Mutex;
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const PROBE_TIMEOUT: Duration = Duration::from_millis(2500);
const CACHE_TTL: Duration = Duration::from_secs(60);

/// Cached `{python, pytest, node, git, cargo}` snapshot. Each entry is
/// `{present, version, path}`; `python` also reports which launcher answered.
pub fn env_snapshot() -> Value {
    static CACHE: OnceLock<Mutex<Option<(Instant, Value)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some((at, value)) = cache.lock().as_ref() {
        if at.elapsed() < CACHE_TTL {
            return value.clone();
        }
    }
    let value = probe_all();
    *cache.lock() = Some((Instant::now(), value.clone()));
    value
}

fn probe_all() -> Value {
    let handles: Vec<_> = [
        ("python", &["python", "python3", "py"][..], &["--version"][..]),
        ("node", &["node"], &["--version"]),
        ("git", &["git"], &["--version"]),
        ("cargo", &["cargo"], &["--version"]),
    ]
    .into_iter()
    .map(|(key, names, args)| {
        let names: Vec<&'static str> = names.to_vec();
        let args: Vec<&'static str> = args.to_vec();
        (
            key,
            std::thread::spawn(move || probe_first(&names, &args, None)),
        )
    })
    .collect();
    let mut env = serde_json::Map::new();
    let mut python_exe: Option<String> = None;
    for (key, handle) in handles {
        let entry = handle
            .join()
            .unwrap_or_else(|_| json!({"present": false, "version": Value::Null, "path": Value::Null}));
        if key == "python" {
            python_exe = entry["path"].as_str().map(str::to_string);
        }
        env.insert(key.into(), entry);
    }
    env.insert("pytest".into(), probe_pytest(python_exe.as_deref()));
    Value::Object(env)
}

/// pytest counts as present when a `pytest` executable is on PATH or `python -m pytest` works.
fn probe_pytest(python: Option<&str>) -> Value {
    let direct = probe_first(&["pytest"], &["--version"], None);
    if direct["present"] == true {
        return direct;
    }
    match python {
        Some(python) => {
            let via_module = probe_first(&[python], &["-m", "pytest", "--version"], Some(python));
            if via_module["present"] == true {
                via_module
            } else {
                direct
            }
        }
        None => direct,
    }
}

fn probe_first(names: &[&str], args: &[&str], known_path: Option<&str>) -> Value {
    probe_first_within(names, args, known_path, PROBE_TIMEOUT)
}

fn probe_first_within(names: &[&str], args: &[&str], known_path: Option<&str>, timeout: Duration) -> Value {
    for name in names {
        let path = match known_path {
            Some(path) => Some(PathBuf::from(path)),
            None => find_on_path(name),
        };
        let Some(path) = path else { continue };
        if let Some(version) = run_version(&path, args, timeout) {
            return json!({
                "present": true,
                "version": version,
                "path": path.to_string_lossy(),
            });
        }
    }
    json!({"present": false, "version": Value::Null, "path": Value::Null})
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    let extensions: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .map(|ext| ext.to_ascii_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&paths) {
        for ext in &extensions {
            let candidate = dir.join(format!("{name}{ext}"));
            if is_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn is_file(path: &Path) -> bool {
    path.is_file()
}

/// First line of the tool's version output, or `None` if it fails or exceeds the timeout. A
/// non-zero exit (for example the Windows Store `python` stub) means "not usable".
fn run_version(path: &Path, args: &[&str], timeout: Duration) -> Option<String> {
    let mut command = crate::proc_util::hidden_command(path);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().ok()?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    if !status.success() {
        return None;
    }
    let mut output = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut output);
    }
    if output.trim().is_empty() {
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut output);
        }
    }
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_reports_every_tool_with_a_stable_shape_and_is_cached() {
        let first = env_snapshot();
        for key in ["python", "pytest", "node", "git", "cargo"] {
            let entry = &first[key];
            assert!(entry["present"].is_boolean(), "{key}: {entry}");
            assert!(entry.get("version").is_some() && entry.get("path").is_some(), "{key}");
            if entry["present"] == true {
                assert!(entry["version"].is_string(), "{key}: {entry}");
            }
        }
        assert_eq!(first, env_snapshot());
    }

    #[test]
    fn missing_tool_is_reported_absent() {
        let entry = probe_first(&["definitely-not-a-real-tool-xyz"], &["--version"], None);
        assert_eq!(entry["present"], false);
        assert!(entry["version"].is_null());
    }

    #[test]
    fn cargo_is_present_when_tests_run_under_cargo() {
        // `cargo test` puts cargo on PATH for the test process on supported setups. `cargo` is rustup's proxy,
        // which starts the real one: on a machine running the whole suite that took longer than the 2.5 s the app
        // allows a probe, and the tool was then (correctly) reported absent. This test asks "does the probe find
        // cargo", not "is it quick", so it waits as long as a hung process needs to be told apart.
        if find_on_path("cargo").is_some() {
            let probe = probe_first_within(&["cargo"], &["--version"], None, Duration::from_secs(120));
            assert_eq!(probe["present"], true);
        }
    }
}
