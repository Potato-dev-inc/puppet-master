//! Helpers (spec section 2.2): how "do this job on my computer" becomes real work, and only after the
//! person says yes. The same for every surface.
//!
//! The bot cannot start anything. It asks (`helper_request`), the engine stores a card in the bot's app
//! thread, and the person answers on this computer:
//!
//! - **Start**: the folder is matched BY NAME against the person's recent projects (a path the model wrote
//!   is never used), the home folder and broad folders are refused, the day's cap is checked, and then
//!   `Hands::start` runs the companion's own agent in that folder as the companion's helper
//!   (`on_behalf_of_companion`: the 20-turn budget, the home rule, the feed line).
//! - **Change folder**: the page lists the recent project names; the chosen name goes with Start.
//! - **Not now**: the card closes and the brain is told.
//!
//! Caps: three open cards per bot (`service.rs` refuses the fourth) and ten starts per bot per day.
//!
//! When a helper ends (the operation reaches a final state, seen by `watch_helpers`, which `tick` calls),
//! exactly once: a row "Helper finished in <folder>." with an Open button is added, the brain gets one
//! `helper_update` (which gives it one turn; it is skipped while the brain is paused or over its limit), and
//! nothing else is sent anywhere. Running helpers are kept in `<app data>/potato/helpers.json`, so a restart
//! of the app does not lose track of them. Nothing here names an agent or a chat app.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::protocol::{self, HelperStatus};
use super::service::Service;
use super::store::{self, FromKind, Message, MsgKind, StoreError};
use crate::agent_runs::AgentRunRequest;
use crate::companion::broad::{self, Broad, Places};

type Result<T> = std::result::Result<T, StoreError>;

/// Helper starts per bot per local day.
pub const MAX_STARTS_PER_DAY: u32 = 10;
/// How often a running helper's operation is looked at.
const POLL_EVERY_MS: i64 = 2_000;
/// A helper nobody saw end after this long is let go.
const WATCH_LIMIT_MS: i64 = 24 * 60 * 60 * 1000;
/// A helper whose record cannot be found this many polls in a row is let go.
const GONE_LIMIT: u32 = 5;
/// The longest summary kept from a helper's result.
const SUMMARY_CHARS: usize = 1_500;

fn err(code: &'static str, message: impl Into<String>) -> StoreError {
    StoreError::new(code, message)
}

// ---- what the app lends the helpers -----------------------------------------------------------------

/// A folder the person has used: its own name and where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub name: String,
    pub path: String,
}

/// What a bot's companion is: the agent that does a helper's work, and the companion's own home folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanionAgent {
    pub agent_type: String,
    pub label: String,
    /// `companion-<id>`: the name the helper's run is filed under.
    pub pane_id: String,
    pub home: Option<String>,
}

/// A run that was started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub operation_id: String,
    pub pane_id: Option<String>,
}

/// Where a started run is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunState {
    Running,
    /// The run's record cannot be read (yet, or any more).
    Gone,
    /// `pane_id` is the pane the run worked in, as its record says now (the start may not have had one yet).
    Done { status: HelperStatus, summary: String, pane_id: Option<String> },
}

/// Everything the helpers do outside their own memory. The real one is `live::LiveHands`; the tests have a
/// fake that records what it was asked to start.
pub trait Hands: Send + Sync {
    fn agent_of(&self, bot: &str) -> Option<CompanionAgent>;
    /// The person's recent projects, most recent first. Only folders that exist.
    fn recent_folders(&self) -> Vec<Folder>;
    /// The standard folders of this account, for the "too broad" rule.
    fn places(&self) -> Places;
    /// `agent_runs::run_agent`, in the background. The error is a plain sentence.
    fn start(&self, request: AgentRunRequest) -> std::result::Result<Started, String>;
    fn poll(&self, project: &str, operation_id: &str) -> RunState;
    /// One tiny turn with the key: `Ok` when the model maker accepted it, else a plain sentence.
    fn check_key(&self, key: &str) -> std::result::Result<(), String>;
}

// ---- the saved book ------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
struct DayCount {
    day: String,
    n: u32,
}

/// A helper that is working now.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Running {
    pub bot: String,
    pub request_id: String,
    pub folder_name: String,
    pub project: String,
    pub operation_id: String,
    pub pane_id: Option<String>,
    pub started_ms: i64,
}

#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(default, rename_all = "camelCase")]
struct Book {
    v: u32,
    starts: HashMap<String, DayCount>,
    running: Vec<Running>,
}

#[derive(Default)]
struct Inner {
    loaded: bool,
    book: Book,
    /// Cards whose Start is being handled right now, so a second click cannot start a second run.
    starting: HashSet<String>,
    last_poll_ms: i64,
    gone: HashMap<String, u32>,
}

/// The helpers' state inside the `Service`: the hands (installed once, by `live::start`) and the book.
#[derive(Default)]
pub struct HelperState {
    hands: OnceLock<Arc<dyn Hands>>,
    inner: Mutex<Inner>,
}

fn book_path(root: &Path) -> std::path::PathBuf {
    root.join("helpers.json")
}

// ---- pure rules --------------------------------------------------------------------------------------

/// The recent folder with this name (case does not matter), the most recent one first. A name is never a path:
/// anything with a slash in it matches nothing.
pub fn pick_folder<'a>(recent: &'a [Folder], name: &str) -> Option<&'a Folder> {
    let name = name.trim();
    if name.is_empty() || name.contains(['/', '\\']) {
        return None;
    }
    recent.iter().find(|f| f.name.eq_ignore_ascii_case(name))
}

/// The names of the recent folders, each once (the most recent one wins a shared name).
pub fn unique_names(recent: &[Folder]) -> Vec<String> {
    let mut seen = HashSet::new();
    recent.iter().filter(|f| seen.insert(f.name.to_lowercase())).map(|f| f.name.clone()).collect()
}

/// Why a helper may not work in `path`, as a plain sentence, or `None` when it may. The companion's home, the
/// user home, a drive root, the standard folders and a folder that only holds other projects are refused. A
/// project that has its own `.git` is fine even when it has more projects below it.
pub fn folder_problem(path: &str, companion_home: Option<&str>, places: &Places) -> Option<String> {
    let folder = Path::new(path);
    if !folder.is_dir() {
        return Some("That folder is not there any more. Choose another one.".to_string());
    }
    if let Some(home) = companion_home {
        if crate::companion::delegate::inside_home(home, path) {
            return Some("That is the bot's own home folder. Choose a project folder.".to_string());
        }
    }
    match broad::broad_kind(folder, places) {
        None => None,
        Some(Broad::HasProjects) if folder.join(".git").exists() => None,
        Some(Broad::HasProjects) => Some("That folder holds many projects. Choose the one project the helper should work in.".to_string()),
        Some(_) => Some("That folder is too broad for a helper. Choose a project folder.".to_string()),
    }
}

/// The run a Start asks for. The task is the card's text; the folder is a path that passed the checks above.
pub fn run_request(agent: &CompanionAgent, project: &str, task: &str, bot: &str, request_id: &str) -> std::result::Result<AgentRunRequest, String> {
    let mut request: AgentRunRequest = serde_json::from_value(json!({
        "project_path": project,
        "task": task,
        "agent_type": agent.agent_type,
        "headless": false,
        "background": true,
        "keep_pane": true,
        "wait_ms": 0,
        "placement": "grid",
        "idempotency_key": format!("potato-{bot}-{request_id}"),
        "context_policy": "fresh",
    }))
    .map_err(|e| format!("The helper could not be set up: {e}"))?;
    request.on_behalf_of_companion = Some(agent.pane_id.clone());
    Ok(request)
}

fn finished_line(status: HelperStatus, folder: &str) -> String {
    match status {
        HelperStatus::Finished => format!("Helper finished in {folder}."),
        HelperStatus::Failed => format!("Helper could not finish in {folder}."),
        HelperStatus::Stopped => format!("Helper stopped in {folder}."),
        _ => format!("Helper ended in {folder}."),
    }
}

// ---- the engine's side ---------------------------------------------------------------------------------

/// What Start answers: the folder the helper works in, for the page's line.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HelperStartOutcome {
    pub folder: String,
}

impl Service {
    /// Installs what starts helpers. Once, at startup; a second call changes nothing.
    pub fn set_hands(&self, hands: Arc<dyn Hands>) {
        let _ = self.helpers.hands.set(hands);
    }

    fn hands(&self) -> Result<Arc<dyn Hands>> {
        self.helpers.hands.get().cloned().ok_or_else(|| err("POTATO_NOT_READY", "Potato Bot is not ready yet. Try again in a moment."))
    }

    /// Reads the book once. A damaged file is an error and is never overwritten.
    fn load_book(&self, inner: &mut Inner) -> Result<()> {
        if inner.loaded {
            return Ok(());
        }
        let found: Option<Book> = store::read_json(&book_path(self.root()), "helpers.json")?;
        inner.book = found.unwrap_or_default();
        inner.loaded = true;
        Ok(())
    }

    fn save_book(&self, inner: &mut Inner) -> Result<()> {
        inner.book.v = 1;
        store::write_json(&book_path(self.root()), &inner.book, "helpers.json")
    }

    fn day_now(&self) -> String {
        self.host().local_day(self.host().now_ms())
    }

    fn starts_today(&self, inner: &Inner, bot: &str) -> u32 {
        let day = self.day_now();
        inner.book.starts.get(bot).filter(|c| c.day == day).map(|c| c.n).unwrap_or(0)
    }

    /// The card with this request id, when nobody has answered it yet.
    fn open_card(&self, bot: &str, request_id: &str) -> Result<Message> {
        let gone = || err("POTATO_HELPER_GONE", "That helper request is not open any more.");
        let messages = store::read_messages(self.root(), &store::app_conversation(bot), 500, None)?;
        if messages.iter().any(|m| m.kind == MsgKind::HelperResult && m.request_id.as_deref() == Some(request_id)) {
            return Err(gone());
        }
        messages.into_iter().find(|m| m.kind == MsgKind::HelperCard && m.request_id.as_deref() == Some(request_id)).ok_or_else(gone)
    }

    /// The folder a new card shows. The name the bot gave counts only if it is one of the person's recent
    /// projects (then it is spelled the way the project is). No name: the person's most recent project that a
    /// helper may work in. A name that matches nothing: none, so the person must choose.
    pub(super) fn card_folder(&self, asked: Option<String>) -> Option<String> {
        let Some(hands) = self.helpers.hands.get() else { return asked };
        let recent = hands.recent_folders();
        match asked {
            Some(name) => pick_folder(&recent, &name).map(|f| f.name.clone()),
            None => {
                let places = hands.places();
                recent.iter().find(|f| folder_problem(&f.path, None, &places).is_none()).map(|f| f.name.clone())
            }
        }
    }

    /// The names "Change folder" offers: recent projects a helper may work in.
    pub fn helper_folders(&self, bot: &str) -> Result<Vec<String>> {
        self.require_bot(bot)?;
        let hands = self.hands()?;
        let home = hands.agent_of(bot).and_then(|a| a.home);
        let places = hands.places();
        let usable: Vec<Folder> = hands.recent_folders().into_iter().filter(|f| folder_problem(&f.path, home.as_deref(), &places).is_none()).collect();
        Ok(unique_names(&usable))
    }

    /// Start: the one place a helper begins. Nothing else in the app calls `Hands::start` for a bot.
    pub fn helper_start(&self, bot: &str, request_id: &str, folder: Option<&str>) -> Result<HelperStartOutcome> {
        if !self.enabled() {
            return Err(err("POTATO_OFF", "Potato Bot is switched off."));
        }
        self.require_bot(bot)?;
        let hands = self.hands()?;
        // No card, no helper. A card that was answered, or withdrawn, is not a card.
        let card = self.open_card(bot, request_id)?;
        {
            let mut inner = self.helpers.inner.lock();
            if !inner.starting.insert(request_id.to_string()) {
                return Err(err("POTATO_HELPER_BUSY", "That helper is already starting."));
            }
        }
        let outcome = self.start_checked(&*hands, bot, request_id, &card, folder);
        self.helpers.inner.lock().starting.remove(request_id);
        outcome
    }

    fn start_checked(&self, hands: &dyn Hands, bot: &str, request_id: &str, card: &Message, folder: Option<&str>) -> Result<HelperStartOutcome> {
        let name = self.name_of(bot);
        let agent = hands.agent_of(bot).ok_or_else(|| err("POTATO_NO_AGENT", format!("{name}'s agent is not available.")))?;
        {
            let mut inner = self.helpers.inner.lock();
            self.load_book(&mut inner)?;
            if self.starts_today(&inner, bot) >= MAX_STARTS_PER_DAY {
                return Err(err("POTATO_HELPER_LIMIT", format!("{name} has started {MAX_STARTS_PER_DAY} helpers today. Try again tomorrow.")));
            }
        }
        let wanted = folder.map(str::trim).filter(|f| !f.is_empty()).or(card.project.as_deref());
        let wanted = wanted.ok_or_else(|| err("POTATO_FOLDER_NEEDED", "Choose a folder for the helper first."))?;
        let recent = hands.recent_folders();
        let chosen = pick_folder(&recent, wanted).ok_or_else(|| err("POTATO_FOLDER_UNKNOWN", "Helpers only work in folders you have used before. Use Change folder."))?.clone();
        if let Some(problem) = folder_problem(&chosen.path, agent.home.as_deref(), &hands.places()) {
            return Err(err("POTATO_FOLDER_REFUSED", problem));
        }
        let request = run_request(&agent, &chosen.path, &card.text, bot, request_id).map_err(|m| err("POTATO_HELPER_FAILED", m))?;
        let started = hands.start(request).map_err(|m| err("POTATO_HELPER_FAILED", m))?;

        let now = self.host().now_ms();
        let day = self.day_now();
        {
            let mut inner = self.helpers.inner.lock();
            let count = inner.book.starts.entry(bot.to_string()).or_default();
            if count.day != day {
                *count = DayCount { day, n: 0 };
            }
            count.n += 1;
            inner.book.running.push(Running {
                bot: bot.to_string(),
                request_id: request_id.to_string(),
                folder_name: chosen.name.clone(),
                project: chosen.path.clone(),
                operation_id: started.operation_id.clone(),
                pane_id: started.pane_id.clone(),
                started_ms: now,
            });
            if let Err(e) = self.save_book(&mut inner) {
                tracing::warn!(error = %e, "could not save the running helpers");
            }
        }
        // The card closes: its request id now has a result row.
        let mut row = Message::new(now, FromKind::Note, None, MsgKind::HelperResult, &format!("Started a helper in {}.", chosen.name));
        row.request_id = Some(request_id.to_string());
        row.pane_id = started.pane_id.clone();
        self.save(bot, &store::app_conversation(bot), row)?;
        self.push_to_brain(bot, protocol::helper_update_item(request_id, HelperStatus::Started, ""));
        self.changed(bot);
        Ok(HelperStartOutcome { folder: chosen.name })
    }

    /// Not now: the card closes, the brain is told once.
    pub fn helper_decline(&self, bot: &str, request_id: &str) -> Result<()> {
        self.require_bot(bot)?;
        self.open_card(bot, request_id)?;
        let mut row = Message::new(self.host().now_ms(), FromKind::Note, None, MsgKind::HelperResult, "Not now.");
        row.request_id = Some(request_id.to_string());
        self.save(bot, &store::app_conversation(bot), row)?;
        if self.brain_may_take_a_turn(bot) {
            self.push_to_brain(bot, protocol::helper_update_item(request_id, HelperStatus::Declined, ""));
        }
        self.changed(bot);
        Ok(())
    }

    /// Settings > Potato Bot > "Check key": one tiny turn with the saved key. The key is read here and goes
    /// nowhere but the model maker; only a plain sentence comes back.
    pub fn check_key(&self) -> Result<String> {
        let hands = self.hands()?;
        let key = self.host().secret(super::service::SECRET_DEEPSEEK).filter(|k| !k.trim().is_empty());
        let key = key.ok_or_else(|| err("POTATO_NO_KEY", "Add your DeepSeek key first."))?;
        hands.check_key(&key).map_err(|m| err("POTATO_KEY_CHECK", m))?;
        Ok("Works.".to_string())
    }

    // ---- watching ------------------------------------------------------------------------------------

    /// Called by `tick`: looks at every running helper and finishes the ones that ended.
    pub(super) fn watch_helpers(&self) {
        let Some(hands) = self.helpers.hands.get().cloned() else { return };
        if !self.enabled() {
            return;
        }
        let now = self.host().now_ms();
        let due: Vec<Running> = {
            let mut inner = self.helpers.inner.lock();
            if self.load_book(&mut inner).is_err() || inner.book.running.is_empty() || now - inner.last_poll_ms < POLL_EVERY_MS {
                return;
            }
            inner.last_poll_ms = now;
            inner.book.running.clone()
        };
        for helper in due {
            match hands.poll(&helper.project, &helper.operation_id) {
                RunState::Done { status, summary, pane_id } => self.finish_helper(&helper, status, &summary, pane_id),
                RunState::Running => {
                    self.helpers.inner.lock().gone.remove(&helper.request_id);
                    if now - helper.started_ms > WATCH_LIMIT_MS {
                        self.finish_helper(&helper, HelperStatus::Stopped, "", None);
                    } else {
                        self.keep_awake(&helper.bot);
                    }
                }
                RunState::Gone => {
                    let misses = {
                        let mut inner = self.helpers.inner.lock();
                        let n = inner.gone.entry(helper.request_id.clone()).or_insert(0);
                        *n += 1;
                        *n
                    };
                    if misses >= GONE_LIMIT {
                        self.finish_helper(&helper, HelperStatus::Stopped, "", None);
                    }
                }
            }
        }
    }

    /// A helper ended. Exactly once: the book gives it up before anything is said.
    fn finish_helper(&self, helper: &Running, status: HelperStatus, summary: &str, pane_id: Option<String>) {
        {
            let mut inner = self.helpers.inner.lock();
            let before = inner.book.running.len();
            inner.book.running.retain(|r| r.request_id != helper.request_id);
            inner.gone.remove(&helper.request_id);
            if inner.book.running.len() == before {
                return;
            }
            if let Err(e) = self.save_book(&mut inner) {
                tracing::warn!(error = %e, "could not save the running helpers");
            }
        }
        let bot = helper.bot.as_str();
        let home = self.home_of(bot);
        let now = self.host().now_ms();
        let mut row = Message::new(now, FromKind::Note, None, MsgKind::HelperResult, &finished_line(status, &helper.folder_name));
        row.request_id = Some(helper.request_id.clone());
        row.pane_id = pane_id.or_else(|| helper.pane_id.clone());
        if let Err(e) = self.save(bot, &store::app_conversation(bot), row) {
            tracing::debug!(error = %e, "could not save a helper result");
        }
        // The brain hears once, and only when it can answer.
        if self.brain_may_take_a_turn(bot) {
            let cleaned = crate::redact::text(&store::clean_text(summary, home.as_deref())).into_inner();
            let short: String = cleaned.chars().take(SUMMARY_CHARS).collect();
            if self.push_to_brain(bot, protocol::helper_update_item(&helper.request_id, status, &short)) {
                self.keep_awake(bot);
            }
        }
        self.changed(bot);
    }

    /// The helpers working now (for tests and for the page's count).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn running_helpers(&self) -> Vec<Running> {
        let mut inner = self.helpers.inner.lock();
        let _ = self.load_book(&mut inner);
        inner.book.running.clone()
    }
}
