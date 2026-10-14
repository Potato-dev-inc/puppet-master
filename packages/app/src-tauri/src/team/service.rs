//! Live wiring of the Team feature: config, keys, the roster, the transport, the push watcher, the
//! GitHub poller, the queue runner and what the cockpit commands call. The decisions live in the
//! sibling modules (and are tested there); this file connects them to the running app.
//!
//! Threads (only while Team is on in Settings and this app is on a team, or waiting to join one):
//! - `team-inbound`: reads envelopes from the transport, opens them (`secure::Session::open`:
//!   roster, signature, key, replay, role), and only then `store::ingest`s; saves; tells the cockpit;
//! - `team-runtime`: watches the repositories of open panes (and `team.json` projects) for pushes,
//!   which become **drafts** ("Tell the team?"); polls GitHub for facts; keeps the roster and keys
//!   current; picks up join requests (admin) or the admin's yes (joiner); moves queued updates.
//!
//! Nothing is sent without a click (`send`), and a teammate's event never runs anything.

use super::actions::{self, LocalFacts, Refusal};
use super::config::{self, TeamConfig};
use super::event::{self, EventSource, ProjectRef, Subject, TeamEvent, TeamEventKind, Tier};
use super::invite::{self, InviteLink};
use super::keystore::KeyStore;
use super::membership;
use super::merge_green;
use super::project_key;
use super::roster::{Role, Roster, SignedOp};
use super::secure::{Opened, Payload, Session, Verified};
use super::sources::git_refs::{self, PaneFacts, PushFact, RepoWatch};
use super::sources::github::{self, Gh, GhCli, RepoPoll};
use super::sources::pr::{GhCli as PrGh, PrLookup, RETRY_AFTER_MS};
use super::store::{ingest, Card, CardState, Draft, Ingested, Job, JobState, PendingJoin, TeamStore};
use super::transport::{folder::FolderTransport, relay::RelayTransport, Inbound, TeamTransport};
use crate::audit::{self, Kind, Who};
use crate::pty::PaneRegistry;
use notify::{RecursiveMode, Watcher};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

type Registry = Arc<Mutex<PaneRegistry>>;

const TICK: Duration = Duration::from_millis(1000);
/// Ref snapshots are re-read at least this often even if the watcher stayed quiet.
const REF_RESCAN_MS: u64 = 15_000;
/// How often the roster and keys are re-read without a nudge.
const ROSTER_REFRESH_MS: u64 = 60_000;
/// How often join requests (admin) or the admin's answer (joiner) are looked for.
const JOIN_POLL_MS: u64 = 4_000;

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

struct Inner {
    config: TeamConfig,
    /// `team.json` exists but could not be read; Team stays off until it is saved again.
    read_error: Option<String>,
    store: TeamStore,
    session: Option<Session>,
    transport: Option<Arc<dyn TeamTransport>>,
    /// Bumped on every (re)start so old reader threads stop.
    generation: u64,
    error: Option<String>,
    watches: HashMap<PathBuf, RepoWatch>,
    keys: HashMap<PathBuf, Option<String>>,
    github: HashMap<String, RepoPoll>,
    needs_refresh: bool,
    last_refresh_ms: u64,
    last_join_poll_ms: u64,
}

pub struct Service {
    app: Option<tauri::AppHandle>,
    registry: Registry,
    dir: PathBuf,
    keystore: KeyStore,
    gh: Arc<dyn Gh>,
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
    let svc = Arc::new(Service::new(Some(app), registry, dir, KeyStore::app(), Arc::new(GhCli)));
    if SERVICE.set(svc.clone()).is_ok() {
        svc.restart();
        let runtime = svc.clone();
        let _ = std::thread::Builder::new().name("team-runtime".into()).spawn(move || runtime.runtime_loop());
    }
}

fn roster_path(dir: &Path) -> PathBuf {
    dir.join("roster.json")
}

fn load_roster(dir: &Path, config: &TeamConfig) -> Option<Roster> {
    if !config.has_team() || config.join.is_some() {
        return None;
    }
    let bytes = std::fs::read(roster_path(dir)).ok()?;
    let ops: Vec<SignedOp> = serde_json::from_slice(&bytes).ok()?;
    match Roster::verify(&config.team, &ops) {
        Ok(roster) if roster.root == config.root => Some(roster),
        Ok(_) => None,
        Err(error) => {
            tracing::warn!(%error, "team: the saved team list does not check out");
            None
        }
    }
}

fn save_roster(dir: &Path, roster: &Roster) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_vec_pretty(&roster.ops).map_err(|e| e.to_string())?;
    let path = roster_path(dir);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

fn make_transport(config: &TeamConfig, team: &str, keys: &KeyStore) -> Result<Arc<dyn TeamTransport>, String> {
    if !config.relay.trim().is_empty() {
        Ok(Arc::new(RelayTransport::new(&config.relay, team, keys.device_sign()?)?))
    } else if !config.folder.trim().is_empty() {
        Ok(Arc::new(FolderTransport::new(config.folder.clone())))
    } else {
        Err("Pick a relay or a shared folder first.".into())
    }
}

fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// `team.json` and why it couldn't be used, if so (Settings shows "couldn't be read").
pub(crate) fn load_config(dir: &Path) -> (TeamConfig, Option<String>) {
    match config::load_checked(&config::path_in(dir)) {
        Ok(config) => (config, None),
        Err(error) => {
            tracing::warn!(%error, "team: team.json could not be read; Team stays off");
            (TeamConfig::default(), Some(error))
        }
    }
}

/// What one inbound envelope did.
#[derive(Debug, Default)]
pub(crate) struct Received {
    /// A card was added or updated: (card id, sender member, kind).
    pub card: Option<(String, String, TeamEventKind)>,
    /// The roster or keys should be re-read.
    pub refresh: bool,
}

/// Open an envelope with every check (`Session::open`), then `ingest` what passed. The relay
/// cursor moves on either way (back, too, when the relay's counter went back). Pure apart from
/// logging.
pub(crate) fn open_and_ingest(session: &Session, store: &mut TeamStore, config: &TeamConfig, inbound: &Inbound) -> Received {
    let mut out = Received::default();
    if inbound.reset || inbound.cursor > store.cursor {
        store.cursor = inbound.cursor;
    }
    match session.open(&inbound.envelope, &mut store.seq, inbound.received_at_ms) {
        Ok(Opened::Event(verified)) => {
            let member = verified.member().unwrap_or_default().to_string();
            let kind = verified.event().kind;
            match ingest(store, config, verified, inbound.received_at_ms) {
                Ingested::Card(id) => out.card = Some((id, member, kind)),
                Ingested::Ignored(why) => tracing::debug!(why, "team: event not shown"),
            }
        }
        Ok(Opened::RosterChanged(_)) => out.refresh = true,
        Ok(Opened::Mine) => {}
        Err(reject) => {
            tracing::debug!(why = reject.as_str(), from = %short_id(&inbound.envelope.from), "team: envelope refused");
            out.refresh = matches!(reject, super::secure::Reject::NoKey | super::secure::Reject::UnknownDevice);
        }
    }
    out
}

/// `team.received` in the audit log.
pub(crate) fn audit_received(card: &(String, String, TeamEventKind)) {
    let (id, member, kind) = card;
    audit::record(
        Kind::TeamReceived,
        Who::Teammate(member.clone()),
        &format!("A teammate's update arrived: {}", kind.as_str().replace('_', " ")),
        json!({ "event": id, "kind": kind.as_str() }),
    );
}

impl Service {
    fn new(app: Option<tauri::AppHandle>, registry: Registry, dir: PathBuf, keystore: KeyStore, gh: Arc<dyn Gh>) -> Self {
        let (config, read_error) = load_config(&dir);
        let store = TeamStore::load(&dir.join("state.json"));
        let session = load_roster(&dir, &config).map(|r| Session::new(r, keystore.clone()));
        Self {
            app,
            registry,
            dir,
            keystore,
            gh,
            inner: Mutex::new(Inner {
                config,
                read_error,
                store,
                session,
                transport: None,
                generation: 0,
                error: None,
                watches: HashMap::new(),
                keys: HashMap::new(),
                github: HashMap::new(),
                needs_refresh: true,
                last_refresh_ms: 0,
                last_join_poll_ms: 0,
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

    fn poke(&self) {
        if let Some(wake) = self.wake.lock().as_ref() {
            let _ = wake.send(());
        }
    }

    /// Stop the transport and start it again from the current config.
    fn restart(self: &Arc<Self>) {
        let (transport, generation, cursor, subscribe) = {
            let mut inner = self.inner.lock();
            if let Some(old) = inner.transport.take() {
                old.stop();
            }
            inner.generation += 1;
            inner.error = None;
            inner.watches.clear();
            inner.github.clear();
            inner.needs_refresh = true;
            let joining = inner.config.join.is_some();
            if !(inner.config.enabled && (inner.config.is_active() || joining)) {
                drop(inner);
                self.changed();
                return;
            }
            if !joining && inner.session.is_none() {
                inner.error = Some("The saved team list doesn't check out. Join the team again.".into());
                drop(inner);
                self.changed();
                return;
            }
            let team = inner.config.team.clone();
            let transport = match make_transport(&inner.config, &team, &self.keystore) {
                Ok(t) => t,
                Err(error) => {
                    inner.error = Some(error);
                    drop(inner);
                    self.changed();
                    return;
                }
            };
            inner.transport = Some(transport.clone());
            (transport, inner.generation, inner.store.cursor, !joining)
        };
        if subscribe {
            match transport.subscribe(cursor) {
                Ok(rx) => {
                    let svc = self.clone();
                    let _ = std::thread::Builder::new().name("team-inbound".into()).spawn(move || {
                        for inbound in rx {
                            if svc.inner.lock().generation != generation {
                                break;
                            }
                            svc.receive(inbound);
                        }
                    });
                }
                Err(error) => self.inner.lock().error = Some(error.0),
            }
        }
        self.poke();
        self.changed();
    }

    /// One envelope from the wire: open it (every check), then ingest what it carries.
    fn receive(&self, inbound: Inbound) {
        let outcome = {
            let mut inner = self.inner.lock();
            let Some(session) = inner.session.clone() else { return };
            let config = inner.config.clone();
            let outcome = open_and_ingest(&session, &mut inner.store, &config, &inbound);
            if outcome.refresh {
                inner.needs_refresh = true;
            }
            self.save(&inner);
            outcome
        };
        if let Some(card) = &outcome.card {
            tracing::info!(card = %card.0, "team.received");
            audit_received(card);
            self.changed();
        }
        if outcome.refresh {
            self.poke();
        }
    }

    /// A GitHub fact this app found itself.
    fn receive_fact(&self, event: TeamEvent) {
        let Some(verified) = Verified::local_fact(event) else { return };
        let outcome = {
            let mut inner = self.inner.lock();
            let config = inner.config.clone();
            let outcome = ingest(&mut inner.store, &config, verified, now_ms());
            self.save(&inner);
            outcome
        };
        if let Ingested::Card(id) = outcome {
            tracing::info!(card = %id, "team: GitHub fact");
            self.changed();
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
        let ref_wake = wake_tx.clone();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let relevant = event.paths.iter().any(|p| {
                    let text = p.to_string_lossy().replace('\\', "/");
                    text.contains("refs/remotes") || text.ends_with("packed-refs")
                });
                if relevant {
                    let _ = ref_wake.send(());
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
            if !config.enabled {
                continue;
            }
            let now = now_ms();
            if config.join.is_some() {
                self.poll_join_answer(now);
                continue;
            }
            if !config.is_active() {
                continue;
            }
            self.keep_roster_current(now);
            self.poll_join_requests(now);
            if woke || now.saturating_sub(last_scan) >= REF_RESCAN_MS || self.inner.lock().watches.is_empty() {
                last_scan = now;
                self.scan_pushes(&config, &mut watcher, &mut watched);
            }
            self.poll_github(&config, now);
            self.advance_jobs();
        }
    }

    /// Member and admin: re-read the roster and keys when nudged, and now and then.
    fn keep_roster_current(self: &Arc<Self>, now: u64) {
        let (due, session, transport) = {
            let inner = self.inner.lock();
            let due = inner.needs_refresh || now.saturating_sub(inner.last_refresh_ms) >= ROSTER_REFRESH_MS;
            (due, inner.session.clone(), inner.transport.clone())
        };
        let (true, Some(mut session), Some(transport)) = (due, session, transport) else { return };
        {
            let mut inner = self.inner.lock();
            inner.needs_refresh = false;
            inner.last_refresh_ms = now;
        }
        let am_admin = session.my_member().is_ok_and(|m| session.roster.is_admin(&m));
        if am_admin {
            // My copy is the source. Its one chore: a fresh team key every 30 days.
            if now.saturating_sub(membership::key_made_at(&session.roster)) >= membership::ROTATE_EVERY_MS {
                match membership::rotate(&mut session.roster, &self.keystore, transport.as_ref(), now) {
                    Ok(epoch) => {
                        let _ = save_roster(&self.dir, &session.roster);
                        self.inner.lock().session = Some(session.clone());
                        let seq = next_seq(self);
                        let _ = membership::announce(&session, seq, transport.as_ref(), now);
                        audit::record(Kind::TeamKeyRotated, Who::Desktop, "The team key was replaced (every 30 days)", json!({ "epoch": epoch }));
                        self.changed();
                    }
                    Err(error) => tracing::warn!(%error, "team: couldn't replace the team key"),
                }
            }
            return;
        }
        match membership::refresh(&mut session.roster, &self.keystore, transport.as_ref()) {
            Ok(found) => {
                if found.roster_changed {
                    let _ = save_roster(&self.dir, &session.roster);
                }
                let mut inner = self.inner.lock();
                if found.removed {
                    inner.error = Some("You're no longer on this team. The admin removed this computer.".into());
                    if let Some(t) = inner.transport.take() {
                        t.stop();
                    }
                }
                if found.roster_changed || found.new_epoch {
                    tracing::info!(epoch = session.roster.epoch, "team: team list updated");
                }
                inner.session = Some(session);
                drop(inner);
                if found.roster_changed || found.new_epoch || found.removed {
                    self.changed();
                }
            }
            Err(error) => tracing::debug!(%error, "team: couldn't refresh the team list"),
        }
    }

    /// Admin: look for join requests for my open invites.
    fn poll_join_requests(&self, now: u64) {
        let (open, transport) = {
            let mut inner = self.inner.lock();
            if now.saturating_sub(inner.last_join_poll_ms) < JOIN_POLL_MS {
                return;
            }
            inner.last_join_poll_ms = now;
            let open: Vec<invite::Invite> = inner.store.invites.iter().filter(|i| i.is_open(now)).cloned().collect();
            (open, inner.transport.clone())
        };
        let (false, Some(transport)) = (open.is_empty(), transport) else { return };
        let Ok(boxes) = transport.joins() else { return };
        let Ok(kem) = self.keystore.device_kem() else { return };
        let team = self.inner.lock().config.team.clone();
        let mut found = false;
        for join in boxes {
            let digest = pm_relay_protocol::hex(&pm_relay_protocol::sha256(&[join.ct.as_bytes()]));
            if self.inner.lock().store.processed_joins.contains(&digest) {
                continue;
            }
            let Some(inv) = open.iter().find(|i| i.id == join.invite) else { continue };
            self.inner.lock().store.processed_joins.insert(digest);
            let Some(request) = invite::open_join(&kem, &join) else { continue };
            let Ok(Some(code)) = self.keystore.invite_code(&team, &inv.id) else { continue };
            let mut inner = self.inner.lock();
            match invite::check_join(&team, &request, &code) {
                Ok(()) => {
                    if !inner.store.joins.iter().any(|p| p.request.member == request.member) {
                        inner.store.joins.push(PendingJoin { id: uuid::Uuid::new_v4().to_string(), request, received_at_ms: now });
                        found = true;
                    }
                }
                Err(why) => {
                    tracing::info!(why, "team: a join request was refused");
                    if let Some(i) = inner.store.invites.iter_mut().find(|i| i.id == inv.id) {
                        i.attempts += 1;
                    }
                }
            }
            self.save(&inner);
        }
        if found {
            self.changed();
        }
    }

    /// Joiner: has the admin let me in?
    fn poll_join_answer(self: &Arc<Self>, now: u64) {
        let (link, transport) = {
            let mut inner = self.inner.lock();
            if now.saturating_sub(inner.last_join_poll_ms) < JOIN_POLL_MS {
                return;
            }
            inner.last_join_poll_ms = now;
            let Some(join) = inner.config.join.clone() else { return };
            (join.link, inner.transport.clone())
        };
        let Some(transport) = transport else { return };
        let roster = match membership::try_finish_join(&link, &self.keystore, transport.as_ref()) {
            Ok(Some(roster)) => roster,
            Ok(None) => return,
            Err(error) => {
                self.inner.lock().error = Some(error);
                self.changed();
                return;
            }
        };
        if let Err(error) = save_roster(&self.dir, &roster) {
            self.inner.lock().error = Some(error);
            return;
        }
        let config = {
            let mut inner = self.inner.lock();
            inner.config.join = None;
            inner.session = Some(Session::new(roster.clone(), self.keystore.clone()));
            inner.config.clone()
        };
        if let Err(error) = config::save(&config::path_in(&self.dir), &config) {
            self.inner.lock().error = Some(error);
        }
        audit::record(Kind::TeamJoined, Who::Desktop, &format!("You joined the team {}", roster.name), json!({ "team": roster.team }));
        tracing::info!(team = %roster.team, "team: joined");
        self.restart();
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
                if let Some(pr) = PrGh.open_pr_for(&repo, &branch) {
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

    /// GitHub facts for the GitHub repositories open here, each at its own pace (ETag, 304s).
    fn poll_github(&self, config: &TeamConfig, now: u64) {
        let roster = self.inner.lock().session.as_ref().map(|s| s.roster.clone());
        for repo in self.known_repos(config) {
            let Some(key) = self.key_of(&repo) else { continue };
            let Some(owner_repo) = github::owner_repo(&key) else { continue };
            let mut state = {
                let mut inner = self.inner.lock();
                let state = inner.github.entry(owner_repo.clone()).or_default();
                if state.next_at_ms > now {
                    continue;
                }
                std::mem::take(state)
            };
            let facts = github::poll_repo(self.gh.as_ref(), &owner_repo, &mut state, now);
            if state.next_at_ms <= now {
                state.next_at_ms = now + github::DEFAULT_POLL_S * 1000;
            }
            self.inner.lock().github.insert(owner_repo.clone(), state);
            match facts {
                Ok(facts) => {
                    for fact in facts {
                        self.receive_fact(github::fact_event(&key, &fact, roster.as_ref(), now));
                    }
                }
                Err(error) => tracing::debug!(%error, repo = %owner_repo, "team: GitHub poll failed"),
            }
        }
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
            // A background chore: headless when the agent has a headless mode (no window, no
            // folder-trust prompt for the new copy), else a visible pane.
            match serde_json::from_value(actions::run_request(
                &job.project,
                &with_conflicts,
                crate::tui::run_modes(&job.agent_type).headless,
            )) {
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
        let finished = actions::finish_merge(Path::new(&copy.path), &job.conflicts)
            .and_then(|()| actions::check_resolved(Path::new(&copy.path), &job.commit));
        match finished {
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
pub struct MemberView {
    pub id: String,
    pub name: String,
    pub roles: Vec<Role>,
    pub admin: bool,
    pub me: bool,
    pub github: Option<String>,
    pub github_confirmed: bool,
    pub devices: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinView {
    pub id: String,
    pub name: String,
    pub github: Option<String>,
    pub device: String,
    pub received_at_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamView {
    pub enabled: bool,
    pub active: bool,
    pub me: String,
    /// Teammates' names (not me).
    pub members: Vec<String>,
    pub transport: Option<&'static str>,
    /// `live`, `polling` or `offline`.
    pub link: Option<&'static str>,
    pub error: Option<String>,
    /// Messages from teammates known to be missing (gap detection).
    pub missing: u64,
    pub drafts: Vec<Draft>,
    pub cards: Vec<CardView>,
    pub jobs: Vec<Job>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
    pub enabled: bool,
    /// Team id, empty when not on a team.
    pub team: String,
    pub team_name: String,
    pub relay: String,
    pub folder: String,
    pub name: String,
    pub projects: Vec<String>,
    /// `PUPPET_MASTER_PROFILE`, when this is not the default app.
    pub profile: Option<String>,
    /// `team.json` is there but could not be read.
    pub read_error: Option<String>,
    pub error: Option<String>,
    /// Waiting for the admin's yes.
    pub joining: Option<String>,
    pub admin: bool,
    pub members: Vec<MemberView>,
    /// Join requests waiting for my yes or no (admin).
    pub joins: Vec<JoinView>,
    pub epoch: u64,
    /// Where keys are kept ("Windows Credential Manager").
    pub key_store: &'static str,
    pub transport: Option<&'static str>,
    pub link: Option<&'static str>,
}

fn config_view(svc: &Service, inner: &Inner) -> ConfigView {
    let session = inner.session.as_ref();
    let me = session.and_then(|s| s.my_member().ok()).unwrap_or_default();
    let roster = session.map(|s| &s.roster);
    let members = roster
        .map(|r| {
            r.members
                .iter()
                .map(|m| MemberView {
                    id: m.id.clone(),
                    name: m.display.clone(),
                    roles: m.roles.clone(),
                    admin: r.is_admin(&m.id),
                    me: m.id == me,
                    github: m.github.clone(),
                    github_confirmed: m.github_confirmed,
                    devices: m.devices.len(),
                })
                .collect()
        })
        .unwrap_or_default();
    ConfigView {
        enabled: inner.config.enabled,
        team: inner.config.team.clone(),
        team_name: roster.map(|r| r.name.clone()).unwrap_or_else(|| inner.config.name.clone()),
        relay: inner.config.relay.clone(),
        folder: inner.config.folder.clone(),
        name: inner.config.me.name.clone(),
        projects: inner.config.projects.clone(),
        profile: crate::app_paths::profile(),
        read_error: inner.read_error.clone(),
        error: inner.error.clone(),
        joining: inner.config.join.as_ref().map(|j| j.link.name.clone()),
        admin: roster.is_some_and(|r| r.is_admin(&me)),
        members,
        joins: inner
            .store
            .joins
            .iter()
            .map(|p| JoinView {
                id: p.id.clone(),
                name: event::clean_text(&p.request.display, 40),
                github: p.request.github.clone(),
                device: event::clean_text(&p.request.device.name, 40),
                received_at_ms: p.received_at_ms,
            })
            .collect(),
        epoch: roster.map(|r| r.epoch).unwrap_or(0),
        key_store: svc.keystore.label(),
        transport: inner.transport.as_ref().map(|t| t.kind()),
        link: inner.transport.as_ref().map(|t| t.status()),
    }
}

pub fn view() -> Result<TeamView, String> {
    let svc = service()?;
    let (config, store, transport, link, error, names) = {
        let inner = svc.inner.lock();
        let me = inner.session.as_ref().and_then(|s| s.my_member().ok()).unwrap_or_default();
        let names: Vec<String> = inner
            .session
            .as_ref()
            .map(|s| s.roster.members.iter().filter(|m| m.id != me).map(|m| m.display.clone()).collect())
            .unwrap_or_default();
        (
            inner.config.clone(),
            inner.store.clone(),
            inner.transport.as_ref().map(|t| t.kind()),
            inner.transport.as_ref().map(|t| t.status()),
            inner.error.clone().or_else(|| inner.read_error.clone()),
            names,
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
        members: names,
        transport,
        link,
        error,
        missing: store.seq.missing(),
        drafts: store.drafts.iter().rev().cloned().collect(),
        cards,
        jobs: store.jobs.iter().rev().filter(|j| j.state != JobState::Discarded).cloned().collect(),
    })
}

pub fn config_get() -> Result<ConfigView, String> {
    let svc = service()?;
    let inner = svc.inner.lock();
    Ok(config_view(svc, &inner))
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigInput {
    pub enabled: bool,
    #[serde(default)]
    pub projects: Vec<String>,
}

fn save_config(svc: &Service, next: TeamConfig) -> Result<(), String> {
    config::save(&config::path_in(&svc.dir), &next)?;
    let mut inner = svc.inner.lock();
    inner.config = next;
    inner.read_error = None;
    Ok(())
}

/// The Labs switch and the extra project folders.
pub fn config_set(input: ConfigInput) -> Result<ConfigView, String> {
    let svc = service()?;
    let mut next = svc.inner.lock().config.clone();
    next.enabled = input.enabled;
    next.projects = input.projects;
    let next = next.normalized()?;
    save_config(svc, next)?;
    svc.restart();
    config_get()
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInput {
    pub team_name: String,
    pub name: String,
    #[serde(default)]
    pub relay: String,
    #[serde(default)]
    pub folder: String,
}

/// **Create a team**: I am its admin.
pub fn create(input: CreateInput) -> Result<ConfigView, String> {
    let svc = service()?;
    if svc.inner.lock().session.is_some() {
        return Err("This computer is already on a team. Leave it first.".into());
    }
    let mut next = svc.inner.lock().config.clone();
    next.name = input.team_name;
    next.me.name = input.name;
    next.relay = input.relay;
    next.folder = input.folder;
    let mut next = next.normalized()?;
    if next.name.is_empty() {
        return Err("Name the team first.".into());
    }
    if next.me.name.is_empty() {
        return Err("Type your name first; teammates see it on their cards.".into());
    }
    if next.relay.is_empty() == next.folder.is_empty() {
        return Err("Pick a relay or a shared folder.".into());
    }
    let roster = membership::create_team(&next.name, &next.me.name, &svc.keystore, |team| make_transport(&next, team, &svc.keystore), now_ms())?;
    save_roster(&svc.dir, &roster)?;
    next.team = roster.team.clone();
    next.root = roster.root.clone();
    next.salt = roster.salt.clone();
    next.me.id = roster.root.clone();
    next.device = svc.keystore.device_sign().map(|k| super::crypto::key_id(&k))?;
    next.enabled = true;
    next.join = None;
    {
        let mut inner = svc.inner.lock();
        inner.store = TeamStore::default();
        inner.session = Some(Session::new(roster.clone(), svc.keystore.clone()));
        svc.save(&inner);
    }
    save_config(svc, next)?;
    audit::record(Kind::TeamRoleChanged, Who::Desktop, &format!("You created the team {}", roster.name), json!({ "team": roster.team }));
    svc.restart();
    config_get()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteView {
    pub link: String,
    /// `ABCD-EFGH`.
    pub code: String,
    pub expires_at_ms: u64,
}

/// **Invite**: a new link and code (admin).
pub fn create_invite() -> Result<InviteView, String> {
    let svc = service()?;
    let (session, config) = {
        let inner = svc.inner.lock();
        (inner.session.clone().ok_or("Create a team first.")?, inner.config.clone())
    };
    if !session.my_member().is_ok_and(|m| session.roster.is_admin(&m)) {
        return Err("Only the team's admin can invite people.".into());
    }
    let now = now_ms();
    let (inv, code) = invite::new_invite(now);
    svc.keystore.set_invite_code(&config.team, &inv.id, &code)?;
    let link = membership::invite_link(&session.roster, &svc.keystore, Some(&config.relay), Some(&config.folder), &inv.id)?;
    let view = InviteView { link: link.to_url(), code: invite::show_code(&code), expires_at_ms: inv.expires_ms };
    {
        let mut inner = svc.inner.lock();
        inner.store.invites.retain(|i| i.is_open(now));
        inner.store.invites.push(inv);
        inner.last_join_poll_ms = 0;
        svc.save(&inner);
    }
    svc.poke();
    Ok(view)
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinInput {
    pub link: String,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub github: Option<String>,
}

/// **Join a team** with a link and a code; then wait for the admin.
pub fn join(input: JoinInput) -> Result<ConfigView, String> {
    let svc = service()?;
    if svc.inner.lock().session.is_some() {
        return Err("This computer is already on a team. Leave it first.".into());
    }
    let link = InviteLink::parse(&input.link)?;
    let code = invite::normalize_code(&input.code);
    if code.len() != invite::CODE_LEN {
        return Err("The code has 8 letters and digits.".into());
    }
    let name = event::clean_text(&input.name, 40);
    if name.is_empty() {
        return Err("Type your name first; teammates see it on their cards.".into());
    }
    let mut next = svc.inner.lock().config.clone();
    next.relay = link.relay.clone().unwrap_or_default();
    next.folder = link.folder.clone().unwrap_or_default();
    next.me.name = name.clone();
    let mut next = next.normalized()?;
    let transport = make_transport(&next, &link.team, &svc.keystore)?;
    let now = now_ms();
    membership::send_join(&link, &code, &name, input.github.clone(), &svc.keystore, transport.as_ref(), now)?;
    next.team = link.team.clone();
    next.name = link.name.clone();
    next.root = link.root.clone();
    next.salt = link.salt.clone();
    next.me.id = svc.keystore.member().map(|k| super::crypto::key_id(&k))?;
    next.device = svc.keystore.device_sign().map(|k| super::crypto::key_id(&k))?;
    next.github = input.github.filter(|g| !g.trim().is_empty());
    next.enabled = true;
    next.join = Some(config::PendingJoin { link, sent_at_ms: now });
    {
        let mut inner = svc.inner.lock();
        inner.store = TeamStore::default();
        inner.session = None;
        inner.last_join_poll_ms = 0;
        svc.save(&inner);
    }
    save_config(svc, next)?;
    svc.restart();
    config_get()
}

/// Stop waiting for the admin, or leave the team: forget it on this computer.
pub fn leave() -> Result<ConfigView, String> {
    let svc = service()?;
    let (team, epoch) = {
        let inner = svc.inner.lock();
        (inner.config.team.clone(), inner.session.as_ref().map(|s| s.roster.epoch).unwrap_or(1))
    };
    let mut next = svc.inner.lock().config.clone();
    next.team.clear();
    next.name.clear();
    next.root.clear();
    next.salt.clear();
    next.join = None;
    next.enabled = false;
    save_config(svc, next)?;
    svc.keystore.forget_team(&team, epoch);
    let _ = std::fs::remove_file(roster_path(&svc.dir));
    {
        let mut inner = svc.inner.lock();
        inner.session = None;
        inner.store = TeamStore::default();
        svc.save(&inner);
    }
    audit::record(Kind::TeamRoleChanged, Who::Desktop, "You left the team on this computer", json!({ "team": team }));
    svc.restart();
    config_get()
}

fn admin_session(svc: &Service) -> Result<(Session, Arc<dyn TeamTransport>), String> {
    let inner = svc.inner.lock();
    let session = inner.session.clone().ok_or("Create a team first.")?;
    let transport = inner.transport.clone().ok_or("Team is off. Turn it on first.")?;
    if !session.my_member().is_ok_and(|m| session.roster.is_admin(&m)) {
        return Err("Only the team's admin can do that.".into());
    }
    Ok((session, transport))
}

fn next_seq(svc: &Service) -> u64 {
    let mut inner = svc.inner.lock();
    inner.store.next_seq += 1;
    let seq = inner.store.next_seq;
    svc.save(&inner);
    seq
}

/// **Let them in** (admin).
pub fn approve_join(join_id: &str) -> Result<ConfigView, String> {
    let svc = service()?;
    let (mut session, transport) = admin_session(svc)?;
    let pending = svc.inner.lock().store.joins.iter().find(|p| p.id == join_id).cloned().ok_or("That request is gone.")?;
    let now = now_ms();
    membership::approve(&mut session.roster, &pending.request, &svc.keystore, transport.as_ref(), now)?;
    save_roster(&svc.dir, &session.roster)?;
    let seq = next_seq(svc);
    if let Err(error) = membership::announce(&session, seq, transport.as_ref(), now) {
        tracing::warn!(%error, "team: couldn't nudge the team about the new member");
    }
    {
        let mut inner = svc.inner.lock();
        inner.store.joins.retain(|p| p.id != join_id);
        if let Some(i) = inner.store.invites.iter_mut().find(|i| i.id == pending.request.invite) {
            i.used = true;
        }
        inner.session = Some(session.clone());
        svc.save(&inner);
    }
    let team = session.team().to_string();
    let _ = svc.keystore.forget_invite(&team, &pending.request.invite);
    audit::record(
        Kind::TeamJoined,
        Who::Desktop,
        &format!("You let {} into the team", event::clean_text(&pending.request.display, 40)),
        json!({ "member": short_id(&pending.request.member), "github": pending.request.github }),
    );
    svc.changed();
    config_get()
}

/// **No** (admin): the request is dropped and its invite closed.
pub fn decline_join(join_id: &str) -> Result<ConfigView, String> {
    let svc = service()?;
    {
        let mut inner = svc.inner.lock();
        let Some(pending) = inner.store.joins.iter().find(|p| p.id == join_id).cloned() else {
            return Err("That request is gone.".into());
        };
        inner.store.joins.retain(|p| p.id != join_id);
        if let Some(i) = inner.store.invites.iter_mut().find(|i| i.id == pending.request.invite) {
            i.used = true;
        }
        svc.save(&inner);
    }
    svc.changed();
    config_get()
}

/// **Remove** a member (admin). The team key rotates.
pub fn remove_member(member_id: &str) -> Result<ConfigView, String> {
    let svc = service()?;
    let (mut session, transport) = admin_session(svc)?;
    let name = session.roster.member(member_id).map(|m| m.display.clone()).ok_or("They aren't on the team.")?;
    let now = now_ms();
    let epoch = membership::remove(&mut session.roster, member_id, &svc.keystore, transport.as_ref(), now)?;
    save_roster(&svc.dir, &session.roster)?;
    svc.inner.lock().session = Some(session.clone());
    let seq = next_seq(svc);
    if let Err(error) = membership::announce(&session, seq, transport.as_ref(), now) {
        tracing::warn!(%error, "team: couldn't nudge the team about the removal");
    }
    audit::record(Kind::TeamRoleChanged, Who::Desktop, &format!("You removed {name} from the team"), json!({ "member": short_id(member_id) }));
    audit::record(Kind::TeamKeyRotated, Who::Desktop, "The team key was replaced", json!({ "epoch": epoch }));
    svc.changed();
    config_get()
}

/// "I've seen it": forget the missing-updates warning.
pub fn clear_gaps() -> Result<(), String> {
    let svc = service()?;
    let mut inner = svc.inner.lock();
    inner.store.seq.clear_gaps();
    svc.save(&inner);
    drop(inner);
    svc.changed();
    Ok(())
}

/// **Tell the team**: seal and send the draft, with my note. The one place a push leaves this
/// computer.
pub fn send(draft_id: &str, note: Option<String>) -> Result<(), String> {
    let svc = service()?;
    let (mut draft, transport, session) = {
        let mut inner = svc.inner.lock();
        let transport = inner.transport.clone().ok_or("Team is off. Turn it on in Settings first.")?;
        let session = inner.session.clone().ok_or("Join a team first.")?;
        let draft = inner.store.take_draft(draft_id).ok_or("That update was already sent or dismissed.")?;
        (draft, transport, session)
    };
    draft.event.note = note.map(|n| event::clean_text(&n, event::MAX_NOTE_CHARS)).filter(|n| !n.is_empty());
    draft.event.at_ms = now_ms();
    draft.event.team = session.team().to_string();
    let seq = next_seq(svc);
    let sent = session
        .seal(&Payload::Event { event: draft.event.clone() }, seq, now_ms())
        .and_then(|env| transport.send(&env).map_err(|e| e.0));
    if let Err(error) = sent {
        let mut inner = svc.inner.lock();
        inner.store.add_draft(draft);
        return Err(error);
    }
    tracing::info!(event = %draft.event.id, "team.sent");
    audit::record(
        Kind::TeamSent,
        Who::Desktop,
        &format!("You told the team: {}", draft.event.kind.as_str().replace('_', " ")),
        json!({ "event": draft.event.id, "kind": draft.event.kind.as_str() }),
    );
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

fn audit_action(what: &str, detail: serde_json::Value, who: &Who) {
    audit::record(Kind::TeamAction, who.clone(), what, detail);
}

/// **Pull now** (fast-forward only).
pub fn pull(card_id: &str) -> Result<String, String> {
    pull_as(card_id, &Who::Desktop)
}

pub fn pull_as(card_id: &str, who: &Who) -> Result<String, String> {
    let svc = service()?;
    let (card, repo) = card_and_repo(svc, card_id)?;
    let message = actions::pull_now(&repo, &card.event).map_err(refusal_text)?;
    tracing::info!(card = %card_id, "team.action pull");
    audit_action("Pulled a teammate's push (fast-forward)", json!({ "card": card_id }), who);
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
    queue_as(card_id, pane_id, when, &Who::Desktop)
}

pub fn queue_as(card_id: &str, pane_id: &str, when: &str, who: &Who) -> Result<Job, String> {
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
    audit_action("Queued an agent to bring a teammate's push into a separate copy", json!({ "card": card_id, "job": job.id }), who);
    let mut inner = svc.inner.lock();
    inner.store.add_job(job.clone());
    if let Some(c) = inner.store.card_mut(card_id) {
        c.state = CardState::Queued;
    }
    svc.save(&inner);
    drop(inner);
    svc.changed();
    svc.poke();
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
    apply_as(job_id, &Who::Desktop)
}

pub fn apply_as(job_id: &str, who: &Who) -> Result<String, String> {
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
    audit_action(&format!("Applied a queued update to {} (fast-forward, nothing pushed)", job.into), json!({ "job": job_id }), who);
    svc.update_job(job_id, |j| {
        j.state = JobState::Applied;
        j.message = Some(message.clone());
    });
    Ok(message)
}

/// **Discard**: delete the separate copy and its branch.
pub fn discard(job_id: &str) -> Result<(), String> {
    discard_as(job_id, &Who::Desktop)
}

pub fn discard_as(job_id: &str, who: &Who) -> Result<(), String> {
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
    audit_action("Discarded a queued update", json!({ "job": job_id }), who);
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

/// What the "Merge when green" sheet shows. Reads only.
pub fn merge_plan(card_id: &str) -> Result<merge_green::MergePlan, String> {
    let svc = service()?;
    let card = svc.inner.lock().store.card(card_id).cloned().ok_or("That card is gone.")?;
    merge_green::plan(svc.gh.as_ref(), &card.event)
}

/// **Merge when green**, after the sheet: arm GitHub's auto-merge. Never merges by itself.
pub fn merge_when_green(card_id: &str) -> Result<String, String> {
    let svc = service()?;
    let card = svc.inner.lock().store.card(card_id).cloned().ok_or("That card is gone.")?;
    let plan = merge_green::plan(svc.gh.as_ref(), &card.event)?;
    let message = merge_green::arm(svc.gh.as_ref(), &plan)?;
    audit_action(
        &format!("Armed GitHub auto-merge for #{} in {}", plan.number, plan.repo),
        json!({ "card": card_id, "pr": plan.number, "repo": plan.repo, "method": plan.method }),
        &Who::Desktop,
    );
    Ok(message)
}
