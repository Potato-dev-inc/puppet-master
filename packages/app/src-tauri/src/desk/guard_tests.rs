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
    let pattern = Regex::new(r"\bdesk::|crate::desk\b|\bmod desk\b|desk_snapshot|desk_command|\bdesk_enabled\b|desk-core|desk-proto|desk_core|desk_proto|desk_link_|DeskLink|desk/input|desk/clipboard").unwrap();
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

/// The link commands are desktop window commands too: registered once, none of them public functions.
#[test]
fn the_link_commands_are_desktop_window_commands_only() {
    let lib = read(&tauri_src().join("lib.rs"));
    for name in ["desk_link_view", "desk_link_set_side", "desk_link_pause", "desk_link_panic"] {
        assert_eq!(lib.matches(&format!("desk::link::commands::{name}")).count(), 1, "{name}");
    }
    let commands = read(&tauri_src().join("desk").join("link").join("commands.rs"));
    assert_eq!(commands.matches("#[tauri::command]").count(), 4);
    assert!(!commands.contains("pub fn "));
    // Every one goes through `link_ready`, which says FEATURE_OFF while Labs is off.
    assert_eq!(commands.matches("link_ready(&app)?").count(), 4);
}

/// Nothing starts sharing by itself: the link starts only from the page's command (or a person's settings change),
/// never from a tool, an agent, a timer at app start or a network message.
#[test]
fn nothing_but_the_page_starts_the_link() {
    let mut starters = Vec::new();
    for file in files(&tauri_src(), &["rs"]) {
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        if name.ends_with("tests.rs") {
            continue;
        }
        let rel = file.strip_prefix(tauri_src()).unwrap().to_string_lossy().replace('\\', "/");
        let text = code_only(&read(&file));
        if text.contains("link.start()") || text.contains(".link.start(") {
            starters.push(rel);
        }
    }
    // The page's commands, and the hand-run soak tool (a pretend PC, never the real one).
    starters.sort();
    assert_eq!(starters, vec!["desk_soak.rs".to_string(), "pairing_commands.rs".to_string()]);
}

/// A copy is never put into a log line: tracing calls in the link carry sizes only.
#[test]
fn the_link_never_logs_a_copy() {
    let mut checked = 0;
    for file in files(&tauri_src().join("desk").join("link"), &["rs"]) {
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        if name == "tests.rs" || name == "mock.rs" {
            continue;
        }
        for line in code_only(&read(&file)).lines().filter(|l| l.contains("tracing::")) {
            checked += 1;
            assert!(!line.contains('?') && !line.contains('%') && !line.contains("{:") && !line.contains("{}"), "{name}: a log line may carry sizes only: {line}");
        }
    }
    assert!(checked >= 1, "the guard must see the log lines it guards");
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

/// Injecting input, capturing it and the clipboard live in two Windows files; nothing else may. Nobody registers a
/// hotkey, changes a process-wide setting, blocks input, clips or hides the pointer.
#[test]
fn input_injection_and_capture_live_in_two_files_only() {
    let only_there = ["SendInput", "mouse_event", "keybd_event", "WH_MOUSE_LL", "WH_KEYBOARD_LL"];
    let never = ["RegisterHotKey", "SetProcessDpiAwarenessContext", "SetProcessDPIAware", "ClipCursor", "ShowCursor", "BlockInput", "SetSystemCursor"];
    let allowed = ["remote.rs", "capture.rs", "hook.rs"];
    for file in desk_shipped_files() {
        let text = code_only(&read(&file));
        let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let in_windows = file.to_string_lossy().replace('\\', "/").contains("/os/windows/");
        for word in never {
            assert!(!text.contains(word), "{} uses {word}", file.display());
        }
        for word in only_there {
            if !(in_windows && allowed.contains(&name)) {
                assert!(!text.contains(word), "{} uses {word}: only the Windows host files may", file.display());
            }
        }
    }
}

/// Tests never build the real Windows host or call the real default host.
#[test]
fn desk_tests_use_only_the_mock_host() {
    for path in [vec!["tests.rs"], vec!["guard_tests.rs"], vec!["link", "tests.rs"]] {
        let name = path.join("/");
        let mut full = tauri_src().join("desk");
        for part in &path {
            full = full.join(part);
        }
        let text = read(&full);
        let code = code_only(&text);
        let real = ["Windows", "Host::new"].concat();
        let real2 = ["Windows", "Remote"].concat();
        assert!(!code_only(&text).contains(&real2), "{name} builds the real remote host");
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

/// The capture hooks run on a thread of their own, installed only after it starts, and their callbacks never wait.
#[test]
fn the_capture_hooks_are_installed_only_on_their_own_named_thread() {
    let text = code_only(&read(&tauri_src().join("desk").join("os").join("windows").join("capture.rs")));
    assert_eq!(text.matches("SetWindowsHookExW(").count(), 2, "one mouse hook and one keyboard hook");
    let spawn = text.find("thread::Builder::new().name(\"desk-capture\"").expect("a named capture thread");
    assert!(spawn < text.find("SetWindowsHookExW(").unwrap());
    assert!(text.contains("GetMessageW"));
    let callbacks = &text[text.find("unsafe extern \"system\" fn mouse_proc").unwrap()..];
    assert!(!callbacks.contains(".lock()"), "capture callbacks use try_lock only");
    assert!(!callbacks.contains("recv") && !callbacks.contains("sleep") && !callbacks.contains(".send("));
}

/// Files with behaviour taken from desktoplab say so on their first line.
#[test]
fn ported_files_carry_the_source_header() {
    let ported = [
        "hotkeys.rs", "hops.rs", "config.rs", "hook.rs", "dpi.rs", "cursor.rs", "display.rs", "host.rs", "remote.rs", "capture.rs", "clipboard.rs",
        "geometry.rs", "hub.rs", "io.rs", "wire.rs", "clip_link.rs",
    ];
    let mut found = 0;
    for file in desk_shipped_files() {
        let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if ported.contains(&name) {
            found += 1;
            assert!(read(&file).starts_with("// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT."), "{} lacks the header", file.display());
        }
    }
    assert!(found >= ported.len(), "only {found} ported files found");
}
