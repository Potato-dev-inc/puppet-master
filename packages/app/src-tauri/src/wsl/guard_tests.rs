//! Source-scan guards for the Linux Inside rules (spec section 4). Tests only.
//! They read the non-test sources of `wsl/` (`wsl/*.rs` minus `*_tests.rs`, and the inline
//! `#[cfg(test)]` blocks inside them) and fail when a forbidden word shows up.

use std::fs;
use std::path::{Path, PathBuf};

/// The non-test source files of `wsl/`, sorted.
fn wsl_sources() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("wsl");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("read src/wsl")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .filter(|path| !path.to_string_lossy().ends_with("_tests.rs"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no wsl sources found");
    files
}

/// The lines of one source that are not inside a `#[cfg(test)]` item (a block or a `mod x;`).
/// Brace counting is enough here: the sources are rustfmt-shaped and the test blocks are balanced.
fn production_lines(src: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut armed = false; // a `#[cfg(test)]` was seen; the next item is test code
    let mut depth: i64 = 0; // open braces inside a skipped test item (0 = not skipping)
    for (index, line) in src.lines().enumerate() {
        let trimmed = line.trim();
        if depth == 0 && trimmed.starts_with("#[cfg(test)]") {
            armed = true;
            continue;
        }
        if depth == 0 && !armed {
            out.push((index + 1, line));
            continue;
        }
        if depth == 0 && !line.contains('{') && trimmed.ends_with(';') {
            // A test item without a block (`mod live_tests;`): skip this line and stop arming.
            armed = false;
            continue;
        }
        for ch in line.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    armed = false;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
    }
    out
}

/// Every production line of `wsl/` that holds one of `needles` (lower case, matched against the
/// lower-cased line), unless `allowed` says the line is fine. Returns `file:line` findings.
fn findings(needles: &[&str], allowed: impl Fn(&str) -> bool) -> Vec<String> {
    let mut found = Vec::new();
    for path in wsl_sources() {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let src = fs::read_to_string(&path).expect("read wsl source");
        for (number, line) in production_lines(&src) {
            if allowed(line) {
                continue;
            }
            let lower = line.to_lowercase();
            for needle in needles {
                if lower.contains(needle) {
                    found.push(format!("{name}:{number} contains {needle:?}"));
                }
            }
        }
    }
    found
}

#[test]
fn s2_no_credential_files_are_named_in_wsl() {
    let found = findings(&[".credentials", "auth.json", ".netrc", "id_rsa", "keyring"], |_| false);
    assert!(found.is_empty(), "credential names in wsl/: {found:?}");
}

#[test]
fn s3_no_wsl_shutdown_and_no_name_based_kills() {
    let needles = [
        "--shutdown",
        "--terminate",
        "--unregister",
        " -t ",
        "pkill",
        "killall",
        "taskkill",
        "/im",
    ];
    let found = findings(&needles, |_| false);
    assert!(found.is_empty(), "shutdown or name-based kill in wsl/: {found:?}");
}

#[test]
fn s5_pm_never_runs_wsl_admin_commands() {
    let needles = [
        "--install",
        "--set-version",
        "--import",
        "--unregister",
        "--set-default",
        "--shutdown",
        "--terminate",
        "--update",
        ".wslconfig",
    ];
    // Only the two text constants that the UI shows (never run) may hold these words.
    let text_only = |line: &str| {
        line.contains("pub const INSTALL_COMMAND") || line.contains("pub const ADD_DISTRO_COMMAND")
    };
    let found = findings(&needles, text_only);
    assert!(found.is_empty(), "WSL admin command or config file in wsl/: {found:?}");
}

#[test]
fn s5_the_install_text_is_only_shown_by_the_model() {
    // The two commands are text for the UI. Nothing else in `wsl/` may name them (so nothing can
    // pass them to a runner).
    let found = findings(&["install_command", "add_distro_command"], |line| {
        line.contains("pub const INSTALL_COMMAND") || line.contains("pub const ADD_DISTRO_COMMAND")
    });
    let elsewhere: Vec<&String> = found.iter().filter(|f| !f.starts_with("model.rs:")).collect();
    assert!(elsewhere.is_empty(), "install commands used outside model.rs: {elsewhere:?}");
}

#[test]
fn s7_nothing_in_wsl_writes_into_a_pane() {
    let needles = [
        "write_input",
        "registry_write_input",
        "press_key",
        "typingpermit",
        "gatepermit",
    ];
    let found = findings(&needles, |_| false);
    assert!(found.is_empty(), "pane input writers in wsl/: {found:?}");
}

#[test]
fn s11_wsl_code_names_no_agent() {
    let needles = ["claude", "codex", "cursor", "opencode", "agy"];
    let found = findings(&needles, |_| false);
    assert!(found.is_empty(), "agent names in generic wsl/ code: {found:?}");
}

#[test]
fn production_lines_skip_inline_and_file_test_items() {
    let src = "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn b() { let _ = 1; }\n}\n#[cfg(test)]\n#[path = \"x.rs\"]\nmod x;\nfn c() {}\n";
    let kept: Vec<&str> = production_lines(src).into_iter().map(|(_, line)| line).collect();
    assert_eq!(kept, vec!["fn a() {}", "fn c() {}"]);
}
