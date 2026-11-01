//! Chat sessions on the agent tools (`run_agent`, `wait_agents`, `inspect_agent`, `followup_task`, `send_message`,
//! `answer_prompt`, `interrupt_agent`, `close_agent`): an agent that orchestrates others can start a worker as a chat
//! session and drive it with the same tools. Their handle is the chat pane's id.
//!
//! What an agent reads from a chat session is the chat (the agent's own words, what it asked, how the turn ended),
//! never the one-line screen of its pane. The request/answer shapes follow the terminal runs' (`status`, `result`,
//! `prompt`, `pane_id`, a cursor), so a caller does not need two code paths.
//!
//! * **Choosing.** `run_agent` takes `lane`: `"acp"` asks for a chat session (the Labs switch still applies), `"terminal"`
//!   asks for today's way, and without it the agent's saved choice ("How to run") decides. When the saved choice
//!   decides and the chat cannot start, the request falls through to the terminal way (the automatic fallback);
//!   when the caller asked for `"acp"` it gets the error instead.
//! * **Answering permissions.** `answer_prompt` on a chat's question needs the person's switch for that chat
//!   (`AcpChat::set_agent_answering`, default off, Settings window only). Without it the call is refused and says who
//!   can answer. With it, the choice must be one of the options the agent offered, and the answer goes through the
//!   question store like the person's own, with an audit line naming the caller.
//! * **Whose chat.** An agent may steer, answer, stop or close only a chat it started itself. A chat started by the
//!   person (or by another agent) is `NOT_YOUR_CHAT` for it; reading it is open. A caller without an MCP session (the
//!   command line) is not restricted by this rule, but the answer switch applies to it all the same.
//! * **Generic.** Nothing here names an agent.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{get, AcpChat, AgentAnswerError, Readiness, SendError, StartDeps, StartRequest, LANE};
use crate::acp::lane::{effective_lane, LaneStore, RunLane};
use crate::acp::state::{ExitKind, Phase, Snapshot, StopKind};
use crate::audit::Who;
use crate::operations::OperationError;
use crate::session_chat::ChatItem;

type Reply = Result<Value, OperationError>;

const DEFAULT_WAIT_MS: u64 = 120_000;
const MAX_WAIT_MS: u64 = 300_000;
/// Longest wait `run_agent` and `followup_task` do before they answer "still running".
const POLL: Duration = Duration::from_millis(100);
/// Items `inspect_agent` shows of the chat.
const RECENT_ITEMS: usize = 16;
const ITEM_TEXT_CHARS: usize = 600;

fn err(code: &str, message: impl Into<String>, recoverable: bool) -> OperationError {
    OperationError::new(code, message, recoverable)
}

// ---- choosing the way ---------------------------------------------------------------------------------------

/// What a `run_agent` call wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Want {
    /// A chat session. `fallback`: the agent's saved choice asked for it, so a chat that cannot start falls back to
    /// the terminal way instead of failing.
    Chat { fallback: bool },
    /// Today's way.
    Terminal,
}

/// `lane` is the caller's word; `saved` the agent's "How to run".
pub fn wants(lane: Option<&str>, saved: RunLane, labs_on: bool, has_definition: bool) -> Result<Want, String> {
    match lane.map(str::trim).filter(|l| !l.is_empty()) {
        Some("acp") => Ok(Want::Chat { fallback: false }),
        Some("terminal") => Ok(Want::Terminal),
        Some(other) => Err(format!("lane must be \"acp\" or \"terminal\", not {other:?}")),
        None => Ok(if effective_lane(saved, labs_on, has_definition) == RunLane::Acp { Want::Chat { fallback: true } } else { Want::Terminal }),
    }
}

// ---- the views ----------------------------------------------------------------------------------------------

/// `completed`, `failed`, `cancelled`, `waiting_input`, `running` or `idle` (nothing was asked yet).
pub fn status_of(snapshot: &Snapshot) -> &'static str {
    match snapshot.phase {
        Phase::Working | Phase::Cancelling | Phase::Starting | Phase::Resuming => "running",
        Phase::NeedsInput => "waiting_input",
        Phase::Idle | Phase::Dormant | Phase::Exited => match snapshot.last_stop {
            Some(StopKind::EndTurn | StopKind::MaxTokens | StopKind::MaxTurnRequests) => "completed",
            Some(StopKind::Cancelled) => "cancelled",
            Some(StopKind::Failed | StopKind::Hung | StopKind::Refusal) => "failed",
            None => match snapshot.exit.as_ref().map(|e| e.kind) {
                Some(ExitKind::Crashed | ExitKind::Hung | ExitKind::WriteStalled | ExitKind::StartFailed) => "failed",
                _ => "idle",
            },
        },
    }
}

fn is_final(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled")
}

fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let end = text.char_indices().nth(max).map(|(i, _)| i).unwrap_or(text.len());
    format!("{}…", &text[..end])
}

/// The newest turn's words: the agent's last message after the person's last message.
fn last_turn(items: &[ChatItem]) -> (Option<String>, Option<String>) {
    let mut reply: Option<String> = None;
    let mut problem: Option<String> = None;
    for item in items.iter().rev() {
        match item {
            ChatItem::User { .. } => break,
            ChatItem::Assistant { text, .. } if reply.is_none() && !text.trim().is_empty() => reply = Some(text.clone()),
            ChatItem::Notice { notice_kind, text, .. } if problem.is_none() && notice_kind == "error" => problem = Some(text.clone()),
            _ => {}
        }
    }
    (reply, problem)
}

fn item_line(item: &ChatItem) -> Option<Value> {
    match item {
        ChatItem::User { text, .. } => Some(json!({ "kind": "user", "text": cut(text, ITEM_TEXT_CHARS) })),
        ChatItem::Assistant { text, .. } => Some(json!({ "kind": "assistant", "text": cut(text, ITEM_TEXT_CHARS) })),
        ChatItem::ToolCall { name, summary, .. } => Some(json!({ "kind": "tool", "name": name, "text": cut(summary, 200) })),
        ChatItem::ToolResult { ok, output, .. } => Some(json!({ "kind": "tool_result", "ok": ok, "text": cut(output, 200) })),
        ChatItem::TurnEnd { outcome, duration_ms, .. } => Some(json!({ "kind": "turn_end", "outcome": serde_json::to_value(outcome).unwrap_or(Value::Null), "duration_ms": duration_ms })),
        ChatItem::Notice { text, notice_kind, .. } => Some(json!({ "kind": "notice", "notice_kind": notice_kind, "text": cut(text, 300) })),
        ChatItem::Plan { .. } | ChatItem::Usage { .. } | ChatItem::Session { .. } => None,
    }
}

/// The question the agent waits on, in the shape `answer_prompt` takes: `prompt_id` and one of the option ids.
fn prompt_of(state: &Value) -> Value {
    let Some(first) = state.get("permissions").and_then(Value::as_array).and_then(|list| list.first()) else { return Value::Null };
    let options: Vec<Value> = first
        .get("options")
        .and_then(Value::as_array)
        .map(|list| list.iter().map(|o| json!({ "id": o.get("id"), "name": o.get("name"), "kind": o.get("kind") })).collect())
        .unwrap_or_default();
    json!({
        "prompt_id": first.get("questionId").cloned().unwrap_or_else(|| first.get("askId").cloned().unwrap_or(Value::Null)),
        "kind": "acp_permission",
        "title": first.get("title"),
        "tool_kind": first.get("toolKind"),
        "options": options,
        "answerable_by_agent": state.get("agentAnswering").and_then(Value::as_bool).unwrap_or(false),
        "hint": if state.get("agentAnswering").and_then(Value::as_bool).unwrap_or(false) {
            "answer_prompt with prompt_id and the id of one option"
        } else {
            "Only the person can answer this: they have not let agents answer in this chat. Tell them, or wait."
        },
    })
}

/// One chat as the tools return it.
pub fn view(chat: &AcpChat) -> Value {
    let snapshot = chat.session().snapshot();
    let state = chat.state();
    let items = chat.items();
    let status = status_of(&snapshot);
    let (reply, problem) = last_turn(&items);
    let in_terminal = !state.get("inTerminal").map(Value::is_null).unwrap_or(true);
    json!({
        "handle": chat.pane_id(),
        "pane_id": chat.pane_id(),
        "lane": LANE,
        "agent": chat.agent_id(),
        "name": chat.label(),
        "status": if in_terminal && !is_final(status) { "idle" } else { status },
        "stage": state.get("phase"),
        "result": if status == "completed" { reply.clone().map(Value::String).unwrap_or(Value::Null) } else { Value::Null },
        "result_capture": if is_final(status) { "authoritative" } else { "pending" },
        "prompt": prompt_of(&state),
        "error": if status == "failed" {
            json!({ "code": "ACP_TURN_FAILED", "message": problem.unwrap_or_else(|| "The turn failed.".to_string()), "recoverable": true })
        } else {
            Value::Null
        },
        "pane_open": !chat.is_closed(),
        "in_terminal": in_terminal,
        "turn": snapshot.turn,
        "revision": snapshot.revision,
        "next_cursor": snapshot.revision,
        "warnings": [],
    })
}

fn inspect(chat: &AcpChat) -> Value {
    let mut value = view(chat);
    let items = chat.items();
    let recent: Vec<Value> = items.iter().filter_map(item_line).rev().take(RECENT_ITEMS).collect::<Vec<_>>().into_iter().rev().collect();
    if let Some(map) = value.as_object_mut() {
        map.insert("recent".into(), Value::Array(recent));
        map.insert("capabilities".into(), json!({ "steer": chat.can_steer(), "chat": true, "terminal_screen": false }));
        map.insert("note".into(), json!("A chat session has no terminal screen. `recent` is its conversation; the whole chat is GET /agents/{handle}/transcript."));
    }
    value
}

// ---- helpers ------------------------------------------------------------------------------------------------

fn body_json(body: &[u8]) -> Option<Value> {
    serde_json::from_slice(body).ok()
}

fn text_of(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

/// The chat a handle names, if it names one.
fn chat_of(handle: &str) -> Option<std::sync::Arc<AcpChat>> {
    get(handle.trim())
}

fn who(session: Option<&str>) -> Who {
    match session {
        Some(session) => Who::Agent(session.to_string()),
        None => Who::Local,
    }
}

/// An agent (a caller with an MCP session) may change only a chat it started.
fn owned(chat: &AcpChat, session: Option<&str>) -> Result<(), OperationError> {
    match (session, chat.started_by()) {
        (None, _) => Ok(()),
        (Some(me), Some(owner)) if me == owner => Ok(()),
        (Some(_), _) => Err(err(
            "NOT_YOUR_CHAT",
            "This chat session was not started by you, so you may read it but not steer, answer, stop or close it. Start your own with run_agent(lane: \"acp\"), or ask the person.",
            false,
        )),
    }
}

fn chat_for(handle: &str) -> Result<std::sync::Arc<AcpChat>, OperationError> {
    chat_of(handle).ok_or_else(|| err("AGENT_NOT_FOUND", format!("{handle} is not a chat session"), false))
}

fn send_failure(error: SendError) -> OperationError {
    match error {
        SendError::Busy => err("AGENT_BUSY", "The agent is working on a turn. Use send_message to steer it, or wait_agents and then followup_task.", true),
        SendError::Gone => err("AGENT_CLOSED", "That chat session was closed.", false),
        SendError::Refused(why) => err("MESSAGE_REFUSED", why, true),
    }
}

/// Waits until the chat's status is one of `until`, or `wait_ms` is over. Returns the status last seen.
fn wait_status(chat: &AcpChat, until: &[String], wait_ms: u64) -> &'static str {
    let deadline = Instant::now() + Duration::from_millis(wait_ms);
    loop {
        let status = status_of(&chat.session().snapshot());
        if until.iter().any(|u| u == status) || chat.is_closed() || Instant::now() >= deadline {
            return status;
        }
        std::thread::sleep(POLL);
    }
}

fn default_until() -> Vec<String> {
    ["completed", "failed", "cancelled", "waiting_input"].iter().map(|s| s.to_string()).collect()
}

fn until_of(value: &Value) -> Vec<String> {
    let listed: Vec<String> = value
        .get("until")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).map(|s| if s == "needs_input" { "waiting_input".to_string() } else { s.to_string() }).collect())
        .unwrap_or_default();
    if listed.is_empty() {
        default_until()
    } else {
        listed
    }
}

fn wait_ms_of(value: &Value, default: u64) -> Result<u64, OperationError> {
    let ms = value.get("wait_ms").or_else(|| value.get("timeout_ms")).and_then(Value::as_u64).unwrap_or(default);
    if ms > MAX_WAIT_MS {
        return Err(err("INVALID_TIMEOUT", format!("wait_ms must be between 0 and {MAX_WAIT_MS}"), false));
    }
    Ok(ms)
}

// ---- the routes ---------------------------------------------------------------------------------------------

/// Handles `/agents/*` requests that belong to a chat session. `None`: not one of ours, carry on with the terminal
/// runs. `query` is the request's query string (`?after=`).
pub fn intercept(method: &str, segments: &[&str], query: &str, body: &[u8], deps: &StartDeps, session: Option<&str>) -> Option<Reply> {
    match (method, segments) {
        ("POST", ["agents", "run"]) => run(body, query, deps, session),
        ("POST", ["agents", "wait"]) => wait(body),
        ("POST", ["agents", "followup" | "send"]) => {
            let value = body_json(body)?;
            let chat = chat_of(&text_of(&value, "handle")?)?;
            Some(followup(&chat, &value, session, segments[1] == "followup"))
        }
        ("POST", ["agents", "answer"]) => {
            let value = body_json(body)?;
            let chat = chat_of(&text_of(&value, "handle").or_else(|| text_of(&value, "pane_id"))?)?;
            Some(answer(&chat, &value, session))
        }
        ("POST", ["agents", "cancel" | "interrupt"]) => {
            let value = body_json(body)?;
            let chat = chat_of(&text_of(&value, "handle").or_else(|| text_of(&value, "pane_id"))?)?;
            Some(owned(&chat, session).and_then(|()| {
                chat.cancel().map_err(|e| err("ACP_CANCEL_FAILED", e.to_string(), true))?;
                Ok(view(&chat))
            }))
        }
        ("POST", ["agents", "close"]) => {
            let value = body_json(body)?;
            let chat = chat_of(&text_of(&value, "handle")?)?;
            Some(owned(&chat, session).and_then(|()| {
                let last = view(&chat);
                crate::pty::registry_kill_pane_and_wait(&deps.registry, chat.pane_id(), Some("closed by an agent")).map_err(|e| err("CLOSE_FAILED", e, true))?;
                deps.host.panes_changed();
                let mut done = last;
                if let Some(map) = done.as_object_mut() {
                    map.insert("pane_open".into(), json!(false));
                }
                Ok(done)
            }))
        }
        ("GET", ["agents", handle]) => chat_of(handle).map(|chat| Ok(inspect(&chat))),
        ("GET", ["agents", handle, "transcript"]) => {
            let chat = chat_of(handle)?;
            let after = query.trim_start_matches('?').split('&').find_map(|pair| pair.strip_prefix("after=")).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
            let lines: Vec<Value> = chat.items().iter().filter_map(item_line).collect();
            let total = lines.len();
            Some(Ok(json!({ "handle": handle, "lane": LANE, "items": lines.into_iter().skip(after).collect::<Vec<_>>(), "next_after": total })))
        }
        _ => None,
    }
}

fn run(body: &[u8], query: &str, deps: &StartDeps, session: Option<&str>) -> Option<Reply> {
    let value = body_json(body)?;
    let task = text_of(&value, "task");
    // Adopting a chat that exists: the task is its next message.
    if let Some(id) = ["worker_id", "pane_id", "handle"].iter().find_map(|key| text_of(&value, key)) {
        if let Some(chat) = chat_of(&id) {
            return Some(match task {
                Some(_) => owned(&chat, session).and_then(|()| followup(&chat, &value, session, true)),
                None => Err(err("INVALID_TASK", "task is required", false)),
            });
        }
    }
    let lane = text_of(&value, "lane");
    let agent = text_of(&value, "agent_type");
    let saved = agent.as_deref().map(|a| LaneStore::in_app_data().lane_for(a)).unwrap_or_default();
    let has_definition = agent.as_deref().is_some_and(|a| crate::acp::config::find_builtin(a).is_some());
    let want = match wants(lane.as_deref(), saved, crate::acp::chat_enabled(), has_definition) {
        Ok(want) => want,
        Err(why) => return Some(Err(err("INVALID_ARGUMENT", why, false))),
    };
    // A visible/one-task choice the caller made on purpose (headless) is the terminal way's.
    let Want::Chat { fallback } = want else { return None };
    if fallback && value.get("headless").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let Some(agent) = agent else {
        return Some(Err(err("INVALID_ARGUMENT", "agent_type is required with lane: \"acp\"", false)));
    };
    let Some(task) = task else { return Some(Err(err("INVALID_TASK", "task is required", false))) };
    let folder = text_of(&value, "project_path").or_else(|| query.trim_start_matches('?').split('&').find_map(|p| p.strip_prefix("project_path=").map(str::to_string))).unwrap_or_else(|| deps.registry.lock().project_path.clone());
    let started = super::start_gated(deps, StartRequest { agent_id: agent, folder: folder.into(), explicit: !fallback });
    let chat = match started {
        Ok(chat) => chat,
        // The saved choice asked for a chat and it cannot start: the terminal way runs the task.
        Err(_) if fallback => return None,
        Err(error) => {
            let mut e = err(error.code, error.message.clone(), error.fallback);
            e.context = json!({ "fallback": error.fallback.then_some("terminal"), "hint": "Retry with lane: \"terminal\" to run it in a terminal." });
            return Some(Err(e));
        }
    };
    chat.set_started_by(session);
    deps.host.state_changed(chat.pane_id(), &chat.state());
    if let Err(error) = chat.send(&task) {
        return Some(Err(send_failure(error)));
    }
    let background = value.get("background").and_then(Value::as_bool).unwrap_or(false);
    if !background {
        let wait_ms = match wait_ms_of(&value, DEFAULT_WAIT_MS) {
            Ok(ms) => ms,
            Err(e) => return Some(Err(e)),
        };
        wait_status(&chat, &default_until(), wait_ms);
    }
    Some(Ok(view(&chat)))
}

fn wait(body: &[u8]) -> Option<Reply> {
    let value = body_json(body)?;
    let handles: Vec<String> = value.get("handles")?.as_array()?.iter().filter_map(Value::as_str).map(str::to_string).collect();
    let chats: Vec<Option<std::sync::Arc<AcpChat>>> = handles.iter().map(|h| chat_of(h)).collect();
    if !chats.iter().any(Option::is_some) {
        return None;
    }
    if chats.iter().any(Option::is_none) {
        return Some(Err(err(
            "MIXED_HANDLES",
            "These handles are a mix of chat sessions and terminal runs. Call wait_agents once for the chat sessions and once for the others.",
            false,
        )));
    }
    let chats: Vec<_> = chats.into_iter().flatten().collect();
    let until = until_of(&value);
    let wait_all = value.get("mode").and_then(Value::as_str) == Some("all");
    if value.get("mode").and_then(Value::as_str).is_some_and(|m| !matches!(m, "all" | "any")) {
        return Some(Err(err("INVALID_WAIT_MODE", "mode must be 'any' or 'all'", false)));
    }
    let wait_ms = match wait_ms_of(&value, DEFAULT_WAIT_MS) {
        Ok(ms) => ms,
        Err(e) => return Some(Err(e)),
    };
    let started = Instant::now();
    let deadline = started + Duration::from_millis(wait_ms);
    let mut reason = "timeout";
    loop {
        let hits = chats.iter().filter(|c| until.iter().any(|u| u == status_of(&c.session().snapshot())) || c.is_closed()).count();
        if (wait_all && hits == chats.len()) || (!wait_all && hits > 0) {
            reason = "status";
            break;
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(POLL);
    }
    let agents: Vec<Value> = chats.iter().map(|c| view(c)).collect();
    Some(Ok(json!({
        "agents": agents,
        "reason": reason,
        "mode": if wait_all { "all" } else { "any" },
        "waited_ms": started.elapsed().as_millis() as u64,
        "requested_wait_ms": wait_ms,
    })))
}

/// `followup_task` (`next_turn`: a new turn, then waits like a first run) and `send_message` (steers the turn that
/// runs).
fn followup(chat: &AcpChat, value: &Value, session: Option<&str>, next_turn: bool) -> Reply {
    owned(chat, session)?;
    let task = text_of(value, "task").or_else(|| text_of(value, "message")).ok_or_else(|| err("INVALID_TASK", "task is required", false))?;
    if chat.is_closed() {
        return Err(err("AGENT_CLOSED", "That chat session was closed.", false));
    }
    if next_turn {
        match chat.send(&task) {
            Ok(_) => {}
            Err(SendError::Busy) => return Err(send_failure(SendError::Busy)),
            Err(other) => return Err(send_failure(other)),
        }
        let wait_ms = wait_ms_of(value, DEFAULT_WAIT_MS)?;
        if value.get("background").and_then(Value::as_bool) != Some(true) {
            wait_status(chat, &default_until(), wait_ms);
        }
    } else if chat.readiness() == Readiness::Ready {
        // Nothing runs: a message is a new turn.
        chat.send(&task).map_err(send_failure)?;
    } else {
        match chat.steer(&task) {
            Some(Ok(())) => {}
            Some(Err(why)) => return Err(err("STEER_FAILED", why, true)),
            None => return Err(err("STEER_UNSUPPORTED", "This agent cannot take a message while it works. Wait for the turn to end, then followup_task.", true)),
        }
    }
    Ok(view(chat))
}

fn answer(chat: &AcpChat, value: &Value, session: Option<&str>) -> Reply {
    owned(chat, session)?;
    let prompt = text_of(value, "prompt_id").ok_or_else(|| err("INVALID_ARGUMENT", "answer_prompt needs prompt_id", false))?;
    let choice = text_of(value, "choice").ok_or_else(|| err("INVALID_ARGUMENT", "answer_prompt needs choice", false))?;
    match chat.answer_for_agent(&prompt, &choice, who(session)) {
        Ok(name) => {
            let mut done = view(chat);
            if let Some(map) = done.as_object_mut() {
                map.insert("answered".into(), json!({ "prompt_id": prompt, "choice": choice, "name": name }));
            }
            Ok(done)
        }
        Err(AgentAnswerError::NotAllowed) => Err(err(
            "AGENT_ANSWER_NOT_ALLOWED",
            "The person has not let agents answer questions in this chat session, so only they can. Nothing was answered. Tell them what you need (ask_user) and wait; do not retry in a loop.",
            false,
        )),
        Err(AgentAnswerError::NoSuchQuestion) => Err(err("NO_OPEN_QUESTION", "That question is not open any more (it was answered, or the agent stopped asking). Read the chat again.", true)),
        Err(AgentAnswerError::BadChoice(offered)) => {
            let mut e = err("INVALID_CHOICE", format!("{choice:?} is not one of the options the agent offered"), true);
            e.context = json!({ "options": offered });
            Err(e)
        }
        Err(AgentAnswerError::Closed) => Err(err("AGENT_CLOSED", "That chat session was closed.", false)),
        Err(AgentAnswerError::Failed(why)) => Err(err("ANSWER_FAILED", why, true)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_callers_word_decides_and_otherwise_the_saved_choice() {
        assert_eq!(wants(Some("acp"), RunLane::Terminal, true, true), Ok(Want::Chat { fallback: false }));
        assert_eq!(wants(Some("terminal"), RunLane::Acp, true, true), Ok(Want::Terminal));
        assert_eq!(wants(None, RunLane::Acp, true, true), Ok(Want::Chat { fallback: true }));
        assert_eq!(wants(None, RunLane::Terminal, true, true), Ok(Want::Terminal));
        // The saved choice needs Labs and a definition; without them it is the terminal.
        assert_eq!(wants(None, RunLane::Acp, false, true), Ok(Want::Terminal));
        assert_eq!(wants(None, RunLane::Acp, true, false), Ok(Want::Terminal));
        assert!(wants(Some("pty"), RunLane::Terminal, true, true).is_err());
    }

    #[test]
    fn final_statuses_end_a_wait_and_running_ones_do_not() {
        for status in ["completed", "failed", "cancelled"] {
            assert!(is_final(status));
        }
        for status in ["running", "waiting_input", "idle"] {
            assert!(!is_final(status));
        }
    }

    #[test]
    fn a_prompt_names_the_options_and_who_may_answer() {
        let state = json!({
            "agentAnswering": false,
            "permissions": [{ "askId": 3, "questionId": "q1", "title": "Write notes.txt", "toolKind": "edit", "options": [{ "id": "allow_once", "name": "Allow Once", "kind": "allow_once" }] }]
        });
        let prompt = prompt_of(&state);
        assert_eq!(prompt["prompt_id"], "q1");
        assert_eq!(prompt["options"][0]["id"], "allow_once");
        assert_eq!(prompt["answerable_by_agent"], false);
        assert_eq!(prompt_of(&json!({ "permissions": [] })), Value::Null);
    }
}
