//! Plain-language risk of an approval prompt: what will happen if the user says yes, and how
//! dangerous that is, for every agent.
//!
//! The assessment reads only the command (and where it runs). It never looks at which agent asked:
//! per-agent additions come from the `[risk]` table of a TOML agent definition (`RiskRule`) through
//! `TuiProfile::risk_rules`, and those can only raise a result, never lower one that a built-in rule
//! found. Words are split by the `shlex` crate (MIT OR Apache-2.0); PowerShell and cmd lines use a
//! backslash-preserving splitter in `command.rs`.

mod command;
mod paths;
mod rules;

use regex::Regex;
use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
pub use paths::Where;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

/// The result shown on approval cards (`risk` on `PanePrompt` / the bridge prompt objects).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Risk {
    pub level: RiskLevel,
    /// Short sentence for people who do not read shell: "Delete a folder outside this project".
    pub headline: String,
    /// Plain sentences, most important first (at most six).
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Finding {
    pub level: RiskLevel,
    pub rank: u8,
    pub headline: String,
    pub reason: String,
}

/// A pattern an agent definition adds (`[[risk.patterns]]`). Matched case-insensitively against the
/// whole command text.
#[derive(Debug, Clone)]
pub struct RiskRule {
    pub pattern: Regex,
    pub level: RiskLevel,
    pub headline: String,
    pub reason: String,
}

/// What the assessment reads from a detected prompt.
#[derive(Debug, Clone, Copy, Default)]
pub struct RiskInput<'a> {
    pub kind: &'a str,
    pub command: Option<&'a str>,
    /// Directory the command runs in, when the prompt names one (`cd dir && ...`).
    pub cwd: Option<&'a str>,
    /// The file a `file_edit` prompt would change.
    pub path: Option<&'a str>,
    /// The pane's project folder. Without it absolute paths cannot be judged.
    pub workspace: Option<&'a str>,
    /// Prompt text, used when there is no command.
    pub text: &'a str,
}

const MAX_REASONS: usize = 6;

fn plain(level: RiskLevel, headline: &str, reason: &str) -> Risk {
    Risk { level, headline: headline.to_string(), reasons: vec![reason.to_string()] }
}

/// Assess a prompt. Never panics, never blocks on the file system.
pub fn assess(input: &RiskInput, extra: &[RiskRule]) -> Risk {
    let Some(command) = input.command.map(str::trim).filter(|c| !c.is_empty()) else {
        return assess_without_command(input, extra);
    };
    let mut ctx = rules::Ctx::new(input.workspace, input.cwd);
    rules::analyze(command, &mut ctx);
    let mut findings = std::mem::take(&mut ctx.findings);
    let base_found = !findings.is_empty() || ctx.unknown;
    let known = !findings.is_empty();

    let mut matched_extra: Vec<Finding> = Vec::new();
    for rule in extra {
        if rule.pattern.is_match(command) {
            matched_extra.push(Finding { level: rule.level, rank: 55, headline: rule.headline.clone(), reason: rule.reason.clone() });
        }
    }
    if !matched_extra.is_empty() {
        if known {
            // Definitions can only raise what the built-in rules found.
            let top = findings.iter().map(|f| f.level).max().unwrap_or(RiskLevel::Low);
            findings.extend(matched_extra.into_iter().filter(|f| f.level >= top));
        } else {
            // Nothing built-in recognised the command: the definition decides.
            findings.extend(matched_extra);
            ctx.unknown = false;
        }
    }
    let _ = base_found;
    if findings.is_empty() {
        if ctx.unknown {
            return plain(RiskLevel::Medium, "Run a command", "It is not a command Puppet Master recognises, so check it before allowing it.");
        }
        return plain(RiskLevel::Low, "Run a simple command", "It only moves around or prints text.");
    }
    if ctx.unknown {
        findings.push(Finding { level: RiskLevel::Medium, rank: 1, headline: "Run a command".into(), reason: "Part of it is not a command Puppet Master recognises.".into() });
    }
    let top = findings.iter().max_by_key(|f| (f.level, f.rank)).cloned().expect("non-empty");
    let mut ordered = findings;
    ordered.sort_by(|a, b| (b.level, b.rank).cmp(&(a.level, a.rank)));
    let mut reasons: Vec<String> = Vec::new();
    for f in &ordered {
        if f.level == RiskLevel::Low && top.level != RiskLevel::Low && !reasons.is_empty() {
            continue; // low-level chatter is noise next to a real warning
        }
        if !reasons.contains(&f.reason) && reasons.len() < MAX_REASONS {
            reasons.push(f.reason.clone());
        }
    }
    Risk { level: top.level, headline: top.headline, reasons }
}

fn assess_without_command(input: &RiskInput, extra: &[RiskRule]) -> Risk {
    let mut risk = match input.kind {
        "workspace_trust" => plain(
            RiskLevel::Medium,
            "Trust this folder",
            "The agent will be able to read and change files here and run commands in it.",
        ),
        "file_edit" => assess_file_edit(input.path, input.workspace),
        "broad_approval" => plain(
            RiskLevel::Medium,
            "Allow without asking again",
            "It lets the agent do this kind of thing without asking each time.",
        ),
        _ => plain(RiskLevel::Medium, "Answer the agent", "The agent is asking something Puppet Master cannot summarise. Read it before answering."),
    };
    for rule in extra {
        if rule.level > risk.level && rule.pattern.is_match(input.text) {
            risk = Risk { level: rule.level, headline: rule.headline.clone(), reasons: vec![rule.reason.clone()] };
        }
    }
    risk
}

/// A file the agent wants to create or change: routine inside the project, worth a look outside it
/// or when it holds secrets or logins.
fn assess_file_edit(path: Option<&str>, workspace: Option<&str>) -> Risk {
    let Some(path) = path.map(str::trim).filter(|p| !p.is_empty()) else {
        return plain(RiskLevel::Medium, "Change a file", "The agent wants to change a file. Check which one before allowing it.");
    };
    if rules::is_secret_path(path) {
        return plain(RiskLevel::High, "Change a file that may hold secrets", "It looks like a key, a login or a settings file with passwords in it.");
    }
    match paths::locate(path, None, workspace) {
        paths::Where::Inside => plain(RiskLevel::Low, "Change a file in this project", "It only changes a file inside the project folder."),
        paths::Where::Outside => plain(RiskLevel::High, "Change a file outside this project", "The file is not in the project folder, so the change affects something else on this computer."),
        paths::Where::Unknown => plain(RiskLevel::Medium, "Change a file", "Puppet Master cannot tell where this file is, so check it before allowing it."),
    }
}

/// Should this choice be hidden behind "More options" on a high-risk prompt? True for answers that
/// mean "do not ask me again" or "allow everything".
pub fn choice_is_broad(id: &str, label: &str) -> bool {
    let text = format!("{id} {label}").to_ascii_lowercase().replace(['_', '-'], " ");
    ["always", "allow all", "approve all", "all commands", "all tools", "don't ask", "dont ask", "do not ask", "never ask", "every time", "for this session", "remember", "yes to all", "auto approve", "auto accept"]
        .iter()
        .any(|p| text.contains(p))
        || text.split_whitespace().any(|w| w == "all" || w == "forever")
}

#[cfg(test)]
mod tests;
