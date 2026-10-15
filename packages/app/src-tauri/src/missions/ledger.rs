//! `MISSION.md`: the one file any agent or person can pick the mission up from (DESIGN-MISSIONS
//! 2.6). It lives in the copy at `.puppet-master/MISSION.md`: git-excluded (never merged, never
//! in a pull request) and outside the checkpoint shadow repo (a rollback never rewinds the notes).
//!
//! Puppet Master rewrites every section except "Notes from agents", whose text it keeps (capped at
//! 8 KB, oldest lines dropped). The notes are data: they are fenced whenever they go into a prompt.

use super::model::{Mission, Round, RoundKind};
use std::path::{Path, PathBuf};

pub const NOTES_HEADING: &str = "## Notes from agents";
const NOTES_CAP: usize = 8 * 1024;
const TRIMMED: &str = "(older notes trimmed)";

pub fn path(copy: &Path) -> PathBuf {
    copy.join(".puppet-master").join("MISSION.md")
}

/// The agents' notes in the file now (empty when there are none).
pub fn notes(copy: &Path) -> String {
    let text = std::fs::read_to_string(path(copy)).unwrap_or_default();
    notes_of(&text)
}

pub fn notes_of(text: &str) -> String {
    let Some(at) = text.find(NOTES_HEADING) else { return String::new() };
    let body = &text[at + NOTES_HEADING.len()..];
    cap(body.trim_matches(['\r', '\n']).trim_end())
}

/// Keeps the newest lines within the cap.
fn cap(notes: &str) -> String {
    if notes.len() <= NOTES_CAP {
        return notes.to_string();
    }
    let mut kept: Vec<&str> = Vec::new();
    let mut size = TRIMMED.len() + 1;
    for line in notes.lines().rev() {
        if size + line.len() + 1 > NOTES_CAP {
            break;
        }
        size += line.len() + 1;
        kept.push(line);
    }
    kept.reverse();
    format!("{TRIMMED}\n{}", kept.join("\n"))
}

fn round_line(m: &Mission, r: &Round, before: Option<String>) -> String {
    let agent = m.agent_label(r.agent);
    let checks = match (&r.verdict, before) {
        (Some(v), Some(b)) => format!("{b} → {}", v.short()),
        (Some(v), None) => v.short(),
        (None, _) => "…".into(),
    };
    let what = match (&r.kind, &r.decision) {
        (RoundKind::HandOff { from, .. }, _) => format!("Handed to {agent} from {}", m.agent_label(*from)),
        (_, Some(d)) => d.line.clone(),
        (_, None) => "Working".into(),
    };
    format!("| {} | {} | {} | {} |", r.n, agent, checks, what.replace('|', "/"))
}

/// The whole file for the mission as it is now, keeping `notes`.
pub fn render(m: &Mission, notes: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Mission: {}\n", first_line(&m.objective)));
    out.push_str("<!-- Written by Puppet Master. Agents: add notes only under \"Notes from agents\". -->\n\n");
    out.push_str("## Goal\n");
    out.push_str(m.objective.trim());
    out.push_str("\n\n## How we'll know\n");
    for (i, check) in m.checks.iter().enumerate() {
        let start = m.baseline.as_ref().and_then(|b| b.checks.get(i)).map(|c| match c.counts {
            Some(counts) if counts.total > 0 => format!(" (at the start: {} of {} pass)", counts.passed, counts.total),
            _ if c.passed => " (at the start: passes)".to_string(),
            _ => " (at the start: fails)".to_string(),
        });
        out.push_str(&format!("- {}: `{}`{}\n", check.name, check.display(), start.unwrap_or_default()));
    }
    out.push_str("\n## Rules\n");
    if m.may_change_tests {
        out.push_str("- You may change tests when the goal needs it. Every change to tests is listed for a person at the end.\n");
    } else {
        out.push_str(
            "- Make the code pass the tests as they were at the start. Don't skip, delete or loosen tests, and don't change how\n  the checks run. A person must approve any change to tests.\n",
        );
    }
    out.push_str("- You work in a separate copy. Don't merge, push or open pull requests.\n");
    out.push_str("\n## Progress (Puppet Master)\n| Turn | Agent | Checks | What happened |\n|---|---|---|---|\n");
    let mut before = m.baseline.as_ref().map(|b| b.short());
    for r in &m.rounds {
        out.push_str(&round_line(m, r, before.clone()));
        out.push('\n');
        if let Some(v) = &r.verdict {
            before = Some(v.short());
        }
    }
    let failing: Vec<String> = m
        .latest_verdict()
        .map(|v| v.checks.iter().filter(|c| !c.passed).map(|c| format!("- {}: still failing (last output in the next prompt)", c.name)).collect())
        .unwrap_or_default();
    out.push_str("\n## Still failing\n");
    if failing.is_empty() {
        out.push_str("- Nothing: every check passes.\n");
    } else {
        out.push_str(&failing.join("\n"));
        out.push('\n');
    }
    out.push('\n');
    out.push_str(NOTES_HEADING);
    out.push('\n');
    if !notes.trim().is_empty() {
        out.push_str(notes.trim_end());
        out.push('\n');
    }
    out
}

fn first_line(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    let mut short: String = line.chars().take(80).collect();
    if line.chars().count() > 80 {
        short.push('…');
    }
    short
}

/// Rewrites the file in the copy. Returns `true` when Puppet Master's own sections had been
/// edited by someone (they are put back; the round log notes it).
pub fn write(m: &Mission) -> bool {
    let Some(copy) = m.worktree.as_ref().filter(|w| w.closed.is_none()).map(|w| PathBuf::from(&w.cwd)).filter(|c| c.is_dir()) else {
        return false;
    };
    let file = path(&copy);
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    let notes = notes_of(&existing);
    let text = render(m, &notes);
    // The rules never change during a mission: an agent that rewrote them is worth recording.
    let edited = !existing.is_empty() && section(&existing, "## Rules") != section(&text, "## Rules");
    if existing != text {
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&file, text);
    }
    edited
}

fn section(text: &str, heading: &str) -> String {
    let Some(at) = text.find(heading) else { return String::new() };
    let rest = &text[at + heading.len()..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    rest[..end].trim().to_string()
}
