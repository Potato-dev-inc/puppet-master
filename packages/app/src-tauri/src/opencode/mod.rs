pub mod client;
pub mod keys;
pub mod link;
pub mod messages;
pub mod native;
pub mod quota;
pub mod status;
pub mod watch;

pub use client::{OpenCodeModelRef, OpenCodePermission};
pub use link::OpenCodeLink;

use serde_json::Value;

use parking_lot::Mutex;
use std::sync::Arc;
use tauri::AppHandle;

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

pub fn resolve_model(
    request_provider: Option<&str>,
    request_model_id: Option<&str>,
    settings: &Value,
) -> Option<OpenCodeModelRef> {
    if let Some((provider_id, model_id)) = parse_request_model(request_provider, request_model_id) {
        return Some(OpenCodeModelRef {
            provider_id,
            model_id,
        });
    }
    let model_id = settings
        .get("opencode_model_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let provider = settings
        .get("opencode_model_provider")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_OPENCODE_PROVIDER);
    Some(OpenCodeModelRef {
        provider_id: provider.to_string(),
        model_id: model_id.to_string(),
    })
}

/// Prefer `opencode-go` when the caller only passes a bare model id (e.g. `glm-5.2`).
const DEFAULT_OPENCODE_PROVIDER: &str = "opencode-go";

fn parse_request_model(
    request_provider: Option<&str>,
    request_model_id: Option<&str>,
) -> Option<(String, String)> {
    let provider = request_provider
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let model_id = request_model_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let (Some(provider), Some(model_id)) = (provider, model_id) {
        return Some((provider.to_string(), model_id.to_string()));
    }
    let combined = model_id.or(provider)?;
    if let Some((provider, model_id)) = split_provider_model_id(combined) {
        return Some((provider, model_id));
    }
    // Bare model id → OpenCode Go (agents can still pass model_provider explicitly).
    Some((DEFAULT_OPENCODE_PROVIDER.to_string(), combined.to_string()))
}

fn split_provider_model_id(combined: &str) -> Option<(String, String)> {
    let (provider, model_id) = combined.split_once('/')?;
    if provider.is_empty() || model_id.is_empty() {
        return None;
    }
    Some((provider.to_string(), model_id.to_string()))
}

/// Switch the OpenCode session model and reattach the TUI (no scratch sessions).
pub fn switch_native_model(
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &AppHandle,
    pane_id: &str,
    model: &OpenCodeModelRef,
) -> Result<(), String> {
    set_pending_model(registry, pane_id, model.clone());
    let (base_url, session_id, directory) = {
        let reg = registry.lock();
        let pane = reg
            .panes
            .get(pane_id)
            .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
        let link = pane
            .opencode
            .as_ref()
            .ok_or_else(|| format!("pane {pane_id} is not an opencode native worker"))?;
        (
            link.base_url.clone(),
            link.session_id.clone(),
            link.directory.clone(),
        )
    };
    client::switch_session_model(&base_url, &session_id, Some(&directory), model)?;
    // attach ignores -m; footer syncs from last user message model on load
    client::stamp_last_user_model(&base_url, &session_id, Some(&directory), model)?;
    crate::event_log::append_system_event(crate::events::SystemEvent::PaneModelSwitched {
        pane_id: crate::events::PaneId(pane_id.to_string()),
        provider_id: model.provider_id.clone(),
        model_id: model.model_id.clone(),
    });
    native::reattach_tui(Arc::clone(registry), app, pane_id)?;
    crate::pane_wait_notify::bump_waiters();
    Ok(())
}

pub fn set_pending_model(
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    pane_id: &str,
    model: OpenCodeModelRef,
) {
    let mut reg = registry.lock();
    if let Some(pane) = reg.panes.get_mut(pane_id) {
        if let Some(link) = pane.opencode.as_ref() {
            *link.pending_model().lock() = Some(model);
        }
    }
}

pub fn switch_model_response(
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    pane_id: &str,
    model: &OpenCodeModelRef,
) -> serde_json::Value {
    let snapshot = status::worker_status_json(registry, pane_id)
        .unwrap_or_else(|err| serde_json::json!({ "error": err }));
    crate::mcp_hints::mutate_ok(
        snapshot,
        crate::mcp_hints::suggested_wait_after_model_switch(pane_id, model),
    )
}

pub fn write_native_input(
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &AppHandle,
    pane_id: &str,
    text: &str,
    model: Option<&OpenCodeModelRef>,
) -> Result<(), String> {
    let (base_url, session_id, directory) = {
        let reg = registry.lock();
        let pane = reg
            .panes
            .get(pane_id)
            .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
        let link = pane
            .opencode
            .as_ref()
            .ok_or_else(|| format!("pane {pane_id} is not an opencode native worker"))?;
        (
            link.base_url.clone(),
            link.session_id.clone(),
            link.directory.clone(),
        )
    };
    if let Some(model) = model {
        client::switch_session_model(&base_url, &session_id, Some(&directory), model)?;
    }
    if text.trim().is_empty() {
        if let Some(model) = model {
            client::stamp_last_user_model(&base_url, &session_id, Some(&directory), model)?;
            spawn_reattach_tui(Arc::clone(registry), app.clone(), pane_id.to_string());
        }
        return Ok(());
    }
    match client::prompt_async(&base_url, &session_id, Some(&directory), text, model) {
        Ok(()) => {
            spawn_reattach_tui(Arc::clone(registry), app.clone(), pane_id.to_string());
            Ok(())
        }
        Err(err) if client::is_rate_limit_status(err.status, &err.body) => {
            quota::handle_rate_limit(registry, app, pane_id)?;
            let (base_url, current_session, directory) = {
                let reg = registry.lock();
                let pane = reg
                    .panes
                    .get(pane_id)
                    .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
                let link = pane.opencode.as_ref().ok_or_else(|| {
                    format!("pane {pane_id} is not an opencode native worker")
                })?;
                (
                    link.base_url.clone(),
                    link.session_id.clone(),
                    link.directory.clone(),
                )
            };
            if current_session != session_id && pane_rebound_to(registry, pane_id, &current_session)
            {
                spawn_reattach_tui(Arc::clone(registry), app.clone(), pane_id.to_string());
                return Ok(());
            }
            client::prompt_async(&base_url, &current_session, Some(&directory), text, model)
                .map_err(|retry| retry.into_message())
                .map(|()| {
                    spawn_reattach_tui(Arc::clone(registry), app.clone(), pane_id.to_string());
                })
        }
        Err(err) => Err(err.into_message()),
    }
}

/// Prompt the live native session without key-rotation / pane restart.
pub fn prompt_native_session(
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &AppHandle,
    pane_id: &str,
    text: &str,
) -> Result<(), String> {
    let (base_url, session_id, directory) = {
        let reg = registry.lock();
        let pane = reg
            .panes
            .get(pane_id)
            .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
        let link = pane
            .opencode
            .as_ref()
            .ok_or_else(|| format!("pane {pane_id} is not an opencode native worker"))?;
        (
            link.base_url.clone(),
            link.session_id.clone(),
            link.directory.clone(),
        )
    };
    client::prompt_async(&base_url, &session_id, Some(&directory), text, None)
        .map_err(|error| error.into_message())?;
    spawn_reattach_tui(Arc::clone(registry), app.clone(), pane_id.to_string());
    Ok(())
}

fn pane_rebound_to(
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    pane_id: &str,
    session_id: &str,
) -> bool {
    let project = registry.lock().project_path.clone();
    crate::operations::list_indexed_operations(&project)
        .ok()
        .into_iter()
        .flatten()
        .any(|snapshot| {
            snapshot.pane_id.as_deref() == Some(pane_id)
                && !snapshot.worker.closed
                && snapshot.worker.provider_session_id.as_deref() == Some(session_id)
        })
}

fn spawn_reattach_tui(
    registry: Arc<Mutex<crate::pty::PaneRegistry>>,
    app: AppHandle,
    pane_id: String,
) {
    std::thread::spawn(move || {
        if let Err(err) = native::reattach_tui(registry, &app, &pane_id) {
            tracing::warn!(%pane_id, %err, "opencode tui reattach failed");
        }
    });
}

pub fn refresh_native_tui(
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &AppHandle,
    pane_id: &str,
) {
    spawn_reattach_tui(Arc::clone(registry), app.clone(), pane_id.to_string());
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

pub fn reply_pane_question(
    registry: &Mutex<crate::pty::PaneRegistry>,
    pane_id: &str,
    answer: &str,
    request_id: Option<&str>,
) -> Result<serde_json::Value, String> {
    let (base_url, session_id, directory) = {
        let reg = registry.lock();
        let pane = reg
            .panes
            .get(pane_id)
            .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
        let link = pane
            .opencode
            .as_ref()
            .ok_or_else(|| format!("pane {pane_id} is not an opencode native worker"))?;
        (
            link.base_url.clone(),
            link.session_id.clone(),
            link.directory.clone(),
        )
    };
    let request_id = match request_id.map(str::trim).filter(|value| !value.is_empty()) {
        Some(id) => id.to_string(),
        None => client::list_questions(&base_url, Some(&directory))?
            .into_iter()
            .find(|item| item.session_id == session_id)
            .map(|item| item.id)
            .ok_or_else(|| "no pending question for this pane".to_string())?,
    };
    let label = answer.trim();
    if label.is_empty() {
        return Err("answer is required (option label, e.g. Yes or No)".into());
    }
    client::reply_question(
        &base_url,
        &request_id,
        Some(&directory),
        &[label.to_string()],
    )?;
    crate::pane_wait_notify::bump_waiters();
    Ok(serde_json::json!({
        "ok": true,
        "pane_id": pane_id,
        "request_id": request_id,
        "answer": label,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolve_model_accepts_provider_slash_model_id() {
        let model = resolve_model(None, Some("openrouter/z-ai/glm-5.2"), &json!({})).unwrap();
        assert_eq!(model.provider_id, "openrouter");
        assert_eq!(model.model_id, "z-ai/glm-5.2");
    }

    #[test]
    fn resolve_model_prefers_request_over_settings() {
        let settings = json!({
            "opencode_model_provider": "openai",
            "opencode_model_id": "gpt-4.1",
        });
        let model = resolve_model(Some("anthropic"), Some("claude-sonnet-4"), &settings).unwrap();
        assert_eq!(model.provider_id, "anthropic");
        assert_eq!(model.model_id, "claude-sonnet-4");
    }

    #[test]
    fn resolve_model_falls_back_to_settings() {
        let settings = json!({
            "opencode_model_provider": "anthropic",
            "opencode_model_id": "claude-sonnet-4",
        });
        let model = resolve_model(None, None, &settings).unwrap();
        assert_eq!(model.provider_id, "anthropic");
        assert_eq!(model.model_id, "claude-sonnet-4");
    }

    #[test]
    fn resolve_model_none_when_unset() {
        assert!(resolve_model(None, None, &json!({})).is_none());
    }

    #[test]
    fn resolve_model_bare_id_defaults_to_opencode_go() {
        let model = resolve_model(None, Some("glm-5.2"), &json!({})).unwrap();
        assert_eq!(model.provider_id, "opencode-go");
        assert_eq!(model.model_id, "glm-5.2");
    }

    #[test]
    fn resolve_model_settings_model_id_defaults_provider_to_opencode_go() {
        let settings = json!({ "opencode_model_id": "deepseek-v4-pro" });
        let model = resolve_model(None, None, &settings).unwrap();
        assert_eq!(model.provider_id, "opencode-go");
        assert_eq!(model.model_id, "deepseek-v4-pro");
    }
}
