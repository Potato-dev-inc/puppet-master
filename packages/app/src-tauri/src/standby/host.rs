//! The live `Host`: real hidden PTY panes and real waiting headless processes.
//!
//! Agent-specific knowledge stays in `tui/<agent>/`: this file asks the profile for the launch
//! line (`headless_standby_command`) and for what a pane's screen means (`standby_screen`).

use super::key::PoolKey;
use super::pool::{Handle, Health, Host};
use crate::pty::status::PaneStatus;
use crate::pty::{PaneRegistry, SpawnPaneArgs};
use crate::tui::{HeadlessLaunch, StandbyScreen};
use super::stillness::Stillness;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tauri::AppHandle;

/// A headless standby process prints nothing until it gets its task, so readiness cannot be
/// observed: it counts as warm after its startup has had this long (a Claude standby spent ~1s
/// on startup in the live probe).
const HEADLESS_WARM_MS: i64 = 4_000;

/// A headless CLI process started ahead of its task, with stdin still open.
pub struct StandbyProcess {
    pub child: Child,
}

pub struct LiveHost {
    registry: Arc<Mutex<PaneRegistry>>,
    app: AppHandle,
    processes: Mutex<HashMap<u64, StandbyProcess>>,
    next_token: AtomicU64,
    /// Stillness state of experimental panes, by pane id (`standby::stillness`).
    stillness: Mutex<HashMap<String, Stillness>>,
    started: Instant,
}

impl LiveHost {
    pub fn new(registry: Arc<Mutex<PaneRegistry>>, app: AppHandle) -> Self {
        Self {
            registry,
            app,
            processes: Mutex::new(HashMap::new()),
            next_token: AtomicU64::new(1),
            stillness: Mutex::new(HashMap::new()),
            started: Instant::now(),
        }
    }

    /// Hand a claimed headless standby to the run that claimed it.
    pub fn take_process(&self, token: u64) -> Option<StandbyProcess> {
        self.processes.lock().remove(&token)
    }

    fn spawn_pane(&self, key: &PoolKey, cwd: &str) -> Result<Handle, String> {
        let pane_id = uuid::Uuid::new_v4().to_string();
        // Hidden BEFORE the spawn: the spawn announces a panes change, and no list may show it.
        crate::pty::hidden::hide(&pane_id);
        match crate::pty::registry_spawn_pane(
            &self.registry,
            &self.app,
            SpawnPaneArgs {
                agent_type: key.agent_type.clone(),
                cwd: Some(cwd.to_string()),
                cols: None,
                rows: None,
                extra_args: None,
                pane_id: Some(pane_id.clone()),
                read_only: key.read_only,
                account: key.account.clone(),
                guard_env: None,
            },
        ) {
            Ok(id) => {
                // Preload: the pane's window is created now, hidden, so a claim only shows it.
                let (cols, rows) = self
                    .registry
                    .lock()
                    .list_all()
                    .into_iter()
                    .find(|pane| pane.id == id)
                    .map(|pane| (pane.cols, pane.rows))
                    .unwrap_or((120, 30));
                crate::app_windows::open_warm_pane_window(&self.app, &id, &key.agent_type, cols, rows);
                Ok(Handle::Pane(id))
            }
            Err(error) => {
                crate::pty::hidden::forget(&pane_id);
                Err(error)
            }
        }
    }

    fn spawn_process(&self, key: &PoolKey, cwd: &str) -> Result<Handle, String> {
        let profile = crate::tui::profile_for(&key.agent_type);
        let launch = HeadlessLaunch {
            project: cwd,
            task: "",
            read_only: key.read_only,
            resume_session: None,
            trust_workspace: false,
            allow_shell: key.allow_shell,
            model: key.model.as_deref(),
        };
        let spec = profile
            .headless_standby_command(&launch)
            .ok_or_else(|| format!("{} has no headless standby mode", key.agent_type))??;
        if key.read_only && !spec.read_only_enforced {
            return Err("adapter did not enforce read-only mode".into());
        }
        let account_env = match key.account.as_deref() {
            Some(id) => crate::accounts::launch_env(&key.agent_type, id)?,
            None => crate::tui::LaunchEnv::default(),
        };
        let mut command = standby_command(&spec, &account_env);
        let child = command
            .spawn()
            .map_err(|error| format!("spawn failed ({}): {error}", spec.program))?;
        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
        self.processes.lock().insert(
            token,
            StandbyProcess {
                child,
            },
        );
        Ok(Handle::Process(token))
    }

    fn check_pane(&self, key: &PoolKey, pane_id: &str) -> Health {
        let (status, screen) = {
            let registry = self.registry.lock();
            let Some(pane) = registry.panes.get(pane_id) else {
                return Health::Dead;
            };
            if *pane.exited.lock() {
                return Health::Dead;
            }
            let status = *pane.status.lock();
            (status, pane.screen.clone())
        };
        if status == PaneStatus::Error {
            return Health::Dead;
        }
        let text = screen.lock().screen().contents();
        let profile = crate::tui::profile_for(&key.agent_type);
        // An experimental screen must also hold still; every observation feeds the clock, so a
        // screen that changed since the last look restarts it.
        let still = if profile.standby_support().experimental {
            let now_ms = self.started.elapsed().as_millis() as i64;
            self.stillness
                .lock()
                .entry(pane_id.to_string())
                .or_default()
                .observe(&text, now_ms)
        } else {
            true
        };
        match profile.standby_screen(&text) {
            StandbyScreen::Dialog => Health::Dialog,
            StandbyScreen::NeedsAttention => Health::NeedsAttention,
            StandbyScreen::Ready if status == PaneStatus::Idle && still => Health::Ready,
            StandbyScreen::Ready | StandbyScreen::NotReady => Health::Starting,
        }
    }
}

impl Host for LiveHost {
    fn spawn(&self, key: &PoolKey, cwd: &str) -> Result<Handle, String> {
        if !std::path::Path::new(cwd).is_dir() {
            return Err(format!("project folder is gone: {cwd}"));
        }
        if key.headless {
            self.spawn_process(key, cwd)
        } else {
            self.spawn_pane(key, cwd)
        }
    }

    fn check(&self, key: &PoolKey, handle: &Handle, age_ms: i64) -> Health {
        match handle {
            Handle::Pane(id) => self.check_pane(key, id),
            Handle::Process(token) => {
                let mut processes = self.processes.lock();
                let Some(process) = processes.get_mut(token) else {
                    return Health::Dead;
                };
                match process.child.try_wait() {
                    Ok(None) if age_ms >= HEADLESS_WARM_MS => Health::Ready,
                    Ok(None) => Health::Starting,
                    Ok(Some(_)) | Err(_) => Health::Dead,
                }
            }
        }
    }

    fn default_project(&self) -> Option<String> {
        Some(super::default_project(&self.registry))
    }

    fn kill(&self, handle: &Handle, reason: &str) {
        tracing::info!(?handle, reason, "standby shut down");
        match handle {
            Handle::Pane(id) => {
                self.stillness.lock().remove(id);
                // Taking the pane out of the registry is quick; stopping its process tree is not.
                let (registry, id, reason) = (self.registry.clone(), id.clone(), reason.to_string());
                let app = self.app.clone();
                let _ = std::thread::Builder::new()
                    .name("standby-kill".into())
                    .spawn(move || {
                        crate::app_windows::close_warm_pane_window(&app, &id);
                        let _ = crate::pty::registry_kill_pane_with_reason(
                            &registry,
                            &id,
                            Some(&format!("standby_{reason}")),
                        );
                    });
            }
            Handle::Process(token) => {
                if let Some(mut process) = self.take_process(*token) {
                    let _ = std::thread::Builder::new()
                        .name("standby-kill".into())
                        .spawn(move || {
                            let _ = process.child.kill();
                            let _ = process.child.wait();
                        });
                }
            }
        }
    }
}

/// The waiting child for a headless standby: stdin stays open for the task, and the environment is
/// the spec's `[env]` plus the account's, exactly as a headless run would get it.
pub(crate) fn standby_command(spec: &crate::tui::LaunchSpec, account_env: &crate::tui::LaunchEnv) -> Command {
    let mut command = crate::proc_util::hidden_command(&spec.program);
    command
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    spec.apply_to_command(&mut command);
    for (key, value) in &account_env.set {
        command.env(key, value);
    }
    for key in &account_env.remove {
        command.env_remove(key);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}

#[cfg(test)]
mod env_tests {
    use super::*;
    use crate::tui::{LaunchEnv, LaunchSpec};
    use std::io::Read;

    #[test]
    fn headless_standby_process_gets_spec_and_account_env() {
        let (program, args) = if cfg!(windows) {
            ("cmd.exe", vec!["/C".to_string(), "echo S=%PM_SB_S% A=%PM_SB_A%".to_string()])
        } else {
            ("sh", vec!["-c".to_string(), "echo S=$PM_SB_S A=$PM_SB_A".to_string()])
        };
        let mut spec = LaunchSpec::new(program, args, std::env::temp_dir(), false);
        spec.env = vec![("PM_SB_S".into(), "spec-env".into())];
        let account = LaunchEnv { set: vec![("PM_SB_A".into(), "acct-env".into())], remove: vec![] };
        let mut child = standby_command(&spec, &account).spawn().expect("spawn");
        let mut out = String::new();
        child.stdout.take().unwrap().read_to_string(&mut out).unwrap();
        let _ = child.wait();
        assert!(out.contains("S=spec-env") && out.contains("A=acct-env"), "{out}");
    }
}
