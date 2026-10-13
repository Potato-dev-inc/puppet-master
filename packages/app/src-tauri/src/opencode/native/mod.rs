//! Spawn an OpenCode native worker: `opencode serve` + API session + attach TUI.

use crate::events::{PaneId, SystemEvent};
use crate::opencode::client;
use crate::opencode::OpenCodeLink;
use crate::pty::registry::{PaneInfo, PaneRegistry, PaneState, SpawnPaneArgs};
use crate::pty::scrollback::Scrollback;
use crate::pty::status::PaneStatus;
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::Read;
use std::process::Command as StdCommand;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use tracing::{error, info};
use uuid::Uuid;

mod process;
mod reattach;

use process::*;
pub use reattach::*;

const SCROLLBACK_CAP: usize = 10_000;
const PORT_MIN: u16 = 4096;
const PORT_MAX: u16 = 4199;
const SERVE_START_TIMEOUT: Duration = Duration::from_secs(45);

pub fn spawn_native_pane(
    registry: Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    args: SpawnPaneArgs,
) -> Result<String, String> {
    if let Err(err) = crate::opencode::keys::apply_active_auth() {
        tracing::warn!(%err, "opencode auth profile not applied before spawn");
    }

    let opencode_exe = crate::shell_env::resolve_opencode_executable()?;

    let cwd = crate::project_path::resolve_spawn_cwd(
        args.cwd.clone(),
        registry.lock().project_path.clone(),
    )?
    .to_string_lossy()
    .to_string();

    let cols = args.cols.unwrap_or(120);
    let rows = args.rows.unwrap_or(30);
    let pane_id = args.pane_id.unwrap_or_else(|| Uuid::new_v4().to_string());

    let replaced = {
        let mut reg = registry.lock();
        reg.take(&pane_id)
    };
    let previous_session = replaced
        .as_ref()
        .and_then(|pane| pane.opencode.as_ref().map(|link| link.session_id.clone()));
    if let Some(pane) = replaced {
        crate::pty::registry::shutdown_pane(pane);
        crate::event_log::append_system_event(SystemEvent::PaneKilled {
            pane_id: PaneId(pane_id.clone()),
            reason: Some("native_respawn_replaced".into()),
        });
    }

    crate::startup_timing::mark(&pane_id, "native_spawn_begin");
    let port = pick_free_port(PORT_MIN, PORT_MAX)?;
    let base_url = format!("http://127.0.0.1:{port}");

    let mut serve = StdCommand::new(&opencode_exe);
    serve
        .args([
            "serve",
            "--hostname",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ])
        .current_dir(&cwd)
        .env("PATH", crate::shell_env::path_for_spawn());
    #[cfg(windows)]
    serve.env("Path", crate::shell_env::path_for_spawn());
    if args.read_only {
        serve.env(
            "OPENCODE_CONFIG_CONTENT",
            crate::tui::opencode::read_only::config_content(),
        );
    } else {
        serve.env_remove("OPENCODE_CONFIG_CONTENT");
    }
    crate::tui::opencode::read_only::set_enforced(&pane_id, args.read_only);

    let mut serve_child = serve
        .spawn()
        .map_err(|err| format!("opencode serve spawn ({opencode_exe}): {err}"))?;

    crate::startup_timing::mark(&pane_id, "serve_spawned");
    let health = client::wait_for_health(&base_url, SERVE_START_TIMEOUT);
    if health.is_ok() {
        crate::startup_timing::mark(&pane_id, "health_ready");
    }
    if let Err(err) = health {
        crate::pty::proc_tree::kill_process_tree(serve_child.id());
        let _ = serve_child.kill();
        error!(%pane_id, %base_url, %err, "opencode serve failed health check");
        return Err(err);
    }

    let reused_session = previous_session
        .as_deref()
        .filter(|id| !id.is_empty())
        .and_then(|id| client::get_session(&base_url, id, Some(&cwd)).ok())
        .map(|session| session.id)
        .filter(|id| !id.is_empty());
    let session_id = if let Some(id) = reused_session {
        info!(%pane_id, %base_url, session_id = %id, "opencode native worker reattached session");
        id
    } else {
        match client::create_session(&base_url, &format!("puppet-master {pane_id}")) {
            Ok(session) => session.id,
            Err(err) => {
                crate::pty::proc_tree::kill_process_tree(serve_child.id());
                let _ = serve_child.kill();
                error!(%pane_id, %err, "opencode create session failed");
                return Err(err);
            }
        }
    };
    crate::startup_timing::mark(&pane_id, "session_created");
    info!(%pane_id, %base_url, session_id = %session_id, "opencode native worker ready");

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty: {e}"));
    let pair = match pair {
        Ok(pair) => pair,
        Err(err) => {
            kill_serve_child(&mut serve_child);
            return Err(err);
        }
    };

    let cmd = build_attach_command(&opencode_exe, &base_url, &session_id, &cwd);
    let child = match pair.slave.spawn_command(cmd) {
        Ok(child) => child,
        Err(e) => {
            kill_serve_child(&mut serve_child);
            return Err(format!("spawn attach: {e}"));
        }
    };
    let pid = child.process_id().unwrap_or(0);
    crate::startup_timing::mark(&pane_id, "attach_spawned");
    drop(pair.slave);

    let io = pair
        .master
        .take_writer()
        .map_err(|e| format!("take_writer: {e}"))
        .and_then(|writer| {
            pair.master
                .try_clone_reader()
                .map(|reader| (writer, reader))
                .map_err(|e| format!("clone_reader: {e}"))
        });
    let (writer, reader) = match io {
        Ok(io) => io,
        Err(err) => {
            kill_serve_child(&mut serve_child);
            let mut attach = child;
            if let Some(pid) = attach.process_id() {
                crate::pty::proc_tree::kill_process_tree(pid);
            }
            let _ = attach.kill();
            return Err(err);
        }
    };

    let scrollback = Arc::new(Mutex::new(Scrollback::new(SCROLLBACK_CAP)));
    let screen = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, SCROLLBACK_CAP)));
    let status = Arc::new(Mutex::new(PaneStatus::Running));
    let last_output = Arc::new(Mutex::new(Instant::now()));
    let exited = Arc::new(Mutex::new(false));

    let info = PaneInfo {
        id: pane_id.clone(),
        agent_type: "opencode_native".to_string(),
        pid,
        status: "running".into(),
        created_at: chrono_now_ms(),
        last_output_at: Some(chrono_now_ms()),
        cwd: cwd.clone(),
        cols,
        rows,
        viewer_held: false,
        reprints_on_resize: false,
    };

    let opencode = OpenCodeLink::new(
        base_url.clone(),
        session_id.clone(),
        cwd.clone(),
        serve_child,
    );
    let serve_handle = opencode.serve_handle();
    let reattaching = opencode.reattaching();
    let keep_serve_on_attach_exit = opencode.keep_serve_on_attach_exit();
    let attach_generation = opencode.attach_generation();

    let pane = PaneState {
        info,
        scrollback: scrollback.clone(),
        screen: screen.clone(),
        status: status.clone(),
        last_output: last_output.clone(),
        master: Arc::new(Mutex::new(pair.master)),
        writer: Arc::new(Mutex::new(writer)),
        child,
        exited: exited.clone(),
        opencode: Some(opencode),
        opencode_key_event: None,
    };

    spawn_reader_thread(
        pane_id.clone(),
        app.clone(),
        reader,
        scrollback,
        screen,
        status,
        last_output,
        exited,
        serve_handle,
        reattaching,
        keep_serve_on_attach_exit,
        attach_generation.load(std::sync::atomic::Ordering::SeqCst),
        attach_generation,
    );

    registry.lock().panes.insert(pane_id.clone(), pane);

    crate::opencode::watch::ensure_watching(pane_id.clone(), app.clone(), Arc::clone(&registry));

    crate::event_log::append_system_event(SystemEvent::PaneSpawned {
        pane_id: PaneId(pane_id.clone()),
        agent_type: "opencode_native".to_string(),
        pid,
        cwd: cwd.clone(),
        cols,
        rows,
    });
    info!(pane = %pane_id, agent = "opencode_native", pid, "pane spawned");
    let _ = app.emit("pty://panes-changed", ());
    if let Some(previous_session) = previous_session {
        crate::agent_runs::on_native_pane_replaced(
            &pane_id,
            &previous_session,
            &session_id,
            &registry,
            app,
        );
    }

    Ok(pane_id)
}

pub fn restart_native_pane(
    registry: Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    pane_id: &str,
) -> Result<String, String> {
    let args = {
        let reg = registry.lock();
        let pane = reg
            .panes
            .get(pane_id)
            .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
        if pane.info.agent_type != "opencode_native" {
            return Err(format!("pane {pane_id} is not opencode_native"));
        }
        SpawnPaneArgs {
            agent_type: pane.info.agent_type.clone(),
            cwd: Some(pane.info.cwd.clone()),
            cols: Some(pane.info.cols),
            rows: Some(pane.info.rows),
            extra_args: None,
            pane_id: Some(pane_id.to_string()),
            read_only: crate::tui::opencode::read_only::is_enforced(pane_id),
        }
    };
    crate::opencode::keys::apply_active_auth()?;
    spawn_native_pane(registry, app, args)
}

pub fn restart_all_native_panes(
    registry: Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
) -> Result<Vec<String>, String> {
    let pane_ids: Vec<String> = registry
        .lock()
        .list()
        .into_iter()
        .filter(|pane| pane.agent_type == "opencode_native")
        .map(|pane| pane.id)
        .collect();
    let mut restarted = Vec::new();
    for pane_id in pane_ids {
        restart_native_pane(Arc::clone(&registry), app, &pane_id)?;
        restarted.push(pane_id);
    }
    Ok(restarted)
}

#[cfg(test)]
mod tests;
