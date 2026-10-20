//! Regression guard: no shipped code may start a helper process except through `proc_util`.
//!
//! The rule (see `proc_util.rs`): a child process of the GUI app that is not a pane goes through
//! `proc_util::hidden_command` / `hidden_tokio_command` / `hide_window`, so it can never open a
//! console window. This test reads every `.rs` file under `src/`, drops what is not shipped
//! (comments, string contents, `#[cfg(test)]` items and modules, the `dev-fake-tui` harness) and
//! fails on a direct `Command::new(` or on an ad-hoc `creation_flags(` call.
//!
//! A documented exception goes in [`EXCEPTIONS`] with its reason. An exception that no longer
//! needs to be one (the file no longer starts a process itself) fails too, so the list cannot rot.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Files allowed to call `Command::new(` / `creation_flags(` themselves, and why. Paths are relative
/// to `src/`, with `/` separators.
const EXCEPTIONS: &[(&str, &str)] = &[
    (
        "proc_util.rs",
        "the helper itself: it is where the flag is set",
    ),
    (
        "tui/declarative/wizard/exec.rs",
        "sets CREATE_NO_WINDOW itself on every spawn and on its taskkill; the wizard answerer is being \
         edited on another branch, so it moves to the helper after that merges",
    ),
    (
        "tui/declarative/wizard/visible/api.rs",
        "sets CREATE_NO_WINDOW itself on its git probe; same reason as wizard/exec.rs",
    ),
];

/// Needles looked for in shipped code.
const FORBIDDEN: &[(&str, &str)] = &[
    (
        "Command::new(",
        "start helper processes with `crate::proc_util::hidden_command(program)` (or `hidden_tokio_command`)",
    ),
    (
        "creation_flags(",
        "do not set process-creation flags by hand; use `crate::proc_util::hide_window(&mut cmd)`",
    ),
];

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rel(path: &Path) -> String {
    path.strip_prefix(src_dir()).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// Every `.rs` file under `dir`, sorted.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = fs::read_dir(dir).expect("read dir").filter_map(Result::ok).map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// An attribute that makes the item below it absent from the app users run: `cfg(test)`,
/// `cfg(all(test, ...))` and the dev-only `dev-fake-tui` feature (never `not(test)`).
fn is_unshipped_cfg(attr: &str) -> bool {
    let compact: String = attr.chars().filter(|c| !c.is_whitespace()).collect();
    if !compact.starts_with("#[cfg(") {
        return false;
    }
    if compact.contains("feature=\"dev-fake-tui\"") && !compact.contains("not(feature") {
        return true;
    }
    compact.starts_with("#[cfg(test)]") || compact.starts_with("#[cfg(all(test,")
}

/// `source` with comments and the inside of string/char literals blanked to spaces (newlines kept,
/// so line numbers hold). Also returns the raw text of every `#[...]` attribute with its start offset.
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

/// `blanked` code (from `raw`, same offsets) with every item guarded by an unshipped `#[cfg(...)]`
/// blanked out, plus the names of `mod x;` files that guard declared (their files are not shipped
/// either). The attribute is read from `raw`, because blanking empties the `"dev-fake-tui"` string.
fn drop_unshipped_items(raw: &str, blanked: &str) -> (String, Vec<String>) {
    let b = blanked.as_bytes();
    let mut out = b.to_vec();
    let mut test_mods = Vec::new();
    let mut i = 0;
    while let Some(found) = blanked[i..].find("#[") {
        let start = i + found;
        let Some(close) = matching(b, start + 1, b'[', b']') else { break };
        let attr = &raw[start..=close];
        i = close + 1;
        if !is_unshipped_cfg(attr) {
            continue;
        }
        // Skip more attributes, then the item: up to `;` or through its `{ ... }` body.
        let mut j = i;
        loop {
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if blanked[j..].starts_with("#[") {
                j = matching(b, j + 1, b'[', b']').map_or(b.len(), |c| c + 1);
            } else {
                break;
            }
        }
        let item_start = j;
        let mut end = b.len();
        let mut k = j;
        while k < b.len() {
            match b[k] {
                b';' => {
                    end = k + 1;
                    break;
                }
                b'{' => {
                    end = matching(b, k, b'{', b'}').map_or(b.len(), |c| c + 1);
                    break;
                }
                _ => k += 1,
            }
        }
        let item = &blanked[item_start..end];
        if item.trim_end().ends_with(';') {
            if let Some(name) = mod_name(item) {
                test_mods.push(name);
            }
        }
        for byte in &mut out[start..end] {
            if *byte != b'\n' && *byte != b'\r' {
                *byte = b' ';
            }
        }
        i = end;
    }
    (String::from_utf8(out).expect("utf-8"), test_mods)
}

/// `pub(crate) mod name;` -> `name`.
fn mod_name(item: &str) -> Option<String> {
    let words: Vec<&str> = item.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).collect();
    let at = words.iter().position(|w| *w == "mod")?;
    words.get(at + 1).map(|w| w.to_string())
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

/// Where the file for `mod name;` declared in `declaring` lives (both candidates).
fn mod_files(declaring: &Path, name: &str) -> [PathBuf; 2] {
    let stem = declaring.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let dir = declaring.parent().unwrap_or(Path::new(""));
    let base = if matches!(stem, "mod" | "lib" | "main") { dir.to_path_buf() } else { dir.join(stem) };
    [base.join(format!("{name}.rs")), base.join(name).join("mod.rs")]
}

struct Hit {
    file: String,
    line: usize,
    needle: &'static str,
    advice: &'static str,
}

/// Shipped files with their blanked code, and the files dropped as unshipped.
fn shipped_code() -> Vec<(String, String)> {
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);
    // Pass 1: which files are declared `#[cfg(test)] mod x;` (and so are not shipped, nor is anything below them).
    let mut unshipped_roots: BTreeSet<PathBuf> = BTreeSet::new();
    let mut code = Vec::new();
    for path in &files {
        let raw = fs::read_to_string(path).expect("read source");
        let (kept, test_mods) = drop_unshipped_items(&raw, &blank_literals(&raw));
        for name in test_mods {
            for candidate in mod_files(path, &name) {
                if candidate.is_file() {
                    unshipped_roots.insert(candidate);
                }
            }
        }
        code.push((path.clone(), kept));
    }
    let under_unshipped = |path: &Path| {
        unshipped_roots.iter().any(|root| {
            path == root || {
                let stem = root.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                let dir = if stem == "mod" { root.parent().map(Path::to_path_buf) } else { Some(root.with_extension("")) };
                dir.is_some_and(|dir| path.starts_with(dir))
            }
        })
    };
    code.into_iter().filter(|(path, _)| !under_unshipped(path)).map(|(path, text)| (rel(&path), text)).collect()
}

fn scan(files: &[(String, String)]) -> Vec<Hit> {
    let mut hits = Vec::new();
    for (file, text) in files {
        for &(needle, advice) in FORBIDDEN {
            for (index, line) in text.lines().enumerate() {
                if line.contains(needle) {
                    hits.push(Hit { file: file.clone(), line: index + 1, needle, advice });
                }
            }
        }
    }
    hits
}

#[test]
fn no_direct_command_new() {
    let files = shipped_code();
    assert!(files.len() > 100, "the scan found only {} files; is the source tree layout different?", files.len());
    let hits: Vec<_> = scan(&files).into_iter().filter(|hit| !EXCEPTIONS.iter().any(|(path, _)| *path == hit.file)).collect();
    if !hits.is_empty() {
        let list: Vec<String> = hits.iter().map(|h| format!("  {}:{}  `{}` -> {}", h.file, h.line, h.needle, h.advice)).collect();
        panic!(
            "a helper process is started without proc_util (it would flash a console window on Windows):\n{}\n\
             If a spawn truly cannot use the helper, add the file to EXCEPTIONS in src/proc_util/guard.rs with the reason.",
            list.join("\n")
        );
    }
}

#[test]
fn exceptions_are_real_and_still_set_the_flag_themselves() {
    let files = shipped_code();
    let hits = scan(&files);
    for (path, reason) in EXCEPTIONS {
        assert!(!reason.trim().is_empty(), "{path}: an exception needs a reason");
        assert!(
            hits.iter().any(|hit| hit.file == *path),
            "{path} is listed in EXCEPTIONS but no longer calls Command::new / creation_flags itself; remove it"
        );
        if *path != "proc_util.rs" {
            // A file that spawns on its own must hide the window on its own.
            let raw = fs::read_to_string(src_dir().join(path)).expect("exception file");
            assert!(
                raw.contains("creation_flags(0x0800_0000)"),
                "{path} starts processes itself but does not set CREATE_NO_WINDOW"
            );
        }
    }
}

/// The scanner itself: a fixture with one violation and every kind of look-alike.
#[test]
fn the_scanner_sees_real_calls_and_ignores_look_alikes() {
    let fixture = r##"
        // Command::new("in a comment")
        /* Command::new("in a block comment") /* nested */ */
        fn shipped() {
            let message = "Command::new(\"in a string\")";
            let raw = r#"Command::new("in a raw string")"#;
            let tick = '"'; let life: &'static str = "x";
            let ok = CommandBuilder::new("a pane");
            let bad = std::process::Command::new("git");
        }
        #[cfg(test)]
        mod tests {
            fn t() { let x = std::process::Command::new("git"); { } }
        }
        #[cfg(all(test, windows))]
        fn only_in_tests() { Command::new("tasklist"); }
        #[cfg(not(test))]
        fn also_shipped() { Command::new("shipped-too"); }
        #[cfg(test)]
        mod helper_tests;
    "##;
    let (code, test_mods) = drop_unshipped_items(fixture, &blank_literals(fixture));
    let found: Vec<&str> = code.lines().filter(|l| l.contains("Command::new(")).map(str::trim).collect();
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().any(|l| l.contains("let bad")), "{found:?}");
    assert!(found.iter().any(|l| l.contains("also_shipped")), "{found:?}");
    assert!(!found.iter().any(|l| l.contains("only_in_tests") || l.contains("fn t()")), "{found:?}");
    assert_eq!(test_mods, ["helper_tests"]);
}

/// The tree today: the helper's own file is the only place a std `Command` is made from a bare name.
#[test]
fn the_scan_covers_the_files_that_matter() {
    let files = shipped_code();
    let names: BTreeSet<&str> = files.iter().map(|(f, _)| f.as_str()).collect();
    for must in ["agent_runs/headless/process.rs", "tui/cursor/launch.rs", "pty/proc_tree.rs", "mcp_sessions/access.rs", "proc_util.rs"] {
        assert!(names.contains(must), "{must} is not scanned");
    }
    for must_not in ["proc_util/tests.rs", "proc_util/guard.rs", "fake_tui/world.rs"] {
        assert!(!names.contains(must_not), "{must_not} is test or dev-only code and must not be scanned");
    }
}
