//! The live world of "Red to Green" (SPEC n33 section 3.5): the [`Io`] trait the engine talks to, its
//! real implementation, and the few functions the rest of the app calls.
//!
//! - [`start`]: the `ci-fix` thread (a look every 20 seconds, only while the Labs switch is on).
//! - [`on_pr_opened`]: a pull request was opened from a Puppet Master copy (`worktrees::service`). Starts
//!   a watch when the Labs switch is on. Never fails the pull request.
//! - [`on_resolved`]: a card was answered or dismissed (`questions::QuestionStore`). Maps the card to a
//!   saved fix by its question id and does the work on its own thread, never inside the question lock.
//! - [`offer`], [`act`], [`view`], [`log`], [`poll_now`], [`save_settings`]: what the desktop commands call.
//!
//! The engine (`engine.rs`, `flows.rs`, `answers.rs`) holds all the rules and is tested with a fake
//! [`Io`]. Nothing here is a bridge route or an MCP tool: the settings and the answers come from the
//! person only. Nothing here writes to a pane: a fix reaches an agent as a follow-up turn or a new run.

use super::engine::Engine;
use super::model::Settings;
use super::store::Store;
use super::view::{CiFixView, FixLogView, WatchView};
use crate::agent_runs::{AgentRunRequest, AgentRunView};
use crate::issue_intake::live::{parse_repo_facts, RepoFacts};
use crate::issue_intake::model::AgentPick;
use crate::issue_intake::pushcheck::{self, DiffText};
use crate::operations::{self, OperationError, OperationStatus};
use crate::questions::{NewQuestion, Question};
use crate::team::sources::github::{Gh, GhCli, GhResponse};
use crate::worktrees::{self, harden, WorktreeRef};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use serde_json::Value;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

type Registry = Arc<Mutex<crate::pty::PaneRegistry>>;

/// The Tauri event the Settings page, the PR block and the Inbox listen to. The payload is not read.
pub const CHANGED_EVENT: &str = "ci-fix://changed";

/// How long one job log read may take.
const LOG_READ_LIMIT: Duration = Duration::from_secs(90);

/// What `run_agent` or `send_agent` answered for a turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub handle: String,
    pub operation_id: String,
    pub pane_id: Option<String>,
}

/// What the engine needs to know about a run's latest turn and its pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunFacts {
    /// The latest turn is over (completed, failed or cancelled).
    pub terminal: bool,
    /// `completed`, `failed`, `cancelled` or `running`.
    pub status: String,
    pub pane_id: Option<String>,
    /// The run's pane is still open and inside the copy.
    pub pane_open: bool,
    /// The run started under the `no_push` guard (the agent cannot push).
    pub guarded: bool,
    /// Keep going moved the task to another agent.
    pub handed_off: bool,
    pub agent_type: String,
    pub account: Option<String>,
    /// Turns used so far.
    pub turns: u32,
    /// Minutes used, rounded up.
    pub minutes: u32,
}

/// Everything the engine does outside its own memory. The real one is [`LiveIo`]; the tests have a
/// fake with a virtual clock, so no test touches GitHub, git, an agent or the app's data folder.
pub trait Io: Send + Sync {
    fn now_ms(&self) -> u64;
    fn store(&self) -> &Store;
    /// Run `job` on its own thread (the fake runs it right here).
    fn spawn(&self, name: &str, job: Box<dyn FnOnce() + Send>);
    /// Tell the windows that a watch or a fix changed.
    fn changed(&self);
    /// One audit line (`event` is `watched`, `paused`, `asked`, `started`, `asked_push`, `held`, `pushed`,
    /// `green`, `not_pushed`, `stopped` or `settings`).
    fn audit(&self, event: &str, who: &str, summary: &str, detail: Value);

    // GitHub, through the person's own `gh` login. Reads only.
    fn gh_state(&self) -> String;
    fn gh_api(&self, path: &str, etag: Option<&str>) -> Result<GhResponse, String>;
    /// The raw log of one GitHub Actions job: at most 8 MiB, the end of it kept.
    fn job_log(&self, path: &str) -> Result<String, String>;
    fn repo_facts(&self, dir: &Path) -> Result<RepoFacts, String>;
    /// The `owner/repo` the folder's `origin` points at.
    fn folder_owner_repo(&self, folder: &str) -> Option<String>;

    // Git and the copies. `env` is `harden::git_env`.
    fn find_copy(&self, project: &str, handle: &str) -> Option<WorktreeRef>;
    /// The copy and the project it was found under: `project` when given, else every known one.
    fn find_copy_any(&self, project: Option<&str>, handle: &str) -> Option<(String, WorktreeRef)>;
    fn git_env(&self) -> Result<Vec<(String, String)>, OperationError>;
    fn head_sha(&self, copy: &WorktreeRef, env: &[(String, String)]) -> Result<String, OperationError>;
    fn current_branch(&self, copy: &WorktreeRef, env: &[(String, String)]) -> Result<Option<String>, OperationError>;
    fn is_ancestor(&self, copy: &WorktreeRef, a: &str, b: &str, env: &[(String, String)]) -> Result<bool, OperationError>;
    fn fingerprint(&self, copy: &WorktreeRef) -> Result<String, OperationError>;
    fn commit_pending(&self, copy: &WorktreeRef, env: &[(String, String)]) -> Result<bool, OperationError>;
    fn changed_files(&self, copy: &WorktreeRef, base: &str, env: &[(String, String)]) -> Result<Vec<String>, OperationError>;
    fn name_status(&self, copy: &WorktreeRef, base: &str, env: &[(String, String)]) -> Result<Vec<(String, String)>, OperationError>;
    fn push_diff(&self, copy: &WorktreeRef, base: &str, env: &[(String, String)]) -> Result<DiffText, OperationError>;
    fn commit_count(&self, copy: &WorktreeRef, base: &str, env: &[(String, String)]) -> Result<u32, OperationError>;
    /// The `+` lines of a patch that look like a secret, and those with the home folder: `(path, line)`.
    fn scan_added(&self, diff: &str) -> (Vec<(String, u32)>, Vec<(String, u32)>);
    /// The commit the copy's branch points to on the remote, or `None` when the remote has no such branch.
    fn remote_branch_sha(&self, copy: &WorktreeRef, env: &[(String, String)]) -> Result<Option<String>, OperationError>;
    /// `worktrees::service::pr_push_fix`: commit loose edits, then a fast-forward push of the copy's `pm/` branch.
    fn push_fix(
        &self,
        project: &str,
        handle: &str,
        expected_remote: &str,
        default_branch: Option<&str>,
        env: &[(String, String)],
    ) -> Result<String, OperationError>;
    /// Write `.puppet-master/<name>` into the copy (a folder that is a link is refused).
    fn write_log_file(&self, copy: &WorktreeRef, name: &str, text: &str) -> Result<(), String>;

    // Agents.
    fn agent_ready(&self, agent_type: &str) -> bool;
    fn agent_label(&self, agent_type: &str) -> String;
    /// The agent and account of the newest run in the copy.
    fn agent_of_copy(&self, project: &str, handle: &str) -> Option<AgentPick>;
    /// An issue run (Assign an issue) made this copy.
    fn made_by_issue_run(&self, handle: &str) -> bool;
    fn run_snapshot(&self, project: &str, handle: &str) -> Option<RunFacts>;
    /// The agent is still working in the copy (a run, or a pane that is busy).
    fn copy_busy(&self, project: &str, copy: &WorktreeRef) -> bool;
    /// The plain sentence the app wrote about the pane's last turn.
    fn turn_sentence(&self, pane_id: &str) -> Option<String>;
    /// Route A: the next turn of the same conversation (`agent_runs::send_agent`, in the background).
    fn followup(&self, project: &str, handle: &str, task: String, key: &str) -> Result<Started, OperationError>;
    /// Route B: a new run in the copy.
    fn run_agent(&self, request: AgentRunRequest) -> Result<Started, OperationError>;

    // Cards. `fix_id` names the fix a Red to Green card is about (never text).
    fn ask(&self, new: NewQuestion, fix_id: Option<&str>) -> Result<String, String>;
    /// Like `ask`, under an id the caller already stored in a fix record.
    fn ask_as(&self, new: NewQuestion, id: &str, fix_id: Option<&str>) -> Result<String, String>;
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

    /// The ids of the panes whose folder is inside the copy.
    fn panes_in(&self, copy: &WorktreeRef) -> Vec<String> {
        self.registry
            .lock()
            .list()
            .into_iter()
            .filter(|pane| crate::project_path::workspace_covers(Path::new(&copy.path), Path::new(&pane.cwd)))
            .map(|pane| pane.id)
            .collect()
    }
}

fn plain(err: impl std::fmt::Display) -> String {
    err.to_string()
}

/// Read a child's output keeping only the last `max` bytes (the end of a log is what matters). The
/// buffer never holds more than `2 * max`.
fn read_tail(mut source: impl Read, max: usize) -> std::io::Result<Vec<u8>> {
    let mut kept: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = source.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        kept.extend_from_slice(&chunk[..n]);
        if kept.len() > max * 2 {
            let drop_to = kept.len() - max;
            kept.drain(..drop_to);
        }
    }
    if kept.len() > max {
        let drop_to = kept.len() - max;
        kept.drain(..drop_to);
    }
    Ok(kept)
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
            tracing::warn!(%error, name, "ci-fix: could not start a thread");
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
        crate::audit::record(crate::audit::Kind::CiFix, who, summary, detail);
    }

    fn gh_state(&self) -> String {
        worktrees::pr::gh_state(&std::env::temp_dir(), &worktrees::proc::ToolEnv::default())
    }

    fn gh_api(&self, path: &str, etag: Option<&str>) -> Result<GhResponse, String> {
        self.gh.api(path, etag)
    }

    fn job_log(&self, path: &str) -> Result<String, String> {
        // A plain GET of the job's log (GitHub answers with a redirect to the text, which `gh` follows).
        // The read is capped: only the last 8 MiB are kept, and it is cut off after a minute and a half.
        let mut command = crate::team::sources::github::gh_command();
        command.args(["api", path]);
        let mut child = command.spawn().map_err(|error| format!("gh isn't available: {error}"))?;
        let stdout = child.stdout.take().ok_or_else(|| "gh gave no output".to_string())?;
        let (sender, receiver) = std::sync::mpsc::channel();
        let reader = std::thread::Builder::new().name("ci-fix-log".into()).spawn(move || {
            let _ = sender.send(read_tail(stdout, super::model::LOG_RAW_MAX_BYTES));
        });
        if reader.is_err() {
            let _ = child.kill();
            return Err("could not read the log".to_string());
        }
        match receiver.recv_timeout(LOG_READ_LIMIT) {
            Ok(Ok(bytes)) => {
                let status = child.wait().map_err(plain)?;
                if status.success() {
                    Ok(String::from_utf8_lossy(&bytes).into_owned())
                } else {
                    Err("GitHub gave no log for this job".to_string())
                }
            }
            Ok(Err(error)) => {
                let _ = child.kill();
                Err(format!("could not read the log: {error}"))
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                Err("reading the log took too long".to_string())
            }
        }
    }

    fn repo_facts(&self, dir: &Path) -> Result<RepoFacts, String> {
        let ran = worktrees::proc::run_str(
            "gh",
            &["repo", "view", "--json", "nameWithOwner,defaultBranchRef"],
            dir,
            &worktrees::proc::ToolEnv::default(),
            Duration::from_secs(30),
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

    fn find_copy(&self, project: &str, handle: &str) -> Option<WorktreeRef> {
        worktrees::find_by_handle(project, handle).filter(|copy| copy.closed.is_none())
    }

    fn find_copy_any(&self, project: Option<&str>, handle: &str) -> Option<(String, WorktreeRef)> {
        if let Some(project) = project {
            return self.find_copy(project, handle).map(|copy| (project.to_string(), copy));
        }
        worktrees::service::list(None, &self.registry)
            .into_iter()
            .map(|item| item.entry.worktree)
            .find(|copy| copy.handle == handle && copy.closed.is_none())
            .map(|copy| (copy.repo.clone(), copy))
    }

    fn git_env(&self) -> Result<Vec<(String, String)>, OperationError> {
        let hooks = crate::issue_intake::live::store()
            .empty_hooks_dir()
            .map_err(|message| OperationError::new("GIT_FAILED", message, false))?;
        Ok(harden::git_env(&hooks))
    }

    fn head_sha(&self, copy: &WorktreeRef, env: &[(String, String)]) -> Result<String, OperationError> {
        harden::head_sha(Path::new(&copy.path), env)
    }

    fn current_branch(&self, copy: &WorktreeRef, env: &[(String, String)]) -> Result<Option<String>, OperationError> {
        harden::current_branch(Path::new(&copy.path), env)
    }

    fn is_ancestor(&self, copy: &WorktreeRef, a: &str, b: &str, env: &[(String, String)]) -> Result<bool, OperationError> {
        harden::is_ancestor(Path::new(&copy.path), a, b, env)
    }

    fn fingerprint(&self, copy: &WorktreeRef) -> Result<String, OperationError> {
        Ok(pushcheck::fingerprint_of(&harden::config_snapshot(Path::new(&copy.path))?))
    }

    fn commit_pending(&self, copy: &WorktreeRef, env: &[(String, String)]) -> Result<bool, OperationError> {
        worktrees::commit_pending_with(copy, env)
    }

    fn changed_files(&self, copy: &WorktreeRef, base: &str, env: &[(String, String)]) -> Result<Vec<String>, OperationError> {
        pushcheck::changed_files(Path::new(&copy.path), base, env)
    }

    fn name_status(&self, copy: &WorktreeRef, base: &str, env: &[(String, String)]) -> Result<Vec<(String, String)>, OperationError> {
        harden::name_status(Path::new(&copy.path), base, env)
    }

    fn push_diff(&self, copy: &WorktreeRef, base: &str, env: &[(String, String)]) -> Result<DiffText, OperationError> {
        pushcheck::push_diff(Path::new(&copy.path), base, env)
    }

    fn commit_count(&self, copy: &WorktreeRef, base: &str, env: &[(String, String)]) -> Result<u32, OperationError> {
        harden::commit_count(Path::new(&copy.path), base, env)
    }

    fn scan_added(&self, diff: &str) -> (Vec<(String, u32)>, Vec<(String, u32)>) {
        let ctx = crate::redact::known::context();
        let hit = |line: &str| !crate::redact::rules::find_spans(line, &ctx, crate::redact::rules::Mode::Secrets).is_empty();
        let home = crate::project_path::home_dir();
        pushcheck::scan_added(diff, &hit, home.as_deref().and_then(|path| path.to_str()))
    }

    fn remote_branch_sha(&self, copy: &WorktreeRef, env: &[(String, String)]) -> Result<Option<String>, OperationError> {
        harden::remote_branch_sha(Path::new(&copy.path), "origin", &copy.branch, env)
    }

    fn push_fix(
        &self,
        project: &str,
        handle: &str,
        expected_remote: &str,
        default_branch: Option<&str>,
        env: &[(String, String)],
    ) -> Result<String, OperationError> {
        worktrees::service::pr_push_fix(
            Some(project),
            handle,
            expected_remote,
            default_branch,
            &self.registry,
            &worktrees::proc::ToolEnv::default(),
            env,
        )
    }

    fn write_log_file(&self, copy: &WorktreeRef, name: &str, text: &str) -> Result<(), String> {
        let dir = Path::new(&copy.cwd).join(".puppet-master");
        if std::fs::symlink_metadata(&dir).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err("the .puppet-master folder in this copy is a link, so the log was not written there".to_string());
        }
        std::fs::create_dir_all(&dir).map_err(|error| format!("could not make {}: {error}", dir.display()))?;
        crate::secure_file::write_private(&dir.join(name), text)
    }

    fn agent_ready(&self, agent_type: &str) -> bool {
        crate::crews::live::agent_state(agent_type) == crate::crews::validate::AgentState::Ready
    }

    fn agent_label(&self, agent_type: &str) -> String {
        crate::crews::live::label_of(agent_type)
    }

    fn agent_of_copy(&self, project: &str, handle: &str) -> Option<AgentPick> {
        let op = operations::latest_agent_run(project, handle).ok()?;
        let pick = AgentPick { agent_type: op.agent_type.clone(), account: op.worker.account.clone() };
        AgentPick::parse(&pick.key())
    }

    fn made_by_issue_run(&self, handle: &str) -> bool {
        crate::issue_intake::live::store().runs().iter().any(|run| run.handle.as_deref() == Some(handle))
    }

    fn run_snapshot(&self, project: &str, handle: &str) -> Option<RunFacts> {
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
        let in_copy = self.find_copy(project, handle).map(|copy| self.panes_in(&copy)).unwrap_or_default();
        // The same safe check "Send back" makes: only a pane of the newest run, inside the copy, under the guard.
        let guarded = crate::worktrees::sendback::pane_if_guarded(
            op.worker.guard_env.as_deref(),
            op.pane_id.as_deref(),
            &in_copy,
        )
        .is_ok();
        let pane_open = op.pane_id.as_ref().is_some_and(|id| in_copy.iter().any(|pane| pane == id));
        let handed_off = op.error.as_ref().is_some_and(|error| error.code == operations::HANDED_OFF);
        Some(RunFacts {
            terminal: op.status.terminal(),
            status: status.to_string(),
            pane_id: op.pane_id.clone(),
            pane_open,
            guarded,
            handed_off,
            agent_type: op.agent_type.clone(),
            account: op.worker.account.clone(),
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

    fn followup(&self, project: &str, handle: &str, task: String, key: &str) -> Result<Started, OperationError> {
        let opts = crate::agent_runs::persist::FollowupOpts {
            background: true,
            idempotency_key: Some(key.to_string()),
            ..Default::default()
        };
        let view: AgentRunView =
            crate::agent_runs::send_agent(project, handle, task, None, self.registry.clone(), self.app.clone(), opts)?;
        Ok(Started { handle: view.handle, operation_id: view.operation_id, pane_id: view.pane_id })
    }

    fn run_agent(&self, request: AgentRunRequest) -> Result<Started, OperationError> {
        let view: AgentRunView = crate::agent_runs::run_agent(request, self.registry.clone(), self.app.clone(), None)?;
        Ok(Started { handle: view.handle, operation_id: view.operation_id, pane_id: view.pane_id })
    }

    fn ask(&self, new: NewQuestion, fix_id: Option<&str>) -> Result<String, String> {
        crate::questions::store()
            .create_for_fix(new, None, fix_id, crate::questions::now_ms())
            .map(|question| question.id)
            .map_err(|error| plain(error.message))
    }

    fn ask_as(&self, new: NewQuestion, id: &str, fix_id: Option<&str>) -> Result<String, String> {
        crate::questions::store()
            .create_for_fix(new, Some(id), fix_id, crate::questions::now_ms())
            .map(|question| question.id)
            .map_err(|error| plain(error.message))
    }

    fn question(&self, id: &str) -> Option<Question> {
        crate::questions::store().get(id, crate::questions::now_ms())
    }

    fn dismiss(&self, id: &str) {
        let _ = crate::questions::store().dismiss(id, crate::questions::now_ms());
    }
}

// ------------------------------------------------------------------------------- the one engine

static ENGINE: OnceCell<Arc<Engine<LiveIo>>> = OnceCell::new();

/// The app's own store: `<app data>/ci-fix`. Tests use a folder of their own.
#[cfg(not(test))]
fn live_store() -> Store {
    Store::live()
}

#[cfg(test)]
fn live_store() -> Store {
    Store::at(std::env::temp_dir().join(format!("pm-ci-fix-live-{}", std::process::id())))
}

/// The store the commands read and write.
// Reason: only the dev-fake-tui rig reads this (`fake_tui/ci_fix.rs`). Normal builds do not compile it.
#[allow(dead_code)]
pub fn store() -> Store {
    match ENGINE.get() {
        Some(engine) => engine.io().store().clone(),
        None => live_store(),
    }
}

/// Start the `ci-fix` thread (one line in `lib.rs` setup). It looks at GitHub every 20 seconds, and only
/// while the Labs switch is on. A second call does nothing.
pub fn start(registry: Registry, app: tauri::AppHandle) {
    let engine = Arc::new(Engine::new(LiveIo::new(live_store(), registry, app)));
    if ENGINE.set(engine.clone()).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new().name("ci-fix".into()).spawn(move || engine.run_loop());
    if let Err(error) = spawned {
        tracing::warn!(%error, "ci-fix: could not start the watcher");
    }
}

/// A pull request was opened from a Puppet Master copy. Watches it when the Labs switch is on. Never fails
/// the pull request: every problem is only logged.
pub fn on_pr_opened(project: &str, copy: &WorktreeRef) {
    #[cfg(not(test))]
    if let Some(engine) = ENGINE.get() {
        engine.on_pr_opened(project, copy);
    }
    #[cfg(test)]
    let _ = (project, copy);
}

/// A card was answered or dismissed. Cards that are not ours are ignored at once; ours are mapped to a saved
/// fix by their question id (never by kind or text) and handled on a thread of their own.
pub fn on_resolved(question: &Question) {
    if !super::engine::is_ci_card(question) {
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

/// Watch the pull request of this copy (if not watched) and, when its head already failed, raise the fix card now.
pub fn offer(project: Option<&str>, handle: &str) -> Result<WatchView, OperationError> {
    ENGINE.get().ok_or_else(not_ready)?.offer(project, handle)
}

/// One button of a row: `stop_watching` and `watch_again` for a watch id, `ask_again`, `push` and `dont_push`
/// for a fix id. `who` is `desktop` or `phone`.
pub fn act(id: &str, action: &str, who: &str) -> Result<CiFixView, OperationError> {
    ENGINE.get().ok_or_else(not_ready)?.act(id, action, who)
}

/// What the Settings page shows: the settings, the GitHub status, the watches and the latest fixes.
pub fn view() -> CiFixView {
    match ENGINE.get() {
        Some(engine) => engine.view(),
        None => super::answers::offline_view(&live_store()),
    }
}

/// The cleaned snapshot a fix started with (the stored one, never a new read).
pub fn log(fix_id: &str) -> Result<FixLogView, OperationError> {
    match ENGINE.get() {
        Some(engine) => engine.log(fix_id),
        None => super::answers::log_of(&live_store(), fix_id),
    }
}

/// Look at GitHub now instead of waiting for the next poll.
pub fn poll_now() {
    if let Some(engine) = ENGINE.get() {
        engine.poll_now();
    }
}

/// Save the settings (clamped) and tell the windows.
pub fn save_settings(settings: &Settings, who: &str) -> Result<CiFixView, OperationError> {
    match ENGINE.get() {
        Some(engine) => engine.save_settings(settings, who),
        None => {
            live_store()
                .save_settings(settings)
                .map_err(|message| OperationError::new("INVALID_ARGUMENT", message, false))?;
            Ok(view())
        }
    }
}
