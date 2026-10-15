//! Draft generator (REC-2, REC-4): turns recorded captures into a draft definition TOML.
//!
//! The draft is a starting point for a human, not a finished definition: every candidate carries a
//! `# from: <sample line>` comment naming the capture text that produced it, every regex is checked
//! against the captures it was derived from (busy must match working and not idle), and the file is
//! written to `<app data>/agents/drafts/`, a folder the loader never scans (REC-4).

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value as Json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::snapshots::{SnapKind, Session};

/// A headless probe: the argv that was run (without the task) and its raw stdout.
#[derive(Debug, Clone)]
pub struct Probe {
    pub argv: Vec<String>,
    pub output: String,
}

#[derive(Debug, Clone, Default)]
pub struct DraftInput {
    pub id: String,
    pub label: String,
    /// The command that was recorded (becomes `[binary].names`).
    pub command: String,
    pub idle: Vec<String>,
    pub working: Vec<String>,
    pub prompt: Vec<String>,
    pub reply: Vec<String>,
    pub probe: Option<Probe>,
}

impl DraftInput {
    pub fn from_session(session: &Session, id: &str, label: &str, command: &str, probe: Option<Probe>) -> Self {
        let pick = |k: SnapKind| session.of_kind(k).into_iter().map(str::to_string).collect::<Vec<_>>();
        DraftInput {
            id: id.to_string(),
            label: label.to_string(),
            command: command.to_string(),
            idle: pick(SnapKind::Idle),
            working: pick(SnapKind::Working),
            prompt: pick(SnapKind::Prompt),
            reply: pick(SnapKind::Reply),
            probe,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Draft {
    pub id: String,
    pub toml: String,
    /// What the generator could not derive, for the UI to show next to the path.
    pub notes: Vec<String>,
}

/// Quote `s` as a TOML basic string.
fn q(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

fn one_line(s: &str) -> String {
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let clipped: String = flat.chars().take(100).collect();
    // A newline inside a `#` comment would end the comment, so the sample is flattened above.
    clipped
}

/// Text segments of a capture: split at line breaks and at runs of two or more spaces (the gaps
/// between columns), trimmed.
fn segments(capture: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in capture.lines().filter(|l| !l.trim_start().starts_with("# capture:")) {
        let mut cur = String::new();
        let mut spaces = 0;
        for ch in line.chars() {
            if ch == ' ' || ch == '\t' {
                spaces += 1;
                continue;
            }
            if spaces >= 2 && !cur.trim().is_empty() {
                out.push(cur.trim().to_string());
                cur.clear();
            } else if spaces > 0 && !cur.is_empty() {
                cur.push(' ');
            }
            spaces = 0;
            cur.push(ch);
        }
        if !cur.trim().is_empty() {
            out.push(cur.trim().to_string());
        }
    }
    out
}

/// Words of a segment: ASCII-alphanumeric tokens (with `+`, `.`, `:`, `_`, `-` inside), glyph-only
/// tokens dropped unless `keep_symbols`.
fn words(segment: &str, keep_symbols: bool) -> Vec<String> {
    segment
        .split_whitespace()
        .filter(|w| keep_symbols || w.chars().any(|c| c.is_ascii_alphanumeric()))
        .map(|w| {
            if keep_symbols {
                w.to_string()
            } else {
                w.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '$' && c != '%').to_string()
            }
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// A case-insensitive regex for `segment`: words joined by `\s+`, digit runs generalised.
fn regex_for(segment: &str, keep_symbols: bool) -> Option<String> {
    let ws = words(segment, keep_symbols);
    if ws.is_empty() {
        return None;
    }
    let digits = Regex::new(r"[0-9][0-9.,]*").expect("digits");
    let parts: Vec<String> = ws
        .iter()
        .map(|w| {
            let mut out = String::new();
            let mut last = 0;
            for m in digits.find_iter(w) {
                out.push_str(&regex::escape(&w[last..m.start()]));
                out.push_str(r"[\d.,]+");
                last = m.end();
            }
            out.push_str(&regex::escape(&w[last..]));
            out
        })
        .collect();
    Some(parts.join(r"\s+"))
}

fn norm(segment: &str) -> String {
    static NUM: Lazy<Regex> = Lazy::new(|| Regex::new(r"[0-9][0-9.,]*").expect("num"));
    NUM.replace_all(&words(segment, false).join(" ").to_lowercase(), "#").into_owned()
}

fn matches_any(re: &str, texts: &[String]) -> bool {
    match Regex::new(&format!("(?is){re}")) {
        Ok(r) => texts.iter().any(|t| r.is_match(t)),
        Err(_) => false,
    }
}

fn matches_all(re: &str, texts: &[String]) -> bool {
    match Regex::new(&format!("(?is){re}")) {
        Ok(r) => !texts.is_empty() && texts.iter().all(|t| r.is_match(t)),
        Err(_) => false,
    }
}

fn norm_set(captures: &[String]) -> BTreeSet<String> {
    captures.iter().flat_map(|c| segments(c)).map(|s| norm(&s)).filter(|s| !s.is_empty()).collect()
}

const BUSY_HINTS: [&str; 8] = ["interrupt", "stop", "cancel", "thinking", "working", "generating", "processing", "running"];
const FOOTER_HINTS: [&str; 7] = ["ctrl", "tab", "enter", "commands", "help", "shift", "alt"];

struct Candidate {
    sample: String,
    regex: String,
}

fn busy_candidates(input: &DraftInput, notes: &mut Vec<String>) -> Vec<Candidate> {
    let quiet: Vec<String> = input.idle.iter().chain(input.reply.iter()).cloned().collect();
    let quiet_set = norm_set(&quiet);
    let mut found: Vec<(usize, Candidate)> = Vec::new();
    let mut seen = BTreeSet::new();
    for capture in &input.working {
        for seg in segments(capture) {
            let n = norm(&seg);
            if n.len() < 4 || n.len() > 40 || quiet_set.contains(&n) || !seen.insert(n.clone()) {
                continue;
            }
            let Some(hint_rank) = BUSY_HINTS.iter().position(|h| n.contains(h)) else { continue };
            let Some(re) = regex_for(&seg, false) else { continue };
            // The marker must show on the working screens and never on an idle or finished one.
            if !matches_any(&re, &input.working) || matches_any(&re, &quiet) {
                continue;
            }
            found.push((hint_rank, Candidate { sample: seg, regex: re }));
        }
    }
    found.sort_by_key(|(rank, c)| (*rank, c.regex.len()));
    if found.is_empty() {
        notes.push("no busy marker: no text on the working captures that is absent from the idle ones (record a longer turn)".into());
    }
    found.into_iter().map(|(_, c)| c).take(3).collect()
}

fn idle_candidates(input: &DraftInput) -> Vec<Candidate> {
    let quiet: Vec<String> = input.idle.iter().chain(input.reply.iter()).cloned().collect();
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for capture in quiet.iter() {
        for seg in segments(capture) {
            let n = norm(&seg);
            if n.len() < 6 || n.len() > 40 || n.chars().any(|c| c.is_ascii_digit()) || !seen.insert(n.clone()) {
                continue;
            }
            if !FOOTER_HINTS.iter().any(|h| n.contains(h)) {
                continue;
            }
            let Some(re) = regex_for(&seg, false) else { continue };
            // Present on every idle/finished capture: that is what makes it a ready signal.
            if matches_all(&re, &quiet) {
                out.push(Candidate { sample: seg, regex: re });
            }
        }
    }
    out.into_iter().take(2).collect()
}

fn chrome_candidates(input: &DraftInput, busy: &[Candidate], idle: &[Candidate]) -> Vec<Candidate> {
    let sets: Vec<BTreeSet<String>> = [&input.idle, &input.working, &input.prompt, &input.reply]
        .iter()
        .filter(|v| !v.is_empty())
        .map(|v| norm_set(v))
        .collect();
    let mut counts: Vec<(usize, Candidate)> = Vec::new();
    let mut seen = BTreeSet::new();
    for capture in input.idle.iter().chain(&input.working).chain(&input.prompt).chain(&input.reply) {
        for seg in segments(capture) {
            let n = norm(&seg);
            if n.len() < 6 || n.len() > 50 || !seen.insert(n.clone()) {
                continue;
            }
            // UI chrome repeats from one screen kind to the next; a reply or a task does not.
            let hits = sets.iter().filter(|s| s.contains(&n)).count();
            if hits < 2 {
                continue;
            }
            let Some(re) = regex_for(&seg, false) else { continue };
            let anchored = format!(r"^\s*{re}");
            if busy.iter().chain(idle).any(|c| c.regex == re) {
                continue;
            }
            counts.push((hits, Candidate { sample: seg, regex: anchored }));
        }
    }
    counts.sort_by_key(|(h, c)| (std::cmp::Reverse(*h), c.regex.len()));
    counts.into_iter().map(|(_, c)| c).take(24).collect()
}

struct PromptCandidate {
    sample: String,
    detect: String,
    menu: Option<Candidate>,
}

fn prompt_candidate(input: &DraftInput, notes: &mut Vec<String>) -> Option<PromptCandidate> {
    let others: Vec<String> = input.idle.iter().chain(&input.working).chain(&input.reply).cloned().collect();
    let other_set = norm_set(&others);
    let ask = Regex::new(r"(?i)permission|allow|approve|confirm|proceed|y/n|do you want|are you sure|continue\?").expect("ask");
    let mut detect = None;
    let mut menu = None;
    for capture in &input.prompt {
        for seg in segments(capture) {
            let n = norm(&seg);
            if n.is_empty() || other_set.contains(&n) {
                continue;
            }
            if detect.is_none() && ask.is_match(&seg) {
                // The first few words are enough and survive small wording changes.
                let head: Vec<String> = words(&seg, false).into_iter().take(4).collect();
                if let Some(re) = regex_for(&head.join(" "), false) {
                    if matches_any(&re, &input.prompt) && !matches_any(&re, &others) {
                        detect = Some((seg.clone(), re));
                    }
                }
            }
            if menu.is_none() && n.contains("select") {
                if let Some(re) = regex_for(&seg, true) {
                    if matches_any(&re, &input.prompt) && !matches_any(&re, &others) {
                        menu = Some(Candidate { sample: seg.clone(), regex: re });
                    }
                }
            }
        }
    }
    if detect.is_none() {
        notes.push("no prompt: nothing on the prompt captures that looks like a permission or question (record a prompt)".into());
    }
    detect.map(|(sample, detect)| PromptCandidate { sample, detect, menu })
}

fn flatten(prefix: &str, v: &Json, out: &mut Vec<(String, String)>) {
    match v {
        Json::Object(m) => {
            for (k, v) in m {
                let p = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                flatten(&p, v, out);
            }
        }
        Json::String(s) => out.push((prefix.to_string(), s.clone())),
        _ => {}
    }
}

struct Event {
    line: String,
    kind: String,
    leaves: Vec<(String, String)>,
}

fn parse_events(output: &str) -> Vec<Event> {
    output
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            if !l.starts_with('{') {
                return None;
            }
            let v: Json = serde_json::from_str(l).ok()?;
            let kind = v.get("type")?.as_str()?.to_string();
            let mut leaves = Vec::new();
            flatten("", &v, &mut leaves);
            Some(Event { line: l.to_string(), kind, leaves })
        })
        .collect()
}

fn last_key(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or(path)
}

/// `[headless]` skeleton from a probe that printed JSON lines; None when the output is not JSON lines.
fn headless_section(probe: &Probe, notes: &mut Vec<String>) -> Option<String> {
    let events = parse_events(&probe.output);
    if events.len() < 2 {
        notes.push("no [headless] skeleton: the probe output has fewer than two JSON lines".into());
        return None;
    }
    let mut s = String::new();
    s.push_str("# Headless skeleton from the probe. TODO: check the argv and where the task goes.\n");
    s.push_str("[headless]\n");
    s.push_str(&format!("argv = [{}]\n", probe.argv.iter().map(|a| q(a)).collect::<Vec<_>>().join(", ")));
    s.push_str("task_separator = \"--\"\n\n[headless.output]\nformat = \"json_lines\"\n\n");

    let is_error = |k: &str| k.to_lowercase().contains("error");
    let answer_keys = ["text", "content", "result", "response", "answer"];

    // result: the last non-blank answer-looking leaf, by event type.
    let mut result: Option<(&Event, String, bool)> = None;
    for e in events.iter().filter(|e| !is_error(&e.kind)) {
        for (path, value) in &e.leaves {
            if answer_keys.contains(&last_key(path)) {
                let blank_seen = events.iter().any(|o| {
                    o.kind == e.kind && o.leaves.iter().any(|(p, v)| p == path && v.trim().is_empty())
                });
                if !value.trim().is_empty() {
                    result = Some((e, path.clone(), blank_seen));
                }
            }
        }
    }
    if let Some((e, path, blank)) = &result {
        s.push_str(&format!("# from: {}\n[headless.output.result]\n", one_line(&e.line)));
        s.push_str(&format!("where = {{ type = {} }}\n", q(&e.kind)));
        if *blank {
            s.push_str(&format!("where_regex = {{ {} = {} }}\n", q(path), q(r"\S")));
        }
        s.push_str(&format!("path = {}\npick = \"last\"\n\n", q(path)));
    } else {
        notes.push("no result selector: no answer-like text field in the probe output".into());
    }

    // session: a leaf named like a session id, preferring a top-level key.
    let session = events.iter().find_map(|e| {
        e.leaves
            .iter()
            .filter(|(p, v)| last_key(p).to_lowercase().contains("session") && !v.is_empty())
            .min_by_key(|(p, _)| p.matches('.').count())
            .map(|(p, _)| (e, p.clone()))
    });
    if let Some((e, path)) = session {
        s.push_str(&format!("# from: {}\n[headless.output.session]\n", one_line(&e.line)));
        if !path.contains('.') {
            s.push_str(&format!("where = {{ {} = true }}\n", q(&format!("~{path}"))));
        } else {
            s.push_str(&format!("where = {{ type = {} }}\n", q(&e.kind)));
        }
        s.push_str(&format!("path = {}\npick = \"first\"\n\n", q(&path)));
    } else {
        notes.push("no session selector: no session-id field in the probe output".into());
    }

    // done: a final event type, last in the stream by name.
    let done_words = ["finish", "done", "complete", "end", "stop"];
    let done = events.iter().rev().find(|e| {
        let k = e.kind.to_lowercase();
        done_words.iter().any(|w| k.contains(w)) && !k.contains("start") && !is_error(&k)
    });
    if let Some(e) = done {
        s.push_str(&format!("# from: {}\n[headless.output.done]\nwhere = {{ type = {} }}\n\n", one_line(&e.line), q(&e.kind)));
        // model: a model-looking leaf on that event type.
        if let Some(path) = e.leaves.iter().map(|(p, _)| p).find(|p| last_key(p).to_lowercase().contains("model")) {
            s.push_str(&format!("# from: {}\n[headless.output.model]\nwhere = {{ type = {} }}\n", one_line(&e.line), q(&e.kind)));
            s.push_str(&format!("path = {}\npick = \"last\"\n\n", q(path)));
        }
    } else {
        notes.push("no done selector: no finish-like event in the probe output".into());
    }

    if let Some(e) = events.iter().rev().find(|e| is_error(&e.kind)) {
        if let Some((path, _)) = e.leaves.iter().find(|(p, _)| last_key(p) == "message") {
            s.push_str(&format!("# from: {}\n[headless.output.error]\nwhere = {{ type = {} }}\npath = {}\npick = \"last\"\n\n", one_line(&e.line), q(&e.kind), q(path)));
        }
    }
    Some(s)
}

fn sanitize_id(id: &str) -> String {
    let mut out: String = id
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    if !out.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
        out.insert(0, 'a');
    }
    while out.len() < 2 {
        out.push('x');
    }
    out.truncate(32);
    out
}

/// Build the draft text. Never fails: whatever could not be derived is listed in `notes`.
pub fn generate(input: &DraftInput) -> Draft {
    generate_with(input, super::snapshots::process_ctx())
}

/// `generate` with an explicit redaction context (sample comments are redacted again, REC-3).
pub fn generate_with(input: &DraftInput, ctx: &super::redact::RedactContext) -> Draft {
    let id = sanitize_id(&input.id);
    let label = if input.label.trim().is_empty() { id.clone() } else { input.label.trim().to_string() };
    let command = input.command.trim();
    let mut notes = Vec::new();
    let busy = busy_candidates(input, &mut notes);
    let idle = idle_candidates(input);
    let chrome = chrome_candidates(input, &busy, &idle);
    let prompt = prompt_candidate(input, &mut notes);

    let mut s = String::new();
    s.push_str("# DRAFT generated by the Puppet Master session recorder. It is NOT approved and is never loaded from\n");
    s.push_str("# the drafts folder: review every line, copy it to the agents folder, validate, then approve it.\n");
    s.push_str("# Each candidate has a `# from:` comment with the captured text it was derived from.\n");
    s.push_str("schema_version = 1\n\n[agent]\n");
    s.push_str(&format!("id = {}\nlabel = {}\n\n", q(&id), q(&label)));
    s.push_str(&format!("[binary]\nnames = [{}]\n", q(command)));
    s.push_str(&format!("install_hint = {}\n\n", q(&format!("install {command} and make sure it is on PATH"))));

    let mut headless = None;
    if let Some(probe) = &input.probe {
        headless = headless_section(probe, &mut notes);
    }
    if headless.is_some() {
        s.push_str("[features]\ndispatchable = true\n\n");
    }
    if let Some(h) = &headless {
        s.push_str(h);
    }

    s.push_str("[tui]\nargv = []\n\n");
    if !busy.is_empty() || !idle.is_empty() {
        s.push_str("[tui.busy]\n");
        if !busy.is_empty() {
            for c in &busy {
                s.push_str(&format!("# from (working): {}\n", one_line(&c.sample)));
            }
            s.push_str(&format!("working_regex = [{}]\n", busy.iter().map(|c| q(&c.regex)).collect::<Vec<_>>().join(", ")));
        }
        if !idle.is_empty() {
            for c in &idle {
                s.push_str(&format!("# from (idle): {}\n", one_line(&c.sample)));
            }
            s.push_str(&format!("idle_regex = [{}]\n", idle.iter().map(|c| q(&c.regex)).collect::<Vec<_>>().join(", ")));
        }
        s.push('\n');
    }
    if !chrome.is_empty() {
        s.push_str("[tui.chrome]\nline_regex = [\n");
        for c in &chrome {
            s.push_str(&format!("  # from: {}\n  {},\n", one_line(&c.sample), q(&c.regex)));
        }
        s.push_str("]\n\n");
    } else {
        notes.push("no chrome lines: no text repeated across screen kinds".into());
    }
    if let Some(p) = &prompt {
        if let Some(m) = &p.menu {
            s.push_str(&format!("# from: {}\n[tui.menu]\nregex = [{}]\n\n", one_line(&m.sample), q(&m.regex)));
        }
        s.push_str(&format!("# from: {}\n", one_line(&p.sample)));
        s.push_str("# TODO: the keys below are a guess (Enter allows, Escape denies); check them against the real dialog.\n");
        s.push_str("[[tui.prompt]]\nkind = \"command_approval\"\n");
        s.push_str(&format!("detect_regex = {}\n", q(&p.detect)));
        s.push_str("choices = [\n  { id = \"allow_once\", key = \"enter\" },\n  { id = \"deny\", key = \"escape\" },\n]\n");
    }
    let toml = s
        .lines()
        .map(|l| if l.trim_start().starts_with('#') { super::redact::redact(l, ctx) } else { l.to_string() })
        .collect::<Vec<_>>()
        .join("
")
        + "
";
    Draft { id, toml, notes }
}

/// `<dir>/<id>.toml`, created new (an existing draft is never overwritten: a numeric suffix is added).
pub fn write_draft(dir: &Path, draft: &Draft) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create the drafts folder: {e}"))?;
    for n in 0..100u32 {
        let name = if n == 0 { format!("{}.toml", draft.id) } else { format!("{}-{n}.toml", draft.id) };
        let path = dir.join(name);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                std::io::Write::write_all(&mut f, draft.toml.as_bytes()).map_err(|e| e.to_string())?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("cannot write {}: {e}", path.display())),
        }
    }
    Err("too many drafts with this id".into())
}

/// `<app data>/agents/drafts`.
pub fn drafts_dir() -> PathBuf {
    crate::app_paths::agents_dir().join("drafts")
}
