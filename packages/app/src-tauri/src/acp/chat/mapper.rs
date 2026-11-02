//! ACP session events -> chat items (`ChatItem` v2). Pure: no I/O, no clock of its own (the caller passes the
//! time), so the golden tests feed it recorded traffic and compare the items.
//!
//! | ACP | chat item |
//! |---|---|
//! | `agent_message_chunk` | `Assistant`, one per message: chunks of the same message grow one item (same id) |
//! | `agent_thought_chunk` | nothing: `ChatItem` has no thinking item |
//! | `tool_call`, `tool_call_update` | `ToolCall` (title, kind, files, diff) and, once finished, `ToolResult` |
//! | `plan` | `Plan` (the whole list, `replace`), one per turn |
//! | `usage_update` | `Usage` (context in use / window) |
//! | `session_info_update` (title), `compaction_update`, `notice` | `Notice` |
//! | `config_option_update` (model) | `Session` |
//! | stop reason | `TurnEnd` with the duration, plus one `Notice` (kind `ending:<reason>`, see `ending.rs`) that says how the turn ended |
//! | `available_commands_update`, `current_mode_update` | nothing here: they are session state (`AcpChat::state`) |
//!
//! Ids are stable so an item sent again replaces its old copy: `u<turn>` (the person's message), `a<turn>.<n>`
//! (the n-th assistant message of the turn), `t:<call>` and `r:<call>` (a tool call and its result), `plan:<turn>`,
//! `end:<turn>`, `usage`, `session`, `title`. History replayed by `session/load` gets an `h` prefix on the
//! turn-based ids, so it never meets the live turns of the same pane. A replay is mapped only into an empty
//! chat: when the chat already has items the replay is the same conversation again and is dropped.

use std::collections::{BTreeSet, HashMap};
use std::time::Instant;

use serde_json::{json, Value};

use crate::acp::session::{SessionEvent, StartKind};
use crate::acp::chat::ending::{self, EndReason, TurnFacts};
use crate::acp::state::{ExitKind, ExitSummary, TurnSummary};
use crate::acp::types::{ContentChunk, SessionUpdate, ToolCall, ToolCallUpdate, ToolStatus};
use crate::session_chat::detail::{bound_input, cut_to, paths_from_input, PlanItem, PlanStatus, ToolDetail};
use crate::session_chat::model::{cut, ChatItem, TurnOutcome};

/// Longest assistant message kept, in characters. A runaway message is cut, not allowed to grow forever.
pub const MAX_TEXT_CHARS: usize = 200_000;
/// Longest one-line summary of a tool call.
const MAX_SUMMARY_CHARS: usize = 300;
/// The fallback reason for a turn closed because the chat was closed.
const CHAT_CLOSED: &str = "The chat was closed.";
const TRUNCATED: &str = "\n… (cut: the message was too long)";

struct Segment {
    id: String,
    text: String,
    message_id: Option<String>,
    cut: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct Diff {
    path: String,
    old: Option<String>,
    new: String,
}

struct Tool {
    call_id: String,
    kind: String,
    title: String,
    status: ToolStatus,
    paths: Vec<String>,
    raw_input: Option<Value>,
    diffs: Vec<Diff>,
    text: String,
    raw_output: Option<Value>,
    /// A `tool_call` (or the first sight of an update) showed it in the chat.
    shown: bool,
    /// A `ToolResult` was sent for it.
    resulted: bool,
}

impl Tool {
    fn new(call_id: &str) -> Self {
        Tool {
            call_id: call_id.to_string(),
            kind: "other".into(),
            title: String::new(),
            status: ToolStatus::Pending,
            paths: Vec::new(),
            raw_input: None,
            diffs: Vec::new(),
            text: String::new(),
            raw_output: None,
            shown: false,
            resulted: false,
        }
    }

    fn add_path(&mut self, path: &str) {
        let path = path.trim();
        if !path.is_empty() && !self.paths.iter().any(|known| known == path) {
            self.paths.push(path.to_string());
        }
    }

    fn absorb_locations(&mut self, locations: &[Value]) {
        for location in locations {
            if let Some(path) = location.get("path").and_then(Value::as_str) {
                self.add_path(path);
            }
        }
    }

    /// The content of a call or an update replaces what the call showed before: its text is the newest. The diff
    /// of an edit stays until another diff replaces it, so a refused or failed edit still shows what it proposed.
    fn absorb_content(&mut self, content: &[Value]) {
        let mut diffs = Vec::new();
        self.text.clear();
        for block in content {
            match block.get("type").and_then(Value::as_str) {
                Some("diff") => {
                    let path = block.get("path").and_then(Value::as_str).unwrap_or_default().to_string();
                    let new = block.get("newText").and_then(Value::as_str).unwrap_or_default().to_string();
                    let old = block.get("oldText").and_then(Value::as_str).map(str::to_string);
                    self.add_path(&path);
                    diffs.push(Diff { path, old, new });
                }
                Some("content") => {
                    if let Some(text) = block.pointer("/content/text").and_then(Value::as_str) {
                        if !self.text.is_empty() {
                            self.text.push('\n');
                        }
                        self.text.push_str(text);
                    }
                }
                _ => {}
            }
        }
        if !diffs.is_empty() {
            self.diffs = diffs;
        }
    }

    fn apply_call(&mut self, call: &ToolCall) {
        self.kind = call.kind.clone();
        if !call.title.is_empty() {
            self.title = call.title.clone();
        }
        self.status = call.status;
        self.absorb_locations(&call.locations);
        if !call.content.is_empty() {
            self.absorb_content(&call.content);
        }
        if call.raw_input.is_some() {
            self.raw_input = call.raw_input.clone();
        }
        if call.raw_output.is_some() {
            self.raw_output = call.raw_output.clone();
        }
    }

    fn apply_update(&mut self, update: &ToolCallUpdate) {
        if let Some(kind) = &update.kind {
            self.kind = kind.clone();
        }
        if let Some(title) = update.title.as_ref().filter(|t| !t.is_empty()) {
            self.title = title.clone();
        }
        if let Some(status) = update.status {
            self.status = status;
        }
        if let Some(locations) = &update.locations {
            self.absorb_locations(locations);
        }
        if let Some(content) = &update.content {
            self.absorb_content(content);
        }
        if update.raw_input.is_some() {
            self.raw_input = update.raw_input.clone();
        }
        if update.raw_output.is_some() {
            self.raw_output = update.raw_output.clone();
        }
    }

    fn call_item(&self, at: &str) -> ChatItem {
        let mut paths = self.paths.clone();
        if let Some(input) = &self.raw_input {
            for path in paths_from_input(input) {
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
        }
        let input = if self.diffs.is_empty() { self.raw_input.as_ref().and_then(bound_input) } else { bound_input(&diff_input(&self.diffs)) };
        let summary = if self.title.trim().is_empty() { self.kind.clone() } else { cut_to(self.title.trim(), MAX_SUMMARY_CHARS) };
        ChatItem::ToolCall {
            id: format!("t:{}", self.call_id),
            at: Some(at.to_string()),
            call_id: self.call_id.clone(),
            name: view_name(&self.kind),
            summary,
            detail: ToolDetail { input, paths, writes: matches!(self.kind.as_str(), "edit" | "delete" | "move"), helper: false },
        }
    }

    fn result_item(&self, at: &str, ok: bool, output: String) -> ChatItem {
        ChatItem::ToolResult { id: format!("r:{}", self.call_id), at: Some(at.to_string()), call_id: self.call_id.clone(), ok, output: cut(&output) }
    }

    /// What the finished call reports: its text content, else its raw output.
    fn output_text(&self) -> String {
        let text = unfence(self.text.trim());
        if !text.is_empty() {
            return text;
        }
        match &self.raw_output {
            Some(Value::String(text)) => unfence(text.trim()),
            Some(Value::Null) | None => String::new(),
            Some(Value::Object(map)) => ["stdout", "output", "formatted_output", "aggregated_output", "text"]
                .iter()
                .find_map(|key| map.get(*key).and_then(Value::as_str).filter(|t| !t.trim().is_empty()))
                .map(|t| t.trim().to_string())
                .unwrap_or_else(|| Value::Object(map.clone()).to_string()),
            Some(other) => other.to_string(),
        }
    }
}

/// An agent's tool kind in the words the chat view's tool table knows (`lib/chatToolKinds.ts`).
fn view_name(kind: &str) -> String {
    match kind {
        "execute" => "run".to_string(),
        "delete" | "move" => "edit".to_string(),
        "" => "tool".to_string(),
        other => other.to_string(),
    }
}

/// The most diffs one tool call shows. A call that edits more files shows the first ones and says how many it left
/// out, because the view's input limit (`bound_input`) would otherwise drop the whole diff.
pub const MAX_SHOWN_DIFFS: usize = 8;

/// The input an edit call carries so the view can draw its diff (`old_string` / `new_string`, or a list of them).
fn diff_input(diffs: &[Diff]) -> Value {
    let one = |diff: &Diff| json!({ "file_path": diff.path, "old_string": diff.old.clone().unwrap_or_default(), "new_string": diff.new });
    match diffs {
        [single] => one(single),
        many => {
            let mut input = json!({ "edits": many.iter().take(MAX_SHOWN_DIFFS).map(one).collect::<Vec<_>>() });
            if many.len() > MAX_SHOWN_DIFFS {
                input["omitted_edits"] = json!(many.len() - MAX_SHOWN_DIFFS);
            }
            input
        }
    }
}

/// Text some adapters wrap in a code fence on its own lines: the fence is formatting, not output.
fn unfence(text: &str) -> String {
    let inner = text.strip_prefix("```").and_then(|rest| rest.strip_suffix("```"));
    match inner {
        Some(inner) => {
            let inner = inner.strip_prefix('\n').unwrap_or(inner);
            inner.strip_suffix('\n').unwrap_or(inner).to_string()
        }
        None => text.to_string(),
    }
}

/// The words of one content block as the chat shows them: text as it is, other blocks as a short mark.
fn chunk_words(chunk: &ContentChunk) -> Option<String> {
    if let Some(text) = chunk.text() {
        return Some(text.to_string());
    }
    match chunk.content.get("type").and_then(Value::as_str) {
        Some("image") => Some("[image]".into()),
        Some("audio") => Some("[audio]".into()),
        Some("resource_link") => Some(format!("[{}]", chunk.content.get("name").and_then(Value::as_str).unwrap_or("file"))),
        Some("resource") => Some("[attached file]".into()),
        _ => None,
    }
}

struct OpenTurn {
    n: u64,
    started: Instant,
}

pub struct Mapper {
    turn: Option<OpenTurn>,
    /// The number of the last live turn.
    turn_no: u64,
    /// Turn number of the replayed history (a count of replayed user messages).
    replay_turn: u64,
    seg_n: u32,
    assistant: Option<Segment>,
    /// The last assistant message of the turn, for its `final_answer` mark.
    last_assistant: Option<(ChatItem, u32)>,
    user: Option<Segment>,
    tools: HashMap<String, Tool>,
    tool_order: Vec<String>,
    notice_n: u64,
    steer_n: u64,
    ignore_replay: bool,
    emitted: bool,
    model: Option<String>,
    agent_version: Option<String>,
    agent_label: Option<String>,
    /// The end of the last turn already reported, so an exit after it adds nothing.
    last_ended: Option<u64>,
    /// Turns that came from a journal (an earlier run of the app): live turn numbers start after them, so a new
    /// `u1` or `end:1` never replaces an item of the old chat.
    turn_offset: u64,
    /// Files this turn changed (edits, deletes, moves), for "Done · 3 files".
    turn_files: BTreeSet<String>,
    /// The turn showed the person an answer or a tool call.
    turn_visible: bool,
    /// The quiet window, in minutes, for "No answer for 5 min".
    quiet_minutes: u64,
}

impl Default for Mapper {
    fn default() -> Self {
        Self::new()
    }
}

impl Mapper {
    pub fn new() -> Self {
        Mapper {
            turn: None,
            turn_no: 0,
            replay_turn: 0,
            seg_n: 0,
            assistant: None,
            last_assistant: None,
            user: None,
            tools: HashMap::new(),
            tool_order: Vec::new(),
            notice_n: 0,
            steer_n: 0,
            ignore_replay: false,
            emitted: false,
            model: None,
            agent_version: None,
            agent_label: None,
            last_ended: None,
            turn_offset: 0,
            turn_files: BTreeSet::new(),
            turn_visible: false,
            quiet_minutes: 5,
        }
    }

    pub fn with_quiet_secs(secs: u64) -> Self {
        let mut mapper = Self::new();
        mapper.set_quiet_secs(secs);
        mapper
    }

    /// The quiet window of the session, so an ending can say how long the agent was quiet.
    pub fn set_quiet_secs(&mut self, secs: u64) {
        self.quiet_minutes = (secs / 60).max(1);
    }

    /// The chat already has `items` from a journal (an earlier run of the app): the replay of the agent's own
    /// history is the same conversation again and is dropped, and new turns, notices and steers are numbered after
    /// the ones in `items`, so nothing of the old chat is replaced.
    pub fn resume_after(&mut self, items: &[ChatItem]) {
        if items.is_empty() {
            return;
        }
        self.emitted = true;
        let number = |s: &str| -> Option<u64> {
            let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        };
        for item in items {
            let id = item.id();
            let found = if let Some(rest) = id.strip_prefix("end:").or_else(|| id.strip_prefix("plan:")) {
                number(rest).map(|n| (n, false))
            } else if let Some(rest) = id.strip_prefix("n:") {
                rest.rsplit(':').next().and_then(number).map(|n| (n, true))
            } else if let Some(rest) = id.strip_prefix('u').or_else(|| id.strip_prefix('a')) {
                number(rest).map(|n| (n, false))
            } else {
                None
            };
            match found {
                Some((n, true)) => self.notice_n = self.notice_n.max(n),
                Some((n, false)) => self.turn_offset = self.turn_offset.max(n),
                None => {}
            }
        }
        self.turn_no = self.turn_offset;
    }

    /// The title a tool call showed, for a card that names it.
    pub fn tool_title(&self, call_id: &str) -> Option<String> {
        self.tools.get(call_id).map(|t| t.title.clone()).filter(|t| !t.is_empty())
    }

    /// The kind and files of a tool call, for a card that names it.
    pub fn tool_brief(&self, call_id: &str) -> Option<(String, Vec<String>)> {
        self.tools.get(call_id).map(|t| (t.kind.clone(), t.paths.clone()))
    }

    /// A turn is open (started, not yet ended).
    pub fn turn_open(&self) -> bool {
        self.turn.is_some()
    }

    fn note(&mut self, items: Vec<ChatItem>) -> Vec<ChatItem> {
        if !items.is_empty() {
            self.emitted = true;
        }
        items
    }

    /// `(id prefix, turn number)` of the turn an item belongs to.
    fn ctx(&self, replay: bool) -> (&'static str, u64) {
        if replay {
            ("h", self.replay_turn)
        } else {
            ("", self.turn.as_ref().map(|t| t.n).unwrap_or(self.turn_no))
        }
    }

    fn notice(&mut self, kind: &str, text: String, at: &str) -> ChatItem {
        self.notice_n += 1;
        ChatItem::Notice { id: format!("n:{}:{}", kind, self.notice_n), at: Some(at.to_string()), notice_kind: kind.to_string(), text, url: None }
    }

    // ---- the person's side ---------------------------------------------------------------------------------

    /// A turn starts. `user_text`: what the person sent, shown as their message.
    pub fn turn_started(&mut self, turn: u64, user_text: Option<&str>, now: Instant, at: &str) -> Vec<ChatItem> {
        let mut out = Vec::new();
        if self.turn.is_some() {
            // The last turn never reported its end: close it so the items stay consistent.
            out.extend(self.finish_turn(None, Some("The turn did not report an end."), now, at));
        }
        let turn = turn + self.turn_offset;
        self.turn = Some(OpenTurn { n: turn, started: now });
        self.turn_no = turn;
        self.seg_n = 0;
        self.steer_n = 0;
        self.assistant = None;
        self.last_assistant = None;
        self.turn_files.clear();
        self.turn_visible = false;
        if let Some(text) = user_text.filter(|t| !t.is_empty()) {
            out.push(ChatItem::User { id: format!("u{turn}"), at: Some(at.to_string()), text: text.to_string() });
        }
        self.note(out)
    }

    /// Text sent into the turn that runs (steering): shown as another message of the person, in the turn.
    pub fn steered(&mut self, text: &str, at: &str) -> Vec<ChatItem> {
        let turn = self.turn.as_ref().map(|t| t.n).unwrap_or(self.turn_no);
        self.steer_n += 1;
        // What the agent says next is a new message after the person's.
        self.assistant = None;
        self.note(vec![ChatItem::User { id: format!("u{turn}.s{}", self.steer_n), at: Some(at.to_string()), text: text.to_string() }])
    }

    // ---- permission asks -----------------------------------------------------------------------------------

    /// The notice that stands for a permission question in the chat: waiting, then what was answered.
    pub fn permission_notice(&mut self, ask_id: u64, text: String, at: &str) -> Vec<ChatItem> {
        self.assistant = None;
        self.note(vec![ChatItem::Notice { id: format!("perm:{ask_id}"), at: Some(at.to_string()), notice_kind: "permission".into(), text, url: None }])
    }

    // ---- events --------------------------------------------------------------------------------------------

    pub fn on_event(&mut self, event: &SessionEvent, now: Instant, at: &str) -> Vec<ChatItem> {
        match event {
            SessionEvent::Started { how, caps } => {
                // A restored session replays its history: into an empty chat it is the chat, otherwise it is a copy.
                self.ignore_replay = self.emitted;
                self.agent_version = caps.agent_version.clone();
                self.agent_label = caps.agent_title.clone().or_else(|| caps.agent_name.clone());
                let mut out = vec![self.session_item(at)];
                if *how == StartKind::HistoryLost {
                    out.push(self.notice("note", "The earlier conversation could not be brought back. This is a new one.".into(), at));
                }
                self.note(out)
            }
            SessionEvent::Update { update, replay } => {
                let out = self.update(update, *replay, at);
                self.note(out)
            }
            SessionEvent::TurnEnded(summary) => {
                let out = self.finish_turn(Some(summary), None, now, at);
                self.note(out)
            }
            SessionEvent::Exited(exit) => {
                let out = self.exited(exit, now, at);
                self.note(out)
            }
            SessionEvent::Snapshot(_)
            | SessionEvent::HistoryLost(_)
            | SessionEvent::UpdateUnparsed { .. }
            | SessionEvent::Extension { .. }
            | SessionEvent::PermissionRequested(_)
            | SessionEvent::PermissionResolved { .. }
            | SessionEvent::TurnStarted { .. }
            | SessionEvent::Slept => Vec::new(),
        }
    }

    fn session_item(&self, at: &str) -> ChatItem {
        ChatItem::Session { id: "session".into(), at: Some(at.to_string()), model: self.model.clone(), cli_version: self.agent_version.clone() }
    }

    fn update(&mut self, update: &SessionUpdate, replay: bool, at: &str) -> Vec<ChatItem> {
        if replay && self.ignore_replay {
            return Vec::new();
        }
        // In a replay, anything but the person's words ends the person's message.
        if replay && !matches!(update, SessionUpdate::UserMessageChunk(_)) {
            self.user = None;
        }
        match update {
            SessionUpdate::UserMessageChunk(chunk) if replay => self.user_chunk(chunk, at),
            // A live agent does not echo the person's message; one that does is not shown twice.
            SessionUpdate::UserMessageChunk(_) => Vec::new(),
            SessionUpdate::AgentMessageChunk(chunk) => self.agent_chunk(chunk, replay, at),
            SessionUpdate::AgentThoughtChunk(_) => Vec::new(),
            SessionUpdate::ToolCall(call) => self.tool_call(call, at),
            SessionUpdate::ToolCallUpdate(update) => self.tool_update(update, at),
            SessionUpdate::Plan(plan) => self.plan(&plan.entries, replay, at),
            SessionUpdate::UsageUpdate(usage) => {
                vec![ChatItem::Usage { id: "usage".into(), at: Some(at.to_string()), input_tokens: usage.used, output_tokens: 0, context_window: Some(usage.size).filter(|s| *s > 0) }]
            }
            SessionUpdate::SessionInfoUpdate(info) => match info.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                Some(title) => vec![ChatItem::Notice { id: "title".into(), at: Some(at.to_string()), notice_kind: "title".into(), text: cut_to(title, 200), url: None }],
                None => Vec::new(),
            },
            SessionUpdate::ConfigOptionUpdate(update) => self.config_options(&update.config_options, at),
            SessionUpdate::CurrentModeUpdate(_) | SessionUpdate::AvailableCommandsUpdate(_) => Vec::new(),
            SessionUpdate::Notice(raw) => match notice_words(raw) {
                Some(text) => {
                    self.assistant = None;
                    vec![self.notice("note", text, at)]
                }
                None => Vec::new(),
            },
            SessionUpdate::CompactionUpdate(_) => {
                self.assistant = None;
                vec![ChatItem::Notice { id: "n:compacted".into(), at: Some(at.to_string()), notice_kind: "compacted".into(), text: "The conversation was compacted to save room.".into(), url: None }]
            }
            SessionUpdate::CompactionSummaryChunk(_) => Vec::new(),
        }
    }

    /// The settings the agent offers, as the session knows them (from `session/new`, `session/load` and what the
    /// person changed): the chat's header names the model from them, also before any `config_option_update`.
    pub fn observe_config_options(&mut self, options: &[Value], at: &str) -> Vec<ChatItem> {
        let items = self.config_options(options, at);
        self.note(items)
    }

    /// The model the agent says it uses now, from its config options (`category: "model"`).
    fn config_options(&mut self, options: &[Value], at: &str) -> Vec<ChatItem> {
        let model = options.iter().find(|o| o.get("category").and_then(Value::as_str) == Some("model")).and_then(|o| {
            let current = o.get("currentValue").and_then(Value::as_str)?;
            // The option's own name for the value, when it lists one.
            let named = o
                .get("options")
                .and_then(Value::as_array)
                .and_then(|list| list.iter().find(|v| v.get("value").and_then(Value::as_str) == Some(current)))
                .and_then(|v| v.get("name").and_then(Value::as_str));
            Some(named.unwrap_or(current).to_string())
        });
        match model {
            Some(model) if self.model.as_deref() != Some(model.as_str()) => {
                self.model = Some(model);
                vec![self.session_item(at)]
            }
            _ => Vec::new(),
        }
    }

    fn user_chunk(&mut self, chunk: &ContentChunk, at: &str) -> Vec<ChatItem> {
        let Some(words) = chunk_words(chunk) else { return Vec::new() };
        // A replayed user message after the agent's reply is a new turn of the history.
        if self.user.is_none() {
            self.replay_turn += 1;
            self.seg_n = 0;
            self.assistant = None;
            self.user = Some(Segment { id: format!("hu{}", self.replay_turn), text: String::new(), message_id: None, cut: false });
        }
        let Some(seg) = self.user.as_mut() else { return Vec::new() };
        push_text(seg, &words);
        vec![ChatItem::User { id: seg.id.clone(), at: Some(at.to_string()), text: seg.text.clone() }]
    }

    fn agent_chunk(&mut self, chunk: &ContentChunk, replay: bool, at: &str) -> Vec<ChatItem> {
        let Some(words) = chunk_words(chunk) else { return Vec::new() };
        if words.is_empty() {
            return Vec::new();
        }
        if replay {
            // The agent's reply ends the replayed user message.
            self.user = None;
        }
        let (prefix, n) = self.ctx(replay);
        let new_message = match &self.assistant {
            None => true,
            Some(seg) => chunk.message_id.is_some() && seg.message_id.is_some() && chunk.message_id != seg.message_id,
        };
        if new_message {
            self.seg_n += 1;
            self.assistant = Some(Segment { id: format!("{prefix}a{n}.{}", self.seg_n), text: String::new(), message_id: chunk.message_id.clone(), cut: false });
        }
        let seg_n = self.seg_n;
        let Some(seg) = self.assistant.as_mut() else { return Vec::new() };
        if seg.message_id.is_none() {
            seg.message_id = chunk.message_id.clone();
        }
        push_text(seg, &words);
        // Control tokens a model leaked are not part of the answer.
        let shown = ending::strip_leaked_markup(&seg.text);
        if shown.trim().is_empty() {
            return Vec::new();
        }
        if !replay {
            self.turn_visible = true;
        }
        let item = ChatItem::Assistant { id: seg.id.clone(), at: Some(at.to_string()), text: shown, model: self.model.clone(), final_answer: false };
        self.last_assistant = Some((item.clone(), seg_n));
        vec![item]
    }

    fn tool_call(&mut self, call: &ToolCall, at: &str) -> Vec<ChatItem> {
        self.assistant = None;
        let mut tool = self.tools.remove(&call.tool_call_id).unwrap_or_else(|| Tool::new(&call.tool_call_id));
        tool.apply_call(call);
        self.finish_tool_update(tool, at)
    }

    fn tool_update(&mut self, update: &ToolCallUpdate, at: &str) -> Vec<ChatItem> {
        let first_sight = !self.tools.contains_key(&update.tool_call_id);
        if first_sight {
            self.assistant = None;
        }
        let mut tool = self.tools.remove(&update.tool_call_id).unwrap_or_else(|| Tool::new(&update.tool_call_id));
        tool.apply_update(update);
        self.finish_tool_update(tool, at)
    }

    /// Puts the tool back and tells what changed: its call (always: it may have a title now), and its result once
    /// it is finished.
    fn finish_tool_update(&mut self, mut tool: Tool, at: &str) -> Vec<ChatItem> {
        if self.turn.is_some() {
            self.turn_visible = true;
            let changes_files = !tool.diffs.is_empty() || matches!(tool.kind.as_str(), "edit" | "delete" | "move");
            if changes_files && tool.status == ToolStatus::Completed {
                for path in &tool.paths {
                    self.turn_files.insert(path.clone());
                }
                for diff in &tool.diffs {
                    self.turn_files.insert(diff.path.clone());
                }
            }
        }
        let mut out = vec![tool.call_item(at)];
        if !tool.shown {
            tool.shown = true;
            if !self.tool_order.contains(&tool.call_id) {
                self.tool_order.push(tool.call_id.clone());
            }
        }
        if tool.status.is_finished() {
            tool.resulted = true;
            out.push(tool.result_item(at, tool.status == ToolStatus::Completed, tool.output_text()));
        }
        self.tools.insert(tool.call_id.clone(), tool);
        out
    }

    fn plan(&mut self, entries: &[Value], replay: bool, at: &str) -> Vec<ChatItem> {
        self.assistant = None;
        let (prefix, n) = self.ctx(replay);
        let items = entries
            .iter()
            .filter_map(|entry| {
                let text = entry.get("content").and_then(Value::as_str)?.trim().to_string();
                if text.is_empty() {
                    return None;
                }
                let status = entry.get("status").and_then(Value::as_str).and_then(PlanStatus::parse).unwrap_or(PlanStatus::Pending);
                Some(PlanItem { text, status: Some(status), task_id: None })
            })
            .collect::<Vec<_>>();
        vec![ChatItem::Plan { id: format!("{prefix}plan:{n}"), at: Some(at.to_string()), items, replace: true }]
    }

    // ---- the end of a turn -----------------------------------------------------------------------------------

    /// Ends the open turn: closes the text, settles tools still open, marks the final answer, and says how it
    /// ended. `summary`: what the session reported; `None` when the turn is closed without a report (`fallback`
    /// is then the reason).
    fn finish_turn(&mut self, summary: Option<&TurnSummary>, fallback: Option<&str>, now: Instant, at: &str) -> Vec<ChatItem> {
        let Some(turn) = self.turn.take() else {
            // No turn is open: a second report of an end already shown changes nothing.
            return Vec::new();
        };
        self.assistant = None;
        self.user = None;
        let mut out = Vec::new();
        let duration_ms = now.checked_duration_since(turn.started).map(|d| d.as_millis() as u64);
        let facts = TurnFacts { files: self.turn_files.len(), visible: self.turn_visible, duration_ms, agent: self.agent_label.as_deref(), quiet_minutes: self.quiet_minutes };
        let end = match summary {
            Some(s) => ending::ending_of(s, &facts),
            None if fallback == Some(CHAT_CLOSED) => ending::Ending { reason: EndReason::Stopped, text: "The chat was closed".to_string() },
            None => ending::ending_without_report(fallback.unwrap_or("The turn ended."), &facts),
        };
        let outcome = outcome_of(end.reason);
        // Tools the agent never finished: closed, so nothing looks like it still runs.
        let leftover: Vec<String> = self.tool_order.iter().filter(|id| self.tools.get(*id).is_some_and(|t| !t.resulted)).cloned().collect();
        for id in leftover {
            if let Some(tool) = self.tools.get_mut(&id) {
                tool.resulted = true;
                let (ok, output) = if outcome == TurnOutcome::Completed { (true, String::new()) } else { (false, "Stopped before it finished.".to_string()) };
                out.push(tool.result_item(at, ok, output));
            }
        }
        // Tools of this turn are finished business: forget them so the table stays small.
        self.tools.retain(|_, tool| !tool.resulted);
        self.tool_order.retain(|id| self.tools.contains_key(id));
        if outcome == TurnOutcome::Completed {
            if let Some((ChatItem::Assistant { id, at, text, model, .. }, _)) = self.last_assistant.take() {
                out.push(ChatItem::Assistant { id, at, text, model, final_answer: true });
            }
        }
        self.last_assistant = None;
        // One grey line says how the turn ended; the view puts the buttons that fit it next to the words.
        out.push(self.notice(&end.reason.notice_kind(), end.text, at));
        out.push(ChatItem::TurnEnd { id: format!("end:{}", turn.n), at: Some(at.to_string()), outcome, duration_ms });
        self.last_ended = Some(turn.n);
        out
    }

    /// The adapter went away. A turn it left open ends as failed; the notice says what happened.
    fn exited(&mut self, exit: &ExitSummary, now: Instant, at: &str) -> Vec<ChatItem> {
        let reason = match exit.kind {
            ExitKind::Closed => return self.finish_turn(None, Some(CHAT_CLOSED), now, at),
            ExitKind::Crashed => match exit.code {
                Some(code) => format!("The agent stopped unexpectedly (exit code {code})."),
                None => "The agent stopped unexpectedly.".to_string(),
            },
            ExitKind::Hung => "The agent stopped answering and was stopped.".to_string(),
            ExitKind::WriteStalled => "The agent stopped reading its input and was stopped.".to_string(),
            ExitKind::StartFailed => exit.reason.clone().unwrap_or_else(|| "The agent could not start.".to_string()),
        };
        let had_turn = self.turn.is_some();
        let mut out = self.finish_turn(None, Some(&reason), now, at);
        if !had_turn {
            // Nothing was running: the notice is the only trace.
            out.push(self.notice("error", reason, at));
        }
        out
    }
}

/// The chat outcome an ending counts as.
fn outcome_of(reason: EndReason) -> TurnOutcome {
    match reason {
        EndReason::Done | EndReason::CutShort => TurnOutcome::Completed,
        EndReason::Stopped => TurnOutcome::Interrupted,
        EndReason::UsageLimit | EndReason::NoAnswer | EndReason::SignedOut | EndReason::Empty | EndReason::Declined | EndReason::Failed => TurnOutcome::Failed,
    }
}

fn push_text(seg: &mut Segment, words: &str) {
    if seg.cut {
        return;
    }
    seg.text.push_str(words);
    if seg.text.chars().count() > MAX_TEXT_CHARS {
        let end = seg.text.char_indices().nth(MAX_TEXT_CHARS).map(|(i, _)| i).unwrap_or(seg.text.len());
        seg.text.truncate(end);
        seg.text.push_str(TRUNCATED);
        seg.cut = true;
    }
}

/// A line for the person from a `notice` update, whatever shape the agent gave it.
fn notice_words(raw: &Value) -> Option<String> {
    if let Some(text) = raw.as_str() {
        return Some(cut_to(text.trim(), 300)).filter(|t| !t.is_empty());
    }
    ["message", "text", "title"].iter().find_map(|key| raw.get(*key).and_then(Value::as_str)).map(|t| cut_to(t.trim(), 300)).filter(|t| !t.is_empty())
}
