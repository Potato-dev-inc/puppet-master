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
    // Hook Lane: the pane's hook token stops working the moment the pane is gone.
    crate::agent_hooks::revoke(pane_id);
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
    crate::proc_util::pid_alive(pid)
}

#[cfg(not(windows))]
pub(super) fn process_alive(_pid: u32) -> bool {
    true
}

/// The pane's launch env, lowest to highest: the account's, the hook lane's (token), the pane id,
/// then the env guard (when the run has one), which wins over all of them. The hook token and the
/// pane id are the app's own variables for this pane, so a guard never strips them.
pub(super) fn compose_pane_env(
    account: crate::tui::LaunchEnv,
    hook_env: Vec<(String, String)>,
    pane_id: &str,
    guard: Option<crate::tui::LaunchEnv>,
) -> crate::tui::LaunchEnv {
    let mut env = account;
    let mut own: Vec<String> = hook_env.iter().map(|(name, _)| name.clone()).collect();
    env.set.extend(hook_env);
    env.set.push((crate::pty::PANE_ID_ENV.to_string(), pane_id.to_string()));
    own.push(crate::pty::PANE_ID_ENV.to_string());
    match guard {
        Some(guard) => crate::pty::env_guard::layer(env, guard, &own),
        None => env,
    }
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
        // A native pane is not a launched process this spawn can put an env on: a guarded run is
        // refused instead of running unguarded.
        if let Some(guard) = args.guard_env.as_deref() {
            return Err(format!("env guard {guard:?} cannot be applied to a native pane; the launch is refused"));
        }
        return crate::opencode::native::spawn_native_pane(Arc::clone(registry), app, args);
    }

    let profile = crate::tui::require_registered(&args.agent_type).map_err(|unknown| {
        crate::operations::OperationError::unknown_agent_type(unknown.id, unknown.valid).message
    })?;

    // The guard is resolved before anything starts or is replaced: an unknown name refuses the spawn.
    let guard_env = match args.guard_env.as_deref() {
        Some(guard) => Some(crate::pty::guard_launch::launch_env_for(guard)?),
        None => None,
    };

    let cwd = crate::project_path::resolve_spawn_cwd(
        args.cwd.clone(),
        registry.lock().project_path.clone(),
    )?
    .to_string_lossy()
    .to_string();

    // One brain: the project's shared memory is in every agent's instruction file before it starts.
    crate::one_brain::sync_on_spawn(&cwd);

    let cols = args.cols.unwrap_or(120);
    let rows = args.rows.unwrap_or(30);
    let pane_id = args.pane_id.unwrap_or_else(|| Uuid::new_v4().to_string());

    // Linux Inside: where this pane runs. `None` = Windows, and then nothing below differs from a
    // build without the feature. A refusal fails the spawn before any pane is replaced.
    let linux = linux::plan(
        profile.as_ref(),
        &pane_id,
        args.account.as_deref().map(str::trim).filter(|a| !a.is_empty()),
        args.guard_env.as_deref(),
        &cwd,
    )?;

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
    // Hook Lane (shadow): a per-pane token in env and, for a launch-flag agent, its flag. Empty when
    // the agent has no hooks or they are off, so such a pane starts exactly as before.
    let user_args = args.extra_args.clone().unwrap_or_default();
    // A Linux pane has no hooks in v1: nothing is installed or recorded for it.
    let hooks = if linux.is_some() {
        crate::agent_hooks::HookLaunch::default()
    } else {
        crate::agent_hooks::prepare_spawn(profile.id(), &pane_id, &cwd, &user_args)
    };
    // Puppet Master's own helper tools (`run_agent`, `wait_agents`) for this one launch, where the agent
    // can take a server that way. Nothing is added for a Linux pane (the app's program is a Windows
    // one) or when the Settings switch is off. The person's own config files are never written.
    let pane_mcp = if linux.is_some() {
        crate::tui::pane_mcp::PaneMcpLaunch::default()
    } else {
        crate::pane_mcp::prepare(profile.as_ref(), &pane_id, &user_args)
    };
    // Names the pane to any MCP shim the agent starts, so a helper it starts can be nested under it.
    let mut own_env = hooks.env;
    own_env.extend(pane_mcp.env);
    let account_env = compose_pane_env(account_env, own_env, &pane_id, guard_env);
    // An agent that serves its session over a local API is told where (the chat view reads it from there).
    let api_args = profile.prepare_session_api(&pane_id, &user_args);
    // An agent that can be told its session id gets one now, so the chat view finds this pane's own file.
    let (session_id, session_args) = match profile.launch_session(&user_args, &Uuid::new_v4().to_string()) {
        crate::tui::LaunchSession::Chosen { id, args } => (Some(id), args),
        crate::tui::LaunchSession::Named(id) => (Some(id), Vec::new()),
        crate::tui::LaunchSession::Unmanaged => (None, Vec::new()),
    };
    let extra_args: Vec<String> =
        user_args.into_iter().chain(hooks.extra_args).chain(api_args).chain(session_args).chain(pane_mcp.args).collect();
    let cmd = linux::pane_command(
        profile.as_ref(),
        &crate::tui::SpawnRequest {
            cwd: &cwd,
            extra_args: &extra_args,
            cols,
            rows,
            read_only: args.read_only,
        },
        &account_env,
        linux.as_ref(),
    )?;
    let PtyChild { master, child, pid } = open_pty_child(cmd, rows, cols)?;
    crate::pty::launch_session::set(&pane_id, session_id);

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
    let flight = crate::flight::start(&pane_id, cols, rows);
    let status = Arc::new(Mutex::new(PaneStatus::Running));
    let last_output = Arc::new(Mutex::new(Instant::now()));
    let idle_clock = Arc::new(idle::IdleClock::new(Instant::now()));
    let idle_profile = crate::tui::profile_for(&args.agent_type);
    let (idle_after, quiet_idle_after) = (idle_profile.idle_after(), idle_profile.quiet_idle_after());
    // 0 = unknown, 1 = the screen shows work, 2 = it provably does not (see `TuiProfile::busy_marker`).
    let busy_state = Arc::new(std::sync::atomic::AtomicU8::new(0));
    // The screen shows a startup gate (an update notice, a trust question): the pane is waiting, never idle.
    let gate_up = Arc::new(std::sync::atomic::AtomicBool::new(false));
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
        title: None,
        parent_pane_id: None,
        place: linux.as_ref().map(crate::wsl::place::pane_place),
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
        let flight = flight.clone();
        let status = status.clone();
        let last_output = last_output.clone();
        let idle_clock = idle_clock.clone();
        let busy_state = busy_state.clone();
        let gate_up = gate_up.clone();
        let exited = exited.clone();
        let folder = cwd.clone();
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
                            // Dev-only (fake-agent e2e): a slow ConPTY on demand.
                            #[cfg(feature = "dev-fake-tui")]
                            crate::fake_tui::hold_output_chunk(&buf[..n]);
                            // Flight recorder: one memcpy into the pane's ring (the live stream below is untouched).
                            flight.output(&buf[..n]);
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
                            // Hyperlinks (OSC 8) the emulator does not keep: what a shortened path pointed at.
                            crate::pty::pane_facts::feed(&pane_id, &buf[..n]);
                            // Emit raw bytes for the xterm.js renderer, while
                            // also feeding the headless emulator used by MCP
                            // buffer reads and reattach snapshots.
                            let chunk = buf[..n].to_vec();
                            let snapshot = stream_pos.feed(&screen, &mut stream_total, &buf[..n], |offset| {
                                let _ = app.emit(
                                    "terminal-data",
                                    TerminalDataEvent {
                                        pane_id: pane_id.clone(),
                                        data: chunk,
                                        offset: Some(offset),
                                    },
                                );
                            });
                            *last_output.lock() = Instant::now();

                            // Output that leaves every visible cell alone (cursor and
                            // attribute sequences) is not activity: it neither postpones
                            // idleness nor re-runs the classifier, which would flip an idle
                            // pane back to running on every chunk.
                            if snapshot == last_snapshot {
                                continue;
                            }
                            idle_clock.screen_changed(Instant::now());
                            // The model any look of this pane showed (a banner, a status row while it works) stays
                            // known after the screen moves on.
                            crate::pty::pane_facts::observe_model(&pane_id, &agent_type, &snapshot);
                            let gate = crate::pty::status::startup_gate_visible(&agent_type, &snapshot);
                            gate_up.store(gate, std::sync::atomic::Ordering::Relaxed);
                            // A gate's screen proves nothing about work: an idle pane stays idle only when the screen
                            // is provably quiet, and a menu is neither.
                            let busy = if gate { None } else { crate::pty::status::busy_marker(&agent_type, &snapshot) };
                            busy_state.store(
                                match busy {
                                    Some(true) => 1,
                                    Some(false) => 2,
                                    None => 0,
                                },
                                std::sync::atomic::Ordering::Relaxed,
                            );

                            // Classify the current VT screen. Historical prompt text in
                            // scrollback must not make a later agent response look blocked.
                            let new_status = crate::pty::status::classify_agent_observation_in(
                                &agent_type,
                                &snapshot,
                                "",
                                Some(&folder),
                            );
                            let changed = {
                                let mut s = status.lock();
                                // A redraw of an idle pane that shows no work is not activity.
                                let new_status = crate::pty::status::settle_status_after_resize(
                                    *s,
                                    new_status,
                                    busy,
                                    !gate && crate::pty::resize_clock::recently_resized(&pane_id),
                                );
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
                                flight.mark(&format!("status={}", s.as_str()));
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
                                // The pane is waiting: a menu no profile describes still gets its card by shape.
                                crate::pane_prompt::detect_in_screen_with_status(&agent_type, &last_snapshot, false, Some("waiting_input"))
                                    .and_then(|p| crate::pane_prompt::PanePrompt::from_detected(&p))
                            }) {
                                crate::attention::on_prompt(&pane_id, waiting, prompt.as_ref());
                                // A new occurrence: an answer to it is not a repeat of the last one.
                                crate::pane_prompt::answers::note_prompt_event(&pane_id);
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
        let gate_up = gate_up.clone();
        let status = status.clone();
        let exited = exited.clone();
        thread::Builder::new()
            .name(format!("pty-idle-{pane_id}"))
            .spawn(move || loop {
                thread::sleep(idle::IDLE_POLL);
                if *exited.lock() {
                    break;
                }
                // A startup gate on screen is a pane that waits, not one that rests: it never reads idle.
                if gate_up.load(std::sync::atomic::Ordering::Relaxed) {
                    continue;
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

    if let Some(place) = &linux {
        linux::watch_fast_exit(place.distro.clone(), exited.clone());
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
