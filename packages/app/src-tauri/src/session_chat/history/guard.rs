//! Guard tests of Bring All My Chats (spec section 4, rows 1 and 3). They read the source files, the same way
//! `proc_util::guard` does, and fail when old-chat code could reach the person's real agent folders or the process
//! environment, or when old chat text could reach a surface that is not the desktop window.
//!
//! The rules (see [`RULES`]): `from_process` (only the importer's commands build the process environment),
//! `home_or_env` (stores never call `home_dir(` or `env::var` themselves), `user_folder` (no user folder in
//! fixtures, only the `you` placeholder), `test_join` (a test never joins `.claude` / `.codex` to a real
//! `home_dir(`) and `not_on_pc_surfaces` (no `chat_import` under bridge, tool registry, MCP shim, companion,
//! team or mobile pairing code) and `all_chats_search` (no every-row search call, `.search(`, `.search_filtered(`,
//! `.facets(`, `chat_search(`, under those surfaces: they use `search_live` or `search_in`, which never return old chats).
//!
//! An exception goes in [`ALLOWED`] with its reason. The forbidden snippets in the tests below are data, so this
//! file is exempt from every rule.

use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

/// A place allowed to break one rule (`"*"` = every rule), and why. Paths are relative to `src/`, `/` separators.
struct Allowed {
    file: &'static str,
    rule: &'static str,
    reason: &'static str,
}

const ALLOWED: &[Allowed] = &[
    Allowed {
        file: "commands/chat_import.rs",
        rule: "from_process",
        reason: "the Tauri commands are the one caller that builds the real environment, once per command (BC-19)",
    },
    Allowed {
        file: "session_chat/history/env.rs",
        rule: "from_process",
        reason: "defines `HistoryEnv::from_process`; it is the only place that builds the process environment",
    },
    Allowed {
        file: "session_chat/history/env.rs",
        rule: "home_or_env",
        reason: "`from_process` reads `std::env::var` and the OS home folder; no store reads them directly",
    },
    Allowed {
        file: "session_chat/history/guard.rs",
        rule: "*",
        reason: "this guard: its tests hold the forbidden snippets as data, so a scan of this file would flag them",
    },
];

/// Every rule id a `Hit` can carry.
const RULE_IDS: &[&str] = &["from_process", "home_or_env", "user_folder", "test_join", "not_on_pc_surfaces", "all_chats_search"];

/// Which text a rule reads. `Code` has comments and string contents blanked; `Raw` is the file as written.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Text {
    Code,
    Raw,
}

/// One forbidden needle set, for the files in `scope`.
struct Rule {
    id: &'static str,
    text: Text,
    scope: fn(&str) -> bool,
    needles: &'static [&'static str],
    advice: &'static str,
}

const RULES: &[Rule] = &[
    Rule {
        id: "from_process",
        text: Text::Code,
        scope: everywhere,
        needles: &["HistoryEnv::from_process"],
        advice: "only `commands/chat_import.rs` builds the process environment; take a `HistoryEnv` argument instead",
    },
    Rule {
        id: "home_or_env",
        text: Text::Code,
        scope: old_chat_code,
        needles: &["home_dir(", "env::var"],
        advice: "ask the `HistoryEnv` argument (`home()`, `var()`); an old-chat store never reads the real home itself",
    },
    Rule {
        id: "not_on_pc_surfaces",
        text: Text::Raw,
        scope: pc_surfaces,
        needles: &["chat_import", "chat_index::import"],
        advice: "old chats stay in the desktop window's own commands; bridge, tools, companion, team and phone code never reach them",
    },
    Rule {
        id: "all_chats_search",
        text: Text::Code,
        scope: pc_surfaces,
        needles: &[".search(", ".search_filtered(", ".facets(", "chat_search("],
        advice: "the every-row search (live and old chats) is for the desktop window only; a phone, tool, companion or team surface calls `search_live` (or `search_in` with its own pane ids)",
    },
];

fn everywhere(_file: &str) -> bool {
    true
}

/// Old-chat code: the history core, the importer, and each agent's history reader.
fn old_chat_code(file: &str) -> bool {
    file.starts_with("session_chat/history/") || file.starts_with("chat_index/import/") || (file.starts_with("tui/") && file.ends_with("/history.rs"))
}

/// Surfaces that must never see old chat text.
fn pc_surfaces(file: &str) -> bool {
    ["bridge", "tool_registry", "mcp_shim", "companion", "team", "mobile_pairing"].iter().any(|dir| file.starts_with(&format!("{dir}/")))
}

/// One forbidden thing found: where, which rule, and what to do instead.
struct Hit {
    file: String,
    line: usize,
    rule: &'static str,
    advice: &'static str,
}

/// One source file: as written, and with comments and string contents blanked (same length, same lines).
struct Source {
    file: String,
    raw: String,
    code: String,
}

impl Source {
    fn new(file: &str, raw: &str) -> Self {
        Self { file: file.to_string(), raw: raw.to_string(), code: blank_literals(raw) }
    }
}

fn allowed(file: &str, rule: &str) -> bool {
    ALLOWED.iter().any(|entry| entry.file == file && (entry.rule == "*" || entry.rule == rule))
}

/// Every hit in `sources`, before the allow-list. The one scan that the tree test and the synthetic test share.
fn scan(sources: &[Source]) -> Vec<Hit> {
    let mut hits = Vec::new();
    for source in sources {
        for rule in RULES.iter().filter(|rule| (rule.scope)(source.file.as_str())) {
            let text = if rule.text == Text::Code { &source.code } else { &source.raw };
            for (index, line) in text.lines().enumerate() {
                if rule.needles.iter().any(|needle| line.contains(needle)) {
                    hits.push(Hit { file: source.file.clone(), line: index + 1, rule: rule.id, advice: rule.advice });
                }
            }
        }
        if !old_chat_code(&source.file) {
            continue;
        }
        // A user folder is reported by line only: the name itself must not reach a CI log.
        for (index, line) in source.raw.lines().enumerate() {
            if !user_folders(line).is_empty() {
                hits.push(Hit {
                    file: source.file.clone(),
                    line: index + 1,
                    rule: "user_folder",
                    advice: "fixtures use the placeholder folder `you`, for example `C:\\Users\\you\\.claude`",
                });
            }
        }
        for item in test_items(&source.code) {
            let joined = source.code[item.clone()].contains("home_dir(");
            let raw = &source.raw[item.clone()];
            if joined && (raw.contains("\".claude\"") || raw.contains("\".codex\"")) {
                hits.push(Hit {
                    file: source.file.clone(),
                    line: source.raw[..item.start].matches('\n').count() + 1,
                    rule: "test_join",
                    advice: "a test joins `.claude` / `.codex` to a fixture folder, never to the real `home_dir()`",
                });
            }
        }
    }
    hits
}

fn is_sep(c: char) -> bool {
    c == '\\' || c == '/'
}

/// User-folder names in `line` written as `Users\name`, `Users/name` or `/home/name`, except placeholders.
fn user_folders(line: &str) -> Vec<String> {
    let mut names = Vec::new();
    for (at, _) in line.match_indices("Users") {
        let glued = line[..at].chars().next_back().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        let after = &line[at + "Users".len()..];
        let name_part = after.trim_start_matches(is_sep);
        if !glued && name_part.len() < after.len() {
            names.extend(segment(name_part));
        }
    }
    for (at, _) in line.match_indices("/home/") {
        names.extend(segment(&line[at + "/home/".len()..]));
    }
    names.into_iter().filter(|name| !is_placeholder(name)).collect()
}

/// The folder name at the start of `text`: up to a separator, a space, a quote or punctuation.
fn segment(text: &str) -> Option<String> {
    let name: String = text.chars().take_while(|&c| !is_sep(c) && !c.is_whitespace() && !"'\"`<>|*?:;,)]}".contains(c)).collect();
    (!name.is_empty()).then_some(name)
}

/// `you` is the placeholder; `%USERNAME%`, `$env:USERNAME`, `<name>`, `~` and wildcards are shapes, not people.
fn is_placeholder(name: &str) -> bool {
    name.eq_ignore_ascii_case("you") || name.contains(|c: char| "%$<>{}*[~".contains(c))
}

/// Byte ranges of the items behind a test-only `#[cfg(...)]` (`test` or `all(test, ...)`), attributes included.
fn test_items(code: &str) -> Vec<Range<usize>> {
    let b = code.as_bytes();
    let mut items = Vec::new();
    let mut i = 0;
    while let Some(found) = code[i..].find("#[") {
        let start = i + found;
        let Some(close) = matching(b, start + 1, b'[', b']') else { break };
        let attr: String = code[start..=close].chars().filter(|c| !c.is_whitespace()).collect();
        i = close + 1;
        if attr.starts_with("#[cfg(test)]") || attr.starts_with("#[cfg(all(test,") {
            let end = item_end(b, i);
            items.push(start..end);
            i = end;
        }
    }
    items
}

/// Where the item starting at `from` ends: after more attributes, through `;` or through its `{ ... }` body.
fn item_end(b: &[u8], from: usize) -> usize {
    let mut j = from;
    loop {
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        if b[j..].starts_with(b"#[") {
            j = matching(b, j + 1, b'[', b']').map_or(b.len(), |close| close + 1);
        } else {
            break;
        }
    }
    while j < b.len() {
        match b[j] {
            b';' => return j + 1,
            b'{' => return matching(b, j, b'{', b'}').map_or(b.len(), |close| close + 1),
            _ => j += 1,
        }
    }
    b.len()
}

/// Index of the delimiter closing the one at `open` (`b[open] == open_ch`).
fn matching(b: &[u8], open: usize, open_ch: u8, close_ch: u8) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, &byte) in b[open..].iter().enumerate() {
        if byte == open_ch {
            depth += 1;
        } else if byte == close_ch {
            depth -= 1;
            if depth == 0 {
                return Some(open + offset);
            }
        }
    }
    None
}

/// `source` with comments and the inside of string/char literals blanked to spaces (newlines kept, so line
/// numbers and byte offsets hold).
fn blank_literals(source: &str) -> String {
    let b = source.as_bytes();
    let mut out = b.to_vec();
    let blank = |out: &mut Vec<u8>, from: usize, to: usize| {
        for byte in &mut out[from..to] {
            if *byte != b'\n' && *byte != b'\r' {
                *byte = b' ';
            }
        }
    };
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                let end = source[i..].find('\n').map_or(b.len(), |n| i + n);
                blank(&mut out, i, end);
                i = end;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let (mut depth, mut j) = (1, i + 2);
                while j < b.len() && depth > 0 {
                    if b[j] == b'/' && b.get(j + 1) == Some(&b'*') {
                        depth += 1;
                        j += 2;
                    } else if b[j] == b'*' && b.get(j + 1) == Some(&b'/') {
                        depth -= 1;
                        j += 2;
                    } else {
                        j += 1;
                    }
                }
                blank(&mut out, i, j);
                i = j;
            }
            b'r' | b'b' if is_raw_string_start(b, i) => {
                let mut j = i + 1;
                if b[i] == b'b' {
                    j += 1;
                }
                let hashes = b[j..].iter().take_while(|&&c| c == b'#').count();
                j += hashes;
                // b[j] is the opening quote
                let close: Vec<u8> = std::iter::once(b'"').chain(std::iter::repeat(b'#').take(hashes)).collect();
                let end = find_bytes(&b[j + 1..], &close).map_or(b.len(), |n| j + 1 + n + close.len());
                blank(&mut out, j + 1, end.saturating_sub(close.len()).max(j + 1));
                i = end;
            }
            b'"' => {
                let mut j = i + 1;
                while j < b.len() && b[j] != b'"' {
                    j += if b[j] == b'\\' { 2 } else { 1 };
                }
                blank(&mut out, i + 1, j.min(b.len()));
                i = j + 1;
            }
            b'\'' => {
                // A char literal ('x', '\n', '\u{..}') or a lifetime ('a).
                if b.get(i + 1) == Some(&b'\\') {
                    let end = source[i + 2..].find('\'').map_or(b.len(), |n| i + 2 + n);
                    blank(&mut out, i + 1, end);
                    i = end + 1;
                } else if source[i + 1..].chars().nth(1) == Some('\'') {
                    let width = source[i + 1..].chars().next().map_or(1, char::len_utf8);
                    blank(&mut out, i + 1, i + 1 + width);
                    i += width + 2;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    String::from_utf8(out).expect("blanking keeps utf-8")
}

fn is_raw_string_start(b: &[u8], i: usize) -> bool {
    if i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_') {
        return false;
    }
    let mut j = i;
    if b[j] == b'b' {
        j += 1;
    }
    if b.get(j) != Some(&b'r') {
        return false;
    }
    j += 1;
    while b.get(j) == Some(&b'#') {
        j += 1;
    }
    b.get(j) == Some(&b'"')
}

fn find_bytes(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rel(path: &Path) -> String {
    path.strip_prefix(src_dir()).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// Every `.rs` file under `dir`, sorted.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut paths: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// The whole tree under `src/`, as the scan sees it.
fn tree() -> Vec<Source> {
    let mut paths = Vec::new();
    rust_files(&src_dir(), &mut paths);
    paths.iter().map(|path| Source::new(&rel(path), &fs::read_to_string(path).expect("read source"))).collect()
}

fn describe(hits: &[Hit]) -> String {
    hits.iter().map(|h| format!("  {}:{}  [{}] {}", h.file, h.line, h.rule, h.advice)).collect::<Vec<_>>().join("\n")
}

#[test]
fn the_tree_passes_every_rule() {
    let sources = tree();
    assert!(sources.len() > 100, "the scan found only {} files; is the source tree layout different?", sources.len());
    let hits: Vec<Hit> = scan(&sources).into_iter().filter(|hit| !allowed(&hit.file, hit.rule)).collect();
    assert!(
        hits.is_empty(),
        "old-chat code breaks a history guard:\n{}\nFix the code, or add the file to ALLOWED in session_chat/history/guard.rs with the reason.",
        describe(&hits)
    );
}

#[test]
fn the_scan_covers_the_files_that_matter() {
    let sources = tree();
    let files: Vec<&str> = sources.iter().map(|s| s.file.as_str()).collect();
    for must in [
        "session_chat/history/env.rs",
        "session_chat/history/file.rs",
        "chat_index/import/fixture_tests.rs",
        "tui/claude/history.rs",
        "tui/codex/history.rs",
        "commands/chat_import.rs",
        "bridge/mod.rs",
    ] {
        assert!(files.contains(&must), "{must} is not scanned");
    }
}

#[test]
fn every_allow_list_entry_names_a_real_file_a_known_rule_and_a_reason() {
    for entry in ALLOWED {
        assert!(!entry.reason.trim().is_empty(), "{}: an exception needs a reason", entry.file);
        assert!(entry.rule == "*" || RULE_IDS.contains(&entry.rule), "{}: unknown rule `{}`", entry.file, entry.rule);
        assert!(src_dir().join(entry.file).is_file(), "{} is listed in ALLOWED but does not exist; remove it", entry.file);
    }
}

#[test]
fn a_bad_snippet_is_flagged_by_the_same_scan() {
    // Every rule, in one fake file under the history core. The snippet is data: the guard never reads it as code.
    let bad = r#"
        use crate::session_chat::history::HistoryEnv;
        pub fn open() -> HistoryEnv { HistoryEnv::from_process() }
        fn home() -> Option<std::path::PathBuf> { crate::project_path::home_dir() }
        fn lookup() -> Option<String> { std::env::var("CLAUDE_CONFIG_DIR").ok() }
        #[cfg(test)]
        mod tests {
            fn reads_the_real_store() {
                let root = crate::project_path::home_dir().unwrap().join(".claude").join("projects");
            }
        }
    "#;
    let hits = scan(&[Source::new("session_chat/history/fake_reader.rs", bad)]);
    let rules: Vec<&str> = hits.iter().map(|h| h.rule).collect();
    assert_eq!(rules.iter().filter(|r| **r == "from_process").count(), 1, "{}", describe(&hits));
    assert_eq!(rules.iter().filter(|r| **r == "home_or_env").count(), 3, "home_dir( and env::var, also inside the test: {}", describe(&hits));
    assert_eq!(rules.iter().filter(|r| **r == "test_join").count(), 1, "{}", describe(&hits));

    // The same snippet in a file the allow-list names is still found by the scan; only the caller drops it.
    assert!(allowed("session_chat/history/env.rs", "from_process"));
    assert!(!allowed("session_chat/history/open.rs", "from_process"));
    assert!(!allowed("session_chat/history/env.rs", "test_join"));
}

#[test]
fn a_user_folder_in_a_fixture_is_flagged_and_the_placeholder_is_not() {
    // Built from pieces so that this file names no real-looking user folder.
    let named = format!("let root = \"{}\";", ["C:", "Users", "zed", "proj"].join("\\"));
    let placeholder = format!("let root = \"{}\";", ["C:", "Users", "you", ".claude"].join("\\"));
    let hits = scan(&[Source::new("session_chat/history/fixture.rs", &named), Source::new("session_chat/history/fixture2.rs", &placeholder)]);
    let found: Vec<&str> = hits.iter().filter(|h| h.rule == "user_folder").map(|h| h.file.as_str()).collect();
    assert_eq!(found, ["session_chat/history/fixture.rs"], "{}", describe(&hits));
}

#[test]
fn a_chat_import_under_a_pc_surface_is_flagged_by_the_same_scan() {
    for file in ["bridge/routes.rs", "tool_registry/mod.rs", "mcp_shim/tools.rs", "companion/today.rs", "team/card.rs", "mobile_pairing/api.rs"] {
        let hits = scan(&[Source::new(file, "use crate::commands::chat_import::chat_import_search;")]);
        assert!(hits.iter().any(|h| h.rule == "not_on_pc_surfaces"), "{file} was not flagged");
    }
    let outside = scan(&[Source::new("commands/chat_search.rs", "use crate::commands::chat_import::chat_import_search;")]);
    assert!(!outside.iter().any(|h| h.rule == "not_on_pc_surfaces"), "the desktop commands may name the importer");
}

#[test]
fn an_every_row_search_under_a_pc_surface_is_flagged_by_the_same_scan() {
    let every_row = "fn phone(index: &ChatIndex) { let _ = index.search(\"license\", 5); }";
    let filtered = "fn phone(index: &ChatIndex) { let _ = index.search_filtered(\"license\", 5, &f); }";
    let facets = "fn phone(index: &ChatIndex) { let _ = index.facets(); }";
    let live = "fn phone(index: &ChatIndex) { let _ = index.search_live(\"license\", 5); let _ = index.search_in(\"license\", 5, &p); }";
    for file in ["bridge/routes/chats.rs", "companion/live.rs", "tool_registry/catalog/chat.rs", "team/card.rs"] {
        let hits = scan(&[Source::new(file, every_row), Source::new(file, filtered), Source::new(file, facets)]);
        assert_eq!(hits.iter().filter(|h| h.rule == "all_chats_search").count(), 3, "{file}: {}", describe(&hits));
        let ok = scan(&[Source::new(file, live)]);
        assert!(ok.iter().all(|h| h.rule != "all_chats_search"), "{file}: {}", describe(&ok));
    }
    // The desktop window's commands may search every row.
    let desktop = scan(&[Source::new("commands/session_chat.rs", every_row)]);
    assert!(desktop.iter().all(|h| h.rule != "all_chats_search"), "{}", describe(&desktop));
}

#[test]
fn comments_and_strings_do_not_trip_the_code_rules() {
    let ok = r#"
        // HistoryEnv::from_process() and home_dir( in a comment
        /* std::env::var("X") in a block comment */
        fn f() { let s = "HistoryEnv::from_process and home_dir( and std::env::var"; let c = 'h'; }
    "#;
    let hits = scan(&[Source::new("session_chat/history/ok.rs", ok)]);
    assert!(hits.iter().all(|h| h.rule != "from_process" && h.rule != "home_or_env"), "{}", describe(&hits));
}

#[test]
fn a_test_that_joins_a_fixture_folder_is_not_a_test_join() {
    let fixture = r#"
        #[cfg(test)]
        mod tests {
            fn fixture(home: &std::path::Path) {
                let root = home.join(".claude").join("projects");
            }
        }
    "#;
    let hits = scan(&[Source::new("session_chat/history/fixture_tests.rs", fixture)]);
    assert!(hits.iter().all(|h| h.rule != "test_join"), "{}", describe(&hits));
}
