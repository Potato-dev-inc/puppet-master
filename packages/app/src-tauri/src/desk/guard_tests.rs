//! Source guards for the rules of My computers. They read the source files, so a change that breaks a rule fails here.

use std::path::{Path, PathBuf};

use regex::Regex;

fn tauri_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn packages() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Every file with one of these extensions under `root` (or `root` itself when it is a file).
fn files(root: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if root.is_file() {
        out.push(root.to_path_buf());
        return out;
    }
    let Ok(rd) = std::fs::read_dir(root) else { return out };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "node_modules" || name == "target" || name == "dist" {
                continue;
            }
            out.extend(files(&p, exts));
        } else if p.extension().and_then(|e| e.to_str()).is_some_and(|e| exts.contains(&e)) {
            out.push(p);
        }
    }
    out
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default().replace("\r\n", "\n")
}

/// Comments dropped, so the words of a rule written in a comment do not trip it.
fn code_only(source: &str) -> String {
    source
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .map(|l| l.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The desk files that ship (not the test files, which are the checkers).
fn desk_shipped_files() -> Vec<PathBuf> {
    let dir = tauri_src().join("desk");
    files(&dir, &["rs"])
        .into_iter()
        .filter(|p| {
            let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            n != "tests.rs" && n != "guard_tests.rs" && n != "mock.rs"
        })
        .collect()
}

/// No agent, no MCP tool and no phone can drive the mouse: nothing outside the desktop window's own command
/// list may reach the desk module, its commands, or its Labs switch.
#[test]
fn nothing_reachable_by_agents_mcp_or_the_phone_mentions_desk() {
    let pattern = Regex::new(r"\bdesk::|crate::desk\b|\bmod desk\b|desk_snapshot|desk_command|\bdesk_enabled\b|desk-core|desk-proto|desk_core|desk_proto").unwrap();
    let src = tauri_src();
    let pkgs = packages();
    let mut roots: Vec<PathBuf> = vec![
        src.join("bridge"),
        src.join("tool_registry"),
        src.join("mcp_shim"),
        src.join("companion"),
        src.join("mobile_pairing"),
        src.join("pwa_server.rs"),
        src.join("channels"),
        src.join("bin"),
        pkgs.join("mcp-server").join("src"),
        pkgs.join("cli").join("src"),
        pkgs.join("app").join("src").join("pwa"),
        pkgs.join("app").join("src").join("PwaApp.tsx"),
        pkgs.join("app").join("src").join("PwaLegacyApp.tsx"),
    ];
    roots.retain(|r| r.exists());
    assert!(roots.len() >= 8, "the guard must actually find the places it guards: {roots:?}");
    let mut seen = 0;
    for root in roots {
        for file in files(&root, &["rs", "ts", "tsx", "js", "mjs", "cjs"]) {
            seen += 1;
            let text = code_only(&read(&file));
            if let Some(m) = pattern.find(&text) {
                panic!("{} reaches My computers ({:?}): agents, MCP and the phone must never drive the mouse", file.display(), m.as_str());
            }
        }
    }
    assert!(seen > 50, "only {seen} files scanned");
}

/// The desk commands are registered once, and only as desktop window commands.
#[test]
fn the_two_commands_are_the_only_desk_entry_points() {
    let lib = read(&tauri_src().join("lib.rs"));
    assert_eq!(lib.matches("desk::commands::desk_snapshot").count(), 1);
    assert_eq!(lib.matches("desk::commands::desk_command").count(), 1);
    let commands = read(&tauri_src().join("desk").join("commands.rs"));
    assert_eq!(commands.matches("#[tauri::command]").count(), 2);
    assert!(!commands.contains("pub fn "), "commands are async and go through Desk, which checks the Labs switch");
}

/// This phase has no network code at all.
#[test]
fn desk_has_no_network_code() {
    let banned = ["std::net", "tokio::net", "ureq", "tungstenite", "reqwest", "hyper", "TcpStream", "UdpSocket", "TcpListener", "iroh", "quinn"];
    for file in desk_shipped_files() {
        let text = code_only(&read(&file));
        for word in banned {
            assert!(!text.contains(word), "{} uses {word}: no network code in this phase", file.display());
        }
    }
}

/// This phase reads and moves the pointer and listens for keys. It injects nothing and changes no process setting.
#[test]
fn desk_injects_no_input_and_changes_no_process_setting() {
    let banned = [
        "SendInput",
        "mouse_event",
        "keybd_event",
        "WH_MOUSE_LL",
        "RegisterHotKey",
        "SetProcessDpiAwarenessContext",
        "SetProcessDPIAware",
        "ClipCursor",
        "ShowCursor",
        "BlockInput",
    ];
    for file in desk_shipped_files() {
        let text = code_only(&read(&file));
        for word in banned {
            assert!(!text.contains(word), "{} uses {word}", file.display());
        }
    }
}

/// Tests never build the real Windows host or call the real default host.
#[test]
fn desk_tests_use_only_the_mock_host() {
    for name in ["tests.rs", "guard_tests.rs"] {
        let text = read(&tauri_src().join("desk").join(name));
        let code = code_only(&text);
        let real = ["Windows", "Host::new"].concat();
        let os_win = ["os::", "windows"].concat();
        let dflt = ["default_", "host()"].concat();
        assert!(!code.contains(&real), "{name} builds the real host");
        assert!(!code.contains(&dflt), "{name} asks for the real host");
        assert!(!code.contains(&os_win), "{name} reaches into the real host");
    }
    // And the app-wide instance is built from the stub in a test build.
    let os_mod = read(&tauri_src().join("desk").join("os").join("mod.rs"));
    assert!(os_mod.contains("all(windows, not(test))"), "the real host must be excluded from test builds");
}

/// The hook runs on a thread of its own: the thread is started before the hook is installed, and the hook is
/// installed in exactly one place.
#[test]
fn the_hook_is_installed_only_on_its_own_named_thread() {
    let text = code_only(&read(&tauri_src().join("desk").join("os").join("windows").join("hook.rs")));
    assert_eq!(text.matches("SetWindowsHookExW(").count(), 1);
    let spawn = text.find("thread::Builder::new().name(\"desk-hook\"").expect("a named hook thread");
    let install = text.find("SetWindowsHookExW(").unwrap();
    assert!(spawn < install, "the hook thread must be spawned before the hook is installed");
    assert!(text.contains("GetMessageW"), "the hook thread needs its own message loop");
    // The callback never waits.
    assert!(text.contains("try_lock"));
    let callback = &text[text.find("unsafe extern \"system\" fn key_proc").unwrap()..];
    assert!(!callback.contains(".lock()"), "the hook callback must use try_lock only");
    assert!(!callback.contains("recv") && !callback.contains("sleep") && !callback.contains(".send("));
}

/// Files with behaviour taken from desktoplab say so on their first line.
#[test]
fn ported_files_carry_the_source_header() {
    let ported = ["hotkeys.rs", "hops.rs", "config.rs", "hook.rs", "dpi.rs", "cursor.rs", "display.rs", "host.rs"];
    let mut found = 0;
    for file in desk_shipped_files() {
        let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if ported.contains(&name) {
            found += 1;
            assert!(read(&file).starts_with("// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT."), "{} lacks the header", file.display());
        }
    }
    assert_eq!(found, ported.len());
}
