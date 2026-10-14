//! Shadow mode (§1.3, §6): hook events are recorded NEXT TO the screen's verdicts and compared.
//! Nothing here feeds a decision: no caller reads a verdict back to act on it. The record is the
//! data the founder uses to decide whether hooks may ever be used as evidence.
//!
//! Three comparisons, each a claim from one lane matched against the other lane within
//! [`WINDOW_MS`] (hooks are async and the screen needs a quiet period before it reads idle):
//!
//! | check | screen claim | hook claim |
//! |---|---|---|
//! | `turn_start` | idle -> running | `prompt_submitted` |
//! | `waiting` | -> waiting_input | `permission_request` |
//! | `turn_end` | running -> idle | `turn_end` / `turn_failed` / `turn_interrupted` |
//!
//! A matched pair is `agree`; a claim nobody matched before its window closed is `screen_only` or
//! `hook_only`. Panes whose hooks never reported are not compared (hooks may be off, untrusted or
//! unsupported there): no claim is recorded until the first hook event.
//!
//! Records go to `<app data>/hooks/shadow/<pane id>.jsonl`: kinds, statuses, times, ids and hashes.
//! No prompt, reply or command text.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::PathBuf;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};

use super::model::{HookEvent, HookKind};

/// How far apart a hook event and the matching screen change may be.
pub const WINDOW_MS: u64 = 15_000;
/// A pane's shadow file stops growing here.
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    TurnStart,
    Waiting,
    TurnEnd,
}

impl Check {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TurnStart => "turn_start",
            Self::Waiting => "waiting",
            Self::TurnEnd => "turn_end",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    Screen,
    Hook,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Agree,
    ScreenOnly,
    HookOnly,
}

#[derive(Debug, Clone)]
struct Claim {
    check: Check,
    lane: Lane,
    at_ms: u64,
}

#[derive(Debug, Default)]
struct PaneShadow {
    agent: Option<String>,
    screen_status: Option<String>,
    claims: Vec<Claim>,
}

/// Per agent and check: how often the lanes agreed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Tally {
    pub agree: u64,
    pub screen_only: u64,
    pub hook_only: u64,
}

pub struct Shadow {
    dir: Option<PathBuf>,
    /// Lines go to a writer thread (the process-wide recorder runs on the PTY reader thread).
    writer: Option<std::sync::mpsc::Sender<(PathBuf, String)>>,
    panes: HashMap<String, PaneShadow>,
    tallies: BTreeMap<(String, Check), Tally>,
}

impl Shadow {
    /// `dir`: where per-pane `.jsonl` records go (`None`: memory only). Writes are synchronous.
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self { dir, writer: None, panes: HashMap::new(), tallies: BTreeMap::new() }
    }

    /// Like [`Shadow::new`], but every line is appended by a background thread.
    pub fn with_writer_thread(dir: PathBuf) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<(PathBuf, String)>();
        let _ = std::thread::Builder::new().name("pm-hook-shadow-writer".into()).spawn(move || {
            for (path, line) in rx {
                append_line(&path, &line);
            }
        });
        Self { dir: Some(dir), writer: Some(tx), panes: HashMap::new(), tallies: BTreeMap::new() }
    }

    /// A pane's classified screen status changed (`pty://status`).
    pub fn observe_screen(&mut self, pane_id: &str, status: &str, now_ms: u64) {
        self.sweep(now_ms);
        let Some(pane) = self.panes.get_mut(pane_id) else {
            // No hook event from this pane yet: remember the status for the side-by-side record.
            self.panes.entry(pane_id.to_string()).or_default().screen_status = Some(status.to_string());
            return;
        };
        let prev = pane.screen_status.replace(status.to_string());
        if pane.agent.is_none() || prev.as_deref() == Some(status) {
            return;
        }
        let claim = match (prev.as_deref(), status) {
            (Some("idle"), "running") => Some(Check::TurnStart),
            (_, "waiting_input") => Some(Check::Waiting),
            (Some("running"), "idle") => Some(Check::TurnEnd),
            _ => None,
        };
        let facts = super::ledger::facts(pane_id);
        self.write(
            pane_id,
            json!({
                "t": now_ms, "lane": "screen", "status": status, "prev": prev,
                "hook_turn_in_flight": facts.as_ref().and_then(|f| f.turn_in_flight),
                "hook_pending_permissions": facts.as_ref().map(|f| f.pending_permissions),
            }),
        );
        if let Some(check) = claim {
            self.claim(pane_id, check, Lane::Screen, now_ms);
        }
    }

    /// A hook event was stored. `applied` is false for foreign, subagent and late events: they are
    /// recorded but claim nothing.
    pub fn observe_hook(&mut self, event: &HookEvent, applied: bool, now_ms: u64) {
        self.sweep(now_ms);
        let pane = self.panes.entry(event.pane_id.clone()).or_default();
        pane.agent.get_or_insert_with(|| event.agent.clone());
        let screen = pane.screen_status.clone();
        self.write(
            &event.pane_id,
            json!({
                "t": now_ms, "lane": "hook", "kind": event.kind, "vendor_event": event.vendor_event,
                "seq": event.seq, "turn": event.turn_key, "session": event.session,
                "applied": applied, "foreign": event.foreign, "subagent": event.subagent.is_some(),
                "screen": screen, "detail": event.detail,
            }),
        );
        if !applied {
            return;
        }
        let check = match event.kind {
            HookKind::PromptSubmitted => Some(Check::TurnStart),
            HookKind::PermissionRequest => Some(Check::Waiting),
            k if k.ends_turn() => Some(Check::TurnEnd),
            _ => None,
        };
        if let Some(check) = check {
            self.claim(&event.pane_id, check, Lane::Hook, now_ms);
        }
    }

    fn claim(&mut self, pane_id: &str, check: Check, lane: Lane, now_ms: u64) {
        let Some(pane) = self.panes.get_mut(pane_id) else { return };
        let other = pane
            .claims
            .iter()
            .position(|c| c.check == check && c.lane != lane && now_ms.saturating_sub(c.at_ms) <= WINDOW_MS);
        match other {
            Some(i) => {
                let matched = pane.claims.remove(i);
                let (screen_at, hook_at) = match lane {
                    Lane::Screen => (now_ms, matched.at_ms),
                    Lane::Hook => (matched.at_ms, now_ms),
                };
                self.verdict(pane_id, check, Verdict::Agree, Some(screen_at), Some(hook_at), now_ms);
            }
            None => pane.claims.push(Claim { check, lane, at_ms: now_ms }),
        }
    }

    /// Close every claim whose window passed without a match.
    pub fn sweep(&mut self, now_ms: u64) {
        let mut expired = Vec::new();
        for (pane_id, pane) in &mut self.panes {
            pane.claims.retain(|c| {
                let keep = now_ms.saturating_sub(c.at_ms) <= WINDOW_MS;
                if !keep {
                    expired.push((pane_id.clone(), c.clone()));
                }
                keep
            });
        }
        for (pane_id, c) in expired {
            let (verdict, screen_at, hook_at) = match c.lane {
                Lane::Screen => (Verdict::ScreenOnly, Some(c.at_ms), None),
                Lane::Hook => (Verdict::HookOnly, None, Some(c.at_ms)),
            };
            self.verdict(&pane_id, c.check, verdict, screen_at, hook_at, now_ms);
        }
    }

    fn verdict(&mut self, pane_id: &str, check: Check, verdict: Verdict, screen_at: Option<u64>, hook_at: Option<u64>, now_ms: u64) {
        let agent = self.panes.get(pane_id).and_then(|p| p.agent.clone()).unwrap_or_default();
        let tally = self.tallies.entry((agent.clone(), check)).or_default();
        match verdict {
            Verdict::Agree => tally.agree += 1,
            Verdict::ScreenOnly => tally.screen_only += 1,
            Verdict::HookOnly => tally.hook_only += 1,
        }
        let delta = screen_at.zip(hook_at).map(|(s, h)| s as i64 - h as i64);
        self.write(
            pane_id,
            json!({
                "t": now_ms, "check": check, "verdict": verdict, "agent": agent,
                "screen_at": screen_at, "hook_at": hook_at, "screen_minus_hook_ms": delta,
            }),
        );
        crate::flight::mark(pane_id, &format!("hook_shadow:{}:{}", check.as_str(), verdict_str(verdict)));
    }

    fn write(&mut self, pane_id: &str, record: Value) {
        let Some(dir) = &self.dir else { return };
        if !self.panes.contains_key(pane_id) {
            return;
        }
        let path = dir.join(format!("{}.jsonl", safe_name(pane_id)));
        let line = record.to_string();
        match &self.writer {
            Some(tx) => {
                let _ = tx.send((path, line));
            }
            None => append_line(&path, &line),
        }
    }

    pub fn tallies(&self) -> Vec<(String, Check, Tally)> {
        self.tallies.iter().map(|((agent, check), t)| (agent.clone(), *check, t.clone())).collect()
    }

    /// The pane is gone: its open claims close now (unmatched), then it is dropped.
    pub fn forget(&mut self, pane_id: &str, now_ms: u64) {
        if let Some(pane) = self.panes.get_mut(pane_id) {
            for claim in &mut pane.claims {
                claim.at_ms = 0;
            }
        }
        self.sweep(now_ms.max(WINDOW_MS + 1));
        self.panes.remove(pane_id);
    }

    pub fn file_for(&self, pane_id: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(format!("{}.jsonl", safe_name(pane_id))))
    }
}

/// Append one record; a file at [`MAX_FILE_BYTES`] stops growing.
fn append_line(path: &std::path::Path, line: &str) {
    if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) >= MAX_FILE_BYTES {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}

fn verdict_str(v: Verdict) -> &'static str {
    match v {
        Verdict::Agree => "agree",
        Verdict::ScreenOnly => "screen_only",
        Verdict::HookOnly => "hook_only",
    }
}

fn safe_name(pane_id: &str) -> String {
    pane_id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).take(80).collect()
}

// ---- the process-wide recorder ------------------------------------------------------------

static SHADOW: Lazy<Mutex<Shadow>> = Lazy::new(|| Mutex::new(Shadow::with_writer_thread(super::hooks_dir().join("shadow"))));
static SWEEPER: std::sync::Once = std::sync::Once::new();

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn start_sweeper() {
    SWEEPER.call_once(|| {
        let _ = std::thread::Builder::new().name("pm-hook-shadow".into()).spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            SHADOW.lock().sweep(now_ms());
        });
    });
}

/// Hook for `bridge/sse.rs::push_pane_status_sse`: the screen's verdict, recorded beside the hooks.
pub fn on_screen_status(pane_id: &str, status: &str) {
    SHADOW.lock().observe_screen(pane_id, status, now_ms());
}

pub(super) fn on_hook_event(event: &HookEvent, applied: bool) {
    start_sweeper();
    crate::flight::mark(&event.pane_id, &format!("hook:{}", event.kind.as_str()));
    SHADOW.lock().observe_hook(event, applied, now_ms());
}

pub(super) fn forget(pane_id: &str) {
    SHADOW.lock().forget(pane_id, now_ms());
}

/// Agreement per agent and check since launch (doctor and report).
pub fn tallies() -> Vec<(String, Check, Tally)> {
    SHADOW.lock().tallies()
}
