//! Tauri command for the tamper guard: "Test changes to check" on a receipt (read-only).

use crate::tamper::{self, Report, RunCounts};

/// Compare a project with a snapshot a mission took earlier. Reads files, writes nothing.
/// `total` and `skipped` are the test runner's counts now, when the caller has them.
#[tauri::command]
pub async fn tamper_check(project: String, since: String, total: Option<u32>, skipped: Option<u32>) -> Result<Report, String> {
    let counts = match (total, skipped) {
        (None, None) => None,
        (total, skipped) => Some(RunCounts { total, skipped, ..RunCounts::default() }),
    };
    tauri::async_runtime::spawn_blocking(move || tamper::http::check_project(&project, &since, counts).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}
