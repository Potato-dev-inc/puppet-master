//! Tauri command behind the "Your plans" page (`crate::plans_report`).

/// The report for one month (`"2026-10"`; empty or missing = this month). Reads every project's
/// operation records, so it runs off the UI thread.
#[tauri::command]
pub async fn plans_report(month: Option<String>) -> Result<crate::plans_report::model::PlansReport, String> {
    tauri::async_runtime::spawn_blocking(move || crate::plans_report::plans_report(month.as_deref()))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
}
