//! Review Desk: guard tests (SPEC n33 section 4: S1, S7, S11, S12). Source scans over the real files.

use std::path::{Path, PathBuf};

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `.rs` file under `dir`, with its text.
fn rust_files(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(rust_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            if let Ok(text) = std::fs::read_to_string(&path) {
                let text = text.replace("\r\n", "\n");
                out.push((path, text));
            }
        }
    }
    out
}

/// Where a file's test module starts.
const MARKER: &str = "#[cfg(test)]\nmod tests";

/// The part of a file before its test module.
fn production(text: &str) -> &str {
    text.split(MARKER).next().unwrap_or(text)
}

fn is_test_file(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    name.ends_with("_tests.rs") || name == "tests.rs"
}

/// S1: nothing that an agent, a phone, a companion, a channel, a teammate or a push can reach calls the actions.
#[test]
fn no_remote_surface_names_the_desk_actions() {
    let src = src_dir();
    let forbidden = [
        "review_desk::act",
        "review_desk::live::keep",
        "review_desk::live::throw_away",
        "review_desk::live::fix",
        "review_desk::{act",
        "review_desk::{gather, act",
    ];
    let mut hits = Vec::new();
    for folder in ["bridge", "tool_registry", "mcp_shim", "companion", "channels", "team", "push"] {
        for (path, text) in rust_files(&src.join(folder)) {
            for word in forbidden {
                if text.contains(word) {
                    hits.push(format!("{} names {word}", path.display()));
                }
            }
        }
    }
    assert!(hits.is_empty(), "{hits:?}");
}

/// S7, S8: Review Desk never sends text or presses keys in a pane. Ask to fix only keeps a draft.
#[test]
fn the_desk_never_writes_to_a_pane() {
    let words = ["send(", "send_", "outbox", "write_input", "send_keys", "press_key", "TypingPermit", "write_terminal"];
    let mut hits = Vec::new();
    for (path, text) in rust_files(&src_dir().join("review_desk")) {
        if is_test_file(&path) {
            continue;
        }
        let body = production(&text);
        for word in words {
            if body.contains(word) {
                hits.push(format!("{} has {word}", path.display()));
            }
        }
    }
    assert!(hits.is_empty(), "{hits:?}");
}

/// S11: the order is a table over recorded facts. `risk.rs` reads nothing but the model and the path roles.
#[test]
fn the_risk_table_imports_only_the_model_and_path_roles() {
    let text = std::fs::read_to_string(src_dir().join("review_desk/risk.rs")).unwrap().replace("\r\n", "\n");
    let uses: Vec<&str> = production(&text).lines().filter(|line| line.trim_start().starts_with("use ")).collect();
    for line in &uses {
        assert!(
            line.contains("super::model") || line.contains("crate::tamper::Role") || line.contains("sha2"),
            "unexpected import in risk.rs: {line}"
        );
    }
    for word in ["second_opinion", "agent_runs", "reqwest", "std::fs", "std::process"] {
        assert!(!production(&text).contains(word), "{word} in risk.rs");
    }
}

/// S12: generic code never names an agent.
#[test]
fn generic_code_names_no_agent() {
    let names = ["claude", "codex", "cursor", "opencode", "agy", "kilo"];
    let mut hits = Vec::new();
    let mut files = rust_files(&src_dir().join("review_desk"));
    let ui = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/components/cockpit/review");
    files.retain(|(path, _)| !is_test_file(path));
    for (path, text) in &files {
        let body = production(text).to_ascii_lowercase();
        for name in names {
            if body.contains(name) {
                hits.push(format!("{} names {name}", path.display()));
            }
        }
    }
    // The window code, when it is in this tree: non-test .ts and .tsx files.
    if let Ok(entries) = std::fs::read_dir(&ui) {
        for entry in entries.flatten() {
            let path = entry.path();
            let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if !(file.ends_with(".ts") || file.ends_with(".tsx")) || file.contains(".test.") {
                continue;
            }
            let body = std::fs::read_to_string(&path).unwrap_or_default().to_ascii_lowercase();
            for name in names {
                if body.contains(name) {
                    hits.push(format!("{} names {name}", path.display()));
                }
            }
        }
    }
    assert!(hits.is_empty(), "{hits:?}");
}
