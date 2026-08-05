//! Compact OpenCode native worker status for MCP (low token footprint).

use crate::opencode::client::{self, OpenCodeModelRef};
use crate::opencode::quota::PendingKeySwapInfo;
use crate::pty::registry::{PaneInfo, PaneRegistry};
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModelSnapshot {
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
}

impl From<OpenCodeModelRef> for ModelSnapshot {
    fn from(model: OpenCodeModelRef) -> Self {
        Self {
            provider_id: model.provider_id,
            model_id: model.model_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FooterModelSource {
    Session,
    LastUser,
    Unknown,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LastEventSummary {
    pub event_type: String,
    pub timestamp_ms: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OpenCodeWorkerStatus {
    pub pane_id: String,
    pub pane_status: String,
    pub serve_healthy: bool,
    pub session_id: String,
    pub pending_permission_count: usize,
    pub pending_permission_ids: Vec<String>,
    pub active_key_profile: Option<String>,
    pub key_swap_pending: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_swap_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_model: Option<ModelSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_user_model: Option<ModelSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer_model_source: Option<FooterModelSource>,
    pub tui_attached: bool,
    pub reattaching: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event: Option<LastEventSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OpenCodeWaitSnapshot {
    pub serve_healthy: bool,
    pub pending_permission_count: usize,
    pub pending_permission_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_key_swap: Option<PendingKeySwapInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_model: Option<ModelSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_user_model: Option<ModelSnapshot>,
    pub tui_attached: bool,
    pub reattaching: bool,
}

struct LinkRuntime {
    base_url: String,
    session_id: String,
    directory: String,
    reattaching: bool,
    tui_attached: bool,
}

fn link_runtime(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> Result<LinkRuntime, String> {
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
    let reattaching = link.reattaching().load(std::sync::atomic::Ordering::SeqCst);
    let exited = *pane.exited.lock();
    Ok(LinkRuntime {
        base_url: link.base_url.clone(),
        session_id: link.session_id.clone(),
        directory: link.directory.clone(),
        reattaching,
        tui_attached: !exited && !reattaching,
    })
}

fn model_snapshots(
    base_url: &str,
    session_id: &str,
    directory: &str,
) -> (Option<ModelSnapshot>, Option<ModelSnapshot>, Option<FooterModelSource>) {
    let session_model = client::session_model_ref(base_url, session_id, Some(directory))
        .ok()
        .flatten()
        .map(ModelSnapshot::from);
    let last_user_model = client::last_user_message_model(base_url, session_id, Some(directory))
        .ok()
        .flatten()
        .map(ModelSnapshot::from);
    let footer_model_source = if last_user_model.is_some() {
        Some(FooterModelSource::LastUser)
    } else if session_model.is_some() {
        Some(FooterModelSource::Session)
    } else {
        Some(FooterModelSource::Unknown)
    };
    (session_model, last_user_model, footer_model_source)
}

pub fn worker_status(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
) -> Result<OpenCodeWorkerStatus, String> {
    let (pane, key_event, runtime) = {
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
        let runtime = LinkRuntime {
            base_url: link.base_url.clone(),
            session_id: link.session_id.clone(),
            directory: link.directory.clone(),
            reattaching: link.reattaching().load(std::sync::atomic::Ordering::SeqCst),
            tui_attached: !*pane.exited.lock()
                && !link.reattaching().load(std::sync::atomic::Ordering::SeqCst),
        };
        (
            pane.info.clone(),
            pane.opencode_key_event.clone(),
            runtime,
        )
    };

    let pane_status = pane.status.clone();
    let permissions = client::list_permissions(&runtime.base_url)?;
    let session_permissions: Vec<_> = permissions
        .into_iter()
        .filter(|permission| {
            permission.session_id.as_deref() == Some(runtime.session_id.as_str())
                || permission.session_id_camel.as_deref() == Some(runtime.session_id.as_str())
        })
        .collect();
    let pending_permission_ids: Vec<String> = session_permissions
        .iter()
        .map(|permission| permission.id.clone())
        .collect();

    let active_key_profile = crate::opencode::keys::status()
        .ok()
        .map(|status| status.active_profile);

    let (session_model, last_user_model, footer_model_source) = model_snapshots(
        &runtime.base_url,
        &runtime.session_id,
        &runtime.directory,
    );

    let last_event = crate::event_log::last_pane_event_summary(pane_id);

    Ok(OpenCodeWorkerStatus {
        pane_id: pane_id.to_string(),
        pane_status,
        serve_healthy: client::health_ok(&runtime.base_url),
        session_id: runtime.session_id,
        pending_permission_count: pending_permission_ids.len(),
        pending_permission_ids,
        active_key_profile,
        key_swap_pending: key_event.is_some(),
        key_swap_kind: key_event.as_ref().map(|event| event.kind.clone()),
        from_profile: key_event.as_ref().map(|event| event.from_profile.clone()),
        to_profile: key_event.as_ref().and_then(|event| event.to_profile.clone()),
        session_model,
        last_user_model,
        footer_model_source,
        tui_attached: runtime.tui_attached,
        reattaching: runtime.reattaching,
        last_event,
    })
}

pub fn wait_snapshot(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
) -> Result<Option<OpenCodeWaitSnapshot>, String> {
    match worker_status(registry, pane_id) {
        Ok(status) => {
            let pending_key_swap = status
                .key_swap_kind
                .as_ref()
                .map(|kind| PendingKeySwapInfo {
                    kind: kind.clone(),
                    from_profile: status.from_profile.clone().unwrap_or_default(),
                    to_profile: status.to_profile.clone(),
                });
            Ok(Some(OpenCodeWaitSnapshot {
                serve_healthy: status.serve_healthy,
                pending_permission_count: status.pending_permission_count,
                pending_permission_ids: status.pending_permission_ids,
                pending_key_swap,
                session_model: status.session_model,
                last_user_model: status.last_user_model,
                tui_attached: status.tui_attached,
                reattaching: status.reattaching,
            }))
        }
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

pub fn is_model_ready(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    provider_id: Option<&str>,
    model_id: Option<&str>,
) -> Result<bool, String> {
    let runtime = link_runtime(registry, pane_id)?;
    let last_user = client::last_user_message_model(
        &runtime.base_url,
        &runtime.session_id,
        Some(&runtime.directory),
    )?;
    let Some(last_user) = last_user else {
        return Ok(false);
    };
    Ok(client::model_matches_filter(&last_user, provider_id, model_id))
}

pub fn is_tui_ready(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> Result<bool, String> {
    let runtime = link_runtime(registry, pane_id)?;
    Ok(runtime.tui_attached && !runtime.reattaching && client::health_ok(&runtime.base_url))
}

pub fn worker_status_json(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
) -> Result<serde_json::Value, String> {
    let status = worker_status(registry, pane_id)?;
    serde_json::to_value(status).map_err(|err| format!("serialize worker status: {err}"))
}
