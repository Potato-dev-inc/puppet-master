//! Tauri commands for agent contexts and model inspection/switching.

use super::*;

#[tauri::command]
pub async fn list_agent_contexts() -> Result<Vec<crate::agent_contexts::AgentContextProfile>, String>
{
    Ok(crate::agent_contexts::list_agent_context_profiles())
}

#[derive(Deserialize)]
pub struct SwitchModelArgs {
    #[serde(default)]
    pub model_provider: Option<String>,
    pub model_id: String,
}

#[tauri::command]
pub async fn switch_agent_model(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    args: SwitchModelArgs,
) -> Result<serde_json::Value, String> {
    let settings = state.public_settings.lock().clone();
    let model = crate::opencode::resolve_model(
        args.model_provider.as_deref(),
        Some(args.model_id.as_str()),
        &settings,
    )
    .ok_or_else(|| "model_id required".to_string())?;
    crate::opencode::switch_native_model(&state.registry, &app, &pane_id, &model)?;
    Ok(crate::opencode::switch_model_response(
        &state.registry,
        &pane_id,
        &model,
    ))
}

#[tauri::command]
pub async fn inspect_agent_model(
    state: State<'_, AppState>,
    pane_id: String,
    lines: Option<usize>,
) -> Result<crate::agent_contexts::AgentModelInspection, String> {
    let pane = state
        .registry
        .lock()
        .list()
        .into_iter()
        .find(|pane| pane.id == pane_id)
        .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
    let agent_type = AgentType::parse(&pane.agent_type)
        .ok_or_else(|| format!("unknown agent_type: {}", pane.agent_type))?;
    let buffer = registry_read_buffer(&state.registry, &pane_id, lines.unwrap_or(200))?;
    Ok(crate::agent_contexts::inspect_agent_model_with_registry(
        Some(&state.registry),
        pane_id,
        agent_type,
        &buffer,
    ))
}

#[tauri::command]
pub async fn read_agent_context(
    state: State<'_, AppState>,
    agent_type: Option<String>,
    pane_id: Option<String>,
) -> Result<serde_json::Value, String> {
    if let Some(pane_id) = pane_id {
        let pane = state
            .registry
            .lock()
            .list()
            .into_iter()
            .find(|pane| pane.id == pane_id)
            .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
        let buffer = registry_read_buffer(&state.registry, &pane_id, 200)?;
        let context =
            crate::agent_contexts::build_pane_agent_context(Some(&state.registry), pane, &buffer)
                .ok_or_else(|| "unknown pane agent_type".to_string())?;
        return serde_json::to_value(context)
            .map_err(|err| format!("serialize agent context: {err}"));
    }

    let agent_type = agent_type.ok_or_else(|| "agent_type or pane_id is required".to_string())?;
    let agent_type =
        AgentType::parse(&agent_type).ok_or_else(|| format!("unknown agent_type: {agent_type}"))?;
    serde_json::to_value(crate::agent_contexts::get_agent_context_profile(agent_type))
        .map_err(|err| format!("serialize agent context profile: {err}"))
}
