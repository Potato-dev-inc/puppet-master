//! Agent doctor: "which of my AI agents work, and how do I fix the ones that don't?"
//!
//! Per registered CLI agent the quick check answers installed? and which version, in parallel
//! with a timeout, without spending a token. The optional deep check (`test_agent`) asks the
//! agent one tiny read-only question through `quick_ask` and turns the outcome into a plain
//! state. Nothing here names an agent: install/login hints, the version command and
//! headless capability all come from the `TuiProfile`.

use crate::operations::OperationError;
use crate::tui::{Kind, VersionCommand};
use serde::Serialize;
use serde_json::Value;
use std::io::Read;
use std::process::Stdio;
use std::time::{Duration, Instant};

/// Longest a `--version` call may take before the agent counts as broken.
pub const VERSION_TIMEOUT: Duration = Duration::from_millis(4000);
/// The deep check's question; the agent only has to echo it.
pub const TEST_PROMPT: &str = "Reply with exactly OK";
const TEST_WAIT_MS: u64 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    /// Installed and its version command works; the deep check has not run.
    Unchecked,
    /// The deep check got an answer.
    Ready,
    NotInstalled,
    NeedsLogin,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentHealth {
    pub id: String,
    pub label: String,
    pub state: HealthState,
    pub installed: bool,
    pub version: Option<String>,
    /// Plain-language sentence for the state.
    pub reason: String,
    pub install_hint: Option<String>,
    pub install_fallback: Option<String>,
    pub login_hint: Option<String>,
    /// A deep check is possible (headless, read-only capable, installed).
    pub can_test: bool,
    /// Hook Lane: whether this agent reports to Puppet Master directly, in one plain sentence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hooks: Option<crate::agent_hooks::health::HookHealth>,
}

/// What the checker needs to know about one agent (built from its profile; tests build their own).
#[derive(Debug, Clone)]
pub struct Probe {
    pub id: String,
    pub label: String,
    pub installed: bool,
    pub version_command: Option<VersionCommand>,
    pub install_hint: Option<String>,
    pub install_fallback: Option<String>,
    pub login_hint: Option<String>,
    pub can_test: bool,
}

/// Probes for every registered CLI agent (shells and protocol backends are not agents to set up).
pub fn collect_probes() -> Vec<Probe> {
    let read_only = crate::tui::read_only_backends();
    crate::tui::descriptors()
        .into_iter()
        .filter(|d| d.kind == Kind::Cli.as_str() && d.dispatchable)
        .map(|d| {
            let profile = crate::tui::profile_for(d.id);
            Probe {
                id: d.id.to_string(),
                label: d.label,
                installed: d.installed,
                version_command: if d.installed { profile.version_command() } else { None },
                install_hint: d.install_hint,
                install_fallback: d.install_fallback,
                login_hint: d.login_hint,
                can_test: d.installed && d.modes.headless && read_only.contains(&d.id),
            }
        })
        .collect()
}

/// Quick check of every agent, one thread each so a hung CLI cannot delay the others.
pub fn check_all() -> Vec<AgentHealth> {
    let handles: Vec<_> = collect_probes()
        .into_iter()
        .map(|probe| std::thread::spawn(move || check_probe(&probe, VERSION_TIMEOUT)))
        .collect();
    handles.into_iter().filter_map(|h| h.join().ok()).collect()
}

/// Quick check of one agent: installed? version?
pub fn check_probe(probe: &Probe, timeout: Duration) -> AgentHealth {
    let mut health = AgentHealth {
        id: probe.id.clone(),
        label: probe.label.clone(),
        state: HealthState::Unchecked,
        installed: probe.installed,
        version: None,
        reason: String::new(),
        install_hint: probe.install_hint.clone(),
        install_fallback: probe.install_fallback.clone(),
        login_hint: probe.login_hint.clone(),
        can_test: probe.can_test,
        hooks: None,
    };
    if !probe.installed {
        health.state = HealthState::NotInstalled;
        health.reason = "Not installed on this computer.".into();
        return health;
    }
    let installed_text = if probe.can_test {
        "Installed. Press Test to check it can answer."
    } else {
        "Installed."
    };
    let Some(command) = &probe.version_command else {
        health.reason = installed_text.into();
        return health;
    };
    match run_version_cached(command, timeout) {
        VersionOutcome::Version(version) => {
            health.hooks = crate::agent_hooks::health::for_agent(&probe.id, &probe.label, Some(&version));
            health.version = Some(version);
            health.reason = installed_text.into();
        }
        VersionOutcome::Failed(why) => {
            health.state = HealthState::Error;
            health.reason = format!("Found it, but it did not start properly: {why}");
        }
        VersionOutcome::TimedOut => {
            health.state = HealthState::Error;
            health.reason = format!(
                "Found it, but it did not answer within {} seconds.",
                timeout.as_secs().max(1)
            );
        }
    }
    health
}

/// How long a good `--version` answer is reused. The doctor, first-run and the Agents settings all
/// ask on every visit, one process per agent each time (nine PowerShells for Cursor-style launchers).
const VERSION_TTL: Duration = Duration::from_secs(30);

/// [`run_version`], but a successful answer for the same program and arguments is remembered for
/// [`VERSION_TTL`]. Failures and time-outs are never remembered, so Recheck after a fix always asks again.
fn run_version_cached(command: &VersionCommand, timeout: Duration) -> VersionOutcome {
    static VERSIONS: once_cell::sync::Lazy<crate::proc_util::TtlCache<(String, Vec<String>), String>> =
        once_cell::sync::Lazy::new(|| crate::proc_util::TtlCache::new(VERSION_TTL));
    let mut fresh = None;
    let cached = VERSIONS.get_or_probe((command.program.clone(), command.args.clone()), || {
        let outcome = run_version(command, timeout);
        let version = match &outcome {
            VersionOutcome::Version(version) => Some(version.clone()),
            _ => None,
        };
        fresh = Some(outcome);
        version
    });
    match (fresh, cached) {
        (Some(outcome), _) => outcome,
        (None, Some(version)) => VersionOutcome::Version(version),
        (None, None) => VersionOutcome::Failed("no answer".into()),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum VersionOutcome {
    Version(String),
    Failed(String),
    TimedOut,
}

/// Runs the version command with a hard timeout and returns its first non-empty line.
pub fn run_version(command: &VersionCommand, timeout: Duration) -> VersionOutcome {
    let mut process = crate::proc_util::hidden_command(&command.program);
    process
        .args(&command.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(error) => return VersionOutcome::Failed(short(&error.to_string())),
    };
    // Drain both pipes on threads so a chatty CLI cannot block on a full pipe before the deadline.
    let out = child.stdout.take().map(drain);
    let err = child.stderr.take().map(drain);
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(15)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return VersionOutcome::TimedOut;
            }
            Err(error) => return VersionOutcome::Failed(short(&error.to_string())),
        }
    };
    let stdout = out.and_then(|h| h.join().ok()).unwrap_or_default();
    let stderr = err.and_then(|h| h.join().ok()).unwrap_or_default();
    if !status.success() {
        let why = first_line(&stderr)
            .or_else(|| first_line(&stdout))
            .map(|l| short(&l))
            .unwrap_or_else(|| format!("exit code {}", status.code().unwrap_or(-1)));
        return VersionOutcome::Failed(why);
    }
    match first_line(&stdout).or_else(|| first_line(&stderr)) {
        Some(line) => VersionOutcome::Version(short(&line)),
        None => VersionOutcome::Failed("it printed no version".into()),
    }
}

fn drain<R: Read + Send + 'static>(mut pipe: R) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = pipe.read_to_string(&mut text);
        text
    })
}

fn first_line(text: &str) -> Option<String> {
    text.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string)
}

/// One short line: control characters dropped, at most 120 characters.
fn short(text: &str) -> String {
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    let clean = clean.trim();
    if clean.chars().count() > 120 {
        format!("{}...", clean.chars().take(117).collect::<String>())
    } else {
        clean.to_string()
    }
}

// ---- deep check ----------------------------------------------------------------------------

/// Words that mean "the agent wants you signed in" in any CLI's error text.
const LOGIN_WORDS: &[&str] = &[
    "log in",
    "login",
    "logged in",
    "not logged",
    "sign in",
    "signed in",
    "authenticat",
    "api key",
    "apikey",
    "unauthorized",
    "401",
    "credential",
    "/connect",
    "not authorized",
];

/// Plain state for a failed one-shot question. `code` is the run error code (for a headless
/// failure it is what `classify_headless_failure` chose) and `message` the CLI's own text.
pub fn classify_failure(code: &str, message: &str) -> (HealthState, String) {
    let lower = message.to_ascii_lowercase();
    if code == "BACKEND_UNAVAILABLE" {
        return (HealthState::NotInstalled, "Not installed on this computer.".into());
    }
    if LOGIN_WORDS.iter().any(|w| lower.contains(w)) {
        return (HealthState::NeedsLogin, "It needs you to sign in first.".into());
    }
    let detail = first_line(message).map(|l| short(&l)).unwrap_or_default();
    let reason = if detail.is_empty() {
        "It failed without saying why.".to_string()
    } else {
        format!("It failed: {detail}")
    };
    (HealthState::Error, reason)
}

/// Plain state for the whole `quick_ask` outcome.
pub fn classify_test(result: &Result<Value, OperationError>) -> (HealthState, String) {
    match result {
        Ok(value) => {
            match value.get("status").and_then(Value::as_str) {
                Some("running") => {
                    return (HealthState::Error, "It is taking too long to answer.".into())
                }
                Some("needs_input") => {
                    return (
                        HealthState::Error,
                        "It stopped to ask for permission instead of answering.".into(),
                    )
                }
                _ => {}
            }
            let answer = value.get("answer").and_then(Value::as_str).unwrap_or("").trim();
            if answer.is_empty() {
                (HealthState::Error, "It ran but gave an empty answer.".into())
            } else {
                (HealthState::Ready, "Ready. It answered the test question.".into())
            }
        }
        Err(error) => classify_failure(&error.code, &error.message),
    }
}

/// Deep check: one bounded, read-only `quick_ask`. Blocks up to about a minute; call off the UI
/// thread. Costs a few tokens, so it only ever runs when the user asks.
pub fn test_agent(
    agent_id: &str,
    registry: std::sync::Arc<parking_lot::Mutex<crate::pty::PaneRegistry>>,
    app: tauri::AppHandle,
) -> AgentHealth {
    let probe = collect_probes().into_iter().find(|p| p.id.eq_ignore_ascii_case(agent_id));
    let Some(probe) = probe else {
        return unknown_agent(agent_id);
    };
    let mut health = check_probe(&probe, VERSION_TIMEOUT);
    if health.state == HealthState::NotInstalled || health.state == HealthState::Error {
        return health;
    }
    if !probe.can_test {
        health.reason =
            "This agent cannot be tested automatically. Open it and say hello instead.".into();
        return health;
    }
    let dir = std::env::temp_dir().join("puppet-master-doctor");
    let _ = std::fs::create_dir_all(&dir);
    let request = crate::agent_runs::quick::QuickAskRequest {
        task: TEST_PROMPT.into(),
        project_path: Some(dir.to_string_lossy().into_owned()),
        backend: Some(probe.id.clone()),
        model: None,
        wait_ms: Some(TEST_WAIT_MS),
        read_only: Some(true),
        idempotency_key: None,
    };
    let outcome = crate::agent_runs::quick::quick_ask(request, registry, app, None);
    let (state, reason) = classify_test(&outcome);
    health.state = state;
    health.reason = reason;
    health
}

fn unknown_agent(id: &str) -> AgentHealth {
    AgentHealth {
        id: id.to_string(),
        label: id.to_string(),
        state: HealthState::Error,
        installed: false,
        version: None,
        reason: "Puppet Master does not know this agent.".into(),
        install_hint: None,
        install_fallback: None,
        login_hint: None,
        can_test: false,
        hooks: None,
    }
}

#[cfg(test)]
mod tests;
