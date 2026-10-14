//! Hook Lane v0 tests (§8): projection of real captured payloads, the ledger state machine, the
//! shadow record, token scope through the real bridge gate, the byte-stable Codex entries, the
//! project-file writer, Cursor isolation, redaction and the "hooks never act" architecture rule.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use super::*;
use crate::agent_hooks::shadow::{Check, Shadow, Tally};
use crate::tui::TuiProfile;

/// Tests that change the process-wide hook settings run one at a time and restore the default.
static SETTINGS_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-hooks-{name}-{}-{}", std::process::id(), uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/agent_hooks/fixtures")
}

fn fixture(agent: &str, version: &str, name: &str) -> Value {
    let path = fixtures_dir().join(agent).join(version).join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).unwrap()
}

fn scheme(agent: &str) -> Arc<HooksTable> {
    scheme_for(agent).unwrap_or_else(|| panic!("{agent} declares hooks"))
}

fn pane() -> String {
    format!("test-pane-{}", uuid::Uuid::new_v4())
}

// ---- 8.1 fixtures project through the shipped tables ----------------------------------------

#[test]
fn every_captured_fixture_projects_with_its_required_fields() {
    let mut checked = 0;
    for agent_dir in std::fs::read_dir(fixtures_dir()).unwrap() {
        let agent_dir = agent_dir.unwrap().path();
        let agent = agent_dir.file_name().unwrap().to_string_lossy().into_owned();
        let table = scheme(&agent);
        for version in std::fs::read_dir(&agent_dir).unwrap() {
            for file in std::fs::read_dir(version.unwrap().path()).unwrap() {
                let path = file.unwrap().path();
                let payload: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
                let vendor = payload["hook_event_name"].as_str().unwrap().to_string();
                let Some(row) = table.row_for(&vendor) else {
                    continue; // captured but not subscribed (e.g. Codex SessionEnd)
                };
                let event = ingest::project(&table, &payload, "p", &agent, "D:\\pm-demo", 1, 1)
                    .unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
                let required = crate::tui::declarative::hooks::PM_EVENTS.iter().find(|(k, _, _)| *k == row.pm).unwrap().1;
                for field in required {
                    let present = match *field {
                        "session" => event.session.is_some(),
                        "tool" => event.detail.tool.is_some(),
                        "kind" => event.detail.kind.is_some(),
                        _ => true,
                    };
                    assert!(present, "{} {vendor} has no {field}", path.display());
                }
                if matches!(vendor.as_str(), "UserPromptSubmit" | "Stop" | "PermissionRequest" | "PostToolUse" | "Interrupt") {
                    assert!(event.turn_key.is_some(), "{} {vendor} has no turn key", path.display());
                }
                checked += 1;
            }
        }
    }
    assert!(checked >= 14, "only {checked} fixtures checked");
}

#[test]
fn projection_keeps_ids_hashes_and_paths_never_the_prompt_text() {
    let table = scheme("claude");
    let prompt = fixture("claude", "2.1.299", "UserPromptSubmit");
    let event = ingest::project(&table, &prompt, "p", "claude", "D:\\pm-demo", 1, 1).unwrap();
    assert_eq!(event.kind, model::HookKind::PromptSubmitted);
    assert_eq!(event.turn_key.as_deref(), Some("e0ab09c1-858f-449d-a4da-7e82f9249f44"));
    let text = prompt["prompt"].as_str().unwrap();
    assert_eq!(event.detail.prompt_sha.as_deref(), Some(ingest::prompt_sha(text).as_str()));
    let serialized = serde_json::to_string(&event).unwrap();
    assert!(!serialized.contains("hook-probe.txt containing"), "prompt text never serialized: {serialized}");

    let write = fixture("claude", "2.1.299", "PermissionRequest-Write");
    let event = ingest::project(&table, &write, "p", "claude", "D:\\pm-demo", 2, 2).unwrap();
    assert_eq!(event.detail.tool.as_deref(), Some("Write"));
    assert_eq!(event.detail.paths, vec!["D:\\pm-demo\\hook-probe.txt".to_string()]);
    let outside = ingest::project(&table, &write, "p", "claude", "C:\\elsewhere", 2, 2).unwrap();
    assert_eq!(outside.detail.paths, vec!["hook-probe.txt (outside the project)".to_string()]);

    let stop = fixture("claude", "2.1.299", "Stop");
    let event = ingest::project(&table, &stop, "p", "claude", "D:\\pm-demo", 3, 3).unwrap();
    assert_eq!(event.detail.final_text.as_ref().map(|t| t.as_str().to_string()).as_deref(), Some("v22.20.0"));
    assert!(!serde_json::to_string(&event).unwrap().contains("v22.20.0"), "reply text stays in memory only");
}

#[test]
fn a_pasted_content_wrapper_does_not_change_the_prompt_hash() {
    let plain = "Fix the bug\nin sum.js";
    let wrapped = "<pasted_content id=\"1\">\r\nFix the bug\r\n  in sum.js\r\n</pasted_content>";
    assert_eq!(ingest::prompt_sha(plain), ingest::prompt_sha(wrapped));
}

// ---- 4.6 the ledger, replayed from real captures -------------------------------------------

#[test]
fn a_claude_turn_replayed_from_captures_opens_suspends_and_closes_and_subagents_change_nothing() {
    let table = scheme("claude");
    let p = pane();
    let step = |name: &str, t: u64| ledger::append(&table, &fixture("claude", "2.1.299", name), &p, "claude", "D:\\pm-demo", t).unwrap();
    step("SessionStart", 1);
    let f = ledger::facts(&p).unwrap();
    assert!(f.connected);
    assert_eq!(f.turn_in_flight, Some(false));
    step("UserPromptSubmit", 2);
    assert_eq!(ledger::facts(&p).unwrap().turn_in_flight, Some(true));
    step("PermissionRequest-Write", 3);
    assert_eq!(ledger::facts(&p).unwrap().pending_permissions, 1);
    step("PostToolUse", 4);
    let f = ledger::facts(&p).unwrap();
    assert_eq!(f.pending_permissions, 0);
    assert_eq!(f.attributed_paths, vec!["D:\\pm-demo\\hook-probe.txt".to_string()]);
    let (_, applied) = step("Stop", 5);
    assert!(applied);
    assert_eq!(ledger::facts(&p).unwrap().turn_in_flight, Some(false));
    // Live: SubagentStop arrives AFTER the Stop with the same prompt_id. It must not reopen anything.
    let (_, applied) = step("SubagentStop", 6);
    assert!(!applied);
    let f = ledger::facts(&p).unwrap();
    assert_eq!(f.turn_in_flight, Some(false));
    assert_eq!(f.counts.subagent, 1);
    assert_eq!(ledger::last_final_text(&p).unwrap().as_str(), "v22.20.0");
}

#[test]
fn codex_esc_sends_stop_then_interrupt_and_the_interrupt_is_late_not_a_new_turn() {
    let table = scheme("codex");
    let p = pane();
    for (i, name) in ["UserPromptSubmit-2", "Stop-interrupted", "Interrupt"].iter().enumerate() {
        ledger::append(&table, &fixture("codex", "0.162.1", name), &p, "codex", "D:\\pm-scratch-hook\\cx1", i as u64).unwrap();
    }
    let f = ledger::facts(&p).unwrap();
    assert_eq!(f.turn_in_flight, Some(false));
    assert_eq!(f.counts.late, 1);
}

#[test]
fn events_from_another_session_are_foreign_and_change_nothing() {
    let table = scheme("claude");
    let p = pane();
    ledger::append(&table, &fixture("claude", "2.1.299", "SessionStart"), &p, "claude", "D:\\pm-demo", 1).unwrap();
    let mut other = fixture("claude", "2.1.299", "UserPromptSubmit");
    other["session_id"] = json!("another-session");
    let (event, applied) = ledger::append(&table, &other, &p, "claude", "D:\\pm-demo", 2).unwrap();
    assert!(event.foreign && !applied);
    assert_eq!(ledger::facts(&p).unwrap().turn_in_flight, Some(false));
}

// ---- 1.3 / 6: the shadow record, from real captured payloads -------------------------------

fn read_jsonl(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path).unwrap_or_default().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

#[test]
fn shadow_records_real_hook_events_beside_the_screen_and_scores_agreement() {
    let dir = temp("shadow");
    let mut shadow = Shadow::new(Some(dir.clone()));
    let table = scheme("claude");
    let p = pane();
    let hook = |shadow: &mut Shadow, name: &str, t: u64| {
        let (event, applied) = ledger::append(&table, &fixture("claude", "2.1.299", name), &p, "claude", "D:\\pm-demo", t).unwrap();
        shadow.observe_hook(&event, applied, t);
    };
    // The live order of the captured turn, with the screen's verdicts around it.
    shadow.observe_screen(&p, "idle", 0);
    hook(&mut shadow, "SessionStart", 1_000);
    shadow.observe_screen(&p, "idle", 1_200);
    hook(&mut shadow, "UserPromptSubmit", 10_000);
    shadow.observe_screen(&p, "running", 10_300);
    hook(&mut shadow, "PermissionRequest-Write", 12_700);
    shadow.observe_screen(&p, "waiting_input", 13_100);
    shadow.observe_screen(&p, "running", 50_000);
    hook(&mut shadow, "PostToolUse", 52_400);
    hook(&mut shadow, "Stop", 68_400);
    hook(&mut shadow, "SubagentStop", 70_600);
    shadow.observe_screen(&p, "idle", 72_000);
    // A repaint blip with no hook behind it (VERIFY #9): the screen alone claims a new turn.
    shadow.observe_screen(&p, "running", 90_000);
    shadow.observe_screen(&p, "idle", 90_800);
    shadow.sweep(200_000);

    let t = |check| shadow.tallies().into_iter().find(|(a, c, _)| a == "claude" && *c == check).map(|(_, _, t)| t).unwrap_or_default();
    assert_eq!(t(Check::TurnStart), Tally { agree: 1, screen_only: 1, hook_only: 0 });
    assert_eq!(t(Check::Waiting), Tally { agree: 1, screen_only: 0, hook_only: 0 });
    assert_eq!(t(Check::TurnEnd), Tally { agree: 1, screen_only: 1, hook_only: 0 });

    let records = read_jsonl(&shadow.file_for(&p).unwrap());
    let stop = records.iter().find(|r| r["lane"] == "hook" && r["kind"] == "turn_end").expect("the Stop is recorded");
    assert_eq!(stop["screen"], "running", "side by side: what the screen said when the hook arrived");
    let end = records.iter().find(|r| r["check"] == "turn_end" && r["verdict"] == "agree").expect("verdict");
    assert_eq!(end["screen_minus_hook_ms"], 3_600);
    let blip = records.iter().find(|r| r["check"] == "turn_start" && r["verdict"] == "screen_only").expect("blip");
    assert_eq!(blip["screen_at"], 90_000);
    let all = std::fs::read_to_string(shadow.file_for(&p).unwrap()).unwrap();
    for secret in ["hook-probe.txt containing", "v22.20.0", "\"hi\\n\""] {
        assert!(!all.contains(secret), "no prompt, reply or file content in the shadow file ({secret})");
    }
    std::fs::copy(shadow.file_for(&p).unwrap(), dir.join("evidence-copy.jsonl")).unwrap();
}

#[test]
fn a_pane_whose_hooks_never_reported_is_not_compared() {
    let mut shadow = Shadow::new(None);
    let p = pane();
    shadow.observe_screen(&p, "idle", 0);
    shadow.observe_screen(&p, "running", 1_000);
    shadow.observe_screen(&p, "idle", 5_000);
    shadow.sweep(100_000);
    assert!(shadow.tallies().is_empty());
}

// ---- 4.2 the route: token, scope, local only ------------------------------------------------

fn hook_headers(pane: &str, token: &str, agent: &str) -> String {
    format!("POST /hooks/event HTTP/1.1\r\nHost: 127.0.0.1:17321\r\nX-PM-Hook-Token: {token}\r\nX-PM-Hook-Pane: {pane}\r\nX-PM-Hook-Agent: {agent}\r\nContent-Type: application/json")
}

#[test]
fn the_route_takes_a_live_token_for_its_own_pane_and_agent_only() {
    let _guard = SETTINGS_LOCK.lock();
    let p = pane();
    let token = token::mint(&p, "claude", "D:\\pm-demo");
    let body = serde_json::to_vec(&fixture("claude", "2.1.299", "SessionStart")).unwrap();
    let ok = route::handle_at(&hook_headers(&p, &token, "claude"), &body, true, 1);
    assert_eq!(ok, (202, json!({})), "append-only: nothing comes back");
    assert!(ledger::facts(&p).unwrap().connected);

    assert_eq!(route::handle_at(&hook_headers(&p, "wrong-token", "claude"), &body, true, 2).1["code"], "HOOK_TOKEN_INVALID");
    assert_eq!(route::handle_at(&hook_headers("other-pane", &token, "claude"), &body, true, 3).1["code"], "HOOK_TOKEN_INVALID");
    let mismatch = route::handle_at(&hook_headers(&p, &token, "codex"), &body, true, 4);
    assert_eq!((mismatch.0, mismatch.1["code"].clone()), (403, json!("HOOK_AGENT_MISMATCH")));
    let mixed = format!("{}\r\nAuthorization: Bearer x", hook_headers(&p, &token, "claude"));
    assert_eq!(route::handle_at(&mixed, &body, true, 5), (400, json!({"code": "HOOK_MIXED_CREDENTIALS"})));
    let session = format!("{}\r\nX-Puppet-Master-Session: s1", hook_headers(&p, &token, "claude"));
    assert_eq!(route::handle_at(&session, &body, true, 6).0, 400);
    assert_eq!(route::handle_at(&hook_headers(&p, &token, "claude"), &vec![b' '; route::MAX_BODY + 1], true, 7).0, 413);

    revoke(&p);
    assert_eq!(route::handle_at(&hook_headers(&p, &token, "claude"), &body, true, 8), (401, json!({"code": "HOOK_TOKEN_INVALID"})), "a revoked token is dead");
}

#[test]
fn a_phone_or_tunnel_caller_is_refused_even_with_a_valid_token() {
    let _guard = SETTINGS_LOCK.lock();
    let p = pane();
    let token = token::mint(&p, "claude", "D:\\pm-demo");
    let body = b"{}";
    // Not a loopback peer (a phone on the LAN).
    assert_eq!(route::handle_at(&hook_headers(&p, &token, "claude"), body, false, 1), (403, json!({"code": "HOOK_LOCAL_ONLY"})));
    // Loopback, but it came through the PWA proxy / a tunnel.
    let proxied = format!("{}\r\nX-PM-Proxied: 1", hook_headers(&p, &token, "claude"));
    assert_eq!(route::handle_at(&proxied, body, true, 2).0, 403);
    let foreign_host = hook_headers(&p, &token, "claude").replace("Host: 127.0.0.1:17321", "Host: my-tunnel.example");
    assert_eq!(route::handle_at(&foreign_host, body, true, 3).0, 403);
    revoke(&p);
}

#[test]
fn the_rate_limit_drops_a_burst() {
    let _guard = SETTINGS_LOCK.lock();
    let p = pane();
    let token = token::mint(&p, "claude", "D:\\pm-demo");
    let body = serde_json::to_vec(&fixture("claude", "2.1.299", "SessionStart")).unwrap();
    let statuses: Vec<u16> = (0..25).map(|i| route::handle_at(&hook_headers(&p, &token, "claude"), &body, true, 1_000 + i).0).collect();
    assert_eq!(statuses.iter().filter(|s| **s == 202).count(), token::BURST_PER_SEC);
    assert!(statuses.contains(&429));
    assert_eq!(route::handle_at(&hook_headers(&p, &token, "claude"), &body, true, 3_000).0, 202, "a second later it flows again");
    revoke(&p);
}

/// One raw request through the real bridge gate (`bridge::http::gate`) on a TCP socket.
fn send(addr: SocketAddr, request: &str) -> (u16, Value) {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status = text.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    let body = text.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

/// Every route the bridge serves, from the tool catalog plus the routes no tool names.
fn every_route() -> Vec<(String, String)> {
    let mut routes: Vec<(String, String)> = crate::tool_registry::tools()
        .iter()
        .map(|t| {
            let path = t.path.split('/').map(|s| if s.starts_with('{') || s.starts_with(':') { "x" } else { s }).collect::<Vec<_>>().join("/");
            (t.method.to_string(), path)
        })
        .collect();
    for (m, p) in [
        ("GET", "/"), ("GET", "/health"), ("GET", "/events"), ("GET", "/events/ws"), ("GET", "/events/poll"),
        ("GET", "/panes"), ("POST", "/panes/x/input"), ("POST", "/panes/x/key"), ("POST", "/pair"),
        ("POST", "/push/act"), ("GET", "/agents/definitions"), ("POST", "/agents/run"), ("GET", "/hooks/event"),
        ("PUT", "/hooks/event"), ("POST", "/hooks/other"), ("GET", "/bridge/panes"), ("POST", "/mcp/mode"),
        ("PATCH", "/settings"), ("POST", "/questions/x/answer"),
    ] {
        routes.push((m.to_string(), p.to_string()));
    }
    routes.sort();
    routes.dedup();
    routes
}

#[test]
fn the_hook_token_gets_403_on_every_other_route_through_the_real_gate() {
    let _guard = SETTINGS_LOCK.lock();
    let addr = crate::bridge::spawn_gate_listener();
    let p = pane();
    let token = token::mint(&p, "claude", "D:\\pm-demo");
    let routes = every_route();
    assert!(routes.len() > 40, "the route table is not vacuous: {}", routes.len());
    for (method, path) in &routes {
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nX-PM-Hook-Token: {token}\r\nX-PM-Hook-Pane: {p}\r\nX-PM-Hook-Agent: claude\r\nContent-Length: 2\r\n\r\n{{}}"
        );
        let (status, body) = send(addr, &request);
        assert_eq!((status, body["code"].as_str()), (403, Some("HOOK_TOKEN_SCOPE")), "{method} {path}");
    }
    // ... and its one route works, also under /bridge.
    let body = serde_json::to_string(&fixture("claude", "2.1.299", "SessionStart")).unwrap();
    for path in ["/hooks/event", "/bridge/hooks/event"] {
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nX-PM-Hook-Token: {token}\r\nX-PM-Hook-Pane: {p}\r\nX-PM-Hook-Agent: claude\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        assert_eq!(send(addr, &request), (202, json!({})), "{path}");
    }
    eprintln!("HOOK_TOKEN_SCOPE on {} routes", routes.len());
    revoke(&p);
}

#[test]
fn a_paired_phone_is_refused_on_the_hook_route_through_the_real_gate() {
    let _guard = SETTINGS_LOCK.lock();
    let addr = crate::bridge::spawn_gate_listener();
    let dir = temp("pairing");
    let _ = crate::mobile_pairing::init_pairing_store(dir.join("pairing.json"));
    let bearer = {
        let store = crate::mobile_pairing::pairing_store().expect("pairing store");
        let mut guard = store.lock();
        let session = guard.create_pairing_session_with_caps("https://example.invalid".into(), crate::mobile_pairing::Caps::all());
        guard
            .pair_device(crate::mobile_pairing::PairRequestBody {
                pairing_code: session.pairing_code,
                device_name: "hook test phone".into(),
                device_public_key: session.server_public_key,
            })
            .expect("pair")
            .device_token
    };
    let p = pane();
    let token = token::mint(&p, "claude", "D:\\pm-demo");
    let body = "{}";
    for extra in [format!("Authorization: Bearer {bearer}\r\nX-PM-Proxied: 1\r\n"), "X-PM-Proxied: 1\r\n".to_string()] {
        let request = format!(
            "POST /hooks/event HTTP/1.1\r\nHost: tunnel.example\r\n{extra}X-PM-Hook-Token: {token}\r\nX-PM-Hook-Pane: {p}\r\nX-PM-Hook-Agent: claude\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let (status, reply) = send(addr, &request);
        assert!(status == 403 || status == 401, "a phone never posts hook events: {status} {reply}");
        assert_ne!(status, 202);
    }
    assert!(ledger::facts(&p).is_none(), "nothing was appended");
    revoke(&p);
}

// ---- 8.4 byte-stable Codex entries -----------------------------------------------------------

const GOLDEN_EXE: &str = "C:\\Users\\user\\AppData\\Roaming\\com.puppetmaster.app\\hooks\\v1\\puppet-master-mcp.exe";

fn golden_text() -> String {
    let table = scheme("codex");
    let file = table.project_file.as_ref().unwrap();
    let doc = render::document(&table, &file.root_key, file.layout, GOLDEN_EXE, "codex");
    format!("{}\n", serde_json::to_string_pretty(&doc).unwrap())
}

#[test]
fn codex_entries_are_byte_for_byte_the_golden_file() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/agent_hooks/tests/golden/codex-hooks.json");
    let rendered = golden_text();
    if std::env::var("PM_UPDATE_GOLDEN").is_ok_and(|v| v == "1") {
        std::fs::write(&path, &rendered).unwrap();
    }
    let golden = std::fs::read_to_string(&path).expect("golden file");
    assert_eq!(
        rendered, golden,
        "this change makes every user re-trust Puppet Master's Codex hooks: update the golden file AND note it in CHANGELOG.md"
    );
    assert_eq!(golden_text(), rendered, "rendering is deterministic");
    let entry = render::entry(&scheme("codex"), GOLDEN_EXE, "codex");
    assert_eq!(entry["command"], format!("{GOLDEN_EXE} hook codex"), "no pane, port or project bytes in the command");
}

#[test]
fn a_path_with_spaces_is_quoted_and_the_rest_is_bare() {
    assert_eq!(render::exe_for_shell("C:\\a b\\x.exe"), "\"C:\\a b\\x.exe\"");
    assert_eq!(render::exe_for_shell("C:\\ab\\x.exe"), "C:\\ab\\x.exe");
}

// ---- 5.2 the project-file writer ------------------------------------------------------------

fn git(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap().status.success();
    assert!(ok, "git {args:?}");
}

#[test]
fn the_codex_file_is_created_excluded_idempotent_and_removed_again() {
    let project = temp("codex-project");
    git(&project, &["init", "-q"]);
    let hooks = temp("codex-hooksdir");
    let table = scheme("codex");
    let out = install::project::ensure(&table, GOLDEN_EXE, "codex", &project, &hooks);
    assert_eq!(out, install::project::Outcome::Written { created: true });
    let file = project.join(".codex/hooks.json");
    let written = std::fs::read_to_string(&file).unwrap();
    assert_eq!(written, golden_text(), "a fresh file is exactly the golden entries");
    let exclude = std::fs::read_to_string(project.join(".git/info/exclude")).unwrap();
    assert!(exclude.contains("/.codex/hooks.json"), "{exclude}");
    let mtime = std::fs::metadata(&file).unwrap().modified().unwrap();
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(install::project::ensure(&table, GOLDEN_EXE, "codex", &project, &hooks), install::project::Outcome::Unchanged);
    assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), mtime, "a second run writes nothing");
    assert!(!project.join(".claude").exists(), "never a .claude file");

    let removed = install::project::uninstall_all(&hooks, |_| Some("hooks".into()));
    assert_eq!(removed, 1);
    assert!(!file.exists(), "PM created it, PM removes it");
    assert!(!std::fs::read_to_string(project.join(".git/info/exclude")).unwrap().contains("/.codex/hooks.json"));
}

#[test]
fn the_users_own_codex_hooks_keep_their_place_and_content() {
    let project = temp("codex-user");
    std::fs::create_dir_all(project.join(".codex")).unwrap();
    let user = json!({
        "hooks": {
            "Stop": [ { "hooks": [ { "type": "command", "command": "python lint.py", "timeout": 10 } ] } ],
            "PreToolUse": [ { "matcher": "shell", "hooks": [ { "type": "command", "command": "guard.sh" } ] } ]
        },
        "other": { "keep": true }
    });
    std::fs::write(project.join(".codex/hooks.json"), serde_json::to_string_pretty(&user).unwrap()).unwrap();
    let hooks = temp("codex-user-hooksdir");
    let table = scheme("codex");
    assert_eq!(install::project::ensure(&table, GOLDEN_EXE, "codex", &project, &hooks), install::project::Outcome::Written { created: false });
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(project.join(".codex/hooks.json")).unwrap()).unwrap();
    assert_eq!(doc["other"], json!({ "keep": true }));
    assert_eq!(doc["hooks"]["PreToolUse"], user["hooks"]["PreToolUse"]);
    assert_eq!(doc["hooks"]["Stop"][0], user["hooks"]["Stop"][0], "the user's group stays first (Codex trust is keyed by position)");
    assert!(render::is_pm_group(&doc["hooks"]["Stop"][1], "codex"));
    assert!(project.join(".codex/hooks.json.pm-bak").exists(), "a backup before PM's first change");
    // Removing PM's entries leaves the user's file as the user wrote it (content-wise).
    assert!(install::project::remove(&project, ".codex/hooks.json", "hooks", "codex", false));
    let after: Value = serde_json::from_str(&std::fs::read_to_string(project.join(".codex/hooks.json")).unwrap()).unwrap();
    assert_eq!(after, user);
}

#[test]
fn a_git_tracked_hooks_file_is_never_written() {
    let project = temp("codex-tracked");
    git(&project, &["init", "-q"]);
    std::fs::create_dir_all(project.join(".codex")).unwrap();
    std::fs::write(project.join(".codex/hooks.json"), "{\"hooks\":{}}\n").unwrap();
    git(&project, &["add", ".codex/hooks.json"]);
    let before = std::fs::read(project.join(".codex/hooks.json")).unwrap();
    let out = install::project::ensure(&scheme("codex"), GOLDEN_EXE, "codex", &project, &temp("tracked-hooksdir"));
    assert_eq!(out, install::project::Outcome::BlockedTracked);
    assert_eq!(std::fs::read(project.join(".codex/hooks.json")).unwrap(), before);
}

#[test]
fn a_path_out_of_the_project_is_refused() {
    let project = temp("contain");
    assert!(install::project::contained(&project, "../x/hooks.json").is_none());
    assert!(install::project::contained(&project, ".codex/hooks.json").is_some());
}

// ---- 5.1 / 8.5: spawn, Claude's --settings file, Cursor isolation, mode off ------------------

/// A small stand-in for the shipped MCP exe (the copier copies bytes; it never runs them).
fn source_exe() -> PathBuf {
    let path = std::env::temp_dir().join(format!("pm-hooks-fake-mcp-{}.exe", std::process::id()));
    if !path.exists() {
        std::fs::write(&path, b"MZ fake puppet-master-mcp for tests").unwrap();
    }
    path
}

#[test]
fn claude_gets_a_settings_flag_to_a_file_in_pms_folder_and_a_token_in_env() {
    let _guard = SETTINGS_LOCK.lock();
    let project = temp("claude-project");
    let hooks = temp("claude-hooksdir");
    let p = pane();
    let (state, launch) = prepare_spawn_in("claude", &p, &project.to_string_lossy(), &[], &hooks, Some(&source_exe()), Some("127.0.0.1:17999".into()));
    assert_eq!(state, InstallState::Installed);
    assert_eq!(launch.extra_args[0], "--settings");
    let settings_path = PathBuf::from(&launch.extra_args[1]);
    assert!(settings_path.starts_with(&hooks), "the settings file lives in PM's hooks folder");
    let settings: Value = serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
    let stop = &settings["hooks"]["Stop"][0]["hooks"][0];
    assert_eq!(stop["args"], json!(["hook", "claude"]));
    assert_eq!(stop["async"], json!(true));
    assert!(stop["command"].as_str().unwrap().ends_with("puppet-master-mcp.exe") || stop["command"].as_str().unwrap().ends_with("puppet-master-mcp"));
    let env: std::collections::HashMap<_, _> = launch.env.iter().cloned().collect();
    assert_eq!(env["PM_HOOK_PANE"], p);
    assert_eq!(env["PM_HOOK_BRIDGE"], "127.0.0.1:17999");
    assert_eq!(env["PM_HOOK_TOKEN"].len(), 43);
    assert!(!project.join(".claude").exists(), "nothing written into the project");
    // A user who passes their own --settings keeps it: PM adds nothing.
    let (state, launch) = prepare_spawn_in("claude", &pane(), &project.to_string_lossy(), &["--settings".into(), "mine.json".into()], &hooks, Some(&source_exe()), Some("127.0.0.1:17999".into()));
    assert_eq!((state, launch), (InstallState::UserFlag, HookLaunch::default()));
    revoke(&p);
}

#[test]
fn enabling_hooks_for_every_profile_writes_no_file_any_other_agent_reads() {
    let _guard = SETTINGS_LOCK.lock();
    let project = temp("isolation");
    let hooks = temp("isolation-hooksdir");
    for descriptor in crate::tui::descriptors() {
        let (state, launch) = prepare_spawn_in(descriptor.id, &pane(), &project.to_string_lossy(), &[], &hooks, Some(&source_exe()), Some("127.0.0.1:17999".into()));
        if crate::tui::profile_for(descriptor.id).hook_scheme().is_none() {
            assert_eq!(state, InstallState::NotSupported, "{}", descriptor.id);
            assert!(launch.env.is_empty() && launch.extra_args.is_empty(), "{} gets no PM_HOOK_* env", descriptor.id);
        }
    }
    assert!(!project.join(".claude").exists(), "no .claude/settings*.json (Cursor imports them)");
    assert!(!project.join(".cursor").exists());
    let cursor = prepare_spawn_in("cursor_agent", &pane(), &project.to_string_lossy(), &[], &hooks, Some(&source_exe()), Some("127.0.0.1:1".into()));
    assert_eq!(cursor, (InstallState::NotSupported, HookLaunch::default()));
}

#[test]
fn mode_off_adds_nothing_and_evidence_runs_as_shadow_in_v0() {
    let _guard = SETTINGS_LOCK.lock();
    let hooks = temp("off-hooksdir");
    let project = temp("off-project");
    apply_settings(&json!({ "agent_hooks_mode": "off" }));
    let off = prepare_spawn_in("claude", &pane(), &project.to_string_lossy(), &[], &hooks, Some(&source_exe()), Some("127.0.0.1:1".into()));
    apply_settings(&json!({ "agent_hooks_mode": "evidence" }));
    let evidence = effective_mode("claude");
    apply_settings(&json!({ "agent_hooks_mode": "shadow", "agent_hooks_agents": { "codex": "off" } }));
    let codex_off = effective_mode("codex");
    let claude_on = effective_mode("claude");
    apply_settings(&json!({}));
    assert_eq!(off, (InstallState::Off, HookLaunch::default()));
    assert_eq!(evidence, Mode::Shadow, "evidence exists but stays off in v0");
    assert_eq!((codex_off, claude_on), (Mode::Off, Mode::Shadow));
    assert_eq!(effective_mode("claude"), DEFAULT_MODE);
    assert_eq!(DEFAULT_MODE, Mode::Shadow, "founder decision: shadow by default");
}

#[test]
fn the_frozen_exe_path_is_recorded_once_and_reused_byte_for_byte() {
    let hooks = temp("stable");
    let first = install::stable_exe::ensure(&source_exe(), &hooks).unwrap();
    let second = install::stable_exe::ensure(&source_exe(), &hooks).unwrap();
    assert_eq!(first, second);
    assert_eq!(install::stable_exe::recorded(&hooks).as_deref(), Some(first.as_str()));
    assert!(Path::new(&first).is_file());
    assert!(first.contains("v1"));
}

// ---- 4.3 redaction, 7.3 architecture -----------------------------------------------------------

#[test]
fn a_planted_secret_in_the_reply_or_command_never_survives_projection() {
    let secret = "pm-hook-planted-Secret-7731ab";
    crate::redact::known::register(secret);
    let table = scheme("claude");
    let mut stop = fixture("claude", "2.1.299", "Stop");
    stop["last_assistant_message"] = json!(format!("done, key is {secret}"));
    let event = ingest::project(&table, &stop, "p", "claude", "D:\\pm-demo", 1, 1).unwrap();
    assert!(!event.detail.final_text.unwrap().as_str().contains(secret));
    let mut ask = fixture("claude", "2.1.299", "PermissionRequest-Bash");
    ask["tool_input"]["command"] = json!(format!("curl -H 'x: {secret}' example.com"));
    let event = ingest::project(&table, &ask, "p", "claude", "D:\\pm-demo", 2, 2).unwrap();
    assert!(!event.detail.command.unwrap().as_str().contains(secret));
}

#[test]
fn the_hook_lane_cannot_reach_anything_that_types_answers_or_unlocks() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/agent_hooks");
    let forbidden = [
        "pane_prompt::answer", "answer_guard", "write_input", "registry_write", "PaneIo", "TypingPermit", "GatePermit",
        "WriteOrigin", "leases", "take_over", "send_keys", "press_key", "approve", "allow_once", "--dangerously",
    ];
    let mut files = Vec::new();
    let mut stack = vec![dir];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("tests/mod.rs") {
                files.push(path);
            }
        }
    }
    assert!(files.len() >= 10);
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        let code: String = text.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        for word in forbidden {
            assert!(!code.contains(word), "{} mentions `{word}`", file.display());
        }
    }
}

// ---- 5.5 doctor -----------------------------------------------------------------------------

#[test]
fn versions_are_checked_against_the_tested_range() {
    let v = |s| health::first_version(s).unwrap();
    assert_eq!(v("codex-cli 0.162.1"), vec![0, 162, 1]);
    assert_eq!(v("2.1.300 (Claude Code)"), vec![2, 1, 300]);
    assert!(health::in_range(&v("2.1.300"), ">=2.1.299, <=2.1.300"));
    assert!(!health::in_range(&v("2.1.301"), ">=2.1.299, <=2.1.300"));
    assert!(!health::in_range(&v("0.159.3"), ">=0.162.1, <=0.162.1"));
}

#[test]
fn the_doctor_row_uses_plain_words() {
    let _guard = SETTINGS_LOCK.lock();
    let codex = health::for_agent("codex", "Codex", Some("codex-cli 0.162.1")).unwrap();
    assert!(!codex.sentence.contains('_'), "{}", codex.sentence);
    let cursor = health::for_agent("cursor_agent", "Cursor", None).unwrap();
    assert_eq!(cursor.state, health::HookHealthState::NotSupported);
    let untested = health::for_agent("claude", "Claude Code", Some("9.9.9")).unwrap();
    assert_eq!(untested.state, health::HookHealthState::UntestedVersion);
    apply_settings(&json!({ "agent_hooks_mode": "off" }));
    let off = health::for_agent("claude", "Claude Code", Some("2.1.300")).unwrap();
    apply_settings(&json!({}));
    assert_eq!((off.state, off.sentence.as_str()), (health::HookHealthState::Off, "Hooks are off."));
}
