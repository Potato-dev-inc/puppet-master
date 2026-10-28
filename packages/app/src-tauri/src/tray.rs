//! Tray icon and its menu (panes-first PF2). The menu model is pure data built from the agent
//! registry listing and the open panes, so it is unit-tested without a window system; the Tauri
//! menu is just a rendering of it.

use crate::pane_jobs::Job;
use crate::ui_mode::UiMode;
use serde_json::Value;
use std::time::Duration;

/// The tooltip when nothing is going on, and the prefix of every tooltip.
const TOOLTIP_NAME: &str = "Puppet Master";

/// Tray tooltip: `Puppet Master · 2 need you · 1 error · 3 working · 1 server running`. `need_you` is
/// the attention count (`attention::counts().blocking`), the same number the Inbox shows; the other
/// parts come from pane statuses and background jobs (a job that listens on a port is a server).
/// Parts with 0 are left out. Only the words, no agent names. Standby (hidden) panes must be left
/// out of `statuses` and `jobs` by the caller.
pub fn summary_line(statuses: &[&str], jobs: &[Job], need_you: u32) -> String {
    let errors = statuses.iter().filter(|s| **s == "error").count();
    let working = statuses.iter().filter(|s| **s == "running").count();
    let servers = jobs.iter().filter(|j| !j.ports.is_empty()).count();
    let mut parts: Vec<String> = Vec::new();
    if need_you > 0 {
        parts.push(format!("{need_you} {}", if need_you == 1 { "needs you" } else { "need you" }));
    }
    if errors > 0 {
        parts.push(format!("{errors} {}", if errors == 1 { "error" } else { "errors" }));
    }
    if working > 0 {
        parts.push(format!("{working} working"));
    }
    if servers > 0 {
        parts.push(format!("{servers} {} running", if servers == 1 { "server" } else { "servers" }));
    }
    if parts.is_empty() {
        parts.push("nothing running".into());
    }
    format!("{TOOLTIP_NAME} \u{b7} {}", parts.join(" \u{b7} "))
}

/// The tooltip while a Live Room is on (paused too): the person must always see that a link is open.
pub const LIVE_TOOLTIP: &str = "Puppet Master \u{b7} LIVE";

/// The tray tooltip: `LIVE_TOOLTIP` while a Live Room is on, else `summary`.
pub fn tooltip_for(live: bool, summary: String) -> String {
    if live {
        LIVE_TOOLTIP.to_string()
    } else {
        summary
    }
}

/// How a refresh that asks at `since_last` after the previous one is handled.
#[derive(Debug, PartialEq, Eq)]
pub enum Throttle {
    /// Run now.
    Now,
    /// Run once the window closes, after this wait.
    Later(Duration),
    /// A run is already waiting for the window to close; it will read the newest state.
    Skip,
}

/// The tray refreshes at most once per second.
pub const REFRESH_EVERY: Duration = Duration::from_secs(1);

pub fn throttle(since_last: Option<Duration>, pending: bool) -> Throttle {
    match since_last {
        Some(elapsed) if elapsed < REFRESH_EVERY => {
            if pending {
                Throttle::Skip
            } else {
                Throttle::Later(REFRESH_EVERY - elapsed)
            }
        }
        _ => Throttle::Now,
    }
}

/// One entry of the tray menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Action { id: String, label: String, enabled: bool },
    /// A check item (the Mode submenu).
    Check { id: String, label: String, checked: bool },
    Submenu { label: String, items: Vec<Item> },
    Separator,
}

/// An open pane as the menu needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneRow {
    pub id: String,
    pub agent: String,
    /// Human agent label ("Codex"); empty falls back to `agent`.
    pub label: String,
    /// Auto or user title ("auth refactor"), when the pane has one.
    pub title: Option<String>,
}

pub const ID_SETTINGS: &str = "settings";
pub const ID_QUIT: &str = "quit";
pub const ID_COCKPIT: &str = "cockpit";
pub const ID_STOP_ALL: &str = "stop-all";
pub const PREFIX_MODE: &str = "mode:";
pub const PREFIX_NEW: &str = "new:";
pub const PREFIX_PANE: &str = "pane:";

/// What a clicked menu id means.
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Settings,
    Quit,
    Cockpit,
    StopAll,
    SetMode(UiMode),
    NewPane(String),
    FocusPane(String),
}

pub fn parse_command(id: &str) -> Option<Command> {
    match id {
        ID_SETTINGS => Some(Command::Settings),
        ID_QUIT => Some(Command::Quit),
        ID_COCKPIT => Some(Command::Cockpit),
        ID_STOP_ALL => Some(Command::StopAll),
        _ if id.starts_with(PREFIX_MODE) => UiMode::parse(&id[PREFIX_MODE.len()..]).map(Command::SetMode),
        _ => id
            .strip_prefix(PREFIX_NEW)
            .map(|a| Command::NewPane(a.to_string()))
            .or_else(|| id.strip_prefix(PREFIX_PANE).map(|p| Command::FocusPane(p.to_string()))),
    }
}

/// What a second launch of the executable asks for, from its command-line arguments.
#[derive(Debug, PartialEq, Eq)]
pub enum SecondLaunch {
    /// Plain relaunch: open the Settings window.
    Settings,
    /// `--new-pane <agent>`: open just that pane, no Settings window.
    NewPane(String),
    /// A sign-in start (`--background`) while the app already runs: show nothing and take no focus.
    Quiet,
}

pub fn second_launch_action(args: &[String]) -> SecondLaunch {
    if crate::autostart::is_background_start(args) {
        return SecondLaunch::Quiet;
    }
    args.iter()
        .position(|a| a == "--new-pane")
        .and_then(|i| args.get(i + 1))
        .filter(|a| !a.is_empty() && !a.starts_with("--"))
        .map(|a| SecondLaunch::NewPane(a.clone()))
        .unwrap_or(SecondLaunch::Settings)
}

/// `agents` is the `agents` array of `GET /agents/types`. An agent that can open a visible pane is
/// an enabled entry; a headless-only one stays listed but disabled.
pub fn new_pane_items(agents: &[Value], ready: &[String]) -> Vec<Item> {
    let mut items = Vec::new();
    for agent in agents {
        let Some(id) = agent.get("id").and_then(Value::as_str) else { continue };
        let label = agent.get("label").and_then(Value::as_str).unwrap_or(id);
        let visible = agent.pointer("/modes/visible").and_then(Value::as_bool).unwrap_or(false);
        let headless = agent.pointer("/modes/headless").and_then(Value::as_bool).unwrap_or(false);
        if visible {
            // A Preload worker is waiting for this provider: opening it is instant.
            let label = if ready.iter().any(|r| r == id) { format!("{label} (ready)") } else { label.to_string() };
            items.push(Item::Action { id: format!("{PREFIX_NEW}{id}"), label, enabled: true });
        } else if headless {
            items.push(Item::Action {
                id: format!("{PREFIX_NEW}{id}"),
                label: format!("{label} (headless only)"),
                enabled: false,
            });
        }
    }
    if items.is_empty() {
        items.push(Item::Action { id: "none".into(), label: "(no agents)".into(), enabled: false });
    }
    items
}

pub fn pane_items(panes: &[PaneRow]) -> Vec<Item> {
    if panes.is_empty() {
        return vec![Item::Action { id: "none".into(), label: "(no open panes)".into(), enabled: false }];
    }
    panes
        .iter()
        .map(|p| Item::Action {
            id: format!("{PREFIX_PANE}{}", p.id),
            label: match &p.title {
                Some(title) => format!("{} \u{b7} {}", if p.label.is_empty() { &p.agent } else { &p.label }, title),
                None => format!("{} \u{b7} {}", p.agent, p.id.chars().take(8).collect::<String>()),
            },
            enabled: true,
        })
        .collect()
}

/// "Mode" submenu: one check item per mode, the current one checked.
pub fn mode_items(mode: UiMode) -> Vec<Item> {
    [(UiMode::Orchestration, "Orchestration"), (UiMode::Individual, "Individual")]
        .into_iter()
        .map(|(m, label)| Item::Check { id: format!("{PREFIX_MODE}{}", m.as_str()), label: label.into(), checked: m == mode })
        .collect()
}

/// `pane://spawn-stage` payload. `stage` is claiming | spawning | attached | ready | failed.
pub fn spawn_stage_payload(agent_id: &str, pane_id: Option<&str>, stage: &str, warm: bool, message: Option<&str>) -> Value {
    let mut v = serde_json::json!({ "agent_id": agent_id, "pane_id": pane_id, "stage": stage, "warm": warm });
    if let Some(message) = message {
        v["message"] = Value::String(message.into());
    }
    v
}

/// How long after attach a cold pane is called ready even if it printed nothing yet.
pub const READY_GRACE_MS: u64 = 1500;

#[derive(Debug, PartialEq, Eq)]
pub enum ReadyDecision {
    Wait,
    Ready,
    Failed,
}

/// Readiness of a freshly attached cold pane. A pane that is still listed is never "failed": it is
/// ready on first output or after the grace period, regardless of its status.
pub fn ready_decision(alive: bool, has_output: bool, elapsed_ms: u64) -> ReadyDecision {
    if !alive {
        ReadyDecision::Failed
    } else if has_output || elapsed_ms >= READY_GRACE_MS {
        ReadyDecision::Ready
    } else {
        ReadyDecision::Wait
    }
}

pub fn build_model(agents: &[Value], panes: &[PaneRow], ready: &[String], mode: UiMode) -> Vec<Item> {
    vec![
        Item::Submenu { label: "New pane".into(), items: new_pane_items(agents, ready) },
        Item::Submenu { label: "Panes".into(), items: pane_items(panes) },
        Item::Separator,
        Item::Action { id: ID_COCKPIT.into(), label: "Open cockpit".into(), enabled: true },
        Item::Submenu { label: "Mode".into(), items: mode_items(mode) },
        Item::Action { id: ID_STOP_ALL.into(), label: "Stop all agents".into(), enabled: true },
        Item::Action { id: ID_SETTINGS.into(), label: "Settings\u{2026}".into(), enabled: true },
        Item::Action { id: ID_QUIT.into(), label: "Quit Puppet Master".into(), enabled: true },
    ]
}

mod runtime {
    use super::*;
    use crate::pane_jobs::PaneRef;
    use crate::AppState;
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;
    use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use tauri::{AppHandle, Listener, Manager, Wry};

    const TRAY_ID: &str = "puppet-master-tray";

    fn render_items(app: &AppHandle, items: &[Item]) -> tauri::Result<Vec<Box<dyn IsMenuItem<Wry>>>> {
        let mut out: Vec<Box<dyn IsMenuItem<Wry>>> = Vec::new();
        for item in items {
            match item {
                Item::Action { id, label, enabled } => {
                    out.push(Box::new(MenuItem::with_id(app, id, label, *enabled, None::<&str>)?))
                }
                Item::Check { id, label, checked } => {
                    out.push(Box::new(CheckMenuItem::with_id(app, id, label, true, *checked, None::<&str>)?))
                }
                Item::Separator => out.push(Box::new(PredefinedMenuItem::separator(app)?)),
                Item::Submenu { label, items } => {
                    let children = render_items(app, items)?;
                    let refs: Vec<&dyn IsMenuItem<Wry>> = children.iter().map(|c| c.as_ref()).collect();
                    out.push(Box::new(Submenu::with_items(app, label, true, &refs)?));
                }
            }
        }
        Ok(out)
    }

    fn current_model(app: &AppHandle) -> Vec<Item> {
        let listing = crate::tui::agent_types_listing();
        let agents = listing.get("agents").and_then(Value::as_array).cloned().unwrap_or_default();
        let labels: std::collections::HashMap<String, String> = agents
            .iter()
            .filter_map(|a| Some((a.get("id")?.as_str()?.to_string(), a.get("label")?.as_str()?.to_string())))
            .collect();
        let panes: Vec<PaneRow> = app
            .try_state::<AppState>()
            .map(|s| {
                s.registry
                    .lock()
                    .list()
                    .into_iter()
                    .map(|p| PaneRow {
                        label: labels.get(&p.agent_type).cloned().unwrap_or_default(),
                        id: p.id,
                        agent: p.agent_type,
                        title: p.title,
                    })
                    .collect()
            })
            .unwrap_or_default();
        build_model(&agents, &panes, &crate::standby::ready_agents(), crate::ui_mode::current())
    }

    fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
        let items = render_items(app, &current_model(app))?;
        let refs: Vec<&dyn IsMenuItem<Wry>> = items.iter().map(|c| c.as_ref()).collect();
        Menu::with_items(app, &refs)
    }

    /// Last refresh time and whether a deferred refresh is already waiting (see `throttle`).
    struct RefreshGate {
        last: Option<Instant>,
        pending: bool,
    }

    static GATE: Mutex<RefreshGate> = Mutex::new(RefreshGate { last: None, pending: false });

    fn gate() -> std::sync::MutexGuard<'static, RefreshGate> {
        GATE.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Rebuild the menu and the tooltip so they reflect the current panes, agents and jobs. At most
    /// once per second; a call inside that window runs once more when it closes, so the last state
    /// always lands.
    pub fn refresh(app: &AppHandle) {
        let decision = {
            let mut gate = gate();
            let decision = throttle(gate.last.map(|t| t.elapsed()), gate.pending);
            match decision {
                Throttle::Now => gate.last = Some(Instant::now()),
                Throttle::Later(_) => gate.pending = true,
                Throttle::Skip => {}
            }
            decision
        };
        match decision {
            Throttle::Now => refresh_now(app),
            Throttle::Skip => {}
            Throttle::Later(wait) => {
                let app = app.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(wait);
                    {
                        let mut gate = gate();
                        gate.pending = false;
                        gate.last = Some(Instant::now());
                    }
                    refresh_now(&app);
                });
            }
        }
    }

    /// The app handle, kept so code that has no handle (the attention worker) can ask for a refresh.
    static APP: OnceLock<AppHandle> = OnceLock::new();

    /// Ask for a refresh from anywhere in the app (e.g. the attention count changed). A no-op until the
    /// tray is set up.
    pub fn request_refresh() {
        if let Some(app) = APP.get() {
            refresh(app);
        }
    }

    fn refresh_now(app: &AppHandle) {
        let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
        match build_menu(app) {
            Ok(menu) => {
                let _ = tray.set_menu(Some(menu));
            }
            Err(e) => tracing::warn!(error = %e, "tray menu rebuild failed"),
        }
        let _ = tray.set_tooltip(Some(tooltip_for(crate::live_room::service::is_live(), current_summary(app))));
    }

    /// The tooltip for the panes and jobs open now. Standby (hidden) panes are not the person's work.
    fn current_summary(app: &AppHandle) -> String {
        let need_you = crate::attention::counts().blocking;
        let Some(state) = app.try_state::<AppState>() else { return summary_line(&[], &[], need_you) };
        let panes: Vec<_> = state.registry.lock().list().into_iter().filter(|p| !crate::pty::hidden::is_hidden(&p.id)).collect();
        let statuses: Vec<&str> = panes.iter().map(|p| p.status.as_str()).collect();
        let refs: Vec<PaneRef> = panes
            .iter()
            .map(|p| PaneRef { id: p.id.clone(), agent_type: p.agent_type.clone(), pid: p.pid })
            .collect();
        let jobs: Vec<Job> = crate::pane_jobs::jobs_for_panes(&refs).into_values().flatten().collect();
        summary_line(&statuses, &jobs, need_you)
    }

    /// Open a visible pane for `agent` in the default project folder and pop it out. A Preload worker
    /// for that provider is claimed when one is warm (its hidden window is just shown); otherwise
    /// the pane is spawned cold. A person asked for it, so the window takes focus.
    pub fn new_pane(app: &AppHandle, agent: &str) -> Result<String, String> {
        new_pane_for(app, agent, crate::cockpit::Origin::Tray)
    }

    fn emit_stage(app: &AppHandle, agent: &str, pane_id: Option<&str>, stage: &str, warm: bool, message: Option<&str>) {
        use tauri::Emitter;
        let _ = app.emit("pane://spawn-stage", spawn_stage_payload(agent, pane_id, stage, warm, message));
    }

    /// Emit `ready` once the attached pane has produced output or `READY_GRACE_MS` has passed,
    /// whichever is first. Never waits for idle (agents stay "running" while they start). `failed`
    /// only if the pane vanished from the registry. Runs on its own thread.
    fn watch_until_ready(app: &AppHandle, agent: &str, pane_id: &str) {
        let (app, agent, pane_id) = (app.clone(), agent.to_string(), pane_id.to_string());
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            loop {
                let registry = app.state::<AppState>().registry.clone();
                let alive = registry.lock().list().iter().any(|p| p.id == pane_id);
                let has_output = alive
                    && crate::pty::registry_read_snapshot(&registry, &pane_id).map(|t| !t.trim().is_empty()).unwrap_or(false);
                match ready_decision(alive, has_output, started.elapsed().as_millis() as u64) {
                    ReadyDecision::Wait => std::thread::sleep(std::time::Duration::from_millis(50)),
                    ReadyDecision::Ready => return emit_stage(&app, &agent, Some(&pane_id), "ready", false, None),
                    ReadyDecision::Failed => {
                        return emit_stage(&app, &agent, Some(&pane_id), "failed", false, Some("the agent exited right after it started"))
                    }
                }
            }
        });
    }

    /// Open a pane for `agent` as `origin` asked: warm standby first, else cold; shown by UI mode
    /// (`cockpit::present_pane`). Emits `pane://spawn-stage` events along the way.
    pub fn new_pane_for(app: &AppHandle, agent: &str, origin: crate::cockpit::Origin) -> Result<String, String> {
        new_pane_with(app, agent, origin, None, false)
    }

    /// `new_pane_for` under a user account (`crate::accounts`): the pane's agent is pointed at that
    /// account's folder, and only a warm worker started for the same account is claimed.
    pub fn new_pane_for_account(
        app: &AppHandle,
        agent: &str,
        origin: crate::cockpit::Origin,
        account: Option<&str>,
    ) -> Result<String, String> {
        new_pane_with(app, agent, origin, account, false)
    }

    /// `new_pane_for_account`, optionally `isolated`: the pane starts in its own git worktree (a
    /// cold spawn, never a warm worker, which was launched in the project folder).
    pub fn new_pane_with(
        app: &AppHandle,
        agent: &str,
        origin: crate::cockpit::Origin,
        account: Option<&str>,
        isolated: bool,
    ) -> Result<String, String> {
        new_pane_in(app, agent, origin, account, isolated, None)
    }

    /// `new_pane_with` in a chosen project folder (the "+" menu's project chip) instead of the
    /// default project. A warm worker belongs to the default project, so it is not claimed then.
    pub fn new_pane_in(
        app: &AppHandle,
        agent: &str,
        origin: crate::cockpit::Origin,
        account: Option<&str>,
        isolated: bool,
        project: Option<&str>,
    ) -> Result<String, String> {
        emit_stage(app, agent, None, "claiming", false, None);
        let result = open_new_pane(app, agent, origin, account, isolated, project);
        if let Err(e) = &result {
            emit_stage(app, agent, None, "failed", false, Some(e));
        }
        result
    }

    fn open_new_pane(
        app: &AppHandle,
        agent: &str,
        origin: crate::cockpit::Origin,
        account: Option<&str>,
        isolated: bool,
        chosen_project: Option<&str>,
    ) -> Result<String, String> {
        use tauri::Emitter;
        let started = std::time::Instant::now();
        let state = app.state::<AppState>();
        // The folder the person selected, exactly (canonical; a separate copy's folder maps back to its
        // project). Without a copy the agent starts here, and nowhere under `.puppet-master`.
        let chosen = crate::project_path::resolve_selected_folder(chosen_project)?;
        let default_project = crate::standby::default_project(&state.registry);
        // The cockpit always names the project its chip shows; naming the default project is not
        // a different project (a warm worker can still be claimed).
        let chosen = chosen.filter(|p| {
            crate::project_path::comparable_project_path(std::path::Path::new(p))
                != crate::project_path::comparable_project_path(std::path::Path::new(&default_project))
        });
        let project = chosen.clone().unwrap_or(default_project);
        let account = account.map(str::trim).filter(|a| !a.is_empty());
        let key = if isolated || chosen.is_some() {
            None
        } else {
            crate::standby::preload_key(agent, &project).map(|key| key.with_account(account))
        };
        let claim = key.as_ref().and_then(crate::standby::claim_visible);
        if let Some(key) = &key {
            // Counts as a use of this provider: lifts idle/backoff holds and warms a replacement.
            crate::standby::note_run(key, &project);
        }
        if let Some(claim) = claim {
            let warm_ms = claim.age_ms();
            let pane_id = claim.commit();
            let info = state.registry.lock().list().into_iter().find(|p| p.id == pane_id);
            let (cols, rows) = info.map(|p| (p.cols, p.rows)).unwrap_or((120, 30));
            crate::cockpit::present_pane(app, &pane_id, agent, cols, rows, origin);
            let _ = app.emit("pty://panes-changed", ());
            emit_stage(app, agent, Some(&pane_id), "attached", true, None);
            emit_stage(app, agent, Some(&pane_id), "ready", true, None);
            tracing::info!(%agent, pane = %pane_id, warm_ms, took_ms = started.elapsed().as_millis() as u64, "tray: opened new pane from a warm worker");
            return Ok(pane_id);
        }
        emit_stage(app, agent, None, "spawning", false, None);
        let spawn_dir = if isolated {
            let handle = format!("pane-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
            if crate::worktrees::preset::load(std::path::Path::new(&project)).is_ok_and(|p| !p.is_empty()) {
                emit_stage(app, agent, None, "spawning", false, Some("Setting up the copy"));
            }
            let worktree = crate::worktrees::create(&project, &handle)
                .map_err(|e| format!("{}: {}", e.code, e.message))?;
            if let Some(setup) = &worktree.setup {
                emit_stage(app, agent, None, "spawning", false, Some(&setup.sentence()));
            }
            worktree.cwd
        } else {
            project.clone()
        };
        let pane_id = crate::pty::registry_spawn_pane(
            &state.registry,
            app,
            crate::pty::SpawnPaneArgs {
                agent_type: agent.to_string(),
                cwd: Some(spawn_dir),
                cols: None,
                rows: None,
                extra_args: None,
                pane_id: None,
                read_only: false,
                account: account.map(str::to_string),
                guard_env: None,
            },
        )?;
        let info = state.registry.lock().list().into_iter().find(|p| p.id == pane_id);
        let (cols, rows) = info.map(|p| (p.cols, p.rows)).unwrap_or((120, 30));
        crate::cockpit::present_pane(app, &pane_id, agent, cols, rows, origin);
        let _ = app.emit("pty://panes-changed", ());
        emit_stage(app, agent, Some(&pane_id), "attached", false, None);
        watch_until_ready(app, agent, &pane_id);
        tracing::info!(%agent, pane = %pane_id, took_ms = started.elapsed().as_millis() as u64, "tray: opened new pane cold");
        Ok(pane_id)
    }

    fn handle(app: &AppHandle, id: &str) {
        match parse_command(id) {
            Some(Command::Settings) => crate::cockpit::open_settings(app, None),
            Some(Command::Quit) => crate::pane_windows::request_quit(app),
            Some(Command::Cockpit) => crate::cockpit::open_cockpit(app, None),
            Some(Command::StopAll) => {
                let app = app.clone();
                std::thread::spawn(move || {
                    crate::panic::stop_all_and_show(&app);
                });
            }
            Some(Command::SetMode(mode)) => {
                if let Err(e) = crate::cockpit::set_mode(app, mode) {
                    tracing::warn!(error = %e, "tray: mode switch failed");
                }
            }
            Some(Command::NewPane(agent)) => {
                let app = app.clone();
                std::thread::spawn(move || match new_pane(&app, &agent) {
                    Ok(id) => tracing::info!(%agent, pane = %id, "tray: opened new pane"),
                    Err(e) => tracing::warn!(%agent, error = %e, "tray: new pane failed"),
                });
            }
            Some(Command::FocusPane(pane_id)) => {
                let app = app.clone();
                std::thread::spawn(move || {
                    let found = app
                        .state::<AppState>()
                        .registry
                        .lock()
                        .list()
                        .into_iter()
                        .find(|p| p.id == pane_id)
                        .map(|p| (p.agent_type, p.cols, p.rows));
                    if let Some((agent, cols, rows)) = found {
                        match crate::ui_mode::current() {
                            UiMode::Individual => {
                                crate::app_windows::open_pane_window(&app, &pane_id, &agent, cols, rows, false)
                            }
                            UiMode::Orchestration => crate::cockpit::open_cockpit(&app, Some(&pane_id)),
                        }
                    }
                });
            }
            None => {}
        }
    }

    pub fn init(app: &AppHandle) -> tauri::Result<()> {
        let _ = APP.set(app.clone());
        let menu = build_menu(app)?;
        let mut builder = TrayIconBuilder::with_id(TRAY_ID)
            .tooltip("Puppet Master")
            .menu(&menu)
            .show_menu_on_left_click(false)
            .on_menu_event(|app, event| handle(app, event.id().as_ref()))
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    crate::cockpit::open_primary_window(tray.app_handle());
                }
            });
        if let Some(icon) = app.default_window_icon() {
            builder = builder.icon(icon.clone());
        }
        builder.build(app)?;
        tracing::info!("tray icon created (tooltip \"Puppet Master\")");
        for event in ["pty://panes-changed", "pty://status", "agent_definitions_changed", crate::standby::READY_EVENT, "ui://mode-changed"] {
            let handle = app.clone();
            app.listen(event, move |_| refresh(&handle));
        }
        Ok(())
    }
}

pub use runtime::{init, new_pane, new_pane_for, new_pane_for_account, new_pane_in, new_pane_with, request_refresh};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn agent(id: &str, label: &str, visible: bool, headless: bool) -> Value {
        json!({"id": id, "label": label, "modes": {"visible": visible, "headless": headless}})
    }

    #[test]
    fn visible_agents_are_enabled_and_headless_only_ones_are_disabled_and_marked() {
        let items = new_pane_items(
            &[
                agent("kilo", "Kilo", true, true),
                agent("kiro", "Kiro", false, true),
                agent("ghost", "Ghost", false, false),
            ],
            &[],
        );
        assert_eq!(items.len(), 2, "an agent with no mode at all is omitted");
        assert_eq!(items[0], Item::Action { id: "new:kilo".into(), label: "Kilo".into(), enabled: true });
        assert_eq!(
            items[1],
            Item::Action { id: "new:kiro".into(), label: "Kiro (headless only)".into(), enabled: false }
        );
    }

    #[test]
    fn second_launch_with_new_pane_opens_only_the_pane() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(second_launch_action(&a(&["app.exe"])), SecondLaunch::Settings);
        assert_eq!(
            second_launch_action(&a(&["app.exe", "--new-pane", "claude"])),
            SecondLaunch::NewPane("claude".into())
        );
        assert_eq!(second_launch_action(&a(&["app.exe", "--new-pane"])), SecondLaunch::Settings);
        assert_eq!(
            second_launch_action(&a(&["app.exe", "--new-pane", "--other"])),
            SecondLaunch::Settings
        );
    }

    #[test]
    fn a_sign_in_second_launch_is_quiet_and_never_opens_settings() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(second_launch_action(&a(&["app.exe", "--background"])), SecondLaunch::Quiet);
        // Quiet wins over a pane request, so a sign-in start never raises a window.
        assert_eq!(
            second_launch_action(&a(&["app.exe", "--new-pane", "claude", "--background"])),
            SecondLaunch::Quiet
        );
    }

    #[test]
    fn agents_with_a_warm_worker_are_marked_ready() {
        let items = new_pane_items(
            &[agent("claude", "Claude Code", true, true), agent("kilo", "Kilo", true, true)],
            &["kilo".to_string()],
        );
        assert_eq!(items[0], Item::Action { id: "new:claude".into(), label: "Claude Code".into(), enabled: true });
        assert_eq!(items[1], Item::Action { id: "new:kilo".into(), label: "Kilo (ready)".into(), enabled: true });
    }

    #[test]
    fn empty_lists_show_a_disabled_placeholder() {
        assert!(matches!(&new_pane_items(&[], &[])[0], Item::Action { enabled: false, .. }));
        assert!(matches!(&pane_items(&[])[0], Item::Action { enabled: false, .. }));
    }

    #[test]
    fn panes_are_listed_with_agent_and_short_id() {
        let items = pane_items(&[PaneRow { id: "abcdef012345".into(), agent: "kilo".into(), label: "Kilo".into(), title: None }]);
        assert_eq!(
            items,
            vec![Item::Action { id: "pane:abcdef012345".into(), label: "kilo \u{b7} abcdef01".into(), enabled: true }]
        );
    }

    #[test]
    fn a_titled_pane_is_listed_with_agent_label_and_title() {
        let items = pane_items(&[PaneRow {
            id: "abcdef012345".into(),
            agent: "codex".into(),
            label: "Codex".into(),
            title: Some("auth refactor".into()),
        }]);
        assert_eq!(
            items,
            vec![Item::Action { id: "pane:abcdef012345".into(), label: "Codex \u{b7} auth refactor".into(), enabled: true }]
        );
    }

    #[test]
    fn model_has_the_top_level_entries_in_order() {
        let model = build_model(&[agent("kilo", "Kilo", true, true)], &[], &[], UiMode::Orchestration);
        assert!(matches!(&model[0], Item::Submenu { label, .. } if label == "New pane"));
        assert!(matches!(&model[1], Item::Submenu { label, .. } if label == "Panes"));
        assert_eq!(model[2], Item::Separator);
        assert!(matches!(&model[3], Item::Action { id, label, .. } if id == "cockpit" && label == "Open cockpit"));
        assert!(matches!(&model[4], Item::Submenu { label, .. } if label == "Mode"));
        assert!(matches!(&model[5], Item::Action { id, label, .. } if id == "stop-all" && label == "Stop all agents"));
        assert!(matches!(&model[6], Item::Action { id, .. } if id == "settings"));
        assert!(matches!(&model[7], Item::Action { id, label, .. } if id == "quit" && label == "Quit Puppet Master"));
    }

    #[test]
    fn mode_submenu_checks_exactly_the_current_mode() {
        for mode in [UiMode::Orchestration, UiMode::Individual] {
            let checked: Vec<_> = mode_items(mode)
                .into_iter()
                .filter_map(|i| match i {
                    Item::Check { id, checked: true, .. } => Some(id),
                    _ => None,
                })
                .collect();
            assert_eq!(checked, vec![format!("mode:{}", mode.as_str())]);
        }
        let model = build_model(&[], &[], &[], UiMode::Individual);
        let Item::Submenu { items, .. } = &model[4] else { panic!("mode submenu") };
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn a_listed_pane_is_never_failed_and_readiness_ignores_status() {
        assert_eq!(ready_decision(true, false, 0), ReadyDecision::Wait);
        assert_eq!(ready_decision(true, true, 10), ReadyDecision::Ready);
        assert_eq!(ready_decision(true, false, READY_GRACE_MS), ReadyDecision::Ready);
        assert_eq!(ready_decision(true, false, 600_000), ReadyDecision::Ready);
        assert_eq!(ready_decision(false, true, 10), ReadyDecision::Failed);
    }

    #[test]
    fn spawn_stage_payload_has_the_contract_shape() {
        let v = spawn_stage_payload("kilo", Some("p1"), "attached", true, None);
        assert_eq!(v, serde_json::json!({"agent_id": "kilo", "pane_id": "p1", "stage": "attached", "warm": true}));
        let v = spawn_stage_payload("kilo", None, "failed", false, Some("boom"));
        assert_eq!(v["pane_id"], Value::Null);
        assert_eq!(v["message"], "boom");
    }

    #[test]
    fn menu_ids_parse_to_commands() {
        assert_eq!(parse_command("settings"), Some(Command::Settings));
        assert_eq!(parse_command("quit"), Some(Command::Quit));
        assert_eq!(parse_command("stop-all"), Some(Command::StopAll));
        assert_eq!(parse_command("cockpit"), Some(Command::Cockpit));
        assert_eq!(parse_command("mode:individual"), Some(Command::SetMode(UiMode::Individual)));
        assert_eq!(parse_command("mode:grid"), None);
        assert_eq!(parse_command("new:kilo"), Some(Command::NewPane("kilo".into())));
        assert_eq!(parse_command("pane:abc"), Some(Command::FocusPane("abc".into())));
        assert_eq!(parse_command("none"), None);
    }

    fn job(ports: &[u16]) -> Job {
        Job { pid: 1, parent_pid: 0, name: "node".into(), command: "node server.js".into(), started_at_ms: 0, ports: ports.to_vec() }
    }

    #[test]
    fn zero_panes_say_nothing_is_running() {
        assert_eq!(summary_line(&[], &[], 0), "Puppet Master \u{b7} nothing running");
        // Open panes that are idle are not running either.
        assert_eq!(summary_line(&["idle", "idle"], &[], 0), "Puppet Master \u{b7} nothing running");
    }

    #[test]
    fn mixed_counts_and_a_server_give_the_right_parts_in_order() {
        let statuses = ["waiting_input", "error", "running", "running", "running", "idle"];
        assert_eq!(
            summary_line(&statuses, &[job(&[8765])], 2),
            "Puppet Master \u{b7} 2 need you \u{b7} 1 error \u{b7} 3 working \u{b7} 1 server running"
        );
    }

    #[test]
    fn need_you_is_the_attention_count_not_the_waiting_panes() {
        // The Inbox counts a question, a stuck pane and a consent card too: the tooltip shows that
        // number, and the one waiting pane does not change it.
        assert_eq!(summary_line(&["waiting_input"], &[], 6), "Puppet Master \u{b7} 6 need you");
        assert_eq!(summary_line(&["waiting_input"], &[], 0), "Puppet Master \u{b7} nothing running");
    }

    #[test]
    fn plural_words_follow_the_count() {
        assert_eq!(summary_line(&[], &[], 1), "Puppet Master \u{b7} 1 needs you");
        assert_eq!(summary_line(&["error"], &[], 0), "Puppet Master \u{b7} 1 error");
        assert_eq!(summary_line(&["error", "error"], &[], 0), "Puppet Master \u{b7} 2 errors");
        assert_eq!(summary_line(&["running"], &[], 0), "Puppet Master \u{b7} 1 working");
        assert_eq!(summary_line(&[], &[job(&[3000])], 0), "Puppet Master \u{b7} 1 server running");
        assert_eq!(
            summary_line(&[], &[job(&[3000]), job(&[5173, 5174])], 0),
            "Puppet Master \u{b7} 2 servers running"
        );
    }

    #[test]
    fn a_background_job_without_a_port_is_not_a_server() {
        // A test runner or watcher is a job, but it does not count as a server.
        assert_eq!(summary_line(&[], &[job(&[])], 0), "Puppet Master \u{b7} nothing running");
        assert_eq!(
            summary_line(&["running"], &[job(&[]), job(&[8000])], 0),
            "Puppet Master \u{b7} 1 working \u{b7} 1 server running"
        );
    }

    #[test]
    fn throttle_runs_now_when_idle_and_defers_inside_the_window() {
        let almost = Duration::from_millis(200);
        assert_eq!(throttle(None, false), Throttle::Now);
        assert_eq!(throttle(Some(REFRESH_EVERY), false), Throttle::Now);
        assert_eq!(throttle(Some(almost), false), Throttle::Later(REFRESH_EVERY - almost));
        // A deferred run is already waiting: a further call adds nothing, it will read the newest state.
        assert_eq!(throttle(Some(almost), true), Throttle::Skip);
    }
}
