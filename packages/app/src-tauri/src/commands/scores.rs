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
