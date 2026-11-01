//! `pm-fake-tui --acp`: a fake agent that speaks the Agent Client Protocol over stdin/stdout, for the end-to-end tests
//! of the chat lane. No tokens, no network, deterministic. The same program is the fake TUI without the flag, so one
//! definition (`packages/e2e/fixtures/agents/nimbus.toml`) has a terminal and a chat way to run.
//!
//! What it does is chosen by the first word of the prompt text (the rest is free text):
//!
//! | prompt | behaviour |
//! |---|---|
//! | `stream N` | N words, one every 40 ms, then `end_turn` |
//! | `work` | text, two reads, two edits (with diffs), a command, text: the activity line of a turn |
//! | `perm` | an edit that asks for permission; waits (no timeout) for the answer and says `chose:<option id>` |
//! | `slow` | a word every 30 ms for 20 s; ends with `cancelled` when asked to stop |
//! | `crash` | one word, then the process exits with code 3 |
//! | anything else | `echo: <text>` |
//!
//! Sessions are remembered across processes in `PM_FAKE_ACP_STORE` (else `acp-sessions` next to the script file named
//! by `PM_FAKE_TUI_SCRIPT`, else the temp folder): `session/load` replays what was said, `session/resume` answers
//! without replay, and an id nobody remembers is "Session not found". Settings: a `model` option (`a`, `b`).

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

struct Shared {
    out: Mutex<std::io::Stdout>,
    cancel: AtomicBool,
    responses: Mutex<HashMap<u64, mpsc::Sender<Value>>>,
    next_request: AtomicU64,
    store: PathBuf,
}

impl Shared {
    fn send(&self, value: Value) {
        let mut out = self.out.lock().unwrap();
        let _ = writeln!(out, "{value}");
        let _ = out.flush();
    }

    fn update(&self, session: &str, update: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": session, "update": update}}));
    }

    fn chunk(&self, session: &str, text: &str) {
        self.update(session, json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}));
    }

    fn reply(&self, id: &Value, result: Value) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "result": result}));
    }

    fn fail(&self, id: &Value, code: i64, message: &str) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}));
    }

    fn ask(&self, method: &str, params: Value) -> mpsc::Receiver<Value> {
        let id = self.next_request.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        self.responses.lock().unwrap().insert(id, tx);
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        rx
    }

    // ---- what was said, per session ---------------------------------------------------------------------------

    fn file(&self, session: &str) -> PathBuf {
        self.store.join(format!("{session}.json"))
    }

    fn history(&self, session: &str) -> Option<Vec<(String, String)>> {
        let text = std::fs::read_to_string(self.file(session)).ok()?;
        let value: Value = serde_json::from_str(&text).ok()?;
        Some(value.as_array()?.iter().map(|t| (t["user"].as_str().unwrap_or("").to_string(), t["assistant"].as_str().unwrap_or("").to_string())).collect())
    }

    fn remember(&self, session: &str, turns: &[(String, String)]) {
        let _ = std::fs::create_dir_all(&self.store);
        let list: Vec<Value> = turns.iter().map(|(u, a)| json!({"user": u, "assistant": a})).collect();
        let _ = std::fs::write(self.file(session), Value::Array(list).to_string());
    }

    fn add_turn(&self, session: &str, user: &str, assistant: &str) {
        let mut turns = self.history(session).unwrap_or_default();
        turns.push((user.to_string(), assistant.to_string()));
        self.remember(session, &turns);
    }
}

fn config_options(current: &str) -> Value {
    json!([{
        "id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": current,
        "options": [{"value": "a", "name": "Nimbus Small"}, {"value": "b", "name": "Nimbus Large"}]
    }])
}

fn store_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("PM_FAKE_ACP_STORE") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    if let Ok(script) = std::env::var("PM_FAKE_TUI_SCRIPT") {
        if let Some(parent) = Path::new(&script).parent() {
            return parent.join("acp-sessions");
        }
    }
    std::env::temp_dir().join("pm-fake-acp-sessions")
}

/// The agent loop. Returns when stdin closes.
pub fn run() {
    let shared = Arc::new(Shared {
        out: Mutex::new(std::io::stdout()),
        cancel: AtomicBool::new(false),
        responses: Mutex::new(HashMap::new()),
        next_request: AtomicU64::new(1000),
        store: store_dir(),
    });
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        let method = message.get("method").and_then(Value::as_str).map(str::to_string);
        let id = message.get("id").cloned();
        match (method, id) {
            (Some(method), Some(id)) => handle_request(&shared, &method, &id, message.get("params").cloned().unwrap_or(Value::Null)),
            (Some(method), None) => {
                if method == "session/cancel" {
                    shared.cancel.store(true, Ordering::SeqCst);
                }
            }
            (None, Some(id)) => {
                if let Some(tx) = id.as_u64().and_then(|n| shared.responses.lock().unwrap().remove(&n)) {
                    let _ = tx.send(message);
                }
            }
            (None, None) => {}
        }
    }
}

fn handle_request(shared: &Arc<Shared>, method: &str, id: &Value, params: Value) {
    match method {
        "initialize" => shared.reply(
            id,
            json!({
                "protocolVersion": 1,
                "agentInfo": {"name": "nimbus-acp", "title": "Nimbus", "version": "0.4.2"},
                "agentCapabilities": {
                    "loadSession": true,
                    "promptCapabilities": {"image": false, "embeddedContext": true},
                    "sessionCapabilities": {"list": {}, "close": {}, "resume": {}},
                    "mcpCapabilities": {"http": false, "sse": false}
                },
                "authMethods": []
            }),
        ),
        "session/new" => {
            let session = format!("nimbus-{}-{}", std::process::id(), nanos());
            shared.remember(&session, &[]);
            shared.reply(id, json!({"sessionId": session, "configOptions": config_options("a")}));
            shared.update(&session, json!({"sessionUpdate": "available_commands_update", "availableCommands": [
                {"name": "review", "description": "Review the changes in this folder", "input": {"hint": "branch"}},
                {"name": "summarize", "description": "Summarize the conversation so far"}
            ]}));
        }
        "session/load" | "session/resume" => {
            let session = params.get("sessionId").and_then(Value::as_str).unwrap_or_default().to_string();
            let Some(turns) = shared.history(&session) else {
                shared.fail(id, -32002, &format!("Session not found: {session}"));
                return;
            };
            if method == "session/load" {
                for (user, assistant) in turns {
                    shared.update(&session, json!({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": user}}));
                    if !assistant.is_empty() {
                        shared.chunk(&session, &assistant);
                    }
                }
            }
            shared.reply(id, json!({"configOptions": config_options("a")}));
            shared.update(&session, json!({"sessionUpdate": "available_commands_update", "availableCommands": [
                {"name": "review", "description": "Review the changes in this folder", "input": {"hint": "branch"}},
                {"name": "summarize", "description": "Summarize the conversation so far"}
            ]}));
        }
        "session/set_config_option" => {
            let value = params.get("value").and_then(Value::as_str).unwrap_or("a").to_string();
            shared.reply(id, json!({"configOptions": config_options(&value)}));
        }
        "session/prompt" => {
            // Reset here, not in the worker: a cancel that arrives right behind the prompt must survive.
            shared.cancel.store(false, Ordering::SeqCst);
            let shared = shared.clone();
            let id = id.clone();
            std::thread::spawn(move || prompt(&shared, &id, &params));
        }
        other => shared.fail(id, -32601, &format!("method not found: {other}")),
    }
}

fn nanos() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}

fn nap(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

fn prompt(shared: &Arc<Shared>, id: &Value, params: &Value) {
    let session = params.get("sessionId").and_then(Value::as_str).unwrap_or_default().to_string();
    let text = params.pointer("/prompt/0/text").and_then(Value::as_str).unwrap_or_default().to_string();
    let mut words = text.split_whitespace();
    let verb = words.next().unwrap_or("");
    let count: usize = words.next().and_then(|w| w.parse().ok()).unwrap_or(3);
    let said = Mutex::new(String::new());
    let say = |piece: &str| {
        shared.chunk(&session, piece);
        said.lock().unwrap().push_str(piece);
    };
    let done = |stop: &str| {
        shared.add_turn(&session, &text, &said.lock().unwrap());
        shared.reply(id, json!({"stopReason": stop}));
    };
    let tool = |call: &str, title: &str, kind: &str, path: Option<&str>, diff: Option<(&str, &str)>, output: &str| {
        let mut start = json!({"sessionUpdate": "tool_call", "toolCallId": call, "title": title, "kind": kind, "status": "in_progress"});
        if let Some(path) = path {
            start["locations"] = json!([{"path": path}]);
        }
        if let Some((old, new)) = diff {
            start["content"] = json!([{"type": "diff", "path": path.unwrap_or(""), "oldText": old, "newText": new}]);
        }
        shared.update(&session, start);
        nap(150);
        let mut finish = json!({"sessionUpdate": "tool_call_update", "toolCallId": call, "status": "completed"});
        if !output.is_empty() {
            finish["content"] = json!([{"type": "content", "content": {"type": "text", "text": output}}]);
        }
        shared.update(&session, finish);
    };

    match verb {
        "stream" => {
            for n in 0..count {
                say(&format!("w{n} "));
                nap(40);
            }
            done("end_turn");
        }
        "work" => {
            say("Looking at the project. ");
            nap(120);
            tool("r1", "Read README.md", "read", Some("README.md"), None, "");
            tool("r2", "Read src/main.rs", "read", Some("src/main.rs"), None, "");
            tool("e1", "Edit src/main.rs", "edit", Some("src/main.rs"), Some(("fn main() {}", "fn main() { run(); }")), "");
            tool("e2", "Edit notes.md", "edit", Some("notes.md"), Some(("", "# Notes\n")), "");
            tool("x1", "Run npm test", "execute", None, None, "3 passed");
            say("All done: two files changed, tests pass.");
            done("end_turn");
        }
        "perm" => {
            shared.update(&session, json!({"sessionUpdate": "tool_call", "toolCallId": "w1", "title": "Write notes.txt", "kind": "edit", "status": "pending",
                "locations": [{"path": "notes.txt"}]}));
            let rx = shared.ask(
                "session/request_permission",
                json!({
                    "sessionId": session,
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
            if answer.pointer("/result/outcome/outcome").and_then(Value::as_str) == Some("cancelled") {
                return done("cancelled");
            }
            let option = answer.pointer("/result/outcome/optionId").and_then(Value::as_str).unwrap_or("?").to_string();
            let allowed = option.starts_with("allow");
            shared.update(&session, json!({"sessionUpdate": "tool_call_update", "toolCallId": "w1", "status": if allowed { "completed" } else { "failed" }}));
            say(&format!("chose:{option}"));
            done("end_turn");
        }
        "slow" => {
            for n in 0..700 {
                if shared.cancel.load(Ordering::SeqCst) {
                    return done("cancelled");
                }
                say(&format!("s{n} "));
                nap(30);
            }
            done("end_turn");
        }
        "crash" => {
            say("partial ");
            eprintln!("nimbus-acp: boom (pretending to crash)");
            std::process::exit(3);
        }
        _ => {
            say(&format!("echo: {text}"));
            done("end_turn");
        }
    }
}
