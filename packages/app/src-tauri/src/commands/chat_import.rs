//! Tauri commands of "Bring my old chats" (`chat_import_*`): the first look at the agents' folders, the person's choice,
//! the run, the read-only view of one old chat, and the plan to continue one in a pane. Desktop only: nothing here is
//! on the bridge, the phone or MCP.
//!
//! Thin: each command does its work on a blocking thread and returns what the screen shows. The importer is built once
//! (`ChatImportState`) on the index of this profile. `HistoryEnv::from_process()` is called only in this file, so every
//! agent folder is read through an environment built here. Nothing is logged here: no path, no chat text.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::{AppState, SessionChatState};
use crate::chat_index::import::{
    source_named, Emitter as ProgressEmitter, Found, ImportPrefs, ImportRequest, Importer, Phase, Progress, RegistryCatalog, PROGRESS_EVENT,
};
use crate::chat_index::{self, ChatIndex, Status};
use crate::session_chat::history::{HistoryEnv, OpenedChat, ResumePlan};

/// The importer and the lock of the choices file. The importer is built on first use, on the index of this profile.
#[derive(Default)]
pub struct ChatImportState {
    /// `None` when the index could not be opened: then there is nothing to import into.
    importer: OnceCell<Option<Arc<Importer>>>,
    /// Held while the choices file is read, changed and written, so two commands never overwrite each other.
    prefs_lock: Mutex<()>,
}

const NO_INDEX: &str = "Chat search could not be opened on this PC.";
/// How long "Remove imported chats" waits for a running import to stop before it clears anyway.
const STOP_WAIT: Duration = Duration::from_secs(3);

/// What the screen shows before anything is read: the saved choices, the agents, the run, and what is in the index.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportStateOut {
    asked: bool,
    since_days: Option<u32>,
    index_enabled: bool,
    agents: Vec<ImportAgentOut>,
    progress: Progress,
    imported: ImportCountsOut,
}

/// One agent in the list. An agent with no reader is listed too (installed ones), so the screen can grey it out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportAgentOut {
    id: String,
    label: String,
    from_definition: bool,
    enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCountsOut {
    chats: usize,
    items: usize,
}

/// Answer of `chat_import_look`: what each agent with a reader has.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportLookOut {
    found: Vec<Found>,
}

/// One agent as the registry knows it, before the choices are applied.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentFact {
    id: String,
    label: String,
    from_definition: bool,
    reader: bool,
}

/// The screen's state (`chat_import_state`). Reads no agent folder.
#[tauri::command]
pub async fn chat_import_state(app: AppHandle) -> Result<ImportStateOut, String> {
    blocking(move || {
        let prefs = read_prefs();
        let progress = importer_built(&app).map(|importer| importer.progress()).unwrap_or_else(Progress::idle);
        let imported = index_in(&app).and_then(|index| index.status().ok()).map(counts_of).unwrap_or((0, 0));
        Ok(state_out(&prefs, &agent_facts(), chat_index::enabled(), progress, imported))
    })
    .await
}

/// "Find my chats": what each agent with a reader holds. The first call that touches the agents' folders; only after
/// the person pressed the button.
#[tauri::command]
pub async fn chat_import_look(app: AppHandle, since_days: Option<u32>) -> Result<ImportLookOut, String> {
    blocking(move || {
        let importer = importer_in(&app).ok_or_else(|| NO_INDEX.to_string())?;
        let mut found = importer.scan(&HistoryEnv::from_process(), since_days);
        for entry in &mut found {
            entry.label = label_of(&entry.id);
        }
        Ok(ImportLookOut { found })
    })
    .await
}

/// "Add N chats to search": saves the choice (and marks the first question answered), then starts the run.
#[tauri::command]
pub async fn chat_import_start(app: AppHandle, agents: Vec<String>, since_days: Option<u32>) -> Result<Progress, String> {
    blocking(move || {
        let importer = importer_in(&app).ok_or_else(|| NO_INDEX.to_string())?;
        let sources = importer.sources();
        for agent in &agents {
            source_named(&sources, agent)?;
        }
        update_prefs(&app, |prefs| {
            prefs.asked = true;
            prefs.since_days = since_days;
            for source in &sources {
                prefs.set_agent(&source.id, agents.contains(&source.id));
            }
            // The importer has no end hook, so this is the time the run started.
            prefs.last_run_ms = now_ms();
        })?;
        importer.start(ImportRequest { agents: agents.clone(), since_days }, HistoryEnv::from_process())?;
        Ok(importer.progress())
    })
    .await
}

/// "Stop": asks the run to stop. Chats read so far stay.
#[tauri::command]
pub async fn chat_import_cancel(app: AppHandle) -> Result<Progress, String> {
    blocking(move || {
        Ok(match importer_built(&app) {
            Some(importer) => {
                importer.cancel();
                importer.progress()
            }
            None => Progress::idle(),
        })
    })
    .await
}

/// "Not now": marks the first question answered. Nothing is read.
#[tauri::command]
pub async fn chat_import_dismiss(app: AppHandle) -> Result<serde_json::Value, String> {
    blocking(move || {
        update_prefs(&app, |prefs| prefs.asked = true)?;
        Ok(serde_json::json!({}))
    })
    .await
}

/// "Remove imported chats": stops a running import, waits a moment for it to stop, then removes the old chats from
/// search. The agents' own files are not touched. Returns the index status after it.
#[tauri::command]
pub async fn chat_import_clear(app: AppHandle) -> Result<Status, String> {
    blocking(move || {
        if let Some(importer) = importer_built(&app) {
            importer.cancel();
            wait_until_stopped(&importer);
        }
        match index_in(&app) {
            Some(index) => index.clear_imported(),
            None => Ok(nothing_kept()),
        }
    })
    .await
}

/// The read-only view of one old chat (`Importer::open`).
#[tauri::command]
pub async fn chat_import_open(app: AppHandle, agent: String, session: String) -> Result<OpenedChat, String> {
    blocking(move || {
        let importer = importer_in(&app).ok_or_else(|| NO_INDEX.to_string())?;
        importer.open(&HistoryEnv::from_process(), &agent, &session)
    })
    .await
}

/// The plan to continue an old chat in a new pane (`Importer::resume_plan`). The screen starts the pane with it.
#[tauri::command]
pub async fn chat_import_resume_plan(app: AppHandle, agent: String, session: String) -> Result<ResumePlan, String> {
    blocking(move || {
        let importer = importer_in(&app).ok_or_else(|| NO_INDEX.to_string())?;
        importer.resume_plan(&HistoryEnv::from_process(), &agent, &session)
    })
    .await
}

/// Runs `work` on a blocking thread: opening the index and reading the agents' folders are not for the async runtime.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| format!("INTERNAL: {e}"))?
}

/// The search index of this profile (opened on first use, shared with the chat view).
fn index_in(app: &AppHandle) -> Option<Arc<ChatIndex>> {
    let state = app.state::<AppState>();
    let chat = app.state::<SessionChatState>();
    chat.index(&state.registry)
}

/// The importer, built on the first call and kept.
fn importer_in(app: &AppHandle) -> Option<Arc<Importer>> {
    let imports = app.state::<ChatImportState>();
    imports
        .importer
        .get_or_init(|| index_in(app).map(|index| Arc::new(Importer::new(Arc::new(RegistryCatalog), index, progress_emitter(app.clone())))))
        .clone()
}

/// The importer if one was built already. Never builds one (so a progress read never opens the index).
fn importer_built(app: &AppHandle) -> Option<Arc<Importer>> {
    app.state::<ChatImportState>().importer.get().cloned().flatten()
}

/// Sends the progress to the windows as `chat-import-progress` (the importer throttles it).
fn progress_emitter(app: AppHandle) -> ProgressEmitter {
    Arc::new(move |progress: &Progress| {
        let _ = app.emit(PROGRESS_EVENT, progress.clone());
    })
}

/// Waits (up to `STOP_WAIT`) while an import is still looking or reading, so a removal does not race a write.
fn wait_until_stopped(importer: &Importer) {
    let until = Instant::now() + STOP_WAIT;
    while matches!(importer.progress().phase, Phase::Looking | Phase::Reading) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The status when the index could not be opened (it holds nothing).
fn nothing_kept() -> Status {
    Status { enabled: chat_index::enabled(), items: 0, bytes: 0, old_copies: 0, imported_chats: 0, imported_items: 0 }
}

/// The old-chat counts of an index status: (chats, items).
fn counts_of(status: Status) -> (usize, usize) {
    (status.imported_chats, status.imported_items)
}

/// The choices file. Missing or damaged: the defaults.
fn read_prefs() -> ImportPrefs {
    match prefs_path() {
        Ok(path) => ImportPrefs::load_from(&path),
        Err(_) => ImportPrefs::default(),
    }
}

/// Changes the choices file under the lock and writes it back.
fn update_prefs(app: &AppHandle, change: impl FnOnce(&mut ImportPrefs)) -> Result<(), String> {
    let imports = app.state::<ChatImportState>();
    let _guard = imports.prefs_lock.lock();
    let path = prefs_path()?;
    let mut prefs = ImportPrefs::load_from(&path);
    change(&mut prefs);
    prefs.save_to(&path)
}

/// `<app data>/chat_import.json`, with the app data folder created if it is missing.
fn prefs_path() -> Result<std::path::PathBuf, String> {
    crate::app_paths::ensure_app_data_dir()?;
    Ok(ImportPrefs::default_path())
}

/// Every agent the screen lists: each one with a reader, and each installed one without a reader (greyed out).
/// Names come from the registry; nothing here names an agent.
fn agent_facts() -> Vec<AgentFact> {
    let registry = crate::tui::registry::registry();
    let code = registry.code_ids();
    registry
        .ordered()
        .iter()
        .filter_map(|profile| {
            let id = profile.id();
            let reader = profile.session_history().is_some();
            if !reader && !profile.is_installed() {
                return None;
            }
            Some(AgentFact {
                id: id.to_string(),
                label: label_of(id),
                from_definition: !code.iter().any(|known| known.eq_ignore_ascii_case(id)),
                reader,
            })
        })
        .collect()
}

/// The name the screen shows for an agent id: its registry label, or the id when there is none.
fn label_of(id: &str) -> String {
    crate::tui::descriptor_for(id).map(|descriptor| descriptor.label).unwrap_or_else(|| id.to_string())
}

/// The agents as the screen lists them. Only an agent with a reader can be on; the others are greyed out.
fn agent_rows(facts: &[AgentFact], prefs: &ImportPrefs) -> Vec<ImportAgentOut> {
    facts
        .iter()
        .map(|fact| ImportAgentOut {
            id: fact.id.clone(),
            label: fact.label.clone(),
            from_definition: fact.from_definition,
            enabled: fact.reader && prefs.enabled_for(&fact.id, fact.from_definition),
        })
        .collect()
}

/// The whole state object from its parts (pure, so the shape is tested without the app).
fn state_out(
    prefs: &ImportPrefs,
    facts: &[AgentFact],
    index_enabled: bool,
    progress: Progress,
    imported: (usize, usize),
) -> ImportStateOut {
    ImportStateOut {
        asked: prefs.asked,
        since_days: prefs.since_days,
        index_enabled,
        agents: agent_rows(facts, prefs),
        progress,
        imported: ImportCountsOut { chats: imported.0, items: imported.1 },
    }
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_millis() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat_index::import::{check_session, Source};
    use crate::session_chat::history::{ListLimits, Listing, ReadError, ReadLimits, ReadOutcome, SessionHistory};
    use crate::session_chat::ChatItem;

    /// A reader with nothing in it. Only gives a `Source` a history in these tests; nothing is read.
    struct NoChats;

    impl SessionHistory for NoChats {
        fn list(&self, _env: &HistoryEnv, _limits: &ListLimits) -> Listing {
            Listing::default()
        }

        fn read(
            &self,
            _env: &HistoryEnv,
            _session: &str,
            _limits: &ReadLimits,
            _sink: &mut dyn FnMut(Vec<ChatItem>) -> bool,
        ) -> Result<ReadOutcome, ReadError> {
            Err(ReadError::Missing)
        }

        fn resume_args(&self, _session: &str) -> Option<Vec<String>> {
            None
        }

        fn places(&self, _env: &HistoryEnv) -> Vec<String> {
            Vec::new()
        }
    }

    static NO_CHATS: NoChats = NoChats;

    fn source(id: &str) -> Source {
        Source { id: id.to_string(), label: id.to_string(), from_definition: false, history: &NO_CHATS }
    }

    fn fact(id: &str, from_definition: bool, reader: bool) -> AgentFact {
        AgentFact { id: id.to_string(), label: format!("{id} label"), from_definition, reader }
    }

    /// The object keys of a JSON object, sorted.
    fn keys(value: &serde_json::Value) -> Vec<String> {
        let mut keys: Vec<String> = value.as_object().map(|map| map.keys().cloned().collect()).unwrap_or_default();
        keys.sort();
        keys
    }

    #[test]
    fn state_has_the_screen_shape() {
        let facts = [fact("claude", false, true), fact("kilo", true, true), fact("cursor", false, false)];
        let state = state_out(&ImportPrefs::default(), &facts, true, Progress::idle(), (3, 40));
        let value = serde_json::to_value(&state).unwrap();
        assert_eq!(keys(&value), ["agents", "asked", "imported", "indexEnabled", "progress", "sinceDays"]);
        assert_eq!(value["asked"], false);
        assert_eq!(value["sinceDays"], 365);
        assert_eq!(value["indexEnabled"], true);
        assert_eq!(value["imported"], serde_json::json!({ "chats": 3, "items": 40 }));
        assert_eq!(value["progress"]["phase"], "idle");
        assert_eq!(
            keys(&value["progress"]),
            ["agent", "chatsDone", "chatsTotal", "items", "message", "phase", "tooBig", "unchanged", "unreadable"]
        );
        assert_eq!(keys(&value["agents"][0]), ["enabled", "fromDefinition", "id", "label"]);
    }

    #[test]
    fn agents_without_a_reader_are_listed_off() {
        let facts = [fact("claude", false, true), fact("kilo", true, true), fact("cursor", false, false)];
        let rows = agent_rows(&facts, &ImportPrefs::default());
        let on: Vec<(&str, bool)> = rows.iter().map(|row| (row.id.as_str(), row.enabled)).collect();
        // Built-in readers start on, a definition's reader starts off, an agent with no reader cannot be on.
        assert_eq!(on, [("claude", true), ("kilo", false), ("cursor", false)]);
        assert!(rows[1].from_definition);
        assert_eq!(rows[2].label, "cursor label");
    }

    #[test]
    fn the_saved_switch_wins_over_the_default() {
        let facts = [fact("claude", false, true), fact("kilo", true, true), fact("cursor", false, false)];
        let mut prefs = ImportPrefs::default();
        prefs.set_agent("claude", false);
        prefs.set_agent("kilo", true);
        prefs.set_agent("cursor", true);
        let on: Vec<bool> = agent_rows(&facts, &prefs).iter().map(|row| row.enabled).collect();
        assert_eq!(on, [false, true, false]);
    }

    #[test]
    fn unknown_agent_is_refused() {
        let sources = vec![source("claude"), source("codex")];
        assert_eq!(source_named(&sources, "codex").map(|found| found.id.clone()), Ok("codex".to_string()));
        assert_eq!(source_named(&sources, "nope").err().as_deref(), Some("UNKNOWN_AGENT"));
        assert_eq!(source_named(&[], "claude").err().as_deref(), Some("UNKNOWN_AGENT"));
    }

    #[test]
    fn invalid_session_is_refused() {
        assert_eq!(check_session("0f3c2a9e-1b2d-4c5e-8f60-7a8b9c0d1e2f"), Ok(()));
        assert_eq!(check_session("../../secrets").err().as_deref(), Some("INVALID_SESSION"));
        assert_eq!(check_session("abc & calc").err().as_deref(), Some("INVALID_SESSION"));
        assert_eq!(check_session("short").err().as_deref(), Some("INVALID_SESSION"));
    }
}
