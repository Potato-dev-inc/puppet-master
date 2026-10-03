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

/// Re-spawn `opencode attach` against the existing serve session.
/// Footer model comes from last user message on attach — stamp that before calling.
pub fn reattach_tui(
    registry: Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    pane_id: &str,
) -> Result<(), String> {
    let opencode_exe = crate::shell_env::resolve_opencode_executable()?;
    let (
        base_url,
        session_id,
        directory,
        cols,
        rows,
        reattaching,
        keep_serve,
        attach_generation,
        scrollback,
        screen,
        status,
        last_output,
        exited,
    ) = {
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

    // Drain in-flight ConPTY writes without holding PaneRegistry.
    let (writer_slot, master_slot) = {
        let reg = registry.lock();
        let pane = reg
            .panes
            .get(pane_id)
            .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
        (pane.writer.clone(), pane.master.clone())
    };
    *writer_slot.lock() = writer;
    *master_slot.lock() = pair.master;

    {
        let mut reg = registry.lock();
        let pane = reg
            .panes
            .get_mut(pane_id)
            .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
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

fn kill_serve_child(child: &mut std::process::Child) {
    crate::pty::proc_tree::kill_process_tree(child.id());
    let _ = child.kill();
    let _ = child.wait();
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
        let mut first_output = true;
        let mut painted = false;
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = &buf[..n];
                    scrollback.lock().push_chunk(chunk);
                    screen.lock().process(chunk);
                    *last_output.lock() = Instant::now();
                    *status.lock() = PaneStatus::Running;
                    if first_output {
                        first_output = false;
                        crate::startup_timing::mark(&pane_id, "first_tui_output");
                    }
                    // Wake wait_for_panes until the TUI has painted text: that is what
                    // flips "running with visible output", and nothing else bumps.
                    if !painted {
                        painted = !screen.lock().screen().contents().trim().is_empty();
                        crate::pane_wait_notify::bump_waiters();
                    }
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
                    kill_serve_child(&mut child);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Manual measurement: `cargo test --lib measure_native_startup -- --ignored --nocapture`.
    /// Needs `opencode` installed. Prints per-stage ms (serve spawn -> health -> session
    /// -> attach spawn -> first TUI output).
    #[test]
    #[ignore]
    fn measure_native_startup() {
        let pre = Instant::now();
        let _ = crate::opencode::keys::apply_active_auth();
        println!("apply_active_auth={}ms", pre.elapsed().as_millis());
        let pre = Instant::now();
        let exe = crate::shell_env::resolve_opencode_executable().expect("opencode installed");
        println!("resolve_exe={}ms ({exe})", pre.elapsed().as_millis());
        let cwd = std::env::temp_dir().to_string_lossy().to_string();
        for run in 0..3 {
            let t0 = Instant::now();
            let port = pick_free_port(PORT_MIN, PORT_MAX).unwrap();
            let base_url = format!("http://127.0.0.1:{port}");
            let mut serve = StdCommand::new(&exe);
            serve
                .args(["serve", "--hostname", "127.0.0.1", "--port", &port.to_string()])
                .current_dir(&cwd)
                .env("PATH", crate::shell_env::path_for_spawn());
            let mut child = serve.spawn().unwrap();
            let spawned = t0.elapsed().as_millis();
            client::wait_for_health(&base_url, Duration::from_secs(45)).unwrap();
            let health = t0.elapsed().as_millis();
            let session = client::create_session(&base_url, "measure").unwrap().id;
            let sess = t0.elapsed().as_millis();
            let pair = native_pty_system()
                .openpty(PtySize { rows: 30, cols: 120, pixel_width: 0, pixel_height: 0 })
                .unwrap();
            let mut attach = pair
                .slave
                .spawn_command(build_attach_command(&exe, &base_url, &session, &cwd))
                .unwrap();
            let attached = t0.elapsed().as_millis();
            drop(pair.slave);
            let mut reader = pair.master.try_clone_reader().unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            thread::spawn(move || {
                let mut parser = vt100::Parser::new(30, 120, 0);
                let mut buf = [0u8; 4096];
                let mut first = None;
                loop {
                    let n = reader.read(&mut buf).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    if first.is_none() {
                        first = Some(());
                        let _ = tx.send((true, false));
                    }
                    parser.process(&buf[..n]);
                    if !parser.screen().contents().trim().is_empty() {
                        let _ = tx.send((false, true));
                        break;
                    }
                }
            });
            let mut first = 0;
            let mut text = 0;
            while let Ok((raw, vis)) = rx.recv_timeout(Duration::from_secs(30)) {
                if raw {
                    first = t0.elapsed().as_millis();
                }
                if vis {
                    text = t0.elapsed().as_millis();
                    break;
                }
            }
            println!(
                "run={run} serve_spawned={spawned}ms health={health}ms session={sess}ms attach_spawned={attached}ms first_tui_output={first}ms first_visible_text={text}ms"
            );
            crate::pty::proc_tree::kill_process_tree(attach.process_id().unwrap_or(0));
            let _ = attach.kill();
            kill_serve_child(&mut child);
        }
    }
}
