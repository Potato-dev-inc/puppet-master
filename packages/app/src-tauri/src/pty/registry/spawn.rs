//! Pane lifecycle start: spawn the PTY, run the reader thread, emit pane events.

use super::*;

/// Message for a pane lookup that failed. Keeps the `unknown pane:` prefix that callers match on.
pub fn unknown_pane_message(pane_id: &str) -> String {
    format!(
        "unknown pane: {pane_id}; pane_id must be a full pane id from list_panes (not a UI number or an agent run handle)"
    )
}

/// Spawn a pane. The reader thread pushes raw bytes to the scrollback,
/// emits Tauri events, and updates status heuristics.
///
/// Returns the new pane id.
pub fn spawn_pane(
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    args: SpawnPaneArgs,
) -> Result<String, String> {
    if args.agent_type == "opencode_native" {
        return crate::opencode::native::spawn_native_pane(Arc::clone(registry), app, args);
    }

    let agent = AgentType::parse(&args.agent_type)
        .ok_or_else(|| format!("unknown agent_type: {}", args.agent_type))?;

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
    if let Some(pane) = replaced {
        shutdown_pane(pane);
        crate::event_log::append_system_event(SystemEvent::PaneKilled {
            pane_id: PaneId(pane_id.clone()),
            reason: Some("respawn_replaced".into()),
        });
    }

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty: {e}"))?;

    let cmd = build_command(agent, &cwd, args.extra_args.as_deref().unwrap_or(&[]));
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn_command: {e}"))?;
    let pid = child.process_id().unwrap_or(0);

    // Slave handle is now owned by the child; drop our copy so the master
    // gets EOF when the child exits.
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
    // Mirror the live screen at the PTY's dimensions with a matching scrollback
    // depth so reattach can reconstruct a coherent grid.
    let screen = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, SCROLLBACK_CAP)));
    let status = Arc::new(Mutex::new(PaneStatus::Running));
    let last_output = Arc::new(Mutex::new(Instant::now()));
    let idle_clock = Arc::new(idle::IdleClock::new(Instant::now()));
    let idle_profile = crate::tui::profile_for(&args.agent_type);
    let (idle_after, quiet_idle_after) = (idle_profile.idle_after(), idle_profile.quiet_idle_after());
    // 0 = unknown, 1 = the screen shows work, 2 = it provably does not (see `TuiProfile::busy_marker`).
    let busy_state = Arc::new(std::sync::atomic::AtomicU8::new(0));
    let exited = Arc::new(Mutex::new(false));

    let info = PaneInfo {
        id: pane_id.clone(),
        agent_type: args.agent_type.clone(),
        pid,
        status: "running".into(),
        created_at: chrono_now_ms(),
        last_output_at: Some(chrono_now_ms()),
        cwd: cwd.clone(),
        cols,
        rows,
    };

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
        opencode: None,
        opencode_key_event: None,
    };

    // Spawn the reader thread.
    {
        let pane_id = pane_id.clone();
        let app = app.clone();
        let agent_type = args.agent_type.clone();
        let mut adapter = crate::agent_adapters::adapter_for(&args.agent_type);
        let scrollback = scrollback.clone();
        let screen = screen.clone();
        let status = status.clone();
        let last_output = last_output.clone();
        let idle_clock = idle_clock.clone();
        let busy_state = busy_state.clone();
        let exited = exited.clone();
        thread::Builder::new()
            .name(format!("pty-reader-{pane_id}"))
            .spawn(move || {
                let mut reader = reader;
                let mut buf = [0u8; 4096];
                let mut last_snapshot = String::new();
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => {
                            debug!(pane = %pane_id, "pty reader EOF");
                            break;
                        }
                        Ok(n) => {
                            crate::event_log::append_system_event(
                                SystemEvent::PaneOutputObserved {
                                    pane_id: PaneId(pane_id.clone()),
                                    byte_count: n,
                                },
                            );
                            let clean_text = strip_ansi(&String::from_utf8_lossy(&buf[..n]));
                            for observation in adapter.observe(&pane_id, &clean_text) {
                                crate::event_log::append_system_event(observation);
                            }
                            // Push RAW BYTES to scrollback — never decode to
                            // String here, as from_utf8_lossy corrupts multi-byte
                            // UTF-8 at chunk boundaries.
                            scrollback.lock().push_chunk(&buf[..n]);
                            // Emit raw bytes for the xterm.js renderer, while
                            // also feeding the headless emulator used by MCP
                            // buffer reads and reattach snapshots.
                            let chunk = buf[..n].to_vec();
                            let _ = app.emit(
                                "terminal-data",
                                TerminalDataEvent {
                                    pane_id: pane_id.clone(),
                                    data: chunk,
                                },
                            );
                            let snapshot = {
                                let mut parser = screen.lock();
                                parser.process(&buf[..n]);
                                parser.screen().contents()
                            };
                            *last_output.lock() = Instant::now();

                            // Output that leaves every visible cell alone (cursor and
                            // attribute sequences) is not activity: it neither postpones
                            // idleness nor re-runs the classifier, which would flip an idle
                            // pane back to running on every chunk.
                            if snapshot == last_snapshot {
                                continue;
                            }
                            idle_clock.screen_changed(Instant::now());
                            busy_state.store(
                                match crate::pty::status::busy_marker(&agent_type, &snapshot) {
                                    Some(true) => 1,
                                    Some(false) => 2,
                                    None => 0,
                                },
                                std::sync::atomic::Ordering::Relaxed,
                            );

                            // Classify the current VT screen. Historical prompt text in
                            // scrollback must not make a later agent response look blocked.
                            let new_status = crate::pty::status::classify_agent_observation(
                                &agent_type,
                                &snapshot,
                                "",
                            );
                            let changed = {
                                let mut s = status.lock();
                                if *s != new_status {
                                    *s = new_status;
                                    true
                                } else {
                                    false
                                }
                            };

                            last_snapshot = snapshot.clone();
                            let _ = app.emit(
                                "terminal-snapshot",
                                TerminalSnapshotEvent {
                                    pane_id: pane_id.clone(),
                                    snapshot,
                                },
                            );

                            if changed {
                                let s = *status.lock();
                                let _ = app.emit(
                                    "pty://status",
                                    PaneStatusEvent {
                                        pane_id: pane_id.clone(),
                                        status: s.as_str().to_string(),
                                    },
                                );
                                crate::event_log::append_system_event(
                                    SystemEvent::PaneStatusChanged {
                                        pane_id: PaneId(pane_id.clone()),
                                        status: s.as_str().to_string(),
                                    },
                                );
                            }
                        }
                        Err(e) => {
                            // On Windows ConPTY, ERROR_BROKEN_PIPE (after child exit) is expected.
                            if e.kind() == std::io::ErrorKind::BrokenPipe
                                || e.kind() == std::io::ErrorKind::UnexpectedEof
                            {
                                debug!(pane = %pane_id, "pty broken pipe: {e}");
                                break;
                            }
                            // Treat any other read error as fatal for the reader thread.
                            error!(pane = %pane_id, "pty read error: {e}");
                            break;
                        }
                    }
                }
                *exited.lock() = true;
                *status.lock() = PaneStatus::Error;
                crate::event_log::append_system_event(SystemEvent::PaneStatusChanged {
                    pane_id: PaneId(pane_id.clone()),
                    status: PaneStatus::Error.as_str().to_string(),
                });
                let _ = app.emit(
                    "pty://exit",
                    PaneExitEvent {
                        pane_id: pane_id.clone(),
                    },
                );
            })
            .map_err(|e| format!("spawn reader thread: {e}"))?;
    }

    // Idle watcher thread: flips Running -> Idle once the screen has held still for the profile's idle_after.
    {
        let pane_id = pane_id.clone();
        let app = app.clone();
        let idle_clock = idle_clock.clone();
        let busy_state = busy_state.clone();
        let status = status.clone();
        let exited = exited.clone();
        thread::Builder::new()
            .name(format!("pty-idle-{pane_id}"))
            .spawn(move || loop {
                thread::sleep(idle::IDLE_POLL);
                if *exited.lock() {
                    break;
                }
                let marker = match busy_state.load(std::sync::atomic::Ordering::Relaxed) {
                    1 => Some(true),
                    2 => Some(false),
                    _ => None,
                };
                let window = idle::idle_window(marker, idle_after, quiet_idle_after);
                if idle_clock.idle_due(Instant::now(), window) {
                    let became_idle = {
                        let mut s = status.lock();
                        if *s == PaneStatus::Running {
                            *s = PaneStatus::Idle;
                            true
                        } else {
                            false
                        }
                    };
                    if became_idle {
                        let _ = app.emit(
                            "pty://status",
                            PaneStatusEvent {
                                pane_id: pane_id.clone(),
                                status: PaneStatus::Idle.as_str().to_string(),
                            },
                        );
                        crate::event_log::append_system_event(SystemEvent::PaneStatusChanged {
                            pane_id: PaneId(pane_id.clone()),
                            status: PaneStatus::Idle.as_str().to_string(),
                        });
                    }
                }
            })
            .map_err(|e| format!("spawn idle thread: {e}"))?;
    }

    let id = pane.info.id.clone();
    registry.lock().panes.insert(pane.info.id.clone(), pane);
    let _ = app.emit("pty://panes-changed", ());
    crate::event_log::append_system_event(SystemEvent::PaneSpawned {
        pane_id: PaneId(id.clone()),
        agent_type: args.agent_type.clone(),
        pid,
        cwd,
        cols,
        rows,
    });
    info!(pane = %id, agent = %args.agent_type, pid, "pane spawned");
    Ok(id)
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct PaneStatusEvent {
    pane_id: String,
    status: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct PaneExitEvent {
    pane_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct TerminalSnapshotEvent {
    pane_id: String,
    snapshot: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct TerminalDataEvent {
    pane_id: String,
    data: Vec<u8>,
}
