//! The client of the sandbox's control service (`pm-sbx-control`, inside the distro).
//!
//! The service listens on the distro's loopback (`127.0.0.1:47801`), which WSL forwards to the same
//! address on Windows. Every request carries `Authorization: Bearer <token>`. The token is made here
//! (`ControlToken::generate`), pushed into the distro over stdin by `lifecycle.rs` and kept in memory
//! only: it has no `Clone`, no `Serialize`, no `Display`, and `Debug` does not print it.
//!
//! An `Action` is turned into the exact argv shapes the service allows (`xdotool` forms and
//! `pm-sbx-focus-or-launch`); the service re-checks every one, so this is the polite side of a rule
//! that the guest enforces. Nothing here uses a shell.

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use rand::RngCore;
use serde_json::{json, Value};

use super::Observation;
use crate::potato::wire_names::GUEST_POINTER_FIELD;

pub const DEFAULT_PORT: u16 = 47801;
pub const DISPLAY: &str = ":99";
/// The guest refuses more steps than this in one request.
pub const MAX_STEPS: usize = 64;
const MAX_TEXT_CHARS: usize = 2000;
const MAX_COORD: i32 = 16_384;
const MAX_WAIT_MS: u32 = 5000;
const MAX_REPLY_BYTES: u64 = 40 * 1024 * 1024;
const MAX_TITLE_CHARS: usize = 200;
const FOCUS_OR_LAUNCH: &str = "pm-sbx-focus-or-launch";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const READ_TIMEOUT: Duration = Duration::from_secs(40);

/// The secret that opens the control service. 32 random bytes as hex.
pub struct ControlToken(String);

impl ControlToken {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        Self(bytes.iter().map(|b| format!("{b:02x}")).collect())
    }

    /// The value, for the one request header and the one push into the distro. Nothing else calls this
    /// (`guard_tests` checks which files do).
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for ControlToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ControlToken(..)")
    }
}

impl Drop for ControlToken {
    fn drop(&mut self) {
        // Best effort: the memory holding the secret is overwritten before it is freed.
        let mut bytes = std::mem::take(&mut self.0).into_bytes();
        for b in bytes.iter_mut() {
            *b = 0;
        }
        std::hint::black_box(&bytes);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// One thing the bot (or a test) does on the screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Move { x: i32, y: i32 },
    Click { x: i32, y: i32, button: Button },
    DoubleClick { x: i32, y: i32 },
    Drag { from: (i32, i32), to: (i32, i32) },
    Scroll { x: i32, y: i32, direction: Direction, amount: u32 },
    Type { text: String },
    Key { combo: String },
    Wait { ms: u32 },
    /// Bring an app forward or start it: `app` is a name from the guest's launcher list.
    Launch { app: String, arg: Option<String> },
}

/// One request step, in the form the service takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Argv(Vec<String>),
    Wait(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlError {
    /// The service did not answer (not started, or WSL's forward is down).
    Unreachable(String),
    /// 401: the token is wrong. The service was started by someone else, or restarted.
    Unauthorized,
    /// 400: the service refused the request (the action is outside its list).
    Rejected(String),
    /// 500: the action itself failed.
    Failed(String),
    /// The reply was not what the protocol says.
    BadReply(String),
    /// The request was not sent: an action is outside what the guest allows.
    Invalid(&'static str),
}

impl ControlError {
    pub fn code(&self) -> &'static str {
        match self {
            ControlError::Unreachable(_) => "SANDBOX_UNREACHABLE",
            ControlError::Unauthorized => "SANDBOX_UNAUTHORIZED",
            ControlError::Rejected(_) => "SANDBOX_REJECTED",
            ControlError::Failed(_) => "SANDBOX_ACTION_FAILED",
            ControlError::BadReply(_) => "SANDBOX_BAD_REPLY",
            ControlError::Invalid(_) => "SANDBOX_INVALID_ACTION",
        }
    }

    pub fn message(&self) -> String {
        match self {
            ControlError::Unreachable(_) => "The sandbox screen is not answering.".to_string(),
            ControlError::Unauthorized => "The sandbox did not accept the app's key. Stop it and start it again.".to_string(),
            ControlError::Rejected(why) => format!("The sandbox refused that action: {why}."),
            ControlError::Failed(why) => format!("That action did not work: {why}."),
            ControlError::BadReply(_) => "The sandbox sent an answer the app could not read.".to_string(),
            ControlError::Invalid(why) => format!("That action cannot be sent: {why}."),
        }
    }
}

impl std::fmt::Display for ControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for ControlError {}

fn coord(v: i32) -> Result<String, ControlError> {
    if (0..=MAX_COORD).contains(&v) {
        Ok(v.to_string())
    } else {
        Err(ControlError::Invalid("a position is outside the screen"))
    }
}

fn argv(parts: &[&str]) -> Step {
    let mut v = vec!["env".to_string(), format!("DISPLAY={DISPLAY}"), "xdotool".to_string()];
    v.extend(parts.iter().map(|p| p.to_string()));
    Step::Argv(v)
}

fn move_to(x: i32, y: i32, tail: &[&str]) -> Result<Step, ControlError> {
    let (x, y) = (coord(x)?, coord(y)?);
    let mut parts = vec!["mousemove", "--", x.as_str(), y.as_str()];
    parts.extend_from_slice(tail);
    Ok(argv(&parts))
}

fn plain_text(text: &str) -> bool {
    !text.is_empty() && text.chars().count() <= MAX_TEXT_CHARS && !text.contains('\0')
}

fn key_combo(combo: &str) -> bool {
    (1..=40).contains(&combo.len()) && combo.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'+')
}

fn launcher_name(name: &str) -> bool {
    let mut chars = name.chars();
    (1..=32).contains(&name.len())
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The service steps for one action.
pub fn steps_for(action: &Action) -> Result<Vec<Step>, ControlError> {
    let button = |b: &Button| if *b == Button::Left { "1" } else { "3" };
    Ok(match action {
        Action::Move { x, y } => vec![move_to(*x, *y, &[])?],
        Action::Click { x, y, button: b } => vec![move_to(*x, *y, &["click", button(b)])?],
        Action::DoubleClick { x, y } => {
            vec![move_to(*x, *y, &["click", "1"])?, Step::Wait(60), move_to(*x, *y, &["click", "1"])?]
        }
        Action::Drag { from, to } => vec![
            move_to(from.0, from.1, &["mousedown", "1"])?,
            move_to(to.0, to.1, &[])?,
            argv(&["mouseup", "1"]),
        ],
        Action::Scroll { x, y, direction, amount } => {
            let wheel = match direction {
                Direction::Up => "4",
                Direction::Down => "5",
                Direction::Left => "6",
                Direction::Right => "7",
            };
            let repeat = (*amount).clamp(1, 20).to_string();
            vec![move_to(*x, *y, &["click", "--repeat", &repeat, wheel])?]
        }
        Action::Type { text } => {
            if !plain_text(text) {
                return Err(ControlError::Invalid("the text is empty, too long or has a NUL"));
            }
            vec![argv(&["type", "--clearmodifiers", "--", text])]
        }
        Action::Key { combo } => {
            if !key_combo(combo) {
                return Err(ControlError::Invalid("a key is letters, digits, _ and + only"));
            }
            vec![argv(&["key", "--clearmodifiers", combo])]
        }
        Action::Wait { ms } => vec![Step::Wait((*ms).min(MAX_WAIT_MS))],
        Action::Launch { app, arg } => {
            if !launcher_name(app) {
                return Err(ControlError::Invalid("an app name is a lowercase word"));
            }
            let mut v = vec!["env".to_string(), format!("DISPLAY={DISPLAY}"), FOCUS_OR_LAUNCH.to_string(), app.clone()];
            if let Some(arg) = arg {
                if arg.is_empty() || arg.starts_with('-') || arg.contains('\0') || arg.chars().count() > MAX_TEXT_CHARS {
                    return Err(ControlError::Invalid("an app argument is empty, starts with - or is too long"));
                }
                v.push(arg.clone());
            }
            vec![Step::Argv(v)]
        }
    })
}

/// The steps of a list of actions, in order. At most `MAX_STEPS` in one request.
pub fn plan(actions: &[Action]) -> Result<Vec<Step>, ControlError> {
    let mut steps = Vec::new();
    for action in actions {
        steps.extend(steps_for(action)?);
    }
    if steps.len() > MAX_STEPS {
        return Err(ControlError::Invalid("too many steps in one request"));
    }
    Ok(steps)
}

fn step_json(step: &Step) -> Value {
    match step {
        Step::Argv(argv) => json!({ "argv": argv }),
        Step::Wait(ms) => json!({ "waitMs": ms }),
    }
}

/// The answer to `act`.
#[derive(Clone, Debug)]
pub struct ActResult {
    pub completed: usize,
    pub observation: Option<Observation>,
}

#[derive(Clone)]
pub struct ControlClient {
    base: String,
    token: Arc<ControlToken>,
    agent: ureq::Agent,
}

impl std::fmt::Debug for ControlClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlClient").field("base", &self.base).finish_non_exhaustive()
    }
}

impl ControlClient {
    pub fn new(port: u16, token: Arc<ControlToken>) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout_read(READ_TIMEOUT)
            // A redirect could carry the token somewhere else. The service never redirects.
            .redirects(0)
            .build();
        Self { base: format!("http://127.0.0.1:{port}"), token, agent }
    }

    fn call(&self, path: &str, body: Value) -> Result<Value, ControlError> {
        let url = format!("{}{path}", self.base);
        let auth = format!("Bearer {}", self.token.expose());
        match self.agent.post(&url).set("Authorization", &auth).send_json(body) {
            Ok(resp) => {
                let mut bytes = Vec::new();
                resp.into_reader()
                    .take(MAX_REPLY_BYTES)
                    .read_to_end(&mut bytes)
                    .map_err(|_| ControlError::BadReply("read".to_string()))?;
                serde_json::from_slice(&bytes).map_err(|_| ControlError::BadReply("not json".to_string()))
            }
            Err(ureq::Error::Status(code, resp)) => {
                let mut text = String::new();
                let _ = resp.into_reader().take(4096).read_to_string(&mut text);
                let why = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
                    .unwrap_or_default();
                let why: String = crate::redact::text(&why).into_inner().chars().take(120).collect();
                Err(match code {
                    401 => ControlError::Unauthorized,
                    400 => ControlError::Rejected(why),
                    _ => ControlError::Failed(why),
                })
            }
            // The transport text can hold the URL; it names no secret, but it is not shown either.
            Err(ureq::Error::Transport(t)) => Err(ControlError::Unreachable(t.kind().to_string())),
        }
    }

    /// One picture of the screen.
    pub fn screenshot(&self) -> Result<Observation, ControlError> {
        let reply = self.call("/v1/screenshot", json!({ "display": DISPLAY }))?;
        parse_observation(reply.get("observation").ok_or(ControlError::BadReply("no observation".to_string()))?)
    }

    /// Runs the actions and (when `observe`) returns the screen after them.
    pub fn act(&self, actions: &[Action], settle_ms: u32, observe: bool) -> Result<ActResult, ControlError> {
        let steps: Vec<Value> = plan(actions)?.iter().map(step_json).collect();
        let body = json!({ "display": DISPLAY, "steps": steps, "settleMs": settle_ms.min(MAX_WAIT_MS), "observe": observe });
        let reply = self.call("/v1/desktop", body)?;
        let completed = reply.get("completed").and_then(Value::as_u64).unwrap_or(0) as usize;
        let observation = match reply.get("observation") {
            Some(obs) if observe => Some(parse_observation(obs)?),
            _ => None,
        };
        Ok(ActResult { completed, observation })
    }
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// The picture in a service answer. The size is read from the picture itself, not trusted from the JSON.
pub fn parse_observation(obs: &Value) -> Result<Observation, ControlError> {
    let bad = |what: &str| ControlError::BadReply(what.to_string());
    let image = obs.get("image").and_then(Value::as_str).ok_or(bad("no image"))?;
    let png = base64::engine::general_purpose::STANDARD.decode(image).map_err(|_| bad("image is not base64"))?;
    if png.len() < 24 || png[..8] != PNG_SIGNATURE || &png[12..16] != b"IHDR" {
        return Err(bad("image is not a PNG"));
    }
    let width = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let height = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    let pointer = obs.get(GUEST_POINTER_FIELD).and_then(|c| {
        let x = i32::try_from(c.get("x")?.as_i64()?).ok()?;
        let y = i32::try_from(c.get("y")?.as_i64()?).ok()?;
        Some((x, y))
    });
    let active_window_title = obs
        .get("activeWindow")
        .and_then(|w| w.get("title"))
        .and_then(Value::as_str)
        .map(|t| t.chars().filter(|c| !c.is_control()).take(MAX_TITLE_CHARS).collect::<String>())
        .filter(|t| !t.is_empty());
    Ok(Observation { png, width, height, pointer, active_window_title })
}
