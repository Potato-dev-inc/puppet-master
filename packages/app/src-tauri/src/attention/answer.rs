//! `POST /attention/{id}/answer`: a paired person answers one item of the attention list from the
//! phone card (or, later, the lock screen).
//!
//! One function, [`answer`], and an [`Env`] that carries everything outside it (the clock, the
//! snapshot, the live screen, the answer route), so the whole sequence is tested with a fake screen
//! and a counting writer. The sequence (design 4.2):
//!
//! 1. the caller holds the `answer` capability (403 otherwise);
//! 2. a retry with the same `answer_key` gets the first result and never presses twice (ledger);
//! 3. the item is looked up: a tombstone says who answered, a missing item is `gone`;
//! 4. an approval or trust gate is checked against what the card showed (prompt id, occurrence,
//!    fingerprint) and, UNDER the existing answer lock, against the live screen again. A mismatch is
//!    `changed` (a different prompt is up) or `gone` (none is), and nothing is typed;
//! 5. the key is typed through the current answer route, under an [`AnswerPermit`] (a person's
//!    decision about this prompt, refused when the pane printed since it was re-read), never a
//!    `GatePermit`;
//! 6. the result is recorded: the ledger, the item's tombstone ("Allowed once from Anna's iPhone"),
//!    the audit log.
//!
//! A chat answer (`Via::Telegram`) is refused for a trust gate, a consent card, a budget, a limit, a
//! person-only question card (an issue card) and any approval above low risk, right after the item is
//! looked up and before any key is typed (see `chat_refusal` and `person_only_question_refusal`).
//!
//! A question, a consent card, a budget or a usage-limit card goes to the questions store (no pane
//! write). This route is for paired people only: an agent answers through `answer_prompt`, where the
//! agent-authority consent rules apply.

use super::caps::DeviceCaps;
use super::model::{Item, Kind, Level, Resolution, Snapshot, State};
use super::store::PromptRec;
use crate::agent_runs::readiness::{AgentProcess, AnswerPermit, Decider};
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Answers remembered for retries.
pub const LEDGER_CAP: usize = 512;
pub const LEDGER_TTL_MS: u64 = 10 * 60_000;
const MAX_KEY_CHARS: usize = 80;
/// Reads of the live screen an answer makes before it gives up as `changed`.
const MAX_ATTEMPTS: usize = 2;

pub type Reply = (u16, Value);

/// What the phone sends.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Request {
    /// The choice id (`allow_once`, `deny`, a question's choice id).
    #[serde(default)]
    pub choice: Option<String>,
    /// Free text for a question that takes it.
    #[serde(default)]
    pub text: Option<String>,
    /// What the card showed. An approval needs all three.
    #[serde(default)]
    pub prompt_id: Option<String>,
    #[serde(default)]
    pub occurrence: Option<u32>,
    #[serde(default)]
    pub fingerprint: Option<String>,
    /// Made once per tap and re-sent by "Try again": the retry never presses twice.
    #[serde(default)]
    pub answer_key: Option<String>,
}

/// Where the answer came from. A chat answer (`Telegram`) gets the narrowest rules: see [`chat_refusal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    /// The attention card on a paired phone.
    Card,
    /// A notification button on the lock screen.
    LockScreen,
    /// A tap or a reply in the Telegram chat (`channels/`).
    Telegram,
}

/// The paired device that is answering.
#[derive(Debug, Clone)]
pub struct Caller {
    pub device_id: String,
    pub device_name: String,
    pub caps: DeviceCaps,
    pub via: Via,
}

/// The prompt a pane shows right now, read again for an answer.
pub struct Live {
    pub rec: PromptRec,
    /// How much of the pane's output the read screen includes (`pty::stream_pos`).
    pub observed_pos: Option<u64>,
    pub agent: AgentProcess,
}

/// What the answer route did.
#[derive(Debug, PartialEq, Eq)]
pub enum Delivered {
    Typed,
    /// The prompt on screen is not the one that was approved, or the pane printed since it was read.
    Stale,
    /// The key could not be written. Nothing was typed.
    Failed { code: String, message: String },
    /// Nothing was typed: this prompt occurrence was already answered a moment ago (the person's click
    /// on the PC, `pane_prompt::answers::answer_once`). One key per occurrence, whoever answers.
    Repeat,
}

/// What the questions store did.
#[derive(Debug, PartialEq, Eq)]
pub enum QuestionResult {
    /// Accepted; the label of the choice (or the text) for the receipt.
    Answered { label: Option<String> },
    /// Someone answered first (`desktop` or `phone`).
    AlreadyAnswered { by: String, label: Option<String> },
    NotFound,
    Refused { code: String, message: String },
}

pub trait Env {
    fn now_ms(&self) -> u64;
    /// The attention list as of right now.
    fn snapshot(&self) -> Snapshot;
    /// How many times the pane has asked, as the store counts it.
    fn occurrence(&self, pane_id: &str) -> u32;
    /// The prompt the pane shows right now; `None` when it shows none.
    fn live_prompt(&self, pane_id: &str) -> Option<Live>;
    /// Held while the live screen is re-read and the key is typed, so two answers cannot both pass.
    fn lock_answers(&self) -> Box<dyn std::any::Any>;
    fn deliver(&self, pane_id: &str, prompt_id: &str, choice: &str, permit: &AnswerPermit) -> Delivered;
    fn answer_question(&self, question_id: &str, choice: Option<&str>, text: Option<&str>) -> QuestionResult;
    /// The kind a question card was raised with (`budget`, `issue_start`...); `None` for an agent's own question.
    fn question_kind(&self, question_id: &str) -> Option<String>;
    /// The tombstone of this item will say how it was answered.
    fn note_resolution(&self, key: &str, resolution: Resolution);
    fn audit(&self, caller: &Caller, summary: &str, detail: Value);
    fn ledger(&self) -> &Mutex<Ledger>;
}

/// `(device id, answer key)` -> what was answered, so a retry returns it.
#[derive(Default)]
pub struct Ledger {
    entries: HashMap<(String, String), (String, u64)>,
}

impl Ledger {
    fn get(&self, device: &str, key: &str, now: u64) -> Option<String> {
        self.entries
            .get(&(device.to_string(), key.to_string()))
            .filter(|(_, at)| now.saturating_sub(*at) < LEDGER_TTL_MS)
            .map(|(label, _)| label.clone())
    }

    fn put(&mut self, device: &str, key: &str, label: String, now: u64) {
        self.entries.retain(|_, (_, at)| now.saturating_sub(*at) < LEDGER_TTL_MS);
        while self.entries.len() >= LEDGER_CAP {
            let Some(oldest) = self.entries.iter().min_by_key(|(_, (_, at))| *at).map(|(k, _)| k.clone()) else { break };
            self.entries.remove(&oldest);
        }
        self.entries.insert((device.to_string(), key.to_string()), (label, now));
    }
}

fn forbidden(cap: &str) -> Reply {
    (403, json!({"error": "not_allowed", "code": "DEVICE_CAP_REQUIRED", "cap": cap}))
}

fn bad(code: &str, hint: &str) -> Reply {
    (400, json!({"error": code, "hint": hint}))
}

fn ok(body: Value) -> Reply {
    (200, body)
}

fn gone(reason: &str) -> Reply {
    ok(json!({"outcome": "gone", "reason": reason}))
}

fn changed() -> Reply {
    ok(json!({"outcome": "changed"}))
}

/// "Allowed once", "Denied": the receipt wording; any other choice keeps its own label.
pub fn answered_label(choice_id: &str, label: &str) -> String {
    match choice_id {
        "allow_once" => "Allowed once".to_string(),
        "deny" => "Denied".to_string(),
        _ => label.trim().to_string(),
    }
}

/// What a retry (or a second tap) is told when this exact answer already went through.
fn already_by_you(label: &str) -> Reply {
    ok(json!({"outcome": "already_answered", "by": "you", "choice_label": label}))
}

/// What the caller is told about an item that is no longer open.
fn resolved(item: &Item, caller: &Caller) -> Reply {
    let Some(resolution) = &item.resolution else { return gone("moved_on") };
    if resolution.state != "answered" {
        return gone(resolution.reason.as_deref().unwrap_or(&resolution.state));
    }
    let by = match resolution.by.as_deref() {
        Some("desktop") => "pc",
        _ if resolution.device_id.as_deref() == Some(caller.device_id.as_str()) => "you",
        _ => "phone",
    };
    ok(json!({
        "outcome": "already_answered",
        "by": by,
        "device_name": resolution.device_name,
        "choice_label": resolution.choice_label,
    }))
}

const PERSON_ONLY_HINT: &str = "Decide this in Puppet Master on the PC or on a paired phone.";

/// The rule for a chat answer (`Via::Telegram`), enforced here as well as in the Telegram layer
/// (defence in depth: a bug in that layer must not be enough). A trust gate, a consent card, a budget
/// or a limit, and an approval above low risk or with no risk read, are never answered from a chat.
/// `None` means the chat may answer this item. Every kind is listed, so a new kind must be decided here.
fn chat_refusal(item: &Item) -> Option<&'static str> {
    match item.kind {
        Kind::TrustGate => Some("Only the person at this PC can pass this."),
        Kind::Consent => Some("An agent wants more control. Decide on the PC."),
        Kind::Budget | Kind::Limit | Kind::Tamper | Kind::Setup => Some("Decide this in Puppet Master on the PC."),
        Kind::Approval => match item.risk.as_ref().map(|risk| risk.level) {
            Some(Level::Low) => None,
            _ => Some("Only low-risk approvals can be answered from a chat."),
        },
        Kind::Question | Kind::Error | Kind::Stuck | Kind::Review | Kind::Done | Kind::Answer => None,
    }
}

/// A question card that only the person answers (`questions::PERSON_ONLY_KINDS`, an issue card). The list shows it
/// as an ordinary question, so the kind is read from the card itself. A paired phone and the PC may answer it.
fn person_only_question_refusal(env: &dyn Env, item: &Item) -> Option<&'static str> {
    if item.kind != Kind::Question {
        return None;
    }
    let (_, question_id) = item.key.split_once(':')?;
    let kind = env.question_kind(question_id)?;
    crate::questions::PERSON_ONLY_KINDS.contains(&kind.as_str()).then_some(PERSON_ONLY_HINT)
}

/// `item_id` is the opaque id of the card; `request` is what the phone sent.
pub fn answer(env: &dyn Env, caller: &Caller, item_id: &str, request: &Request) -> Reply {
    if !caller.caps.can_answer {
        return forbidden("answer");
    }
    let key = match request.answer_key.as_deref() {
        None => None,
        Some(k) if !k.is_empty() && k.chars().count() <= MAX_KEY_CHARS && k.chars().all(|c| c.is_ascii_graphic()) => Some(k),
        Some(_) => return bad("invalid_answer_key", "answer_key is 1-80 printable characters"),
    };
    let remembered = |env: &dyn Env| {
        key.and_then(|k| env.ledger().lock().get(&caller.device_id, k, env.now_ms()))
    };
    if let Some(label) = remembered(env) {
        return already_by_you(&label);
    }

    let snapshot = env.snapshot();
    let Some(item) = snapshot.items.iter().find(|i| i.id == item_id) else { return gone("unknown") };
    if item.state == State::Resolved {
        return resolved(item, caller);
    }
    if caller.via == Via::Telegram {
        if let Some(hint) = chat_refusal(item).or_else(|| person_only_question_refusal(env, item)) {
            return (403, json!({"error": "not_from_chat", "hint": hint}));
        }
    }
    match item.kind {
        Kind::Approval | Kind::TrustGate => answer_prompt(env, caller, item, request, key),
        Kind::Question | Kind::Consent | Kind::Budget | Kind::Limit => answer_card(env, caller, item, request, key),
        _ => bad("not_answerable", "this item has no answer to send; open the pane"),
    }
}

fn answer_prompt(env: &dyn Env, caller: &Caller, item: &Item, request: &Request, key: Option<&str>) -> Reply {
    let (Some(prompt), Some(pane)) = (&item.prompt, &item.pane) else {
        return bad("not_answerable", "Puppet Master cannot read this question; open the pane");
    };
    let Some(choice) = request.choice.as_deref().filter(|c| !c.is_empty()) else {
        return bad("missing_choice", "choice is required");
    };
    let (Some(prompt_id), Some(occurrence), Some(fingerprint)) =
        (request.prompt_id.as_deref(), request.occurrence, request.fingerprint.as_deref())
    else {
        return bad("stale_check_required", "prompt_id, occurrence and fingerprint are required: send what the card showed");
    };
    // What the card showed must be what the list says is up.
    if prompt.prompt_id != prompt_id || prompt.occurrence != occurrence || prompt.fingerprint != fingerprint {
        tracing::info!(
            same_prompt = prompt.prompt_id == prompt_id,
            same_occurrence = prompt.occurrence == occurrence,
            same_fingerprint = prompt.fingerprint == fingerprint,
            "an answer from a phone was refused: the card is older than the PC's list"
        );
        return changed();
    }
    let Some(picked) = prompt.choices.iter().find(|c| c.id == choice) else {
        return bad("invalid_choice", "that choice is not offered by this prompt");
    };
    if picked.broad {
        // Phones get no "don't ask again" answers, same as the desktop card (`answer_for_pane`).
        return (403, json!({"error": "broad_not_from_phone", "hint": "answer with a single-use choice, or answer on your PC"}));
    }
    let label = answered_label(&picked.id, &picked.label);

    let _serial = env.lock_answers();
    // A parallel request with the same key may have finished while this one waited for the lock.
    if let Some(label) = key.and_then(|k| env.ledger().lock().get(&caller.device_id, k, env.now_ms())) {
        return already_by_you(&label);
    }
    // The live screen, again, under the lock: what the card showed is what is answered. The key goes
    // out under a permit for exactly that read; when the pane printed since (a status line ticking
    // while the route took its checkpoint), the permit refuses and the screen is read ONCE more: the
    // same prompt still up gets one more try, anything else is `changed` or `gone`. Nothing is typed
    // into a screen nobody looked at.
    let decider = match caller.via {
        Via::Card => Decider::Device(caller.device_id.clone()),
        Via::LockScreen => Decider::LockScreen(caller.device_id.clone()),
        Via::Telegram => Decider::Channel(caller.device_id.clone()),
    };
    for attempt in 0..MAX_ATTEMPTS {
        let Some(live) = env.live_prompt(&pane.id) else { return gone("moved_on") };
        let current = env.occurrence(&pane.id);
        let (same_prompt, same_occurrence, same_fingerprint) = (
            live.rec.prompt_id.as_deref() == Some(prompt_id),
            current == occurrence,
            live.rec.fingerprint(current) == fingerprint,
        );
        if !(same_prompt && same_occurrence && same_fingerprint) {
            // Which part differs, never what the prompt says: the pane was resized under the card (the
            // dialog redrew and reads as another prompt), or the same command was asked again.
            tracing::info!(same_prompt, same_occurrence, same_fingerprint, "an answer from a phone was refused: the prompt on screen is not the one on the card");
            return changed();
        }
        let permit = match AnswerPermit::for_prompt(&pane.id, prompt_id, fingerprint, decider.clone(), live.observed_pos, live.agent) {
            Ok(permit) => permit,
            Err(_) => return gone("agent_exited"),
        };
        match env.deliver(&pane.id, prompt_id, choice, &permit) {
            Delivered::Typed => {
                let now = env.now_ms();
                recorded(env, caller, item, &label, key, now);
                return ok(json!({"outcome": "answered", "choice_label": label, "at_ms": now}));
            }
            Delivered::Stale if attempt + 1 < MAX_ATTEMPTS => continue,
            Delivered::Stale => return if env.live_prompt(&pane.id).is_some() { changed() } else { gone("moved_on") },
            Delivered::Failed { code, message } => return ok(json!({"outcome": "failed", "code": code, "detail": message})),
            // The PC answered this very prompt first (the dialog had not redrawn yet): no key, no
            // second audit line, and the phone is told who did, from the tombstone when it is there.
            Delivered::Repeat => {
                let now = env.snapshot();
                if let Some(done) = now.items.iter().find(|i| i.id == item.id && i.state == State::Resolved) {
                    return resolved(done, caller);
                }
                return ok(json!({"outcome": "already_answered", "by": "pc"}));
            }
        }
    }
    changed()
}

fn answer_card(env: &dyn Env, caller: &Caller, item: &Item, request: &Request, key: Option<&str>) -> Reply {
    // A budget or usage-limit card spends or stops a run: it needs `control`, not only `answer`.
    if matches!(item.kind, Kind::Budget | Kind::Limit) && !caller.caps.can_control {
        return forbidden("control");
    }
    let choice = request.choice.as_deref().filter(|c| !c.is_empty());
    let text = request.text.as_deref().map(str::trim).filter(|t| !t.is_empty());
    if choice.is_some() == text.is_some() {
        return bad("choice_or_text", "send either choice or text");
    }
    let Some((_, subject)) = item.key.split_once(':') else { return gone("unknown") };
    let _serial = env.lock_answers();
    if let Some(label) = key.and_then(|k| env.ledger().lock().get(&caller.device_id, k, env.now_ms())) {
        return already_by_you(&label);
    }
    match env.answer_question(subject, choice, text) {
        QuestionResult::Answered { label } => {
            let now = env.now_ms();
            let label = label.unwrap_or_else(|| "Answered".to_string());
            recorded(env, caller, item, &label, key, now);
            ok(json!({"outcome": "answered", "choice_label": label, "at_ms": now}))
        }
        QuestionResult::AlreadyAnswered { by, label } => ok(json!({
            "outcome": "already_answered",
            "by": if by == "desktop" { "pc" } else { "phone" },
            "choice_label": label,
        })),
        QuestionResult::NotFound => gone("moved_on"),
        QuestionResult::Refused { code, message } => bad(&code.to_ascii_lowercase(), &message),
    }
}

/// The answer went through: remember it for retries, tell the tombstone, write the audit line.
fn recorded(env: &dyn Env, caller: &Caller, item: &Item, label: &str, key: Option<&str>, now: u64) {
    if let Some(key) = key {
        env.ledger().lock().put(&caller.device_id, key, label.to_string(), now);
    }
    env.note_resolution(
        &item.key,
        Resolution {
            state: "answered".into(),
            by: Some("phone".into()),
            choice_label: Some(label.to_string()),
            at_ms: now,
            device_name: Some(caller.device_name.clone()),
            device_id: Some(caller.device_id.clone()),
            ..Resolution::default()
        },
    );
    // Who pressed which choice, never the command.
    let (summary, via) = match caller.via {
        Via::LockScreen => (format!("A phone answered from its lock screen: {label}"), "push"),
        Via::Card => (format!("A phone answered a card: {label}"), "card"),
        Via::Telegram => (format!("Answered from Telegram: {label}"), "telegram"),
    };
    env.audit(caller, &summary, json!({"kind": item.kind.as_str(), "choice": label, "via": via}));
}

// ---------------------------------------------------------------------------------------------
// The real environment

pub struct RealEnv {
    pub registry: std::sync::Arc<Mutex<crate::pty::PaneRegistry>>,
    pub app: tauri::AppHandle,
}

fn ledger_global() -> &'static Mutex<Ledger> {
    static LEDGER: once_cell::sync::Lazy<Mutex<Ledger>> = once_cell::sync::Lazy::new(|| Mutex::new(Ledger::default()));
    &LEDGER
}

impl Env for RealEnv {
    fn now_ms(&self) -> u64 {
        super::now_ms()
    }

    fn snapshot(&self) -> Snapshot {
        super::snapshot_now()
    }

    fn occurrence(&self, pane_id: &str) -> u32 {
        super::occurrence_of(pane_id)
    }

    fn live_prompt(&self, pane_id: &str) -> Option<Live> {
        let (detected, observed_pos) = crate::pane_prompt::detect_at(&self.registry, pane_id, false)?;
        let prompt = crate::pane_prompt::PanePrompt::from_detected(&detected)?;
        let pid = self.registry.lock().panes.get(pane_id).map(|p| p.info.pid)?;
        Some(Live {
            rec: super::live_prompt_rec(&prompt),
            observed_pos,
            agent: crate::pty::agent_proc::state(pane_id, pid),
        })
    }

    fn lock_answers(&self) -> Box<dyn std::any::Any> {
        Box::new(crate::agent_runs::serialize_answers())
    }

    fn deliver(&self, pane_id: &str, prompt_id: &str, choice: &str, permit: &AnswerPermit) -> Delivered {
        match crate::agent_runs::answer_decided(&self.registry, &self.app, pane_id, prompt_id, choice, permit) {
            Ok(reply) if crate::pane_prompt::reply_was_repeat(&reply) => Delivered::Repeat,
            Ok(_) => Delivered::Typed,
            Err(error) => match error.code.as_str() {
                "STALE_PROMPT" | "PROMPT_NOT_PENDING" => Delivered::Stale,
                _ if error.message.contains(crate::agent_runs::readiness::PERMIT_STALE) => Delivered::Stale,
                _ => Delivered::Failed { code: error.code.to_string(), message: error.message },
            },
        }
    }

    fn answer_question(&self, question_id: &str, choice: Option<&str>, text: Option<&str>) -> QuestionResult {
        let now = crate::questions::now_ms();
        match crate::questions::store().answer(question_id, choice, text, "phone", now) {
            Ok(resolution) => {
                let label = resolution.question.answer.as_ref().and_then(|a| {
                    a.choice_id
                        .as_deref()
                        .and_then(|id| resolution.question.choices.iter().find(|c| c.id == id).map(|c| c.label.clone()))
                        .or_else(|| a.text.clone())
                });
                if resolution.accepted {
                    crate::attention::on_changed();
                    QuestionResult::Answered { label }
                } else {
                    let by = resolution.question.answer.as_ref().map(|a| a.answered_by.clone()).unwrap_or_else(|| "desktop".into());
                    QuestionResult::AlreadyAnswered { by, label }
                }
            }
            Err(error) if error.code == "QUESTION_NOT_FOUND" => QuestionResult::NotFound,
            Err(error) => QuestionResult::Refused { code: error.code.to_string(), message: error.message },
        }
    }

    fn question_kind(&self, question_id: &str) -> Option<String> {
        crate::questions::store().get(question_id, crate::questions::now_ms()).and_then(|q| q.kind)
    }

    fn note_resolution(&self, key: &str, resolution: Resolution) {
        super::note_resolution(key, resolution);
    }

    fn audit(&self, caller: &Caller, summary: &str, detail: Value) {
        // A chat answer is audited as the channel (not a paired phone); the rest as the phone.
        let (kind, who) = match caller.via {
            Via::Telegram => (crate::audit::Kind::ChannelAnswer, crate::audit::Who::Channel("telegram".into())),
            Via::LockScreen | Via::Card => (crate::audit::Kind::Approval, crate::audit::Who::Device(Some(caller.device_id.clone()))),
        };
        crate::audit::record(kind, who, summary, detail);
    }

    fn ledger(&self) -> &Mutex<Ledger> {
        ledger_global()
    }
}

#[cfg(test)]
pub(crate) mod testing {
    //! A fake screen and a counting writer, for the tests of this module and of the bridge route.
    use super::*;
    use crate::attention::model::{ChoiceFact, Counts, Lane, Layout, PromptView, RiskView, Level};
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub struct FakeEnv {
        pub now: Mutex<u64>,
        pub items: Mutex<Vec<Item>>,
        /// What the pane shows now.
        pub live: Mutex<Option<Live>>,
        pub occurrence: Mutex<u32>,
        pub typed: Mutex<Vec<(String, String)>>,
        pub deliver_result: Mutex<Option<Delivered>>,
        /// The permit is refused on every write (the pane never stops printing).
        pub always_stale: Mutex<bool>,
        pub question_result: Mutex<Option<QuestionResult>>,
        /// Question id -> the kind its card was raised with (what the store would say).
        pub question_kinds: Mutex<HashMap<String, String>>,
        pub notes: Mutex<Vec<(String, Resolution)>>,
        pub audits: Mutex<Vec<String>>,
        pub ledger: Mutex<Ledger>,
        pub locks_taken: AtomicUsize,
    }

    pub fn choices() -> Vec<ChoiceFact> {
        vec![
            ChoiceFact { id: "allow_once".into(), label: "Allow once".into(), broad: false },
            ChoiceFact { id: "allow_all".into(), label: "Allow all commands".into(), broad: true },
            ChoiceFact { id: "deny".into(), label: "Deny".into(), broad: false },
        ]
    }

    pub fn rec(prompt_id: &str, summary: &str, level: &str) -> PromptRec {
        PromptRec {
            prompt_id: Some(prompt_id.into()),
            kind: Some("command".into()),
            summary: Some(summary.into()),
            risk_level: Some(level.into()),
            headline: Some("Run tests".into()),
            choices: choices(),
            reasons: vec![],
            occurrence: 0,
        }
    }

    pub fn approval_item(pane: &str, rec: &PromptRec, occurrence: u32) -> Item {
        let level = rec.risk_level.as_deref().and_then(Level::parse);
        Item {
            id: format!("item-{pane}-{occurrence}"),
            key: format!("approval:{pane}"),
            kind: Kind::Approval,
            lane: Lane::Blocking,
            rank: 2,
            state: State::Open,
            since_ms: Some(1),
            expires_at_ms: None,
            pane: Some(crate::attention::model::PaneRef { id: pane.into(), heading: "auth refactor".into(), agent: Some("Claude Code".into()), project: Some("shop".into()) }),
            title: "Run tests".into(),
            sentence: rec.summary.clone(),
            risk: level.map(|level| RiskView { level, word: level.word(), headline: rec.headline.clone(), reasons: vec![] }),
            prompt: Some(PromptView {
                prompt_id: rec.prompt_id.clone().unwrap(),
                occurrence,
                kind: "command".into(),
                summary: rec.summary.clone(),
                choices: rec.choices.clone(),
                fingerprint: rec.fingerprint(occurrence),
                layout: Layout { primary: Some("allow_once".into()), focus: None, more: vec![] },
                provenance: "screen",
            }),
            actions: vec![],
            resolution: None,
        }
    }

    impl FakeEnv {
        pub fn showing(pane: &str, rec: PromptRec, occurrence: u32) -> Self {
            let item = approval_item(pane, &rec, occurrence);
            Self {
                now: Mutex::new(1_000_000),
                items: Mutex::new(vec![item]),
                live: Mutex::new(Some(Live { rec, observed_pos: Some(10), agent: AgentProcess::Alive })),
                occurrence: Mutex::new(occurrence),
                typed: Mutex::new(Vec::new()),
                deliver_result: Mutex::new(None),
                always_stale: Mutex::new(false),
                question_result: Mutex::new(None),
                question_kinds: Mutex::new(HashMap::new()),
                notes: Mutex::new(Vec::new()),
                audits: Mutex::new(Vec::new()),
                ledger: Mutex::new(Ledger::default()),
                locks_taken: AtomicUsize::new(0),
            }
        }

        pub fn item_id(&self) -> String {
            self.items.lock()[0].id.clone()
        }

        pub fn request(&self, choice: &str, key: Option<&str>) -> Request {
            let item = self.items.lock()[0].clone();
            let prompt = item.prompt.unwrap();
            Request {
                choice: Some(choice.into()),
                text: None,
                prompt_id: Some(prompt.prompt_id),
                occurrence: Some(prompt.occurrence),
                fingerprint: Some(prompt.fingerprint),
                answer_key: key.map(str::to_string),
            }
        }

        /// A request with these exact fields (what a card that has since left the list would send).
        pub fn request_with(&self, choice: &str, key: Option<&str>, prompt_id: &str, occurrence: u32, fingerprint: &str) -> Request {
            Request {
                choice: Some(choice.into()),
                text: None,
                prompt_id: Some(prompt_id.into()),
                occurrence: Some(occurrence),
                fingerprint: Some(fingerprint.into()),
                answer_key: key.map(str::to_string),
            }
        }

        pub fn writes(&self) -> usize {
            self.typed.lock().len()
        }
    }

    impl Env for FakeEnv {
        fn now_ms(&self) -> u64 {
            *self.now.lock()
        }
        fn snapshot(&self) -> Snapshot {
            Snapshot { v: 1, rev: 1, counts: Counts::default(), items: self.items.lock().clone() }
        }
        fn occurrence(&self, _pane_id: &str) -> u32 {
            *self.occurrence.lock()
        }
        fn live_prompt(&self, _pane_id: &str) -> Option<Live> {
            self.live.lock().as_ref().map(|l| Live { rec: l.rec.clone(), observed_pos: l.observed_pos, agent: l.agent })
        }
        fn lock_answers(&self) -> Box<dyn std::any::Any> {
            self.locks_taken.fetch_add(1, Ordering::SeqCst);
            Box::new(())
        }
        fn deliver(&self, pane_id: &str, _prompt_id: &str, choice: &str, _permit: &AnswerPermit) -> Delivered {
            if *self.always_stale.lock() {
                return Delivered::Stale;
            }
            if let Some(result) = self.deliver_result.lock().take() {
                if result != Delivered::Typed {
                    return result;
                }
            }
            self.typed.lock().push((pane_id.to_string(), choice.to_string()));
            Delivered::Typed
        }
        fn answer_question(&self, _id: &str, choice: Option<&str>, _text: Option<&str>) -> QuestionResult {
            self.question_result.lock().take().unwrap_or(QuestionResult::Answered { label: choice.map(str::to_string) })
        }
        fn question_kind(&self, question_id: &str) -> Option<String> {
            self.question_kinds.lock().get(question_id).cloned()
        }
        fn note_resolution(&self, key: &str, resolution: Resolution) {
            self.notes.lock().push((key.to_string(), resolution.clone()));
            // The item leaves the list as a tombstone that says who answered.
            for item in self.items.lock().iter_mut().filter(|i| i.key == key) {
                item.state = State::Resolved;
                item.resolution = Some(resolution.clone());
                item.actions.clear();
            }
        }
        fn audit(&self, _caller: &Caller, summary: &str, _detail: Value) {
            self.audits.lock().push(summary.to_string());
        }
        fn ledger(&self) -> &Mutex<Ledger> {
            &self.ledger
        }
    }

    pub fn caller(id: &str, caps: DeviceCaps) -> Caller {
        Caller { device_id: id.into(), device_name: format!("{id}'s phone"), caps, via: Via::Card }
    }
}

#[cfg(test)]
mod tests;
