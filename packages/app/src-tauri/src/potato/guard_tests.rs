//! The two guards of spec section 0, over `src/potato/`: generic code never names an agent, and never
//! names a chat app. A chat app's code lives only in `potato/surfaces/<app>/`, the way a chat app's code
//! for the existing channels lives only in its own folder. Also: the brain starts only through
//! `proc_util::hidden_command`.

use std::path::{Path, PathBuf};

const AGENTS: [&str; 6] = ["claude", "codex", "cursor", "opencode", "agy", "kilo"];
const CHAT_APPS: [&str; 8] = ["discord", "telegram", "slack", "whatsapp", "imessage", "wechat", "mattermost", "signal-cli"];

/// The one file that may hold names that are fixed by the brain package's wire format and are also the
/// names of other things. Listed with its reason; `exceptions_are_real` fails if it stops needing it.
const AGENT_NAME_EXCEPTIONS: [(&str, &str); 1] =
    [("wire_names.rs", "the brain package's own provider id and the inbox's `cursor` position are wire names, not agents")];

fn potato_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("potato")
}

/// Every `.rs` file of `potato/` that is generic: not a test file, not under `surfaces/`. Returns the
/// relative name and the code before its `#[cfg(test)]`, lowercased.
fn generic_code() -> Vec<(String, String)> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).expect("potato folder").flatten() {
            let path = entry.path();
            let rel = path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
            if path.is_dir() {
                if rel != "surfaces" {
                    walk(&path, base, out);
                }
            } else if rel.ends_with(".rs") && !rel.ends_with("_tests.rs") {
                let text = std::fs::read_to_string(&path).expect("read source");
                out.push((rel, text.split("#[cfg(test)]").next().unwrap_or("").to_lowercase()));
            }
        }
    }
    let mut files = Vec::new();
    walk(&potato_dir(), &potato_dir(), &mut files);
    files
}

#[test]
fn generic_code_never_names_an_agent() {
    let files = generic_code();
    assert!(files.len() >= 12, "the scan must see the potato files, saw {}", files.len());
    for (name, code) in &files {
        if AGENT_NAME_EXCEPTIONS.iter().any(|(file, _)| file == name) {
            continue;
        }
        for word in AGENTS {
            assert!(!code.contains(word), "{name} names an agent ({word}) outside its tests");
        }
    }
}

#[test]
fn generic_potato_names_no_chat_app() {
    let files = generic_code();
    assert!(files.len() >= 12);
    for (name, code) in &files {
        for word in CHAT_APPS {
            assert!(!code.contains(word), "{name} names a chat app ({word}); chat app code lives only in potato/surfaces/<app>/");
        }
    }
}

#[test]
fn the_agent_exceptions_are_real_and_small() {
    let files = generic_code();
    for (file, reason) in AGENT_NAME_EXCEPTIONS {
        assert!(!reason.trim().is_empty(), "{file}: an exception needs a reason");
        let (_, code) = files.iter().find(|(name, _)| name == file).unwrap_or_else(|| panic!("{file} is listed but gone"));
        assert!(AGENTS.iter().any(|word| code.contains(word)), "{file} no longer needs its exception; remove it");
        assert!(code.lines().count() < 30, "{file} is meant to stay a tiny list of names");
    }
}

#[test]
fn the_guards_see_a_violation() {
    // The scanner itself: a fixture with a name in it is caught the way the real files would be.
    let code = "fn pick() { if kind == \"codex\" { } }".to_lowercase();
    assert!(AGENTS.iter().any(|word| code.contains(word)));
    let code = "use discord::Client;".to_lowercase();
    assert!(CHAT_APPS.iter().any(|word| code.contains(word)));
    let tests_are_exempt = "fn ok() {}\n#[cfg(test)]\nmod t { const A: &str = \"claude\"; }";
    assert!(!tests_are_exempt.split("#[cfg(test)]").next().unwrap().contains("claude"));
}

#[test]
fn the_brain_is_started_only_through_the_hidden_command_helper() {
    let sidecar = std::fs::read_to_string(potato_dir().join("sidecar.rs")).unwrap();
    assert!(sidecar.contains("crate::proc_util::hidden_command(&spec.node)"), "the brain starts through hidden_command");
    assert!(sidecar.contains("crate::proc_util::hidden_command(&path)"), "the version probe too");
    for (name, code) in generic_code() {
        assert!(!code.contains("command::new("), "{name} starts a process on its own; use proc_util::hidden_command");
    }
}

#[test]
fn nothing_in_the_potato_folder_reads_the_port_file_or_the_local_secret() {
    // The brain's environment has neither; and the engine never hands it any.
    for (name, code) in generic_code() {
        assert!(!code.contains("local_secret"), "{name} touches the bridge local secret");
        assert!(!code.contains("bridge_port_file"), "{name} touches the port file");
    }
}

#[test]
fn nothing_logs_a_message_or_an_environment() {
    // The brain's lines are cleaned by `log.rs`; the engine itself never logs message text or the env.
    for (name, code) in generic_code() {
        for needle in ["tracing::info!(", "tracing::warn!(", "tracing::error!(", "tracing::debug!("] {
            for line in code.lines().filter(|l| l.contains(needle)) {
                assert!(!line.contains("text") && !line.contains("env") && !line.contains("token") && !line.contains("key"), "{name}: {line}");
            }
        }
    }
}

#[test]
fn the_log_ring_keeps_200_lines_and_cleans_every_one() {
    use super::log::*;
    let secret = "sk-test-the-model-key-0123456789".to_string();
    let mut ring = LogRing::new();
    for n in 0..(RING_LINES + 50) {
        ring.push(&format!("line {n} with {secret} inside"), &[secret.clone()]);
    }
    assert_eq!(ring.len(), RING_LINES);
    let lines = ring.lines();
    assert_eq!(lines.first().unwrap(), "line 50 with [hidden] inside");
    assert!(lines.iter().all(|l| !l.contains(&secret)));
    // the order: exact values first, then token shapes, then the cut
    let long = format!("{} sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghij tail", "a".repeat(250));
    let cleaned = clean(&long, &[]);
    assert!(cleaned.chars().count() <= LINE_CHARS);
    assert!(!cleaned.contains("sk-ant-api03-ABCDEF"));
    // a secret that straddles the cut is hidden before the cut, not left half visible
    let straddle = format!("{}{secret}", "b".repeat(LINE_CHARS - 10));
    let cleaned = clean(&straddle, &[secret.clone()]);
    assert!(!cleaned.contains("sk-test"), "{cleaned}");
    // a too-short secret is not matched by value (it would hide ordinary words)
    assert_eq!(clean("the word cat", &["cat".to_string()]), "the word cat");
    // newlines go
    assert_eq!(clean("one\r\n", &[]), "one");
}
