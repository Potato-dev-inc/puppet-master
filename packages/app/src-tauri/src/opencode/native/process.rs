//! Process plumbing for native panes: serve handle, free port, attach command, PTY reader thread.

use super::*;

pub(super) fn opencode_serve_handle(
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

pub(super) fn kill_serve_child(child: &mut std::process::Child) {
    crate::pty::proc_tree::kill_process_tree(child.id());
    let _ = child.kill();
    let _ = child.wait();
}

pub(super) fn pick_free_port(lo: u16, hi: u16) -> Result<u16, String> {
    for port in lo..=hi {
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Ok(port);
        }
    }
    Err(format!("no free TCP port in {lo}-{hi} for opencode serve"))
}

pub(super) fn build_attach_command(
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

pub(super) fn apply_pty_env(cmd: &mut CommandBuilder) {
    cmd.env("PATH", crate::shell_env::path_for_spawn());
    #[cfg(windows)]
    cmd.env("Path", crate::shell_env::path_for_spawn());
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
}

#[allow(clippy::too_many_arguments)] // bundles the per-pane handles the reader thread owns
pub(super) fn spawn_reader_thread(
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
    probe: SessionProbe,
) {
    let tracker = Arc::new(NativeStatus::new(status.clone(), Instant::now()));
    spawn_idle_watcher(
        pane_id.clone(),
        app.clone(),
        tracker.clone(),
        probe,
        exited.clone(),
        reader_generation,
        attach_generation.clone(),
    );
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
                    let visible = screen.lock().screen().contents();
                    // Output that leaves every visible cell alone is not activity; only a
                    // changed screen restarts the idle window or wakes an idle pane.
                    if let Some(woke) = tracker.on_output(
                        &visible,
                        Instant::now(),
                        crate::pty::resize_clock::recently_resized(&pane_id),
                    ) {
                        publish_status(&app, &pane_id, woke);
                    }
                    if first_output {
                        first_output = false;
                        crate::startup_timing::mark(&pane_id, "first_tui_output");
                    }
                    // Wake wait_for_panes until the TUI has painted text: that is what
                    // flips "running with visible output", and nothing else bumps.
                    if !painted {
                        painted = !visible.trim().is_empty();
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

pub(super) fn chrono_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
