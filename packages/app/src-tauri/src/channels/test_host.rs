//! Text Your Agents: a fake `Host` for tests only. It records everything the engine does to the
//! rest of the app (text sent to panes, answers, audit lines, saves) and serves a scripted world.
//! No test using it talks to Telegram or to a real pane.

use super::config::{PairedChat, TelegramConfig};
use super::host::{CompanionLine, Host, PaneLine};
use crate::attention::answer::{Reply, Request};
use crate::attention::caps::DeviceCaps;
use crate::attention::model::{Counts, Item, Kind, Lane, Level, PaneRef, RiskView, Snapshot, State};
use crate::audit::Kind as AuditKind;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};

/// "Now" for every test: a fixed time, so nothing depends on the clock.
pub const T0: u64 = 1_800_000_000_000;
/// The paired person. In a private chat the chat id is the user id.
pub const PAIRED_ID: i64 = 1001;
pub const STRANGER_ID: i64 = 2002;

pub struct FakeHost {
    pub now: Mutex<u64>,
    pub config: Mutex<TelegramConfig>,
    /// Every config the engine saved, oldest first.
    pub saved: Mutex<Vec<TelegramConfig>>,
    pub save_fails: Mutex<bool>,
    pub panes: Mutex<Vec<PaneLine>>,
    pub items: Mutex<Vec<Item>>,
    pub questions: Mutex<Vec<crate::questions::Question>>,
    /// `(pane id, text)` for each successful `send_to_pane`.
    pub pane_texts: Mutex<Vec<(String, String)>>,
    pub send_result: Mutex<Result<(), String>>,
    pub companions: Mutex<Vec<CompanionLine>>,
    /// `(item id, request, caps)` for each `answer` call.
    pub answers: Mutex<Vec<(String, Request, DeviceCaps)>>,
    pub answer_reply: Mutex<Reply>,
    pub audits: Mutex<Vec<(AuditKind, String, Value)>>,
    pub changed_count: AtomicUsize,
}

impl FakeHost {
    /// A paired, enabled channel with all of watch, answer and type, turned on a minute ago.
    pub fn new() -> Self {
        let config = TelegramConfig {
            enabled: true,
            enabled_at_ms: Some(T0 - 60_000),
            bot_username: Some("anna_pm_bot".into()),
            bot_id: Some(777),
            paired: Some(PairedChat {
                chat_id: PAIRED_ID,
                user_id: PAIRED_ID,
                username: Some("anna_k".into()),
                first_name: Some("Anna".into()),
                paired_at_ms: T0 - 120_000,
            }),
            ..TelegramConfig::default()
        };
        Self {
            now: Mutex::new(T0),
            config: Mutex::new(config),
            saved: Mutex::new(Vec::new()),
            save_fails: Mutex::new(false),
            panes: Mutex::new(Vec::new()),
            items: Mutex::new(Vec::new()),
            questions: Mutex::new(Vec::new()),
            pane_texts: Mutex::new(Vec::new()),
            send_result: Mutex::new(Ok(())),
            companions: Mutex::new(Vec::new()),
            answers: Mutex::new(Vec::new()),
            answer_reply: Mutex::new((200, json!({"outcome": "answered", "choice_label": "Allowed once"}))),
            audits: Mutex::new(Vec::new()),
            changed_count: AtomicUsize::new(0),
        }
    }

    /// Nobody paired, channel off: the state before setup.
    pub fn unpaired() -> Self {
        let host = Self::new();
        *host.config.lock() = TelegramConfig { bot_username: Some("anna_pm_bot".into()), ..TelegramConfig::default() };
        host
    }

    pub fn set_now(&self, ms: u64) {
        *self.now.lock() = ms;
    }

    pub fn edit_config(&self, f: impl FnOnce(&mut TelegramConfig)) {
        f(&mut self.config.lock());
    }

    pub fn pane(&self, id: &str, heading: &str, agent: &str, status: &str) {
        self.panes.lock().push(PaneLine { id: id.into(), heading: heading.into(), agent_label: agent.into(), status: status.into() });
    }

    pub fn companion(&self, id: &str, name: &str, awake: bool) {
        self.companions.lock().push(CompanionLine { id: id.into(), name: name.into(), pane_id: format!("companion-{id}"), awake });
    }

    pub fn add_item(&self, item: Item) {
        self.items.lock().push(item);
    }

    pub fn audit_kinds(&self) -> Vec<AuditKind> {
        self.audits.lock().iter().map(|(k, _, _)| *k).collect()
    }

    pub fn changed_times(&self) -> usize {
        self.changed_count.load(Ordering::SeqCst)
    }
}

impl Default for FakeHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Host for FakeHost {
    fn now_ms(&self) -> u64 {
        *self.now.lock()
    }

    fn config(&self) -> TelegramConfig {
        self.config.lock().clone()
    }

    fn save_config(&self, c: &TelegramConfig) -> Result<(), String> {
        if *self.save_fails.lock() {
            return Err("disk full".into());
        }
        *self.config.lock() = c.clone();
        self.saved.lock().push(c.clone());
        Ok(())
    }

    fn panes(&self) -> Vec<PaneLine> {
        self.panes.lock().clone()
    }

    fn snapshot(&self) -> Snapshot {
        let items = self.items.lock().clone();
        let open = |lane: Lane| items.iter().filter(|i| i.state == State::Open && i.lane == lane).count() as u32;
        Snapshot { v: 1, rev: 1, counts: Counts { blocking: open(Lane::Blocking), ready: open(Lane::Ready) }, items }
    }

    fn question(&self, id: &str) -> Option<crate::questions::Question> {
        self.questions.lock().iter().find(|q| q.id == id).cloned()
    }

    fn send_to_pane(&self, pane_id: &str, text: &str) -> Result<(), String> {
        let result = self.send_result.lock().clone();
        if result.is_ok() {
            self.pane_texts.lock().push((pane_id.to_string(), text.to_string()));
        }
        result
    }

    fn companions(&self) -> Vec<CompanionLine> {
        self.companions.lock().clone()
    }

    fn answer(&self, item_id: &str, req: &Request, caps: DeviceCaps) -> Reply {
        self.answers.lock().push((item_id.to_string(), req.clone(), caps));
        self.answer_reply.lock().clone()
    }

    fn audit(&self, kind: AuditKind, summary: &str, detail: Value) {
        self.audits.lock().push((kind, summary.to_string(), detail));
    }

    fn changed(&self) {
        self.changed_count.fetch_add(1, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------------------------
// Items for the attention list

/// An approval card of this risk (`None` = no risk read) on pane `pane`, asking to run `command`.
pub fn approval(pane: &str, command: &str, risk: Option<Level>) -> Item {
    let level = risk.map(Level::as_str);
    let rec = crate::attention::answer::testing::rec("p1", command, level.unwrap_or("none"));
    let mut item = crate::attention::answer::testing::approval_item(pane, &rec, 1);
    item.risk = risk.map(|level| RiskView { level, word: level.word(), headline: Some("Run tests".into()), reasons: vec![] });
    item
}

/// A card of any other kind, with no prompt. Used for trust gates, consent, budget and limit.
pub fn card(kind: Kind, title: &str) -> Item {
    Item {
        id: format!("item-{}", kind.as_str()),
        key: format!("{}:x", kind.as_str()),
        kind,
        lane: Lane::Blocking,
        rank: 0,
        state: State::Open,
        since_ms: Some(1),
        expires_at_ms: None,
        pane: Some(PaneRef { id: "pane-x".into(), heading: "auth refactor".into(), agent: Some("Claude Code".into()), project: Some("shop".into()) }),
        title: title.into(),
        sentence: None,
        risk: None,
        prompt: None,
        actions: vec![],
        resolution: None,
    }
}

/// An `ask_user` question card for the question with `question_id`.
pub fn question_item(question_id: &str, title: &str) -> Item {
    let mut item = card(Kind::Question, title);
    item.id = format!("item-question-{question_id}");
    item.key = format!("question:{question_id}");
    item.rank = 2;
    item
}

/// A pending question in the store shape, with these `(id, label)` choices.
pub fn pending_question(text: &str, choices: &[(&str, &str)]) -> crate::questions::Question {
    let store = crate::questions::QuestionStore::new();
    let new = crate::questions::NewQuestion {
        question: text.into(),
        choices: choices.iter().map(|(id, label)| crate::questions::Choice { id: (*id).into(), label: (*label).into() }).collect(),
        ..Default::default()
    };
    store.create(new, T0).expect("a valid question")
}
