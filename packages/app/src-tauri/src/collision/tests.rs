//! Guards of Collision Alert (SPEC-n33 sections 4 and 5, CA-15). Source scans, and one test with
//! text planted in a file name and a pane heading.
//!
//! - `no_write_path` (C7): nothing in the module can type into, press a key in, interrupt, stop or
//!   merge an agent.
//! - `not_reachable_from_agents` (C6): no file outside the desktop wiring names `collision::`, and
//!   the folders agents reach are among the files checked.
//! - `no_agent_names` (C12): the code that is not a test names no agent.
//! - `no_network_symbols`: the module names no http client and no socket.
//! - `git_only_through_harden_read` (C1, C2): git runs only through `worktrees::harden::read`.
//! - `strings_have_no_pause_words`: the card's words in `copy.rs` never say "pause", "stopped by"
//!   or "blocked". The footer sentence lives in `advice.rs` and `copy.rs` only calls it.
//! - `view_has_no_private_text` (C4, C11): text planted in a file name and a pane heading never
//!   reaches the serialised view.
//!
//! Like `scores/tests.rs`, the scans are plain text scans. Only test-only items (`#[cfg(test)]`) are
//! skipped, so a test can name what it bans. Comments and literals are blanked where only code
//! matters.

use super::model::{Alert, AlertId, FileOverlap, How, Level, PaneFact, PaneRun, Relation, SideFacts, Span};
use super::view::view;
use crate::redact;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

// ---- source scans (copied from scores/tests.rs) ----

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `.rs` file under `dir` as (path relative to `src/` with `/` separators, text), sorted.
fn sources(dir: &Path, out: &mut Vec<(String, String)>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir).expect("read dir").filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let rel = path.strip_prefix(src_dir()).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            out.push((rel, fs::read_to_string(&path).expect("read source")));
        }
    }
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `needle` occurs in `line` and does not start in the middle of a longer identifier.
fn occurs_at_word_start(line: &str, needle: &str) -> bool {
    line.match_indices(needle).any(|(at, _)| line[..at].chars().next_back().is_none_or(|c| !is_ident(c)))
}

/// `needle` occurs in `line` as a whole word (no identifier character on either side).
fn occurs_as_word(line: &str, needle: &str) -> bool {
    line.match_indices(needle).any(|(at, _)| {
        let before = line[..at].chars().next_back().is_none_or(|c| !is_ident(c));
        let after = line[at + needle.len()..].chars().next().is_none_or(|c| !is_ident(c));
        before && after
    })
}

/// `text` with `//` and `/* */` comments and the contents of string and char literals blanked to
/// spaces, so only code is left. Line breaks are kept, so line numbers hold. Simple on purpose:
/// no nested block comments, and an escaped quote in a char literal is not handled.
fn code_only(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let blank = |out: &mut String, ch: char| out.push(if ch == '\n' { '\n' } else { ' ' });
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch == '/' && c.get(i + 1) == Some(&'/') {
            while i < c.len() && c[i] != '\n' {
                blank(&mut out, c[i]);
                i += 1;
            }
        } else if ch == '/' && c.get(i + 1) == Some(&'*') {
            while i < c.len() && !(c[i] == '*' && c.get(i + 1) == Some(&'/')) {
                blank(&mut out, c[i]);
                i += 1;
            }
            for _ in 0..2 {
                if i < c.len() {
                    blank(&mut out, c[i]);
                    i += 1;
                }
            }
        } else if ch == 'r' && raw_hashes(&c, i).is_some() {
            let n = raw_hashes(&c, i).unwrap_or(0);
            let mut j = i + n + 2; // past `r`, the hashes and the opening quote
            while j < c.len() && !(c[j] == '"' && (1..=n).all(|k| c.get(j + k) == Some(&'#'))) {
                j += 1;
            }
            let end = (j + n + 1).min(c.len());
            for &x in &c[i..end] {
                blank(&mut out, x);
            }
            i = end;
        } else if ch == '"' {
            blank(&mut out, ch);
            i += 1;
            while i < c.len() && c[i] != '"' {
                let escaped = c[i] == '\\';
                blank(&mut out, c[i]);
                i += 1;
                if escaped && i < c.len() {
                    blank(&mut out, c[i]);
                    i += 1;
                }
            }
            if i < c.len() {
                blank(&mut out, c[i]);
                i += 1;
            }
        } else if ch == '\'' && c.get(i + 1) == Some(&'\\') {
            let mut j = i + 2;
            while j < c.len() && c[j] != '\'' {
                j += 1;
            }
            let end = (j + 1).min(c.len());
            for &x in &c[i..end] {
                blank(&mut out, x);
            }
            i = end;
        } else if ch == '\'' && c.get(i + 2) == Some(&'\'') {
            for &x in &c[i..i + 3] {
                blank(&mut out, x);
            }
            i += 3;
        } else {
            out.push(ch);
            i += 1;
        }
    }
    out
}

/// The number of `#` of a raw string opening (`r"` or `r#"`...) at `at`, when `r` starts a token.
fn raw_hashes(c: &[char], at: usize) -> Option<usize> {
    if at > 0 && is_ident(c[at - 1]) {
        return None;
    }
    let mut j = at + 1;
    while c.get(j) == Some(&'#') {
        j += 1;
    }
    (c.get(j) == Some(&'"')).then_some(j - at - 1)
}

// ---- the collision module's own scans ----

fn read_src(rel: &str) -> String {
    fs::read_to_string(src_dir().join(rel)).expect("read source")
}

/// One flag per line of `text`: true when the line belongs to a test-only item, that is a
/// `#[cfg(test)]` attribute and the item after it (up to its closing brace, or its `;`). Braces are
/// counted on `code_only`, so a brace in a string or a comment cannot end an item early.
fn test_only_lines(text: &str) -> Vec<bool> {
    let code: Vec<char> = code_only(text).chars().collect();
    let attr: Vec<char> = "#[cfg(test)]".chars().collect();
    let mut masked = vec![false; code.len()];
    let mut i = 0;
    while i < code.len() {
        if !code[i..].starts_with(&attr) {
            i += 1;
            continue;
        }
        let start = i;
        i += attr.len();
        let (mut depth, mut opened) = (0i32, false);
        while i < code.len() {
            let ch = code[i];
            i += 1;
            match ch {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => {
                    depth -= 1;
                    if opened && depth == 0 {
                        break;
                    }
                }
                ';' if depth == 0 && !opened => break,
                _ => {}
            }
        }
        masked[start..i].iter_mut().for_each(|m| *m = true);
    }
    let mut flags = Vec::new();
    let mut line = false;
    for (ch, m) in code.iter().zip(masked.iter()) {
        line |= *m;
        if *ch == '\n' {
            flags.push(line);
            line = false;
        }
    }
    flags.push(line);
    flags
}

/// `text` with its test-only lines emptied (line numbers hold). `code` = true also blanks comments
/// and the contents of literals, so only code is left.
fn production(text: &str, code: bool) -> String {
    let body = if code { code_only(text) } else { text.to_string() };
    body.split('\n')
        .zip(test_only_lines(text))
        .map(|(line, test)| if test { "" } else { line })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The shipped files of the collision module as (path relative to `src/`, production text).
/// `tests.rs` is test-only by its declaration in `mod.rs`, so it is left out.
fn collision_files(code: bool) -> Vec<(String, String)> {
    let mut files = Vec::new();
    sources(&src_dir().join("collision"), &mut files);
    assert!(files.len() > 10, "the collision scan found only {} files", files.len());
    files
        .into_iter()
        .filter(|(file, _)| file.as_str() != "collision/tests.rs")
        .map(|(file, text)| {
            let body = production(&text, code);
            (file, body)
        })
        .collect()
}

/// `file:line  needle` for every needle that occurs on a line of `files`. `whole_word` picks
/// `occurs_as_word`; otherwise the needle only has to start a word (`kill_` matches `kill_child`).
fn hits(files: &[(String, String)], needles: &[&str], whole_word: bool) -> Vec<String> {
    let mut found = Vec::new();
    for (file, text) in files {
        for (index, line) in text.lines().enumerate() {
            for needle in needles {
                let seen = if whole_word { occurs_as_word(line, needle) } else { occurs_at_word_start(line, needle) };
                if seen {
                    found.push(format!("{file}:{}  `{needle}`", index + 1));
                }
            }
        }
    }
    found
}

/// The scanner itself: a test-only item is skipped, a shipped line after it is not, and a
/// needle in a comment is not seen.
#[test]
fn the_scans_skip_only_test_items() {
    let fixture = r#"fn shipped() { interrupt(); }
#[cfg(test)]
mod tests {
    fn t() { let s = "}"; interrupt(); }
}
// discard in a comment
fn after() { discard(); }
"#;
    let code = production(fixture, true);
    assert!(!code.contains("fn t()"), "{code}");
    assert_eq!(code.lines().count(), fixture.lines().count(), "line numbers must hold");
    let found = hits(&[("x.rs".to_string(), code)], WRITE_NEEDLES, false);
    assert_eq!(found, vec!["x.rs:1  `interrupt`".to_string(), "x.rs:7  `discard`".to_string()]);
}

// ---- 1. no write path (C7) ----

/// Words that would mean PM types into, presses a key in, interrupts, stops or merges an agent.
const WRITE_NEEDLES: &[&str] = &[
    "write_input",
    "send_keys",
    "press_key",
    "TypingPermit",
    "GatePermit",
    "interrupt",
    "kill_",
    "agent_stop",
    "worktrees::merge",
    "discard",
];

#[test]
fn no_write_path() {
    let files = collision_files(true);
    let found = hits(&files, WRITE_NEEDLES, false);
    assert!(found.is_empty(), "the collision module has a write path to an agent (C7):\n  {}", found.join("\n  "));
}

// ---- 2. not reachable from agents (C6) ----

/// Folders and files an agent can reach: the MCP registry and shim, the bridge, the team, the live
/// room, the phone push, the channels, the companion, the context pack and the one brain. None of
/// them may name the module. Relative to `src/`.
const AGENT_REACHABLE: &[&str] = &[
    "bridge/",
    "tool_registry/",
    "mcp_shim/",
    "team/",
    "live_room/",
    "push/",
    "channels/",
    "companion/",
    "context_pack.rs",
    "one_brain/",
];

/// The files that may name `collision::`: the desktop wiring and its window commands only.
const MAY_NAME_COLLISION: &[&str] = &["lib.rs", "commands/", "pane_activity.rs", "collision/"];

/// Line numbers of the lines of `text` that name `collision::`, outside a comment line.
fn collision_refs(text: &str) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with("//") && occurs_at_word_start(line, "collision::"))
        .map(|(index, _)| index + 1)
        .collect()
}

#[test]
fn not_reachable_from_agents() {
    let mut files = Vec::new();
    sources(&src_dir(), &mut files);
    assert!(files.len() > 100, "the scan found only {} files; is the source tree layout different?", files.len());
    // The places an agent can reach must be among the files checked, or the guard proves nothing.
    for must in AGENT_REACHABLE {
        assert!(files.iter().any(|(file, _)| file.starts_with(*must)), "{must} is not scanned");
    }
    let mut found = Vec::new();
    for (file, text) in &files {
        let allowed = MAY_NAME_COLLISION.iter().any(|ok| file.as_str() == *ok || (ok.ends_with('/') && file.starts_with(*ok)));
        if allowed {
            continue;
        }
        for line in collision_refs(text) {
            found.push(format!("{file}:{line}"));
        }
    }
    assert!(
        found.is_empty(),
        "a file outside the desktop wiring names the collision module (agents must never see it):\n  {}\n\
         Only the files in MAY_NAME_COLLISION may name `collision::`.",
        found.join("\n  ")
    );
}

// ---- 3. no agent names (C12) ----

/// Agent words that generic code never names. Labels come from the pane facts and the descriptors.
const AGENT_WORDS: &[&str] = &["claude", "codex", "cursor", "opencode", "agy"];

#[test]
fn no_agent_names() {
    let mut found = Vec::new();
    for (file, text) in collision_files(false) {
        for (index, line) in text.lines().enumerate() {
            let lower = line.to_ascii_lowercase();
            for word in AGENT_WORDS {
                if occurs_as_word(&lower, word) {
                    found.push(format!("{file}:{}  `{word}`", index + 1));
                }
            }
        }
    }
    assert!(found.is_empty(), "generic code names an agent (C12):\n  {}", found.join("\n  "));
}

// ---- 4. no network symbols ----

const NETWORK_NEEDLES: &[&str] = &[
    "reqwest", "ureq", "hyper", "isahc", "attohttpc", "minreq", "surf", "curl", "http", "tungstenite", "TcpStream", "TcpListener",
    "UdpSocket", "std::net", "tokio::net",
];

#[test]
fn no_network_symbols() {
    let files = collision_files(true);
    let found = hits(&files, NETWORK_NEEDLES, true);
    assert!(found.is_empty(), "the collision module names a network symbol:\n  {}", found.join("\n  "));
}

// ---- 5. git only through the hardened read (C1, C2) ----

/// Ways to start a process. In this module only `worktrees::harden::read` may start git.
const SPAWN_NEEDLES: &[&str] = &["Command::new", "hidden_command", "hidden_tokio_command", "std::process", "creation_flags"];

#[test]
fn git_only_through_harden_read() {
    let files = collision_files(true);
    let found = hits(&files, SPAWN_NEEDLES, true);
    assert!(found.is_empty(), "the collision module starts a process itself (C1, C2):\n  {}", found.join("\n  "));
    let probe = files.iter().find(|(file, _)| file.as_str() == "collision/probe.rs").map(|(_, text)| text.as_str()).unwrap_or("");
    assert!(probe.contains("harden::read("), "the git reads in probe.rs do not go through harden::read");
}

// ---- 6. the card words never say pause, stop or block (SPEC 3.1) ----

const PAUSE_WORDS: &[&str] = &["pause", "stopped by", "blocked"];

#[test]
fn strings_have_no_pause_words() {
    let mut found = Vec::new();
    let text = production(&read_src("collision/copy.rs"), false);
    for (index, line) in text.lines().enumerate() {
        let lower = line.to_ascii_lowercase();
        for word in PAUSE_WORDS {
            if lower.contains(word) {
                found.push(format!("collision/copy.rs:{}  `{word}`", index + 1));
            }
        }
    }
    assert!(found.is_empty(), "a card names a pause, stop or block (nothing is ever paused):\n  {}", found.join("\n  "));
}

// ---- 7. no private text in the view (C4, C11) ----

const BIDI: char = '\u{202e}';
const TOKEN: &str = "sk-ant-api03-FAKE0000000000";
const EMAIL: &str = "you@example.com";
const HOME: &str = r"C:\Users\you\secret-proj";

fn pane(id: &str, label: &str, cwd: &str) -> PaneFact {
    PaneFact {
        pane_id: id.to_string(),
        agent_id: "fake-agent-kind".to_string(),
        label: label.to_string(),
        cwd: cwd.to_string(),
        run: PaneRun::Working,
        created_ms: 1,
    }
}

/// Every string in a JSON value, depth first.
fn strings_of(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|item| strings_of(item, out)),
        Value::Object(map) => map.values().for_each(|item| strings_of(item, out)),
        _ => {}
    }
}

#[test]
fn view_has_no_private_text() {
    let label = format!("fake_a {TOKEN} {BIDI}\nx");
    let alert = Alert {
        id: AlertId::of(&["pane-1", "pane-2"]),
        relation: Relation::Copies,
        level: Level::Clash,
        panes: [pane("pane-1", &label, &format!(r"{HOME}\a")), pane("pane-2", "fake_b", &format!(r"{HOME}\b"))],
        sides: [
            SideFacts { pane_id: "pane-1".to_string(), working: true, files: 1, started_ms: 2 },
            SideFacts { pane_id: "pane-2".to_string(), working: true, files: 1, started_ms: 3 },
        ],
        files: vec![
            FileOverlap { path: format!("src/evil{BIDI}txt\n{TOKEN}.ts"), how: How::Lines(Span { start: 50, end: 50 }) },
            FileOverlap { path: format!("notes/{EMAIL}.md"), how: How::BothNew },
        ],
        advice: None,
        first_ms: 5,
        last_ms: 9,
        misses: 0,
        hidden: false,
    };
    let json = serde_json::to_value(view(&alert, &|s: &str| redact::text(s).into_inner())).expect("the view serialises");
    let mut strings = Vec::new();
    strings_of(&json, &mut strings);
    for text in &strings {
        assert!(!text.contains(BIDI) && !text.contains('\n'), "a control or bidi character reached the view: {text:?}");
        for private in [TOKEN, "@example.com", "secret-proj", "C:\\Users"] {
            assert!(!text.contains(private), "{private} reached the view: {text:?}");
        }
    }
    assert!(strings.iter().any(|text| text.contains("<email>")), "an address should show as <email>: {strings:?}");
}
