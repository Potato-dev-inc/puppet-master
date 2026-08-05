//! Compact OpenCode native worker status for MCP (low token footprint).

use crate::opencode::client;
use crate::pty::registry::{PaneInfo, PaneRegistry};
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OpenCodeWorkerStatus {
    pub pane_id: String,
    pub pane_status: String,
    pub serve_healthy: bool,
    pub session_id: String,
    pub pending_permission_count: usize,
    pub pending_permission_ids: Vec<String>,
    pub active_key_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OpenCodeWaitSnapshot {
    pub serve_healthy: bool,
    pub pending_permission_count: usize,
    pub pending_permission_ids: Vec<String>,
}

pub fn worker_status(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
) -> Result<OpenCodeWorkerStatus, String> {
    let (pane, base_url, session_id) = {
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
            .ok_or_else(|| format!("pane {pane_id} has no opencode native session"))?;
        (
            pane.info.clone(),
            link.base_url.clone(),
            link.session_id.clone(),
        )
    };

    let pane_status = pane.status.clone();

    let permissions = client::list_permissions(&base_url)?;
    let session_permissions: Vec<_> = permissions
        .into_iter()
        .filter(|permission| {
            permission.session_id.as_deref() == Some(session_id.as_str())
                || permission.session_id_camel.as_deref() == Some(session_id.as_str())
        })
        .collect();
    let pending_permission_ids: Vec<String> = session_permissions
        .iter()
        .map(|permission| permission.id.clone())
        .collect();

    let active_key_profile = crate::opencode::keys::status()
        .ok()
        .map(|status| status.active_profile);

    Ok(OpenCodeWorkerStatus {
        pane_id: pane_id.to_string(),
        pane_status,
        serve_healthy: client::health_ok(&base_url),
        session_id,
        pending_permission_count: pending_permission_ids.len(),
        pending_permission_ids,
        active_key_profile,
    })
}

pub fn wait_snapshot(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
) -> Result<Option<OpenCodeWaitSnapshot>, String> {
    match worker_status(registry, pane_id) {
        Ok(status) => Ok(Some(OpenCodeWaitSnapshot {
            serve_healthy: status.serve_healthy,
            pending_permission_count: status.pending_permission_count,
            pending_permission_ids: status.pending_permission_ids,
        })),
        Err(err) if err.contains("unknown pane") || err.contains("not opencode_native") => {
            Ok(None)
        }
        Err(err) => Err(err),
    }
}

pub fn pane_info(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> Option<PaneInfo> {
    registry
        .lock()
        .panes
        .get(pane_id)
        .map(|pane| pane.info.clone())
}
