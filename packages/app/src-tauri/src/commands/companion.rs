//! Tauri commands of the companion (spec section 11). Desktop only: making and editing a companion, its
//! memory and its check-ins are never reachable through the bridge, the phone or MCP. The phone has the
//! read, keep-or-drop, wake and sleep routes of `bridge/routes/companions.rs`, over the same service.
//!
//! Errors are one string, `CODE: plain sentence` (`COMPANION_LIMIT: You can have at most 3 ...`).
//! The desktop views carry the home folder and the full check-in list; `companion://changed` sends them
//! all whenever one changes.

use std::sync::Arc;

use serde::Serialize;

use crate::companion::manage::{Draft, MemoryView, Patch};
use crate::companion::model::{CheckIn, CompanionError, CompanionResult, CompanionView, FeedItem, MemoryFile};
use crate::companion::skills::SkillFull;
use crate::companion::{agents, files, live, Service};

/// One agent the setup sheet can pick.
#[derive(Serialize)]
pub struct CompanionAgentOption {
    id: String,
    label: String,
    ok: bool,
    reason: Option<String>,
}

fn service() -> Result<Arc<Service>, String> {
    live::service().ok_or_else(|| "COMPANION_NOT_READY: Companions are not ready yet. Try again in a moment.".to_string())
}

/// Runs one service call off the UI thread (a start may take a moment: it writes files and opens a pane).
async fn run<T: Send + 'static>(call: impl FnOnce(&Service) -> CompanionResult<T> + Send + 'static) -> Result<T, String> {
    let service = service()?;
    tauri::async_runtime::spawn_blocking(move || call(&service).map_err(|e| e.to_string()))
        .await
        .map_err(|e| format!("INTERNAL: {e}"))?
}

#[tauri::command]
pub async fn companion_list() -> Result<Vec<CompanionView>, String> {
    Ok(service()?.views(false))
}

#[tauri::command]
pub async fn companion_get(id: String) -> Result<CompanionView, String> {
    service()?.view(&id, false).map_err(|e| e.to_string())
}

/// Every registered agent, with whether it can be a companion and, if not, why in plain words.
#[tauri::command]
pub async fn companion_agents() -> Result<Vec<CompanionAgentOption>, String> {
    let folder = files::user_home().map(|p| p.display().to_string()).unwrap_or_default();
    Ok(crate::tui::descriptors()
        .into_iter()
        .map(|d| {
            let verdict = agents::agent_ok(d.id, &folder);
            CompanionAgentOption { id: d.id.to_string(), label: d.label, ok: verdict.is_ok(), reason: verdict.err().map(str::to_string) }
        })
        .collect())
}

#[tauri::command]
pub async fn companion_create(input: Draft) -> Result<CompanionView, String> {
    run(move |s| s.create(input)).await
}

#[tauri::command]
pub async fn companion_update(id: String, patch: Patch) -> Result<CompanionView, String> {
    run(move |s| s.update(&id, patch)).await
}

#[tauri::command]
pub async fn companion_archive(id: String) -> Result<(), String> {
    run(move |s| s.archive(&id)).await
}

#[tauri::command]
pub async fn companion_wake(id: String) -> Result<CompanionView, String> {
    run(move |s| s.wake(&id)).await
}

#[tauri::command]
pub async fn companion_sleep(id: String) -> Result<CompanionView, String> {
    run(move |s| s.sleep(&id)).await
}

#[tauri::command]
pub async fn companion_restart(id: String) -> Result<CompanionView, String> {
    run(move |s| s.restart(&id)).await
}

#[tauri::command]
pub async fn companion_memory_get(id: String) -> Result<MemoryView, String> {
    run(move |s| s.memory_view(&id, false)).await
}

#[tauri::command]
pub async fn companion_memory_save(id: String, file: MemoryFile, text: String) -> Result<MemoryView, String> {
    run(move |s| s.memory_save(&id, file, &text)).await
}

/// `decision`: `keep` or `drop`.
#[tauri::command]
pub async fn companion_pending_decide(id: String, item_id: String, decision: String) -> Result<MemoryView, String> {
    let keep = parse_decision(&decision, "keep", "drop").map_err(|e| e.to_string())?;
    run(move |s| s.pending_decide(&id, &item_id, keep, false)).await
}

/// Approves a proposed skill; `title` and `body` are the person's edits (missing = as proposed).
#[tauri::command]
pub async fn companion_skill_approve(id: String, item_id: String, title: Option<String>, body: Option<String>) -> Result<MemoryView, String> {
    run(move |s| s.skill_approve(&id, &item_id, title.as_deref(), body.as_deref())).await
}

#[tauri::command]
pub async fn companion_skill_get(id: String, slug: String) -> Result<SkillFull, String> {
    run(move |s| s.skill_get(&id, &slug)).await
}

#[tauri::command]
pub async fn companion_skill_save(id: String, slug: String, title: String, body: String) -> Result<MemoryView, String> {
    run(move |s| s.skill_save(&id, &slug, &title, &body)).await
}

/// Removing moves the file to `skills/.removed/` inside the home folder.
#[tauri::command]
pub async fn companion_skill_remove(id: String, slug: String) -> Result<MemoryView, String> {
    run(move |s| s.skill_remove(&id, &slug)).await
}

/// `decision`: `keep` (the file as it is now becomes the approved one) or `undo`.
#[tauri::command]
pub async fn companion_outside_change_decide(id: String, file: MemoryFile, decision: String) -> Result<MemoryView, String> {
    let keep = parse_decision(&decision, "keep", "undo").map_err(|e| e.to_string())?;
    run(move |s| s.outside_decide(&id, file, keep)).await
}

#[tauri::command]
pub async fn companion_tidy_up(id: String) -> Result<(), String> {
    run(move |s| s.tidy_up(&id)).await
}

#[tauri::command]
pub async fn companion_checkins_set(id: String, checkins: Vec<CheckIn>) -> Result<CompanionView, String> {
    run(move |s| s.checkins_set(&id, checkins)).await
}

#[tauri::command]
pub async fn companion_feed(id: String, since_ms: Option<i64>) -> Result<Vec<FeedItem>, String> {
    run(move |s| s.feed_list(&id, since_ms.unwrap_or(0))).await
}

/// Opens the home folder in the system file manager.
#[tauri::command]
pub async fn companion_open_folder(id: String) -> Result<(), String> {
    let folder = run(move |s| s.folder_of(&id)).await?;
    open_in_file_manager(&folder)
}

fn parse_decision(word: &str, yes: &str, no: &str) -> CompanionResult<bool> {
    if word == yes {
        Ok(true)
    } else if word == no {
        Ok(false)
    } else {
        Err(CompanionError::new("COMPANION_PENDING_STALE", format!("The answer must be \"{yes}\" or \"{no}\".")))
    }
}

fn open_in_file_manager(folder: &str) -> Result<(), String> {
    #[cfg(windows)]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let program = "xdg-open";
    crate::proc_util::hidden_command(program)
        .arg(folder)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("COMPANION_FOLDER_INVALID: Could not open the folder: {e}."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decisions_are_exact_words() {
        assert!(parse_decision("keep", "keep", "drop").unwrap());
        assert!(!parse_decision("drop", "keep", "drop").unwrap());
        assert!(!parse_decision("undo", "keep", "undo").unwrap());
        assert!(parse_decision("maybe", "keep", "drop").is_err());
    }

    #[test]
    fn a_command_before_the_service_starts_says_so_in_plain_words() {
        // The service is a process-wide singleton started in the app setup; unit tests never start it.
        if live::service().is_none() {
            assert!(service().err().unwrap().starts_with("COMPANION_NOT_READY"));
        }
    }
}
