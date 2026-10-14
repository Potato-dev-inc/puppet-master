//! Fail-open tests of the real `puppet-master-mcp hook <agent>` process (Hook Lane §8.3).
//!
//! Every case runs the built binary as a child process, exactly as Claude Code / Codex would:
//! event JSON on stdin, env from the pane. Whatever the bridge does (absent, black hole, errors),
//! the emitter must exit 0 with empty stdout and stderr within its 300 ms budget, so the agent
//! never notices Puppet Master. Wall times include process start-up on this machine.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const EXE: &str = env!("CARGO_BIN_EXE_puppet-master-mcp");
/// The emitter's own budget is 300 ms; this allows for spawning a process on a busy test machine.
const LIMIT: Duration = Duration::from_millis(900);
const FIXTURE: &str = include_str!("../src/agent_hooks/fixtures/claude/2.1.299/Stop.json");

struct Outcome {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    took: Duration,
}

/// The first start of a freshly built exe can include a one-off antivirus scan (seen: ~1 s on
/// Windows). Agents run the same exe again and again, so time the steady state.
fn warm_up() {
    static WARM: std::sync::Once = std::sync::Once::new();
    WARM.call_once(|| {
        let _ = Command::new(EXE).args(["hook", "--protocol"]).output();
    });
}

fn run(env: &[(&str, &str)], stdin: &[u8]) -> Outcome {
    warm_up();
    let dir = std::env::temp_dir().join(format!("pm-hook-emitter-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let mut cmd = Command::new(EXE);
    cmd.args(["hook", "claude"])
        .env_remove("PM_HOOK_TOKEN")
        .env_remove("PM_HOOK_PANE")
        .env_remove("PM_HOOK_BRIDGE")
        .env_remove("PM_HOOKS_OFF")
        .env("PUPPET_MASTER_HOOKS_DIR", &dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let started = Instant::now();
    let mut child = cmd.spawn().expect("spawn emitter");
    let mut input = child.stdin.take().unwrap();
    let data = stdin.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = input.write_all(&data);
    });
    let out = child.wait_with_output().expect("wait");
    let took = started.elapsed();
    let _ = writer.join();
    Outcome { code: out.status.code(), stdout: out.stdout, stderr: out.stderr, took }
}

fn assert_silent_success(o: &Outcome, case: &str) {
    assert_eq!(o.code, Some(0), "{case}: exit code");
    assert!(o.stdout.is_empty(), "{case}: stdout {:?}", String::from_utf8_lossy(&o.stdout));
    assert!(o.stderr.is_empty(), "{case}: stderr {:?}", String::from_utf8_lossy(&o.stderr));
    assert!(o.took < LIMIT, "{case}: took {:?}", o.took);
    eprintln!("{case}: exit 0, silent, {:?}", o.took);
}

/// A listener that answers each request with `status` and hands the raw request to the test.
fn bridge(status: Option<u16>) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut stream = stream;
                let Some(status) = status else {
                    // Black hole: accept, never read, never answer.
                    std::thread::sleep(Duration::from_secs(5));
                    return;
                };
                let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                let mut raw = Vec::new();
                let mut buf = [0u8; 65536];
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            raw.extend_from_slice(&buf[..n]);
                            let text = String::from_utf8_lossy(&raw);
                            if let Some((head, body)) = text.split_once("\r\n\r\n") {
                                let len = head
                                    .lines()
                                    .find_map(|l| l.strip_prefix("Content-Length: "))
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                                    .unwrap_or(0);
                                if body.len() >= len {
                                    break;
                                }
                            }
                        }
                    }
                }
                let _ = write!(stream, "HTTP/1.1 {status} X\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}");
                let _ = tx.send(String::from_utf8_lossy(&raw).into_owned());
            });
        }
    });
    (addr, rx)
}

fn pane_env(addr: &str) -> Vec<(&'static str, String)> {
    vec![("PM_HOOK_TOKEN", "t".repeat(43)), ("PM_HOOK_PANE", "pane-1".into()), ("PM_HOOK_BRIDGE", addr.to_string())]
}

fn as_refs<'a>(v: &'a [(&'static str, String)]) -> Vec<(&'static str, &'a str)> {
    v.iter().map(|(k, s)| (*k, s.as_str())).collect()
}

#[test]
fn no_env_exits_zero_silently_and_fast() {
    let o = run(&[], FIXTURE.as_bytes());
    assert_silent_success(&o, "no env");
}

#[test]
fn a_closed_port_means_pm_is_down_and_the_agent_never_notices() {
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let env = pane_env(&format!("127.0.0.1:{port}"));
    let o = run(&as_refs(&env), FIXTURE.as_bytes());
    assert_silent_success(&o, "PM down (closed port)");
}

#[test]
fn a_black_hole_bridge_is_cut_off_by_the_watchdog() {
    let (addr, _rx) = bridge(None);
    let env = pane_env(&addr);
    let o = run(&as_refs(&env), FIXTURE.as_bytes());
    assert_silent_success(&o, "black hole");
}

#[test]
fn every_error_status_still_exits_zero_silently() {
    for status in [500u16, 401, 403, 429, 413] {
        let (addr, rx) = bridge(Some(status));
        let env = pane_env(&addr);
        let o = run(&as_refs(&env), FIXTURE.as_bytes());
        assert_silent_success(&o, &format!("bridge {status}"));
        assert!(rx.recv_timeout(Duration::from_secs(2)).is_ok(), "the request reached the bridge");
    }
}

#[test]
fn the_payload_and_only_the_hook_headers_reach_the_bridge() {
    let (addr, rx) = bridge(Some(202));
    let env = pane_env(&addr);
    let o = run(&as_refs(&env), FIXTURE.as_bytes());
    assert_silent_success(&o, "forward");
    let raw = rx.recv_timeout(Duration::from_secs(2)).expect("request");
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("POST /hooks/event HTTP/1.1\r\n"), "{head}");
    assert!(head.contains("X-PM-Hook-Token: ttttttttttttttttttttttttttttttttttttttttttt\r\n"));
    assert!(head.contains("X-PM-Hook-Pane: pane-1\r\n"));
    assert!(head.contains("X-PM-Hook-Agent: claude\r\n"));
    assert!(!head.contains("Authorization"), "never a bearer");
    assert!(!head.contains("X-Puppet-Master-Session"), "never an MCP session");
    assert!(!head.contains("X-PM-Local-Secret"), "the local secret never reaches a hook");
    let sent: serde_json::Value = serde_json::from_str(body).unwrap();
    let fixture: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(sent, fixture);
}

#[test]
fn invalid_json_is_sent_as_a_marker_never_as_text() {
    let (addr, rx) = bridge(Some(202));
    let env = pane_env(&addr);
    let o = run(&as_refs(&env), b"not json at all, maybe a secret sk-ant-xyz");
    assert_silent_success(&o, "invalid json");
    let raw = rx.recv_timeout(Duration::from_secs(2)).expect("request");
    let body = raw.split_once("\r\n\r\n").unwrap().1;
    assert!(!body.contains("sk-ant") && !body.contains("not json"), "{body}");
    let sent: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(sent, serde_json::json!({ "unparsed": true, "bytes": 42 }));
}

#[test]
fn fifty_megabytes_of_stdin_never_block_the_writer() {
    let (addr, _rx) = bridge(Some(202));
    let env = pane_env(&addr);
    let big = vec![b'x'; 50 * 1024 * 1024];
    let o = run(&as_refs(&env), &big);
    assert_silent_success(&o, "50 MB stdin");
}

#[test]
fn the_kill_switch_exits_before_any_connection() {
    let (addr, rx) = bridge(Some(202));
    let mut env = pane_env(&addr);
    env.push(("PM_HOOKS_OFF", "1".into()));
    let o = run(&as_refs(&env), FIXTURE.as_bytes());
    assert_silent_success(&o, "kill switch");
    assert!(rx.recv_timeout(Duration::from_millis(500)).is_err(), "no request with the kill switch on");
}

#[test]
fn a_non_loopback_bridge_address_is_never_contacted() {
    let env = pane_env("10.255.255.1:80");
    let o = run(&as_refs(&env), FIXTURE.as_bytes());
    assert_silent_success(&o, "non-loopback bridge");
}

#[test]
fn protocol_flag_prints_the_emitter_contract_version() {
    let out = Command::new(EXE).args(["hook", "--protocol"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "1");
}

#[test]
fn big_strings_and_deep_nesting_are_shrunk_before_sending() {
    let (addr, rx) = bridge(Some(202));
    let env = pane_env(&addr);
    let mut deep = serde_json::json!("leaf");
    for _ in 0..100 {
        deep = serde_json::json!({ "d": deep });
    }
    let payload = serde_json::json!({
        "hook_event_name": "Stop",
        "last_assistant_message": "y".repeat(700 * 1024),
        "a": "z".repeat(200 * 1024),
        "deep": deep,
    });
    let o = run(&as_refs(&env), payload.to_string().as_bytes());
    assert_silent_success(&o, "shrink");
    let raw = rx.recv_timeout(Duration::from_secs(2)).expect("request");
    let body = raw.split_once("\r\n\r\n").unwrap().1;
    assert!(body.len() <= 512 * 1024, "body is {} bytes", body.len());
    let sent: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(sent["hook_event_name"], "Stop");
    assert!(sent["last_assistant_message"].as_str().unwrap().contains("…[cut "), "cut strings are marked");
}

/// Live finding (Claude Code 2.1.300 at startup): the event JSON arrives at once but stdin is closed
/// only ~300-800 ms later. The emitter must post as soon as the JSON is complete, not wait for EOF.
#[test]
fn a_complete_event_is_posted_even_while_the_agent_keeps_stdin_open() {
    warm_up();
    let (addr, rx) = bridge(Some(202));
    let dir = std::env::temp_dir().join(format!("pm-hook-emitter-open-{}", std::process::id()));
    let started = Instant::now();
    let mut child = Command::new(EXE)
        .args(["hook", "claude"])
        .env("PM_HOOK_TOKEN", "t".repeat(43))
        .env("PM_HOOK_PANE", "pane-1")
        .env("PM_HOOK_BRIDGE", &addr)
        .env("PUPPET_MASTER_HOOKS_DIR", &dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all(FIXTURE.as_bytes()).unwrap();
    input.flush().unwrap();
    // Keep stdin open well past the budget, like the agent does.
    let holder = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        drop(input);
    });
    let out = child.wait_with_output().unwrap();
    let took = started.elapsed();
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    assert!(took < LIMIT, "took {took:?}");
    let raw = rx.recv_timeout(Duration::from_secs(2)).expect("the event reached the bridge before EOF");
    assert!(raw.contains("\"hook_event_name\":\"Stop\""));
    let _ = holder.join();
    eprintln!("stdin held open: posted and exited 0 in {took:?}");
}
