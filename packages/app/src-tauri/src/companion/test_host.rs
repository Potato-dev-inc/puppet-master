//! A fake `Host` for the service and route tests: a clock you move by hand, panes you set, and a record of
//! everything the service asked for. No terminal, no thread, no Tauri.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;

use chrono::TimeZone;
use parking_lot::Mutex;
use serde_json::json;

use super::host::{AgentLimit, Blocking, FinishedTurn, Host, RecallAnswer, RecallHit, Reply, SpawnSpec};
use super::keeper::{Pane, Status};
use super::manage::Draft;
use super::model::CompanionView;
use super::Service;
use crate::tui::InstructionFile;

/// Wednesday 2026-10-28 08:00:00 UTC. The fake zone is UTC, so local time equals this.
pub fn start_ms() -> i64 {
    chrono::Utc.with_ymd_and_hms(2026, 10, 28, 8, 0, 0).unwrap().timestamp_millis()
}

pub struct FakeHost {
    now: AtomicI64,
    pub offset: AtomicI64,
    pub panes: Mutex<HashMap<String, Pane>>,
    pub spawned: Mutex<Vec<SpawnSpec>>,
    pub killed: Mutex<Vec<String>>,
    pub sent: Mutex<Vec<(String, String)>>,
    pub waiting: Mutex<Vec<String>>,
    pub blocking: Mutex<Option<Blocking>>,
    pub sign_in: AtomicBool,
    /// A dismissible startup notice is on the pane (by pane id), and whether its keys close it.
    pub notice: Mutex<Vec<String>>,
    pub notice_obeys: AtomicBool,
    /// Every pane the keeper pressed the notice keys on.
    pub notice_presses: Mutex<Vec<String>>,
    pub activity: Mutex<Option<String>>,
    pub reply: Mutex<Option<Reply>>,
    pub sentence: Mutex<Option<Reply>>,
    pub raised: Mutex<Vec<String>>,
    pub cleared: Mutex<Vec<String>>,
    pub released: Mutex<Vec<String>>,
    pub published: Mutex<Vec<Vec<CompanionView>>>,
    pub agent_error: Mutex<Option<String>>,
    pub spawn_error: Mutex<Option<String>>,
    pub send_error: Mutex<Option<String>>,
    pub files: Mutex<Vec<InstructionFile>>,
    /// Instruction files of one agent (id), when it differs from `files`.
    pub files_of: Mutex<HashMap<String, Vec<InstructionFile>>>,
    /// `reply_pushed` calls: (companion id, name, text, enabled).
    pub pushed: Mutex<Vec<(String, String, String, bool)>>,
    /// The turns `finished_turns` lists (the turn summaries).
    pub turns: Mutex<Vec<FinishedTurn>>,
    /// Agents at a usage limit: (id, account) -> when it ends.
    pub limits: Mutex<HashMap<(String, Option<String>), Option<i64>>>,
    /// Agents `agent_ok` refuses (by id), besides the single `agent_error` for all.
    pub refused: Mutex<Vec<String>>,
    /// The chat search index: on or off, what it holds (pane, hit), and the pane lists it was asked about.
    pub recall_enabled: AtomicBool,
    pub recall_hits: Mutex<Vec<(String, RecallHit)>>,
    pub recall_asked: Mutex<Vec<Vec<String>>>,
    /// The account's known folders (`Host::places`): a fake home that is not a real folder.
    pub places: Mutex<super::broad::Places>,
}

impl FakeHost {
    pub fn new(now_ms: i64) -> Arc<Self> {
        Arc::new(Self {
            now: AtomicI64::new(now_ms),
            offset: AtomicI64::new(0),
            panes: Mutex::new(HashMap::new()),
            spawned: Mutex::new(Vec::new()),
            killed: Mutex::new(Vec::new()),
            sent: Mutex::new(Vec::new()),
            waiting: Mutex::new(Vec::new()),
            blocking: Mutex::new(None),
            sign_in: AtomicBool::new(false),
            notice: Mutex::new(Vec::new()),
            notice_obeys: AtomicBool::new(true),
            notice_presses: Mutex::new(Vec::new()),
            activity: Mutex::new(None),
            reply: Mutex::new(None),
            sentence: Mutex::new(None),
            raised: Mutex::new(Vec::new()),
            cleared: Mutex::new(Vec::new()),
            released: Mutex::new(Vec::new()),
            published: Mutex::new(Vec::new()),
            agent_error: Mutex::new(None),
            spawn_error: Mutex::new(None),
            send_error: Mutex::new(None),
            files: Mutex::new(Vec::new()),
            files_of: Mutex::new(HashMap::new()),
            pushed: Mutex::new(Vec::new()),
            turns: Mutex::new(Vec::new()),
            limits: Mutex::new(HashMap::new()),
            refused: Mutex::new(Vec::new()),
            recall_enabled: AtomicBool::new(false),
            recall_hits: Mutex::new(Vec::new()),
            recall_asked: Mutex::new(Vec::new()),
            places: Mutex::new(super::broad::Places::for_home(std::path::Path::new("C:/Users/you"))),
        })
    }

    pub fn now(&self) -> i64 {
        self.now.load(Ordering::SeqCst)
    }

    pub fn advance_ms(&self, ms: i64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }

    pub fn set_now(&self, ms: i64) {
        self.now.store(ms, Ordering::SeqCst);
    }

    pub fn set_pane(&self, pane_id: &str, pane: Pane) {
        self.panes.lock().insert(pane_id.to_string(), pane);
    }

    pub fn remove_pane(&self, pane_id: &str) {
        self.panes.lock().remove(pane_id);
    }

    pub fn spawn_count(&self) -> usize {
        self.spawned.lock().len()
    }

    pub fn sent_texts(&self) -> Vec<String> {
        self.sent.lock().iter().map(|(_, text)| text.clone()).collect()
    }
}

impl Host for FakeHost {
    fn now_ms(&self) -> i64 {
        self.now.load(Ordering::SeqCst)
    }

    fn offset_secs(&self) -> i32 {
        self.offset.load(Ordering::SeqCst) as i32
    }

    fn pane(&self, pane_id: &str) -> Pane {
        self.panes.lock().get(pane_id).copied().unwrap_or(Pane::Missing)
    }

    fn spawn(&self, spec: &SpawnSpec) -> Result<(), String> {
        if let Some(error) = self.spawn_error.lock().clone() {
            return Err(error);
        }
        self.spawned.lock().push(spec.clone());
        self.panes.lock().insert(spec.pane_id.clone(), Pane::Alive(Status::Running));
        Ok(())
    }

    fn kill(&self, pane_id: &str) {
        self.killed.lock().push(pane_id.to_string());
        self.notice.lock().retain(|p| p != pane_id);
        self.panes.lock().remove(pane_id);
    }

    fn send(&self, pane_id: &str, text: &str) -> Result<(), String> {
        if let Some(error) = self.send_error.lock().clone() {
            return Err(error);
        }
        self.sent.lock().push((pane_id.to_string(), text.to_string()));
        Ok(())
    }

    fn waiting(&self, _pane_id: &str) -> Vec<String> {
        self.waiting.lock().clone()
    }

    fn blocking(&self, _pane_id: &str) -> Option<Blocking> {
        self.blocking.lock().clone()
    }

    fn sign_in_gate(&self, _pane_id: &str) -> bool {
        self.sign_in.load(Ordering::SeqCst)
    }

    fn notice_gate(&self, pane_id: &str) -> bool {
        self.notice.lock().iter().any(|p| p == pane_id)
    }

    fn dismiss_notice(&self, pane_id: &str) -> bool {
        if !self.notice_gate(pane_id) {
            return false;
        }
        self.notice_presses.lock().push(pane_id.to_string());
        if self.notice_obeys.load(Ordering::SeqCst) {
            self.notice.lock().retain(|p| p != pane_id);
            // The menu is gone: the pane settles at its input box.
            self.panes.lock().insert(pane_id.to_string(), Pane::Alive(Status::Idle));
        }
        true
    }

    fn activity(&self, _pane_id: &str) -> Option<String> {
        self.activity.lock().clone()
    }

    fn last_reply(&self, _pane_id: &str, _companion_id: &str) -> Option<Reply> {
        self.reply.lock().clone()
    }

    fn last_sentence(&self, _pane_id: &str) -> Option<Reply> {
        self.sentence.lock().clone()
    }

    fn finished_turns(&self, since_ms: i64) -> Vec<FinishedTurn> {
        let mut turns: Vec<FinishedTurn> = self.turns.lock().iter().filter(|t| t.ended_at_ms >= since_ms).cloned().collect();
        turns.sort_by_key(|t| t.ended_at_ms);
        turns
    }

    fn release_reader(&self, pane_id: &str, _companion_id: &str) {
        self.released.lock().push(pane_id.to_string());
    }

    fn recall_on(&self) -> bool {
        self.recall_enabled.load(Ordering::SeqCst)
    }

    /// Like the real index: only the chats of the panes asked for, newest first, at most `limit`.
    fn recall(&self, panes: &[String], _query: &str, limit: usize) -> RecallAnswer {
        self.recall_asked.lock().push(panes.to_vec());
        if !self.recall_enabled.load(Ordering::SeqCst) {
            return RecallAnswer { enabled: false, hits: Vec::new() };
        }
        let mut hits: Vec<RecallHit> =
            self.recall_hits.lock().iter().filter(|(pane, _)| panes.contains(pane)).map(|(_, hit)| hit.clone()).collect();
        hits.sort_by_key(|hit| std::cmp::Reverse(hit.at_ms));
        hits.truncate(limit);
        RecallAnswer { enabled: true, hits }
    }

    fn raise_stopped(&self, companion_id: &str, _name: &str) {
        self.raised.lock().push(companion_id.to_string());
    }

    fn clear_stopped(&self, companion_id: &str) {
        self.cleared.lock().push(companion_id.to_string());
    }

    fn places(&self) -> super::broad::Places {
        self.places.lock().clone()
    }

    fn agent_ok(&self, agent_type: &str, _folder: &str) -> Result<(), String> {
        if self.refused.lock().iter().any(|id| id == agent_type) {
            return Err("This agent is not installed on this computer.".to_string());
        }
        match self.agent_error.lock().clone() {
            Some(reason) => Err(reason),
            None => Ok(()),
        }
    }

    fn agent_label(&self, agent_type: &str) -> String {
        format!("Agent {agent_type}")
    }

    fn instruction_files(&self, agent_type: &str) -> Vec<InstructionFile> {
        if let Some(own) = self.files_of.lock().get(agent_type) {
            return own.clone();
        }
        self.files.lock().clone()
    }

    fn agent_limit(&self, agent_type: &str, account: Option<&str>) -> Option<AgentLimit> {
        self.limits.lock().get(&(agent_type.to_string(), account.map(str::to_string))).map(|reset_at_ms| AgentLimit { reset_at_ms: *reset_at_ms })
    }

    fn publish(&self, views: &[CompanionView]) {
        self.published.lock().push(views.to_vec());
    }

    fn reply_pushed(&self, companion_id: &str, name: &str, text: &str, enabled: bool) {
        self.pushed.lock().push((companion_id.into(), name.into(), text.into(), enabled));
    }
}

/// A fresh temp folder.
pub fn temp_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A setup-sheet draft for `name` living in `folder`.
pub fn draft(name: &str, folder: &std::path::Path) -> Draft {
    serde_json::from_value(json!({
        "name": name,
        "avatar": "mochi",
        "persona": "calm",
        "agentType": "claude",
        "folder": folder.display().to_string(),
    }))
    .unwrap()
}

/// A service over a fake host with one companion, Mochi, already created (and started: the fake host
/// answers every spawn). Returns the app data root too.
pub fn service_with_one() -> (Service, Arc<FakeHost>, PathBuf) {
    let root = temp_dir("pm-companion-service");
    let host = FakeHost::new(start_ms());
    let service = Service::new(host.clone(), root.clone());
    service.create(draft("Mochi", &root.join("mochi"))).unwrap();
    (service, host, root)
}
