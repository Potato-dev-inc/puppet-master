//! Source guards for the link layer. They read the source files, so a change that breaks a rule fails here.

use std::path::{Path, PathBuf};

use regex::Regex;

fn tauri_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn packages() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

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

/// The p2p files that ship (not the tests, which are the checkers, and not the test-only net).
fn shipped() -> Vec<PathBuf> {
    files(&tauri_src().join("p2p"), &["rs"])
        .into_iter()
        .filter(|p| {
            let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            n != "tests.rs" && n != "guard_tests.rs" && n != "memory.rs" && n != "pair_tests.rs"
        })
        .collect()
}

/// Agents, MCP tools and the phone can never reach the link: no bridge route, MCP shim, tool registry, phone route,
/// channel or CLI may mention the module or the network crate.
#[test]
fn p2p_is_not_reachable_from_the_bridge_mcp_or_the_phone() {
    let pattern = Regex::new(r"\bp2p::|crate::p2p\b|\bmod p2p\b|\biroh\b|iroh::|P2p\b|\bpm/desk|\bpm/spaces|\bpm/files").unwrap();
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
                panic!("{} reaches the PC link ({:?}): agents, MCP and the phone must never open a link", file.display(), m.as_str());
            }
        }
    }
    assert!(seen > 50, "only {seen} files scanned");
}

/// The only users of the module are the module itself and the module list in lib.rs (until DL-6 / DL-7 add theirs,
/// each of which must add itself to the allowed list here, on purpose).
#[test]
fn only_known_files_use_the_p2p_module() {
    let allowed = ["lib.rs", "pairing_commands.rs"];
    let pattern = Regex::new(r"\bp2p::|crate::p2p\b|\bmod p2p\b").unwrap();
    for file in files(&tauri_src(), &["rs"]) {
        let rel = file.strip_prefix(tauri_src()).unwrap().to_string_lossy().replace('\\', "/");
        if rel.starts_with("p2p/") || allowed.contains(&rel.as_str()) {
            continue;
        }
        let text = code_only(&read(&file));
        assert!(!pattern.is_match(&text), "{rel} uses the p2p module: add it to the allowed list on purpose");
    }
}

/// The key never leaves memory: p2p code opens no file, reads no argument or environment, and prints nothing.
#[test]
fn p2p_code_writes_no_file_and_prints_nothing() {
    let banned = [
        "std::fs", "tokio::fs", "File::", "OpenOptions", "fs::write", "fs::read", "env::args", "env::var", "println!", "eprintln!",
        "dbg!", "tracing::", "info!(", "debug!(", "warn!(", "error!(", "Serialize", "serde_json::to_",
    ];
    for file in shipped() {
        let text = code_only(&read(&file));
        for word in banned {
            assert!(!text.contains(word), "{} uses {word}: the node key must never reach a file, a log or an argument", file.display());
        }
    }
}

/// The secret bytes come out of the identity in exactly one place and are only handed to the network crate.
#[test]
fn the_secret_is_asked_for_only_by_the_iroh_transport() {
    let mut users = Vec::new();
    for file in shipped() {
        let text = code_only(&read(&file));
        if text.contains("secret_bytes()") && !file.ends_with("identity.rs") {
            users.push(file.file_name().unwrap().to_string_lossy().to_string());
        }
    }
    assert_eq!(users, vec!["iroh_net.rs".to_string()]);
    let identity = code_only(&read(&tauri_src().join("p2p").join("identity.rs")));
    assert!(identity.contains("pub(super) fn secret_bytes"));
    assert!(!identity.contains("pub fn secret"));
}

/// The key comes from the team keystore helpers, not from a second implementation.
#[test]
fn identity_uses_the_existing_keystore() {
    let identity = code_only(&read(&tauri_src().join("p2p").join("identity.rs")));
    assert!(identity.contains("KeyStore") && identity.contains("device_sign()"));
    assert!(!identity.contains("keyring") && !identity.contains("SigningKey::generate") && !identity.contains("new_signing_key"));
}

/// Real-network code lives in one file, and no test binds it.
#[test]
fn only_one_file_touches_the_network_and_no_test_binds_it() {
    for file in shipped() {
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        let text = code_only(&read(&file));
        let net_words = ["iroh::", "tokio::net", "std::net::Tcp", "std::net::Udp", "UdpSocket", "TcpStream", "TcpListener", "reqwest", "ureq"];
        for word in net_words {
            if name != "iroh_net.rs" {
                assert!(!text.contains(word), "{name} uses {word}: only iroh_net.rs may touch the network");
            }
        }
    }
    for name in ["tests.rs", "guard_tests.rs", "memory.rs"] {
        let text = code_only(&read(&tauri_src().join("p2p").join(name)));
        let bad = ["Iroh", "iroh_net"].concat();
        let (a, b) = (bad.as_str(), "IrohFactory");
        if name != "guard_tests.rs" {
            assert!(!text.contains(b) && !text.contains(a), "{name} builds the real network");
        }
    }
}

/// The core is generic: no capability-specific words in the link rules.
#[test]
fn the_core_knows_no_agent_or_capability_behaviour() {
    for name in ["endpoint.rs", "iroh_net.rs", "memory.rs", "identity.rs"] {
        let text = code_only(&read(&tauri_src().join("p2p").join(name))).to_lowercase();
        for word in ["claude", "codex", "agent", "pane", "mouse", "keyboard", "clipboard", "desk"] {
            assert!(!text.contains(word), "{name} mentions {word}: capability behaviour belongs to the capability");
        }
    }
}

/// Nobody but the desktop window answers the "wants to pair" sheet, shows a code or enters one: the pairing
/// manager, the trust list and their commands have no route from agents, MCP, the phone or the CLI.
#[test]
fn pairing_and_its_sheet_have_no_route_from_agents_mcp_or_the_phone() {
    let pattern = Regex::new(r"PairManager|pairing_commands|pair_answer|pair_join|pair_host_start|trust_set_permission|trust_remove|TrustList|p2p::pairing|p2p::trust").unwrap();
    let src = tauri_src();
    let pkgs = packages();
    let roots = [
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
    ];
    let mut seen = 0;
    for root in roots.iter().filter(|r| r.exists()) {
        for file in files(root, &["rs", "ts", "tsx", "js", "mjs", "cjs"]) {
            seen += 1;
            let text = code_only(&read(&file));
            if let Some(m) = pattern.find(&text) {
                panic!("{} reaches pairing ({:?}): only the desktop window may answer the sheet", file.display(), m.as_str());
            }
        }
    }
    assert!(seen > 50, "only {seen} files scanned");
}

/// The answer to the sheet has exactly one caller: the window's command. And the pairing code never takes a
/// permission from a peer: nothing in pairing.rs calls `Permissions::set` or `set_permission` for a link's peer.
#[test]
fn the_sheet_is_answered_in_one_place_and_a_peer_never_sets_permissions() {
    let mut callers = Vec::new();
    for file in files(&tauri_src(), &["rs"]) {
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        if name.ends_with("tests.rs") {
            continue;
        }
        let text = code_only(&read(&file));
        if text.contains(".answer(") && !name.starts_with("pairing") && name != "pairing_commands.rs" {
            continue;
        }
        if text.contains("mgr.answer(") {
            callers.push(name);
        }
    }
    assert_eq!(callers, vec!["pairing_commands.rs".to_string()]);
    let pairing = code_only(&read(&tauri_src().join("p2p").join("pairing.rs")));
    assert!(!pairing.contains(".set(&") && !pairing.contains("Permissions"), "pairing must not hand out rights");
    // Nothing turns a switch on by itself: the only writer of a permission is `set_permission`, called from the manager's
    // person-facing method and the command.
    assert_eq!(pairing.matches("set_permission(").count(), 2); // the manager method + its call into the trust list
}
