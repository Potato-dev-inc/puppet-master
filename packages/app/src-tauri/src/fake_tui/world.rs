//! The process-wide test world and the per-test session.

use super::profile::FakeAgent;
use crate::pty::{registry_read_snapshot, registry_read_snapshot_full, PaneRegistry};
use crate::tui::TuiProfile;
use parking_lot::{Mutex, MutexGuard};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Listener};
/// Every line of the audit log. A fixed window of the newest lines slides once the log is longer than it, and "the
/// lines since I looked" (an index into the approval lines) then points at the wrong place. (The log used to live
/// in the real app-data folder and grow with every run of the suite; each test process now has its own.)
const WHOLE_LOG: usize = 10_000_000;


/// Everything shared by the tests of one process: one app handle, one pane registry, one fake build.
pub struct World {
    pub(super) registry: Arc<Mutex<PaneRegistry>>,
    pub(super) app: AppHandle,
    pub(super) exe: PathBuf,
    script: Arc<Mutex<Option<PathBuf>>>,
    /// One session at a time: the agent registry and the global pane observers are process-wide.
    serial: Mutex<()>,
}

static WORLD: OnceLock<World> = OnceLock::new();

/// `tools/pm-fake-tui`, built on demand (`PM_FAKE_TUI_EXE` points at a ready build instead).
fn locate_fake_exe() -> PathBuf {
    if let Some(path) = std::env::var_os("PM_FAKE_TUI_EXE") {
        return PathBuf::from(path);
    }
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tools/pm-fake-tui/Cargo.toml");
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
        .join("pm-fake-tui");
    let status = std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--quiet", "--manifest-path"])
        .arg(&manifest)
        .arg("--target-dir")
        .arg(&target)
        .status()
        .expect("cargo is needed to build tools/pm-fake-tui");
    assert!(status.success(), "building tools/pm-fake-tui failed");
    target.join("debug").join(if cfg!(windows) { "pm-fake-tui.exe" } else { "pm-fake-tui" })
}

impl World {
    pub fn get() -> &'static World {
        WORLD.get_or_init(World::start)
    }

    fn start() -> World {
        // A turn summary waits this long for its checkpoint before it is built without the files (25 s in the app).
        crate::turn_summary::set_settle_timeout_for_e2e(std::time::Duration::from_secs(300));
        // Before anything reads `app_paths::profile()` (a process-wide OnceLock): a separate app-data folder.
        std::env::set_var(crate::app_paths::PROFILE_ENV, "fake-tui-e2e");
        // ... rooted in this process's own temp folder, not the real `%APPDATA%`: test processes run side
        // by side (nextest runs each test in its own process), and a shared folder leaks between them and
        // across runs (an audit log that grew with every run of the suite, settings, agent definitions).
        // Removed with the process's other folders once stale (below).
        crate::app_paths::set_data_root_for_e2e(
            std::env::temp_dir().join(format!("pm-ftui-e2e-{}", std::process::id())).join("appdata"),
        );
        // Leftovers of earlier runs (a failed test keeps its folder to be looked at).
        if let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) {
            // `pm-ftui-e2e-<pid>`: older builds of this harness wipe every `pm-fake-tui-e2e-*` folder but their
            // own, including a parallel run's from another worktree.
            let mine = format!("pm-ftui-e2e-{}", std::process::id());
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                // Only old leftovers: another worktree's e2e run may be using its folder right now (several
                // branches run this suite at once on one machine, and each wiped the others' projects).
                let stale = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|age| age > Duration::from_secs(6 * 3600));
                if (name.starts_with("pm-fake-tui-e2e-") || name.starts_with("pm-ftui-e2e-")) && name != mine && stale {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }
        }
        // No test in this process may reach the real `wsl.exe` (Linux Inside; see `linux.rs`).
        super::linux::block_real_wsl();
        let exe = locate_fake_exe();
        assert!(exe.is_file(), "pm-fake-tui is not built at {}", exe.display());

        // A real Wry app with no windows, its event loop on its own thread.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("fake-tui-tauri".into())
            .spawn(move || {
                let app = tauri::Builder::default()
                    .any_thread()
                    .build(tauri::generate_context!())
                    .expect("build a windowless tauri app");
                let _ = tx.send(app.handle().clone());
                app.run(|_, event| {
                    if let tauri::RunEvent::ExitRequested { api, code: None, .. } = &event {
                        api.prevent_exit();
                    }
                });
            })
            .expect("start the tauri event loop thread");
        let app = rx.recv_timeout(Duration::from_secs(60)).expect("the tauri app handle");

        let registry = Arc::new(Mutex::new(PaneRegistry::new()));
        crate::bridge::install_for_e2e(registry.clone(), app.clone());

        // The pane event wiring `lib.rs::run` does in `setup`, minus everything that is UI.
        app.listen("terminal-data", |event| {
            if let Ok(value) = serde_json::from_str::<Value>(event.payload()) {
                let pane_id = value.get("pane_id").and_then(Value::as_str).unwrap_or("");
                if let (false, Some(bytes)) = (pane_id.is_empty(), value.get("data").and_then(Value::as_array)) {
                    let raw: Vec<u8> = bytes.iter().filter_map(|v| v.as_u64().map(|n| n as u8)).collect();
                    crate::bridge::push_terminal_sse(pane_id, &raw, value.get("offset").and_then(Value::as_u64));
                }
            }
        });
        app.listen("pty://status", |event| {
            if let Ok(value) = serde_json::from_str::<Value>(event.payload()) {
                let pane_id = value.get("pane_id").and_then(Value::as_str).unwrap_or("");
                let status = value.get("status").and_then(Value::as_str).unwrap_or("");
                if !pane_id.is_empty() && !status.is_empty() {
                    crate::bridge::push_pane_status_sse(pane_id, status);
                }
            }
        });
        crate::pane_activity::init(registry.clone(), app.clone());
        crate::loop_guard::init(app.clone());
        // The Agent Wizard's panes (guided capture, visible test) go through this app's pane registry.
        crate::tui::declarative::wizard::visible::host::install(Arc::new(
            crate::tui::declarative::wizard::visible::registry_host::RegistryHost::new(registry.clone(), app.clone()),
        ));

        World { registry, app, exe, script: Arc::new(Mutex::new(None)), serial: Mutex::new(()) }
    }

    /// Start a test: the fake agent types are registered, a git project exists, and every pane the
    /// test opens is closed again when the session drops.
    /// The windowless app handle and the pane registry of this process (the Team facade starts the
    /// Team service on them).
    pub(super) fn handles(&self) -> (AppHandle, Arc<Mutex<PaneRegistry>>) {
        (self.app.clone(), self.registry.clone())
    }

    pub fn session(&'static self, name: &str) -> Session {
        let guard = self.serial.lock();
        let root = std::env::temp_dir().join(format!("pm-ftui-e2e-{}", std::process::id())).join(name);
        let _ = std::fs::remove_dir_all(&root);
        let project = root.join("project");
        let aux = root.join("aux");
        std::fs::create_dir_all(&project).expect("project dir");
        std::fs::create_dir_all(&aux).expect("aux dir");
        git(&project, &["init", "-q"]);
        std::fs::write(project.join("README.md"), "# fake project\n").expect("seed file");
        git(&project, &["add", "."]);
        git(&project, &["-c", "user.name=pm", "-c", "user.email=pm@example.invalid", "commit", "-q", "-m", "init"]);

        let registry = crate::tui::registry::registry();
        let saved = registry.snapshot();
        let mut with_fakes: Vec<Arc<dyn TuiProfile>> = saved.iter().cloned().collect();
        for (id, real) in [("fake_claude", "claude"), ("fake_codex", "codex")] {
            let inner = crate::tui::profile_for(real);
            with_fakes.push(Arc::new(FakeAgent::new(id, inner, self.exe.clone(), self.script.clone())));
        }
        registry.swap(with_fakes);
        self.registry.lock().project_path = project.to_string_lossy().into_owned();
        Session { world: self, _serial: guard, root, project, aux, saved }
    }
}

fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git").args(args).current_dir(dir).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// One test's view of the world.
pub struct Session {
    pub(super) world: &'static World,
    _serial: MutexGuard<'static, ()>,
    root: PathBuf,
    pub(super) project: PathBuf,
    pub(super) aux: PathBuf,
    saved: Arc<Vec<Arc<dyn TuiProfile>>>,
}

impl Session {
    pub fn project(&self) -> &Path {
        &self.project
    }

    /// A scratch folder of this test (outside the project): scripts, logs, whatever the test needs.
    pub fn aux(&self) -> &Path {
        &self.aux
    }

    /// The `pm-fake-tui` executable this world built.
    pub fn fake_exe(&self) -> &Path {
        &self.world.exe
    }

    /// The CLI-update canary for the fake agent `agent` (`fake_codex`): its `--version` and its start-up
    /// check both play whatever script `script()` last wrote.
    pub fn canary(&self, agent: &'static str) -> super::CanaryRig {
        super::CanaryRig::new(agent, self.world.exe.clone(), self.script_path())
    }

    /// The script file `script()` last wrote (`aux/script.toml`).
    pub fn script_path(&self) -> PathBuf {
        self.aux.join("script.toml")
    }

    /// Write the script the next pane will play. A `log` line is added so `events()` can read what the
    /// fake saw (prompts typed, keys answered).
    pub fn script(&self, toml: &str) {
        let path = self.aux.join("script.toml");
        let log = self.aux.join("events.jsonl");
        let _ = std::fs::remove_file(&log);
        let text = format!("log = '{}'\n{toml}", log.display());
        std::fs::write(&path, text).expect("write the script");
        *self.world.script.lock() = Some(path);
    }

    /// A slow ConPTY: the pane readers hold any chunk of output containing `marker` for `wait` before
    /// the pane sees it, until the session ends. What a loaded Windows machine does to an agent's
    /// last frame: its process is gone before that frame is on screen.
    pub fn hold_output(&self, marker: &str, wait: Duration) {
        super::set_hold_output(Some((marker.to_string(), wait)));
    }

    /// The fake's own event log: one JSON object per line (`prompt`, `approval_answer`, `exit`, ...).
    pub fn events(&self) -> Vec<Value> {
        std::fs::read_to_string(self.aux.join("events.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// Call one of the `/agents/*` routes the way the bridge does. `Err` carries the error JSON.
    pub fn agents(&self, method: &str, path: &str, body: Value) -> Result<Value, Value> {
        self.agents_as(None, method, path, body)
    }

    /// The same as an MCP connection: `session` is the id its `X-Puppet-Master-Session` header carries,
    /// so the run's pane belongs to that connection.
    pub fn agents_as(&self, session: Option<&str>, method: &str, path: &str, body: Value) -> Result<Value, Value> {
        let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
        let bytes = serde_json::to_vec(&body).expect("json body");
        match crate::agent_runs::handle_request(method, &segments, "", &bytes, &self.world.registry, &self.world.app, session) {
            Some(Ok(value)) => Ok(value),
            Some(Err(error)) => Err(json!({ "code": error.code, "message": error.message, "recoverable": error.recoverable, "context": error.context })),
            None => Err(json!({ "code": "NO_SUCH_ROUTE", "message": path })),
        }
    }

    /// `run_agent` with the defaults a visible run gets: a pane in this session's project.
    pub fn run(&self, agent_type: &str, task: &str, extra: Value) -> Result<Value, Value> {
        let mut body = json!({
            "agent_type": agent_type,
            "task": task,
            "project_path": self.project.to_string_lossy(),
            "wait_ms": 1,
            "background": true,
            // The run's own wall-clock limit is only a hang detector here. 90 s ended a plain run with TIMEOUT
            // on a starved machine (pinned to one core among 12 busy processes the whole run took 148 s).
            "timeout_ms": 600_000,
            // A pane a person watches stays open after the run, like the cockpit's: its screen and its
            // turn summary are what several tests read.
            "keep_pane": true,
        });
        if let (Some(map), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
            map.extend(extra.clone());
        }
        self.agents("POST", "/agents/run", body)
    }

    /// `run`, as the MCP connection `session`.
    pub fn run_as(&self, session: &str, agent_type: &str, task: &str, extra: Value) -> Result<Value, Value> {
        let mut body = json!({
            "agent_type": agent_type,
            "task": task,
            "project_path": self.project.to_string_lossy(),
            "wait_ms": 1,
            "background": true,
            "timeout_ms": 600_000,
            "keep_pane": true,
        });
        if let (Some(map), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
            map.extend(extra.clone());
        }
        self.agents_as(Some(session), "POST", "/agents/run", body)
    }

    /// "How agent runs work" for a fake agent: `one_task` or `visible`, and whether its headless mode
    /// counts as verified (the real verdict reads a wizard test that fake agents do not have).
    pub fn run_mode(&self, agent: &str, one_task: bool, verified: bool) {
        use crate::agent_run_mode::{e2e, set_for_agent, RunMode};
        e2e::force(agent, Some(verified));
        set_for_agent(agent, if one_task { RunMode::OneTask } else { RunMode::Visible }).expect("save the run mode");
    }

    /// Switch the MCP connection `session` to catalog mode `mode` (`agent`, `shell` or `both`).
    pub fn set_mode_as(&self, session: &str, mode: &str) {
        crate::mcp_sessions::set_mode(session, mode).expect("a valid catalog mode");
    }

    /// The bridge's session gate (`mcp_sessions::authorize`) for one call of the connection `session`:
    /// `Ok` when the route would run, `Err` with the refusal's code and message.
    pub fn authorize_as(&self, session: &str, method: &str, path: &str, body: Value) -> Result<(), Value> {
        crate::mcp_sessions::authorize(Some(session), method, path, &body)
            .map_err(|error| json!({ "code": error.code, "message": error.message }))
    }

    /// Wait (blocking) until the run behind `handle` is in one of `until`, or `wait_ms` passes; returns the
    /// run's view as `wait_agents` reports it.
    pub fn wait(&self, handle: &str, until: &[&str], wait_ms: u64) -> Result<Value, Value> {
        let reply = self.agents(
            "POST",
            "/agents/wait",
            json!({ "project_path": self.project.to_string_lossy(), "handles": [handle], "until": until, "wait_ms": wait_ms }),
        )?;
        Ok(reply["agents"][0].clone())
    }

    /// The run's current view without waiting (`wait_ms = 0`).
    pub fn view(&self, handle: &str) -> Value {
        self.wait(handle, &[], 0).unwrap_or(Value::Null)
    }

    /// Registers a SCREEN-READ agent for this test: `definition` is a declarative agent definition (TOML) and the
    /// registered agent `id` is that real `DeclarativeProfile` launching `pm-fake-tui` with the script of the
    /// moment. Every screen-reading method is the definition's own, so the test exercises exactly what a
    /// wizard-written definition would. Removed again when the session drops.
    pub fn screen_agent(&self, id: &'static str, definition: &str) {
        let def = crate::tui::declarative::parse_definition(definition).unwrap_or_else(|e| panic!("the definition does not parse: {e}"));
        let inner: Arc<dyn TuiProfile> =
            Arc::new(crate::tui::declarative::DeclarativeProfile::new(def).unwrap_or_else(|e| panic!("the definition does not build: {e}")));
        let registry = crate::tui::registry::registry();
        let mut all: Vec<Arc<dyn TuiProfile>> = registry.snapshot().iter().cloned().collect();
        all.push(Arc::new(FakeAgent::new(id, inner, self.world.exe.clone(), self.world.script.clone())));
        registry.swap(all);
    }

    /// A bare pane of `agent_type` in the project, 120x30, with nothing typed: for tests that read screens.
    pub fn spawn_pane(&self, agent_type: &str) -> Result<String, String> {
        self.spawn_pane_in(agent_type, &self.project)
    }

    /// `spawn_pane` in another folder (a second project of the same test).
    pub fn spawn_pane_in(&self, agent_type: &str, cwd: &Path) -> Result<String, String> {
        crate::pty::registry_spawn_pane(
            &self.world.registry,
            &self.world.app,
            crate::pty::SpawnPaneArgs {
                agent_type: agent_type.to_string(),
                cwd: Some(cwd.to_string_lossy().into_owned()),
                cols: Some(120),
                rows: Some(30),
                extra_args: None,
                pane_id: None,
                read_only: false,
                account: None,
                guard_env: None,
            },
        )
    }

    /// Keystrokes from a person (the terminal view): `text` as typed, then Enter when `enter`.
    pub fn type_as_user(&self, pane_id: &str, text: &str, enter: bool) -> Result<(), String> {
        crate::pty::registry_write_input(
            &self.world.registry,
            &self.world.app,
            pane_id,
            text,
            enter,
            false,
            None,
            crate::pty::WriteOrigin::User,
        )
    }

    /// The pane's screen as the emulator holds it: every cell's text, dimmed ones included.
    ///
    /// Not `registry_read_snapshot`: since 4832207 that is the screen completion and approval
    /// detection read, which leaves out the text an agent dims in its own input box (Claude's
    /// suggestion, Codex's "Ask Codex to do anything", Claude's `Try "..."` hint). The tests that wait
    /// for those words to be drawn read the emulator; what readiness reads is `input_screen`.
    pub fn screen(&self, pane_id: &str) -> String {
        let parser = self.world.registry.lock().panes.get(pane_id).map(|pane| pane.screen.clone());
        parser.map(|parser| parser.lock().screen().contents()).unwrap_or_default()
    }

    /// The screen completion and approval detection read (`registry_read_snapshot`): what a reply is
    /// cut from. Dimmed input-box text is left out.
    pub fn watch_screen(&self, pane_id: &str) -> String {
        registry_read_snapshot(&self.world.registry, pane_id).unwrap_or_default()
    }

    /// The pane's screen as readiness reads it (the profile's `input_screen`).
    pub fn input_screen(&self, pane_id: &str) -> String {
        registry_read_snapshot_full(&self.world.registry, pane_id).unwrap_or_default()
    }

    /// Everything a viewer that mounts now would show: the scrollback followed by the screen.
    pub fn history(&self, pane_id: &str) -> String {
        crate::pty::registry_read_full_text(&self.world.registry, pane_id).unwrap_or_default()
    }

    pub fn pane_status(&self, pane_id: &str) -> Option<String> {
        self.world.registry.lock().list().into_iter().find(|p| p.id == pane_id).map(|p| p.status)
    }

    pub fn pane_ids(&self) -> Vec<String> {
        self.world.registry.lock().panes.keys().cloned().collect()
    }

    /// The approval / trust card the pane shows right now (what the desktop UI renders).
    pub fn card(&self, pane_id: &str) -> Option<Value> {
        crate::pane_prompt::detect(&self.world.registry, pane_id, false)
    }

    /// The desktop UI's answer to a card (`answer_pane_prompt`): the same function the Allow button
    /// calls, with the click it sends (a fresh, trusted gesture).
    pub fn answer_card(&self, pane_id: &str, prompt_id: &str, choice: &str) -> Result<Value, Value> {
        let click = crate::pane_prompt::answers::UserGesture {
            id: uuid::Uuid::new_v4().to_string(),
            trusted: true,
            at_ms: crate::pane_prompt::answers::now_ms(),
            kind: "click".into(),
        };
        self.answer_card_with(pane_id, prompt_id, choice, Some(&click))
    }

    /// The person has another project selected in the window than the run's (the bridge's default
    /// project): pane-to-run lookups must not depend on which one it is.
    pub fn select_other_project(&self) -> PathBuf {
        let other = self.root.join("other-project");
        std::fs::create_dir_all(&other).expect("other project dir");
        self.world.registry.lock().project_path = other.to_string_lossy().into_owned();
        other
    }

    /// The audit log's answer lines (`kind: approval`) for one pane, oldest first.
    pub fn approvals_for(&self, pane_id: &str) -> Vec<Value> {
        let mut lines: Vec<Value> = crate::audit::global()
            .recent(WHOLE_LOG)
            .into_iter()
            .filter(|entry| entry.kind == "approval" && entry.detail.get("pane_id").and_then(Value::as_str) == Some(pane_id))
            .map(|entry| json!({ "who": entry.who, "summary": entry.summary, "detail": entry.detail, "ts_ms": entry.ts_ms }))
            .collect();
        lines.reverse();
        lines
    }

    /// `answer_pane_prompt` with whatever gesture (or none) a caller sends.
    pub fn answer_card_with(
        &self,
        pane_id: &str,
        prompt_id: &str,
        choice: &str,
        gesture: Option<&crate::pane_prompt::answers::UserGesture>,
    ) -> Result<Value, Value> {
        crate::agent_runs::answer_for_pane(&self.world.registry, &self.world.app, pane_id, prompt_id, choice, gesture)
            .map_err(|error| json!({ "code": error.code, "message": error.message }))
    }

    /// The card the attention list shows for the approval or trust question on `pane_id`, as a paired phone
    /// gets it (`GET /attention`): its opaque id and what an answer must send back. `None` while the list
    /// has no such card yet.
    pub fn attention_card(&self, pane_id: &str) -> Option<Value> {
        let snapshot = crate::attention::snapshot_now();
        let item = snapshot.items.iter().find(|item| {
            item.state == crate::attention::model::State::Open
                && matches!(item.kind, crate::attention::model::Kind::Approval | crate::attention::model::Kind::TrustGate)
                && item.pane.as_ref().is_some_and(|pane| pane.id == pane_id)
                && item.prompt.is_some()
        })?;
        serde_json::to_value(item).ok()
    }

    /// A paired phone (all four abilities) answers `item_id` through `POST /attention/{id}/answer`: the very
    /// function the route calls, on the real registry, the real screen and the real write path. `body` is
    /// the request JSON; the reply is `(status, body)` as the route would send it.
    pub fn answer_from_phone(&self, item_id: &str, body: Value) -> (u16, Value) {
        let request: crate::attention::answer::Request = serde_json::from_value(body).expect("a request body");
        let caller = crate::attention::answer::Caller {
            device_id: "e2e-phone".into(),
            device_name: "E2E phone".into(),
            caps: crate::attention::caps::DeviceCaps::all(),
            via: crate::attention::answer::Via::Card,
        };
        let env = crate::attention::answer::RealEnv { registry: self.world.registry.clone(), app: self.world.app.clone() };
        crate::attention::answer::answer(&env, &caller, item_id, &request)
    }

    /// The summaries of the audit log's approval lines so far, oldest first (the log is shared by the whole
    /// test process).
    pub fn approval_audit_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = crate::audit::global().recent(WHOLE_LOG).into_iter().filter(|entry| entry.kind == "approval").map(|entry| entry.summary).collect();
        lines.reverse();
        lines
    }

    /// Test support for the guard on `approval_audit_lines`: `n` unrelated lines in the audit log.
    pub fn audit_noise(&self, n: usize) {
        for i in 0..n {
            crate::audit::record(crate::audit::Kind::Setting, crate::audit::Who::Local, &format!("fake-tui noise {i}"), json!({}));
        }
    }

    /// Test support: one approval line, as an answered card leaves.
    pub fn audit_approval(&self, summary: &str) {
        crate::audit::record(crate::audit::Kind::Approval, crate::audit::Who::Local, summary, json!({}));
    }

    /// The turn summaries kept for a pane (the card the cockpit shows after a turn).
    pub fn turn_summaries(&self, pane_id: &str) -> Value {
        serde_json::to_value(crate::turn_summary::tracker().summaries(pane_id)).unwrap_or(Value::Null)
    }

    /// The pane's PTY grid changed (a window resize): what a viewer's resize does to the agent.
    pub fn resize(&self, pane_id: &str, cols: u16, rows: u16) -> Result<bool, String> {
        crate::pty::registry_resize(&self.world.registry, pane_id, cols, rows)
    }

    /// How many times the pane's PTY grid has changed since it started (the flight recorder's resize
    /// events). Every one makes an inline TUI reprint its screen, so a pane nobody resized must read 0.
    pub fn pty_resizes(&self, pane_id: &str) -> usize {
        crate::flight::get(pane_id)
            .map(|ring| ring.snapshot().events.iter().filter(|event| event.kind == crate::flight::ring::Kind::Resize).count())
            .unwrap_or(0)
    }

    /// A warm-standby-style pane: hidden BEFORE it is spawned (as `standby::host` does), so no pane list offers it.
    pub fn spawn_hidden_pane(&self, agent_type: &str) -> Result<String, String> {
        let pane_id = format!("standby-e2e-{}", uuid::Uuid::new_v4());
        crate::pty::hidden::hide(&pane_id);
        let spawned = crate::pty::registry_spawn_pane(
            &self.world.registry,
            &self.world.app,
            crate::pty::SpawnPaneArgs {
                agent_type: agent_type.to_string(),
                cwd: Some(self.project.to_string_lossy().into_owned()),
                cols: Some(120),
                rows: Some(30),
                extra_args: None,
                pane_id: Some(pane_id.clone()),
                read_only: false,
                account: None,
                guard_env: None,
            },
        );
        if spawned.is_err() {
            crate::pty::hidden::forget(&pane_id);
        }
        spawned
    }

    /// The ids a window's pane list (`list_panes`) shows: hidden standbys are not in it.
    pub fn listed_pane_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.world.registry.lock().list().into_iter().map(|p| p.id).collect();
        ids.sort();
        ids
    }

    /// The pane's PTY grid as the registry lists it.
    pub fn world_pane_size(&self, pane_id: &str) -> Option<(u16, u16)> {
        self.world.registry.lock().list().into_iter().find(|p| p.id == pane_id).map(|p| (p.cols, p.rows))
    }

    /// What a lead's `read_terminal_buffer` does: the screen (`view=screen`) or the scrollback.
    pub fn read_terminal(&self, pane_id: &str, scrollback: bool) -> String {
        if scrollback {
            crate::pty::registry_read_buffer(&self.world.registry, pane_id, 200).unwrap_or_default()
        } else {
            registry_read_snapshot(&self.world.registry, pane_id).unwrap_or_default()
        }
    }

    /// What a lead's `press_key` does (the `/panes/{id}/key` route): the named key's bytes, written as the API.
    pub fn press_key(&self, pane_id: &str, key: &str) -> Result<(), String> {
        let seq = crate::pty::keys::sequence(key)?;
        crate::pty::registry_write_input(&self.world.registry, &self.world.app, pane_id, &seq, false, false, None, crate::pty::WriteOrigin::Api)
    }

    /// Waits for the process tree: a test ends (and its folder goes) right after, with no orphans left.
    pub fn close_pane(&self, pane_id: &str) {
        let _ = crate::pty::registry_kill_pane_and_wait(&self.world.registry, pane_id, None);
    }

    /// A pane outbox over this world's registry and app: the message box under the Chat tab.
    pub fn outbox(&self) -> crate::pane_outbox::Outbox {
        let host = crate::pane_outbox::LiveHost::new(self.world.registry.clone(), self.world.app.clone());
        crate::pane_outbox::Outbox::new(Arc::new(host))
    }

    /// The Point box's "To:" list over this world's panes, as the box gets it: `{ "targets": [...], "default": id | null }`.
    pub fn point_targets(&self, opener: Option<&str>, cwd: Option<&str>) -> Value {
        let infos = self.world.registry.lock().list();
        let facts = crate::point::window::target_facts(&self.world.registry, &infos);
        let (targets, default) = crate::point::target::targets(&facts, opener, cwd);
        serde_json::json!({ "targets": targets, "default": default })
    }

    /// The Chat tab of a pane, read off its screen (what the app does for an agent with no session file).
    pub fn open_screen_chat(&self, pane_id: &str, agent_type: &str) -> super::ChatProbe {
        super::ChatProbe::open(self.world.registry.clone(), pane_id, agent_type)
    }

    /// Polls `check` every 100 ms until it returns `Some` or `secs` pass.
    pub fn until<T>(&self, secs: u64, mut check: impl FnMut() -> Option<T>) -> Option<T> {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            if let Some(found) = check() {
                return Some(found);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// What the REAL profile of `agent_type` (`claude`, `codex`, ...) reads off `screen`: for the
    /// conformance test that every screen the fake draws is recognised.
    pub fn read_screen(&self, agent_type: &str, screen: &str) -> Value {
        let profile = crate::tui::profile_for(agent_type);
        let gate = profile.startup_gate(screen);
        json!({
            "ready": profile.ready_recogniser(screen).map(|state| format!("{state:?}")),
            "gate": gate.as_ref().map(|g| g.id),
            "gate_dismiss": gate.and_then(|g| g.dismiss),
            "busy": profile.busy_marker(screen),
            "status": profile.classify_screen(screen).map(|s| format!("{s:?}")),
            "prompt": profile.detect_prompt(screen, false),
            // The usage-limit detector (`handoff::detect`) on the profile's own `limits.toml`.
            "limit": profile.detect_limit(screen, &crate::handoff::reset::SystemClock),
        })
    }
}

/// Missions (Preview) driven the way the desktop's commands drive them: the real engine, the real
/// live world (`run_agent` / `followup_task` into fake panes), real checks in the mission's copy.
impl Session {
    /// Starts a mission in this session's project. `checks` are argvs; the budget is in turns.
    pub fn mission_start(&self, agents: &[&str], objective: &str, checks: &[&[&str]], turns: u32, may_change_tests: bool) -> Result<Value, Value> {
        let checks = checks
            .iter()
            .map(|argv| crate::missions::model::MissionCheck {
                name: crate::missions::routes::name_for(&argv.iter().map(|s| s.to_string()).collect::<Vec<_>>()),
                check: crate::agent_runs::checks::Check::CommandExits { command: argv.iter().map(|s| s.to_string()).collect(), expect_exit: 0, timeout_ms: Some(60_000) },
            })
            .collect();
        let agents = agents.iter().map(|a| crate::missions::model::AgentChoice { agent_type: a.to_string(), account: None, model: None }).collect();
        let mut req = crate::missions::StartRequest::new(&self.project.to_string_lossy(), objective, checks, agents);
        req.budget_turns = Some(turns);
        req.may_change_tests = Some(may_change_tests);
        req.round_timeout_ms = 120_000;
        crate::missions::start(req)
            .map(|m| crate::missions::routes::agent_object(&m))
            .map_err(|e| json!({ "code": e.code, "message": e.message }))
    }

    /// The mission's view (what `inspect_agent` returns for its handle).
    pub fn mission(&self, id: &str) -> Value {
        crate::missions::find(id).map(|m| crate::missions::routes::agent_object(&m)).unwrap_or(Value::Null)
    }

    /// Waits until the mission's state is one of `states` (`green`, `needs_you`, `stopped`...).
    pub fn mission_wait(&self, id: &str, states: &[&str], secs: u64) -> Value {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            let view = self.mission(id);
            let state = view["stage"].as_str().unwrap_or_default().to_string();
            if states.contains(&state.as_str()) || Instant::now() >= deadline {
                return view;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// The person's answer to a mission card (`undo_tests`, `accept_tests`, `hand_to_me`...).
    pub fn mission_decide(&self, id: &str, choice: &str) -> Result<Value, Value> {
        crate::missions::decide_card(id, choice, "you")
            .map(|m| crate::missions::routes::agent_object(&m))
            .map_err(|e| json!({ "code": e.code, "message": e.message }))
    }

    pub fn mission_stop(&self, id: &str) {
        let _ = crate::missions::stop(id, "you");
    }

    /// The mission's own event log (`rounds.jsonl`).
    pub fn mission_log(&self, id: &str) -> Vec<Value> {
        crate::missions::store::project_of(id).map(|p| crate::missions::store::read_log_all(&p, id)).unwrap_or_default()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // A mission the test left running stops with it (its engine would keep starting turns).
        for m in crate::missions::store::list(&self.project.to_string_lossy()) {
            if m.state.active() {
                let _ = crate::missions::stop(&m.id, "you");
            }
        }
        let panes = self.pane_ids();
        for pane in panes {
            self.close_pane(&pane);
        }
        // The run-mode choices and forced verdicts of the fake agents end with the test.
        for agent in ["fake_claude", "fake_codex"] {
            let _ = crate::agent_run_mode::set_for_agent(agent, crate::agent_run_mode::RunMode::Visible);
            crate::agent_run_mode::e2e::force(agent, None);
        }
        super::linux::end();
        crate::tui::registry::registry().swap(self.saved.iter().cloned().collect());
        *self.world.script.lock() = None;
        super::set_hold_output(None);
        if !std::thread::panicking() {
            // The killed agent's shell may still hold the folder for a moment.
            for _ in 0..30 {
                if std::fs::remove_dir_all(&self.root).is_ok() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// Keep going (SPEC n32 section 5, KG-10): the limit card answered the way the desktop and a paired phone
/// answer it, the hand-over records, and the two fake agents' own logs. The real engine does the work; these
/// only reach for what a window or a phone would read.
impl Session {
    /// Like [`Session::script`], but the fake writes its events to `aux/<log_name>` and no earlier log is
    /// deleted: the old agent's log and the new agent's stay side by side. The next pane plays this script;
    /// a pane already running keeps the one it started with.
    pub fn script_with_log(&self, toml: &str, log_name: &str) {
        let path = self.aux.join("script.toml");
        let log = self.aux.join(log_name);
        let _ = std::fs::remove_file(&log);
        let text = format!("log = '{}'\n{toml}", log.display());
        std::fs::write(&path, text).expect("write the script");
        *self.world.script.lock() = Some(path);
    }

    /// The events of the fake that logs to `aux/<log_name>`.
    pub fn events_of(&self, log_name: &str) -> Vec<Value> {
        std::fs::read_to_string(self.aux.join(log_name))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// The pane's working folder as the registry lists it.
    pub fn pane_cwd(&self, pane_id: &str) -> Option<String> {
        self.world.registry.lock().list().into_iter().find(|p| p.id == pane_id).map(|p| p.cwd)
    }

    /// The pid of the agent in `pane_id` (the confirmed child of its host shell), once known.
    pub fn agent_pid(&self, pane_id: &str) -> Option<u32> {
        let root = self.world.registry.lock().get(pane_id).map(|pane| pane.info.pid)?;
        crate::pty::agent_proc::agent_pid(pane_id, root)
    }

    /// Whether process `pid` is still running.
    pub fn process_alive(&self, pid: u32) -> bool {
        crate::proc_util::pid_alive(pid)
    }

    /// Settings > Agents > Keep going: the order the limit card offers agents in. Also forgets the limits earlier tests
    /// of this process left behind (a limit note lasts until its reset and would decide what this test is offered).
    pub fn keep_going_order(&self, agents: &[&str]) {
        crate::handoff::forget_limit_notes_for_e2e();
        use crate::handoff::keep_going::model::{Settings, Target};
        let order = agents.iter().map(|agent| Target::parse(agent).expect("a valid target")).collect();
        crate::handoff::keep_going::live::save_settings(&Settings { order, ask_back: true }).expect("save the Keep going settings");
    }

    /// The usage-limit card the pane raised, as the Inbox lists it (`None` while there is none pending).
    pub fn limit_card(&self, pane_id: &str) -> Option<Value> {
        self.pending_cards(pane_id).into_iter().find(|card| card["limit"]["kind"] == "usage_limit")
    }

    /// Every card (question) pending for `pane_id`, as JSON.
    pub fn pending_cards(&self, pane_id: &str) -> Vec<Value> {
        crate::questions::store()
            .list_pending(crate::questions::now_ms())
            .into_iter()
            .filter(|q| q.pane_id.as_deref() == Some(pane_id))
            .filter_map(|q| serde_json::to_value(q).ok())
            .collect()
    }

    /// A person's answer to a card in the desktop window (`answered_by: desktop`) or on a paired phone
    /// (`phone`): the store's own answer, which then asks `handoff` to act.
    pub fn answer_question(&self, id: &str, choice: &str, answered_by: &str) -> Result<Value, Value> {
        crate::questions::store()
            .answer(id, Some(choice), None, answered_by, crate::questions::now_ms())
            .map(|resolution| json!({ "accepted": resolution.accepted }))
            .map_err(|error| json!({ "code": error.code, "message": error.message }))
    }

    /// `POST /questions/{id}/answer` as the bridge routes it: `via_phone` is true only behind the paired-device
    /// gate. Plain loopback (`false`) is what any local program, an agent's shell included, can reach.
    pub fn answer_question_route(&self, id: &str, choice: &str, via_phone: bool) -> (u16, Value) {
        let body = json!({ "choice_id": choice });
        match crate::questions::routes::handle("POST", &["questions", id, "answer"], &body, None, via_phone, &|| {}) {
            Some(Ok(reply)) | Some(Err(reply)) => reply,
            None => (404, json!({ "code": "NO_SUCH_ROUTE" })),
        }
    }

    /// A question an agent asked in `pane_id` that nobody answered (`ask_user`): its text may reach a brief.
    pub fn ask_in_pane(&self, pane_id: &str, question: &str) -> String {
        let created = crate::questions::store()
            .create(
                crate::questions::NewQuestion {
                    pane_id: Some(pane_id.to_string()),
                    question: question.to_string(),
                    allow_free_text: Some(true),
                    ..Default::default()
                },
                crate::questions::now_ms(),
            )
            .expect("the question is stored");
        created.id
    }

    /// Dismisses a card (cleanup for a test that leaves one behind).
    pub fn dismiss_question(&self, id: &str) {
        let _ = crate::questions::store().dismiss(id, crate::questions::now_ms());
    }

    /// Every hand-over record, newest first, as the private store keeps them.
    pub fn keep_going_records(&self) -> Vec<Value> {
        crate::handoff::keep_going::live::records().iter().filter_map(|record| serde_json::to_value(record).ok()).collect()
    }

    /// The hand-over whose old side is `pane_id`, once its record exists.
    pub fn keep_going_record_from(&self, pane_id: &str) -> Option<Value> {
        self.keep_going_records().into_iter().find(|record| record["from"]["pane_id"] == pane_id)
    }

    /// What the windows are told about the hand-overs of `pane_id` (`keep_going_lineage`), as JSON text.
    pub fn keep_going_lineage_json(&self, pane_id: &str) -> String {
        serde_json::to_string(&crate::handoff::keep_going::live::lineage(pane_id)).unwrap_or_default()
    }

    /// The stored brief of hand-over `id` (the desktop-only `keep_going_brief` command's answer).
    pub fn keep_going_brief(&self, id: &str) -> Result<String, String> {
        crate::handoff::keep_going::live::brief_text(id)
    }

    /// The audit log's Keep going lines (who, summary and detail as one JSON text each), oldest first.
    pub fn keep_going_audit_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = crate::audit::global()
            .recent(WHOLE_LOG)
            .into_iter()
            .filter(|entry| entry.kind.starts_with("keep_going"))
            .map(|entry| json!({ "who": entry.who, "summary": entry.summary, "detail": entry.detail }).to_string())
            .collect();
        lines.reverse();
        lines
    }

    /// The folder `<app data>/keep-going`, where the records and briefs live.
    pub fn keep_going_root(&self) -> PathBuf {
        crate::app_paths::app_data_dir().join("keep-going")
    }
}

impl Session {
    /// Whether the separate copy (worktree) that holds `cwd` is still open and on disk. `None` when `cwd` is
    /// not in a copy at all.
    pub fn worktree_open(&self, cwd: &str) -> Option<bool> {
        let origin = crate::worktrees::origin_of_cwd(cwd)?;
        crate::worktrees::list(&origin)
            .into_iter()
            .find(|entry| crate::project_path::workspace_covers(Path::new(&entry.worktree.path), Path::new(cwd)))
            .map(|entry| entry.exists && entry.worktree.closed.is_none())
    }
}

/// Night Shift (SPEC n32 NS-21): a night driven by the real runner, with two fake agents that each play their
/// own script, and a clock the test moves.
impl Session {
    /// Write the script the next pane of agent type `agent_id` (`fake_claude`, `fake_codex`) plays, and log what
    /// that fake sees to `aux/events.<agent_id>.jsonl`. Another agent keeps its own script. A script written
    /// again (after the first pane started) is played by the panes that start later; a log is never cleared.
    pub fn script_for(&self, agent_id: &str, toml: &str) {
        let log = self.aux.join(format!("events.{agent_id}.jsonl"));
        let text = format!("log = '{}'
{toml}", log.display());
        std::fs::write(self.aux.join(format!("script.{agent_id}.toml")), text).expect("write the agent's script");
        // The shared script's place: a pane of an agent with no script of its own falls back to it.
        let shared = self.aux.join("script.toml");
        let mut current = self.world.script.lock();
        if current.is_none() {
            *current = Some(shared);
        }
    }

    /// What the fake of `agent_id` logged (`script_for`'s log), oldest first.
    pub fn events_for(&self, agent_id: &str) -> Vec<Value> {
        self.events_of(&format!("events.{agent_id}.jsonl"))
    }

    /// The night rig for this session's project.
    pub fn night(&self) -> super::NightRig {
        super::NightRig::new(self.world.registry.clone(), self.project.clone())
    }
}

/// Collision Alert (SPEC n33 CA-16): what the Inbox card, the phone item and the settings row read, off the real
/// watch thread (`collision::live`) and the real attention list.
impl Session {
    /// `collision_list`: the alerts the window shows, as the window gets them (camelCase JSON), newest first.
    pub fn collision_list(&self) -> Vec<Value> {
        crate::collision::live::views().iter().filter_map(|v| serde_json::to_value(v).ok()).collect()
    }

    /// `collision_ignore` (the command validates the id first; ids here come from `collision_list`).
    pub fn collision_ignore(&self, id: &str) -> Result<(), String> {
        crate::collision::live::ignore(id)
    }

    /// Settings > Agents > "Warn me when two agents change the same file".
    pub fn collision_set_enabled(&self, enabled: bool) {
        crate::collision::live::set_settings(crate::collision::model::Settings { enabled }).expect("save collision.json");
    }

    /// The git calls the watch thread's probe has made so far.
    pub fn collision_probe_calls(&self) -> u64 {
        crate::collision::live::probe_calls_for_e2e()
    }

    /// The open attention items a paired phone gets for Collision Alert (key contains `collision:`), as JSON.
    pub fn collision_attention_items(&self) -> Vec<Value> {
        crate::attention::snapshot_now()
            .items
            .iter()
            .filter(|item| item.state == crate::attention::model::State::Open && item.key.contains("collision:"))
            .filter_map(|item| serde_json::to_value(item).ok())
            .collect()
    }

    /// The numbers on the Inbox and the phone: (needs you now, ready for you).
    pub fn attention_counts(&self) -> (u32, u32) {
        let counts = crate::attention::snapshot_now().counts;
        (counts.blocking, counts.ready)
    }
}
