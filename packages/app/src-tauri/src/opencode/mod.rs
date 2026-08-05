pub mod client;
pub mod keys;
pub mod link;
pub mod native;
pub mod status;
pub mod watch;

pub use client::OpenCodePermission;
pub use link::OpenCodeLink;

use parking_lot::Mutex;

#[derive(Debug, Clone, serde::Serialize)]
pub struct OpenCodeLinkSnapshot {
    pub base_url: String,
    pub session_id: String,
    pub healthy: bool,
}

pub fn pane_link(
    registry: &Mutex<crate::pty::PaneRegistry>,
    pane_id: &str,
) -> Option<OpenCodeLinkSnapshot> {
    let reg = registry.lock();
    let pane = reg.panes.get(pane_id)?;
    let link = pane.opencode.as_ref()?;
    Some(OpenCodeLinkSnapshot {
        base_url: link.base_url.clone(),
        session_id: link.session_id.clone(),
        healthy: client::health_ok(&link.base_url),
    })
}

pub fn write_native_input(
    registry: &Mutex<crate::pty::PaneRegistry>,
    pane_id: &str,
    text: &str,
) -> Result<(), String> {
    let reg = registry.lock();
    let pane = reg
        .panes
        .get(pane_id)
        .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
    let link = pane
        .opencode
        .as_ref()
        .ok_or_else(|| format!("pane {pane_id} is not an opencode native worker"))?;
    if text.trim().is_empty() {
        return Ok(());
    }
    client::prompt_async(&link.base_url, &link.session_id, text)
}

pub fn kill_serve_if_present(pane: &mut crate::pty::registry::PaneState) {
    if let Some(link) = pane.opencode.take() {
        link.kill_serve();
    }
}

fn permission_matches_session(permission: &OpenCodePermission, session_id: &str) -> bool {
    permission.session_id.as_deref() == Some(session_id)
        || permission.session_id_camel.as_deref() == Some(session_id)
}

pub fn list_pane_permissions(
    registry: &Mutex<crate::pty::PaneRegistry>,
    pane_id: &str,
) -> Result<Vec<OpenCodePermission>, String> {
    let reg = registry.lock();
    let pane = reg
        .panes
        .get(pane_id)
        .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
    let link = pane
        .opencode
        .as_ref()
        .ok_or_else(|| format!("pane {pane_id} is not an opencode native worker"))?;
    let permissions = client::list_permissions(&link.base_url)?;
    Ok(permissions
        .into_iter()
        .filter(|permission| permission_matches_session(permission, &link.session_id))
        .collect())
}

pub fn reply_pane_permission(
    registry: &Mutex<crate::pty::PaneRegistry>,
    pane_id: &str,
    request_id: &str,
    reply: &str,
) -> Result<(), String> {
    let reg = registry.lock();
    let pane = reg
        .panes
        .get(pane_id)
        .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
    let link = pane
        .opencode
        .as_ref()
        .ok_or_else(|| format!("pane {pane_id} is not an opencode native worker"))?;
    client::reply_permission(&link.base_url, request_id, reply)
}
