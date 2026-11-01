//! Source guards for the rules of the ACP lane. They read this folder's own files.

use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src").join("acp")
}

/// Shipped code only: comments dropped, and everything from the first test module on.
fn shipped(source: &str) -> String {
    let cut = ["#[cfg(test)]\nmod tests {", "#[cfg(test)]\nmod labs_tests {"]
        .iter()
        .filter_map(|marker| source.find(marker))
        .min()
        .unwrap_or(source.len());
    source[..cut]
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn read(name: &str) -> String {
    std::fs::read_to_string(dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}")).replace("\r\n", "\n")
}

/// Generic code never names an agent: what differs between agents lives in `config.rs` as data.
#[test]
fn generic_code_names_no_agent() {
    let names = ["codex", "claude", "gemini", "cursor", "opencode", "kilo", "copilot", "nuphos", "openab"];
    let files = [
        "conn.rs", "conversations.rs", "error.rs", "handover.rs", "handshake.rs", "job.rs", "lane.rs", "mod.rs", "pin.rs",
        "overlay.rs", "record.rs", "ring.rs", "session.rs", "state.rs", "supervisor.rs", "timers.rs", "types.rs", "wire.rs",
        "write_queue.rs", "writers.rs", "bridge/mod.rs", "bridge/server.rs", "bridge/turn.rs",
        "../terminal_chat/mod.rs", "../terminal_chat/lane.rs", "../terminal_chat/start.rs",
        "../commands/acp_remove.rs", "chat/mod.rs",
    ];
    for file in files {
        let code = shipped(&read(file)).to_lowercase();
        for name in names {
            assert!(!code.contains(name), "{file} names an agent ({name}): agent specifics belong in config.rs");
        }
    }
}

/// The app never picks an option on the person's behalf: the only place that builds a "selected" answer to
/// a permission request is `answer_permission`, which is called with the option the person chose.
#[test]
fn only_answer_permission_selects_an_option() {
    let code = shipped(&read("session.rs"));
    assert_eq!(code.matches("selected_outcome(").count(), 1, "a second place answers a permission request with a choice");
    let at = code.find("selected_outcome(").unwrap();
    let before = &code[..at];
    let fn_start = before.rfind("pub fn ").unwrap();
    assert!(before[fn_start..].starts_with("pub fn answer_permission"), "selected_outcome is used outside answer_permission");
    // And nothing looks at the offered options to pick one.
    assert!(!code.contains("allow_once") && !code.contains("allow_always"), "session.rs must not know which option is the allowing one");
}

/// Every wait has a limit written down: no unbounded channel and no unbounded blocking recv in shipped code.
#[test]
fn no_unbounded_queues() {
    for file in ["conn.rs", "session.rs", "write_queue.rs", "supervisor.rs"] {
        let code = shipped(&read(file));
        assert!(!code.contains("mpsc::channel"), "{file}: use sync_channel (bounded)");
        assert!(!code.contains(".recv()"), "{file}: use recv_timeout");
    }
}

/// Processes start through `proc_util` (the repo-wide guard checks `Command::new`); this adds that the
/// connection also never lets the child inherit the app's environment.
#[test]
fn the_adapter_environment_is_cleared() {
    let code = shipped(&read("conn.rs"));
    assert!(code.contains(".env_clear()"));
    assert!(code.contains("proc_util::hidden_command"));
}

/// The official schema crate must not come back by accident: it turns on `serde_json/preserve_order` for
/// the whole app (see `types.rs`).
#[test]
fn the_acp_crates_stay_out_of_the_dependency_list() {
    let manifest = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    for line in manifest.lines().filter(|l| !l.trim_start().starts_with('#')) {
        assert!(!line.contains("agent-client-protocol"), "an ACP crate was added to Cargo.toml: {line}");
    }
}

/// The listing command that finds the person's tool servers runs hidden, with the same cleared environment as
/// the adapter, and is only ever stopped through its own handle (never by image name).
#[test]
fn the_tool_server_listing_runs_hidden_with_a_cleared_environment() {
    let code = shipped(&read("config.rs"));
    let at = code.find("fn capture_stdout").expect("capture_stdout");
    let body = &code[at..];
    assert!(body.contains("proc_util::hidden_command") && body.contains(".env_clear()"));
    assert!(!code.contains("taskkill") && !code.contains("/IM"), "never kill by image name");
}

/// Hand-over never touches the agent's session or credential files: it only passes the session id on.
#[test]
fn hand_over_reads_and_writes_no_agent_files() {
    for file in ["handover.rs", "conversations.rs", "writers.rs"] {
        let code = shipped(&read(file));
        for word in ["std::fs", "File::", "auth.json", ".credentials", "read_to_string"] {
            assert!(!code.contains(word), "{file} touches files ({word}); only record.rs keeps our own record");
        }
    }
}
