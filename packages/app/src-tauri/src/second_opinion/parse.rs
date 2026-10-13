//! Reads a reviewer's answer. The prompt asks for `FILE:LINE | severity | finding` lines and a
//! `VERDICT:` line, but agents drift (bullets, tables, bold, code fences, JSON). The parser takes
//! every line it can understand and ignores the rest; if no line works it tries a JSON block; if
//! that fails too the whole answer is kept as plain text so the user still sees it.
//!
//! No agent names here: the same rules read every reviewer.

use super::{Finding, ParsedReview, Severity, Verdict, VerdictLevel};
use serde_json::Value;

const MAX_FINDINGS: usize = 100;
const MAX_TEXT_CHARS: usize = 600;
const MAX_UNSTRUCTURED_CHARS: usize = 4_000;

pub fn parse_review(answer: &str) -> ParsedReview {
    let mut findings = Vec::new();
    let mut verdict = None;
    for raw in answer.lines() {
        let line = clean_line(raw);
        if line.is_empty() {
            continue;
        }
        if verdict.is_none() {
            if let Some(v) = parse_verdict(&line) {
                verdict = Some(v);
                continue;
            }
        }
        if findings.len() < MAX_FINDINGS {
            if let Some(f) = parse_finding_line(&line) {
                findings.push(f);
            }
        }
    }
    if findings.is_empty() {
        if let Some((json_findings, json_verdict)) = parse_json(answer) {
            findings = json_findings;
            verdict = verdict.or(json_verdict);
        }
    }
    for (i, f) in findings.iter_mut().enumerate() {
        f.id = format!("f{}", i + 1);
    }
    let unstructured = (findings.is_empty() && verdict.is_none())
        .then(|| cut(answer.trim(), MAX_UNSTRUCTURED_CHARS))
        .filter(|text| !text.is_empty());
    ParsedReview { findings, verdict, unstructured }
}

/// Strip list markers, table pipes, bold and backticks around a line.
fn clean_line(raw: &str) -> String {
    let mut s = raw.trim();
    if s.starts_with("```") {
        return String::new();
    }
    s = s.trim_start_matches(|c: char| c == '-' || c == '*' || c == '•' || c == '>' || c.is_whitespace());
    // "1. " / "2) "
    let digits = s.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits <= 3 {
        let rest = &s[digits..];
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            s = rest.trim_start();
        }
    }
    let s = s.trim_matches('|').trim();
    s.replace("**", "").replace('`', "").trim().to_string()
}

fn parse_verdict(line: &str) -> Option<Verdict> {
    let head: String = line.chars().take(7).collect();
    if !head.eq_ignore_ascii_case("verdict") {
        return None;
    }
    let rest = &line[7..];
    // Needs a separator right after the word ("verdict: x", "verdict - x"), so a sentence that
    // merely starts with "Verdicts are..." is not taken for one.
    if !rest.starts_with(|c: char| c == ':' || c == '-' || c == '=' || c == '|' || c.is_whitespace()) {
        return None;
    }
    let rest = rest.trim_start_matches(|c: char| c == ':' || c == '-' || c == '=' || c == '|' || c.is_whitespace());
    let (word, tail) = match rest.split_once(|c: char| c == '|' || c == '-' || c == ':' || c == '—' || c == ',') {
        Some((w, t)) => (w.trim(), t.trim()),
        None => {
            let mut it = rest.splitn(2, char::is_whitespace);
            (it.next().unwrap_or("").trim(), it.next().unwrap_or("").trim())
        }
    };
    let level = level_of(word);
    let text = if level == VerdictLevel::Unknown { rest } else { tail };
    Some(Verdict { level, text: cut(text.trim(), MAX_TEXT_CHARS) })
}

fn level_of(word: &str) -> VerdictLevel {
    let first = word.split_whitespace().next().unwrap_or("");
    let w = first.trim_matches(|c: char| !c.is_alphanumeric()).to_ascii_lowercase();
    match w.as_str() {
        "approve" | "approved" | "lgtm" | "ok" | "pass" | "good" | "fine" | "clean" | "looks" => VerdictLevel::Approve,
        "concerns" | "concern" | "minor" | "caution" | "warn" | "warning" | "comment" | "comments" => VerdictLevel::Concerns,
        "changes" | "reject" | "rejected" | "block" | "blocked" | "fail" | "failed" | "request" => VerdictLevel::Changes,
        _ => VerdictLevel::Unknown,
    }
}

fn severity_of(word: &str) -> Option<Severity> {
    let w = word.trim_matches(|c: char| !c.is_alphanumeric()).to_ascii_lowercase();
    Some(match w.as_str() {
        "high" | "critical" | "blocker" | "blocking" | "major" | "error" | "severe" | "bug" => Severity::High,
        "medium" | "med" | "moderate" | "warning" | "warn" => Severity::Medium,
        "low" | "minor" | "nit" | "nitpick" | "style" | "suggestion" => Severity::Low,
        "info" | "note" | "question" | "praise" | "fyi" => Severity::Info,
        _ => return None,
    })
}

/// `(file, line, end_line)` from `src/a.ts:42`, `src/a.ts:42-50`, `src/a.ts:L42`, `src/a.ts#L42`,
/// `C:\x\a.ts:42` or a bare `src/a.ts`.
fn parse_location(loc: &str) -> Option<(String, Option<u32>, Option<u32>)> {
    let loc = loc.trim();
    if loc.is_empty() || loc.len() > 300 || loc.chars().any(char::is_control) {
        return None;
    }
    let (file, spec) = match loc.rsplit_once('#') {
        Some((f, s)) if s.starts_with('L') || s.starts_with('l') => (f, Some(s)),
        _ => match loc.rsplit_once(':') {
            Some((f, s)) if looks_like_line(s) => (f, Some(s)),
            _ => (loc, None),
        },
    };
    let file = file.trim();
    if file.is_empty() {
        return None;
    }
    let (line, end) = match spec {
        None => (None, None),
        Some(spec) => {
            let spec = spec.trim_start_matches(['L', 'l']);
            let mut parts = spec.splitn(2, ['-', '–']);
            let first = parts.next().and_then(|p| p.trim().parse::<u32>().ok()).filter(|n| *n > 0);
            let second = parts
                .next()
                .and_then(|p| p.trim().trim_start_matches(['L', 'l']).parse::<u32>().ok())
                .filter(|n| Some(*n) > first);
            (first, second)
        }
    };
    Some((file.to_string(), line, end))
}

fn looks_like_line(s: &str) -> bool {
    let s = s.trim().trim_start_matches(['L', 'l']);
    !s.is_empty()
        && s.chars().next().is_some_and(|c| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_digit() || c == '-' || c == '–' || c == 'L' || c == 'l')
}

fn parse_finding_line(line: &str) -> Option<Finding> {
    let mut parts = line.splitn(3, '|');
    let loc = parts.next()?;
    let sev = parts.next()?;
    let text = parts.next()?.trim().trim_matches('|').trim();
    let severity = severity_of(sev)?;
    if text.is_empty() {
        return None;
    }
    let (file, line, end_line) = parse_location(loc)?;
    Some(Finding { id: String::new(), file, line, end_line, severity, text: cut(text, MAX_TEXT_CHARS) })
}

// ---------------------------------------------------------------- JSON fallback

fn json_candidates(answer: &str) -> Vec<&str> {
    let mut out = Vec::new();
    // Fenced blocks first.
    let mut rest = answer;
    while let Some(start) = rest.find("```") {
        let after = &rest[start + 3..];
        let Some(end) = after.find("```") else { break };
        let block = &after[..end];
        let block = block.strip_prefix("json").or_else(|| block.strip_prefix("JSON")).unwrap_or(block);
        out.push(block.trim());
        rest = &after[end + 3..];
    }
    // Then the widest brace / bracket span.
    for (open, close) in [('{', '}'), ('[', ']')] {
        if let (Some(a), Some(b)) = (answer.find(open), answer.rfind(close)) {
            if a < b {
                out.push(&answer[a..=b]);
            }
        }
    }
    out
}

fn parse_json(answer: &str) -> Option<(Vec<Finding>, Option<Verdict>)> {
    for candidate in json_candidates(answer) {
        let Ok(value) = serde_json::from_str::<Value>(candidate) else { continue };
        let (items, verdict_value) = match &value {
            Value::Array(items) => (items.clone(), None),
            Value::Object(map) => {
                let items = ["findings", "issues", "comments", "review"]
                    .iter()
                    .find_map(|k| map.get(*k).and_then(Value::as_array))
                    .cloned()
                    .unwrap_or_default();
                (items, map.get("verdict").cloned())
            }
            _ => continue,
        };
        let findings: Vec<Finding> = items.iter().filter_map(json_finding).take(MAX_FINDINGS).collect();
        let verdict = verdict_value.as_ref().and_then(json_verdict);
        if !findings.is_empty() || verdict.is_some() {
            return Some((findings, verdict));
        }
    }
    None
}

fn pick<'a>(item: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|k| item.get(*k)).filter(|v| !v.is_null())
}

fn as_line(v: &Value) -> Option<u32> {
    v.as_u64()
        .map(|n| n as u32)
        .or_else(|| v.as_str().and_then(|s| s.trim().trim_start_matches(['L', 'l']).parse().ok()))
        .filter(|n| *n > 0)
}

fn json_finding(item: &Value) -> Option<Finding> {
    let file = pick(item, &["file", "path", "filename"])?.as_str()?.trim().to_string();
    let text = pick(item, &["finding", "text", "message", "description", "comment", "issue"])?
        .as_str()?
        .trim();
    if file.is_empty() || text.is_empty() {
        return None;
    }
    let severity = pick(item, &["severity", "level", "priority"])
        .and_then(Value::as_str)
        .and_then(severity_of)
        .unwrap_or(Severity::Medium);
    let line = pick(item, &["line", "line_number", "start_line", "lineno"]).and_then(as_line);
    let end_line = pick(item, &["end_line", "endLine"]).and_then(as_line).filter(|e| Some(*e) > line);
    Some(Finding { id: String::new(), file, line, end_line, severity, text: cut(text, MAX_TEXT_CHARS) })
}

fn json_verdict(v: &Value) -> Option<Verdict> {
    match v {
        Value::String(s) => Some(
            parse_verdict(&format!("VERDICT: {s}"))
                .unwrap_or(Verdict { level: VerdictLevel::Unknown, text: cut(s, MAX_TEXT_CHARS) }),
        ),
        Value::Object(map) => {
            let level = map
                .get("level")
                .or_else(|| map.get("decision"))
                .and_then(Value::as_str)
                .map(level_of)
                .unwrap_or(VerdictLevel::Unknown);
            let text = ["text", "summary", "message"]
                .iter()
                .find_map(|k| map.get(*k).and_then(Value::as_str))
                .unwrap_or("");
            Some(Verdict { level, text: cut(text.trim(), MAX_TEXT_CHARS) })
        }
        _ => None,
    }
}

fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}
