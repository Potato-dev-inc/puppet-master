//! Global Ask bar: a Spotlight-style bar opened from anywhere in Windows that sends one question
//! to an agent through the read-only one-shot `quick_ask` path and shows the answer in the bar.
//! If the bar was hidden before the answer arrived, the answer becomes a notice (an Inbox card in
//! the cockpit) instead.
//!
//! Everything here is pure or injectable so it can be tested without a window: the window spec,
//! the shortcut sync (`ShortcutRegistrar`), clipboard cutting, the `quick_ask` wiring (`run_ask`
//! takes the runner as a closure, the same way the agent doctor does) and the notice list.
//! `glue.rs` holds the Tauri side (the real window, the real shortcut, events). Generic code never
//! looks at agent names: the agents offered are whatever the registry says can answer a headless
//! read-only question (`tui::read_only_backends`).

pub mod glue;
#[cfg(test)]
mod tests;

use crate::agent_runs::quick::QuickAskRequest;
use crate::operations::OperationError;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;

pub const WINDOW_LABEL: &str = "askbar";
pub const WINDOW_URL: &str = "index.html?askbar=1";
pub const DEFAULT_SHORTCUT: &str = "Ctrl+Alt+Space";
/// The shortcuts the Settings list offers (a recorder would need a key-capture control).
pub const SHORTCUT_CHOICES: [&str; 4] = ["Ctrl+Alt+Space", "Ctrl+Shift+Space", "Alt+Space", "Ctrl+Alt+K"];
/// Clipboard text attached to a question is cut to this many characters.
pub const CLIPBOARD_LIMIT: usize = 8_000;
/// How long the bar waits for an answer before it says so (quick_ask clamps at 120 s anyway).
pub const ANSWER_WAIT_MS: u64 = 90_000;
const MAX_NOTICES: usize = 20;
/// A blur this soon after the bar was shown is the focus handover, not the user clicking away.
pub const BLUR_GRACE_MS: u64 = 300;

// ---------------------------------------------------------------- settings

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Off until the user turns it on in Settings > General.
    pub enabled: bool,
    pub shortcut: String,
    pub default_agent: Option<String>,
    pub last_agent: Option<String>,
    /// Off by default: the clipboard is attached only when the user asks. On: each open reads it.
    pub attach_clipboard_default: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            shortcut: DEFAULT_SHORTCUT.into(),
            default_agent: None,
            last_agent: None,
            attach_clipboard_default: false,
        }
    }
}

impl Settings {
    /// Anything outside the choice list falls back to the default shortcut; blank agents become `None`.
    pub fn sanitized(mut self) -> Self {
        if !SHORTCUT_CHOICES.contains(&self.shortcut.as_str()) {
            self.shortcut = DEFAULT_SHORTCUT.into();
        }
        let clean = |value: Option<String>| value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        self.default_agent = clean(self.default_agent.take());
        self.last_agent = clean(self.last_agent.take());
        self
    }
}

static SETTINGS: Lazy<Mutex<Settings>> = Lazy::new(|| Mutex::new(Settings::default()));

fn settings_path() -> std::path::PathBuf {
    crate::app_paths::app_data_dir().join("askbar.json")
}

pub fn settings() -> Settings {
    SETTINGS.lock().clone()
}

/// Apply, save and return the settings (the caller syncs the shortcut and window).
pub fn set_settings(next: Settings) -> Settings {
    let applied = next.sanitized();
    *SETTINGS.lock() = applied.clone();
    save_settings(&applied);
    applied
}

pub fn remember_agent(agent: &str) {
    let mut guard = SETTINGS.lock();
    if guard.last_agent.as_deref() != Some(agent) {
        guard.last_agent = Some(agent.to_string());
        let saved = guard.clone();
        drop(guard);
        save_settings(&saved);
    }
}

fn save_settings(settings: &Settings) {
    if let Ok(text) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::create_dir_all(crate::app_paths::app_data_dir());
        let _ = std::fs::write(settings_path(), text);
    }
}

pub fn load_settings() -> Settings {
    let loaded = std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
        .unwrap_or_default()
        .sanitized();
    *SETTINGS.lock() = loaded.clone();
    loaded
}

// ---------------------------------------------------------------- window

/// What the bar's window looks like. `glue::build_window` applies it field by field.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowSpec {
    pub label: &'static str,
    pub url: &'static str,
    pub width: f64,
    /// Height of the input line alone.
    pub height: f64,
    /// Height once an answer is showing.
    pub expanded_height: f64,
    pub decorations: bool,
    pub transparent: bool,
    pub always_on_top: bool,
    pub resizable: bool,
    pub skip_taskbar: bool,
    pub shadow: bool,
    /// Created hidden: it appears only on the user's shortcut.
    pub visible: bool,
    /// Never takes focus when created.
    pub focused: bool,
}

pub fn window_spec() -> WindowSpec {
    WindowSpec {
        label: WINDOW_LABEL,
        url: WINDOW_URL,
        width: 640.0,
        height: 64.0,
        expanded_height: 420.0,
        decorations: false,
        transparent: true,
        always_on_top: true,
        resizable: false,
        skip_taskbar: true,
        shadow: false,
        visible: false,
        focused: false,
    }
}

pub fn height_for(expanded: bool) -> f64 {
    let spec = window_spec();
    if expanded {
        spec.expanded_height
    } else {
        spec.height
    }
}

/// Physical top-left for the bar on a monitor `(x, y, width, height)`: horizontally centred, a
/// fifth of the way down (where Spotlight sits).
pub fn spotlight_position(monitor: (i32, i32, u32, u32), window_width: u32) -> (i32, i32) {
    let (mx, my, mw, mh) = monitor;
    let x = mx + (mw.saturating_sub(window_width) / 2) as i32;
    let y = my + (f64::from(mh) * 0.2) as i32;
    (x, y)
}

/// Hide on blur, except for the focus handover right after the bar was shown.
pub fn should_hide_on_blur(now_ms: u64, shown_at_ms: u64) -> bool {
    now_ms.saturating_sub(shown_at_ms) >= BLUR_GRACE_MS
}

// ---------------------------------------------------------------- shortcut

/// The OS shortcut registry, abstracted so the registration rules are testable.
pub trait ShortcutRegistrar {
    fn register(&mut self, shortcut: &str) -> Result<(), String>;
    fn unregister(&mut self, shortcut: &str);
}

/// Make the registered shortcut match the settings: nothing while disabled; otherwise exactly
/// the chosen combination (re-registered when it changed).
pub fn sync_shortcut(
    registrar: &mut dyn ShortcutRegistrar,
    current: &mut Option<String>,
    settings: &Settings,
) -> Result<(), String> {
    let wanted = settings.enabled.then(|| settings.shortcut.clone());
    if *current == wanted {
        return Ok(());
    }
    if let Some(old) = current.take() {
        registrar.unregister(&old);
    }
    if let Some(new) = wanted {
        registrar.register(&new)?;
        *current = Some(new);
    }
    Ok(())
}

// ---------------------------------------------------------------- clipboard

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Clip {
    /// At most `CLIPBOARD_LIMIT` characters.
    pub text: String,
    /// Characters on the clipboard before cutting.
    pub chars: usize,
    pub truncated: bool,
}

/// Cut clipboard text for attaching; `None` when there is nothing but whitespace.
pub fn clip_text(raw: &str) -> Option<Clip> {
    if raw.trim().is_empty() {
        return None;
    }
    let chars = raw.chars().count();
    if chars <= CLIPBOARD_LIMIT {
        return Some(Clip { text: raw.to_string(), chars, truncated: false });
    }
    let text: String = raw.chars().take(CLIPBOARD_LIMIT).collect();
    Some(Clip { text, chars, truncated: true })
}

/// The task text the agent receives: the question, then the clipboard as quoted context.
pub fn compose_task(question: &str, clipboard: Option<&str>) -> String {
    let question = question.trim();
    match clipboard.and_then(clip_text) {
        Some(clip) => format!(
            "{question}\n\nContext from my clipboard{}:\n```\n{}\n```",
            if clip.truncated { " (cut short)" } else { "" },
            clip.text
        ),
        None => question.to_string(),
    }
}

// ---------------------------------------------------------------- agents

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentEntry {
    pub id: String,
    pub label: String,
    /// A warm standby worker is waiting.
    pub ready: bool,
}

/// The agents the bar offers: capable ones (given by the registry) that are installed.
pub fn list_agents(
    capable: &[(String, String)],
    installed: &dyn Fn(&str) -> bool,
    ready: &[String],
) -> Vec<AgentEntry> {
    capable
        .iter()
        .filter(|(id, _)| installed(id))
        .map(|(id, label)| AgentEntry {
            id: id.clone(),
            label: label.clone(),
            ready: ready.iter().any(|r| r.eq_ignore_ascii_case(id)),
        })
        .collect()
}

// ---------------------------------------------------------------- asking

#[derive(Debug, Clone, Deserialize)]
pub struct AskRequest {
    pub ask_id: String,
    pub question: String,
    pub agent: String,
    #[serde(default)]
    pub clipboard: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AskError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AskResult {
    pub ask_id: String,
    pub agent: String,
    pub agent_label: String,
    pub question: String,
    pub ok: bool,
    pub answer: Option<String>,
    pub error: Option<AskError>,
}

impl AskResult {
    fn failure(request: &AskRequest, label: &str, code: &str, message: impl Into<String>) -> Self {
        Self {
            ask_id: request.ask_id.clone(),
            agent: request.agent.clone(),
            agent_label: label.to_string(),
            question: request.question.trim().to_string(),
            ok: false,
            answer: None,
            error: Some(AskError { code: code.to_string(), message: message.into() }),
        }
    }
}

/// The `quick_ask` request an ask becomes: read-only, headless, fresh context.
pub fn quick_request(request: &AskRequest, project: Option<String>) -> QuickAskRequest {
    QuickAskRequest {
        task: compose_task(&request.question, request.clipboard.as_deref()),
        project_path: project,
        backend: Some(request.agent.clone()),
        model: None,
        wait_ms: Some(ANSWER_WAIT_MS),
        read_only: Some(true),
        idempotency_key: None,
    }
}

/// Run one question through `runner` (the real `quick_ask` in production) and turn every outcome
/// into a plain `AskResult`: the answer, or a short human sentence for the error.
pub fn run_ask(
    request: &AskRequest,
    label: &str,
    project: Option<String>,
    runner: &dyn Fn(QuickAskRequest) -> Result<Value, OperationError>,
) -> AskResult {
    if request.question.trim().is_empty() {
        return AskResult::failure(request, label, "EMPTY_QUESTION", "Type a question first.");
    }
    match runner(quick_request(request, project)) {
        Ok(value) => match value.get("status").and_then(Value::as_str) {
            Some("running") => AskResult::failure(
                request,
                label,
                "ASK_TIMEOUT",
                format!("{label} is taking longer than expected. Continue in a pane to watch it work."),
            ),
            Some("needs_input") => AskResult::failure(
                request,
                label,
                "ASK_NEEDS_INPUT",
                format!("{label} stopped to ask for permission. Continue in a pane to answer it."),
            ),
            _ => match value.get("answer").and_then(Value::as_str).map(str::trim).filter(|a| !a.is_empty()) {
                Some(answer) => AskResult {
                    ask_id: request.ask_id.clone(),
                    agent: request.agent.clone(),
                    agent_label: label.to_string(),
                    question: request.question.trim().to_string(),
                    ok: true,
                    answer: Some(answer.to_string()),
                    error: None,
                },
                None => AskResult::failure(request, label, "ASK_EMPTY", format!("{label} gave an empty answer.")),
            },
        },
        Err(error) => {
            let (_, plain) = crate::agent_health::classify_failure(&error.code, &error.message);
            AskResult::failure(request, label, &error.code, plain)
        }
    }
}

/// The `run_agent` request for "Continue in a pane": a visible, kept pane whose first task is the
/// question (with the clipboard), not read-only because the user is now steering it.
pub fn continue_request(agent: &str, question: &str, clipboard: Option<&str>, project: Option<String>) -> Value {
    json!({
        "task": compose_task(question, clipboard),
        "project_path": project,
        "agent_type": agent,
        "headless": false,
        "background": true,
        "keep_pane": true,
        "read_only": false,
        "context_policy": "fresh",
        "wait_ms": 1000,
    })
}

// ---------------------------------------------------------------- notices

/// An answer that arrived while the bar was hidden (an Inbox card).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Notice {
    pub id: String,
    pub agent: String,
    pub agent_label: String,
    pub question: String,
    pub result: AskResult,
    pub at_ms: u64,
}

/// A notice is needed exactly when nobody could have seen the answer in the bar.
pub fn needs_notice(bar_visible: bool) -> bool {
    !bar_visible
}

#[derive(Default)]
pub struct Notices {
    items: VecDeque<Notice>,
}

impl Notices {
    pub fn push(&mut self, result: &AskResult, at_ms: u64) -> Notice {
        let notice = Notice {
            id: format!("ask-{}", result.ask_id),
            agent: result.agent.clone(),
            agent_label: result.agent_label.clone(),
            question: result.question.clone(),
            result: result.clone(),
            at_ms,
        };
        self.items.retain(|n| n.id != notice.id);
        self.items.push_front(notice.clone());
        self.items.truncate(MAX_NOTICES);
        notice
    }
    pub fn take(&mut self, id: &str) -> Option<Notice> {
        let at = self.items.iter().position(|n| n.id == id)?;
        self.items.remove(at)
    }
    pub fn list(&self) -> Vec<Notice> {
        self.items.iter().cloned().collect()
    }
}

static NOTICES: Lazy<Mutex<Notices>> = Lazy::new(|| Mutex::new(Notices::default()));

pub fn push_notice(result: &AskResult) -> Notice {
    NOTICES.lock().push(result, now_ms())
}
pub fn take_notice(id: &str) -> Option<Notice> {
    NOTICES.lock().take(id)
}
pub fn notices() -> Vec<Notice> {
    NOTICES.lock().list()
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
