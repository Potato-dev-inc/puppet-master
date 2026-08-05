//! Push OpenCode worker changes over bridge SSE (permissions, serve health).

use crate::opencode::status::{self, OpenCodeWorkerStatus};
use crate::pty::registry::PaneRegistry;
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const POLL_MS: u64 = 2_000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OpenCodeWorkerEvent {
    pub pane_id: String,
    pub event: String,
    pub pane_status: String,
    pub serve_healthy: bool,
    pub pending_permission_count: usize,
    pub pending_permission_ids: Vec<String>,
}

struct LastSnapshot {
    pane_status: String,
    serve_healthy: bool,
    pending_permission_count: usize,
}

static WATCHING: std::sync::OnceLock<Mutex<HashSet<String>>> = std::sync::OnceLock::new();

fn watching() -> &'static Mutex<HashSet<String>> {
    WATCHING.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn ensure_watching(pane_id: String, app: AppHandle, registry: Arc<Mutex<PaneRegistry>>) {
    let mut guard = watching().lock();
    if !guard.insert(pane_id.clone()) {
        return;
    }
    drop(guard);

    thread::spawn(move || {
        let mut last: Option<LastSnapshot> = None;
        loop {
            thread::sleep(Duration::from_millis(POLL_MS));
            let still_there = registry.lock().panes.contains_key(&pane_id);
            if !still_there {
                emit_event(
                    &app,
                    OpenCodeWorkerEvent {
                        pane_id: pane_id.clone(),
                        event: "gone".into(),
                        pane_status: "error".into(),
                        serve_healthy: false,
                        pending_permission_count: 0,
                        pending_permission_ids: vec![],
                    },
                );
                break;
            }

            let Ok(current) = status::worker_status(&registry, &pane_id) else {
                continue;
            };
            let changed = last.as_ref().is_none_or(|prev| snapshot_changed(prev, &current));
            if changed {
                let event = classify_event(last.as_ref(), &current);
                emit_event(
                    &app,
                    OpenCodeWorkerEvent {
                        pane_id: pane_id.clone(),
                        event,
                        pane_status: current.pane_status.clone(),
                        serve_healthy: current.serve_healthy,
                        pending_permission_count: current.pending_permission_count,
                        pending_permission_ids: current.pending_permission_ids.clone(),
                    },
                );
                last = Some(LastSnapshot {
                    pane_status: current.pane_status,
                    serve_healthy: current.serve_healthy,
                    pending_permission_count: current.pending_permission_count,
                });
            }
        }
        watching().lock().remove(&pane_id);
    });
}

fn snapshot_changed(prev: &LastSnapshot, current: &OpenCodeWorkerStatus) -> bool {
    prev.pane_status != current.pane_status
        || prev.serve_healthy != current.serve_healthy
        || prev.pending_permission_count != current.pending_permission_count
}

fn classify_event(prev: Option<&LastSnapshot>, current: &OpenCodeWorkerStatus) -> String {
    if !current.serve_healthy {
        return "unhealthy".into();
    }
    if current.pending_permission_count > 0
        && prev.is_none_or(|p| p.pending_permission_count == 0)
    {
        return "permission".into();
    }
    if prev.is_none_or(|p| p.pane_status != current.pane_status) {
        return "status_changed".into();
    }
    "updated".into()
}

fn emit_event(app: &AppHandle, event: OpenCodeWorkerEvent) {
    let _ = app.emit("opencode://worker", &event);
    if let Ok(json) = serde_json::to_string(&event) {
        crate::bridge::push_sse(format!("event: opencode-worker\ndata: {json}\n\n"));
    }
}
