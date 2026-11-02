//! Guards: Model Bridge is a desktop-window feature. It is not a bridge route, an MCP tool, a phone route, and the
//! webview can never read a key back.

use std::path::{Path, PathBuf};

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn sources(path: &Path) -> Vec<(PathBuf, String)> {
    let mut found = Vec::new();
    let mut todo = vec![path.to_path_buf()];
    while let Some(next) = todo.pop() {
        if next.is_file() {
            if next.extension().is_some_and(|e| e == "rs") {
                found.push((next.clone(), std::fs::read_to_string(&next).unwrap()));
            }
            continue;
        }
        for entry in std::fs::read_dir(&next).unwrap().flatten() {
            todo.push(entry.path());
        }
    }
    assert!(!found.is_empty(), "no sources under {}", path.display());
    found
}

#[test]
fn model_bridge_is_not_reachable_from_bridge_mcp_or_phone_routes() {
    // Built from pieces so this file does not contain the word itself.
    let word = ["model", "_bridge"].concat();
    let places = [
        "bridge",
        "tool_registry",
        "mcp_shim",
        "mcp_sessions",
        "mobile_pairing",
        "agent_runs",
        "pwa_server.rs",
        "mobile_tunnel.rs",
        "tunnel_config.rs",
        "mcp_runtime.rs",
    ];
    for place in places.iter().filter(|p| src_dir().join(p).exists()) {
        for (path, text) in sources(&src_dir().join(place)) {
            assert!(!text.contains(&word), "{} mentions {word}: it must not be a bridge route, MCP tool or phone route", path.display());
        }
    }
}

#[test]
fn key_slots_are_not_ui_secrets_so_the_webview_cannot_read_them() {
    assert!(!crate::secrets::is_ui_secret("model_bridge:opencode-go"));
    assert!(!crate::secrets::is_provider_secret("model_bridge:opencode-go"));
    // The shared secret commands only take the names the UI may use.
    for (path, text) in sources(&src_dir().join("commands").join("secrets.rs")) {
        assert!(!text.contains("model_bridge"), "{} must not handle bridge keys", path.display());
    }
}

#[test]
fn commands_check_the_window_and_never_return_a_key() {
    let text = std::fs::read_to_string(src_dir().join("commands").join("model_bridge.rs")).unwrap();
    assert!(text.contains("webview.label()"), "commands must check the calling window");
    for banned in ["read_secret", "secrets.get(", ".get(&a.key_slot", "launch_env"] {
        assert!(!text.contains(banned), "commands/model_bridge.rs must not touch the raw key: {banned}");
    }
    // Every command takes the window and checks it.
    let commands = text.matches("#[tauri::command]").count();
    let checked = text.matches("on(&webview)?;").count() + text.matches("guard(&webview)?;").count();
    assert!(commands >= 7 && checked >= commands, "{commands} commands, {checked} checks");
}

#[test]
fn the_key_goes_to_a_pane_through_its_environment_not_its_arguments() {
    let text = std::fs::read_to_string(src_dir().join("pty").join("registry").join("spawn.rs")).unwrap();
    let line = text.lines().find(|l| l.contains("model_bridge::launch_env")).expect("the spawn path adds the key");
    assert!(line.contains("own_env.extend"), "the key must go to the environment: {line}");
    assert!(!line.contains("extra_args") && !line.contains("args"), "never argv: {line}");
}
