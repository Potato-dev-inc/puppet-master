//! Push OpenCode worker changes over bridge SSE (permissions, serve health).

use crate::opencode::quota::{self, scrollback_indicates_rate_limit};
use crate::opencode::status::{self, OpenCodeWorkerStatus};
use crate::pty::registry::{read_buffer, PaneRegistry};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const POLL_MS: u64 = 2_000;
const SCROLLBACK_LINES: usize = 200;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OpenCodeWorkerEvent {
    pub pane_id: String,
    pub event: String,
    pub pane_status: String,
    pub serve_healthy: bool,
    pub pending_permission_count: usize,
    pub pending_permission_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_rotated: Option<bool>,
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

fn worker_event(
    pane_id: String,
    event: String,
    pane_status: String,
    serve_healthy: bool,
    pending_permission_count: usize,
    pending_permission_ids: Vec<String>,
) -> OpenCodeWorkerEvent {
    OpenCodeWorkerEvent {
        pane_id,
        event,
        pane_status,
        serve_healthy,
        pending_permission_count,
        pending_permission_ids,
        from_profile: None,
        to_profile: None,
        reason: None,
        auto_rotated: None,
    }
}

pub fn emit_opencode_worker_event(app: &AppHandle, event: OpenCodeWorkerEvent) {
    emit_event(app, event);
}

pub fn ensure_watching(pane_id: String, app: AppHandle, registry: Arc<Mutex<PaneRegistry>>) {
    let mut guard = watching().lock();
    if !guard.insert(pane_id.clone()) {
        return;
    }
    drop(guard);

    thread::spawn(move || {
        let mut last: Option<LastSnapshot> = None;
        let mut quota_scrollback_notified = false;
        loop {
            thread::sleep(Duration::from_millis(POLL_MS));
            let still_there = registry.lock().panes.contains_key(&pane_id);
            if !still_there {
                emit_event(
                    &app,
                    worker_event(
                        pane_id.clone(),
                        "gone".into(),
                        "error".into(),
                        false,
                        0,
                        vec![],
                    ),
                );
                break;
            }

            let Ok(current) = status::worker_status(&registry, &pane_id) else {
                continue;
            };
            let changed = last
                .as_ref()
                .is_none_or(|prev| snapshot_changed(prev, &current));
            if changed {
                let event = classify_event(last.as_ref(), &current);
                emit_event(
                    &app,
                    worker_event(
                        pane_id.clone(),
                        event,
                        current.pane_status.clone(),
                        current.serve_healthy,
                        current.pending_permission_count,
                        current.pending_permission_ids.clone(),
                    ),
                );
                last = Some(LastSnapshot {
                    pane_status: current.pane_status,
                    serve_healthy: current.serve_healthy,
                    pending_permission_count: current.pending_permission_count,
                });
            }

            poll_scrollback_rate_limit(&registry, &app, &pane_id, &mut quota_scrollback_notified);
        }
        watching().lock().remove(&pane_id);
    });
}

fn poll_scrollback_rate_limit(
    registry: &Arc<Mutex<PaneRegistry>>,
    app: &AppHandle,
    pane_id: &str,
    quota_scrollback_notified: &mut bool,
) {
    let Ok(text) = read_buffer(registry, pane_id, SCROLLBACK_LINES) else {
        return;
    };
    if scrollback_indicates_rate_limit(&text) {
        let already_pending = quota::pending_key_event(registry, pane_id).is_some();
        if !*quota_scrollback_notified && !already_pending {
            if let Err(err) = quota::handle_rate_limit(registry, app, pane_id) {
                tracing::warn!(%pane_id, %err, "scrollback rate-limit handler failed");
            }
            *quota_scrollback_notified = true;
        }
    } else {
        *quota_scrollback_notified = false;
    }
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
    if current.pending_permission_count > 0 && prev.is_none_or(|p| p.pending_permission_count == 0)
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
