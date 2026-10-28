//! Telegram Bot API client: the `Api` trait, `HttpApi` over ureq, and the error mapping.
//!
//! Only this module (and its `api/` submodules) knows Telegram's JSON. What it returns is the
//! generic shape the engine uses. Errors carry no text from Telegram or from ureq: a ureq error
//! names the request URL, and the URL holds the bot token, so ureq errors are never formatted.

use crate::redact::Redacted;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

mod wire;
#[cfg(test)]
mod tests;

/// Connect timeout for every call.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Read timeout for every call. Longer than the poll, so a long poll is never cut short.
pub const READ_TIMEOUT: Duration = Duration::from_secs(65);
/// `timeout` sent with getUpdates, in seconds.
pub const POLL_TIMEOUT_S: u32 = 50;

const API_BASE: &str = "https://api.telegram.org";
/// Debug builds only: may point the API at a loopback fake (see `pick_base`).
const BASE_OVERRIDE_ENV: &str = "PUPPET_MASTER_TELEGRAM_API_BASE";
const ALLOWED_UPDATES: [&str; 2] = ["message", "callback_query"];
const DEFAULT_RETRY_AFTER_S: u64 = 30;
const MAX_RETRY_AFTER_S: u64 = 3600;

/// A message from a person in a chat, as the engine needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InMessage {
    pub message_id: i64,
    pub chat_id: i64,
    /// `chat.type == "private"`. Groups and channels are `false` and the engine drops them.
    pub chat_private: bool,
    pub from_id: Option<i64>,
    pub from_is_bot: bool,
    pub username: Option<String>,
    pub first_name: Option<String>,
    /// Unix seconds, as Telegram sends it.
    pub date_s: i64,
    pub text: Option<String>,
    /// The message this one replies to, when there is one.
    pub reply_to: Option<i64>,
}

/// A tap on one of the bot's buttons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InCallback {
    pub id: String,
    pub from_id: i64,
    /// Absent for inline-mode buttons, which have no chat.
    pub chat_id: Option<i64>,
    pub message_id: Option<i64>,
    pub data: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateKind {
    Message(InMessage),
    Callback(InCallback),
    /// Anything else (edited messages, channel posts, ...). Acknowledged, never acted on.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub update_id: i64,
    pub kind: UpdateKind,
}

/// Why a call failed. No field holds text from Telegram or from the network layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiError {
    /// 401, or 404 (Telegram answers 404 for a token it does not know).
    Unauthorized,
    /// 409: another `getUpdates` consumer or a webhook is using this bot.
    Conflict,
    /// 403: the bot may not write to this chat (blocked).
    Forbidden,
    /// 429: wait this long before the next call.
    RateLimited { retry_after_s: u64 },
    /// Could not reach Telegram (connect, DNS, timeout, TLS).
    Network,
    /// Any other HTTP error status, 5xx included.
    Bad { status: u16 },
    /// Telegram answered, but not in the shape we know.
    Parse,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized => f.write_str("Telegram did not accept this token."),
            Self::Conflict => f.write_str("Another program is using this bot."),
            Self::Forbidden => f.write_str("Telegram refused this for this chat."),
            Self::RateLimited { retry_after_s } => write!(f, "Telegram asked us to wait {retry_after_s} seconds."),
            Self::Network => f.write_str("Could not reach Telegram."),
            Self::Bad { status } => write!(f, "Telegram answered with an error ({status})."),
            Self::Parse => f.write_str("Telegram sent an answer we could not read."),
        }
    }
}

impl std::error::Error for ApiError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotInfo {
    pub id: i64,
    pub username: String,
}

/// A message on its way out, in the shape the wire needs. Built by the engine from the rendered text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireMessage {
    pub text: Redacted<String>,
    pub keyboard: Vec<Vec<WireButton>>,
}

/// One inline button. `data` is at most 64 bytes (Telegram's limit for `callback_data`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireButton {
    pub label: String,
    pub data: String,
}

pub trait Api: Send + Sync {
    fn get_me(&self) -> Result<BotInfo, ApiError>;
    /// The webhook URL if one is set (Telegram then refuses getUpdates), `None` when none is set.
    fn webhook_url(&self) -> Result<Option<String>, ApiError>;
    fn get_updates(&self, offset: Option<i64>, timeout_s: u32) -> Result<Vec<Update>, ApiError>;
    /// Sends a message and returns its message id.
    fn send(&self, chat_id: i64, out: &WireMessage, reply_to: Option<i64>) -> Result<i64, ApiError>;
    /// Replaces the text and buttons of a sent message. No buttons in `out` removes them.
    fn edit(&self, chat_id: i64, message_id: i64, out: &WireMessage) -> Result<(), ApiError>;
    /// Stops the spinner on a button tap, with an optional short note.
    fn answer_callback(&self, id: &str, text: Option<&Redacted<String>>) -> Result<(), ApiError>;
    fn set_commands(&self, list: &[(&str, &str)]) -> Result<(), ApiError>;
}

/// The envelope every Bot API answer comes in.
#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    error_code: Option<u16>,
    #[serde(default)]
    parameters: Option<Params>,
}

#[derive(Debug, Deserialize)]
struct Params {
    #[serde(default)]
    retry_after: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct Me {
    id: i64,
    username: String,
}

#[derive(Debug, Deserialize)]
struct WebhookInfo {
    #[serde(default)]
    url: String,
}

/// The real client. Debug prints no fields, so the token never reaches a log through `{:?}`.
pub struct HttpApi {
    token: String,
    base: String,
    agent: ureq::Agent,
}

impl std::fmt::Debug for HttpApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HttpApi(<hidden>)")
    }
}

impl HttpApi {
    /// The client for `token`. The base is Telegram's, except a debug build may point it at
    /// `http://127.0.0.1:<port>` through `PUPPET_MASTER_TELEGRAM_API_BASE`.
    pub fn new(token: &str) -> Self {
        let raw = std::env::var(BASE_OVERRIDE_ENV).ok();
        Self::with_base(token, pick_base(cfg!(debug_assertions), raw.as_deref()))
    }

    fn with_base(token: &str, base: String) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout_read(READ_TIMEOUT)
            // A redirect could send the token to another host. Telegram never redirects.
            .redirects(0)
            .build();
        Self { token: token.to_string(), base, agent }
    }

    /// One Bot API method. The URL holds the token: it is used here and never formatted into an error.
    fn call(&self, method: &str, body: Value) -> Result<Value, ApiError> {
        let url = format!("{}/bot{}/{}", self.base, self.token, method);
        let resp = match self.agent.post(&url).send_json(body) {
            Ok(resp) if (300..400).contains(&resp.status()) => {
                // Not followed (see `with_base`), so a redirect is just a failed call.
                return Err(ApiError::Bad { status: resp.status() });
            }
            Ok(resp) => resp,
            Err(ureq::Error::Status(code, resp)) => {
                let text = resp.into_string().unwrap_or_default();
                return Err(status_error(code, &text));
            }
            // The transport error text names the URL, so it is dropped, not mapped to text.
            Err(ureq::Error::Transport(_)) => return Err(ApiError::Network),
        };
        let text = resp.into_string().map_err(|_| ApiError::Network)?;
        let env: Envelope = serde_json::from_str(&text).map_err(|_| ApiError::Parse)?;
        if env.ok {
            return Ok(env.result);
        }
        match env.error_code {
            Some(code) => Err(error_for(code, env.parameters.and_then(|p| p.retry_after))),
            None => Err(ApiError::Parse),
        }
    }
}

/// Where the API lives. Only a debug build may move it, and only to a loopback address with a port.
fn pick_base(debug_build: bool, raw: Option<&str>) -> String {
    match raw {
        Some(v) if debug_build && is_loopback_base(v) => v.trim_end_matches('/').to_string(),
        _ => API_BASE.to_string(),
    }
}

fn is_loopback_base(v: &str) -> bool {
    let Some(rest) = v.strip_prefix("http://127.0.0.1:") else { return false };
    rest.trim_end_matches('/').parse::<u16>().is_ok_and(|port| port != 0)
}

/// A non-200 answer. Telegram puts the reason in the body; only `retry_after` is read from it.
fn status_error(code: u16, body: &str) -> ApiError {
    let retry = serde_json::from_str::<Envelope>(body)
        .ok()
        .and_then(|env| env.parameters.and_then(|p| p.retry_after));
    error_for(code, retry)
}

fn error_for(code: u16, retry_after: Option<u64>) -> ApiError {
    match code {
        401 | 404 => ApiError::Unauthorized,
        403 => ApiError::Forbidden,
        409 => ApiError::Conflict,
        429 => ApiError::RateLimited {
            retry_after_s: retry_after.unwrap_or(DEFAULT_RETRY_AFTER_S).clamp(1, MAX_RETRY_AFTER_S),
        },
        other => ApiError::Bad { status: other },
    }
}

/// Inline keyboard rows for the wire.
fn markup(keyboard: &[Vec<WireButton>]) -> Value {
    let rows: Vec<Vec<Value>> = keyboard
        .iter()
        .map(|row| row.iter().map(|b| json!({"text": b.label, "callback_data": b.data})).collect())
        .collect();
    json!({ "inline_keyboard": rows })
}

/// Plain text, no link previews. Telegram's servers would otherwise fetch URLs that may carry secrets.
fn no_preview() -> Value {
    json!({ "is_disabled": true })
}

impl Api for HttpApi {
    fn get_me(&self) -> Result<BotInfo, ApiError> {
        let me: Me = serde_json::from_value(self.call("getMe", json!({}))?).map_err(|_| ApiError::Parse)?;
        Ok(BotInfo { id: me.id, username: me.username })
    }

    fn webhook_url(&self) -> Result<Option<String>, ApiError> {
        let info: WebhookInfo =
            serde_json::from_value(self.call("getWebhookInfo", json!({}))?).map_err(|_| ApiError::Parse)?;
        Ok(Some(info.url).filter(|u| !u.is_empty()))
    }

    fn get_updates(&self, offset: Option<i64>, timeout_s: u32) -> Result<Vec<Update>, ApiError> {
        let mut body = json!({ "timeout": timeout_s, "allowed_updates": ALLOWED_UPDATES });
        if let Some(o) = offset {
            body["offset"] = json!(o);
        }
        let result = self.call("getUpdates", body)?;
        wire::map_updates(&result).ok_or(ApiError::Parse)
    }

    fn send(&self, chat_id: i64, out: &WireMessage, reply_to: Option<i64>) -> Result<i64, ApiError> {
        // No parse_mode: plain text only, so nothing in a message can format or spoof the chat.
        let mut body = json!({
            "chat_id": chat_id,
            "text": out.text.as_str(),
            "link_preview_options": no_preview(),
            "protect_content": true,
        });
        if let Some(r) = reply_to {
            body["reply_to_message_id"] = json!(r);
        }
        if !out.keyboard.is_empty() {
            body["reply_markup"] = markup(&out.keyboard);
        }
        let result = self.call("sendMessage", body)?;
        result.get("message_id").and_then(Value::as_i64).ok_or(ApiError::Parse)
    }

    fn edit(&self, chat_id: i64, message_id: i64, out: &WireMessage) -> Result<(), ApiError> {
        let mut body = json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": out.text.as_str(),
            "link_preview_options": no_preview(),
        });
        // Telegram removes the buttons when reply_markup is left out, so it is only sent with rows.
        if !out.keyboard.is_empty() {
            body["reply_markup"] = markup(&out.keyboard);
        }
        self.call("editMessageText", body).map(|_| ())
    }

    fn answer_callback(&self, id: &str, text: Option<&Redacted<String>>) -> Result<(), ApiError> {
        let mut body = json!({ "callback_query_id": id });
        if let Some(t) = text {
            body["text"] = json!(t.as_str());
        }
        self.call("answerCallbackQuery", body).map(|_| ())
    }

    fn set_commands(&self, list: &[(&str, &str)]) -> Result<(), ApiError> {
        let commands: Vec<Value> = list
            .iter()
            .map(|(command, description)| json!({ "command": command, "description": description }))
            .collect();
        self.call("setMyCommands", json!({ "commands": commands })).map(|_| ())
    }
}
