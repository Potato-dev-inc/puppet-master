//! Pane lifecycle start: spawn the PTY, run the reader thread, emit pane events.

use super::*;

/// Message for a pane lookup that failed. Keeps the `unknown pane:` prefix that callers match on.
pub fn unknown_pane_message(pane_id: &str) -> String {
    format!(
        "unknown pane: {pane_id}; pane_id must be a full pane id from list_panes (not a UI number or an agent run handle)"
    )
}

/// Mark a pane's process as gone: status Error, `pty://exit` event. Runs once per pane, from the
/// reader thread (EOF) or from the exit watcher, whichever notices first. On Windows ConPTY the
/// reader can stay blocked after the child exits, so the watcher must not depend on EOF.
fn finish_exit(
    pane_id: &str,
    app: &AppHandle,
    status: &Arc<Mutex<PaneStatus>>,
    exited: &Arc<Mutex<bool>>,
) {
    {
        let mut done = exited.lock();
        if *done {
            return;
        }
        *done = true;
    }
    *status.lock() = PaneStatus::Error;
    crate::event_log::append_system_event(SystemEvent::PaneStatusChanged {
        pane_id: PaneId(pane_id.to_string()),
        status: PaneStatus::Error.as_str().to_string(),
    });
    let _ = app.emit(
        "pty://exit",
        PaneExitEvent {
            pane_id: pane_id.to_string(),
        },
    );
}

/// True while process `pid` is running. Unknown pids (0) count as alive so nothing is declared dead
/// without evidence.
#[cfg(windows)]
pub(super) fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return true;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut core::ffi::c_void;
        fn GetExitCodeProcess(handle: *mut core::ffi::c_void, code: *mut u32) -> i32;
        fn CloseHandle(handle: *mut core::ffi::c_void) -> i32;
    }
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false; // no such process
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        ok == 0 || code == STILL_ACTIVE
    }
}

#[cfg(not(windows))]
pub(super) fn process_alive(_pid: u32) -> bool {
    true
}

/// The PTY command for a registered profile: its `spawn_command` when it has one, otherwise the
/// code launch path (`AgentType` + `resolve_command`), which only the built-in ids have.
pub(super) fn launch_command(
    profile: &dyn crate::tui::TuiProfile,
    request: &crate::tui::SpawnRequest,
    env: &crate::tui::LaunchEnv,
) -> Result<CommandBuilder, String> {
    if let Some(spec) = profile.spawn_command(request) {
        return Ok(build_launch_spec_command(&spec, env));
    }
    let agent = AgentType::parse(profile.id())
        .ok_or_else(|| format!("agent_type {} has no launch command", profile.id()))?;
    Ok(build_command(agent, request.cwd, request.extra_args, env))
}

/// A child process running on a fresh PTY.
pub(super) struct PtyChild {
    pub master: Box<dyn MasterPty + Send>,
    pub child: Box<dyn Child + Send + Sync>,
    pub pid: u32,
}

/// Open a PTY of the given size and start `cmd` on it.
pub(super) fn open_pty_child(cmd: CommandBuilder, rows: u16, cols: u16) -> Result<PtyChild, String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty: {e}"))?;
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn_command: {e}"))?;
    let pid = child.process_id().unwrap_or(0);
    // Slave handle is now owned by the child; drop our copy so the master
    // gets EOF when the child exits.
    drop(pair.slave);
    Ok(PtyChild {
        master: pair.master,
        child,
        pid,
    })
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

    let profile = crate::tui::require_registered(&args.agent_type).map_err(|unknown| {
        crate::operations::OperationError::unknown_agent_type(unknown.id, unknown.valid).message
    })?;

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
        // Not `take`: a replaced id keeps its hidden mark (a standby is spawned under a pre-hidden id).
        reg.panes.remove(&pane_id)
    };
    if let Some(pane) = replaced {
        shutdown_pane(pane);
        crate::event_log::append_system_event(SystemEvent::PaneKilled {
            pane_id: PaneId(pane_id.clone()),
            reason: Some("respawn_replaced".into()),
        });
    }

    // The account (if any) is resolved before anything spawns: an unknown or foreign account fails
    // the spawn instead of silently running under the CLI's own login.
    let (account_env, account_label) = match args.account.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
        Some(id) => {
            let (env, label) = crate::accounts::launch_for(&args.agent_type, id)?;
            (env, Some(label))
        }
        None => (crate::tui::LaunchEnv::default(), None),
    };
    let cmd = launch_command(
        profile.as_ref(),
        &crate::tui::SpawnRequest {
            cwd: &cwd,
            extra_args: args.extra_args.as_deref().unwrap_or(&[]),
            cols,
            rows,
            read_only: args.read_only,
        },
        &account_env,
    )?;
    let PtyChild { master, child, pid } = open_pty_child(cmd, rows, cols)?;

    let writer = master
        .take_writer()
        .map_err(|e| format!("take_writer: {e}"))?;
    let reader = master
        .try_clone_reader()
        .map_err(|e| format!("clone_reader: {e}"))?;

    let scrollback = Arc::new(Mutex::new(Scrollback::new(SCROLLBACK_CAP)));
    // Mirror the live screen at the PTY's dimensions with a matching scrollback
    // depth so reattach can reconstruct a coherent grid.
    let screen = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, SCROLLBACK_CAP)));
    let stream_pos = crate::pty::stream_pos::start(&pane_id);
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
        viewer_held: false,
        reprints_on_resize: false,
        account: args.account.as_deref().map(str::trim).filter(|a| !a.is_empty()).map(str::to_string),
        account_label,
    };

    let pane = PaneState {
        info,
        scrollback: scrollback.clone(),
        screen: screen.clone(),
        status: status.clone(),
        last_output: last_output.clone(),
        master: Arc::new(Mutex::new(master)),
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
        let stream_pos = stream_pos.clone();
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
                let mut stream_total: u64 = 0;
                let mut prompt_track = crate::pane_prompt::PromptTrack::default();
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
                                    offset: Some(stream_total),
                                },
                            );
                            stream_total += n as u64;
                            stream_pos
                                .emitted
                                .store(stream_total, std::sync::atomic::Ordering::SeqCst);
                            let snapshot = {
                                let mut parser = screen.lock();
                                parser.process(&buf[..n]);
                                // Under the emulator lock: a screen replay reads both together.
                                stream_pos
                                    .processed
                                    .store(stream_total, std::sync::atomic::Ordering::SeqCst);
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

                            super::recorder_tap(&pane_id, *status.lock(), &snapshot);
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

                            // Structured prompt for the desktop UI: on entering or leaving
                            // waiting_input and whenever the detected prompt changes.
                            let waiting = *status.lock() == PaneStatus::WaitingInput;
                            if let Some(prompt) = crate::pane_prompt::next_prompt_event(&mut prompt_track, waiting, || {
                                crate::pane_prompt::detect_in_screen(&agent_type, &last_snapshot, false)
                                    .and_then(|p| crate::pane_prompt::PanePrompt::from_detected(&p))
                            }) {
                                let _ = app.emit(
                                    "pty://prompt",
                                    serde_json::json!({ "pane_id": pane_id, "prompt": prompt }),
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
                finish_exit(&pane_id, &app, &status, &exited);
            })
            .map_err(|e| format!("spawn reader thread: {e}"))?;
    }

    // Exit watcher: the reader only sees EOF when ConPTY closes, which on Windows can lag the
    // child's exit indefinitely. Poll the process so a dead pane is reported (and its pop-out
    // window closed) right away.
    if pid != 0 {
        let pane_id = pane_id.clone();
        let app = app.clone();
        let status = status.clone();
        let exited = exited.clone();
        let _ = thread::Builder::new()
            .name(format!("pty-exit-watch-{pane_id}"))
            .spawn(move || loop {
                thread::sleep(Duration::from_millis(250));
                if *exited.lock() {
                    break;
                }
                if !process_alive(pid) {
                    // Let the reader drain the child's last output before declaring it gone.
                    thread::sleep(Duration::from_millis(400));
                    finish_exit(&pane_id, &app, &status, &exited);
                    break;
                }
            });
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
    /// Where this chunk starts in the pane's byte stream (see `pty::stream_pos`).
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<u64>,
}

#[cfg(all(test, windows))]
mod exit_watch_tests {
    use super::process_alive;

    #[test]
    fn process_alive_tells_running_from_gone() {
        assert!(process_alive(std::process::id()));
        assert!(process_alive(0), "unknown pid is never declared dead");
        let mut child = std::process::Command::new("cmd").args(["/C", "exit 0"]).spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert!(!process_alive(pid));
    }
}
