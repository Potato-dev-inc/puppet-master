//! Tauri commands of the chat's side question. Rules live in `crate::side_question`; the answer goes back to the
//! chat view only (nothing is written to the pane, nothing is saved).

use super::*;
use crate::askbar::AskResult;
use crate::side_question::{self, SideRequest, Target};

fn pane_agent(state: &AppState, pane_id: &str) -> Result<String, String> {
    state
        .registry
        .lock()
        .get(pane_id)
        .map(|pane| pane.info.agent_type.clone())
        .ok_or_else(|| format!("unknown pane: {pane_id}"))
}

fn installed(id: &str) -> bool {
    crate::tui::profile_for(id).is_installed()
}

/// Who would answer a side question about this pane (`null`: no installed agent can answer headless).
#[tauri::command]
pub fn side_question_target(state: State<'_, AppState>, pane_id: String) -> Result<Option<Target>, String> {
    let agent_type = pane_agent(&state, &pane_id)?;
    Ok(side_question::resolve_agent(&agent_type, &installed)
        .map(|agent| Target { label: super::askbar::label_for(&agent), agent }))
}

/// Answer one question about the pane's chat through the read-only one-shot path.
#[tauri::command]
pub async fn side_question_ask(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    request: SideRequest,
) -> Result<AskResult, String> {
    let agent_type = pane_agent(&state, &pane_id)?;
    let Some(agent) = side_question::resolve_agent(&agent_type, &installed) else {
        return Err("No agent on this computer can answer a side question without a pane.".into());
    };
    let label = super::askbar::label_for(&agent);
    let registry = state.registry.clone();
    tauri::async_runtime::spawn_blocking(move || {
        side_question::run(
            &request,
            &agent,
            &label,
            side_question::helper_dir(),
            &|text| crate::redact::text(text).into_inner(),
            &|quick| crate::agent_runs::quick::quick_ask(quick, registry.clone(), app.clone(), None),
        )
    })
    .await
    .map_err(|e| e.to_string())
}
