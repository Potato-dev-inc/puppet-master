//! Tauri command behind the "Scores on your code" page (`crate::scores`).

/// The scores for one project (an id from an earlier answer, or a folder path; missing = all
/// projects) over the last `days` days (1 to 3650; missing = all time). Reads every project's
/// operation records, so it runs off the UI thread. Read-only: no bridge route, no MCP tool.
#[tauri::command]
pub async fn agent_scores(project: Option<String>, days: Option<u32>) -> Result<crate::scores::model::ScoresReport, String> {
    tauri::async_runtime::spawn_blocking(move || crate::scores::agent_scores(project.as_deref(), days))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
}

/// "Pick for me": the one agent (and saved login) the "+" menu suggests for a project folder, from
/// the person's own Scores. The only input is the folder path: no text of the person, nothing sent
/// anywhere, nothing stored or logged. Reads the history, so it runs off the UI thread. Desktop
/// only: no bridge route, no MCP tool.
#[tauri::command]
pub async fn pick_agent(project: String) -> Result<crate::scores::pick::model::Pick, String> {
    tauri::async_runtime::spawn_blocking(move || crate::scores::pick::pick_for_folder(&project))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
}
