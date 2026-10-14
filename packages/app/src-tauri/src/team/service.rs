//! Live wiring of the Team feature: config, the transport, the push watcher, the queue runner and
//! what the cockpit commands call. The decisions live in the sibling modules (and are tested
//! there); this file connects them to the running app.
//!
//! Threads (only while Team is on in Settings and complete):
//! - `team-inbound`: reads the transport, validates (`store::ingest`), saves, tells the cockpit;
//! - `team-runtime`: watches the repositories of open panes (and `team.json` projects) for pushes,
//!   which become **drafts** ("Tell the team?"); and moves queued updates along.
//!
//! Nothing is sent without a click (`send`), and a teammate's event never runs anything.

use super::actions::{self, LocalFacts, Refusal};
use super::config::{self, TeamConfig};
use super::event::{self, EventSource, ProjectRef, Subject, TeamEvent, TeamEventKind, Tier};
use super::project_key;
use super::sources::git_refs::{self, PaneFacts, PushFact, RepoWatch};
use super::sources::pr::{GhCli, PrLookup, RETRY_AFTER_MS};
use super::store::{ingest, Card, CardState, Draft, Ingested, Job, JobState, TeamStore};
use super::transport::{folder::FolderTransport, TeamTransport};
use crate::pty::PaneRegistry;
use notify::{RecursiveMode, Watcher};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

type Registry = Arc<Mutex<PaneRegistry>>;

const TICK: Duration = Duration::from_millis(1000);
/// Ref snapshots are re-read at least this often even if the watcher stayed quiet.
const REF_RESCAN_MS: u64 = 15_000;

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

struct Inner {
    config: TeamConfig,
    store: TeamStore,
    transport: Option<Arc<dyn TeamTransport>>,
    /// Bumped on every (re)start so old reader threads stop.
    generation: u64,
    error: Option<String>,
    watches: HashMap<PathBuf, RepoWatch>,
    keys: HashMap<PathBuf, Option<String>>,
}

pub struct Service {
    app: Option<tauri::AppHandle>,
    registry: Registry,
    dir: PathBuf,
    inner: Mutex<Inner>,
    wake: Mutex<Option<std::sync::mpsc::Sender<()>>>,
}

static SERVICE: OnceLock<Arc<Service>> = OnceLock::new();

fn service() -> Result<&'static Arc<Service>, String> {
    SERVICE.get().ok_or_else(|| "Team isn't ready yet.".to_string())
}

/// Start the Team service (one line in `lib.rs` setup). Does nothing visible until Team is turned
/// on in Settings.
pub fn init(app: tauri::AppHandle, registry: Registry) {
    let dir = crate::app_paths::app_data_dir().join("team");
    let svc = Arc::new(Service::new(Some(app), registry, dir));
    if SERVICE.set(svc.clone()).is_ok() {
        svc.restart();
        let runtime = svc.clone();
        let _ = std::thread::Builder::new().name("team-runtime".into()).spawn(move || runtime.runtime_loop());
    }
}

impl Service {
    fn new(app: Option<tauri::AppHandle>, registry: Registry, dir: PathBuf) -> Self {
        let config = config::load(&config::path_in(&dir));
        let store = TeamStore::load(&dir.join("state.json"));
        Self {
            app,
            registry,
            dir,
            inner: Mutex::new(Inner {
                config,
                store,
                transport: None,
                generation: 0,
                error: None,
                watches: HashMap::new(),
                keys: HashMap::new(),
            }),
            wake: Mutex::new(None),
        }
    }

    fn state_path(&self) -> PathBuf {
        self.dir.join("state.json")
    }

    fn save(&self, inner: &Inner) {
        if let Err(error) = inner.store.save(&self.state_path()) {
            tracing::warn!(%error, "team: could not save state");
        }
    }

    fn changed(&self) {
        if let Some(app) = &self.app {
            use tauri::Emitter;
            let _ = app.emit("team://changed", ());
        }
    }

    /// Stop the transport and start it again from the current config.
    fn restart(self: &Arc<Self>) {
        let (transport, generation) = {
            let mut inner = self.inner.lock();
            if let Some(old) = inner.transport.take() {
                old.stop();
            }
            inner.generation += 1;
            inner.error = None;
            inner.watches.clear();
            if !inner.config.is_active() {
                return;
            }
            let transport: Arc<dyn TeamTransport> = Arc::new(FolderTransport::new(inner.config.folder.clone()));
            inner.transport = Some(transport.clone());
            (transport, inner.generation)
        };
        match transport.subscribe() {
            Ok(rx) => {
                let svc = self.clone();
                let _ = std::thread::Builder::new().name("team-inbound".into()).spawn(move || {
                    for inbound in rx {
                        if svc.inner.lock().generation != generation {
                            break;
                        }
                        svc.receive(inbound.event, inbound.received_at_ms);
                    }
                });
            }
            Err(error) => self.inner.lock().error = Some(error.0),
        }
        self.changed();
    }

    fn receive(&self, event: TeamEvent, at_ms: u64) {
        let outcome = {
            let mut inner = self.inner.lock();
            let config = inner.config.clone();
            let outcome = ingest(&mut inner.store, &config, event, at_ms);
            self.save(&inner);
            outcome
        };
        match outcome {
            Ingested::Card(id) => {
                tracing::info!(card = %id, "team.received");
                self.changed();
            }
            Ingested::Ignored(why) => tracing::debug!(why, "team: event not shown"),
        }
    }

    // ------------------------------------------------------------------ panes and repositories

    fn pane_facts(&self) -> Vec<(String, PaneFacts)> {
        let infos = self.registry.lock().list();
        let labels: HashMap<String, String> =
            crate::tui::descriptors().into_iter().map(|d| (d.id.to_string(), d.label)).collect();
        infos
            .into_iter()
            .map(|info| {
                let last = crate::turn_summary::tracker().summaries(&info.id).into_iter().last();
                let facts = PaneFacts {
                    label: labels.get(&info.agent_type).cloned().unwrap_or_else(|| info.agent_type.clone()),
                    is_agent: crate::tui::is_agent_backend(&info.agent_type),
                    status: info.status.clone(),
                    cwd: info.cwd.clone(),
                    last_turn_end_ms: last.map(|s| s.ended_at_ms),
                };
                (info.id, facts)
            })
            .collect()
    }

    /// Top-level folders of the repositories this app knows: `team.json` projects and the folders
    /// of open panes (a separate copy counts as its origin project).
    fn known_repos(&self, config: &TeamConfig) -> Vec<PathBuf> {
        let mut dirs: Vec<String> = config.projects.clone();
        for info in self.registry.lock().list() {
            dirs.push(crate::worktrees::origin_of_cwd(&info.cwd).unwrap_or(info.cwd));
        }
        let mut seen = HashSet::new();
        dirs.into_iter()
            .filter_map(|d| super::git::toplevel(Path::new(&d)))
            .filter(|top| seen.insert(crate::project_path::comparable_project_path(top)))
            .collect()
    }

    fn key_of(&self, repo: &Path) -> Option<String> {
        if let Some(found) = self.inner.lock().keys.get(repo) {
            return found.clone();
        }
        let key = project_key::project_key_of(repo);
        self.inner.lock().keys.insert(repo.to_path_buf(), key.clone());
        key
    }

    /// My local repository for a teammate's project key.
    fn repo_for_key(&self, config: &TeamConfig, key: &str) -> Option<PathBuf> {
        self.known_repos(config).into_iter().find(|repo| self.key_of(repo).as_deref() == Some(key))
    }

    // ------------------------------------------------------------------ the runtime loop

    fn runtime_loop(self: Arc<Self>) {
        let (wake_tx, wake_rx) = std::sync::mpsc::channel::<()>();
        *self.wake.lock() = Some(wake_tx.clone());
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let relevant = event.paths.iter().any(|p| {
                    let text = p.to_string_lossy().replace('\\', "/");
                    text.contains("refs/remotes") || text.ends_with("packed-refs")
                });
                if relevant {
                    let _ = wake_tx.send(());
                }
            }
        })
        .ok();
        let mut watched: HashSet<PathBuf> = HashSet::new();
        let mut last_scan = 0u64;
        loop {
            let woke = match wake_rx.recv_timeout(TICK) {
                Ok(()) => {
                    std::thread::sleep(Duration::from_millis(150));
                    while wake_rx.try_recv().is_ok() {}
                    true
                }
                Err(_) => false,
            };
            let config = self.inner.lock().config.clone();
            if !config.is_active() {
                continue;
            }
            let now = now_ms();
            if woke || now.saturating_sub(last_scan) >= REF_RESCAN_MS || self.inner.lock().watches.is_empty() {
                last_scan = now;
                self.scan_pushes(&config, &mut watcher, &mut watched);
            }
            self.advance_jobs();
        }
    }

    fn scan_pushes(
        self: &Arc<Self>,
        config: &TeamConfig,
        watcher: &mut Option<notify::RecommendedWatcher>,
        watched: &mut HashSet<PathBuf>,
    ) {
        let repos = self.known_repos(config);
        let mut found: Vec<(PathBuf, PushFact)> = Vec::new();
        for repo in repos {
            if self.key_of(&repo).is_none() {
                continue; // no origin remote: nothing to tell a team about
            }
            let mut inner = self.inner.lock();
            if !inner.watches.contains_key(&repo) {
                if let Some(watch) = RepoWatch::open(&repo) {
                    if watched.insert(watch.common_dir.clone()) {
                        if let Some(w) = watcher.as_mut() {
                            let _ = w.watch(&watch.common_dir, RecursiveMode::Recursive);
                        }
                    }
                    inner.watches.insert(repo.clone(), watch);
                }
                continue; // the first snapshot is the baseline
            }
            if let Some(watch) = inner.watches.get_mut(&repo) {
                for fact in watch.poll() {
                    found.push((repo.clone(), fact));
                }
            }
        }
        if found.is_empty() {
            return;
        }
        let panes: Vec<PaneFacts> = self.pane_facts().into_iter().map(|(_, f)| f).collect();
        let now = now_ms();
        for (repo, fact) in found {
            if fact.remote != actions::REMOTE || !event::is_valid_branch(&fact.branch) {
                continue;
            }
            let Some(key) = self.key_of(&repo) else { continue };
            let agent = git_refs::attribute(&repo, now, &panes);
            let event = build_push_event(config, &key, &fact, agent, now);
            // The draft id is the event id: the card id on the teammate's side.
            let draft = Draft {
                id: event.id.clone(),
                event,
                repo: repo.to_string_lossy().into_owned(),
                created_at_ms: now,
            };
            tracing::info!(branch = %fact.branch, "team: push seen, asking to tell the team");
            let id = draft.id.clone();
            {
                let mut inner = self.inner.lock();
                inner.store.add_draft(draft.clone());
                self.save(&inner);
            }
            if let Some(app) = &self.app {
                use tauri::Emitter;
                let _ = app.emit("team://draft", &draft);
            }
            self.changed();
            if key.starts_with("github.com/") {
                self.look_for_pr(id, repo, fact.branch.clone());
            }
        }
    }

    /// `gh pr list --head <branch>` now and at the retry delays; a hit turns the unsent draft into
    /// "PR #n is open".
    fn look_for_pr(self: &Arc<Self>, draft_id: String, repo: PathBuf, branch: String) {
        let svc = self.clone();
        let _ = std::thread::Builder::new().name("team-pr".into()).spawn(move || {
            let mut waited = 0u64;
            for delay in std::iter::once(5_000u64).chain(RETRY_AFTER_MS) {
                std::thread::sleep(Duration::from_millis(delay.saturating_sub(waited)));
                waited = delay;
                if !svc.inner.lock().store.drafts.iter().any(|d| d.id == draft_id) {
                    return; // sent or dismissed meanwhile
                }
                if let Some(pr) = GhCli.open_pr_for(&repo, &branch) {
                    let mut inner = svc.inner.lock();
                    if let Some(draft) = inner.store.drafts.iter_mut().find(|d| d.id == draft_id) {
                        draft.event.kind = TeamEventKind::PrOpened;
                        draft.event.subject.pr = Some(pr);
                        draft.event.dedupe_key =
                            event::dedupe_key(&draft.event.project.key, draft.event.kind, &draft.event.subject);
                    }
                    svc.save(&inner);
                    drop(inner);
                    svc.changed();
                    return;
                }
            }
        });
    }

    // ------------------------------------------------------------------ queued updates

    fn advance_jobs(self: &Arc<Self>) {
        let jobs: Vec<Job> = self
            .inner
            .lock()
            .store
            .jobs
            .iter()
            .filter(|j| matches!(j.state, JobState::Waiting | JobState::Working))
            .cloned()
            .collect();
        for job in jobs {
            match job.state {
                JobState::Waiting => {
                    let busy = job.when != "now"
                        && job.pane_id.as_deref().is_some_and(|id| {
                            self.registry
                                .lock()
                                .list()
                                .iter()
                                .any(|p| p.id == id && matches!(p.status.as_str(), "running" | "waiting_input"))
                        });
                    if !busy {
                        self.start_job(job);
                    }
                }
                JobState::Working if job.agent_ran => self.check_agent(job),
                _ => {}
            }
        }
    }

    fn update_job(&self, id: &str, f: impl FnOnce(&mut Job)) {
        let mut inner = self.inner.lock();
        if let Some(job) = inner.store.job_mut(id) {
            f(job);
            job.updated_at_ms = now_ms();
        }
        self.save(&inner);
        drop(inner);
        self.changed();
    }

    fn fail_job(&self, id: &str, message: String) {
        tracing::info!(job = %id, %message, "team: queued update failed");
        self.update_job(id, |j| {
            j.state = JobState::Failed;
            j.message = Some(message);
        });
    }

    fn start_job(self: &Arc<Self>, job: Job) {
        self.update_job(&job.id, |j| j.state = JobState::Working);
        let repo = PathBuf::from(&job.project);
        if super::git::current_branch(&repo).as_deref() != Some(job.into.as_str()) {
            return self.fail_job(&job.id, format!("You switched away from {}. Queue it again from there.", job.into));
        }
        let prepared = match actions::prepare_merge(&repo, &job.handle, &job.their_branch, &job.commit) {
            Ok(p) => p,
            Err(refusal) => return self.fail_job(&job.id, refusal.message),
        };
        let copy_branch = prepared.worktree.branch.clone();
        if prepared.conflicts.is_empty() {
            let stats = crate::worktrees::changes(&prepared.worktree).ok().map(|c| event::DiffStat {
                files: c.files_changed as u32,
                additions: c.additions,
                deletions: c.deletions,
            });
            tracing::info!(job = %job.id, "team.action queue: merged cleanly, no agent needed");
            return self.update_job(&job.id, |j| {
                j.state = JobState::Ready;
                j.copy_branch = Some(copy_branch);
                j.stats = stats;
                j.message = Some("Merged cleanly. No agent was needed.".into());
            });
        }
        let conflicts = prepared.conflicts.clone();
        self.update_job(&job.id, |j| {
            j.copy_branch = Some(copy_branch);
            j.conflicts = conflicts.clone();
            j.agent_ran = true;
        });
        let mut with_conflicts = job.clone();
        with_conflicts.conflicts = conflicts;
        let Some(app) = self.app.clone() else {
            return self.fail_job(&job.id, "No agent can start here.".into());
        };
        let request: crate::agent_runs::AgentRunRequest =
            match serde_json::from_value(actions::run_request(&job.project, &with_conflicts)) {
                Ok(r) => r,
                Err(e) => return self.fail_job(&job.id, format!("Couldn't start the agent: {e}")),
            };
        match crate::agent_runs::run_agent(request, self.registry.clone(), app, None) {
            Ok(view) if view.handle == job.handle => {
                tracing::info!(job = %job.id, handle = %view.handle, "team.action queue: agent started");
            }
            Ok(view) => {
                // Never expected: the run must reuse the prepared copy. Stop rather than guess.
                if let Some(app) = self.app.clone() {
                    let _ = crate::agent_runs::cancel_agent(&job.project, &view.handle, None, self.registry.clone(), app);
                }
                self.fail_job(&job.id, "The agent started in the wrong copy, so it was stopped.".into());
            }
            Err(error) => self.fail_job(&job.id, format!("Couldn't start {}: {}", job.agent_label, error.message)),
        }
    }

    fn check_agent(&self, job: Job) {
        let Ok(op) = crate::operations::latest_agent_run(&job.project, &job.handle) else {
            if now_ms().saturating_sub(job.updated_at_ms) > actions::QUEUED_RUN_TIMEOUT_MS {
                self.fail_job(&job.id, "The agent run was lost.".into());
            }
            return;
        };
        if !op.status.terminal() {
            return;
        }
        let Some(copy) = crate::worktrees::find_by_handle(&job.project, &job.handle) else {
            return self.fail_job(&job.id, "The separate copy is gone.".into());
        };
        match actions::check_resolved(Path::new(&copy.path), &job.commit) {
            Ok(()) => {
                let stats = crate::worktrees::changes(&copy).ok().map(|c| event::DiffStat {
                    files: c.files_changed as u32,
                    additions: c.additions,
                    deletions: c.deletions,
                });
                let n = job.conflicts.len();
                let message = format!(
                    "{} merged {} and resolved {n} conflict{}.",
                    job.agent_label,
                    job.their_branch,
                    if n == 1 { "" } else { "s" }
                );
                self.update_job(&job.id, |j| {
                    j.state = JobState::Ready;
                    j.stats = stats;
                    j.message = Some(message);
                });
            }
            Err(refusal) => {
                let message = format!("{} couldn't finish: {}", job.agent_label, refusal.message);
                self.fail_job(&job.id, message);
            }
        }
    }
}

/// The event a push becomes (before the person adds a note and sends it).
pub fn build_push_event(
    config: &TeamConfig,
    key: &str,
    fact: &PushFact,
    agent: Option<event::AgentTag>,
    now: u64,
) -> TeamEvent {
    let subject = Subject {
        branch: Some(fact.branch.clone()),
        commit: Some(fact.commit.clone()),
        forced: fact.forced,
        paths: fact.paths.clone(),
        stats: Some(fact.stats),
        ..Subject::default()
    };
    TeamEvent {
        v: event::SCHEMA_VERSION,
        id: event::new_event_id(now),
        team: config.team.clone(),
        project: ProjectRef { key: key.to_string(), name: project_key::display_name(key) },
        actor: event::Actor {
            member: config.me.id.clone(),
            device: config.device.clone(),
            display: config.me.name.clone(),
            agent,
        },
        kind: TeamEventKind::Pushed,
        at_ms: now,
        dedupe_key: event::dedupe_key(key, TeamEventKind::Pushed, &subject),
        subject,
        note: None,
        to: Vec::new(),
        tier_hint: Tier::Ambient,
        expires_at_ms: now + event::DEFAULT_TTL_MS,
        source: EventSource::Git,
    }
}

// ---------------------------------------------------------------------- what the cockpit reads

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChoice {
    pub pane_id: String,
    pub label: String,
    pub status: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardView {
    #[serde(flatten)]
    pub card: Card,
    /// My copy of the project, when I have one.
    pub local: Option<LocalFacts>,
    /// Agent panes open in that project, for "Queue my agent".
    pub panes: Vec<PaneChoice>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamView {
    pub enabled: bool,
    pub active: bool,
    pub me: String,
    pub members: Vec<String>,
    pub transport: Option<&'static str>,
    pub error: Option<String>,
    pub drafts: Vec<Draft>,
    pub cards: Vec<CardView>,
    pub jobs: Vec<Job>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
    pub enabled: bool,
    pub team: String,
    pub folder: String,
    pub name: String,
    pub members: Vec<String>,
    pub projects: Vec<String>,
    /// `PUPPET_MASTER_PROFILE`, when this is not the default app.
    pub profile: Option<String>,
}

fn config_view(config: &TeamConfig) -> ConfigView {
    ConfigView {
        enabled: config.enabled,
        team: config.team.clone(),
        folder: config.folder.clone(),
        name: config.me.name.clone(),
        members: config.member_names(),
        projects: config.projects.clone(),
        profile: crate::app_paths::profile(),
    }
}

pub fn view() -> Result<TeamView, String> {
    let svc = service()?;
    let (config, store, transport, error) = {
        let inner = svc.inner.lock();
        (
            inner.config.clone(),
            inner.store.clone(),
            inner.transport.as_ref().map(|t| t.kind()),
            inner.error.clone(),
        )
    };
    let panes = svc.pane_facts();
    let titles: HashMap<String, Option<String>> =
        svc.registry.lock().list().into_iter().map(|p| (p.id, p.title)).collect();
    let mut cards = Vec::new();
    for card in store.cards.iter().rev().filter(|c| c.state != CardState::Dismissed).take(30) {
        let repo = svc.repo_for_key(&config, &card.event.project.key);
        let local = repo.as_deref().map(|r| actions::local_facts(r, &card.event));
        let choices = repo
            .as_deref()
            .map(|r| {
                panes
                    .iter()
                    .filter(|(_, f)| f.is_agent && crate::project_path::workspace_covers(r, Path::new(&f.cwd)))
                    .filter(|(_, f)| crate::worktrees::origin_of_cwd(&f.cwd).is_none())
                    .map(|(id, f)| PaneChoice {
                        pane_id: id.clone(),
                        label: f.label.clone(),
                        status: f.status.clone(),
                        title: titles.get(id).cloned().flatten(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        cards.push(CardView { card: card.clone(), local, panes: choices });
    }
    Ok(TeamView {
        enabled: config.enabled,
        active: config.is_active(),
        me: config.me.name.clone(),
        members: config.member_names(),
        transport,
        error,
        drafts: store.drafts.iter().rev().cloned().collect(),
        cards,
        jobs: store.jobs.iter().rev().filter(|j| j.state != JobState::Discarded).cloned().collect(),
    })
}

pub fn config_get() -> Result<ConfigView, String> {
    Ok(config_view(&service()?.inner.lock().config))
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigInput {
    pub enabled: bool,
    #[serde(default)]
    pub team: Option<String>,
    pub folder: String,
    pub name: String,
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default)]
    pub projects: Vec<String>,
}

pub fn config_set(input: ConfigInput) -> Result<ConfigView, String> {
    let svc = service()?;
    let current = svc.inner.lock().config.clone();
    let mut next = current.clone();
    next.enabled = input.enabled;
    if let Some(team) = input.team {
        next.team = team;
    }
    next.folder = input.folder;
    next.me.name = input.name;
    next.members = input
        .members
        .into_iter()
        .map(|name| {
            let id = current.members.iter().find(|m| m.name.eq_ignore_ascii_case(name.trim())).map(|m| m.id.clone());
            config::Member { id: id.unwrap_or_default(), name }
        })
        .collect();
    next.projects = input.projects;
    let next = next.normalized()?;
    config::save(&config::path_in(&svc.dir), &next)?;
    svc.inner.lock().config = next.clone();
    svc.restart();
    Ok(config_view(&next))
}

/// **Tell the team**: send the draft, with my note. The one place a push leaves this computer.
pub fn send(draft_id: &str, note: Option<String>) -> Result<(), String> {
    let svc = service()?;
    let (mut draft, transport) = {
        let mut inner = svc.inner.lock();
        let transport = inner.transport.clone().ok_or("Team is off. Turn it on in Settings first.")?;
        let draft = inner.store.take_draft(draft_id).ok_or("That update was already sent or dismissed.")?;
        (draft, transport)
    };
    draft.event.note = note.map(|n| event::clean_text(&n, event::MAX_NOTE_CHARS)).filter(|n| !n.is_empty());
    draft.event.at_ms = now_ms();
    if let Err(error) = transport.send(&draft.event) {
        let mut inner = svc.inner.lock();
        inner.store.add_draft(draft);
        return Err(error.0);
    }
    tracing::info!(event = %draft.event.id, "team.sent");
    let inner = svc.inner.lock();
    svc.save(&inner);
    drop(inner);
    svc.changed();
    Ok(())
}

pub fn dismiss_draft(draft_id: &str) -> Result<(), String> {
    let svc = service()?;
    let mut inner = svc.inner.lock();
    inner.store.take_draft(draft_id);
    svc.save(&inner);
    drop(inner);
    svc.changed();
    Ok(())
}

pub fn dismiss_card(card_id: &str) -> Result<(), String> {
    let svc = service()?;
    let mut inner = svc.inner.lock();
    if let Some(card) = inner.store.card_mut(card_id) {
        card.state = CardState::Dismissed;
    }
    svc.save(&inner);
    drop(inner);
    svc.changed();
    Ok(())
}

fn card_and_repo(svc: &Service, card_id: &str) -> Result<(Card, PathBuf), String> {
    let (card, config) = {
        let inner = svc.inner.lock();
        (inner.store.card(card_id).cloned().ok_or("That card is gone.")?, inner.config.clone())
    };
    let repo = svc
        .repo_for_key(&config, &card.event.project.key)
        .ok_or("This project isn't on this computer, or no pane has it open.")?;
    Ok((card, repo))
}

fn refusal_text(r: Refusal) -> String {
    r.message
}

/// **Pull now** (fast-forward only).
pub fn pull(card_id: &str) -> Result<String, String> {
    let svc = service()?;
    let (card, repo) = card_and_repo(svc, card_id)?;
    let message = actions::pull_now(&repo, &card.event).map_err(refusal_text)?;
    tracing::info!(card = %card_id, "team.action pull");
    let mut inner = svc.inner.lock();
    if let Some(c) = inner.store.card_mut(card_id) {
        c.state = CardState::Pulled;
    }
    svc.save(&inner);
    drop(inner);
    svc.changed();
    Ok(message)
}

/// **Queue my agent**: an update of my current branch with the teammate's push, in a separate
/// copy, started when `pane_id` is not working (`when: "idle"`) or at once (`"now"`).
pub fn queue(card_id: &str, pane_id: &str, when: &str) -> Result<Job, String> {
    let svc = service()?;
    let (card, repo) = card_and_repo(svc, card_id)?;
    let facts = actions::local_facts(&repo, &card.event);
    if !facts.can_queue {
        return Err(facts.note.unwrap_or_else(|| "There's nothing to bring in here.".into()));
    }
    let pane = svc
        .registry
        .lock()
        .list()
        .into_iter()
        .find(|p| p.id == pane_id)
        .ok_or("That agent's pane is closed.")?;
    if !crate::tui::is_agent_backend(&pane.agent_type) {
        return Err("Pick an agent pane, not a shell.".into());
    }
    let label = crate::tui::descriptors()
        .into_iter()
        .find(|d| d.id == pane.agent_type)
        .map(|d| d.label)
        .unwrap_or_else(|| pane.agent_type.clone());
    let now = now_ms();
    let job = Job {
        id: uuid::Uuid::new_v4().to_string(),
        card_id: card.id.clone(),
        project: repo.to_string_lossy().into_owned(),
        pane_id: Some(pane.id.clone()),
        agent_type: pane.agent_type.clone(),
        agent_label: label,
        who: card.event.actor.display.clone(),
        their_branch: card.event.subject.branch.clone().unwrap_or_default(),
        commit: card.event.subject.commit.clone().unwrap_or_default(),
        note: card.event.note.clone(),
        into: facts.current_branch.clone().unwrap_or_default(),
        state: JobState::Waiting,
        handle: uuid::Uuid::new_v4().to_string(),
        copy_branch: None,
        conflicts: Vec::new(),
        agent_ran: false,
        stats: None,
        message: None,
        when: if when == "now" { "now".into() } else { "idle".into() },
        created_at_ms: now,
        updated_at_ms: now,
    };
    tracing::info!(card = %card_id, job = %job.id, "team.action queue");
    let mut inner = svc.inner.lock();
    inner.store.add_job(job.clone());
    if let Some(c) = inner.store.card_mut(card_id) {
        c.state = CardState::Queued;
    }
    svc.save(&inner);
    drop(inner);
    svc.changed();
    if let Some(wake) = svc.wake.lock().as_ref() {
        let _ = wake.send(());
    }
    Ok(job)
}

fn job(svc: &Service, job_id: &str) -> Result<Job, String> {
    svc.inner.lock().store.jobs.iter().find(|j| j.id == job_id).cloned().ok_or_else(|| "That update is gone.".into())
}

/// Panes sitting inside the separate copy (the queued run's own pane): closed before the copy is
/// removed. The person's own panes are never touched.
fn close_copy_panes(svc: &Service, copy_path: &str) {
    let ids: Vec<String> = svc
        .registry
        .lock()
        .list()
        .into_iter()
        .filter(|p| crate::project_path::workspace_covers(Path::new(copy_path), Path::new(&p.cwd)))
        .map(|p| p.id)
        .collect();
    for id in ids {
        let _ = crate::pty::registry_kill_pane_with_reason(&svc.registry, &id, Some("worktree_closed"));
    }
}

/// **Apply to my branch**: fast-forward only, local only.
pub fn apply(job_id: &str) -> Result<String, String> {
    let svc = service()?;
    let job = job(svc, job_id)?;
    if job.state != JobState::Ready {
        return Err("This update isn't ready yet.".into());
    }
    let copy = crate::worktrees::find_by_handle(&job.project, &job.handle).ok_or("The separate copy is gone.")?;
    let repo = PathBuf::from(&job.project);
    actions::check_resolved(Path::new(&copy.path), &job.commit).map_err(refusal_text)?;
    let working = svc
        .pane_facts()
        .into_iter()
        .map(|(_, f)| f)
        .find(|f| {
            f.status == "running"
                && crate::project_path::workspace_covers(&repo, Path::new(&f.cwd))
                && !crate::project_path::workspace_covers(Path::new(&copy.path), Path::new(&f.cwd))
        })
        .map(|f| f.label);
    let message = match actions::apply(&repo, &job.into, &copy.branch, working.as_deref()) {
        Ok(m) => m,
        Err(refusal) => {
            if refusal.code == "BRANCH_MOVED" {
                svc.update_job(job_id, |j| j.message = Some(refusal.message.clone()));
            }
            return Err(refusal.message);
        }
    };
    close_copy_panes(svc, &copy.path);
    if let Err(error) = crate::worktrees::cleanup_merged(&copy) {
        tracing::warn!(error = %error.message, "team: copy not removed after apply");
    }
    let _ = crate::operations::close_worktree(&job.project, &job.handle, "merged");
    tracing::info!(job = %job_id, "team.action apply");
    svc.update_job(job_id, |j| {
        j.state = JobState::Applied;
        j.message = Some(message.clone());
    });
    Ok(message)
}

/// **Discard**: delete the separate copy and its branch.
pub fn discard(job_id: &str) -> Result<(), String> {
    let svc = service()?;
    let job = job(svc, job_id)?;
    if job.state == JobState::Working {
        return Err("The agent is still working on it. Stop it first.".into());
    }
    if let Some(copy) = crate::worktrees::find_by_handle(&job.project, &job.handle) {
        close_copy_panes(svc, &copy.path);
        crate::worktrees::discard(&copy).map_err(|e| e.message)?;
        let _ = crate::operations::close_worktree(&job.project, &job.handle, "discarded");
    }
    tracing::info!(job = %job_id, "team.action discard");
    svc.update_job(job_id, |j| j.state = JobState::Discarded);
    Ok(())
}

/// The separate copy's changes, for **See changes** (the cockpit's Changes view).
pub fn diff(job_id: &str) -> Result<crate::worktrees::DiffView, String> {
    let svc = service()?;
    let job = job(svc, job_id)?;
    let copy = crate::worktrees::find_by_handle(&job.project, &job.handle).ok_or("The separate copy is gone.")?;
    crate::worktrees::diff(&copy, None).map_err(|e| e.message)
}
