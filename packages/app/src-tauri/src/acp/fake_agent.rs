//! A fake ACP agent for tests. No real agent is ever started by a test.
//!
//! The unit-test executable starts itself again as the agent: the child runs the one test
//! `tests::child_entry` with `PM_FAKE_ACP_ROLE=agent` in its environment, which turns that test into an
//! agent loop on stdin/stdout. (The test harness prints a header line to stdout first; the transport
//! ignores lines that are not JSON-RPC, which is also something worth testing.)
//!
//! What it does is chosen by the first word of the prompt text:
//!
//! | prompt | behaviour |
//! |---|---|
//! | `stream N` | N text chunks, then `end_turn` |
//! | `slow` | a chunk every 25 ms for 10 s; ends with `cancelled` when asked to stop |
//! | `tool` | a tool call that starts and completes, then text |
//! | `perm` | a tool call that needs permission; waits for the answer (no timeout) |
//! | `crash` | one chunk, a line on stderr, then the process exits with code 3 |
//! | `quiet` | says nothing until cancelled |
//! | `hang` | says nothing and ignores cancel |
//! | `flood N` | N chunks as fast as it can |
//! | `fsreq` | stray response with an unknown id, then asks the client to read a file using the SAME id as the prompt |
//! | `rich` | text, a file edit (diff, path), a plan, a usage update, a thought, a command, text; `end_turn` |
//! | `crashtool` | a tool call that starts, then the process exits with code 3 (a tool left open) |
//! | `toolhang` | a tool call that starts and stays in progress until cancelled; then `cancelled` |
//! | `garbage` | a line of bytes that are not UTF-8, a line of plain text, broken JSON, then real chunks and `end_turn` |
//! | `bigdiff N` | one edit call that carries N diffs of 20 KB each, then `end_turn` |
//! | `permcrash` | a tool that needs permission, then the process exits (the question is left open) |
//! | `empty` | `end_turn` with no output |
//! | `weird` | an update of a kind nobody knows, then text |
//! | anything else | echoes the text |
//!
//! `_session/steering` is answered `injected` while a prompt runs, else `promptRequired` (the option `steer=1` advertises it).
//!
//! Options (`PM_FAKE_ACP=load=0;resume=1;lose=1;auth=1;deaf_ms=3000;deaf_after_new_ms=5000;fail_init_once=1;grandchild=1`) change
//! what the handshake and session calls do. `PM_FAKE_ACP_STORE` is a folder where sessions are remembered
//! across processes, like an agent's own session files.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::config::{AgentConfig, PinPolicy, Timeouts};

pub const ROLE_ENV: &str = "PM_FAKE_ACP_ROLE";
pub const OPTS_ENV: &str = "PM_FAKE_ACP";
pub const STORE_ENV: &str = "PM_FAKE_ACP_STORE";

const HARNESS_ARGS: [&str; 4] = ["--exact", "acp::fake_agent::tests::child_entry", "--nocapture", "--test-threads=1"];

/// A config that starts the fake agent.
pub fn agent_config(opts: &str, store: &Path) -> AgentConfig {
    AgentConfig {
        id: "fake".into(),
        label: "Fake agent".into(),
        command: std::env::current_exe().expect("test exe").to_string_lossy().into_owned(),
        args: HARNESS_ARGS.iter().map(|a| a.to_string()).collect(),
        env: [
            (ROLE_ENV.to_string(), "agent".to_string()),
            (OPTS_ENV.to_string(), opts.to_string()),
            (STORE_ENV.to_string(), store.to_string_lossy().into_owned()),
        ]
        .into_iter()
        .collect(),
        pin_policy: PinPolicy::Off,
        timeouts: Timeouts { request_secs: 20, new_session_secs: 20, load_secs: 20, stdin_write_secs: 2, ..Timeouts::default() },
        ..AgentConfig::default()
    }
}

#[derive(Default, Clone)]
struct Opts {
    values: HashMap<String, String>,
}

impl Opts {
    fn parse(text: &str) -> Self {
        let values = text
            .split(';')
            .filter_map(|pair| pair.split_once('='))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect();
        Opts { values }
    }
    fn flag(&self, name: &str, default: bool) -> bool {
        self.values.get(name).map(|v| v == "1").unwrap_or(default)
    }
    /// The sign-in is gone while the file named by `auth_file` exists (a test creates it to expire the login).
    fn signed_out(&self) -> bool {
        self.flag("auth", false) || self.values.get("auth_file").is_some_and(|p| std::path::Path::new(p).exists())
    }
    fn number(&self, name: &str) -> u64 {
        self.values.get(name).and_then(|v| v.parse().ok()).unwrap_or(0)
    }
}

struct Shared {
    out: Mutex<std::io::Stdout>,
    cancel: AtomicBool,
    /// A prompt is being answered (for `_session/steering`).
    running: AtomicBool,
    responses: Mutex<HashMap<u64, mpsc::Sender<Value>>>,
    next_request: AtomicU64,
    opts: Opts,
    store: PathBuf,
}

impl Shared {
    fn send(&self, value: Value) {
        let mut out = self.out.lock();
        let _ = writeln!(out, "{value}");
        let _ = out.flush();
    }

    fn update(&self, session_id: &str, update: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": session_id, "update": update}}));
    }

    fn chunk(&self, session_id: &str, text: &str) {
        self.update(session_id, json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}));
    }

    fn reply(&self, id: &Value, result: Value) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "result": result}));
    }

    fn fail(&self, id: &Value, code: i64, message: &str) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}));
    }

    /// Sends a request to the client and waits (forever) for its answer.
    fn ask(&self, id: u64, method: &str, params: Value) -> mpsc::Receiver<Value> {
        let (tx, rx) = mpsc::channel();
        self.responses.lock().insert(id, tx);
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        rx
    }

    fn remember(&self, session_id: &str) {
        let _ = std::fs::create_dir_all(&self.store);
        let _ = std::fs::write(self.store.join(session_id), "x");
    }

    fn knows(&self, session_id: &str) -> bool {
        self.store.join(session_id).exists()
    }
}

fn config_options(current: &str) -> Value {
    json!([{
        "id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": current,
        "options": [{"value": "a", "name": "A"}, {"value": "b", "name": "B"}]
    }])
}

/// The agent loop. Returns when stdin closes.
pub fn run_agent() {
    let opts = Opts::parse(&std::env::var(OPTS_ENV).unwrap_or_default());
    let store = PathBuf::from(std::env::var(STORE_ENV).unwrap_or_default());
    let shared = Arc::new(Shared {
        out: Mutex::new(std::io::stdout()),
        cancel: AtomicBool::new(false),
        running: AtomicBool::new(false),
        responses: Mutex::new(HashMap::new()),
        next_request: AtomicU64::new(1000),
        opts,
        store,
    });
    // The test harness leaves "test ... " unterminated on stdout; start on a fresh line.
    {
        let mut out = shared.out.lock();
        let _ = writeln!(out);
        let _ = out.flush();
    }
    if shared.opts.flag("fail_init_once", false) {
        let marker = shared.store.join("failed-once");
        if !marker.exists() {
            let _ = std::fs::create_dir_all(&shared.store);
            let _ = std::fs::write(&marker, "x");
            eprintln!("Error: failed to initialize sqlite state runtime: database is locked");
            std::process::exit(1);
        }
    }
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        let method = message.get("method").and_then(Value::as_str).map(str::to_string);
        let id = message.get("id").cloned();
        match (method, id) {
            (Some(method), Some(id)) => handle_request(&shared, &method, &id, message.get("params").cloned().unwrap_or(Value::Null)),
            (Some(method), None) => {
                if method == "session/cancel" && !shared.opts.flag("ignore_cancel", false) {
                    shared.cancel.store(true, Ordering::SeqCst);
                }
            }
            (None, Some(id)) => {
                if let Some(tx) = id.as_u64().and_then(|n| shared.responses.lock().remove(&n)) {
                    let _ = tx.send(message);
                }
            }
            (None, None) => {}
        }
    }
}

fn handle_request(shared: &Arc<Shared>, method: &str, id: &Value, params: Value) {
    match method {
        "initialize" => {
            let opts = &shared.opts;
            let mut session_caps = json!({"list": {}, "close": {}});
            if opts.flag("resume", true) {
                session_caps["resume"] = json!({});
            }
            shared.reply(
                id,
                json!({
                    "protocolVersion": 1,
                    "agentInfo": {"name": "fake-acp-agent", "title": "Fake", "version": "0.0.1"},
                    "agentCapabilities": {
                        "loadSession": opts.flag("load", true),
                        "promptCapabilities": {"image": true, "embeddedContext": true},
                        "sessionCapabilities": session_caps,
                        "mcpCapabilities": {"http": true, "sse": false}
                    },
                    "authMethods": [{"id": "login", "name": "Sign in"}],
                    "_meta": {"steering": {"supported": opts.flag("steer", false)}}
                }),
            );
            let deaf = opts.number("deaf_ms");
            if deaf > 0 {
                // Stop reading stdin: whatever the client writes next piles up in the pipe.
                std::thread::sleep(Duration::from_millis(deaf));
            }
        }
        "session/new" => {
            if shared.opts.signed_out() {
                shared.fail(id, -32000, "Authentication required");
                return;
            }
            let session_id = format!("fake-{}-{}", std::process::id(), chrono_nanos());
            shared.remember(&session_id);
            if shared.opts.flag("grandchild", false) {
                spawn_grandchild(&shared.store);
            }
            shared.reply(id, json!({"sessionId": session_id, "configOptions": config_options("a")}));
            shared.update(
                &session_id,
                json!({"sessionUpdate": "available_commands_update", "availableCommands": [{"name": "review", "description": "Review changes", "input": {"hint": "branch"}}]}),
            );
            let deaf = shared.opts.number("deaf_after_new_ms");
            if deaf > 0 {
                std::thread::sleep(Duration::from_millis(deaf));
            }
        }
        "session/load" | "session/resume" => {
            let session_id = params.get("sessionId").and_then(Value::as_str).unwrap_or_default().to_string();
            if shared.opts.signed_out() {
                shared.fail(id, -32000, "Authentication required");
                return;
            }
            if shared.opts.flag("lose", false) || !shared.knows(&session_id) {
                shared.fail(id, -32002, &format!("Session not found: {session_id}"));
                return;
            }
            if method == "session/load" {
                shared.update(&session_id, json!({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": "earlier question"}}));
                shared.chunk(&session_id, "earlier answer");
                shared.update(&session_id, json!({"sessionUpdate": "tool_call", "toolCallId": "old-tool", "title": "Old tool", "kind": "read", "status": "in_progress"}));
            }
            shared.reply(id, json!({"configOptions": config_options("a")}));
        }
        "session/set_config_option" => {
            let value = params.get("value").and_then(Value::as_str).unwrap_or("a").to_string();
            shared.reply(id, json!({"configOptions": config_options(&value)}));
        }
        "_session/steering" => {
            let session_id = params.get("sessionId").and_then(Value::as_str).unwrap_or_default().to_string();
            if shared.running.load(Ordering::SeqCst) {
                let text = params.pointer("/prompt/0/text").and_then(Value::as_str).unwrap_or_default().to_string();
                shared.reply(id, json!({"outcome": "injected"}));
                shared.chunk(&session_id, &format!("steered:{text} "));
            } else {
                shared.reply(id, json!({"outcome": "promptRequired"}));
            }
        }
        "session/prompt" => {
            // Reset here, not in the worker: a cancel that arrives right behind the prompt must survive.
            shared.cancel.store(false, Ordering::SeqCst);
            shared.running.store(true, Ordering::SeqCst);
            let shared = shared.clone();
            let id = id.clone();
            std::thread::spawn(move || run_prompt(&shared, &id, &params));
        }
        other => shared.fail(id, -32601, &format!("method not found: {other}")),
    }
}

fn chrono_nanos() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}

fn spawn_grandchild(store: &Path) {
    let exe = std::env::current_exe().expect("test exe");
    let mut command = crate::proc_util::hidden_command(exe);
    command.args(HARNESS_ARGS).env(ROLE_ENV, "sleeper").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    if let Ok(child) = command.spawn() {
        let _ = std::fs::create_dir_all(store);
        let _ = std::fs::write(store.join("grandchild.pid"), child.id().to_string());
    }
}

fn run_prompt(shared: &Arc<Shared>, id: &Value, params: &Value) {
    run_prompt_inner(shared, id, params);
    shared.running.store(false, Ordering::SeqCst);
}

fn run_prompt_inner(shared: &Arc<Shared>, id: &Value, params: &Value) {
    let session_id = params.get("sessionId").and_then(Value::as_str).unwrap_or_default().to_string();
    let text = params.pointer("/prompt/0/text").and_then(Value::as_str).unwrap_or_default().to_string();
    let mut words = text.split_whitespace();
    let verb = words.next().unwrap_or("");
    let count: usize = words.next().and_then(|w| w.parse().ok()).unwrap_or(3);
    let done = |stop: &str| shared.reply(id, json!({"stopReason": stop}));

    match verb {
        "stream" => {
            for n in 0..count {
                shared.chunk(&session_id, &format!("w{n} "));
            }
            done("end_turn");
        }
        "flood" => {
            for n in 0..count {
                shared.chunk(&session_id, &format!("{n},"));
            }
            done("end_turn");
        }
        "slow" => {
            for n in 0..400 {
                if shared.cancel.load(Ordering::SeqCst) {
                    return done("cancelled");
                }
                shared.chunk(&session_id, &format!("s{n} "));
                std::thread::sleep(Duration::from_millis(25));
            }
            done("end_turn");
        }
        "tool" => {
            shared.update(&session_id, json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Run tests", "kind": "execute", "status": "in_progress"}));
            std::thread::sleep(Duration::from_millis(40));
            shared.update(&session_id, json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed"}));
            shared.chunk(&session_id, "tool done");
            done("end_turn");
        }
        "perm" => {
            shared.update(&session_id, json!({"sessionUpdate": "tool_call", "toolCallId": "w1", "title": "Write notes.txt", "kind": "edit", "status": "pending"}));
            let ask_id = shared.next_request.fetch_add(1, Ordering::SeqCst);
            let rx = shared.ask(
                ask_id,
                "session/request_permission",
                json!({
                    "sessionId": session_id,
                    "toolCall": {"toolCallId": "w1", "kind": "edit", "status": "pending", "title": "Write notes.txt"},
                    "options": [
                        {"optionId": "allow_once", "name": "Allow Once", "kind": "allow_once"},
                        {"optionId": "allow_always", "name": "Allow for Session", "kind": "allow_always"},
                        {"optionId": "reject_once", "name": "Reject", "kind": "reject_once"}
                    ]
                }),
            );
            // No timeout: the agent waits as long as the person takes.
            let Ok(answer) = rx.recv() else { return };
            let outcome = answer.pointer("/result/outcome/outcome").and_then(Value::as_str).unwrap_or("?").to_string();
            if outcome == "cancelled" {
                return done("cancelled");
            }
            let option = answer.pointer("/result/outcome/optionId").and_then(Value::as_str).unwrap_or("?").to_string();
            let allowed = option.starts_with("allow");
            shared.update(&session_id, json!({"sessionUpdate": "tool_call_update", "toolCallId": "w1", "status": if allowed { "completed" } else { "failed" }}));
            shared.chunk(&session_id, &format!("chose:{option}"));
            done("end_turn");
        }
        "rich" => {
            shared.chunk(&session_id, "Looking at it. ");
            shared.update(&session_id, json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "thinking about it"}}));
            shared.update(
                &session_id,
                json!({"sessionUpdate": "tool_call", "toolCallId": "e1", "title": "Write notes.txt", "kind": "edit", "status": "pending",
                       "locations": [{"path": "C:\\work\\notes.txt"}],
                       "content": [{"type": "diff", "path": "C:\\work\\notes.txt", "oldText": null, "newText": "hello\n"}]}),
            );
            shared.update(&session_id, json!({"sessionUpdate": "tool_call_update", "toolCallId": "e1", "status": "completed"}));
            shared.update(
                &session_id,
                json!({"sessionUpdate": "plan", "entries": [
                    {"content": "Write the notes", "priority": "high", "status": "completed"},
                    {"content": "Run the tests", "priority": "medium", "status": "in_progress"}]}),
            );
            shared.update(&session_id, json!({"sessionUpdate": "tool_call", "toolCallId": "x1", "title": "Run tests", "kind": "execute", "status": "in_progress"}));
            shared.update(
                &session_id,
                json!({"sessionUpdate": "tool_call_update", "toolCallId": "x1", "status": "completed",
                       "content": [{"type": "content", "content": {"type": "text", "text": "```\n3 passed\n```"}}]}),
            );
            shared.update(&session_id, json!({"sessionUpdate": "usage_update", "used": 1200, "size": 200000}));
            shared.chunk(&session_id, "All done.");
            done("end_turn");
        }
        "crashtool" => {
            shared.update(&session_id, json!({"sessionUpdate": "tool_call", "toolCallId": "c1", "title": "Run build", "kind": "execute", "status": "in_progress"}));
            shared.chunk(&session_id, "starting ");
            std::thread::sleep(Duration::from_millis(50));
            eprintln!("fake agent: boom (pretending to crash)");
            std::process::exit(3);
        }
        "toolhang" => {
            shared.update(&session_id, json!({"sessionUpdate": "tool_call", "toolCallId": "h1", "title": "Run build", "kind": "execute", "status": "in_progress"}));
            shared.chunk(&session_id, "building ");
            loop {
                if shared.cancel.load(Ordering::SeqCst) {
                    return done("cancelled");
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        "garbage" => {
            shared.chunk(&session_id, "before ");
            {
                let mut out = shared.out.lock();
                let _ = out.write_all(&[0xff, 0xfe, 0xfa, b'\n']);
                let _ = out.write_all(b"warning: this is a log line, not JSON-RPC\n");
                let _ = out.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{\n");
                let _ = out.flush();
            }
            shared.chunk(&session_id, "after");
            done("end_turn");
        }
        "bigdiff" => {
            let diffs: Vec<Value> = (0..count).map(|n| json!({"type": "diff", "path": format!("C:\\work\\file{n}.txt"), "oldText": "o".repeat(10_000), "newText": "n".repeat(10_000)})).collect();
            shared.update(&session_id, json!({"sessionUpdate": "tool_call", "toolCallId": "big1", "title": "Apply patch", "kind": "edit", "status": "in_progress", "content": diffs}));
            shared.update(&session_id, json!({"sessionUpdate": "tool_call_update", "toolCallId": "big1", "status": "completed"}));
            shared.chunk(&session_id, "patched");
            done("end_turn");
        }
        "permcrash" => {
            shared.update(&session_id, json!({"sessionUpdate": "tool_call", "toolCallId": "w1", "title": "Write notes.txt", "kind": "edit", "status": "pending"}));
            let ask_id = shared.next_request.fetch_add(1, Ordering::SeqCst);
            let _rx = shared.ask(
                ask_id,
                "session/request_permission",
                json!({
                    "sessionId": session_id,
                    "toolCall": {"toolCallId": "w1", "kind": "edit", "status": "pending", "title": "Write notes.txt"},
                    "options": [
                        {"optionId": "allow_once", "name": "Allow Once", "kind": "allow_once"},
                        {"optionId": "reject_once", "name": "Reject", "kind": "reject_once"}
                    ]
                }),
            );
            std::thread::sleep(Duration::from_millis(400));
            eprintln!("fake agent: boom (crashing with a question open)");
            std::process::exit(4);
        }
        "crash" => {
            shared.chunk(&session_id, "partial ");
            eprintln!("fake agent: boom (pretending to crash)");
            std::process::exit(3);
        }
        "quiet" => loop {
            if shared.cancel.load(Ordering::SeqCst) {
                return done("cancelled");
            }
            std::thread::sleep(Duration::from_millis(20));
        },
        "hang" => loop {
            std::thread::sleep(Duration::from_millis(100));
        },
        "fsreq" => {
            // A response nobody asked for, then a request whose id equals the id of the prompt in flight.
            shared.send(json!({"jsonrpc": "2.0", "id": 99999, "result": {}}));
            let prompt_id = id.as_u64().expect("numeric prompt id");
            let rx = shared.ask(prompt_id, "fs/read_text_file", json!({"sessionId": session_id, "path": "C:\\secret.txt"}));
            let reply = rx.recv().unwrap_or(Value::Null);
            let code = reply.pointer("/error/code").and_then(Value::as_i64).unwrap_or(0);
            shared.chunk(&session_id, &format!("fs-error:{code}"));
            done("end_turn");
        }
        "empty" => done("end_turn"),
        "weird" => {
            shared.update(&session_id, json!({"sessionUpdate": "future_thing", "x": 1}));
            shared.chunk(&session_id, "after weird");
            done("end_turn");
        }
        _ => {
            shared.chunk(&session_id, &format!("echo: {text}"));
            done("end_turn");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Not a test when run normally. As a child of another test (`PM_FAKE_ACP_ROLE` set) it is the agent.
    #[test]
    fn child_entry() {
        match std::env::var(ROLE_ENV).as_deref() {
            Ok("agent") => {
                run_agent();
                std::process::exit(0);
            }
            Ok("sleeper") => {
                std::thread::sleep(Duration::from_secs(120));
                std::process::exit(0);
            }
            _ => {}
        }
    }
}
