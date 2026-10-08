//! The operations behind `/agents/definitions` and the Tauri commands (SPEC 7.5): list, validate,
//! reload, approve, revoke. Pure over explicit dirs and an approvals path, so tests run against a
//! temp tree; the `*_global` wrappers apply the same to the app data dir and the process registry.
//!
//! Approval is the trust boundary (APR-7). Nothing here knows about sessions; the route layer
//! refuses MCP sessions before calling `approve`/`revoke`.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::approvals::{now_rfc3339, Approval, Approvals};
use super::loader::{build, load_dir, load_text, reload_global, DefState, Entry, LoadInputs};
use super::resolve::resolve_binary;
use super::schema::DefError;
use super::sources::{source_name, Origin};
use super::template;
use super::validate::Source;
use crate::standby::key::canonical_project;
use crate::tui::types::HeadlessLaunch;

/// A failed operation: SPEC 7.4 code, message and the HTTP status the bridge answers with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceError {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
}

impl ServiceError {
    fn new(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        Self { status, code, message: message.into() }
    }

    pub fn to_json(&self) -> Value {
        json!({"error": self.message, "message": self.message, "code": self.code, "recoverable": false})
    }
}

pub fn error_json(error: &DefError) -> Value {
    json!({
        "code": error.code,
        "message": error.message,
        "path": error.path,
        "line": error.line,
        "column": error.col,
    })
}

fn entry_json(entry: &Entry) -> Value {
    json!({
        "id": entry.id,
        "file": entry.path.to_string_lossy(),
        "source": entry.source,
        "state": entry.state.as_str(),
        "errors": entry.errors.iter().map(error_json).collect::<Vec<_>>(),
        "fingerprint": entry.fingerprint,
        "shadowed_by": entry.shadowed_by.as_ref().map(|p| p.to_string_lossy().into_owned()),
    })
}

/// `{sources, definitions}` for a fresh scan of the enabled sources. Reads files and approvals;
/// registers nothing.
pub fn listing(inputs: &LoadInputs<'_>, approvals_path: &Path) -> Value {
    let built = build(inputs, &Approvals::load(approvals_path));
    listing_of(inputs, &built.entries)
}

fn listing_of(inputs: &LoadInputs<'_>, entries: &[Entry]) -> Value {
    json!({
        "sources": [
            {"name": "bundled", "enabled": true, "path": Value::Null, "consent": Value::Null},
            {"name": "user", "enabled": inputs.user_dir.is_some(),
             "path": inputs.user_dir.map(|d| d.to_string_lossy().into_owned()), "consent": Value::Null},
            {"name": "project", "enabled": false, "path": Value::Null, "consent": false},
            {"name": "extra", "enabled": false, "path": Value::Null, "consent": false},
        ],
        "definitions": entries.iter().map(entry_json).collect::<Vec<_>>(),
    })
}

/// Reload the process-wide registry; the listing plus `diff: {added, removed, changed}` of the
/// registered (active) ids against the registry as it was.
pub fn reload_with_diff(inputs: &LoadInputs<'_>, approvals_path: &Path) -> Value {
    let before = active_fingerprints(&super::loader::last_entries());
    let built = reload_global(inputs, approvals_path);
    let after = active_fingerprints(&built.entries);
    let mut added = Vec::new();
    let mut changed = Vec::new();
    for (id, fp) in &after {
        match before.iter().find(|(b, _)| b == id) {
            None => added.push(id.clone()),
            Some((_, old)) if old != fp => changed.push(id.clone()),
            _ => {}
        }
    }
    let removed: Vec<String> = before
        .iter()
        .filter(|(id, _)| !after.iter().any(|(a, _)| a == id))
        .map(|(id, _)| id.clone())
        .collect();
    let mut body = listing_of(inputs, &built.entries);
    body["diff"] = json!({"added": added, "removed": removed, "changed": changed});
    body
}

fn active_fingerprints(entries: &[Entry]) -> Vec<(String, String)> {
    entries
        .iter()
        .filter(|e| e.state == DefState::Active)
        .filter_map(|e| Some((e.id.clone()?, e.fingerprint.clone().unwrap_or_default())))
        .collect()
}

fn parse_source(name: &str) -> Option<Source> {
    Some(match name {
        "bundled" => Source::Bundled,
        "user" => Source::User,
        "project" => Source::Project,
        "extra" => Source::Extra,
        _ => return None,
    })
}

/// Validate definition text without registering or approving anything: `{ok, errors, rendered}`.
pub fn validate_text(text: &str, source: &str, code_ids: &[&str]) -> Result<Value, ServiceError> {
    let source = parse_source(source)
        .ok_or_else(|| ServiceError::new(400, "INVALID_ARGUMENT", format!("unknown source: {source}")))?;
    let file = load_text(Path::new("<validate>"), text, text.len(), Origin::new(source), None, code_ids);
    let (headless_argv, tui_argv) = match &file.definition {
        Some(def) => {
            let launch = HeadlessLaunch {
                project: "<project>",
                task: "<task>",
                read_only: false,
                resume_session: Some("<session>"),
                trust_workspace: false,
                allow_shell: false,
                model: Some("<model>"),
            };
            (
                template::render_headless(def, &launch).map(|argv| json!(argv)).unwrap_or(Value::Null),
                def.tui.as_ref().map(|t| json!(t.argv)).unwrap_or(Value::Null),
            )
        }
        None => (Value::Null, Value::Null),
    };
    Ok(json!({
        "ok": file.errors.is_empty(),
        "errors": file.errors.iter().map(error_json).collect::<Vec<_>>(),
        "rendered": {"headless_argv": headless_argv, "tui_argv": tui_argv},
    }))
}

/// Approve exactly the definition the user saw: `fingerprint` must equal the file's current one,
/// and the binary must resolve now (it is pinned into the record, SEC-4).
pub fn approve(
    inputs: &LoadInputs<'_>,
    approvals_path: &Path,
    file: &str,
    fingerprint: &str,
) -> Result<Value, ServiceError> {
    let wanted = canonical_project(file);
    let user_dir = inputs
        .user_dir
        .ok_or_else(|| ServiceError::new(404, "AGENT_DEF_NOT_FOUND", "no definition source is enabled"))?;
    let loaded = load_dir(user_dir, Origin::new(Source::User), None, inputs.code_ids);
    let found = loaded
        .files
        .into_iter()
        .find(|f| canonical_project(&f.path.to_string_lossy()) == wanted)
        .ok_or_else(|| ServiceError::new(404, "AGENT_DEF_NOT_FOUND", format!("no definition file {file}")))?;
    let (Some(def), Some(current)) = (&found.definition, &found.fingerprint) else {
        let first = found.errors.first().map(|e| e.to_string()).unwrap_or_default();
        return Err(ServiceError::new(400, "AGENT_DEF_INVALID", format!("the definition is invalid: {first}")));
    };
    if current != fingerprint {
        return Err(ServiceError::new(
            409,
            "AGENT_DEF_APPROVAL_STALE",
            "the definition changed since it was listed; review it again before approving",
        ));
    }
    let binary = resolve_binary(&def.binary, user_dir)
        .map_err(|e| ServiceError::new(400, e.code(), e.to_string()))?;
    let mut approvals = Approvals::load(approvals_path);
    approvals.approve(Approval {
        id: def.agent.id.clone(),
        source: source_name(found.origin.source).to_string(),
        path: found.path.to_string_lossy().into_owned(),
        fingerprint: current.clone(),
        binary: binary.to_string_lossy().into_owned(),
        approved_at: now_rfc3339(),
    });
    approvals.save(approvals_path).map_err(|e| ServiceError::new(503, "AGENT_DEF_STORE_FAILED", e))?;
    let state = build(inputs, &approvals)
        .entries
        .iter()
        .find(|e| e.id.as_deref() == Some(def.agent.id.as_str()))
        .map(|e| e.state.as_str())
        .unwrap_or("invalid");
    Ok(json!({"ok": true, "id": def.agent.id, "state": state}))
}

/// Remove every approval of agent `id` (APR-6).
pub fn revoke(approvals_path: &Path, id: &str) -> Result<Value, ServiceError> {
    let mut approvals = Approvals::load(approvals_path);
    if !approvals.revoke(id) {
        return Err(ServiceError::new(404, "AGENT_DEF_NOT_FOUND", format!("no approval for agent {id}")));
    }
    approvals.save(approvals_path).map_err(|e| ServiceError::new(503, "AGENT_DEF_STORE_FAILED", e))?;
    Ok(json!({"ok": true, "id": id}))
}

/// Project/extra consent arrives with T34; until then there is nothing to consent to.
pub fn consent(_body: &Value) -> Result<Value, ServiceError> {
    Err(ServiceError::new(
        501,
        "AGENT_DEF_CONSENT_UNSUPPORTED",
        "project and extra definition sources are not enabled yet",
    ))
}

// ---- process-wide wrappers ----------------------------------------------------------------

/// Run `f` with the real app inputs (app data agents dir, approvals file, code ids).
pub fn with_global<R>(f: impl FnOnce(&LoadInputs<'_>, &Path) -> R) -> R {
    let code_ids = crate::tui::registry::registry().code_ids();
    let user_dir: PathBuf = crate::app_paths::agents_dir();
    let inputs = LoadInputs { bundled: &[], user_dir: Some(&user_dir), code_ids: &code_ids };
    f(&inputs, &super::approvals::default_path())
}

pub fn listing_global() -> Value {
    with_global(listing)
}

pub fn reload_global_with_diff() -> Value {
    with_global(reload_with_diff)
}

pub fn validate_global(text: &str, source: &str) -> Result<Value, ServiceError> {
    let code_ids = crate::tui::registry::registry().code_ids();
    validate_text(text, source, &code_ids)
}

/// Approve, then reload the registry so the agent is spawnable at once.
pub fn approve_global(file: &str, fingerprint: &str) -> Result<Value, ServiceError> {
    with_global(|inputs, approvals| {
        let result = approve(inputs, approvals, file, fingerprint)?;
        reload_global(inputs, approvals);
        Ok(result)
    })
}

pub fn revoke_global(id: &str) -> Result<Value, ServiceError> {
    with_global(|inputs, approvals| {
        let result = revoke(approvals, id)?;
        reload_global(inputs, approvals);
        Ok(result)
    })
}
