//! The bot's computer, as the brain's tools use it: `computer_observe`, `computer_act`, `browser_navigate`,
//! `browser_snapshot` and `browser_act`. The brain sends a `sandbox_call` event; this file decides whether it
//! may run, runs it, and builds the answer (`sandbox_result`). The engine (`service.rs`) calls it on its own thread.
//!
//! The order of the checks, and why. Everything before the sandbox is touched is cheap and cannot start anything:
//!
//! 1. Potato Bot is on and the Sandbox Labs switch is on, else `POTATO_OFF` / `SANDBOX_OFF`;
//! 2. the rules (`rules::tool_access`): the trust of the turn, set by Puppet Master when it sent the message,
//!    may use the sandbox. Anyone may; only the person may also use local helpers (that is `helpers`' check);
//! 3. the tool is one of the five, and the arguments are valid (`validate`): sizes, counts, kinds, coordinates,
//!    web addresses (`sandbox/browser.rs`). A refusal here costs nothing;
//! 4. a sandbox is installed and wired in (`Port`), else `SANDBOX_NOT_SET_UP`;
//! 5. the screen lease. The bot holds it or takes it. The person holding it is `LEASE_HELD_BY_PERSON` (with
//!    `needsYou: true`: say so and wait). A lease that was taken, ran out or was handed back since the last call
//!    is `LEASE_LOST` once ("look again before you act") and the next call goes on with a new one;
//! 6. the day's action cap (`cost::spend_sandbox_actions`, a setting, default 500). A call that would pass the
//!    cap is refused whole and not counted;
//! 7. the call itself, one at a time (the screen has one pointer). It starts the sandbox if it is stopped.
//!
//! What goes back is plain JSON the brain can read: window title, pointer, a frame id, whether the screen changed
//! since the last look, and, for a model that can see, the picture. For a model that cannot (`image: false` in the
//! call), `computer_observe` returns words: the page text of the front Chrome tab when there is one, else a short
//! note. Every string in an answer is redacted, and a page's words are data: the brain fences them.
//!
//! Nothing here starts a process or builds a `wsl.exe` command; the sandbox does (`sandbox/wsl.rs`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine;
use parking_lot::Mutex;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::protocol::{Trust, MAX_SANDBOX_ARGS_BYTES};
use super::rules;
use super::sandbox::browser::{self, BrowserError};
use super::sandbox::control::{self, Action, Button, Direction};
use super::sandbox::lease::{Holder, LeaseError, NeedsYou, ScreenLease};
use super::sandbox::{Observation, Sandbox, SandboxError};
use super::service::Host;
use super::store;

#[cfg_attr(not(test), allow(dead_code))]
pub const TOOL_NAMES: [&str; 5] = ["computer_observe", "computer_act", "browser_navigate", "browser_snapshot", "browser_act"];
/// Most actions in one `computer_act` or `browser_act` call.
pub const MAX_ACTIONS: usize = 24;
/// A picture bigger than this (PNG bytes) is not sent; the answer says so and gives the words instead.
pub const MAX_IMAGE_BYTES: usize = 1_500_000;
/// The longest a window title is kept in an answer.
const MAX_TITLE_CHARS: usize = 200;
const MAX_COORD: f64 = 16_384.0;
const MAX_WAIT_MS: f64 = 5000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    ComputerObserve,
    ComputerAct,
    BrowserNavigate,
    BrowserSnapshot,
    BrowserAct,
}

impl Tool {
    pub fn parse(name: &str) -> Option<Tool> {
        Some(match name {
            "computer_observe" => Tool::ComputerObserve,
            "computer_act" => Tool::ComputerAct,
            "browser_navigate" => Tool::BrowserNavigate,
            "browser_snapshot" => Tool::BrowserSnapshot,
            "browser_act" => Tool::BrowserAct,
            _ => return None,
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn name(self) -> &'static str {
        match self {
            Tool::ComputerObserve => "computer_observe",
            Tool::ComputerAct => "computer_act",
            Tool::BrowserNavigate => "browser_navigate",
            Tool::BrowserSnapshot => "browser_snapshot",
            Tool::BrowserAct => "browser_act",
        }
    }
}

/// An error result: a short code, one plain sentence, and whether the person has the screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallError {
    pub code: &'static str,
    pub message: String,
    pub needs_you: bool,
}

impl CallError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), needs_you: false }
    }
}

impl From<BrowserError> for CallError {
    fn from(e: BrowserError) -> Self {
        CallError::new(e.code(), e.message())
    }
}

impl From<SandboxError> for CallError {
    fn from(e: SandboxError) -> Self {
        CallError::new(e.code, e.message)
    }
}

impl From<control::ControlError> for CallError {
    fn from(e: control::ControlError) -> Self {
        CallError::new("BAD_ACTIONS", e.message())
    }
}

fn bad(message: impl Into<String>) -> CallError {
    CallError::new("BAD_ARGS", message)
}

/// What a valid call asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Observe,
    /// `show`: the answer carries the screen after the actions.
    Act { actions: Vec<Action>, show: bool },
    Navigate { url: String },
    Snapshot,
    /// Already rebuilt by `browser::clean_actions`.
    BrowserAct { actions: Vec<Value> },
}

impl Request {
    /// How many actions the call counts against the day's cap.
    pub fn cost(&self) -> u32 {
        match self {
            Request::Act { actions, .. } => actions.len() as u32,
            Request::BrowserAct { actions } => actions.len() as u32,
            _ => 1,
        }
    }
}

/// Switches that can change at any time.
#[derive(Clone, Copy, Debug)]
pub struct Gates {
    /// Potato Bot itself (the Labs kill switch).
    pub bot_on: bool,
    /// The Sandbox Labs switch.
    pub sandbox_on: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// A web address inside the sandbox (a test page) may be opened. Only a test or a dev build can set it.
    pub allow_local_pages: bool,
}

impl Options {
    /// The dev switch: only a test or a `dev-fake-tui` build reads it; a release build is always strict.
    pub fn from_environment() -> Options {
        let dev = cfg!(any(test, feature = "dev-fake-tui"));
        Options { allow_local_pages: dev && std::env::var("PM_SBX_ALLOW_LOCAL_PAGES").is_ok_and(|v| v == "1") }
    }
}

// ---- reading the arguments -----------------------------------------------------------------------

fn number(obj: &Map<String, Value>, key: &str) -> Result<Option<f64>, CallError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_f64() {
            Some(n) if n.is_finite() => Ok(Some(n)),
            _ => Err(bad(format!("{key} must be a number."))),
        },
    }
}

fn coordinate(obj: &Map<String, Value>, key: &str, n: usize) -> Result<Option<i32>, CallError> {
    match number(obj, key)? {
        None => Ok(None),
        Some(v) if (0.0..=MAX_COORD).contains(&v) => Ok(Some(v.round() as i32)),
        Some(_) => Err(bad(format!("Action {n}: {key} is outside the screen."))),
    }
}

fn need_point(obj: &Map<String, Value>, n: usize, kind: &str) -> Result<(i32, i32), CallError> {
    match (coordinate(obj, "x", n)?, coordinate(obj, "y", n)?) {
        (Some(x), Some(y)) => Ok((x, y)),
        _ => Err(bad(format!("Action {n}: {kind} needs x and y."))),
    }
}

fn modifier(name: &str) -> Option<&'static str> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "ctrl" | "control" => "ctrl",
        "alt" | "option" => "alt",
        "shift" => "shift",
        "super" | "meta" | "win" | "windows" | "cmd" | "command" => "super",
        _ => return None,
    })
}

fn one_action(n: usize, value: &Value, pointer: Option<(i32, i32)>, opts: &Options) -> Result<Action, CallError> {
    let obj = value.as_object().ok_or_else(|| bad(format!("Action {n} must be an object.")))?;
    let kind = obj.get("kind").and_then(Value::as_str).unwrap_or("");
    Ok(match kind {
        "click" => {
            let (x, y) = need_point(obj, n, kind)?;
            let double = obj.get("double").and_then(Value::as_bool).unwrap_or(false);
            let button = match obj.get("button").and_then(Value::as_str).unwrap_or("left") {
                "left" => Button::Left,
                "right" => Button::Right,
                _ => return Err(bad(format!("Action {n}: button is left or right."))),
            };
            match (double, button) {
                (true, Button::Left) => Action::DoubleClick { x, y },
                (true, Button::Right) => return Err(bad(format!("Action {n}: a double click uses the left button."))),
                (false, button) => Action::Click { x, y, button },
            }
        }
        "move" => {
            let (x, y) = need_point(obj, n, kind)?;
            Action::Move { x, y }
        }
        "drag" => {
            let from = need_point(obj, n, kind)?;
            let (tx, ty) = (coordinate(obj, "to_x", n)?, coordinate(obj, "to_y", n)?);
            match (tx, ty) {
                (Some(tx), Some(ty)) => Action::Drag { from, to: (tx, ty) },
                _ => return Err(bad(format!("Action {n}: drag needs to_x and to_y."))),
            }
        }
        "down" | "up" => return Err(bad(format!("Action {n}: down and up are not supported. Use drag, with x, y, to_x and to_y."))),
        "type" => {
            let text = obj.get("text").and_then(Value::as_str).unwrap_or("");
            Action::Type { text: text.to_string() }
        }
        "key" => {
            let key = obj.get("key").and_then(Value::as_str).unwrap_or("").trim();
            if key.is_empty() {
                return Err(bad(format!("Action {n}: key needs a key name.")));
            }
            let mut parts: Vec<&str> = Vec::new();
            if let Some(mods) = obj.get("modifiers") {
                let list = mods.as_array().ok_or_else(|| bad(format!("Action {n}: modifiers is a list.")))?;
                for m in list {
                    let name = m.as_str().and_then(modifier).ok_or_else(|| bad(format!("Action {n}: a modifier is ctrl, alt, shift or super.")))?;
                    parts.push(name);
                }
            }
            parts.push(key);
            Action::Key { combo: parts.join("+") }
        }
        "scroll" => {
            let at = match (coordinate(obj, "x", n)?, coordinate(obj, "y", n)?) {
                (Some(x), Some(y)) => (x, y),
                (None, None) => pointer.ok_or_else(|| bad(format!("Action {n}: scroll needs x and y until you have looked at the screen once.")))?,
                _ => return Err(bad(format!("Action {n}: give both x and y, or neither."))),
            };
            let direction = match obj.get("direction").and_then(Value::as_str).unwrap_or("down") {
                "up" => Direction::Up,
                "down" => Direction::Down,
                "left" => Direction::Left,
                "right" => Direction::Right,
                _ => return Err(bad(format!("Action {n}: direction is up, down, left or right."))),
            };
            let amount = number(obj, "amount")?.unwrap_or(3.0).round().clamp(1.0, 20.0) as u32;
            Action::Scroll { x: at.0, y: at.1, direction, amount }
        }
        "wait" => {
            let ms = number(obj, "ms")?.unwrap_or(500.0).round().clamp(0.0, MAX_WAIT_MS) as u32;
            Action::Wait { ms }
        }
        "focus" => {
            let app = obj.get("application").and_then(Value::as_str).unwrap_or("").trim().to_ascii_lowercase();
            if app.is_empty() {
                return Err(bad(format!("Action {n}: focus needs an application (chrome, terminal, files or editor).")));
            }
            let arg = match obj.get("uri").and_then(Value::as_str).map(str::trim).filter(|u| !u.is_empty()) {
                Some(uri) => Some(browser::check_launch_arg(uri, opts.allow_local_pages)?),
                None => None,
            };
            Action::Launch { app, arg }
        }
        _ => return Err(bad(format!("Action {n}: the kind must be click, move, drag, type, key, scroll, wait or focus."))),
    })
}

/// Checks the arguments of one call. `pointer` is where the pointer was at the last look (for a scroll with no
/// position). A refusal costs nothing: nothing has been touched.
pub fn validate(tool: Tool, args: &Value, pointer: Option<(i32, i32)>, opts: &Options) -> Result<Request, CallError> {
    if args.to_string().len() > MAX_SANDBOX_ARGS_BYTES {
        return Err(bad("That is too much to send in one call. Split it into smaller calls."));
    }
    let obj = args.as_object().ok_or_else(|| bad("The arguments must be an object."))?;
    Ok(match tool {
        Tool::ComputerObserve => Request::Observe,
        Tool::BrowserSnapshot => Request::Snapshot,
        Tool::BrowserNavigate => {
            let url = obj.get("url").and_then(Value::as_str).ok_or_else(|| bad("browser_navigate needs a url."))?;
            Request::Navigate { url: browser::check_url(url, opts.allow_local_pages)? }
        }
        Tool::BrowserAct => {
            let actions = browser::clean_actions(obj.get("actions").unwrap_or(&Value::Null))?;
            // The tool's own limit, and the size of what will be sent, are checked now so a refusal costs nothing.
            browser::act_args(&actions)?;
            Request::BrowserAct { actions }
        }
        Tool::ComputerAct => {
            let list = obj.get("actions").and_then(Value::as_array).ok_or_else(|| bad("computer_act needs a list of actions."))?;
            if list.is_empty() || list.len() > MAX_ACTIONS {
                return Err(bad("Give 1 to 24 actions."));
            }
            let mut actions = Vec::with_capacity(list.len());
            let mut here = pointer;
            for (i, item) in list.iter().enumerate() {
                let action = one_action(i + 1, item, here, opts)?;
                match &action {
                    Action::Move { x, y } | Action::Click { x, y, .. } | Action::DoubleClick { x, y } | Action::Scroll { x, y, .. } => here = Some((*x, *y)),
                    Action::Drag { to, .. } => here = Some(*to),
                    _ => {}
                }
                actions.push(action);
            }
            // The exact shapes the sandbox's control service takes (text, key names, step count).
            control::plan(&actions)?;
            let show = obj.get("observe").and_then(Value::as_bool).unwrap_or(true);
            Request::Act { actions, show }
        }
    })
}

// ---- the sandbox and the screen ------------------------------------------------------------------

/// The sandbox and the screen lease it shares with the Computer panel and the screen links.
#[derive(Clone)]
pub struct Port {
    pub sandbox: Arc<dyn Sandbox>,
    pub lease: Arc<Mutex<ScreenLease>>,
}

/// Who is asking. `trust` comes from the turn Puppet Master sent, never from the brain.
#[derive(Clone, Debug)]
pub struct Context {
    pub bot: String,
    pub conversation: String,
    pub trust: Trust,
    /// The model can look at a picture, so the answer may carry one.
    pub want_image: bool,
}

#[derive(Default)]
struct Run {
    lease_id: Option<String>,
    /// The bot's hold was lost (taken over, ran out) and it has not been told yet.
    stale: bool,
    frame: Option<String>,
    pointer: Option<(i32, i32)>,
}

pub struct Calls {
    root: PathBuf,
    host: Arc<dyn Host>,
    opts: Mutex<Options>,
    runs: Mutex<HashMap<String, Run>>,
    /// One call at a time: the screen has one pointer and one keyboard.
    serial: Mutex<()>,
}

fn run_id(bot: &str, conversation: &str) -> String {
    format!("{bot}@{conversation}")
}

impl Calls {
    pub fn new(root: PathBuf, host: Arc<dyn Host>) -> Calls {
        Calls { root, host, opts: Mutex::new(Options::from_environment()), runs: Mutex::new(HashMap::new()), serial: Mutex::new(()) }
    }

    #[cfg(test)]
    pub fn set_options(&self, opts: Options) {
        *self.opts.lock() = opts;
    }

    /// Runs one call. `port` is `None` when no sandbox is wired in.
    pub fn execute(&self, port: Option<&Port>, gates: Gates, ctx: &Context, tool: &str, args: &Value) -> Result<Value, CallError> {
        if !gates.bot_on {
            return Err(CallError::new("POTATO_OFF", "Potato Bot is switched off."));
        }
        if !gates.sandbox_on {
            return Err(CallError::new("SANDBOX_OFF", "The bot's computer is switched off. Turn on Sandbox in Settings > General > Labs."));
        }
        if !rules::tool_access(ctx.trust).sandbox {
            return Err(CallError::new("TOOL_NOT_ALLOWED", "This chat may not use the bot's computer."));
        }
        let Some(tool) = Tool::parse(tool) else {
            return Err(CallError::new("UNKNOWN_TOOL", "There is no such computer tool."));
        };
        let id = run_id(&ctx.bot, &ctx.conversation);
        let pointer = self.runs.lock().get(&id).and_then(|r| r.pointer);
        let opts = *self.opts.lock();
        let request = validate(tool, args, pointer, &opts)?;
        let Some(port) = port else {
            return Err(CallError::new("SANDBOX_NOT_SET_UP", "The bot's computer is not set up yet. Set it up in Settings > Potato Bot."));
        };

        let now = self.host.now_ms();
        self.hold_screen(port, ctx, &id, now)?;
        self.spend(ctx, now, request.cost())?;

        let _one_at_a_time = self.serial.lock();
        let answer = self.perform(port, ctx, &id, request)?;
        let mut answer = answer;
        redact(&mut answer);
        Ok(answer)
    }

    fn hold_screen(&self, port: &Port, ctx: &Context, id: &str, now: i64) -> Result<(), CallError> {
        let mut lease = port.lease.lock();
        let mut runs = self.runs.lock();
        let run = runs.entry(id.to_string()).or_default();
        let had = run.lease_id.take();
        if let Some(held) = &had {
            if lease.renew(held, now).is_ok() {
                run.lease_id = had;
                return Ok(());
            }
            run.stale = true;
        }
        match lease.take_for_bot(&ctx.bot, id, now) {
            Ok(new_id) => {
                run.lease_id = Some(new_id);
                if run.stale {
                    // The screen was used by someone else, or the hold ran out, since the last call.
                    run.stale = false;
                    run.frame = None;
                    return Err(CallError::new("LEASE_LOST", "The screen was used by someone else since your last call. Look at the screen again before you act."));
                }
                Ok(())
            }
            Err(LeaseError::PersonHasControl) => Err(CallError {
                code: "LEASE_HELD_BY_PERSON",
                message: "The person is using the screen right now. Tell them you are waiting, and try again after they hand it back.".to_string(),
                needs_you: true,
            }),
            Err(other) => Err(CallError::new(other.code(), "The screen could not be taken.")),
        }
    }

    fn spend(&self, ctx: &Context, now: i64, cost: u32) -> Result<(), CallError> {
        let cap = store::load_settings(&self.root).map(|s| s.sandbox_actions_per_day).unwrap_or(super::cost::DEFAULT_SANDBOX_ACTIONS);
        let day = self.host.local_day(now);
        match super::cost::spend_sandbox_actions(&self.root, &day, &ctx.bot, cost, cap) {
            Ok(spent) if spent.allowed => Ok(()),
            Ok(spent) => Err(CallError::new(
                "SANDBOX_DAILY_CAP",
                format!("The bot has used {} of its {} computer actions for today. It can use the computer again tomorrow, or the person can raise the limit.", spent.used, spent.cap),
            )),
            Err(_) => Err(CallError::new("SANDBOX_COUNT_FAILED", "The day's action count could not be saved, so nothing was run.")),
        }
    }

    fn perform(&self, port: &Port, ctx: &Context, id: &str, request: Request) -> Result<Value, CallError> {
        let sandbox: &dyn Sandbox = &*port.sandbox;
        match request {
            Request::Observe => {
                let observation = sandbox.observe()?;
                let mut out = self.describe(id, &observation, ctx.want_image);
                if !ctx.want_image {
                    let page = front_page_text(sandbox, &observation);
                    let words = if page.is_some() { "The page text is in page." } else { "Use browser_navigate and browser_snapshot for web pages, and computer_act with focus to open an app." };
                    out.insert("note".into(), json!(format!("This model cannot see pictures, so you get words. {words}")));
                    if let Some(page) = page {
                        out.insert("page".into(), page);
                    }
                }
                Ok(Value::Object(out))
            }
            Request::Act { actions, show } => {
                let observation = sandbox.act(&actions)?;
                let mut out = self.describe(id, &observation, ctx.want_image && show);
                out.insert("done".into(), json!(actions.len()));
                if !show {
                    for key in ["image", "frameId", "changed", "width", "height", "pointer", "activeWindow"] {
                        out.remove(key);
                    }
                }
                Ok(Value::Object(out))
            }
            Request::Navigate { url } => Ok(browser::run(sandbox, &browser::navigate_args(&url)?)?),
            Request::Snapshot => Ok(browser::run(sandbox, &browser::snapshot_args())?),
            Request::BrowserAct { actions } => Ok(browser::run(sandbox, &browser::act_args(&actions)?)?),
        }
    }

    /// The part of an answer that is about the screen. Remembers the frame and the pointer for the run.
    pub(super) fn describe(&self, id: &str, obs: &Observation, with_image: bool) -> Map<String, Value> {
        let frame = format!("f-{}", hex8(&Sha256::digest(&obs.png)));
        let changed = {
            let mut runs = self.runs.lock();
            let run = runs.entry(id.to_string()).or_default();
            let changed = run.frame.as_ref().map(|last| *last != frame);
            run.frame = Some(frame.clone());
            if obs.pointer.is_some() {
                run.pointer = obs.pointer;
            }
            changed
        };
        let mut out = Map::new();
        out.insert("frameId".into(), json!(frame));
        out.insert("width".into(), json!(obs.width));
        out.insert("height".into(), json!(obs.height));
        if let Some((x, y)) = obs.pointer {
            out.insert("pointer".into(), json!({ "x": x, "y": y }));
        }
        if let Some(title) = &obs.active_window_title {
            let title: String = title.chars().filter(|c| !c.is_control()).take(MAX_TITLE_CHARS).collect();
            out.insert("activeWindow".into(), json!(title));
        }
        out.insert("changed".into(), changed.map_or(Value::Null, Value::Bool));
        if with_image {
            if obs.png.len() <= MAX_IMAGE_BYTES {
                out.insert("image".into(), json!({ "mime": "image/png", "data": base64::engine::general_purpose::STANDARD.encode(&obs.png) }));
            } else {
                out.insert("note".into(), json!("The picture was too big to send. Use computer_observe again, or the browser tools."));
            }
        }
        out
    }

    /// The run is over (the bot answered): its hold on the screen goes. The person's hold is not touched.
    pub fn end_run(&self, port: Option<&Port>, bot: &str, conversation: &str) {
        let id = run_id(bot, conversation);
        self.runs.lock().remove(&id);
        if let Some(port) = port {
            port.lease.lock().end_run(&id);
        }
    }

    /// The bot's brain is gone: every run of that bot ends.
    pub fn end_bot(&self, port: Option<&Port>, bot: &str) {
        let prefix = format!("{bot}@");
        let ids: Vec<String> = self.runs.lock().keys().filter(|k| k.starts_with(&prefix)).cloned().collect();
        for id in ids {
            self.runs.lock().remove(&id);
            if let Some(port) = port {
                port.lease.lock().end_run(&id);
            }
        }
    }

    /// The bot, holding the screen, asks the person to step in (the "Needs you" card, 30 minutes). The Computer
    /// panel (screen links, piece S2) shows the card; no brain tool raises it yet.
    #[allow(dead_code)]
    pub fn needs_you(&self, port: &Port, bot: &str, conversation: &str, reason: &str) -> Result<NeedsYou, CallError> {
        let id = run_id(bot, conversation);
        let held = self.runs.lock().get(&id).and_then(|r| r.lease_id.clone());
        let Some(held) = held else {
            return Err(CallError::new(LeaseError::NotTheHolder.code(), "The bot does not hold the screen."));
        };
        let now = self.host.now_ms();
        port.lease.lock().ask_for_help(&held, reason, now).map_err(|e| CallError::new(e.code(), "The bot cannot ask now."))
    }

    /// True when the lease table says this bot is using the screen right now (for the Computer panel's header).
    #[allow(dead_code)]
    pub fn bot_is_using(&self, port: &Port, bot: &str) -> bool {
        let now = self.host.now_ms();
        port.lease.lock().current(now).is_some_and(|l| matches!(&l.holder, Holder::Bot(b) if b == bot))
    }
}

/// Eight hex characters of a digest.
fn hex8(digest: &[u8]) -> String {
    digest.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

/// The words of the front Chrome tab, when the front window looks like one and the page tool answers.
pub(super) fn front_page_text(sandbox: &dyn Sandbox, obs: &Observation) -> Option<Value> {
    let title = obs.active_window_title.as_deref()?.to_ascii_lowercase();
    if !(title.contains("chrome") || title.contains("chromium")) {
        return None;
    }
    let reply = browser::run(sandbox, &browser::snapshot_args()).ok()?;
    (reply.get("ok").and_then(Value::as_bool) == Some(true)).then_some(reply)
}

/// Every string in an answer goes through the redactor (a page can print a key). The picture's bytes are left alone.
fn redact(value: &mut Value) {
    match value {
        Value::String(s) => *s = crate::redact::text(s).into_inner(),
        Value::Array(items) => items.iter_mut().for_each(redact),
        Value::Object(map) => {
            for (key, inner) in map.iter_mut() {
                if key == "image" {
                    continue;
                }
                redact(inner);
            }
        }
        _ => {}
    }
}
