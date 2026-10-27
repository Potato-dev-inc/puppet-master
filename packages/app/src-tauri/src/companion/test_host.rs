//! A fake `Host` for the service and route tests: a clock you move by hand, panes you set, and a record of
//! everything the service asked for. No terminal, no thread, no Tauri.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;

use chrono::TimeZone;
use parking_lot::Mutex;
use serde_json::json;

use super::host::{Blocking, Host, Reply, SpawnSpec};
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
    /// `reply_pushed` calls: (companion id, name, text, enabled).
    pub pushed: Mutex<Vec<(String, String, String, bool)>>,
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
            pushed: Mutex::new(Vec::new()),
        })
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

    fn activity(&self, _pane_id: &str) -> Option<String> {
        self.activity.lock().clone()
    }

    fn last_reply(&self, _pane_id: &str, _companion_id: &str) -> Option<Reply> {
        self.reply.lock().clone()
    }

    fn last_sentence(&self, _pane_id: &str) -> Option<Reply> {
        self.sentence.lock().clone()
    }

    fn release_reader(&self, pane_id: &str, _companion_id: &str) {
        self.released.lock().push(pane_id.to_string());
    }

    fn raise_stopped(&self, companion_id: &str, _name: &str) {
        self.raised.lock().push(companion_id.to_string());
    }

    fn clear_stopped(&self, companion_id: &str) {
        self.cleared.lock().push(companion_id.to_string());
    }

    fn agent_ok(&self, _agent_type: &str, _folder: &str) -> Result<(), String> {
        match self.agent_error.lock().clone() {
            Some(reason) => Err(reason),
            None => Ok(()),
        }
    }

    fn agent_label(&self, agent_type: &str) -> String {
        format!("Agent {agent_type}")
    }

    fn instruction_files(&self, _agent_type: &str) -> Vec<InstructionFile> {
        self.files.lock().clone()
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
