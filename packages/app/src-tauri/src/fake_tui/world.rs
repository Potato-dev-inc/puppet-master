//! The process-wide test world and the per-test session.

use super::profile::FakeAgent;
use crate::pty::{registry_kill_pane, registry_read_snapshot, registry_read_snapshot_full, PaneRegistry};
use crate::tui::TuiProfile;
use parking_lot::{Mutex, MutexGuard};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Listener};

/// Everything shared by the tests of one process: one app handle, one pane registry, one fake build.
pub struct World {
    registry: Arc<Mutex<PaneRegistry>>,
    app: AppHandle,
    exe: PathBuf,
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
        // Before anything reads `app_paths::profile()` (a process-wide OnceLock): a separate app-data folder.
        std::env::set_var(crate::app_paths::PROFILE_ENV, "fake-tui-e2e");
        // Leftovers of earlier runs (a failed test keeps its folder to be looked at).
        if let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) {
            let mine = format!("pm-fake-tui-e2e-{}", std::process::id());
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("pm-fake-tui-e2e-") && name != mine {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }
        }
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

        World { registry, app, exe, script: Arc::new(Mutex::new(None)), serial: Mutex::new(()) }
    }

    /// Start a test: the fake agent types are registered, a git project exists, and every pane the
    /// test opens is closed again when the session drops.
    pub fn session(&'static self, name: &str) -> Session {
        let guard = self.serial.lock();
        let root = std::env::temp_dir().join(format!("pm-fake-tui-e2e-{}", std::process::id())).join(name);
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
    world: &'static World,
    _serial: MutexGuard<'static, ()>,
    root: PathBuf,
    project: PathBuf,
    aux: PathBuf,
    saved: Arc<Vec<Arc<dyn TuiProfile>>>,
}

impl Session {
    pub fn project(&self) -> &Path {
        &self.project
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
        let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
        let bytes = serde_json::to_vec(&body).expect("json body");
        match crate::agent_runs::handle_request(method, &segments, "", &bytes, &self.world.registry, &self.world.app, None) {
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
            "timeout_ms": 90_000,
            // A pane a person watches stays open after the run, like the cockpit's: its screen and its
            // turn summary are what several tests read.
            "keep_pane": true,
        });
        if let (Some(map), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
            map.extend(extra.clone());
        }
        self.agents("POST", "/agents/run", body)
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

    /// A bare pane of `agent_type` in the project, 120x30, with nothing typed: for tests that read screens.
    pub fn spawn_pane(&self, agent_type: &str) -> Result<String, String> {
        crate::pty::registry_spawn_pane(
            &self.world.registry,
            &self.world.app,
            crate::pty::SpawnPaneArgs {
                agent_type: agent_type.to_string(),
                cwd: Some(self.project.to_string_lossy().into_owned()),
                cols: Some(120),
                rows: Some(30),
                extra_args: None,
                pane_id: None,
                read_only: false,
                account: None,
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

    /// The pane's screen as the emulator holds it.
    pub fn screen(&self, pane_id: &str) -> String {
        registry_read_snapshot(&self.world.registry, pane_id).unwrap_or_default()
    }

    /// The pane's screen as readiness reads it (the profile's `input_screen`).
    pub fn input_screen(&self, pane_id: &str) -> String {
        registry_read_snapshot_full(&self.world.registry, pane_id).unwrap_or_default()
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

    /// The desktop UI's answer to a card (`answer_pane_prompt`): the same function the Allow button calls.
    pub fn answer_card(&self, pane_id: &str, prompt_id: &str, choice: &str) -> Result<Value, Value> {
        crate::agent_runs::answer_for_pane(&self.world.registry, &self.world.app, pane_id, prompt_id, choice)
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
            lock_screen: false,
        };
        let env = crate::attention::answer::RealEnv { registry: self.world.registry.clone(), app: self.world.app.clone() };
        crate::attention::answer::answer(&env, &caller, item_id, &request)
    }

    /// The summaries of the audit log's approval lines so far, oldest first (the log is shared by the whole
    /// test process).
    pub fn approval_audit_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = crate::audit::global().recent(500).into_iter().filter(|entry| entry.kind == "approval").map(|entry| entry.summary).collect();
        lines.reverse();
        lines
    }

    /// The turn summaries kept for a pane (the card the cockpit shows after a turn).
    pub fn turn_summaries(&self, pane_id: &str) -> Value {
        serde_json::to_value(crate::turn_summary::tracker().summaries(pane_id)).unwrap_or(Value::Null)
    }

    /// The pane's PTY grid changed (a window resize): what a viewer's resize does to the agent.
    pub fn resize(&self, pane_id: &str, cols: u16, rows: u16) -> Result<bool, String> {
        crate::pty::registry_resize(&self.world.registry, pane_id, cols, rows)
    }

    pub fn close_pane(&self, pane_id: &str) {
        let _ = registry_kill_pane(&self.world.registry, pane_id);
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

impl Drop for Session {
    fn drop(&mut self) {
        let panes = self.pane_ids();
        for pane in panes {
            self.close_pane(&pane);
        }
        crate::tui::registry::registry().swap(self.saved.iter().cloned().collect());
        *self.world.script.lock() = None;
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
