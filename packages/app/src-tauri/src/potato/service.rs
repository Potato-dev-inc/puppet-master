//! The Potato Bot engine: one object that owns every bot's brain process, its inbox and what the brain
//! reports (spec sections 1.5, 2, 3 and 6). The bridge routes (`bridge/routes/potato_brain.rs`) and the
//! Tauri commands (`commands/potato.rs`) are thin callers of it.
//!
//! What it does, in the order a message travels:
//!
//! 1. `send`: `rules::decide` (here it gives `owner_pc` and full tools), the message is saved (redacted),
//!    the brain is asked for (lazy start), and a `send` item goes into the bot's inbox.
//! 2. `inbox_poll`: the brain's long poll. Every spawn has a new token and a new inbox.
//! 3. `ingest`: what the brain posts. A reply is saved, redacted and shown; `usage` is priced with
//!    `cost.rs` and judged against the daily limit; `helper_request` is stored as a card (starting a helper
//!    is a later piece); an `error` becomes a plain sentence in the thread. Nothing the brain says reaches
//!    the person unredacted, and a brain can only write into its own bot's thread.
//! 4. `tick`: the supervisor, every half second. It sees a process leave, runs the keeper's actions
//!    (start, ask to leave, kill) and resumes a bot that stopped for the day's limit when the day changes.
//!
//! The things the service needs from the app (clock, secrets, resource folder, events) come through the
//! `Host` trait and the process through the `Launcher` trait, so the tests run it on fakes. Nothing here
//! names an agent or a chat app.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use serde::Serialize;
use serde_json::Value;

use super::auth::{self, Brains};
use super::cost::{self, Budget};
use super::env::{self, Brain, HostEnv};
use super::keeper::{Action, Keeper, Phase};
use super::log::LogRing;
use super::protocol::{self, ErrorCode, Event, HelperStatus, PauseReason, Trust};
use super::rules::{self, Caps, Channel, Decision, Facts, Sender};
use super::sidecar::{Child, Launcher, LineSink, NodeState, SpawnSpec};
use super::wire_names;
use super::store::{self, BrainKind, FromKind, Message, MsgKind, StoreError};
use crate::secrets::{self, SecretMask};

pub const SECRET_DEEPSEEK: &str = "deepseek_api_key";
/// A `shutdown` is given this long before the process tree is killed.
pub const SHUTDOWN_GRACE_MS: i64 = 2_000;
/// A bot that reported `typing` and then went quiet stops looking busy after this long.
pub const TYPING_TTL_MS: i64 = 120_000;
/// Items kept in an inbox that nobody has fetched.
const INBOX_CAP: usize = 200;
/// Open helper cards per bot.
pub const MAX_OPEN_CARDS: usize = 3;
/// Two minutes of sends, kept to count the last minute.
const SEND_WINDOW_MS: i64 = 60_000;

/// What the app lends the engine.
pub trait Host: Send + Sync {
    fn now_ms(&self) -> i64;
    /// The local date `YYYY-MM-DD` of an instant.
    fn local_day(&self, ms: i64) -> String;
    /// `http://127.0.0.1:PORT`, once the bridge is listening.
    fn bridge_url(&self) -> Option<String>;
    fn profile(&self, bot: &str) -> Option<BotProfile>;
    fn host_env(&self) -> HostEnv;
    fn secret(&self, name: &str) -> Option<String>;
    /// The `potato-brain` resource folder (`brain.mjs`, `persona/`).
    fn resources(&self) -> Option<PathBuf>;
    /// Something about this bot changed: tell the windows (`potato://changed`).
    fn changed(&self, bot: &str);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotProfile {
    pub name: String,
    /// The companion's home folder, taken out of any text that is kept.
    pub folder: Option<String>,
}

/// Why a bot cannot start right now. Nothing here is a crash: it waits for the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Blocked {
    NeedsKey,
    NeedsNode,
    /// The brain program is not installed (a broken install).
    MissingProgram,
    /// The bridge is not listening yet.
    NoBridge,
}

/// What the page shows for a bot's brain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BrainState {
    /// Potato Bot is switched off.
    Off,
    /// Not running: it starts on the next message.
    Idle,
    Starting,
    Ready,
    /// A crash: it starts again in a moment.
    Restarting,
    /// It kept crashing: only "Start again" starts it.
    Stopped,
    NeedsKey,
    NeedsNode,
    MissingProgram,
    DailyLimit,
    /// "Through my agents": no brain process of its own.
    Agents,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MoodNow {
    pub mood: String,
    pub intensity: u64,
    pub energy: u64,
    pub hunger: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BotView {
    pub id: String,
    pub name: String,
    pub talk_persona: String,
    pub brain_state: BrainState,
    /// The last error code the brain reported and that nothing has cleared since.
    pub last_error: Option<String>,
    pub typing: bool,
    pub mood: Option<MoodNow>,
    pub provider: Option<String>,
    pub unread: u32,
    pub last_line: Option<String>,
    pub last_at: Option<i64>,
    pub used_today_nano: u64,
    pub limit_cents: u32,
    pub open_cards: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeView {
    pub ready: bool,
    pub found: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PotatoState {
    pub enabled: bool,
    pub settings: store::Settings,
    pub key: SecretMask,
    pub node: NodeView,
    pub bots: Vec<BotView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendOutcome {
    /// The id of the saved message; the brain's reply names it in `inReplyTo`.
    pub id: String,
    pub state: BrainState,
    /// The message went into the brain's inbox. False when the bot cannot answer yet (no key, no Node,
    /// the daily limit): the message is saved with a note that says why.
    pub delivered: bool,
}

// ---- inbox --------------------------------------------------------------------------------------

#[derive(Default)]
struct Inbox {
    /// Index of `items[0]`.
    base: u64,
    items: VecDeque<Value>,
    /// The highest index the brain has been handed (the end of its last answer).
    handed: u64,
}

impl Inbox {
    fn push(&mut self, item: Value) {
        self.items.push_back(item);
        while self.items.len() > INBOX_CAP {
            self.items.pop_front();
            self.base += 1;
        }
    }

    fn end(&self) -> u64 {
        self.base + self.items.len() as u64
    }

    /// Items from position `at` on, and the position to ask with next.
    fn since(&mut self, at: u64) -> (Vec<Value>, u64) {
        let from = at.max(self.base);
        let skip = (from - self.base) as usize;
        let items: Vec<Value> = self.items.iter().skip(skip).cloned().collect();
        let next = self.end();
        self.handed = self.handed.max(next);
        (items, next)
    }
}

struct Session {
    child: Box<dyn Child>,
    inbox: Inbox,
    secrets: Vec<String>,
    shutdown_at: Option<i64>,
    /// `send` id -> conversation, for the sends the brain has not answered.
    in_flight: HashMap<String, String>,
    /// `send` id -> its place in the inbox.
    send_index: HashMap<String, u64>,
}

struct BotRt {
    keeper: Keeper,
    session: Option<Session>,
    log: Arc<Mutex<LogRing>>,
    blocked: Option<Blocked>,
    provider: Option<String>,
    typing: HashMap<String, i64>,
    last_error: Option<ErrorCode>,
    mood: Option<MoodNow>,
    sends_ms: VecDeque<i64>,
    /// Sends for the next session: they came while none was running, or while one was leaving.
    queued: Vec<Value>,
    turn_trust: HashMap<String, Trust>,
    /// The day on which the bot was paused for its limit.
    paused_for_limit: Option<String>,
    last_surface: String,
}

impl BotRt {
    fn new() -> BotRt {
        BotRt {
            keeper: Keeper::new(),
            session: None,
            log: Arc::new(Mutex::new(LogRing::new())),
            blocked: None,
            provider: None,
            typing: HashMap::new(),
            last_error: None,
            mood: None,
            sends_ms: VecDeque::new(),
            queued: Vec::new(),
            turn_trust: HashMap::new(),
            paused_for_limit: None,
            last_surface: "app".into(),
        }
    }
}

#[derive(Default)]
struct Inner {
    bots: HashMap<String, BotRt>,
}

pub struct Service {
    root: PathBuf,
    host: Arc<dyn Host>,
    launcher: Arc<dyn Launcher>,
    brains: Arc<Brains>,
    inner: Mutex<Inner>,
    cv: Condvar,
    /// The helpers' world (`helpers.rs`): what starts them, and the ones now running.
    pub(super) helpers: super::helpers::HelperState,
}

type Result<T> = std::result::Result<T, StoreError>;

fn err(code: &'static str, message: impl Into<String>) -> StoreError {
    StoreError::new(code, message)
}

impl Service {
    pub fn new(root: PathBuf, host: Arc<dyn Host>, launcher: Arc<dyn Launcher>, brains: Arc<Brains>) -> Arc<Service> {
        Arc::new(Service { root, host, launcher, brains, inner: Mutex::new(Inner::default()), cv: Condvar::new(), helpers: super::helpers::HelperState::default() })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn brains(&self) -> &Arc<Brains> {
        &self.brains
    }

    pub fn enabled(&self) -> bool {
        self.brains.enabled()
    }

    pub(super) fn name_of(&self, bot: &str) -> String {
        self.host.profile(bot).map(|p| p.name).unwrap_or_else(|| bot.to_string())
    }

    pub(super) fn home_of(&self, bot: &str) -> Option<String> {
        self.host.profile(bot).and_then(|p| p.folder)
    }

    pub(super) fn require_bot(&self, bot: &str) -> Result<store::Bot> {
        store::load_bots(&self.root)?
            .into_iter()
            .find(|b| b.id == bot)
            .ok_or_else(|| err("POTATO_BOT_NOT_FOUND", "No bot has that name."))
    }

    // ---- saved messages ------------------------------------------------------------------------

    pub(super) fn save(&self, bot: &str, conversation: &str, message: Message) -> Result<Message> {
        store::append_message(&self.root, conversation, message, self.home_of(bot).as_deref())
    }

    /// A note row in the bot's app thread (best effort: a damaged file is logged, not raised).
    pub(super) fn note(&self, bot: &str, text: &str) {
        let message = Message::new(self.host.now_ms(), FromKind::Note, None, MsgKind::Note, text);
        if let Err(e) = self.save(bot, &store::app_conversation(bot), message) {
            tracing::debug!(%e, "could not save a Potato Bot note");
        }
    }

    pub fn messages(&self, bot: &str, limit: usize) -> Result<Vec<Message>> {
        self.require_bot(bot)?;
        store::read_messages(&self.root, &store::app_conversation(bot), limit.clamp(1, 500), self.home_of(bot).as_deref())
    }

    // ---- the person's side ---------------------------------------------------------------------

    pub fn add_bot(&self, bot: &str) -> Result<store::Bot> {
        let settings = store::load_settings(&self.root)?;
        if self.host.profile(bot).is_none() {
            return Err(err("POTATO_BOT_NOT_FOUND", "Make that companion first, then turn it into a Potato Bot."));
        }
        let added = store::add_bot(&self.root, bot, &settings.default_persona, self.host.now_ms())?;
        self.host.changed(bot);
        Ok(added)
    }

    /// Changes the voice. The name, threads, memory and mood stay. The brain restarts, because its
    /// persona prefix is fixed when it starts.
    pub fn set_persona(&self, bot: &str, pack: &str) -> Result<store::Bot> {
        let before = self.require_bot(bot)?;
        let after = store::set_talk_persona(&self.root, bot, pack)?;
        if before.talk_persona != after.talk_persona {
            let line = if pack == "potato" { "Now talking as Potato." } else { "Now talking as Fish." };
            self.note(bot, line);
            self.stop_bot(bot);
        }
        self.host.changed(bot);
        Ok(after)
    }

    pub fn settings(&self) -> Result<store::Settings> {
        store::load_settings(&self.root)
    }

    pub fn set_settings(&self, new: store::Settings) -> Result<store::Settings> {
        let old = store::load_settings(&self.root)?;
        store::save_settings(&self.root, &new)?;
        let saved = store::load_settings(&self.root)?;
        if old.brain != saved.brain {
            // A different brain starts fresh.
            self.stop_all(Duration::from_millis(300));
        }
        self.recheck_limits();
        Ok(saved)
    }

    pub fn mark_read(&self, bot: &str) -> Result<()> {
        self.require_bot(bot)?;
        store::mark_read(&self.root, bot, self.host.now_ms())?;
        self.host.changed(bot);
        Ok(())
    }

    pub fn forget_talk_memory(&self, bot: &str) -> Result<()> {
        self.require_bot(bot)?;
        self.stop_all_of(bot, Duration::from_millis(500));
        let stamp = format!("{}", self.host.now_ms());
        store::forget_talk_memory(&self.root, bot, &stamp)?;
        self.note(bot, "Talk memory was cleared. The old copy is kept in the removed folder.");
        self.host.changed(bot);
        Ok(())
    }

    fn rate_ok(rt: &mut BotRt, now: i64) -> u32 {
        while rt.sends_ms.front().is_some_and(|at| now - *at >= SEND_WINDOW_MS) {
            rt.sends_ms.pop_front();
        }
        rt.sends_ms.len() as u32
    }

    /// What stops a bot from starting at all, found before anything is spawned.
    fn preflight(&self, settings: &store::Settings) -> Option<Blocked> {
        match settings.brain {
            BrainKind::Agents => None,
            BrainKind::Deepseek if !self.has_key() => Some(Blocked::NeedsKey),
            _ => {
                if !self.launcher.node().is_ready() {
                    return Some(Blocked::NeedsNode);
                }
                if super::sidecar::script_path(self.host.resources().as_deref()).is_none() {
                    return Some(Blocked::MissingProgram);
                }
                if self.host.bridge_url().is_none() {
                    return Some(Blocked::NoBridge);
                }
                None
            }
        }
    }

    fn has_key(&self) -> bool {
        self.host.secret(SECRET_DEEPSEEK).is_some_and(|k| !k.trim().is_empty())
    }

    fn blocked_note(&self, bot: &str, blocked: Blocked) -> String {
        let name = self.name_of(bot);
        match blocked {
            Blocked::NeedsKey => format!("Add your DeepSeek key in Settings to talk with {name}."),
            Blocked::NeedsNode => "Potato Bot needs Node.js 20 or newer.".to_string(),
            Blocked::MissingProgram => "Potato Bot's brain program is missing. Reinstall Puppet Master.".to_string(),
            Blocked::NoBridge => "Puppet Master is still starting. Try again in a moment.".to_string(),
        }
    }

    /// One message from the person at this computer.
    pub fn send(&self, bot: &str, text: &str) -> Result<SendOutcome> {
        self.require_bot(bot)?;
        let now = self.host.now_ms();
        let settings = store::load_settings(&self.root)?;
        let day = self.host.local_day(now);
        let used = cost::day_of(&self.root, &day, bot).map_err(|e| err("POTATO_STORE_INVALID", e))?.cost_nano();
        let over = cost::budget(used, settings.daily_limit_cents) == Budget::Over;
        let text = text.trim();
        let conversation = store::app_conversation(bot);
        let name = self.name_of(bot);
        // Asked of the PC before the lock is taken: finding Node can take a moment.
        let blocked = if self.brains.enabled() { self.preflight(&settings) } else { None };

        let mut g = self.inner.lock();
        let rt = g.bots.entry(bot.to_string()).or_insert_with(BotRt::new);
        let caps = Caps {
            disabled: !self.brains.enabled(),
            paused: false,
            over_limit: over,
            sends_last_minute: Self::rate_ok(rt, now),
        };
        let facts = Facts {
            surface: "app",
            sender: Sender::App,
            channel: Channel::NotAChannel,
            mentioned: true,
            replied_to_bot: false,
            from_bot: false,
            age_ms: 0,
            text_is_empty: text.is_empty(),
        };
        let decision = rules::decide(&facts, &rules::Config::default(), &caps);
        let (trust, tools) = match decision {
            Decision::Send { trust, tools, .. } => (trust, tools),
            Decision::Drop(rules::Why::Disabled) => return Err(err("POTATO_OFF", "Potato Bot is switched off.")),
            Decision::Drop(rules::Why::Empty) => return Err(err("POTATO_EMPTY", "Write a message first.")),
            Decision::Drop(rules::Why::RateLimited) => return Err(err("POTATO_RATE", "That is too fast. Wait a moment, then try again.")),
            Decision::Drop(rules::Why::OverLimit) => {
                drop(g);
                let mine = Message::new(now, FromKind::You, None, MsgKind::Text, text);
                let saved = self.save(bot, &conversation, mine)?;
                self.note(bot, &format!("{name} has used today's limit ({}). Raise it in Settings or wait until tomorrow.", cost::dollars(settings.daily_limit_cents)));
                self.host.changed(bot);
                return Ok(SendOutcome { id: saved.id, state: BrainState::DailyLimit, delivered: false });
            }
            Decision::Drop(_) | Decision::Pending => return Err(err("POTATO_REFUSED", "That message was not sent.")),
        };

        let mine = Message::new(now, FromKind::You, None, MsgKind::Text, text);
        let saved = self.save(bot, &conversation, mine)?;
        let sent_text = crate::redact::text(&store::clean_text(text, self.home_of(bot).as_deref())).into_inner();

        if settings.brain == BrainKind::Agents {
            drop(g);
            self.note(bot, "Talking through your agents is not ready yet.");
            self.host.changed(bot);
            return Ok(SendOutcome { id: saved.id, state: BrainState::Agents, delivered: false });
        }
        if let Some(blocked) = blocked {
            rt.blocked = Some(blocked);
            let line = self.blocked_note(bot, blocked);
            drop(g);
            self.note(bot, &line);
            self.host.changed(bot);
            let state = if blocked == Blocked::NeedsKey { BrainState::NeedsKey } else if blocked == Blocked::NeedsNode { BrainState::NeedsNode } else { BrainState::MissingProgram };
            return Ok(SendOutcome { id: saved.id, state, delivered: false });
        }
        if rt.keeper.is_stopped() {
            drop(g);
            self.note(bot, &format!("{name}'s brain stopped. Press Start again."));
            self.host.changed(bot);
            return Ok(SendOutcome { id: saved.id, state: BrainState::Stopped, delivered: false });
        }
        rt.blocked = None;
        rt.sends_ms.push_back(now);
        rt.turn_trust.insert(conversation.clone(), trust);
        let item = protocol::send_item(&protocol::SendFacts {
            id: &saved.id,
            conversation_id: &conversation,
            surface: "app",
            from_id: "owner",
            from_name: "you",
            trust,
            text: &sent_text,
            context: None,
            tools,
        });
        self.deliver(rt, &saved.id, &conversation, item);
        rt.keeper.want_run(now);
        let actions = rt.keeper.step(now);
        self.run_actions(&mut g, bot, actions);
        let state = self.state_of(&g, bot, &settings, over);
        drop(g);
        self.host.changed(bot);
        Ok(SendOutcome { id: saved.id, state, delivered: true })
    }

    /// Puts a `send` into the live session's inbox, or keeps it for the next one.
    fn deliver(&self, rt: &mut BotRt, id: &str, conversation: &str, item: Value) {
        match rt.session.as_mut() {
            Some(session) if session.shutdown_at.is_none() => {
                session.send_index.insert(id.to_string(), session.inbox.end());
                session.in_flight.insert(id.to_string(), conversation.to_string());
                session.inbox.push(item);
                self.cv.notify_all();
            }
            _ => rt.queued.push(item),
        }
    }

    /// "Start again": forgets the crashes and starts.
    pub fn start_bot(&self, bot: &str) -> Result<()> {
        self.require_bot(bot)?;
        let now = self.host.now_ms();
        let settings = store::load_settings(&self.root)?;
        let blocked = self.preflight(&settings);
        let mut g = self.inner.lock();
        let rt = g.bots.entry(bot.to_string()).or_insert_with(BotRt::new);
        rt.blocked = blocked;
        if blocked.is_none() && settings.brain != BrainKind::Agents && self.brains.enabled() {
            rt.keeper.retry(now);
            let actions = rt.keeper.step(now);
            self.run_actions(&mut g, bot, actions);
        }
        drop(g);
        self.host.changed(bot);
        Ok(())
    }

    /// The page's Stop button: asks the brain to leave. The next message starts it again.
    pub fn stop_bot(&self, bot: &str) {
        let now = self.host.now_ms();
        let mut g = self.inner.lock();
        if let Some(rt) = g.bots.get_mut(bot) {
            rt.keeper.person_stop();
            let actions = rt.keeper.step(now);
            self.run_actions(&mut g, bot, actions);
        }
        drop(g);
        self.host.changed(bot);
    }

    /// Stops every brain now: the kill switch, the panic button and the app's exit. Each brain is asked to
    /// leave and given `grace`; what is still there is killed with its whole tree.
    pub fn stop_all(&self, grace: Duration) {
        let bots: Vec<String> = self.inner.lock().bots.keys().cloned().collect();
        for bot in &bots {
            self.ask_to_leave(bot);
        }
        self.finish_stops(&bots, grace);
    }

    fn stop_all_of(&self, bot: &str, grace: Duration) {
        self.ask_to_leave(bot);
        self.finish_stops(&[bot.to_string()], grace);
    }

    fn ask_to_leave(&self, bot: &str) {
        let now = self.host.now_ms();
        let mut g = self.inner.lock();
        if let Some(rt) = g.bots.get_mut(bot) {
            rt.keeper.person_stop();
            let actions = rt.keeper.step(now);
            self.run_actions(&mut g, bot, actions);
        }
    }

    fn finish_stops(&self, bots: &[String], grace: Duration) {
        let deadline = std::time::Instant::now() + grace;
        loop {
            let mut any = false;
            {
                let mut g = self.inner.lock();
                for bot in bots {
                    let gone = g.bots.get_mut(bot).and_then(|rt| rt.session.as_mut()).map(|s| s.child.try_wait().is_some());
                    match gone {
                        Some(true) => self.end_session(&mut g, bot, true),
                        Some(false) => any = true,
                        None => {}
                    }
                }
            }
            if !any || std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let mut g = self.inner.lock();
        for bot in bots {
            if let Some(session) = g.bots.get_mut(bot).and_then(|rt| rt.session.as_mut()) {
                session.child.kill_tree();
            }
            self.end_session(&mut g, bot, true);
        }
        drop(g);
        for bot in bots {
            self.host.changed(bot);
        }
    }

    /// Potato Bot on or off (the Labs switch). Off kills every brain, makes the brain routes 404 and
    /// withdraws the helper cards.
    pub fn set_enabled(&self, on: bool) {
        let was = self.brains.enabled();
        if was && !on {
            // The routes stay open for a moment so `shutdown` can reach a brain; then they close and
            // whatever is left is killed.
            self.stop_all(Duration::from_millis(300));
            self.brains.set_enabled(false);
            self.stop_all(Duration::ZERO);
            self.withdraw_cards();
        } else {
            self.brains.set_enabled(on);
        }
    }

    /// A card nobody answered is withdrawn when Potato Bot goes off.
    fn withdraw_cards(&self) {
        let Ok(bots) = store::load_bots(&self.root) else { return };
        for bot in bots {
            for request_id in self.open_card_ids(&bot.id) {
                let mut row = Message::new(self.host.now_ms(), FromKind::Note, None, MsgKind::HelperResult, "Withdrawn: Potato Bot was switched off.");
                row.request_id = Some(request_id);
                let _ = self.save(&bot.id, &store::app_conversation(&bot.id), row);
            }
            self.host.changed(&bot.id);
        }
    }

    // ---- views ---------------------------------------------------------------------------------

    pub(super) fn open_card_ids(&self, bot: &str) -> Vec<String> {
        let Ok(messages) = store::read_messages(&self.root, &store::app_conversation(bot), 500, None) else { return Vec::new() };
        let closed: Vec<&str> = messages.iter().filter(|m| m.kind == MsgKind::HelperResult).filter_map(|m| m.request_id.as_deref()).collect();
        messages
            .iter()
            .filter(|m| m.kind == MsgKind::HelperCard)
            .filter_map(|m| m.request_id.clone())
            .filter(|id| !closed.contains(&id.as_str()))
            .collect()
    }

    fn state_of(&self, g: &Inner, bot: &str, settings: &store::Settings, over: bool) -> BrainState {
        if !self.brains.enabled() {
            return BrainState::Off;
        }
        if settings.brain == BrainKind::Agents {
            return BrainState::Agents;
        }
        let Some(rt) = g.bots.get(bot) else {
            return match self.preflight_cheap(settings) {
                Some(Blocked::NeedsKey) => BrainState::NeedsKey,
                Some(Blocked::NeedsNode) => BrainState::NeedsNode,
                Some(Blocked::MissingProgram) => BrainState::MissingProgram,
                _ if over => BrainState::DailyLimit,
                _ => BrainState::Idle,
            };
        };
        match rt.keeper.phase() {
            Phase::Stopped => return BrainState::Stopped,
            Phase::Starting { .. } => return BrainState::Starting,
            Phase::Waiting { .. } => return BrainState::Restarting,
            Phase::Ready => {
                return if over && rt.paused_for_limit.is_some() { BrainState::DailyLimit } else { BrainState::Ready };
            }
            Phase::Off => {}
        }
        match rt.blocked.or_else(|| self.preflight_cheap(settings)) {
            Some(Blocked::NeedsKey) => BrainState::NeedsKey,
            Some(Blocked::NeedsNode) => BrainState::NeedsNode,
            Some(Blocked::MissingProgram) => BrainState::MissingProgram,
            _ if over => BrainState::DailyLimit,
            _ => BrainState::Idle,
        }
    }

    /// Like `preflight`, but never probes Node (a probe can take seconds): the key only, and what the
    /// last probe found.
    fn preflight_cheap(&self, settings: &store::Settings) -> Option<Blocked> {
        match settings.brain {
            BrainKind::Deepseek if !self.has_key() => Some(Blocked::NeedsKey),
            _ => None,
        }
    }

    pub fn state(&self) -> Result<PotatoState> {
        let settings = store::load_settings(&self.root)?;
        let now = self.host.now_ms();
        let day = self.host.local_day(now);
        let reads = store::load_reads(&self.root)?;
        let key = secrets::mask(self.host.secret(SECRET_DEEPSEEK).as_deref());
        let node = match self.launcher.node() {
            NodeState::Ready { .. } => NodeView { ready: true, found: None },
            NodeState::Missing => NodeView { ready: false, found: None },
            NodeState::TooOld { found } => NodeView { ready: false, found: Some(found) },
        };
        let mut bots = Vec::new();
        for bot in store::load_bots(&self.root)? {
            let used = cost::day_of(&self.root, &day, &bot.id).map(|d| d.cost_nano()).unwrap_or(0);
            let over = cost::budget(used, settings.daily_limit_cents) == Budget::Over;
            let thread = store::read_messages(&self.root, &store::app_conversation(&bot.id), 200, self.home_of(&bot.id).as_deref()).unwrap_or_default();
            let read_at = reads.get(&bot.id).copied().unwrap_or(0);
            let unread = thread.iter().filter(|m| m.from.kind == FromKind::Bot && m.at > read_at).count() as u32;
            let last = thread.iter().rev().find(|m| m.kind != MsgKind::Note);
            let g = self.inner.lock();
            let rt = g.bots.get(&bot.id);
            let brain_state = self.state_of(&g, &bot.id, &settings, over);
            let view = BotView {
                name: self.name_of(&bot.id),
                talk_persona: bot.talk_persona.clone(),
                brain_state,
                last_error: rt.and_then(|r| r.last_error).map(|c| c.as_str().to_string()),
                typing: rt.is_some_and(|r| r.typing.values().any(|at| now - *at < TYPING_TTL_MS)),
                mood: rt.and_then(|r| r.mood.clone()),
                provider: rt.and_then(|r| r.provider.clone()),
                unread,
                last_line: last.map(|m| m.text.chars().take(80).collect()),
                last_at: last.map(|m| m.at),
                used_today_nano: used,
                limit_cents: settings.daily_limit_cents,
                open_cards: self.open_card_ids(&bot.id).len() as u32,
                id: bot.id,
            };
            drop(g);
            bots.push(view);
        }
        Ok(PotatoState { enabled: self.brains.enabled(), settings, key, node, bots })
    }

    /// The process id of the bot's running brain, if it has one.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn brain_pid(&self, bot: &str) -> Option<u32> {
        self.inner.lock().bots.get(bot).and_then(|rt| rt.session.as_ref()).map(|s| s.child.pid())
    }

    /// The cleaned lines of the brain's log (200 at most).
    pub fn log_lines(&self, bot: &str) -> Vec<String> {
        self.inner.lock().bots.get(bot).map(|rt| rt.log.lock().lines()).unwrap_or_default()
    }

    // ---- hooks for the helpers (helpers.rs) ---------------------------------------------------

    /// Puts one item into the live brain's inbox. False when the bot has no running brain (or one that is
    /// leaving): the item is not kept, because a brain that starts later knows nothing of the request.
    pub(super) fn push_to_brain(&self, bot: &str, item: Value) -> bool {
        let mut g = self.inner.lock();
        match g.bots.get_mut(bot).and_then(|rt| rt.session.as_mut()) {
            Some(session) if session.shutdown_at.is_none() => {
                session.inbox.push(item);
                self.cv.notify_all();
                true
            }
            _ => false,
        }
    }

    /// Whether the brain may be given a turn of its own now: it is not paused for the day's limit and the
    /// day's cost is under the limit.
    pub(super) fn brain_may_take_a_turn(&self, bot: &str) -> bool {
        let Ok(settings) = store::load_settings(&self.root) else { return false };
        let day = self.host.local_day(self.host.now_ms());
        let used = cost::day_of(&self.root, &day, bot).map(|d| d.cost_nano()).unwrap_or(0);
        if cost::budget(used, settings.daily_limit_cents) == Budget::Over {
            return false;
        }
        !self.inner.lock().bots.get(bot).is_some_and(|rt| rt.paused_for_limit.is_some())
    }

    /// A helper is still working: the idle timer of that bot's brain does not run out.
    pub(super) fn keep_awake(&self, bot: &str) {
        let now = self.host.now_ms();
        if let Some(rt) = self.inner.lock().bots.get_mut(bot) {
            rt.keeper.touch(now);
        }
    }

    pub(super) fn host(&self) -> &Arc<dyn Host> {
        &self.host
    }

    pub(super) fn changed(&self, bot: &str) {
        self.host.changed(bot);
    }

    // ---- keeper actions ------------------------------------------------------------------------

    fn run_actions(&self, g: &mut Inner, bot: &str, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::Spawn => self.spawn(g, bot),
                Action::Shutdown => {
                    let now = self.host.now_ms();
                    if let Some(session) = g.bots.get_mut(bot).and_then(|rt| rt.session.as_mut()) {
                        if session.shutdown_at.is_none() {
                            session.shutdown_at = Some(now);
                            session.inbox.push(protocol::shutdown_item());
                            self.cv.notify_all();
                        }
                    }
                }
                Action::Kill => {
                    if let Some(session) = g.bots.get_mut(bot).and_then(|rt| rt.session.as_mut()) {
                        session.child.kill_tree();
                    }
                    self.end_session(g, bot, false);
                }
                Action::RaiseStopped => {
                    let name = self.name_of(bot);
                    self.note(bot, &format!("{name}'s brain stopped. Press Start again."));
                }
            }
        }
    }

    fn spawn(&self, g: &mut Inner, bot: &str) {
        let now = self.host.now_ms();
        let rt = g.bots.entry(bot.to_string()).or_insert_with(BotRt::new);
        match self.try_spawn(rt, bot) {
            Ok(()) => rt.keeper.spawned(now),
            Err(Some(blocked)) => {
                // Not a crash: it waits for the person.
                rt.blocked = Some(blocked);
                rt.keeper.person_stop();
                let line = self.blocked_note(bot, blocked);
                self.note(bot, &line);
            }
            Err(None) => {
                let actions = rt.keeper.spawn_failed(now);
                self.run_actions(g, bot, actions);
            }
        }
    }

    /// Starts the process. `Err(Some(_))`: something the person must fix; `Err(None)`: it failed (a crash).
    fn try_spawn(&self, rt: &mut BotRt, bot: &str) -> std::result::Result<(), Option<Blocked>> {
        let settings = store::load_settings(&self.root).map_err(fail)?;
        if settings.brain == BrainKind::Agents {
            return Err(None);
        }
        let node = match self.launcher.node() {
            NodeState::Ready { path, .. } => path,
            _ => return Err(Some(Blocked::NeedsNode)),
        };
        let resources = self.host.resources();
        let script = super::sidecar::script_path(resources.as_deref()).ok_or(Some(Blocked::MissingProgram))?;
        let url = self.host.bridge_url().ok_or(Some(Blocked::NoBridge))?;
        let key = self.host.secret(SECRET_DEEPSEEK).filter(|k| !k.trim().is_empty());
        if settings.brain == BrainKind::Deepseek && key.is_none() {
            return Err(Some(Blocked::NeedsKey));
        }
        let pack = store::load_bots(&self.root).map_err(fail)?.into_iter().find(|b| b.id == bot).map(|b| b.talk_persona).ok_or(None::<Blocked>)?;
        let cwd = store::ensure_brain_dirs(&self.root, bot).map_err(fail)?;
        if let Some(persona) = resources.as_ref().map(|r| r.join("persona")).filter(|p| p.is_dir()) {
            store::seed_persona(&self.root, bot, &persona).map_err(fail)?;
        }
        let token = auth::mint_token();
        let host_env = self.host.host_env();
        let vault = store::vault_dir(&self.root, bot).map_err(fail)?;
        let persona_dir = store::persona_dir(&self.root, bot).map_err(fail)?;
        let state_dir = store::state_dir(&self.root, bot).map_err(fail)?;
        let share_dir = store::share_dir(&self.root, bot).map_err(fail)?;
        let brain_env = env::build(&env::Inputs {
            brain: if settings.brain == BrainKind::Go { Brain::Go } else { Brain::DeepSeek },
            pack: &pack,
            model: None,
            vault: &vault,
            persona_dir: &persona_dir,
            state_dir: &state_dir,
            share_dir: &share_dir,
            url: &url,
            token: &token,
            bot,
            deepseek_key: key.as_deref(),
            host: &host_env,
        });
        let secret_values = brain_env.secrets();
        self.brains.register(bot, &token);
        let ring = rt.log.clone();
        let hide = secret_values.clone();
        let sink: LineSink = Arc::new(move |line: &str| {
            let cleaned = ring.lock().push(line, &hide);
            tracing::debug!(target: "potato_brain", "{cleaned}");
        });
        let spec = SpawnSpec { node, script, cwd, env: brain_env };
        match self.launcher.spawn(&spec, sink) {
            Ok(child) => {
                self.brains.set_pid(bot, child.pid());
                let mut inbox = Inbox::default();
                let mut session = Session {
                    child,
                    inbox: Inbox::default(),
                    secrets: secret_values,
                    shutdown_at: None,
                    in_flight: HashMap::new(),
                    send_index: HashMap::new(),
                };
                for item in rt.queued.drain(..) {
                    if let (Some(id), Some(conversation)) = (item.get("id").and_then(Value::as_str), item.get("conversationId").and_then(Value::as_str)) {
                        session.send_index.insert(id.to_string(), inbox.end());
                        session.in_flight.insert(id.to_string(), conversation.to_string());
                    }
                    inbox.push(item);
                }
                session.inbox = inbox;
                rt.session = Some(session);
                rt.provider = None;
                self.cv.notify_all();
                Ok(())
            }
            Err(e) => {
                self.brains.unregister(bot);
                tracing::warn!(%bot, error = %e, "could not start the brain");
                Err(None)
            }
        }
    }

    /// The session is over (the process left or was killed): forget its token, keep what was not
    /// handed to it for the next one, and say so for what it never answered.
    fn end_session(&self, g: &mut Inner, bot: &str, asked: bool) {
        let now = self.host.now_ms();
        let Some(rt) = g.bots.get_mut(bot) else { return };
        let Some(session) = rt.session.take() else { return };
        self.brains.unregister(bot);
        let _ = &session.secrets;
        rt.typing.clear();
        let mut lost: Vec<(String, String)> = Vec::new();
        for (id, conversation) in &session.in_flight {
            let index = session.send_index.get(id).copied().unwrap_or(0);
            if index >= session.inbox.handed {
                // Never handed to the brain: it goes to the next one.
                if let Some(item) = session.inbox.items.get((index.saturating_sub(session.inbox.base)) as usize) {
                    rt.queued.push(item.clone());
                }
            } else {
                lost.push((id.clone(), conversation.clone()));
            }
        }
        if !lost.is_empty() {
            let name = self.name_of(bot);
            for (_, conversation) in &lost {
                let msg = Message::new(now, FromKind::Note, None, MsgKind::Note, &format!("{name}'s brain stopped before it answered."));
                let _ = self.save(bot, conversation, msg);
            }
        }
        let actions = if rt.keeper.is_alive() { rt.keeper.exited(now, asked) } else { Vec::new() };
        // A stop that was asked for may still want a start (a new message came while it was leaving).
        let more = if asked { rt.keeper.step(now) } else { Vec::new() };
        let all: Vec<Action> = actions.into_iter().chain(more).collect();
        self.run_actions(g, bot, all);
        self.host.changed(bot);
    }

    /// The supervisor. Called every half second by the live thread; the tests call it by hand.
    pub fn tick(&self) {
        self.watch_helpers();
        let now = self.host.now_ms();
        let day = self.host.local_day(now);
        let bots: Vec<String> = self.inner.lock().bots.keys().cloned().collect();
        for bot in bots {
            let mut g = self.inner.lock();
            let exited = g.bots.get_mut(&bot).and_then(|rt| rt.session.as_mut()).and_then(|s| s.child.try_wait().map(|code| (code, s.shutdown_at.is_some())));
            if let Some((_, asked)) = exited {
                self.end_session(&mut g, &bot, asked);
            }
            // A brain that was asked to leave is killed after the grace.
            let overdue = g.bots.get(&bot).and_then(|rt| rt.session.as_ref()).is_some_and(|s| s.shutdown_at.is_some_and(|at| now - at >= SHUTDOWN_GRACE_MS));
            if overdue {
                if let Some(session) = g.bots.get_mut(&bot).and_then(|rt| rt.session.as_mut()) {
                    session.child.kill_tree();
                }
                self.end_session(&mut g, &bot, true);
            }
            let actions = g.bots.get_mut(&bot).map(|rt| rt.keeper.step(now)).unwrap_or_default();
            self.run_actions(&mut g, &bot, actions);
            // A new day lifts the daily pause.
            if let Some(rt) = g.bots.get_mut(&bot) {
                if rt.paused_for_limit.as_deref().is_some_and(|d| d != day) {
                    rt.paused_for_limit = None;
                    if let Some(session) = rt.session.as_mut() {
                        session.inbox.push(protocol::resume_item());
                        self.cv.notify_all();
                    }
                }
            }
        }
    }

    /// A raised limit lifts the pause at once.
    fn recheck_limits(&self) {
        let Ok(settings) = store::load_settings(&self.root) else { return };
        let day = self.host.local_day(self.host.now_ms());
        let mut g = self.inner.lock();
        let bots: Vec<String> = g.bots.keys().cloned().collect();
        for bot in bots {
            let used = cost::day_of(&self.root, &day, &bot).map(|d| d.cost_nano()).unwrap_or(0);
            if let Some(rt) = g.bots.get_mut(&bot) {
                if rt.paused_for_limit.is_some() && cost::budget(used, settings.daily_limit_cents) != Budget::Over {
                    rt.paused_for_limit = None;
                    if let Some(session) = rt.session.as_mut() {
                        session.inbox.push(protocol::resume_item());
                        self.cv.notify_all();
                    }
                }
            }
        }
    }

    // ---- the brain's side ----------------------------------------------------------------------

    /// `GET /potato/brain/inbox`: items from position `at`, waiting up to `wait_ms` when there are none.
    pub fn inbox_poll(&self, bot: &str, at: u64, wait_ms: u64) -> Value {
        let wait = Duration::from_millis(wait_ms.min(protocol::MAX_WAIT_MS));
        let deadline = std::time::Instant::now() + wait;
        let mut g = self.inner.lock();
        loop {
            let Some(session) = g.bots.get_mut(bot).and_then(|rt| rt.session.as_mut()) else {
                return serde_json::json!({ "v": 1, (wire_names::POLL_POSITION): at, "items": [] });
            };
            let (items, next) = session.inbox.since(at);
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if !items.is_empty() || left.is_zero() {
                return serde_json::json!({ "v": 1, (wire_names::POLL_POSITION): next, "items": items });
            }
            self.cv.wait_for(&mut g, left);
        }
    }

    /// `POST /potato/brain/events`.
    pub fn ingest(&self, bot: &str, events: Vec<Event>) {
        let now = self.host.now_ms();
        let mut changed = false;
        for event in events {
            changed |= self.on_event(bot, event, now);
        }
        if changed {
            self.host.changed(bot);
        }
    }

    fn own_conversation(&self, bot: &str, conversation: &str) -> bool {
        store::app_bot_of(conversation) == Some(bot)
    }

    fn on_event(&self, bot: &str, event: Event, now: i64) -> bool {
        // Only a bot that is known and running may post.
        if self.require_bot(bot).is_err() {
            return false;
        }
        match event {
            Event::Hello { protocol: version, provider, .. } => {
                let mut g = self.inner.lock();
                let Some(rt) = g.bots.get_mut(bot) else { return false };
                if version == protocol::V {
                    rt.keeper.hello(now);
                    rt.provider = Some(provider);
                }
                true
            }
            Event::Typing { conversation_id, on } => {
                if !self.own_conversation(bot, &conversation_id) {
                    return false;
                }
                let mut g = self.inner.lock();
                let Some(rt) = g.bots.get_mut(bot) else { return false };
                rt.keeper.touch(now);
                if on {
                    rt.typing.insert(conversation_id, now);
                } else {
                    rt.typing.remove(&conversation_id);
                }
                true
            }
            Event::Reply { conversation_id, in_reply_to, segments, kaomoji, .. } => {
                if !self.own_conversation(bot, &conversation_id) {
                    return false;
                }
                let name = self.name_of(bot);
                let mood = {
                    let mut g = self.inner.lock();
                    let Some(rt) = g.bots.get_mut(bot) else { return false };
                    rt.keeper.touch(now);
                    rt.last_error = None;
                    if let Some(session) = rt.session.as_mut() {
                        session.in_flight.remove(&in_reply_to);
                    }
                    rt.mood.as_ref().map(|m| m.mood.clone())
                };
                let last = segments.len().saturating_sub(1);
                for (i, mut segment) in segments.into_iter().enumerate() {
                    if i == last {
                        if let Some(face) = kaomoji.as_deref().filter(|k| !k.trim().is_empty()) {
                            segment.push(' ');
                            segment.push_str(face.trim());
                        }
                    }
                    let mut message = Message::new(now, FromKind::Bot, Some(&name), MsgKind::Text, &segment);
                    if i == last {
                        message.mood = mood.clone();
                    }
                    if let Err(e) = self.save(bot, &conversation_id, message) {
                        tracing::debug!(%e, "could not save a reply");
                    }
                }
                true
            }
            Event::Mood { mood, intensity, energy, hunger } => {
                let mut g = self.inner.lock();
                let Some(rt) = g.bots.get_mut(bot) else { return false };
                rt.keeper.touch(now);
                rt.mood = Some(MoodNow { mood, intensity, energy, hunger });
                true
            }
            Event::Usage { model, prompt_tokens, completion_tokens, cache_hit_tokens, cache_miss_tokens } => {
                self.on_usage(bot, now, &model, cost::Tokens { prompt: prompt_tokens, completion: completion_tokens, cache_hit: cache_hit_tokens, cache_miss: cache_miss_tokens });
                true
            }
            Event::HelperRequest { request_id, conversation_id, task, project } => self.on_helper_request(bot, now, request_id, conversation_id, task, project),
            Event::Error { code, conversation_id, in_reply_to } => {
                let name = self.name_of(bot);
                let line = match code {
                    ErrorCode::ProviderAuth => format!("{name} could not use the model key. Check it in Settings."),
                    ErrorCode::ProviderLimit => "The model is out of balance or too busy. Try again later.".to_string(),
                    ErrorCode::ProviderDown => "The model is not answering right now. Try again in a minute.".to_string(),
                    ErrorCode::Network => format!("{name} could not reach the internet."),
                    ErrorCode::PersonaTooBig => format!("{name}'s persona text is too long."),
                    ErrorCode::SurfaceAuth | ErrorCode::Internal => format!("Something went wrong inside {name}'s brain."),
                };
                {
                    let mut g = self.inner.lock();
                    let Some(rt) = g.bots.get_mut(bot) else { return false };
                    rt.keeper.touch(now);
                    rt.last_error = Some(code);
                    if let (Some(session), Some(id)) = (rt.session.as_mut(), in_reply_to.as_deref()) {
                        session.in_flight.remove(id);
                    }
                }
                let conversation = conversation_id.filter(|c| self.own_conversation(bot, c)).unwrap_or_else(|| store::app_conversation(bot));
                let _ = self.save(bot, &conversation, Message::new(now, FromKind::Note, None, MsgKind::Note, &line));
                true
            }
            Event::Unknown(_) => false,
        }
    }

    fn on_usage(&self, bot: &str, now: i64, model: &str, tokens: cost::Tokens) {
        let Ok(settings) = store::load_settings(&self.root) else { return };
        let day = self.host.local_day(now);
        let surface = self.inner.lock().bots.get(bot).map(|rt| rt.last_surface.clone()).unwrap_or_else(|| "app".into());
        let charged = match cost::charge(&self.root, &day, bot, &surface, model, tokens, settings.daily_limit_cents) {
            Ok(charged) => charged,
            Err(e) => {
                tracing::warn!(%bot, error = %e, "could not save usage");
                return;
            }
        };
        let name = self.name_of(bot);
        let limit = cost::dollars(settings.daily_limit_cents);
        if charged.limit_now {
            self.note(bot, &format!("{name} has used today's limit ({limit}). Raise it in Settings or wait until tomorrow."));
            let mut g = self.inner.lock();
            if let Some(rt) = g.bots.get_mut(bot) {
                rt.paused_for_limit = Some(day);
                if let Some(session) = rt.session.as_mut() {
                    session.inbox.push(protocol::pause_item(PauseReason::DailyLimit));
                    self.cv.notify_all();
                }
            }
        } else if charged.warn_now {
            self.note(bot, &format!("{name} has used 80% of today's limit ({limit})."));
        }
    }

    fn on_helper_request(&self, bot: &str, now: i64, request_id: String, conversation_id: String, task: String, project: Option<String>) -> bool {
        if !self.own_conversation(bot, &conversation_id) {
            return false;
        }
        let trust = self.inner.lock().bots.get(bot).and_then(|rt| rt.turn_trust.get(&conversation_id).copied()).unwrap_or(Trust::Others);
        let refuse = |status: HelperStatus, summary: &str| {
            let mut g = self.inner.lock();
            if let Some(session) = g.bots.get_mut(bot).and_then(|rt| rt.session.as_mut()) {
                session.inbox.push(protocol::helper_update_item(&request_id, status, summary));
                self.cv.notify_all();
            }
        };
        if !trust.is_owner() {
            refuse(HelperStatus::Refused, "");
            return false;
        }
        if self.open_card_ids(bot).len() >= MAX_OPEN_CARDS {
            refuse(HelperStatus::Declined, "There are already too many open helper cards.");
            return false;
        }
        let mut card = Message::new(now, FromKind::Bot, Some(&self.name_of(bot)), MsgKind::HelperCard, &task);
        card.request_id = Some(request_id);
        card.project = self.card_folder(project.as_deref().and_then(project_name));
        self.save(bot, &conversation_id, card).is_ok()
    }
}

/// A start that failed for a reason that is not the person's to fix: a crash.
fn fail<E>(_: E) -> Option<Blocked> {
    None
}

/// What a bot asked for as a folder, reduced to a name: the last part of whatever it wrote, with no
/// slash and no dots-only part. A path the model wrote is never used, only matched by name later.
fn project_name(raw: &str) -> Option<String> {
    let name = raw.split(['/', '\\']).map(str::trim).rfind(|part| !part.is_empty())?;
    if name.chars().all(|c| c == '.') {
        return None;
    }
    Some(store::clean_text(name, None).chars().take(protocol::MAX_PROJECT).collect())
}
