//! Channel protocol v1, the Puppet Master side (spec section 2). The brain is an HTTP client of the
//! bridge: it long-polls the inbox and posts events. The fixed wire format is written down in
//! `packages/potato-brain/src/channels/potato.ts`; this file reads and writes exactly that.
//!
//! - Every item and event carries `"v":1`. An unknown `type` is ignored on both sides.
//! - Reading is forgiving about what it does not know and strict about sizes: a post has at most 32
//!   events and 64 KB, a reply at most 6 segments of 2000 characters, and so on.
//! - An event that is known but malformed is dropped on its own, it never fails the whole post.
//!
//! Nothing here names an agent or a chat app. Pure: no files, no clock, no network.

use serde_json::{json, Map, Value};

pub const V: u64 = 1;
pub const MAX_EVENTS: usize = 32;
pub const MAX_POST_BYTES: usize = 64 * 1024;
pub const MAX_SEND_TEXT: usize = 4000;
pub const MAX_CONTEXT: usize = 6000;
pub const MAX_SUMMARY: usize = 1500;
pub const MAX_SEGMENTS: usize = 6;
pub const MAX_SEGMENT_CHARS: usize = 2000;
pub const MAX_TASK: usize = 600;
pub const MAX_PROJECT: usize = 120;
/// The longest the inbox long-poll holds a request.
pub const MAX_WAIT_MS: u64 = 25_000;

/// The moods the brain may report (`MOODS` in `persona/mood.ts`).
pub const MOODS: [&str; 4] = ["sleepy", "hungry", "genki", "tsundere"];

/// Why a post was refused as a whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostError {
    /// More than 64 KB: HTTP 413.
    TooBig,
    /// Not JSON, not `{"v":1,"events":[...]}`, or more than 32 events: HTTP 400.
    Invalid,
}

/// The error codes a brain may report. The response body of a failed model call never leaves the brain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    ProviderAuth,
    ProviderLimit,
    ProviderDown,
    Network,
    PersonaTooBig,
    SurfaceAuth,
    Internal,
}

impl ErrorCode {
    pub fn parse(raw: &str) -> ErrorCode {
        match raw {
            "PROVIDER_AUTH" => ErrorCode::ProviderAuth,
            "PROVIDER_LIMIT" => ErrorCode::ProviderLimit,
            "PROVIDER_DOWN" => ErrorCode::ProviderDown,
            "NETWORK" => ErrorCode::Network,
            "PERSONA_TOO_BIG" => ErrorCode::PersonaTooBig,
            "SURFACE_AUTH" => ErrorCode::SurfaceAuth,
            _ => ErrorCode::Internal,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::ProviderAuth => "PROVIDER_AUTH",
            ErrorCode::ProviderLimit => "PROVIDER_LIMIT",
            ErrorCode::ProviderDown => "PROVIDER_DOWN",
            ErrorCode::Network => "NETWORK",
            ErrorCode::PersonaTooBig => "PERSONA_TOO_BIG",
            ErrorCode::SurfaceAuth => "SURFACE_AUTH",
            ErrorCode::Internal => "INTERNAL",
        }
    }
}

/// One event from the brain.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Hello { protocol: u64, version: String, provider: String, packs: Vec<String>, surfaces: Vec<String> },
    Typing { conversation_id: String, on: bool },
    Reply { conversation_id: String, in_reply_to: String, segments: Vec<String>, kaomoji: Option<String>, tone: Option<String> },
    Mood { mood: String, intensity: u64, energy: u64, hunger: u64 },
    Usage { model: String, prompt_tokens: u64, completion_tokens: u64, cache_hit_tokens: u64, cache_miss_tokens: u64 },
    HelperRequest { request_id: String, conversation_id: String, task: String, project: Option<String> },
    Error { code: ErrorCode, conversation_id: Option<String>, in_reply_to: Option<String> },
    /// A type this version does not know (a later surface's `inbound`, or anything newer). Ignored.
    Unknown(String),
}

fn text(value: &Value, key: &str, max: usize) -> Option<String> {
    value.get(key)?.as_str().map(|s| s.chars().take(max).collect())
}

/// A token count: a non-negative number, never more than ten million (a bigger one is a bug or a lie).
fn count(value: &Value, key: &str) -> u64 {
    const CEILING: f64 = 10_000_000.0;
    value.get(key).and_then(Value::as_f64).filter(|n| n.is_finite() && *n > 0.0).map(|n| n.min(CEILING) as u64).unwrap_or(0)
}

fn strings(value: &Value, key: &str, max_items: usize, max_chars: usize) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).take(max_items).map(|s| s.chars().take(max_chars).collect()).collect())
        .unwrap_or_default()
}

/// Reads one event. `None` for a malformed event of a known type (it is dropped) or a wrong `v`.
pub fn parse_event(value: &Value) -> Option<Event> {
    if value.get("v").and_then(Value::as_u64) != Some(V) {
        return None;
    }
    let kind = value.get("type")?.as_str()?;
    Some(match kind {
        "hello" => Event::Hello {
            protocol: value.get("protocol").and_then(Value::as_u64).unwrap_or(0),
            version: text(value, "version", 40).unwrap_or_default(),
            provider: text(value, "provider", 40).unwrap_or_default(),
            packs: strings(value, "packs", 8, 40),
            surfaces: strings(value, "surfaces", 8, 40),
        },
        "typing" => Event::Typing { conversation_id: text(value, "conversationId", 200)?, on: value.get("on")?.as_bool()? },
        "reply" => {
            let segments: Vec<String> = strings(value, "segments", MAX_SEGMENTS, MAX_SEGMENT_CHARS)
                .into_iter()
                .filter(|s| !s.trim().is_empty())
                .collect();
            if segments.is_empty() {
                return None;
            }
            Event::Reply {
                conversation_id: text(value, "conversationId", 200)?,
                in_reply_to: text(value, "inReplyTo", 128).unwrap_or_default(),
                segments,
                kaomoji: text(value, "kaomoji", 40),
                tone: text(value, "tone", 24),
            }
        }
        "mood" => {
            let mood = text(value, "mood", 24)?;
            if !MOODS.contains(&mood.as_str()) {
                return None;
            }
            Event::Mood { mood, intensity: count(value, "intensity").min(3), energy: count(value, "energy").min(100), hunger: count(value, "hunger").min(100) }
        }
        "usage" => Event::Usage {
            model: text(value, "model", 80).unwrap_or_default(),
            prompt_tokens: count(value, "promptTokens"),
            completion_tokens: count(value, "completionTokens"),
            cache_hit_tokens: count(value, "cacheHitTokens"),
            cache_miss_tokens: count(value, "cacheMissTokens"),
        },
        "helper_request" => {
            let task = text(value, "task", MAX_TASK)?;
            if task.trim().is_empty() {
                return None;
            }
            Event::HelperRequest {
                request_id: text(value, "requestId", 128).filter(|s| !s.is_empty())?,
                conversation_id: text(value, "conversationId", 200)?,
                task,
                project: text(value, "project", MAX_PROJECT).filter(|s| !s.trim().is_empty()),
            }
        }
        "error" => Event::Error {
            code: ErrorCode::parse(value.get("code").and_then(Value::as_str).unwrap_or("")),
            conversation_id: text(value, "conversationId", 200),
            in_reply_to: text(value, "inReplyTo", 128),
        },
        other => Event::Unknown(other.chars().take(40).collect()),
    })
}

/// Reads the body of `POST /potato/brain/events`: `{"v":1,"events":[...]}`, at most 32 events and 64 KB.
pub fn parse_post(body: &[u8]) -> Result<Vec<Event>, PostError> {
    if body.len() > MAX_POST_BYTES {
        return Err(PostError::TooBig);
    }
    let value: Value = serde_json::from_slice(body).map_err(|_| PostError::Invalid)?;
    if value.get("v").and_then(Value::as_u64) != Some(V) {
        return Err(PostError::Invalid);
    }
    let events = value.get("events").and_then(Value::as_array).ok_or(PostError::Invalid)?;
    if events.len() > MAX_EVENTS {
        return Err(PostError::Invalid);
    }
    Ok(events.iter().filter_map(parse_event).collect())
}

/// How one trust level is written on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    OwnerPc,
    OwnerRemote,
    Others,
}

impl Trust {
    pub fn as_str(self) -> &'static str {
        match self {
            Trust::OwnerPc => "owner_pc",
            Trust::OwnerRemote => "owner_remote",
            Trust::Others => "others",
        }
    }

    pub fn is_owner(self) -> bool {
        self != Trust::Others
    }
}

/// What memory a turn may touch (`tools` on a `send`). The brain treats a missing field as `no_memory`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tools {
    Full,
    NoMemory,
}

impl Tools {
    pub fn as_str(self) -> &'static str {
        match self {
            Tools::Full => "full",
            Tools::NoMemory => "no_memory",
        }
    }
}

/// Why a brain pauses.
/// Some reasons are first used by later pieces (a stopped bot, a missing key); the wire words are fixed now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum PauseReason {
    DailyLimit,
    StoppedByYou,
    KeyMissing,
}

impl PauseReason {
    pub fn as_str(self) -> &'static str {
        match self {
            PauseReason::DailyLimit => "daily_limit",
            PauseReason::StoppedByYou => "stopped_by_you",
            PauseReason::KeyMissing => "key_missing",
        }
    }
}

/// What a helper did, told to the brain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperStatus {
    Started,
    Declined,
    Finished,
    Failed,
    Stopped,
    Refused,
}

impl HelperStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            HelperStatus::Started => "started",
            HelperStatus::Declined => "declined",
            HelperStatus::Finished => "finished",
            HelperStatus::Failed => "failed",
            HelperStatus::Stopped => "stopped",
            HelperStatus::Refused => "refused",
        }
    }
}

fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn item(kind: &str, fields: Value) -> Value {
    let mut map = Map::new();
    map.insert("v".into(), json!(V));
    map.insert("type".into(), json!(kind));
    if let Value::Object(fields) = fields {
        map.extend(fields);
    }
    Value::Object(map)
}

/// The facts of one `send`. `text` and `context` must already be redacted by the caller.
pub struct SendFacts<'a> {
    pub id: &'a str,
    pub conversation_id: &'a str,
    pub surface: &'a str,
    pub from_id: &'a str,
    pub from_name: &'a str,
    pub trust: Trust,
    pub text: &'a str,
    pub context: Option<&'a str>,
    pub tools: Tools,
}

pub fn send_item(s: &SendFacts) -> Value {
    let mut fields = json!({
        "id": s.id,
        "conversationId": s.conversation_id,
        "surface": s.surface,
        "from": { "id": s.from_id, "name": s.from_name, "trust": s.trust.as_str() },
        "text": cut(s.text, MAX_SEND_TEXT),
        "tools": s.tools.as_str(),
    });
    if let (Some(context), Value::Object(map)) = (s.context, &mut fields) {
        map.insert("context".into(), json!(cut(context, MAX_CONTEXT)));
    }
    item("send", fields)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn persona_item(pack: &str) -> Value {
    item("persona", json!({ "pack": pack }))
}

/// `summary` must already be redacted.
pub fn helper_update_item(request_id: &str, status: HelperStatus, summary: &str) -> Value {
    item("helper_update", json!({ "requestId": request_id, "status": status.as_str(), "summary": cut(summary, MAX_SUMMARY) }))
}

pub fn pause_item(reason: PauseReason) -> Value {
    item("pause", json!({ "reason": reason.as_str() }))
}

pub fn resume_item() -> Value {
    item("resume", json!({}))
}

pub fn shutdown_item() -> Value {
    item("shutdown", json!({}))
}
