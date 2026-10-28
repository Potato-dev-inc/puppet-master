//! Guards for the privacy design of Live Room (SPEC-n32 section 4): it is never a bridge route or an MCP tool, only
//! the app window can start it, viewers have no way to send anything to a pane, and the link is never logged or
//! audited. These read the source files, so a later change that breaks a rule fails here.

use std::path::{Path, PathBuf};

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `.rs` file under `dir` (or the one file `dir` names), with its text.
fn sources(dir: &Path) -> Vec<(PathBuf, String)> {
    if dir.is_file() {
        return vec![(dir.to_path_buf(), std::fs::read_to_string(dir).unwrap())];
    }
    let mut found = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(next) = todo.pop() {
        for entry in std::fs::read_dir(&next).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                todo.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                found.push((path, text));
            }
        }
    }
    assert!(!found.is_empty(), "no sources under {}", dir.display());
    found
}

fn is_this_file(path: &Path) -> bool {
    path.file_name() == Path::new(file!()).file_name()
}

/// The Live Room sources without this file: the words of the rules below are in here.
fn live_room_sources() -> Vec<(PathBuf, String)> {
    sources(&src_dir().join("live_room")).into_iter().filter(|(path, _)| !is_this_file(path)).collect()
}

#[test]
fn no_bridge_phone_or_tool_source_mentions_live_room_or_rooms() {
    // Built from pieces so this file does not contain the words itself.
    let words = [["live", "_room"].concat(), ["live", "room"].concat(), ["live", " room"].concat(), ["ro", "oms"].concat()];
    for place in ["bridge", "mobile_pairing", "pwa_server.rs", "tool_registry", "mcp_shim"] {
        for (path, text) in sources(&src_dir().join(place)) {
            let lower = text.to_lowercase();
            for word in &words {
                assert!(!lower.contains(word.as_str()), "{} mentions {word}: Live Room must not be a bridge route or a tool", path.display());
            }
        }
    }
}

#[test]
fn the_tunnel_auth_tests_do_not_know_live_room() {
    // The bridge's auth tests stay as they were: Live Room adds no route they would have to cover.
    let word = ["live", "_room"].concat();
    for file in ["tunnel_auth_tests.rs", "device_caps_tests.rs"] {
        let text = std::fs::read_to_string(src_dir().join(file)).unwrap().to_lowercase();
        assert!(!text.contains(word.as_str()), "{file} mentions Live Room");
    }
}

#[test]
fn catalog_version_is_the_pinned_one() {
    // The same value `tool_registry/tests.rs` pins: adding Live Room must not move it.
    assert_eq!(crate::tool_registry::catalog_version(), "73-caaf894656d938ac");
}

#[test]
fn live_room_never_drives_a_pane() {
    // Viewers can never send anything: nothing here writes to a pane, presses a key, answers a card or sends a message.
    let banned = [
        ["write", "_input"].concat(),
        ["press", "_key"].concat(),
        ["answer", "_prompt"].concat(),
        ["send", "_message"].concat(),
    ];
    for (path, text) in live_room_sources() {
        for word in &banned {
            assert!(!text.contains(word.as_str()), "{} mentions {word}", path.display());
        }
    }
}

#[test]
fn the_commands_refuse_a_preview_webview_and_other_labels() {
    use crate::live_room::service::{check_window, LIVE_ROOM_WINDOW_ONLY};
    let app_windows = ["cockpit", "main"];
    assert!(check_window("cockpit", &app_windows).is_ok());
    assert!(check_window("main", &app_windows).is_ok());
    for label in ["pane-preview-x", "pane-preview-1", "preview-1", "detached-pane-1", "askbar", "pointbox", "worker"] {
        assert_eq!(check_window(label, &app_windows), Err(LIVE_ROOM_WINDOW_ONLY.to_string()), "{label}");
    }
}

/// The text of the command `name` in `commands/live_room.rs`: from its `#[tauri::command]` to the next one.
fn command_text(file: &str, name: &str) -> String {
    let marker = format!("fn {name}(");
    let at = file.find(&marker).unwrap_or_else(|| panic!("no command {name}"));
    let from = file[..at].rfind("#[tauri::command]").unwrap_or_else(|| panic!("{name} is not a command"));
    let to = file[at..].find("#[tauri::command]").map_or(file.len(), |n| at + n);
    file[from..to].to_string()
}

#[test]
fn every_command_that_shows_or_changes_what_is_shared_checks_its_window() {
    let file = std::fs::read_to_string(src_dir().join("commands").join("live_room.rs")).unwrap();
    for name in ["live_room_preview", "live_room_start", "live_room_copy_link", "live_room_config_get", "live_room_config_set"] {
        let text = command_text(&file, name);
        assert!(text.contains("webview: tauri::Webview"), "{name} takes no webview");
        assert!(text.contains("guard(&webview"), "{name} does not check its window");
    }
    // Stop, pause, resume and status only reduce exposure or read two numbers: any app window may call them.
    for name in ["live_room_stop", "live_room_pause", "live_room_resume", "live_room_status"] {
        let text = command_text(&file, name);
        assert!(!text.contains("copy_link") && !text.contains(".link()"), "{name} must not touch the link");
    }
}

#[test]
fn the_nine_commands_are_registered_once_in_lib_rs() {
    let lib = std::fs::read_to_string(src_dir().join("lib.rs")).unwrap();
    let names = [
        "live_room_preview",
        "live_room_start",
        "live_room_stop",
        "live_room_pause",
        "live_room_resume",
        "live_room_status",
        "live_room_copy_link",
        "live_room_config_get",
        "live_room_config_set",
    ];
    for name in names {
        assert_eq!(lib.matches(&format!("commands::{name},")).count(), 1, "{name} is registered once");
    }
    assert_eq!(lib.matches("commands::live_room_").count(), names.len());
}

#[test]
fn only_the_glue_files_know_the_room_state() {
    // The window commands start the room; the panic path and the exit hook stop it. Nothing else holds the state.
    let state = ["Live", "RoomState"].concat();
    let allowed = ["lib.rs", "panic.rs", "commands/live_room.rs", "commands/mod.rs"];
    for (path, text) in sources(&src_dir()) {
        let name = path.strip_prefix(src_dir()).unwrap().to_string_lossy().replace('\\', "/");
        if name.starts_with("live_room/") || allowed.contains(&name.as_str()) {
            continue;
        }
        assert!(!text.contains(state.as_str()), "{name} uses the Live Room state");
    }
}

#[test]
fn log_lines_are_fixed_text_and_nothing_is_audited() {
    // A log line in Live Room is a plain sentence: no value is formatted into it, so the link, the key, the room
    // id and the viewers' count can never get there. Nothing is written to the audit log either.
    let audit = ["aud", "it"].concat();
    for (path, text) in live_room_sources() {
        let name = path.to_string_lossy().to_string();
        let production = text.split("#[cfg(test)]").next().unwrap_or("");
        assert!(!production.to_lowercase().contains(audit.as_str()), "{name} mentions the audit log");
        for banned in ["println!", "eprintln!", "dbg!"] {
            assert!(!production.contains(banned), "{name} uses {banned}");
        }
        let mut rest = production;
        while let Some(at) = rest.find("tracing::") {
            rest = &rest[at..];
            let end = rest.find(");").map_or(rest.len(), |n| n + 2);
            let statement = &rest[..end];
            let plain = statement.split_once('(').is_some_and(|(_, args)| {
                let args = args.trim_end_matches(';').trim_end_matches(')').trim();
                args.starts_with('"') && args.ends_with('"') && !args[1..args.len() - 1].contains(['"', '{', '}'])
            });
            assert!(plain, "{name}: a log line must be a plain sentence, found `{statement}`");
            rest = &rest["tracing::".len()..];
        }
    }
}

#[test]
fn the_client_sends_no_viewer_header() {
    // The app reads nothing a viewer wrote: the client only makes the owner's calls and reads two numbers back.
    let client = std::fs::read_to_string(src_dir().join("live_room").join("client.rs")).unwrap();
    let production = client.split("#[cfg(test)]").next().unwrap();
    for viewer_header in ["x-pm-room-read", "x-pm-room-seat", "ROOM_READ_HEADER", "ROOM_SEAT_HEADER"] {
        assert!(!production.contains(viewer_header), "client.rs sends a viewer header: {viewer_header}");
    }
}
