//! The live world of "Assign an issue" (SPEC n32 section 3.6): the [`Io`] trait the engine talks
//! to, its real implementation, and the few functions the rest of the app calls.
//!
//! - [`start`]: the `issue-intake` thread (a look every 15 seconds, only while the Labs switch is on).
//! - [`on_resolved`]: a card was answered or dismissed (`questions::QuestionStore`). Maps the card to a
//!   saved run by its question id and does the work on its own thread, never inside the question lock.
//! - [`act`]: the Settings buttons (`start`, `ask_again`, `push`, `throw_away`, `unignore`).
//! - [`view`]: what the Settings page shows.
//!
//! The engine (`engine.rs`, `flows.rs`) holds all the rules and is tested with a fake [`Io`]. Nothing
//! here is a bridge route or an MCP tool: the settings and the answers come from the person only.

use super::engine::Engine;
use super::model::Config;
use super::store::Store;
use super::view::{IntakeView, RunView};
use crate::agent_runs::{AgentRunRequest, AgentRunView};
use crate::operations::{self, OperationError, OperationStatus};
use crate::questions::{NewQuestion, Question};
use crate::team::sources::github::{Gh, GhCli, GhResponse};
use crate::worktrees::pr::PrRef;
use crate::worktrees::{self, harden, Changes, CreateOptions, WorktreeRef};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;

type Registry = Arc<Mutex<crate::pty::PaneRegistry>>;

/// The Tauri event the Settings page and the Inbox listen to. The payload is not read.
pub const CHANGED_EVENT: &str = "issue-intake://changed";

/// What `run_agent` answered for a new run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub handle: String,
    pub operation_id: String,
    pub pane_id: Option<String>,
}

/// What the engine needs to know about a run's latest turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSnap {
    /// The turn is over (completed, failed or cancelled).
    pub terminal: bool,
    /// `completed`, `failed`, `cancelled` or `running`.
    pub status: String,
    pub pane_id: Option<String>,
    /// Turns used so far.
    pub turns: u32,
    /// Minutes used, rounded up.
    pub minutes: u32,
}

/// `gh repo view` for a folder: the repo it really is and its default branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoFacts {
    pub name_with_owner: String,
    pub default_branch: Option<String>,
}

/// Everything the engine does outside its own memory. The real one is [`LiveIo`]; the tests have a
/// fake with a virtual clock, so no test touches GitHub, git, an agent or the app's data folder.
pub trait Io: Send + Sync {
    fn now_ms(&self) -> u64;
    fn store(&self) -> &Store;
    /// Run `job` on its own thread (the fake runs it right here).
    fn spawn(&self, name: &str, job: Box<dyn FnOnce() + Send>);
    /// Tell the windows that a run or the settings changed.
    fn changed(&self);
    /// One audit line (`event` is `asked`, `auto_approved`, `started`, `pr_opened`, `held`, `thrown_away`).
    fn audit(&self, event: &str, who: &str, summary: &str, detail: Value);

    // GitHub, through the person's own `gh` login.
    fn gh_state(&self) -> String;
    fn gh_api(&self, path: &str, etag: Option<&str>) -> Result<GhResponse, String>;
    fn repo_facts(&self, dir: &Path) -> Result<RepoFacts, String>;
    /// The `owner/repo` the folder's `origin` points at.
    fn folder_owner_repo(&self, folder: &str) -> Option<String>;

    // Agents.
    fn agent_ready(&self, agent_type: &str) -> bool;
    fn agent_label(&self, agent_type: &str) -> String;
    fn run_agent(&self, request: AgentRunRequest) -> Result<Started, OperationError>;
    fn run_snapshot(&self, project: &str, handle: &str) -> Option<RunSnap>;
    /// The agent is still working in the copy (a run, or a pane that is busy).
    fn copy_busy(&self, project: &str, copy: &WorktreeRef) -> bool;
    /// The plain sentence the app wrote about the pane's last turn.
    fn turn_sentence(&self, pane_id: &str) -> Option<String>;

    // Git and the copies. `env` is `harden::git_env`.
    fn git_env(&self) -> Result<Vec<(String, String)>, OperationError>;
    fn fetch_branch(&self, folder: &str, branch: &str, env: &[(String, String)]) -> Result<String, OperationError>;
    fn create_copy(&self, folder: &str, handle: &str, start: &str, env: &[(String, String)]) -> Result<WorktreeRef, OperationError>;
    fn find_copy(&self, folder: &str, handle: &str) -> Option<WorktreeRef>;
    fn write_issue_file(&self, copy: &WorktreeRef, number: u64, text: &str) -> Result<(), String>;
    fn fingerprint(&self, copy: &WorktreeRef) -> Result<String, OperationError>;
    fn commit_pending(&self, copy: &WorktreeRef, env: &[(String, String)]) -> Result<bool, OperationError>;
    fn changes(&self, copy: &WorktreeRef, env: &[(String, String)]) -> Result<Changes, OperationError>;
    fn changed_files(&self, copy: &WorktreeRef, env: &[(String, String)]) -> Result<Vec<String>, OperationError>;
    fn push_diff(&self, copy: &WorktreeRef, env: &[(String, String)]) -> Result<super::pushcheck::DiffText, OperationError>;
    /// The `+` lines of a patch that look like a secret, and those with the home folder: `(path, line)`.
    fn scan_added(&self, diff: &str) -> (Vec<(String, u32)>, Vec<(String, u32)>);
    fn remote_has_branch(&self, copy: &WorktreeRef, env: &[(String, String)]) -> Result<bool, OperationError>;
    #[allow(clippy::too_many_arguments)]
    fn pr_create(&self, folder: &str, handle: &str, title: &str, body: &str, base: &str, env: &[(String, String)]) -> Result<PrRef, OperationError>;
    fn discard(&self, folder: &str, handle: &str) -> Result<(), OperationError>;

    // Cards.
    fn ask(&self, new: NewQuestion) -> Result<String, String>;
    /// Like `ask`, under an id the caller already stored in a run record.
    fn ask_as(&self, new: NewQuestion, id: &str) -> Result<String, String>;
    fn question(&self, id: &str) -> Option<Question>;
    fn dismiss(&self, id: &str);
}

// ----------------------------------------------------------------------------------- the real io

/// The real world: the person's `gh`, git, the app's agents, the question store and the audit log.
pub struct LiveIo {
    store: Store,
    gh: Arc<dyn Gh>,
    registry: Registry,
    app: tauri::AppHandle,
}

impl LiveIo {
    pub fn new(store: Store, registry: Registry, app: tauri::AppHandle) -> Self {
        Self { store, gh: Arc::new(GhCli), registry, app }
    }
}

fn plain(err: impl std::fmt::Display) -> String {
    err.to_string()
}

impl Io for LiveIo {
    fn now_ms(&self) -> u64 {
        crate::questions::now_ms()
    }

    fn store(&self) -> &Store {
        &self.store
    }

    fn spawn(&self, name: &str, job: Box<dyn FnOnce() + Send>) {
        if let Err(error) = std::thread::Builder::new().name(name.to_string()).spawn(job) {
            tracing::warn!(%error, name, "issue-intake: could not start a thread");
        }
    }

    fn changed(&self) {
        use tauri::Emitter;
        let _ = self.app.emit(CHANGED_EVENT, ());
    }

    fn audit(&self, event: &str, who: &str, summary: &str, detail: Value) {
        let who = match who {
            "desktop" => crate::audit::Who::Desktop,
            "phone" => crate::audit::Who::Device(None),
            _ => crate::audit::Who::Local,
        };
        let mut detail = detail;
        if let Some(map) = detail.as_object_mut() {
            map.insert("event".to_string(), Value::from(event));
        }
        crate::audit::record(crate::audit::Kind::IssueIntake, who, summary, detail);
    }

    fn gh_state(&self) -> String {
        worktrees::pr::gh_state(&std::env::temp_dir(), &worktrees::proc::ToolEnv::default())
    }

    fn gh_api(&self, path: &str, etag: Option<&str>) -> Result<GhResponse, String> {
        self.gh.api(path, etag)
    }

    fn repo_facts(&self, dir: &Path) -> Result<RepoFacts, String> {
        let ran = worktrees::proc::run_str(
            "gh",
            &["repo", "view", "--json", "nameWithOwner,defaultBranchRef"],
            dir,
            &worktrees::proc::ToolEnv::default(),
            std::time::Duration::from_secs(30),
        )?;
        if !ran.ok() {
            return Err("gh could not read this repository".to_string());
        }
        parse_repo_facts(&ran.stdout).ok_or_else(|| "gh gave no repository name".to_string())
    }

    fn folder_owner_repo(&self, folder: &str) -> Option<String> {
        let key = crate::team::project_key::project_key_of(Path::new(folder))?;
        crate::team::sources::github::owner_repo(&key)
    }

    fn agent_ready(&self, agent_type: &str) -> bool {
        crate::crews::live::agent_state(agent_type) == crate::crews::validate::AgentState::Ready
    }

    fn agent_label(&self, agent_type: &str) -> String {
        crate::crews::live::label_of(agent_type)
    }

    fn run_agent(&self, request: AgentRunRequest) -> Result<Started, OperationError> {
        let view: AgentRunView = crate::agent_runs::run_agent(request, self.registry.clone(), self.app.clone(), None)?;
        Ok(Started { handle: view.handle, operation_id: view.operation_id, pane_id: view.pane_id })
    }

    fn run_snapshot(&self, project: &str, handle: &str) -> Option<RunSnap> {
        let op = operations::latest_agent_run(project, handle).ok()?;
        let status = match op.status {
            OperationStatus::Completed => "completed",
            OperationStatus::Failed => "failed",
            OperationStatus::Cancelled => "cancelled",
            _ => "running",
        };
        let started = op.started_at_ms.unwrap_or(op.created_at_ms);
        let ended = op.finished_at_ms.unwrap_or_else(|| self.now_ms());
        let minutes = u32::try_from(ended.saturating_sub(started).div_ceil(60_000)).unwrap_or(u32::MAX);
        Some(RunSnap {
            terminal: op.status.terminal(),
            status: status.to_string(),
            pane_id: op.pane_id.clone(),
            turns: op.turn_index.saturating_add(1),
            minutes,
        })
    }

    fn copy_busy(&self, project: &str, copy: &WorktreeRef) -> bool {
        if operations::handle_is_active(project, &copy.handle) {
            return true;
        }
        let root = Path::new(&copy.path);
        self.registry
            .lock()
            .list()
            .iter()
            .any(|pane| pane.status == "running" && crate::project_path::workspace_covers(root, Path::new(&pane.cwd)))
    }

    fn turn_sentence(&self, pane_id: &str) -> Option<String> {
        let last = crate::turn_summary::tracker().summaries(pane_id).into_iter().last()?;
        Some(last.sentence).filter(|sentence| !sentence.trim().is_empty())
    }

    fn git_env(&self) -> Result<Vec<(String, String)>, OperationError> {
        let hooks = self.store.empty_hooks_dir().map_err(|message| OperationError::new("GIT_FAILED", message, false))?;
        Ok(harden::git_env(&hooks))
    }

    fn fetch_branch(&self, folder: &str, branch: &str, env: &[(String, String)]) -> Result<String, OperationError> {
        harden::fetch_branch(folder, "origin", branch, env)
    }

    fn create_copy(&self, folder: &str, handle: &str, start: &str, env: &[(String, String)]) -> Result<WorktreeRef, OperationError> {
        let options = CreateOptions { start_point: Some(start.to_string()), copy_files: false, git_env: env.to_vec() };
        worktrees::create_with(folder, handle, &options)
    }

    fn find_copy(&self, folder: &str, handle: &str) -> Option<WorktreeRef> {
        worktrees::find_by_handle(folder, handle).filter(|copy| copy.closed.is_none())
    }

    fn write_issue_file(&self, copy: &WorktreeRef, number: u64, text: &str) -> Result<(), String> {
        let dir = Path::new(&copy.cwd).join(".puppet-master");
        std::fs::create_dir_all(&dir).map_err(|error| format!("could not make {}: {error}", dir.display()))?;
        crate::secure_file::write_private(&dir.join(format!("issue-{number}.md")), text)
    }

    fn fingerprint(&self, copy: &WorktreeRef) -> Result<String, OperationError> {
        Ok(super::pushcheck::fingerprint_of(&harden::config_snapshot(Path::new(&copy.path))?))
    }

    fn commit_pending(&self, copy: &WorktreeRef, env: &[(String, String)]) -> Result<bool, OperationError> {
        worktrees::commit_pending_with(copy, env)
    }

    fn changes(&self, copy: &WorktreeRef, env: &[(String, String)]) -> Result<Changes, OperationError> {
        worktrees::changes_with(copy, env)
    }

    fn changed_files(&self, copy: &WorktreeRef, env: &[(String, String)]) -> Result<Vec<String>, OperationError> {
        super::pushcheck::changed_files(Path::new(&copy.path), &copy.base_commit, env)
    }

    fn push_diff(&self, copy: &WorktreeRef, env: &[(String, String)]) -> Result<super::pushcheck::DiffText, OperationError> {
        super::pushcheck::push_diff(Path::new(&copy.path), &copy.base_commit, env)
    }

    fn scan_added(&self, diff: &str) -> (Vec<(String, u32)>, Vec<(String, u32)>) {
        let ctx = crate::redact::known::context();
        let hit = |line: &str| !crate::redact::rules::find_spans(line, &ctx, crate::redact::rules::Mode::Secrets).is_empty();
        let home = crate::project_path::home_dir();
        super::pushcheck::scan_added(diff, &hit, home.as_deref().and_then(|path| path.to_str()))
    }

    fn remote_has_branch(&self, copy: &WorktreeRef, env: &[(String, String)]) -> Result<bool, OperationError> {
        harden::remote_has_branch(Path::new(&copy.path), "origin", &copy.branch, env)
    }

    fn pr_create(&self, folder: &str, handle: &str, title: &str, body: &str, base: &str, env: &[(String, String)]) -> Result<PrRef, OperationError> {
        worktrees::service::pr_create_with(
            Some(folder),
            handle,
            title,
            body,
            base,
            &self.registry,
            &worktrees::proc::ToolEnv::default(),
            env,
        )
    }

    fn discard(&self, folder: &str, handle: &str) -> Result<(), OperationError> {
        worktrees::service::discard(Some(folder), handle, &self.registry)
    }

    fn ask(&self, new: NewQuestion) -> Result<String, String> {
        crate::questions::store().create(new, crate::questions::now_ms()).map(|question| question.id).map_err(|error| plain(error.message))
    }

    fn ask_as(&self, new: NewQuestion, id: &str) -> Result<String, String> {
        crate::questions::store().create_as(new, Some(id), crate::questions::now_ms()).map(|question| question.id).map_err(|error| plain(error.message))
    }

    fn question(&self, id: &str) -> Option<Question> {
        crate::questions::store().get(id, crate::questions::now_ms())
    }

    fn dismiss(&self, id: &str) {
        let _ = crate::questions::store().dismiss(id, crate::questions::now_ms());
    }
}

/// `gh repo view --json nameWithOwner,defaultBranchRef` output.
pub fn parse_repo_facts(json: &str) -> Option<RepoFacts> {
    let value: Value = serde_json::from_str(json).ok()?;
    let name = value.get("nameWithOwner")?.as_str()?.trim().to_string();
    let default_branch = value
        .pointer("/defaultBranchRef/name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|branch| !branch.is_empty())
        .map(str::to_string);
    (!name.is_empty()).then_some(RepoFacts { name_with_owner: name, default_branch })
}

// ------------------------------------------------------------------------------- the one engine

static ENGINE: OnceCell<Arc<Engine<LiveIo>>> = OnceCell::new();

/// The app's own store: `<app data>/issue-intake`. Tests use a folder of their own.
#[cfg(not(test))]
fn live_store() -> Store {
    Store::live()
}

#[cfg(test)]
fn live_store() -> Store {
    Store::at(std::env::temp_dir().join(format!("pm-issue-intake-live-{}", std::process::id())))
}

/// The store the Settings commands read and write.
pub fn store() -> Store {
    match ENGINE.get() {
        Some(engine) => engine.io().store().clone(),
        None => live_store(),
    }
}

/// Start the `issue-intake` thread (one line in `lib.rs` setup). It looks at GitHub every 15 seconds,
/// and only while the Labs switch is on. A second call does nothing.
pub fn start(registry: Registry, app: tauri::AppHandle) {
    let engine = Arc::new(Engine::new(LiveIo::new(live_store(), registry, app)));
    if ENGINE.set(engine.clone()).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new().name("issue-intake".into()).spawn(move || engine.run_loop());
    if let Err(error) = spawned {
        tracing::warn!(%error, "issue-intake: could not start the watcher");
    }
}

/// A card was answered or dismissed. Cards that are not ours are ignored at once; ours are mapped to
/// a saved run by their question id (never by kind or text) and handled on a thread of their own.
pub fn on_resolved(question: &Question) {
    if !super::engine::is_issue_card(question) {
        return;
    }
    // Unit tests never start the live engine (it would link the whole window stack into the test
    // program); the engine tests call `Engine::on_resolved` with a fake world.
    #[cfg(not(test))]
    if let Some(engine) = ENGINE.get() {
        engine.on_resolved(question);
    }
}

fn not_ready() -> OperationError {
    OperationError::new("APP_NOT_READY", "Puppet Master is still starting", true)
}

/// One of the Settings buttons on a run: `start`, `ask_again`, `push`, `throw_away` or `unignore`.
/// `who` is `desktop` or `phone`. Returns the run as the list shows it.
pub fn act(run_id: &str, action: &str, who: &str) -> Result<RunView, OperationError> {
    ENGINE.get().ok_or_else(not_ready)?.act(run_id, action, who)
}

/// What the Settings page shows: the config, the GitHub status, the repo rows and the recent runs.
pub fn view() -> IntakeView {
    match ENGINE.get() {
        Some(engine) => engine.view(),
        None => super::answers::offline_view(&live_store()),
    }
}

/// Look at GitHub now instead of waiting for the next minute.
pub fn poll_now() {
    if let Some(engine) = ENGINE.get() {
        engine.poll_now();
    }
}

/// Tell the windows that the settings changed (the save command calls this after it wrote).
pub fn notify_changed() {
    if let Some(engine) = ENGINE.get() {
        engine.io().changed();
    }
}

/// The saved settings (for the commands).
pub fn config() -> Config {
    store().config()
}
