//! Text Your Agents: test support for the runtime. A fake bot API (no test talks to Telegram), the
//! fake host, a memory credential store and a recording audit, wired into a real `Runtime`.

use super::runtime::{Deps, Runtime};
use super::telegram::api::{Api, ApiError, BotInfo, InMessage, Update, UpdateKind, WireMessage};
use super::telegram::poller::Timing;
use super::test_host::{FakeHost, PAIRED_ID, T0};
use crate::audit::{Kind as AuditKind, Who};
use crate::redact::Redacted;
use crate::secrets::MemoryBackend;
use crate::turn_summary::model::{EndState, TurnSummary};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A well-formed fixture token (never a real one).
pub const TOKEN: &str = "123456789:TESTtokenTESTtokenTESTtokenTESTtok";
/// The secret half of [`TOKEN`]: it must never show up in a status, a message or an error.
pub const TOKEN_SECRET: &str = "TESTtokenTESTtokenTESTtokenTESTtok";

pub struct FakeBot {
    pub me: Mutex<Result<BotInfo, ApiError>>,
    pub webhook: Mutex<Option<String>>,
    /// The next `get_updates` fails with this (once).
    pub poll_error: Mutex<Option<ApiError>>,
    pub inbox: Mutex<VecDeque<Vec<Update>>>,
    pub polls: AtomicUsize,
    pub me_calls: AtomicUsize,
    /// (chat id, text) of every message sent.
    pub sent: Mutex<Vec<(i64, String)>>,
    /// One entry per message sent, in the same order as `sent`: `true` when it was silent
    /// (`disable_notification`).
    pub silent: Mutex<Vec<bool>>,
    /// One entry per message sent, in the same order as `sent`: the message it was a reply to.
    pub reply_tos: Mutex<Vec<Option<i64>>>,
    /// How many times "typing…" was shown, and in which chats.
    pub typing: Mutex<Vec<i64>>,
    pub menus: AtomicUsize,
    /// Every token `api_for` was asked to build a client for.
    pub tokens: Mutex<Vec<String>>,
}

impl FakeBot {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            me: Mutex::new(Ok(BotInfo { id: 777, username: "anna_pm_bot".into() })),
            webhook: Mutex::new(None),
            poll_error: Mutex::new(None),
            inbox: Mutex::new(VecDeque::new()),
            polls: AtomicUsize::new(0),
            me_calls: AtomicUsize::new(0),
            sent: Mutex::new(Vec::new()),
            silent: Mutex::new(Vec::new()),
            reply_tos: Mutex::new(Vec::new()),
            typing: Mutex::new(Vec::new()),
            menus: AtomicUsize::new(0),
            tokens: Mutex::new(Vec::new()),
        })
    }

    pub fn push_updates(&self, updates: Vec<Update>) {
        self.inbox.lock().push_back(updates);
    }

    pub fn texts(&self) -> Vec<String> {
        self.sent.lock().iter().map(|(_, t)| t.clone()).collect()
    }
}

impl Api for FakeBot {
    fn get_me(&self) -> Result<BotInfo, ApiError> {
        self.me_calls.fetch_add(1, Ordering::SeqCst);
        self.me.lock().clone()
    }

    fn webhook_url(&self) -> Result<Option<String>, ApiError> {
        Ok(self.webhook.lock().clone())
    }

    fn get_updates(&self, _offset: Option<i64>, _timeout_s: u32) -> Result<Vec<Update>, ApiError> {
        self.polls.fetch_add(1, Ordering::SeqCst);
        // A long poll that returns after a moment, so a running poller does not spin.
        std::thread::sleep(Duration::from_millis(5));
        if let Some(err) = self.poll_error.lock().take() {
            return Err(err);
        }
        Ok(self.inbox.lock().pop_front().unwrap_or_default())
    }

    fn typing(&self, chat_id: i64) -> Result<(), ApiError> {
        self.typing.lock().push(chat_id);
        Ok(())
    }

    fn send(&self, chat_id: i64, out: &WireMessage, reply_to: Option<i64>) -> Result<i64, ApiError> {
        let mut sent = self.sent.lock();
        sent.push((chat_id, out.text.as_str().to_string()));
        self.silent.lock().push(out.disable_notification);
        self.reply_tos.lock().push(reply_to);
        Ok(sent.len() as i64)
    }

    fn edit(&self, _chat_id: i64, _message_id: i64, _out: &WireMessage) -> Result<(), ApiError> {
        Ok(())
    }

    fn answer_callback(&self, _id: &str, _text: Option<&Redacted<String>>) -> Result<(), ApiError> {
        Ok(())
    }

    fn set_commands(&self, _list: &[(&str, &str)]) -> Result<(), ApiError> {
        self.menus.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// One audit line written through `Deps::audit`.
pub type AuditLine = (AuditKind, Who, String);

pub struct Rig {
    pub rt: Arc<Runtime>,
    pub host: Arc<FakeHost>,
    pub bot: Arc<FakeBot>,
    pub secrets: Arc<MemoryBackend>,
    pub audits: Arc<Mutex<Vec<AuditLine>>>,
    pub turns: Arc<Mutex<Vec<TurnSummary>>>,
}

/// Nothing waits for real: short polls, no gap between sends.
pub const FAST: Timing = Timing {
    poll_timeout_s: 0,
    backoff_min: Duration::from_millis(1),
    backoff_max: Duration::from_millis(4),
    send_gap: Duration::ZERO,
    tick: Duration::from_millis(1),
};

/// A runtime over `host` and a fresh fake bot. The credential store is empty.
pub fn rig(host: FakeHost) -> Rig {
    let host = Arc::new(host);
    let bot = FakeBot::new();
    let secrets = Arc::new(MemoryBackend::new());
    let audits: Arc<Mutex<Vec<AuditLine>>> = Arc::new(Mutex::new(Vec::new()));
    let turns: Arc<Mutex<Vec<TurnSummary>>> = Arc::new(Mutex::new(Vec::new()));
    let (api_bot, audit_log, turn_log) = (bot.clone(), audits.clone(), turns.clone());
    let deps = Deps {
        host: host.clone(),
        secrets: secrets.clone(),
        profile: None,
        api_for: Arc::new(move |token: &str| {
            api_bot.tokens.lock().push(token.to_string());
            api_bot.clone() as Arc<dyn Api>
        }),
        timing: FAST,
        audit: Arc::new(move |kind, who, summary, _detail| audit_log.lock().push((kind, who, summary.to_string()))),
        turns: Arc::new(move |since| turn_log.lock().iter().filter(|t| t.ended_at_ms >= since).cloned().collect()),
    };
    Rig { rt: Arc::new(Runtime::new(deps)), host, bot, secrets, audits, turns }
}

impl Rig {
    pub fn with_token(self) -> Self {
        use crate::secrets::SecretBackend;
        self.secrets.set(&super::token::key_name_for(None), TOKEN).unwrap();
        self
    }

    pub fn audit_kinds(&self) -> Vec<AuditKind> {
        self.audits.lock().iter().map(|(k, _, _)| *k).collect()
    }
}

/// Waits (up to 3 s) for `cond`; the poller and send threads are real.
pub fn wait_for(what: &str, cond: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until {
        if cond() {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("timed out waiting for: {what}");
}

/// A fresh text from the paired person.
pub fn text_from(user: i64, id: i64, text: &str) -> Update {
    Update {
        update_id: id,
        kind: UpdateKind::Message(InMessage {
            message_id: id,
            chat_id: user,
            chat_private: true,
            from_id: Some(user),
            from_is_bot: false,
            username: Some("anna_k".into()),
            first_name: Some("Anna".into()),
            date_s: (T0 / 1000) as i64 - 5,
            text: Some(text.into()),
            reply_to: None,
            ..InMessage::default()
        }),
    }
}

pub fn text_from_paired(id: i64, text: &str) -> Update {
    text_from(PAIRED_ID, id, text)
}

/// A finished turn in `pane_id`.
pub fn turn(pane_id: &str, ended_at_ms: u64, sentence: &str) -> TurnSummary {
    TurnSummary {
        pane_id: pane_id.into(),
        agent: "Codex".into(),
        agent_id: None,
        account: None,
        turn: 1,
        started_at_ms: ended_at_ms.saturating_sub(1000),
        ended_at_ms,
        duration_ms: 1000,
        files: None,
        approvals: Vec::new(),
        end: EndState { kind: "idle".into(), prompt: None },
        receipt: None,
        sentence: sentence.into(),
    }
}
