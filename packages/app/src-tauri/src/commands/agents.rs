//! Tauri command for the agent registry listing (SPEC UI-2): the same body as `GET /agents/types`.

use super::*;

/// `{agents, dispatchable_ids}` for every registered agent, in registry order (SPEC 7.5).
#[tauri::command]
pub fn agents_list() -> Value {
    crate::tui::agent_types_listing()
}

fn command_error(error: crate::tui::declarative::service::ServiceError) -> String {
    format!("{}: {}", error.code, error.message)
}

/// `GET /agents/definitions`: every definition with state and errors.
#[tauri::command]
pub fn agents_definitions_list() -> Value {
    crate::tui::declarative::service::listing_global()
}

/// `POST /agents/definitions/reload`: rescan and re-register; the listing plus a diff.
#[tauri::command]
pub fn agents_definitions_reload() -> Value {
    crate::tui::declarative::service::reload_global_with_diff()
}

/// `POST /agents/definitions/validate`: registers and approves nothing.
#[tauri::command]
pub fn agents_definitions_validate(toml: String, source: Option<String>) -> Result<Value, String> {
    crate::tui::declarative::service::validate_global(&toml, source.as_deref().unwrap_or("user"))
        .map_err(command_error)
}

/// `POST /agents/definitions/approve`. Tauri commands are the webview's own channel, so this is
/// the app-session path of APR-7.
#[tauri::command]
pub fn agents_definition_approve(file: String, fingerprint: String) -> Result<Value, String> {
    crate::tui::declarative::service::approve_global(&file, &fingerprint).map_err(command_error)
}

/// The validation result (`ok`, `errors`, `rendered` argv) for one listed definition file, so the
/// approve dialog can show what would run (APR-3). Only files the last scan listed can be read.
#[tauri::command]
pub fn agents_definition_preview(file: String) -> Result<Value, String> {
    let wanted = std::fs::canonicalize(&file).unwrap_or_else(|_| PathBuf::from(&file));
    let entry = crate::tui::declarative::loader::last_entries()
        .into_iter()
        .find(|e| std::fs::canonicalize(&e.path).unwrap_or_else(|_| e.path.clone()) == wanted)
        .ok_or_else(|| format!("AGENT_DEF_NOT_FOUND: no listed definition file {file}"))?;
    let text = std::fs::read_to_string(&entry.path).map_err(|e| format!("AGENT_DEF_NOT_FOUND: {e}"))?;
    let mut preview = crate::tui::declarative::service::validate_global(&text, entry.source).map_err(command_error)?;
    preview["approved"] = crate::tui::declarative::service::approved_snapshot_global(&entry.path.to_string_lossy());
    preview["binary_changed"] = serde_json::json!(entry.binary_changed);
    Ok(preview)
}

/// Runs a definition file against a folder of captures with `*.expect.toml` files (P3-2). Reads the
/// definition and the regular files directly inside the folder; nothing is registered or approved.
#[tauri::command]
pub fn agents_check_file(path: String, fixtures_dir: Option<String>) -> Result<Value, String> {
    let file = PathBuf::from(&path);
    if file.extension().and_then(|e| e.to_str()) != Some("toml") {
        return Err("AGENT_DEF_NOT_FOUND: a definition file ends in .toml".into());
    }
    let dir = fixtures_dir.filter(|d| !d.trim().is_empty()).map(PathBuf::from);
    let ids = crate::tui::registry::registry().code_ids();
    let results = crate::tui::declarative::check::check_file(&file, dir.as_deref(), &ids)?;
    serde_json::to_value(results).map_err(|e| e.to_string())
}

/// "Add as generic agent" (P4-2): writes an unapproved generic definition for `command` into the
/// user agents folder and rescans. The listing shows it as `unapproved`; nothing is approved here.
#[tauri::command]
pub fn agents_add_generic(command: String) -> Result<Value, String> {
    let ids = crate::tui::registry::registry().code_ids();
    crate::tui::declarative::loader::write_generic(&crate::app_paths::agents_dir(), &command, &ids)?;
    Ok(crate::tui::declarative::service::reload_global_with_diff())
}

#[tauri::command]
pub fn agents_definition_revoke(id: String) -> Result<Value, String> {
    crate::tui::declarative::service::revoke_global(&id).map_err(command_error)
}

#[tauri::command]
pub fn agents_definitions_consent(kind: String, path: String, enabled: bool) -> Result<Value, String> {
    crate::tui::declarative::service::consent(&serde_json::json!({"kind": kind, "path": path, "enabled": enabled}))
        .map_err(command_error)
}

/// SRC-5: switch the user / project / extra source on or off.
#[tauri::command]
pub fn agents_source_set(name: String, enabled: bool) -> Result<Value, String> {
    crate::tui::declarative::service::set_source(&serde_json::json!({"name": name, "enabled": enabled}))
        .map_err(command_error)
}

/// SRC-4: add (`"add"`) or remove (`"remove"`) an extra definition directory. A new one has no
/// consent and loads nothing until `agents_definitions_consent` allows it.
#[tauri::command]
pub fn agents_extra_dir(action: String, path: String) -> Result<Value, String> {
    crate::tui::declarative::service::extra_dir(&serde_json::json!({"action": action, "path": path}))
        .map_err(command_error)
}

/// Consent banner (P5-3): the definition file names found in `<project>/.puppet-master/agents`
/// and whether this canonical path is consented. Reads names only, never the files.
#[tauri::command]
pub fn agents_project_scan(project: String) -> Value {
    crate::tui::declarative::service::project_scan(&project)
}

/// Record-session toolbar (REC-1): start recording a pane. Refused for a standby-pool pane.
#[tauri::command]
pub fn recorder_start(state: State<'_, AppState>, pane_id: String) -> Result<Value, String> {
    let (agent_type, status) = {
        let reg = state.registry.lock();
        let pane = reg.get(&pane_id).ok_or_else(|| format!("PANE_NOT_FOUND: {pane_id}"))?;
        let status = *pane.status.lock();
        (pane.info.agent_type.clone(), status)
    };
    let screen = crate::pty::registry::read_snapshot(&state.registry, &pane_id).unwrap_or_default();
    crate::agent_definitions_recorder::start(
        &pane_id,
        &agent_type,
        crate::pty::hidden::is_hidden(&pane_id),
        Some((status, screen.as_str())),
    )?;
    Ok(recorder_status_json(&pane_id))
}

fn recorder_status_json(pane_id: &str) -> Value {
    let session = crate::agent_definitions_recorder::session(pane_id);
    serde_json::json!({
        "recording": crate::agent_definitions_recorder::is_recording(pane_id),
        "snapshots": session.as_ref().map_or(0, |s| s.snapshots.len()),
    })
}

#[tauri::command]
pub fn recorder_status(pane_id: String) -> Value {
    recorder_status_json(&pane_id)
}

/// Stop recording and write the draft to `<app data>/agents/drafts/<id>.toml` (REC-4); the draft is
/// never loaded or approved. `command` defaults to the pane agent type.
#[tauri::command]
pub fn recorder_stop(pane_id: String, id: Option<String>, label: Option<String>, command: Option<String>) -> Result<Value, String> {
    let session = crate::agent_definitions_recorder::stop(&pane_id)
        .ok_or_else(|| "RECORDER_NOT_RECORDING: this pane is not being recorded".to_string())?;
    write_session_draft(&session, id, label, command, &crate::agent_definitions_recorder::draft::drafts_dir())
}

fn write_session_draft(
    session: &crate::agent_definitions_recorder::Session,
    id: Option<String>,
    label: Option<String>,
    command: Option<String>,
    dir: &std::path::Path,
) -> Result<Value, String> {
    use crate::agent_definitions_recorder::draft;
    let command = command.filter(|c| !c.trim().is_empty()).unwrap_or_else(|| session.agent_type.clone());
    let id = id.filter(|c| !c.trim().is_empty()).unwrap_or_else(|| format!("{}_draft", session.agent_type));
    let label = label.filter(|c| !c.trim().is_empty()).unwrap_or_else(|| format!("{} (draft)", session.agent_type));
    let input = draft::DraftInput::from_session(session, &id, &label, &command, None);
    let generated = draft::generate(&input);
    let path = draft::write_draft(dir, &generated)?;
    Ok(serde_json::json!({
        "path": path.display().to_string(),
        "id": generated.id,
        "notes": generated.notes,
        "snapshots": session.snapshots.len(),
    }))
}

/// The drafts folder and the draft files in it, newest first.
#[tauri::command]
pub fn agents_drafts_list() -> Value {
    let dir = crate::agent_definitions_recorder::draft::drafts_dir();
    let mut rows: Vec<(i64, Value)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("toml") {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let modified = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as i64);
            rows.push((
                modified,
                serde_json::json!({
                    "name": e.file_name().to_string_lossy(),
                    "path": path.display().to_string(),
                    "modified_ms": modified,
                    "size": meta.len(),
                }),
            ));
        }
    }
    rows.sort_by_key(|(m, _)| std::cmp::Reverse(*m));
    serde_json::json!({"dir": dir.display().to_string(), "drafts": rows.into_iter().map(|(_, v)| v).collect::<Vec<_>>()})
}

/// "Validate draft": same result as `POST /agents/definitions/validate` for a file in the drafts folder.
#[tauri::command]
pub fn agents_draft_validate(path: String) -> Result<Value, String> {
    validate_draft_in(&crate::agent_definitions_recorder::draft::drafts_dir(), &path)
}

fn validate_draft_in(dir: &std::path::Path, path: &str) -> Result<Value, String> {
    let root = std::fs::canonicalize(dir).map_err(|_| "AGENT_DEF_NOT_FOUND: there is no drafts folder yet".to_string())?;
    let file = std::fs::canonicalize(path).map_err(|_| format!("AGENT_DEF_NOT_FOUND: {path}"))?;
    if file.parent() != Some(root.as_path()) || file.extension().and_then(|e| e.to_str()) != Some("toml") {
        return Err("AGENT_DEF_NOT_FOUND: only a .toml file directly in the drafts folder can be validated here".into());
    }
    let text = std::fs::read_to_string(&file).map_err(|e| format!("AGENT_DEF_NOT_FOUND: {e}"))?;
    crate::tui::declarative::service::validate_global(&text, "user").map_err(command_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_file_refuses_non_toml_and_reports_fixtures() {
        assert!(agents_check_file("C:/x/notes.txt".into(), None).unwrap_err().starts_with("AGENT_DEF_NOT_FOUND"));
        let kilo = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/agent-definitions/kilo.toml");
        let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/src/tui/declarative/fixtures/kilo");
        let body = agents_check_file(kilo.into(), Some(fixtures.into())).unwrap();
        assert!(body.as_array().unwrap().iter().all(|r| r["passed"] == true), "{body}");
    }

    #[test]
    fn preview_refuses_a_file_the_scan_did_not_list() {
        let error = agents_definition_preview("C:\\definitely\\not\\listed.toml".into()).unwrap_err();
        assert!(error.starts_with("AGENT_DEF_NOT_FOUND"), "{error}");
    }

    #[test]
    fn agents_list_returns_the_nine_code_agents() {
        let body = agents_list();
        let ids: Vec<&str> = body["agents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|agent| agent["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            [
                "claude",
                "codex",
                "opencode",
                "opencode_native",
                "cursor_agent",
                "cursor",
                "cmd",
                "powershell",
                "bash"
            ]
        );
        assert_eq!(body["agents"][0]["label"], "Claude Code");
        assert_eq!(body["dispatchable_ids"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn draft_validation_is_limited_to_the_drafts_folder() {
        let dir = std::env::temp_dir().join(format!("pm-drafts-cmd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let inside = dir.join("a.toml");
        std::fs::write(&inside, "schema_version = 1\n[agent]\nid = \"zz_draft\"\nlabel = \"Z\"\n[binary]\nnames = [\"zz\"]\n").unwrap();
        let body = validate_draft_in(&dir, inside.to_str().unwrap()).unwrap();
        assert!(body.get("ok").is_some());
        let outside = std::env::temp_dir().join(format!("pm-drafts-outside-{}.toml", std::process::id()));
        std::fs::write(&outside, "x = 1").unwrap();
        assert!(validate_draft_in(&dir, outside.to_str().unwrap()).unwrap_err().starts_with("AGENT_DEF_NOT_FOUND"));
        std::fs::remove_file(&inside).unwrap();
        std::fs::remove_file(&outside).unwrap();
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn recorder_stop_writes_a_draft_that_validates() {
        let pane = "cmd-rec-test-pane";
        crate::agent_definitions_recorder::start(pane, "kilo", false, Some((crate::pty::status::PaneStatus::Idle, "ready  ctrl+p commands  tab agents"))).unwrap();
        let session = crate::agent_definitions_recorder::stop(pane).unwrap();
        let dir = std::env::temp_dir().join(format!("pm-rec-cmd-{}", std::process::id()));
        let out = write_session_draft(&session, None, None, None, &dir).unwrap();
        let path = out["path"].as_str().unwrap().to_string();
        assert!(path.contains("kilo_draft"));
        let body = validate_draft_in(&dir, &path).unwrap();
        assert_eq!(body["ok"], true, "{body}");
        std::fs::remove_file(&path).unwrap();
        let _ = std::fs::remove_dir(&dir);
    }
}

/// Sharing (P5-6): copy the user definition `id` to `target` (+ `<stem>.fixtures.zip` when it has
/// fixtures). Writes only the chosen target files; no network.
#[tauri::command]
pub fn agents_export_definition(id: String, target: String) -> Result<Value, String> {
    let done = crate::tui::declarative::loader::export_definition(
        &crate::app_paths::agents_dir(),
        &id,
        std::path::Path::new(&target),
    )?;
    Ok(serde_json::json!({
        "path": done.toml_path.to_string_lossy(),
        "fixtures_zip": done.fixtures_zip.map(|p| p.to_string_lossy().to_string()),
    }))
}

/// Sharing (P5-6): validate `source` and place it in the user agents folder UNAPPROVED, then
/// rescan. An old approval record for the same path and fingerprint is dropped first so an import
/// can never come back already active.
#[tauri::command]
pub fn agents_import_definition(source: String) -> Result<Value, String> {
    let ids = crate::tui::registry::registry().code_ids();
    let placed = crate::tui::declarative::loader::import_definition(
        &crate::app_paths::agents_dir(),
        std::path::Path::new(&source),
        &ids,
    )?;
    let file = placed.to_string_lossy().to_string();
    if !crate::tui::declarative::service::approved_snapshot_global(&file).is_null() {
        let id = placed.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let shared = crate::tui::declarative::loader::last_entries()
            .iter()
            .any(|e| e.id.as_deref() == Some(id.as_str()) && e.path != placed);
        if !shared {
            let _ = crate::tui::declarative::service::revoke_global(&id);
        }
    }
    Ok(crate::tui::declarative::service::reload_global_with_diff())
}
