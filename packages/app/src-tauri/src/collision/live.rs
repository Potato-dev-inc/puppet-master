//! Collision Alert: the watch thread, the snapshot and the state the commands read (SPEC-n33
//! sections 3.7, 3.8 and 5 CA-13).
//!
//! One thread, `collision-watch`, wakes every second. It reads the pane list (one registry lock,
//! held only for the copy), turns it into `PaneFact`s, and runs one `Engine::step` with the
//! commands queued since the last second. The step may run git, and so may take a while. Nobody
//! waits for it: the windows read `views()`, which is an `RwLock` read of the last published
//! snapshot, and `ignore` only touches the snapshot and the queue.
//!
//! What a step produces is published here and nowhere else:
//! - the snapshot (`views()`), without any alert the person has just ignored;
//! - the Ready-lane attention item for `Clash` and `SharedFolder` alerts (key `collision:<id>`,
//!   `Kind::Review`, no actions). Resolved or ignored alerts clear it. The engine gives no change
//!   for an ignore, so this layer clears the item itself;
//! - the `collision://changed` event (payload `{}`; the window calls `collision_list`).
//!
//! Lock order, everywhere: `inbox` first, then `snapshot`. The watch thread holds neither while it
//! runs a step.
//!
//! Errors from `ignore` and `set_settings` are already `CODE: message` strings (`COLLISION_NOT_FOUND`,
//! `COLLISION_SAVE_FAILED`). The id check (`COLLISION_BAD_ID`) is the command layer's (CA-14).

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use parking_lot::{Mutex, RwLock};

use crate::attention::model::{Kind, Raised, Resolution};
use crate::attention::copy::{headings, PaneName};
use crate::pty::{PaneInfo, PaneRegistry};
use crate::worktrees::{self, WorktreeRef};

use super::engine::{Change, Cmd, Engine, Out};
use super::model::{
    AlertView, FileHunks, PaneFact, PaneRun, RepoFact, Sample, Settings, REPO_CACHE_MS,
};
use super::probe::{GitProbe, Probe, ProbeError};
use super::settings::{self, Store};
use super::view;

/// The gap between two steps of the watch thread.
const TICK: Duration = Duration::from_millis(1000);
/// Queued commands are bounded: a flood of taps cannot grow the queue without limit.
const MAX_INBOX: usize = 64;
/// The prefix of an attention key. The rest is the alert id.
const KEY_PREFIX: &str = "collision:";
const ERR_NOT_FOUND: &str = "COLLISION_NOT_FOUND";
const ERR_SAVE_FAILED: &str = "COLLISION_SAVE_FAILED";

// ---- the shared state the commands read -----------------------------------------------------------

/// What the watch thread publishes and the commands read.
pub struct Shared {
    /// The last published alerts, newest first. Written by the watch thread after a step and by `ignore`.
    snapshot: RwLock<Vec<AlertView>>,
    /// Commands for the next step.
    inbox: Mutex<Vec<Cmd>>,
}

impl Shared {
    pub fn new() -> Shared {
        Shared { snapshot: RwLock::new(Vec::new()), inbox: Mutex::new(Vec::new()) }
    }

    /// The last snapshot. Never waits for a look.
    pub fn views(&self) -> Vec<AlertView> {
        self.snapshot.read().clone()
    }

    /// Removes the alert from the snapshot at once and queues `Cmd::Ignore` for the next step.
    /// An id that is not in the snapshot changes nothing.
    pub fn ignore(&self, id: &str) -> Result<(), String> {
        let mut inbox = self.inbox.lock();
        let mut snapshot = self.snapshot.write();
        let Some(at) = snapshot.iter().position(|v| v.id == id) else {
            return Err(format!("{ERR_NOT_FOUND}: That heads-up is not here any more."));
        };
        snapshot.remove(at);
        push_bounded(&mut inbox, Cmd::Ignore(id.to_string()));
        Ok(())
    }

    /// Queues the switch for the next step. Switching off empties the snapshot at once.
    pub fn set_enabled(&self, enabled: bool) {
        let mut inbox = self.inbox.lock();
        let mut snapshot = self.snapshot.write();
        if !enabled {
            snapshot.clear();
        }
        push_bounded(&mut inbox, Cmd::SetEnabled(enabled));
    }

    /// The commands queued since the last step.
    fn take_cmds(&self) -> Vec<Cmd> {
        std::mem::take(&mut *self.inbox.lock())
    }

    /// Ids ignored after the last `take_cmds` (the engine has not seen them yet).
    fn pending_ignores(&self) -> BTreeSet<String> {
        ignored_ids(&self.inbox.lock())
    }

    /// Publishes a step's alerts, minus any the person ignored while the step ran.
    fn publish(&self, mut views: Vec<AlertView>) {
        let inbox = self.inbox.lock();
        let pending = ignored_ids(&inbox);
        views.retain(|v| !pending.contains(&v.id));
        *self.snapshot.write() = views;
    }
}

fn push_bounded(inbox: &mut Vec<Cmd>, cmd: Cmd) {
    if inbox.len() >= MAX_INBOX {
        inbox.remove(0);
    }
    inbox.push(cmd);
}

fn ignored_ids(cmds: &[Cmd]) -> BTreeSet<String> {
    cmds.iter()
        .filter_map(|cmd| match cmd {
            Cmd::Ignore(id) => Some(id.clone()),
            Cmd::SetEnabled(_) => None,
        })
        .collect()
}

static SHARED: Lazy<Shared> = Lazy::new(Shared::new);

/// The alerts for the window (`collision_list`): the last snapshot, newest first, hidden ones left out.
pub fn views() -> Vec<AlertView> {
    SHARED.views()
}

/// `collision_ignore` (after its id check): the alert leaves the snapshot now; the engine and the
/// phone item follow within a second. Unknown id: `COLLISION_NOT_FOUND`.
pub fn ignore(id: &str) -> Result<(), String> {
    SHARED.ignore(id)
}

/// `collision_settings_get`: the saved switch (on by default).
pub fn settings() -> Settings {
    settings::load()
}

/// `collision_settings_set`: saves `collision.json` (`COLLISION_SAVE_FAILED`), then queues the switch.
pub fn set_settings(next: Settings) -> Result<Settings, String> {
    set_settings_in(&Store::at(&settings_file()), &SHARED, next)
}

fn set_settings_in(store: &Store, shared: &Shared, next: Settings) -> Result<Settings, String> {
    store.save(&next).map_err(|err| format!("{ERR_SAVE_FAILED}: {err}"))?;
    shared.set_enabled(next.enabled);
    Ok(next)
}

/// The same file `settings::load` reads (`<app data>/collision.json`; a per-process temp file in tests),
/// so a save is always seen by the next read.
fn settings_file() -> PathBuf {
    settings::default_file()
}

// ---- panes -> facts -----------------------------------------------------------------------------------

/// The agent panes as the engine sees them. Shells, protocol panes and companions are left out.
/// Headings are numbered over ALL panes, as the sidebar does ("Name 2").
pub fn pane_facts(
    infos: &[PaneInfo],
    label_of: &dyn Fn(&str) -> String,
    is_agent: &dyn Fn(&str) -> bool,
) -> Vec<PaneFact> {
    let names: Vec<PaneName> = infos
        .iter()
        .map(|p| PaneName {
            id: p.id.clone(),
            agent_type: p.agent_type.clone(),
            created_at: p.created_at,
            title: p.title.clone(),
        })
        .collect();
    let heading_by_id = headings(&names, |agent| label_of(agent));
    let mut shown: Vec<&PaneInfo> = infos
        .iter()
        .filter(|p| !crate::companion::service::is_pane_id(&p.id) && is_agent(&p.agent_type))
        .collect();
    shown.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
    shown
        .into_iter()
        .map(|p| PaneFact {
            pane_id: p.id.clone(),
            agent_id: p.agent_type.clone(),
            label: heading_by_id.get(&p.id).cloned().unwrap_or_else(|| label_of(&p.agent_type)),
            cwd: p.cwd.clone(),
            run: if p.status == "running" { PaneRun::Working } else { PaneRun::Resting },
            created_ms: u64::try_from(p.created_at).unwrap_or(0),
        })
        .collect()
}

/// The display name of an agent type (its descriptor label). An unknown type shows as typed.
fn label_of(agent_type: &str) -> String {
    crate::tui::descriptor_for(agent_type).map(|d| d.label).unwrap_or_else(|| agent_type.to_string())
}

// ---- the real probe ---------------------------------------------------------------------------------------

/// `GitProbe` plus the one fact only this layer knows: a separate copy's start commit, from the
/// worktree records of its project.
pub struct LiveProbe {
    git: GitProbe,
    /// Checkout root -> (when it was read, its start commit). A miss is cached too.
    starts: RefCell<HashMap<String, (Instant, Option<String>)>>,
}

impl LiveProbe {
    pub fn new(git: GitProbe) -> LiveProbe {
        LiveProbe { git, starts: RefCell::new(HashMap::new()) }
    }
}

impl Probe for LiveProbe {
    fn repo_of(&self, cwd: &str) -> Option<RepoFact> {
        self.git.repo_of(cwd)
    }

    fn start_of(&self, repo: &RepoFact) -> Option<String> {
        if !repo.linked {
            return None;
        }
        if let Some((read_at, start)) = self.starts.borrow().get(&repo.root) {
            if read_at.elapsed() < Duration::from_millis(REPO_CACHE_MS) {
                return start.clone();
            }
        }
        let start = worktrees::origin_of_cwd(&repo.root).and_then(|origin| {
            let refs: Vec<WorktreeRef> = worktrees::list(&origin).into_iter().map(|entry| entry.worktree).collect();
            start_from(&refs, &repo.root)
        });
        self.starts.borrow_mut().insert(repo.root.clone(), (Instant::now(), start.clone()));
        start
    }

    fn sample(&self, root: &str, start: &str) -> Result<Sample, ProbeError> {
        #[cfg(feature = "dev-fake-tui")]
        PROBE_CALLS.fetch_add(1, Ordering::SeqCst);
        self.git.sample(root, start)
    }

    fn hunks(&self, root: &str, start: &str, rel: &str) -> Result<FileHunks, ProbeError> {
        #[cfg(feature = "dev-fake-tui")]
        PROBE_CALLS.fetch_add(1, Ordering::SeqCst);
        self.git.hunks(root, start, rel)
    }
}

/// How many times the live probe ran git (a sample or a hunk read), for the end-to-end tests: a count that
/// stops growing proves no git process is started any more.
#[cfg(feature = "dev-fake-tui")]
static PROBE_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The live probe's git calls so far (`dev-fake-tui` builds only).
#[cfg(feature = "dev-fake-tui")]
pub fn probe_calls_for_e2e() -> u64 {
    PROBE_CALLS.load(Ordering::SeqCst)
}

/// The start commit of the copy whose checkout is `root`: its recorded `base_commit`.
pub fn start_from(copies: &[WorktreeRef], root: &str) -> Option<String> {
    let wanted = path_key(root);
    copies
        .iter()
        .find(|copy| copy.closed.is_none() && path_key(&copy.path) == wanted)
        .map(|copy| copy.base_commit.trim().to_string())
        .filter(|commit| !commit.is_empty())
}

/// Paths compare without slash style, a trailing slash, and (on Windows) letter case.
fn path_key(path: &str) -> String {
    let flat = path.replace('\\', "/");
    let trimmed = flat.trim_end_matches('/');
    if cfg!(windows) {
        trimmed.to_lowercase()
    } else {
        trimmed.to_string()
    }
}

/// `<data>/collision/empty-hooks`: PM's own empty private folder, which `harden::git_env` points
/// git's hooks at.
pub fn hooks_dir(data_dir: &Path) -> std::io::Result<PathBuf> {
    let dir = data_dir.join("collision").join("empty-hooks");
    crate::secure_file::create_private_dir(&dir)?;
    Ok(dir)
}

// ---- publishing ---------------------------------------------------------------------------------------------

/// Where a step's results go. The real one writes to `attention` and emits the event; tests record.
pub trait Sink {
    fn raise(&self, item: Raised);
    fn clear(&self, key: &str, resolution: Resolution);
    /// The alerts changed: tell the window.
    fn changed(&self);
}

struct LiveSink {
    app: tauri::AppHandle,
}

impl Sink for LiveSink {
    fn raise(&self, item: Raised) {
        crate::attention::raise(item);
    }

    fn clear(&self, key: &str, resolution: Resolution) {
        crate::attention::clear(key, Some(resolution));
    }

    fn changed(&self) {
        // Not in unit tests: emitting through the app handle needs the windowing runtime.
        #[cfg(not(test))]
        {
            use tauri::Emitter;
            let _ = self.app.emit("collision://changed", serde_json::json!({}));
        }
        #[cfg(test)]
        let _ = &self.app;
    }
}

/// One thing to do to the attention list.
#[derive(Debug)]
enum Act {
    Raise(Raised),
    Clear { key: String, state: &'static str },
}

fn attention_key(id: &str) -> String {
    format!("{KEY_PREFIX}{id}")
}

/// Only these levels reach the phone. The quiet `same_file` row stays on the desktop.
fn is_loud(view: &AlertView) -> bool {
    matches!(view.level.as_str(), "clash" | "shared_folder")
}

/// What to do to the attention list after one step. `raised` is the set of ids that have an item
/// now; it is updated here. `cmds` are the commands the step consumed, `pending` the ignores
/// queued while it ran.
fn plan(
    out: &Out,
    views: &[AlertView],
    cmds: &[Cmd],
    pending: &BTreeSet<String>,
    raised: &mut BTreeSet<String>,
) -> Vec<Act> {
    let mut acts = Vec::new();
    // The engine gives no change for an ignore: the item is withdrawn here.
    for id in ignored_ids(cmds) {
        if raised.remove(&id) {
            acts.push(Act::Clear { key: attention_key(&id), state: "withdrawn" });
        }
    }
    for change in &out.changes {
        match change {
            Change::Resolved(id) => {
                if raised.remove(id) {
                    acts.push(Act::Clear { key: attention_key(id), state: "gone" });
                }
            }
            Change::Raised(id) | Change::Updated(id) => {
                // Ignored while the step ran: the next step withdraws it. Do not raise it now.
                if pending.contains(id) {
                    continue;
                }
                let Some(view) = views.iter().find(|v| &v.id == id) else { continue };
                if is_loud(view) {
                    let (title, sentence) = view::attention_title_sentence(view);
                    raised.insert(id.clone());
                    acts.push(Act::Raise(Raised {
                        key: attention_key(id),
                        kind: Kind::Review,
                        pane_id: None,
                        title,
                        sentence,
                        expires_at_ms: None,
                        actions: Vec::new(),
                    }));
                } else if raised.remove(id) {
                    // A clash that became a quiet row: the phone item goes.
                    acts.push(Act::Clear { key: attention_key(id), state: "gone" });
                }
            }
        }
    }
    acts
}

fn resolution(state: &str, now_ms: u64) -> Resolution {
    Resolution {
        state: state.to_string(),
        by: None,
        choice_label: None,
        reason: None,
        at_ms: now_ms,
        device_name: None,
        device_id: None,
    }
}

/// Redaction for every string that reaches a screen: known secrets and token shapes.
fn clean(raw: &str) -> String {
    crate::redact::text(raw).into_inner()
}

// ---- the watch ------------------------------------------------------------------------------------------------

/// The owner of the engine. One per process, on the `collision-watch` thread.
pub struct Watch {
    engine: Engine,
    /// Alert ids that have an attention item right now.
    raised: BTreeSet<String>,
}

impl Watch {
    pub fn new() -> Watch {
        Watch { engine: Engine::new(), raised: BTreeSet::new() }
    }

    /// One step: the queued commands, the engine, then the snapshot, the phone items and the event.
    pub fn tick(&mut self, shared: &Shared, sink: &dyn Sink, panes: &[PaneFact], probe: &dyn Probe, now_ms: u64) {
        let cmds = shared.take_cmds();
        let out = self.engine.step(panes, &cmds, probe, now_ms);
        let views: Vec<AlertView> = out.alerts.iter().map(|alert| view::view(alert, &clean)).collect();
        let pending = shared.pending_ignores();
        for act in plan(&out, &views, &cmds, &pending, &mut self.raised) {
            match act {
                Act::Raise(item) => sink.raise(item),
                Act::Clear { key, state } => sink.clear(&key, resolution(state, now_ms)),
            }
        }
        shared.publish(views);
        if !out.changes.is_empty() || !cmds.is_empty() {
            sink.changed();
        }
    }
}

/// Starts the watch thread. Call once at startup (`pane_activity::init` does). A second call does nothing.
pub fn start(registry: Arc<Mutex<PaneRegistry>>, app: tauri::AppHandle) {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let hooks = match hooks_dir(&crate::app_paths::app_data_dir()) {
        Ok(dir) => dir,
        Err(err) => {
            // Without its private hooks folder no git call is safe, so the feature stays off.
            tracing::warn!(error = %err, "collision alert is off: could not create its hooks folder");
            return;
        }
    };
    if !settings::load().enabled {
        SHARED.set_enabled(false);
    }
    let spawned = std::thread::Builder::new().name("collision-watch".into()).spawn(move || {
        let probe = LiveProbe::new(GitProbe { hooks });
        let sink = LiveSink { app };
        let mut watch = Watch::new();
        loop {
            std::thread::sleep(TICK);
            // No git on this machine: nothing to watch.
            if !crate::checkpoints::git_available() {
                SHARED.take_cmds();
                continue;
            }
            let infos = registry.lock().list();
            let panes = pane_facts(&infos, &label_of, &|agent| crate::tui::is_agent_backend(agent));
            watch.tick(&SHARED, &sink, &panes, &probe, crate::attention::now_ms());
        }
    });
    if let Err(err) = spawned {
        tracing::warn!(error = %err, "collision alert is off: could not start its thread");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::model::{poll_ms, AlertId, TouchKind, Touched};
    use crate::collision::probe::FakeProbe;
    use std::sync::mpsc;

    const ROOT_A: &str = r"C:\Users\you\copy-a";
    const ROOT_B: &str = r"C:\Users\you\copy-b";
    const A: &str = "fake_a";
    const B: &str = "fake_b";
    const AUTH: &str = "src/auth.ts";

    fn info(id: &str, agent: &str, created_at: i64, status: &str, title: Option<&str>) -> PaneInfo {
        PaneInfo {
            id: id.into(),
            agent_type: agent.into(),
            pid: 0,
            status: status.into(),
            created_at,
            last_output_at: None,
            cwd: r"C:\Users\you\demo".into(),
            cols: 80,
            rows: 24,
            viewer_held: false,
            reprints_on_resize: false,
            account: None,
            account_label: None,
            title: title.map(Into::into),
            parent_pane_id: None,
        }
    }

    fn label(agent: &str) -> String {
        match agent {
            "fake_agent" => "Fake Agent".into(),
            other => other.into(),
        }
    }

    fn is_agent(agent: &str) -> bool {
        agent != "shell"
    }

    fn pane(id: &str, root: &str, run: PaneRun, label: &str) -> PaneFact {
        PaneFact {
            pane_id: id.to_string(),
            agent_id: "fake_agent".to_string(),
            label: label.to_string(),
            cwd: root.to_string(),
            run,
            created_ms: 1_000,
        }
    }

    fn world(roots: &[&str]) -> FakeProbe {
        let probe = FakeProbe::new();
        for root in roots {
            probe.set_repo(root, RepoFact { key: "repo".to_string(), root: root.to_string(), linked: false });
            probe.set_start(root, "s1");
        }
        probe
    }

    fn touched(path: &str) -> Touched {
        Touched { path: path.to_string(), kind: TouchKind::Modified, mtime_ms: Some(5_000) }
    }

    fn look(files: Vec<Touched>) -> Sample {
        Sample { head: "h1".to_string(), files, truncated: false, cost_ms: 50 }
    }

    fn lines(start: u32, end: u32) -> FileHunks {
        FileHunks { spans: vec![crate::collision::model::Span { start, end }], binary: false, cut: false }
    }

    /// Two copies changing the same lines of `src/auth.ts`: a clash once the first look has run.
    fn clash_world() -> FakeProbe {
        let probe = world(&[ROOT_A, ROOT_B]);
        for root in [ROOT_A, ROOT_B] {
            probe.push_sample(root, Ok(look(vec![touched(AUTH)])));
            probe.set_hunks(root, AUTH, Ok(lines(50, 50)));
        }
        probe
    }

    fn two_panes() -> [PaneFact; 2] {
        [pane(A, ROOT_A, PaneRun::Working, "Fake Agent 1"), pane(B, ROOT_B, PaneRun::Working, "Fake Agent 2")]
    }

    fn pair_id() -> String {
        AlertId::of(&[A, B])
    }

    /// What the sink was asked to do, in order.
    #[derive(Default)]
    struct Recorder {
        log: RefCell<Vec<String>>,
        items: RefCell<Vec<Raised>>,
    }

    impl Sink for Recorder {
        fn raise(&self, item: Raised) {
            self.log.borrow_mut().push(format!("raise {}", item.key));
            self.items.borrow_mut().push(item);
        }
        fn clear(&self, key: &str, resolution: Resolution) {
            self.log.borrow_mut().push(format!("clear {key} {}", resolution.state));
        }
        fn changed(&self) {
            self.log.borrow_mut().push("changed".to_string());
        }
    }

    impl Recorder {
        fn take(&self) -> Vec<String> {
            std::mem::take(&mut *self.log.borrow_mut())
        }
    }

    /// Runs the clash world until the alert is raised. Returns the watch, shared state and sink.
    fn raised_clash() -> (Watch, Shared, Recorder, FakeProbe) {
        let (first, _) = poll_ms();
        let probe = clash_world();
        let (shared, sink, mut watch) = (Shared::new(), Recorder::default(), Watch::new());
        let panes = two_panes();
        watch.tick(&shared, &sink, &panes, &probe, 0);
        watch.tick(&shared, &sink, &panes, &probe, first);
        (watch, shared, sink, probe)
    }

    // ------------------------------------------------------------------------ panes -> facts

    #[test]
    fn panes_become_facts_without_shells_and_companions() {
        let infos = vec![
            info("p-1", "fake_agent", 100, "running", None),
            info("p-shell", "shell", 110, "running", None),
            info("companion-1", "fake_agent", 120, "idle", None),
            // A wizard-style pane id (no fixed prefix) is a pane like any other.
            info("wiz-7f3a", "fake_agent", 130, "waiting_input", Some("Review the auth change")),
        ];
        let facts = pane_facts(&infos, &label, &is_agent);
        let ids: Vec<&str> = facts.iter().map(|f| f.pane_id.as_str()).collect();
        assert_eq!(ids, ["p-1", "wiz-7f3a"], "oldest first; shell and companion left out");
        assert_eq!(facts[0].run, PaneRun::Working);
        assert_eq!(facts[1].run, PaneRun::Resting, "waiting_input is resting");
        assert_eq!(facts[0].created_ms, 100);
        assert_eq!(facts[0].agent_id, "fake_agent");
        assert_eq!(facts[0].cwd, r"C:\Users\you\demo");
        // Numbering counts every pane of the agent, the companion included, as the sidebar does.
        assert_eq!(facts[0].label, "Fake Agent 1");
        assert_eq!(facts[1].label, "Review the auth change", "the person's title wins");
    }

    #[test]
    fn every_non_running_status_is_resting() {
        for status in ["idle", "waiting_input", "error", "", "something_new"] {
            let facts = pane_facts(&[info("p", "fake_agent", 1, status, None)], &label, &is_agent);
            assert_eq!(facts[0].run, PaneRun::Resting, "{status}");
        }
        let facts = pane_facts(&[info("p", "fake_agent", -5, "running", None)], &label, &is_agent);
        assert_eq!(facts[0].created_ms, 0, "a negative time is 0");
    }

    // ------------------------------------------------------------------------ the snapshot

    /// A probe whose look is slow, so the step is busy for a while.
    struct SlowProbe {
        inner: FakeProbe,
        started: mpsc::Sender<()>,
        delay: Duration,
    }

    impl Probe for SlowProbe {
        fn repo_of(&self, cwd: &str) -> Option<RepoFact> {
            self.inner.repo_of(cwd)
        }
        fn start_of(&self, repo: &RepoFact) -> Option<String> {
            self.inner.start_of(repo)
        }
        fn sample(&self, root: &str, start: &str) -> Result<Sample, ProbeError> {
            let _ = self.started.send(());
            std::thread::sleep(self.delay);
            self.inner.sample(root, start)
        }
        fn hunks(&self, root: &str, start: &str, rel: &str) -> Result<FileHunks, ProbeError> {
            self.inner.hunks(root, start, rel)
        }
    }

    #[test]
    fn the_snapshot_never_waits_for_a_running_look() {
        let (first, _) = poll_ms();
        let shared = Arc::new(Shared::new());
        let (tx, rx) = mpsc::channel();
        let delay = Duration::from_millis(600);
        let slow = SlowProbe { inner: clash_world(), started: tx, delay };
        let worker_shared = shared.clone();
        let worker = std::thread::spawn(move || {
            let (sink, mut watch) = (Recorder::default(), Watch::new());
            let panes = two_panes();
            watch.tick(&worker_shared, &sink, &panes, &slow, 0);
            watch.tick(&worker_shared, &sink, &panes, &slow, first);
        });
        // The look has begun (inside the probe, off the snapshot's locks).
        rx.recv_timeout(Duration::from_secs(10)).expect("the look started");
        let began = Instant::now();
        assert!(shared.views().is_empty());
        assert!(shared.ignore("ca-000000000000").is_err());
        assert!(began.elapsed() < delay / 2, "views() and ignore() answered while the look was running");
        worker.join().unwrap();
        assert_eq!(shared.views().len(), 1, "the finished look published its alert");
    }

    #[test]
    fn a_clash_is_published_and_raised_as_one_ready_item() {
        let (_, shared, sink, _) = raised_clash();
        let views = shared.views();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].id, pair_id());
        assert_eq!(views[0].level, "clash");
        let key = format!("collision:{}", pair_id());
        assert_eq!(sink.take(), [format!("raise {key}"), "changed".to_string()]);
    }

    #[test]
    fn the_phone_item_is_a_plain_ready_lane_review_with_no_buttons() {
        let (_, shared, sink, _) = raised_clash();
        let items = sink.items.borrow();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].key, format!("collision:{}", pair_id()));
        assert_eq!(items[0].kind, Kind::Review);
        assert!(items[0].actions.is_empty());
        assert_eq!(items[0].pane_id, None);
        assert_eq!(items[0].expires_at_ms, None);
        let view = &shared.views()[0];
        let (title, sentence) = view::attention_title_sentence(view);
        assert_eq!(items[0].title, title);
        assert_eq!(items[0].sentence, sentence);
        assert!(items[0].title.contains(AUTH));
    }

    #[test]
    fn a_quiet_row_is_in_the_snapshot_but_never_raises_an_item() {
        let (first, _) = poll_ms();
        let probe = world(&[ROOT_A, ROOT_B]);
        for (root, at) in [(ROOT_A, 10), (ROOT_B, 80)] {
            probe.push_sample(root, Ok(look(vec![touched(AUTH)])));
            probe.set_hunks(root, AUTH, Ok(lines(at, at)));
        }
        let (shared, sink, mut watch) = (Shared::new(), Recorder::default(), Watch::new());
        let panes = two_panes();
        watch.tick(&shared, &sink, &panes, &probe, 0);
        watch.tick(&shared, &sink, &panes, &probe, first);
        let views = shared.views();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].level, "same_file");
        assert_eq!(sink.take(), ["changed"], "the window is told; the phone is not");
    }

    #[test]
    fn a_closed_pane_clears_the_item_as_gone() {
        let (mut watch, shared, sink, probe) = raised_clash();
        sink.take();
        let only_a = [pane(A, ROOT_A, PaneRun::Working, "Fake Agent 1")];
        watch.tick(&shared, &sink, &only_a, &probe, 10_000);
        assert!(shared.views().is_empty());
        assert_eq!(sink.take(), [format!("clear collision:{} gone", pair_id()), "changed".to_string()]);
    }

    // ------------------------------------------------------------------------ ignore

    #[test]
    fn ignore_removes_from_the_snapshot_at_once() {
        let (_, shared, _, _) = raised_clash();
        assert_eq!(shared.views().len(), 1);
        shared.ignore(&pair_id()).expect("known id");
        assert!(shared.views().is_empty(), "gone before the next step");
        // A second ignore, or a forged id, is not found and changes nothing.
        let err = shared.ignore(&pair_id()).unwrap_err();
        assert!(err.starts_with("COLLISION_NOT_FOUND: "), "{err}");
        assert!(shared.ignore("ca-ffffffffffff").unwrap_err().starts_with("COLLISION_NOT_FOUND: "));
        assert_eq!(shared.inbox.lock().len(), 1, "only the real ignore was queued");
    }

    #[test]
    fn ignore_withdraws_the_phone_item_on_the_next_step() {
        let (mut watch, shared, sink, probe) = raised_clash();
        sink.take();
        shared.ignore(&pair_id()).unwrap();
        watch.tick(&shared, &sink, &two_panes(), &probe, 9_500);
        assert_eq!(sink.take(), [format!("clear collision:{} withdrawn", pair_id()), "changed".to_string()]);
        assert!(shared.views().is_empty());
        assert!(watch.raised.is_empty());
        // The ignore stays: later looks of the same clash do not bring it back.
        let (_, regular) = poll_ms();
        watch.tick(&shared, &sink, &two_panes(), &probe, 9_500 + regular + 20_000);
        assert!(shared.views().is_empty());
        assert!(sink.take().is_empty());
    }

    #[test]
    fn an_alert_ignored_while_a_step_runs_is_not_raised_or_published() {
        let (first, _) = poll_ms();
        let panes = two_panes();
        // The ignore arrives while the engine is busy, after `take_cmds` and before the publish.
        let sink = Recorder::default();
        let shared = Shared::new();
        let mut watch = Watch::new();
        let probe = clash_world();
        watch.tick(&shared, &sink, &panes, &probe, 0);
        let cmds = shared.take_cmds();
        let out = watch.engine.step(&panes, &cmds, &probe, first);
        shared.inbox.lock().push(Cmd::Ignore(pair_id()));
        let views: Vec<AlertView> = out.alerts.iter().map(|a| view::view(a, &clean)).collect();
        let pending = shared.pending_ignores();
        let acts = plan(&out, &views, &cmds, &pending, &mut watch.raised);
        assert!(acts.is_empty(), "no phone item for an alert that was just ignored: {acts:?}");
        shared.publish(views);
        assert!(shared.views().is_empty(), "the published snapshot leaves it out");
        // The next step hands the ignore to the engine; there is nothing to withdraw.
        watch.tick(&shared, &sink, &panes, &probe, first + 1_000);
        assert!(shared.views().is_empty());
        assert!(sink.log.borrow().iter().all(|line| !line.starts_with("raise") && !line.starts_with("clear")));
    }

    // ------------------------------------------------------------------------ the switch

    #[test]
    fn off_at_the_start_publishes_nothing_and_looks_at_nothing() {
        let probe = clash_world();
        let (shared, sink, mut watch) = (Shared::new(), Recorder::default(), Watch::new());
        shared.set_enabled(false);
        let panes = two_panes();
        for now in (0..=60_000u64).step_by(1_000) {
            watch.tick(&shared, &sink, &panes, &probe, now);
        }
        assert!(shared.views().is_empty());
        assert_eq!(probe.calls().sample, 0);
        assert_eq!(probe.calls().repo_of, 0);
        // The only thing sent is the first "changed" for the switch itself.
        assert_eq!(sink.take(), ["changed"]);
    }

    #[test]
    fn turning_it_off_clears_the_item_and_then_publishes_nothing() {
        let (mut watch, shared, sink, probe) = raised_clash();
        sink.take();
        shared.set_enabled(false);
        assert!(shared.views().is_empty(), "the snapshot is empty at once");
        let panes = two_panes();
        watch.tick(&shared, &sink, &panes, &probe, 10_000);
        assert_eq!(sink.take(), [format!("clear collision:{} gone", pair_id()), "changed".to_string()]);
        let sample_calls = probe.calls().sample;
        for now in (11_000..=120_000u64).step_by(1_000) {
            watch.tick(&shared, &sink, &panes, &probe, now);
        }
        assert!(sink.take().is_empty(), "nothing is published while off");
        assert_eq!(probe.calls().sample, sample_calls, "nothing is looked at while off");

        // Back on: it works again.
        shared.set_enabled(true);
        watch.tick(&shared, &sink, &panes, &probe, 130_000);
        assert_eq!(sink.take(), ["changed"]);
    }

    #[test]
    fn saving_the_switch_writes_the_file_and_queues_the_command() {
        let dir = std::env::temp_dir().join(format!("pm-collision-live-{}-save", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::at(&dir.join("collision.json"));
        let shared = Shared::new();
        let saved = set_settings_in(&store, &shared, Settings { enabled: false }).expect("saved");
        assert_eq!(saved, Settings { enabled: false });
        assert_eq!(store.load(), Settings { enabled: false });
        assert_eq!(shared.take_cmds(), vec![Cmd::SetEnabled(false)]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_save_is_a_coded_error_and_queues_nothing() {
        let dir = std::env::temp_dir().join(format!("pm-collision-live-{}-fail", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // The "folder" of the file is a file, so the save cannot succeed.
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, "x").unwrap();
        let store = Store::at(&blocker.join("collision.json"));
        let shared = Shared::new();
        let err = set_settings_in(&store, &shared, Settings { enabled: false }).unwrap_err();
        assert!(err.starts_with("COLLISION_SAVE_FAILED: "), "{err}");
        assert!(shared.take_cmds().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_inbox_is_bounded() {
        let shared = Shared::new();
        for _ in 0..(MAX_INBOX * 3) {
            shared.set_enabled(true);
        }
        assert_eq!(shared.take_cmds().len(), MAX_INBOX);
    }

    // ------------------------------------------------------------------------ privacy

    #[test]
    fn a_secret_in_a_pane_heading_never_reaches_the_snapshot_or_the_phone_item() {
        let (first, _) = poll_ms();
        let probe = clash_world();
        let secret = "sk-ant-api03-GrepTestSecret0123456789";
        let panes = [
            pane(A, ROOT_A, PaneRun::Working, &format!("Fix {secret}")),
            pane(B, ROOT_B, PaneRun::Working, "Fake Agent 2"),
        ];
        let (shared, sink, mut watch) = (Shared::new(), Recorder::default(), Watch::new());
        watch.tick(&shared, &sink, &panes, &probe, 0);
        watch.tick(&shared, &sink, &panes, &probe, first);
        let json = serde_json::to_string(&shared.views()).unwrap();
        assert!(!json.contains("GrepTestSecret"), "{json}");
        let items = sink.items.borrow();
        assert_eq!(items.len(), 1);
        let item = format!("{} {}", items[0].title, items[0].sentence.clone().unwrap_or_default());
        assert!(!item.contains("GrepTestSecret"), "{item}");
    }

    // ------------------------------------------------------------------------ the probe's pieces

    fn copy(path: &str, base_commit: &str) -> WorktreeRef {
        WorktreeRef {
            path: path.to_string(),
            cwd: path.to_string(),
            branch: "pm/demo".to_string(),
            base: "main".to_string(),
            base_commit: base_commit.to_string(),
            repo: r"C:\Users\you\demo".to_string(),
            handle: String::new(),
            created_at_ms: 0,
            closed: None,
            changes: None,
            setup: None,
            pr: None,
        }
    }

    #[test]
    fn a_copys_start_is_its_recorded_base_commit() {
        let copies = vec![copy(r"C:\Users\you\demo\.puppet-master\worktrees\a1", "abc123"), copy("/home/you/x/b2", "def456")];
        // Slash style, a trailing slash and (on Windows) letter case do not matter.
        assert_eq!(
            start_from(&copies, "C:/Users/you/demo/.puppet-master/worktrees/a1/").as_deref(),
            Some("abc123")
        );
        assert_eq!(start_from(&copies, "/home/you/x/b2").as_deref(), Some("def456"));
        assert_eq!(start_from(&copies, r"C:\Users\you\demo\.puppet-master\worktrees\zz"), None);
        let blank = vec![copy("/p/c", "  ")];
        assert_eq!(start_from(&blank, "/p/c"), None, "an empty base commit is no start");
        let mut closed = copy("/p/d", "abc123");
        closed.closed = Some("merged".to_string());
        assert_eq!(start_from(&[closed], "/p/d"), None, "a merged or discarded copy has no checkout");
    }

    #[test]
    fn a_plain_checkout_has_no_recorded_start() {
        let probe = LiveProbe::new(GitProbe { hooks: PathBuf::from("hooks") });
        let repo = RepoFact { key: "k".into(), root: r"C:\Users\you\demo".into(), linked: false };
        assert_eq!(probe.start_of(&repo), None);
        // A linked folder that is not under a PM copies folder has no origin either.
        let linked = RepoFact { key: "k".into(), root: r"C:\Users\you\elsewhere".into(), linked: true };
        assert_eq!(probe.start_of(&linked), None);
    }

    #[test]
    fn the_hooks_folder_is_made_under_the_data_folder() {
        let dir = std::env::temp_dir().join(format!("pm-collision-live-{}-hooks", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let made = hooks_dir(&dir).expect("created");
        assert_eq!(made, dir.join("collision").join("empty-hooks"));
        assert!(made.is_dir());
        assert_eq!(std::fs::read_dir(&made).unwrap().count(), 0, "empty");
        // Making it again is fine.
        assert_eq!(hooks_dir(&dir).unwrap(), made);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
