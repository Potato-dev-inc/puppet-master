//! Probe (SPEC 4.2): learn what the program says about itself. Only fixed argument lists run:
//! `--help`, `-h`, `--version`, `help`, then `help <sub>` for subcommands found in the help text.
//! Nothing the program prints can add an argument other than a vetted subcommand name. Every run is
//! limited (see `session::Limits`), has no stdin, runs in the session work folder and is redacted.

use std::sync::atomic::{AtomicBool, Ordering};

use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};

use super::exec::{self, RunSpec};
use super::locate::probe_env;
use super::report::WizardError;
use super::session::{Limits, State, Wizard};

/// Subcommands read from the help text, at most.
pub const MAX_SUBCOMMANDS: usize = 8;

/// Never asked for help: names that could do something if a program treats `help <name>` as `<name>`.
const DENIED_SUBCOMMANDS: &[&str] = &[
    "update", "upgrade", "install", "uninstall", "login", "logout", "delete", "remove", "rm", "reset", "clean",
    "purge", "self-update", "init", "setup", "auth", "serve", "daemon",
];

const BASE: &[&[&str]] = &[&["--help"], &["-h"], &["--version"], &["help"]];

/// Tools whose `--help` or `help <topic>` opens a manual page or a browser window instead of printing
/// (git, npm, cargo ...). They are not coding agents; for them only `--version` is asked.
const OPENS_DOCUMENTATION: &[&str] = &["git", "npm", "npx", "pnpm", "yarn", "cargo", "rustup", "gh", "man"];

pub fn opens_documentation(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    OPENS_DOCUMENTATION.contains(&lower.as_str())
}

#[derive(Debug, Clone, Serialize)]
pub struct ProbeRunReport {
    pub label: String,
    pub args: Vec<String>,
    pub exit: Option<i32>,
    pub timed_out: bool,
    pub truncated: bool,
    /// Size of the cleaned output before the cap.
    pub bytes: usize,
    pub duplicate_of: Option<String>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Skipped {
    pub args: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProbeReport {
    pub runs: Vec<ProbeRunReport>,
    pub subcommands: Vec<String>,
    pub skipped: Vec<Skipped>,
}

/// A subcommand name the probe may ask help for.
pub fn valid_subcommand(name: &str) -> bool {
    let mut chars = name.chars();
    let ok = chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && name.len() <= 32
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    ok && name != "help" && !DENIED_SUBCOMMANDS.contains(&name)
}

/// The rows under a heading that ends in `commands:` (any case), up to the next blank line or heading:
/// `name   description` (clap) or `program name <args>   description` (yargs). Returns `(name,
/// description)`; names that are not allowed are left out; order is kept; no duplicates.
pub fn parse_subcommand_rows(help: &str, program: &str) -> Vec<(String, String)> {
    let program = program.trim_end_matches(".exe").to_ascii_lowercase();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut in_section = false;
    let mut seen_row = false;
    for line in help.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        let is_heading = lower.ends_with("commands:") || lower == "commands";
        if is_heading {
            in_section = true;
            seen_row = false;
            continue;
        }
        if !in_section {
            continue;
        }
        if trimmed.is_empty() {
            if seen_row {
                in_section = false;
            }
            continue;
        }
        if !line.starts_with(char::is_whitespace) {
            in_section = false; // the next heading
            continue;
        }
        seen_row = true;
        let mut words = trimmed.split_whitespace();
        let mut name = words.next().unwrap_or("").trim_end_matches(',');
        if name.trim_end_matches(".exe").eq_ignore_ascii_case(&program) {
            name = words.next().unwrap_or("").trim_end_matches(',');
        }
        if valid_subcommand(name) && !out.iter().any(|(n, _)| n == name) {
            // the description: what follows the name, without `[args..]` / `<args>` placeholders
            let description = words
                .filter(|w| !(w.starts_with('[') || w.starts_with('<') || w.ends_with("..]") || w.ends_with('>')))
                .collect::<Vec<_>>()
                .join(" ");
            out.push((name.to_string(), description));
        }
    }
    out
}

/// The subcommand names, in the order the help lists them.
pub fn parse_subcommands(help: &str, program: &str) -> Vec<String> {
    parse_subcommand_rows(help, program).into_iter().map(|(n, _)| n).collect()
}

/// Names that, in any CLI, usually mean "do one task and print the answer".
const TASK_NAMES: &[&str] = &["run", "exec", "chat", "ask", "prompt", "query", "print", "send", "message", "task"];
/// Words in a description that point at running a task, and at housekeeping.
const TASK_WORDS: &[&str] = &["run", "execute", "message", "prompt", "chat", "ask", "assistant", "session", "task", "non-interactive", "headless", "answer", "conversation"];
const HOUSEKEEPING_WORDS: &[&str] = &[
    "manage", "list", "show", "install", "config", "debug", "completion", "server", "protocol", "import", "export", "stat", "log",
    "plugin", "shell", "diagnostic", "settings", "update", "profile", "info", "issue", "github", "pull", "agents", "attach",
];

/// How likely `help <name>` documents running one task: the name counts most, then the description.
pub fn subcommand_score(name: &str, description: &str) -> i32 {
    let text = description.to_ascii_lowercase();
    let mut score = if TASK_NAMES.contains(&name) { 4 } else { 0 };
    score += TASK_WORDS.iter().filter(|w| text.contains(*w)).count() as i32;
    score -= HOUSEKEEPING_WORDS.iter().filter(|w| text.contains(*w)).count() as i32;
    score
}

/// The subcommands to ask about: the most task-like first (ties keep the help's order), at most `limit`.
pub fn ranked_subcommands(help: &str, program: &str) -> Vec<String> {
    let mut rows: Vec<(usize, i32, String)> = parse_subcommand_rows(help, program)
        .into_iter()
        .enumerate()
        .map(|(i, (name, desc))| (i, subcommand_score(&name, &desc), name))
        .collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    rows.into_iter().map(|(_, _, name)| name).collect()
}

static LOG_LINE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?m)^[ \t]*(?:INFO|DEBUG|WARN|WARNING|TRACE|ERROR)[ \t]+\d{4}-\d{2}-\d{2}T\S*.*\n?").expect("log line regex")
});
static VOLATILE: Lazy<Vec<(Regex, &'static str)>> = Lazy::new(|| {
    [
        (r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?", "<time>"),
        (r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b", "<id>"),
        (r"\+\d+(?:\.\d+)?m?s\b", "<ms>"),
        (r"(?i)\b[0-9a-f]{16,}\b", "<hex>"),
    ]
    .into_iter()
    .map(|(p, with)| (Regex::new(p).expect("volatile regex"), with))
    .collect()
});

/// Log lines (`INFO 2026-10-16T06:35:07 +62ms ...`) some CLIs print before their help: noise, and different
/// on every run. They are not shown.
pub fn strip_log_lines(text: &str) -> String {
    LOG_LINE.replace_all(text, "").into_owned()
}

/// The text with the parts that change on every run (times, ids, counters) replaced, so two runs of the
/// same help compare equal.
pub fn normalise_volatile(text: &str) -> String {
    let mut out = strip_log_lines(text);
    for (re, with) in VOLATILE.iter() {
        out = re.replace_all(&out, *with).into_owned();
    }
    out.trim().to_string()
}

fn combined(out: &exec::RunOutput) -> String {
    let stdout = strip_log_lines(&exec::clean(&out.stdout));
    let stderr = strip_log_lines(&exec::clean(&out.stderr));
    match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (_, true) => stdout,
        (true, false) => stderr,
        (false, false) => format!("{}\n{}", stdout.trim_end(), stderr),
    }
}

/// Run the probe list against `program`.
pub fn probe_program(
    command: &str,
    program: &std::path::Path,
    work_dir: &std::path::Path,
    limits: &Limits,
    cancel: &AtomicBool,
) -> ProbeReport {
    let vars = probe_env();
    let mut runs: Vec<ProbeRunReport> = Vec::new();
    let mut skipped: Vec<Skipped> = Vec::new();

    let go = |args: Vec<String>, runs: &mut Vec<ProbeRunReport>| {
        let mut spec = RunSpec::new(program, &args, work_dir);
        spec.env = &vars;
        spec.timeout = limits.probe_timeout;
        spec.max_bytes = limits.probe_max_bytes;
        spec.cancel = Some(cancel);
        let out = exec::run(&spec);
        let text = combined(&out);
        let bytes = text.len();
        let kept = exec::cut_bytes(&text, limits.probe_max_bytes);
        let same = normalise_volatile(&kept);
        let duplicate_of = (!same.is_empty())
            .then(|| runs.iter().find(|r| r.duplicate_of.is_none() && normalise_volatile(&r.text) == same).map(|r| r.label.clone()))
            .flatten();
        runs.push(ProbeRunReport {
            label: args.join(" "),
            args,
            exit: out.exit,
            timed_out: out.timed_out,
            truncated: out.truncated || bytes > kept.len(),
            bytes,
            text: if duplicate_of.is_some() { String::new() } else { kept },
            duplicate_of,
            error: out.spawn_error,
        });
    };

    let docs = opens_documentation(command);
    for base in BASE {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        if docs && base[0] != "--version" {
            skipped.push(Skipped {
                args: base.iter().map(|s| s.to_string()).collect(),
                reason: "this program opens documentation windows; its help is not asked for".into(),
            });
            continue;
        }
        if runs.len() >= limits.probe_max_runs {
            skipped.push(Skipped { args: base.iter().map(|s| s.to_string()).collect(), reason: "run limit reached".into() });
            continue;
        }
        go(base.iter().map(|s| s.to_string()).collect(), &mut runs);
    }

    // The help text subcommands come from: the first of --help, -h, help that printed something.
    // (A duplicate run has no text, so look at the original it points to.)
    let help_text = ["--help", "-h", "help"]
        .iter()
        .find_map(|label| {
            let run = runs.iter().find(|r| r.label == *label)?;
            let source = match &run.duplicate_of {
                Some(orig) => runs.iter().find(|r| &r.label == orig)?,
                None => run,
            };
            (!source.text.trim().is_empty()).then(|| source.text.clone())
        })
        .unwrap_or_default();
    let all = ranked_subcommands(&help_text, command);
    let subcommands: Vec<String> = all.iter().take(MAX_SUBCOMMANDS).cloned().collect();
    for extra in all.iter().skip(MAX_SUBCOMMANDS) {
        skipped.push(Skipped {
            args: vec![extra.clone(), "--help".into()],
            reason: format!("only {MAX_SUBCOMMANDS} subcommands are read"),
        });
    }
    // Flags live under `<sub> --help` for most CLIs (clap, yargs); `help <sub>` is the fallback for the
    // ones that print nothing useful there.
    for sub in &subcommands {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        let first = vec![sub.clone(), "--help".to_string()];
        if runs.len() >= limits.probe_max_runs {
            skipped.push(Skipped { args: first, reason: "run limit reached".into() });
            continue;
        }
        go(first, &mut runs);
        let useful = runs.last().is_some_and(|r| r.exit == Some(0) && (!r.text.trim().is_empty() || r.duplicate_of.is_some()));
        if useful {
            continue;
        }
        let second = vec!["help".to_string(), sub.clone()];
        if runs.len() >= limits.probe_max_runs {
            skipped.push(Skipped { args: second, reason: "run limit reached".into() });
            continue;
        }
        go(second, &mut runs);
    }
    ProbeReport { runs, subcommands, skipped }
}

pub fn report_json(report: &ProbeReport, limits: &Limits) -> Value {
    json!({
        "state": State::Probed.as_str(),
        "runs": report.runs,
        "subcommands": report.subcommands,
        "skipped": report.skipped,
        "limits": {
            "timeout_ms": limits.probe_timeout.as_millis() as u64,
            "max_bytes": limits.probe_max_bytes,
            "max_runs": limits.probe_max_runs,
        },
    })
}

/// `POST /agents/wizard/probe`.
pub fn run(wizard: &Wizard, body: &Value) -> Result<Value, WizardError> {
    let id = super::api::session_id(body)?;
    let snap = wizard.begin(&id, "probe", &[State::Resolved, State::Probed, State::Verified, State::Tested])?;
    let env = wizard.env();
    let report = probe_program(&snap.command, &snap.program, &snap.work_dir, &env.limits, &snap.cancel);
    let mut json = report_json(&report, &env.limits);
    let state = wizard.end(&id, |s| {
        s.probe = Some(report);
        if s.state == State::Resolved {
            s.state = State::Probed;
        }
        s.state
    })?;
    json["state"] = json!(state.as_str());
    Ok(json)
}
