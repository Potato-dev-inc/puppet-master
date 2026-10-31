//! Panic button: one action interrupts every running agent pane and reports what it stopped.
//!
//! Entry points: the palette ("Stop all agents", Tauri command `stop_all_agents`), the tray menu
//! item and the optional global shortcut (both go through `stop_all_and_show`, which also opens the
//! cockpit and emits `panic://stopped` so the sheet appears), and `POST /panes/stop-all` for the
//! phone. Interrupt = the same Ctrl+C `interrupt_pane` sends. Only panes whose status is
//! `running` are touched; plain shell panes are left alone (a dev server in a shell is not an
//! agent burning credits), and so are idle, waiting and errored panes.
//!
//! Before a pane is interrupted, a checkpoint of its project is taken (`checkpoints::before_stop`),
//! so "Stop all agents" never leaves the user without a way back. The checkpoints run in parallel
//! under a short time budget: a huge folder must not delay the stop, it just goes without one.

use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::AppHandle;

/// How long the stop waits, in total, for the checkpoints taken before it interrupts.
pub const CHECKPOINT_BUDGET: Duration = Duration::from_secs(4);

/// What the panic button needs to know about one pane.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub pane_id: String,
    pub agent_type: String,
    pub cwd: String,
    pub status: String,
    pub is_shell: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoppedPane {
    pub pane_id: String,
    pub agent_type: String,
    pub cwd: String,
    /// The checkpoint taken just before the interrupt (the way back), when one could be taken.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct StopReport {
    pub stopped: Vec<StoppedPane>,
    /// Panes that were running but could not be interrupted: `(pane_id, reason)`.
    pub failed: Vec<(String, String)>,
}

/// Interrupts every running agent pane through `interrupt` (pure; tests inject the interrupt).
pub fn stop_running(candidates: &[Candidate], interrupt: &dyn Fn(&str) -> Result<(), String>) -> StopReport {
    stop_running_with(candidates, &|_| None, interrupt)
}

/// [`stop_running`] that also records, per pane, the checkpoint `checkpoint_of` names. It is asked
/// BEFORE the pane is interrupted.
pub fn stop_running_with(
    candidates: &[Candidate],
    checkpoint_of: &dyn Fn(&Candidate) -> Option<String>,
    interrupt: &dyn Fn(&str) -> Result<(), String>,
) -> StopReport {
    let mut report = StopReport::default();
    for c in candidates.iter().filter(|c| c.status == "running" && !c.is_shell) {
        let checkpoint_id = checkpoint_of(c);
        match interrupt(&c.pane_id) {
            Ok(()) => report.stopped.push(StoppedPane {
                pane_id: c.pane_id.clone(),
                agent_type: c.agent_type.clone(),
                cwd: c.cwd.clone(),
                checkpoint_id,
            }),
            Err(reason) => report.failed.push((c.pane_id.clone(), reason)),
        }
    }
    report
}

fn candidates(registry: &Arc<Mutex<PaneRegistry>>) -> Vec<Candidate> {
    let panes = registry.lock().list();
    panes
        .into_iter()
        .map(|p| Candidate {
            is_shell: crate::tui::profile_for(&p.agent_type).features().kind == crate::tui::Kind::Shell,
            pane_id: p.id,
            agent_type: p.agent_type,
            cwd: p.cwd,
            status: p.status,
        })
        .collect()
}

/// Checkpoints every running agent pane in `candidates` through `take` (one worker thread per pane,
/// all at once) and waits up to `budget` in total. Panes whose checkpoint is late, absent or failed
/// are simply missing from the result: the stop goes ahead without them.
pub fn checkpoint_running(
    candidates: &[Candidate],
    budget: Duration,
    take: Arc<dyn Fn(&str) -> Option<String> + Send + Sync>,
) -> HashMap<String, String> {
    let (tx, rx) = std::sync::mpsc::channel::<(String, Option<String>)>();
    let mut expected = 0;
    for c in candidates.iter().filter(|c| c.status == "running" && !c.is_shell) {
        let (tx, take, pane_id) = (tx.clone(), take.clone(), c.pane_id.clone());
        let spawned = std::thread::Builder::new().name("pm-panic-checkpoint".into()).spawn(move || {
            let id = take(&pane_id);
            let _ = tx.send((pane_id, id));
        });
        if spawned.is_ok() {
            expected += 1;
        }
    }
    drop(tx);
    let deadline = Instant::now() + budget;
    let mut ids = HashMap::new();
    for _ in 0..expected {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else { break };
        match rx.recv_timeout(left) {
            Ok((pane_id, Some(id))) => {
                ids.insert(pane_id, id);
            }
            Ok((_, None)) => {}
            Err(_) => break,
        }
    }
    ids
}

/// Interrupt every running agent pane of the registry, checkpointing each one's project first.
pub fn stop_all(registry: &Arc<Mutex<PaneRegistry>>, app: &AppHandle) -> StopReport {
    // A Live Room is public exposure: it ends first, before anything else is stopped.
    {
        use tauri::Manager;
        if let Some(room) = app.try_state::<crate::live_room::service::LiveRoomState>() {
            room.stop_all(crate::live_room::publisher::StopReason::Owner);
        }
    }
    // Every mission stops too (its current turn is one of the panes below).
    let _ = std::thread::Builder::new().name("missions-stop-all".into()).spawn(crate::missions::stop_all);
    // A running Night Shift stops too, so it does not start the next task behind the panic.
    crate::night_shift::runner::stop_running_nights();
    // Potato Bot's brains stop too (they cost money per message). A quick ask to leave, then the tree is killed.
    crate::potato::live::stop_all(std::time::Duration::from_millis(300));
    let list = candidates(registry);
    let checkpoints = checkpoint_running(&list, CHECKPOINT_BUDGET, Arc::new(crate::checkpoints::before_stop));
    stop_running_with(
        &list,
        &|c| checkpoints.get(&c.pane_id).cloned(),
        &|id| crate::pty::registry_write_input(registry, app, id, "\u{3}", false, false, None, crate::pty::WriteOrigin::Nudge),
    )
}

/// Shortcut and tray entry: stop everything, bring the cockpit forward and tell it to show the sheet.
pub fn stop_all_and_show(app: &AppHandle) -> StopReport {
    use tauri::{Emitter, Manager};
    let report = match app.try_state::<crate::AppState>() {
        Some(state) => stop_all(&state.registry, app),
        None => StopReport::default(),
    };
    crate::cockpit::open_cockpit(app, None);
    let _ = app.emit("panic://stopped", &report);
    report
}

pub const SHORTCUT: &str = "Ctrl+Alt+Shift+P";

/// Register (or unregister) the global shortcut according to the saved setting. Off by default.
pub fn init_shortcut(app: &AppHandle) {
    apply_shortcut(app, crate::loop_guard::settings().panic_shortcut);
}

pub fn apply_shortcut(app: &AppHandle, enabled: bool) {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister(SHORTCUT);
    if !enabled {
        return;
    }
    let handle = app.clone();
    let result = shortcuts.on_shortcut(SHORTCUT, move |_app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            let handle = handle.clone();
            std::thread::spawn(move || {
                stop_all_and_show(&handle);
            });
        }
    });
    if let Err(e) = result {
        tracing::warn!(error = %e, "panic shortcut could not be registered");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn c(id: &str, status: &str, shell: bool) -> Candidate {
        Candidate { pane_id: id.into(), agent_type: if shell { "bash" } else { "claude" }.into(), cwd: "/p".into(), status: status.into(), is_shell: shell }
    }

    #[test]
    fn interrupts_only_running_agent_panes() {
        let calls = RefCell::new(Vec::<String>::new());
        let panes = [
            c("run-1", "running", false),
            c("idle-1", "idle", false),
            c("wait-1", "waiting_input", false),
            c("err-1", "error", false),
            c("run-2", "running", false),
            c("shell-1", "running", true),
        ];
        let report = stop_running(&panes, &|id| {
            calls.borrow_mut().push(id.to_string());
            Ok(())
        });
        assert_eq!(*calls.borrow(), vec!["run-1", "run-2"]);
        assert_eq!(report.stopped.iter().map(|s| s.pane_id.as_str()).collect::<Vec<_>>(), vec!["run-1", "run-2"]);
        assert!(report.failed.is_empty());
    }

    #[test]
    fn a_failing_interrupt_is_reported_and_does_not_stop_the_rest() {
        let panes = [c("a", "running", false), c("b", "running", false)];
        let report = stop_running(&panes, &|id| if id == "a" { Err("pane has exited".into()) } else { Ok(()) });
        assert_eq!(report.stopped.len(), 1);
        assert_eq!(report.stopped[0].pane_id, "b");
        assert_eq!(report.failed, vec![("a".to_string(), "pane has exited".to_string())]);
    }

    #[test]
    fn a_checkpoint_is_taken_before_each_interrupt_and_its_id_is_reported() {
        let calls = RefCell::new(Vec::<String>::new());
        let panes = [c("run-1", "running", false), c("idle-1", "idle", false), c("run-2", "running", false)];
        let report = stop_running_with(
            &panes,
            &|p| {
                calls.borrow_mut().push(format!("checkpoint {}", p.pane_id));
                (p.pane_id == "run-1").then(|| "7".to_string())
            },
            &|id| {
                calls.borrow_mut().push(format!("interrupt {id}"));
                Ok(())
            },
        );
        assert_eq!(
            *calls.borrow(),
            ["checkpoint run-1", "interrupt run-1", "checkpoint run-2", "interrupt run-2"],
            "each pane is checkpointed first; idle panes are left alone"
        );
        assert_eq!(report.stopped[0].checkpoint_id.as_deref(), Some("7"));
        assert_eq!(report.stopped[1].checkpoint_id, None, "a pane that could not be checkpointed is still stopped");
    }

    #[test]
    fn checkpoints_run_in_parallel_and_a_slow_one_cannot_delay_the_stop() {
        let panes = [c("fast", "running", false), c("slow", "running", false), c("idle", "idle", false), c("sh", "running", true)];
        let started = Instant::now();
        let ids = checkpoint_running(
            &panes,
            Duration::from_millis(300),
            Arc::new(|pane: &str| {
                if pane == "slow" {
                    std::thread::sleep(Duration::from_secs(3));
                }
                Some(format!("ck-{pane}"))
            }),
        );
        assert!(started.elapsed() < Duration::from_secs(2), "waited {:?}", started.elapsed());
        assert_eq!(ids.get("fast").map(String::as_str), Some("ck-fast"));
        assert!(!ids.contains_key("slow"), "the late checkpoint is dropped, the stop goes ahead");
        assert!(!ids.contains_key("idle") && !ids.contains_key("sh"), "only running agent panes are checkpointed");
    }

    #[test]
    fn nothing_running_stops_nothing() {
        let report = stop_running(&[c("a", "idle", false)], &|_| panic!("must not interrupt"));
        assert_eq!(report, StopReport::default());
    }
}
