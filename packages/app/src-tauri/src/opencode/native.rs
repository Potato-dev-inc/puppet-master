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

    {
        let mut reg = registry.lock();
        if reg.panes.contains_key(&pane_id) {
            reg.kill(&pane_id);
            crate::event_log::append_system_event(SystemEvent::PaneKilled {
                pane_id: PaneId(pane_id.clone()),
            });
        }
    }

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

    let mut serve_child = serve
        .spawn()
        .map_err(|err| format!("opencode serve spawn ({opencode_exe}): {err}"))?;

    if let Err(err) = client::wait_for_health(&base_url, SERVE_START_TIMEOUT) {
        let _ = serve_child.kill();
        error!(%pane_id, %base_url, %err, "opencode serve failed health check");
        return Err(err);
    }

    let session = match client::create_session(&base_url, &format!("puppet-master {pane_id}")) {
        Ok(session) => session,
        Err(err) => {
            let _ = serve_child.kill();
            error!(%pane_id, %err, "opencode create session failed");
            return Err(err);
        }
    };
    info!(%pane_id, %base_url, session_id = %session.id, "opencode native worker ready");

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty: {e}"))?;

    let cmd = build_attach_command(&opencode_exe, &base_url, &session.id, &cwd);
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn attach: {e}"))?;
    let pid = child.process_id().unwrap_or(0);
    drop(pair.slave);

    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("take_writer: {e}"))?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("clone_reader: {e}"))?;

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
    };

    let opencode = OpenCodeLink::new(
        base_url.clone(),
        session.id.clone(),
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
        master: pair.master,
        writer,
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

/// Re-spawn `opencode attach` against the existing serve session.
/// Footer model comes from last user message on attach — stamp that before calling.
pub fn reattach_tui(
    registry: Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    pane_id: &str,
) -> Result<(), String> {
    let opencode_exe = crate::shell_env::resolve_opencode_executable()?;
    let (base_url, session_id, directory, cols, rows, reattaching, keep_serve, attach_generation, scrollback, screen, status, last_output, exited) = {
        let reg = registry.lock();
        let pane = reg
            .panes
            .get(pane_id)
            .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
        if pane.info.agent_type != "opencode_native" {
            return Err(format!("pane {pane_id} is not opencode_native"));
        }
        let link = pane
            .opencode
            .as_ref()
            .ok_or_else(|| format!("pane {pane_id} has no opencode link"))?;
        (
            link.base_url.clone(),
            link.session_id.clone(),
            link.directory.clone(),
            pane.info.cols,
            pane.info.rows,
            link.reattaching(),
            link.keep_serve_on_attach_exit(),
            link.attach_generation(),
            Arc::clone(&pane.scrollback),
            Arc::clone(&pane.screen),
            Arc::clone(&pane.status),
            Arc::clone(&pane.last_output),
            Arc::clone(&pane.exited),
        )
    };

    reattaching.store(true, std::sync::atomic::Ordering::SeqCst);
    let generation = {
        let reg = registry.lock();
        let pane = reg
            .panes
            .get(pane_id)
            .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
        pane.opencode
            .as_ref()
            .map(|link| link.bump_attach_generation())
            .ok_or_else(|| format!("pane {pane_id} has no opencode link"))?
    };
    {
        let mut reg = registry.lock();
        let pane = reg
            .panes
            .get_mut(pane_id)
            .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
        let _ = pane.child.kill();
    }
    thread::sleep(Duration::from_millis(100));

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty: {e}"))?;
    // attach does not accept -m; TUI reads last user message model on load
    let cmd = build_attach_command(&opencode_exe, &base_url, &session_id, &directory);
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn attach: {e}"))?;
    let pid = child.process_id().unwrap_or(0);
    drop(pair.slave);
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("take_writer: {e}"))?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("clone_reader: {e}"))?;

    {
        let mut reg = registry.lock();
        let pane = reg
            .panes
            .get_mut(pane_id)
            .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
        pane.master = pair.master;
        pane.writer = writer;
        pane.child = child;
        pane.info.pid = pid;
        *pane.exited.lock() = false;
        *pane.status.lock() = PaneStatus::Running;
        pane.scrollback.lock().clear();
        *pane.screen.lock() = vt100::Parser::new(rows, cols, SCROLLBACK_CAP);
    }

    reattaching.store(false, std::sync::atomic::Ordering::SeqCst);
    crate::event_log::append_system_event(crate::events::SystemEvent::PaneTuiReattached {
        pane_id: crate::events::PaneId(pane_id.to_string()),
        attach_generation: generation,
    });
    crate::pane_wait_notify::bump_waiters();
    spawn_reader_thread(
        pane_id.to_string(),
        app.clone(),
        reader,
        scrollback,
        screen,
        status,
        last_output,
        exited,
        opencode_serve_handle(&registry, pane_id)?,
        reattaching,
        keep_serve,
        generation,
        attach_generation,
    );
    Ok(())
}

fn opencode_serve_handle(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
) -> Result<Arc<std::sync::Mutex<Option<std::process::Child>>>, String> {
    let reg = registry.lock();
    let pane = reg
        .panes
        .get(pane_id)
        .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
    let link = pane
        .opencode
        .as_ref()
        .ok_or_else(|| format!("pane {pane_id} has no opencode link"))?;
    Ok(link.serve_handle())
}

fn pick_free_port(lo: u16, hi: u16) -> Result<u16, String> {
    for port in lo..=hi {
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Ok(port);
        }
    }
    Err(format!("no free TCP port in {lo}-{hi} for opencode serve"))
}

fn build_attach_command(
    opencode_exe: &str,
    base_url: &str,
    session_id: &str,
    cwd: &str,
) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(opencode_exe);
    cmd.arg("attach");
    cmd.arg(base_url);
    cmd.arg("-s");
    cmd.arg(session_id);
    cmd.cwd(cwd);
    apply_pty_env(&mut cmd);
    cmd
}

fn apply_pty_env(cmd: &mut CommandBuilder) {
    cmd.env("PATH", crate::shell_env::path_for_spawn());
    #[cfg(windows)]
    cmd.env("Path", crate::shell_env::path_for_spawn());
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
}

fn spawn_reader_thread(
    pane_id: String,
    app: AppHandle,
    mut reader: Box<dyn Read + Send>,
    scrollback: Arc<Mutex<Scrollback>>,
    screen: Arc<Mutex<vt100::Parser>>,
    status: Arc<Mutex<PaneStatus>>,
    last_output: Arc<Mutex<Instant>>,
    exited: Arc<Mutex<bool>>,
    serve_handle: Arc<std::sync::Mutex<Option<std::process::Child>>>,
    reattaching: Arc<std::sync::atomic::AtomicBool>,
    keep_serve_on_attach_exit: Arc<std::sync::atomic::AtomicBool>,
    reader_generation: u64,
    attach_generation: Arc<std::sync::atomic::AtomicU64>,
) {
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut adapter = crate::agent_adapters::adapter_for("opencode_native");
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = &buf[..n];
                    scrollback.lock().push_chunk(chunk);
                    screen.lock().process(chunk);
                    *last_output.lock() = Instant::now();
                    *status.lock() = PaneStatus::Running;
                    let text = String::from_utf8_lossy(chunk);
                    for event in adapter.observe(&pane_id, &text) {
                        crate::event_log::append_system_event(event);
                    }
                    let payload: Vec<u8> = chunk.to_vec();
                    let _ = app.emit(
                        "terminal-data",
                        serde_json::json!({ "pane_id": pane_id, "data": payload }),
                    );
                }
                Err(_) => break,
            }
        }
        if attach_generation.load(std::sync::atomic::Ordering::SeqCst) != reader_generation {
            return;
        }
        if reattaching.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        *exited.lock() = true;
        *status.lock() = PaneStatus::Error;
        if !keep_serve_on_attach_exit.load(std::sync::atomic::Ordering::SeqCst) {
            if let Ok(mut guard) = serve_handle.lock() {
                if let Some(mut child) = guard.take() {
                    let _ = child.kill();
                }
            }
        }
        let _ = app.emit(
            "pty://status",
            serde_json::json!({ "pane_id": pane_id, "status": "error" }),
        );
        let _ = app.emit("pty://exit", serde_json::json!({ "pane_id": pane_id }));
    });
}

fn chrono_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
