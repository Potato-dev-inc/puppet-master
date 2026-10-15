//! Re-attaching the TUI of a native OpenCode pane to its running `opencode serve`.

use super::*;

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
    let probe = server_probe(base_url, session_id, directory);
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
        probe,
    );
    Ok(())
}
