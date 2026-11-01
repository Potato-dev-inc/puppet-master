//! The ACP side of the bridge: JSON-RPC lines on stdin/stdout. Requests are answered in order by the reading
//! thread, except `session/prompt`, which runs on its own thread so `session/cancel` is read while a turn runs.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::turn::{run_turn, TurnControl, TurnEnd, TurnLimits, TurnRequest};
use super::{info_for_profile, NO_MEMORY_NOTICE};
use crate::tui::TuiProfile;

/// A line longer than this is skipped (the host never sends one: a prompt is a few KiB).
const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

/// JSON-RPC "internal error": the turn failed; `message` is one plain sentence.
const RPC_FAILED: i64 = -32603;
const RPC_INVALID_PARAMS: i64 = -32602;
const RPC_METHOD_NOT_FOUND: i64 = -32601;

#[derive(Debug, Clone, Default)]
pub struct ServeOptions {
    /// Start the agent's headless mode read-only (refused when the definition cannot enforce it).
    pub read_only: bool,
    pub limits: TurnLimits,
}

struct Session {
    cwd: String,
    /// The agent's own conversation id from the last finished turn.
    provider_session: Option<String>,
    /// The "no memory" sentence was already sent.
    noticed: bool,
}

struct Active {
    session: String,
    ctl: Arc<TurnControl>,
}

struct Shared {
    out: Mutex<Box<dyn Write + Send>>,
    profile: Arc<dyn TuiProfile>,
    label: String,
    options: ServeOptions,
    has_memory: bool,
    sessions: Mutex<HashMap<String, Session>>,
    active: Mutex<Option<Active>>,
    next_session: Mutex<u64>,
}

impl Shared {
    fn send(&self, value: Value) {
        let mut out = self.out.lock();
        let written = writeln!(out, "{value}").and_then(|_| out.flush());
        drop(out);
        if written.is_err() {
            // The host is gone: nobody reads the answer, so stop the agent.
            if let Some(active) = self.active.lock().as_ref() {
                active.ctl.cancel();
            }
        }
    }

    fn reply(&self, id: &Value, result: Value) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "result": result}));
    }

    fn fail(&self, id: &Value, code: i64, message: &str) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}));
    }

    fn chunk(&self, session: &str, text: &str) {
        if text.is_empty() {
            return;
        }
        self.send(json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {"sessionId": session, "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}}
        }));
    }
}

fn read_line<R: BufRead>(input: &mut R, buf: &mut Vec<u8>) -> Option<String> {
    loop {
        buf.clear();
        match input.read_until(b'\n', buf) {
            Ok(0) | Err(_) => return None,
            Ok(_) if buf.len() > MAX_LINE_BYTES => continue,
            Ok(_) => return Some(String::from_utf8_lossy(buf).trim().to_string()),
        }
    }
}

/// Before `initialize` can succeed: answer it (and anything else) with the reason, then stop. The host shows
/// the sentence instead of "the adapter closed the connection".
pub fn refuse_initialize<R: BufRead, W: Write>(mut input: R, mut output: W, sentence: &str) {
    let mut buf = Vec::new();
    while let Some(line) = read_line(&mut input, &mut buf) {
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(id) = message.get("id").filter(|_| message.get("method").is_some()) else { continue };
        let reply = json!({"jsonrpc": "2.0", "id": id, "error": {"code": RPC_FAILED, "message": sentence}});
        let _ = writeln!(output, "{reply}").and_then(|_| output.flush());
        if message.get("method").and_then(Value::as_str) == Some("initialize") {
            return;
        }
    }
}

/// Serves ACP for `profile` until the host closes stdin. A turn still running then is stopped.
pub fn serve<R: BufRead, W: Write + Send + 'static>(profile: Arc<dyn TuiProfile>, label: String, mut input: R, output: W, options: ServeOptions) {
    let info = info_for_profile(&*profile, &label);
    let has_memory = info.as_ref().map(|i| i.has_memory).unwrap_or(false);
    let shared = Arc::new(Shared {
        out: Mutex::new(Box::new(output)),
        profile,
        label,
        options,
        has_memory,
        sessions: Mutex::new(HashMap::new()),
        active: Mutex::new(None),
        next_session: Mutex::new(0),
    });
    let mut workers: Vec<std::thread::JoinHandle<()>> = Vec::new();
    let mut buf = Vec::new();
    while let Some(line) = read_line(&mut input, &mut buf) {
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(method) = message.get("method").and_then(Value::as_str).map(str::to_string) else { continue };
        let id = message.get("id").cloned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        match (method.as_str(), id) {
            ("initialize", Some(id)) => match &info {
                Ok(_) => shared.reply(&id, initialize_result(&shared.label)),
                Err(sentence) => {
                    shared.fail(&id, RPC_FAILED, sentence);
                    break;
                }
            },
            ("session/new", Some(id)) => new_session(&shared, &id, &params),
            ("session/prompt", Some(id)) => {
                workers.retain(|worker| !worker.is_finished());
                if let Some(worker) = start_prompt(&shared, id, &params) {
                    workers.push(worker);
                }
            }
            ("session/cancel", id) => {
                cancel(&shared, params.get("sessionId").and_then(Value::as_str));
                if let Some(id) = id {
                    shared.reply(&id, json!({}));
                }
            }
            (other, Some(id)) => shared.fail(&id, RPC_METHOD_NOT_FOUND, &format!("method not found: {other}")),
            (_, None) => {}
        }
    }
    // The host closed the pipe (or initialize was refused): no turn outlives the conversation.
    if let Some(active) = shared.active.lock().as_ref() {
        active.ctl.cancel();
    }
    for worker in workers {
        let _ = worker.join();
    }
}

fn initialize_result(label: &str) -> Value {
    json!({
        "protocolVersion": 1,
        "agentInfo": {"name": label, "title": label, "version": env!("CARGO_PKG_VERSION")},
        "agentCapabilities": {
            "loadSession": false,
            "promptCapabilities": {"image": false, "audio": false, "embeddedContext": false}
        },
        "authMethods": []
    })
}

fn new_session(shared: &Arc<Shared>, id: &Value, params: &Value) {
    let cwd = params
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|c| !c.trim().is_empty())
        .map(str::to_string)
        .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let session_id = {
        let mut next = shared.next_session.lock();
        *next += 1;
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        format!("bridge-{}-{}-{}", std::process::id(), *next, nanos)
    };
    shared.sessions.lock().insert(session_id.clone(), Session { cwd, provider_session: None, noticed: false });
    shared.reply(id, json!({"sessionId": session_id}));
}

fn cancel(shared: &Arc<Shared>, session: Option<&str>) {
    if let Some(active) = shared.active.lock().as_ref() {
        if session.is_none_or(|s| s == active.session) {
            active.ctl.cancel();
        }
    }
}

/// The text of a prompt: its text blocks, in order. Other block kinds are not offered in `initialize`.
fn prompt_text(params: &Value) -> String {
    params
        .get("prompt")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .unwrap_or_default()
}

fn start_prompt(shared: &Arc<Shared>, id: Value, params: &Value) -> Option<std::thread::JoinHandle<()>> {
    let Some(session_id) = params.get("sessionId").and_then(Value::as_str).map(str::to_string) else {
        shared.fail(&id, RPC_INVALID_PARAMS, "The message has no session.");
        return None;
    };
    let text = prompt_text(params);
    if text.trim().is_empty() {
        shared.fail(&id, RPC_INVALID_PARAMS, "This chat takes text messages only.");
        return None;
    }
    let (cwd, resume, notice) = {
        let mut sessions = shared.sessions.lock();
        let Some(session) = sessions.get_mut(&session_id) else {
            drop(sessions);
            shared.fail(&id, RPC_INVALID_PARAMS, "That chat session does not exist.");
            return None;
        };
        let notice = !shared.has_memory && !session.noticed;
        session.noticed = true;
        let resume = if shared.has_memory { session.provider_session.clone() } else { None };
        (session.cwd.clone(), resume, notice)
    };
    let ctl = TurnControl::new();
    {
        let mut active = shared.active.lock();
        if active.is_some() {
            drop(active);
            shared.fail(&id, RPC_FAILED, "A message is still being answered; wait for it or stop it first.");
            return None;
        }
        *active = Some(Active { session: session_id.clone(), ctl: ctl.clone() });
    }
    let shared = shared.clone();
    std::thread::Builder::new()
        .name("acp-bridge-turn".into())
        .spawn(move || {
            if notice {
                shared.chunk(&session_id, &format!("{NO_MEMORY_NOTICE}\n\n"));
            }
            let request = TurnRequest { cwd: cwd.into(), prompt: text, resume, read_only: shared.options.read_only };
            let end = {
                let mut emit = |text: &str| shared.chunk(&session_id, text);
                run_turn(&*shared.profile, &shared.label, &request, &shared.options.limits, &ctl, &mut emit)
            };
            // The next prompt may arrive the moment the answer is out, so the slot is freed first.
            *shared.active.lock() = None;
            match end {
                TurnEnd::Done { provider_session } => {
                    if let (Some(provider), true) = (provider_session, shared.has_memory) {
                        if let Some(session) = shared.sessions.lock().get_mut(&session_id) {
                            session.provider_session = Some(provider);
                        }
                    }
                    shared.reply(&id, json!({"stopReason": "end_turn"}));
                }
                TurnEnd::Cancelled => shared.reply(&id, json!({"stopReason": "cancelled"})),
                TurnEnd::Failed(sentence) => shared.fail(&id, RPC_FAILED, &sentence),
            }
        })
        .ok()
}
