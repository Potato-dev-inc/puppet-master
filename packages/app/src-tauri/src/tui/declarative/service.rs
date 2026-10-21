//! The operations behind `/agents/definitions` and the Tauri commands (SPEC 7.5): list, validate,
//! reload, approve, revoke. Pure over explicit dirs and an approvals path, so tests run against a
//! temp tree; the `*_global` wrappers apply the same to the app data dir and the process registry.
//!
//! Approval is the trust boundary (APR-7). Nothing here knows about sessions; the route layer
//! refuses MCP sessions before calling `approve`/`revoke`.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::approvals::{now_rfc3339, Approval, Approvals};
use super::loader::{build, load_all, load_text, reload_global, DefState, Entry, LoadInputs, OwnedInputs};
use super::resolve::{path_dirs, resolve_binary, resolve_in, Platform, ResolveError};
use super::schema::DefError;
use super::sources::{project_candidates, settings_path, source_name, Origin, SourceSettings};
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
    pub(crate) fn new(status: u16, code: &'static str, message: impl Into<String>) -> Self {
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

fn entry_json(entry: &Entry, approvals: &Approvals) -> Value {
    // Who approved it, when the wizard did (SPEC 11.19): Settings shows "Approved by the wizard after N checks passed".
    let record = (entry.state == DefState::Active).then(|| approvals.get(&entry.path)).flatten();
    let by_wizard = record.is_some_and(|r| r.approved_by.as_deref() == Some(super::approvals::APPROVED_BY_WIZARD));
    json!({
        "id": entry.id,
        "file": entry.path.to_string_lossy(),
        "source": entry.source,
        "state": entry.state.as_str(),
        "errors": entry.errors.iter().map(error_json).collect::<Vec<_>>(),
        "fingerprint": entry.fingerprint,
        "shadowed_by": entry.shadowed_by.as_ref().map(|p| p.to_string_lossy().into_owned()),
        "binary_changed": entry.binary_changed,
        "experimental": entry.meta.as_ref().is_some_and(|m| m.experimental),
        "label": entry.meta.as_ref().map(|m| m.label.clone()),
        "approved_by": if by_wizard { json!(super::approvals::APPROVED_BY_WIZARD) } else { Value::Null },
        "approval_checks": if by_wizard { json!(record.map_or(0, |r| r.checks.len())) } else { Value::Null },
    })
}

/// `{sources, definitions}` for a fresh scan of the enabled sources. Reads files and approvals;
/// registers nothing. Source switches come from `settings`.
pub fn listing_with(inputs: &LoadInputs<'_>, approvals_path: &Path, settings: &SourceSettings) -> Value {
    let approvals = Approvals::load(approvals_path);
    let built = build(inputs, &approvals);
    listing_of(inputs, settings, &built.entries, &approvals)
}

/// `listing_with` for default settings (only the user source differs from "off").
pub fn listing(inputs: &LoadInputs<'_>, approvals_path: &Path) -> Value {
    listing_with(inputs, approvals_path, &SourceSettings::default())
}

fn listing_of(inputs: &LoadInputs<'_>, settings: &SourceSettings, entries: &[Entry], approvals: &Approvals) -> Value {
    let projects: Vec<Value> = settings
        .sources
        .project
        .consented_projects
        .iter()
        .map(|p| json!({"path": p, "files": project_candidates(Path::new(p))}))
        .collect();
    let extra: Vec<Value> = settings
        .sources
        .extra
        .dirs
        .iter()
        .map(|d| json!({"path": d.path, "consent": d.consented, "exists": Path::new(&d.path).is_dir()}))
        .collect();
    json!({
        "sources": [
            {"name": "bundled", "enabled": true, "path": Value::Null, "consent": Value::Null},
            {"name": "user", "enabled": inputs.user_dir.is_some(),
             "path": inputs.user_dir.map(|d| d.to_string_lossy().into_owned()), "consent": Value::Null},
            {"name": "project", "enabled": settings.sources.project.enabled, "path": Value::Null, "consent": Value::Null},
            {"name": "extra", "enabled": settings.sources.extra.enabled, "path": Value::Null, "consent": Value::Null},
        ],
        "projects": projects,
        "extra_dirs": extra,
        "definitions": entries.iter().map(|e| entry_json(e, approvals)).collect::<Vec<_>>(),
    })
}

/// Reload the process-wide registry; the listing plus `diff: {added, removed, changed}` of the
/// registered (active) ids against the registry as it was.
pub fn reload_with_diff(inputs: &LoadInputs<'_>, approvals_path: &Path, settings: &SourceSettings) -> Value {
    let before = super::loader::last_entries();
    let built = reload_global(inputs, approvals_path);
    let diff = super::watch::diff_entries(&before, &built.entries);
    super::watch::announce(&diff);
    let mut body = listing_of(inputs, settings, &built.entries, &Approvals::load(approvals_path));
    body["diff"] = diff.to_json();
    body
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
    validate_text_in(text, source, code_ids, &path_dirs(Path::new(".")))
}

/// `validate_text`, looking for the program in `dirs` (what the Approve dialog shows as the program).
pub fn validate_text_in(text: &str, source: &str, code_ids: &[&str], dirs: &[PathBuf]) -> Result<Value, ServiceError> {
    let source = parse_source(source)
        .ok_or_else(|| ServiceError::new(400, "INVALID_ARGUMENT", format!("unknown source: {source}")))?;
    let file = load_text(Path::new("<validate>"), text, text.len(), Origin::new(source), None, code_ids);
    let warnings = file.definition.as_ref().map(approval_warnings).unwrap_or_default();
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
                tui_argv_preview(def).map(|a| json!(a)).unwrap_or(Value::Null),
            )
        }
        None => (Value::Null, Value::Null),
    };
    Ok(json!({
        "ok": file.errors.is_empty(),
        "errors": file.errors.iter().map(error_json).collect::<Vec<_>>(),
        "rendered": {"headless_argv": headless_argv, "tui_argv": tui_argv},
        "program": file.definition.as_ref().map_or(Value::Null, |def| {
            program_json(def, resolve_in(&def.binary, dirs, Platform::current()))
        }),
        "warnings": warnings,
    }))
}

/// The program the Approve dialog shows in front of the arguments: `name` (the definition's first binary
/// name), `path` (absolute, null when not found), `display` (the path with the user folder as `~`, else the
/// name), `found`, a short `note` when it was not found, and `tui_argv_has_program` (a generic agent's visible
/// command already starts with the program name, so the dialog must not print it twice). This is the same
/// resolution approval pins into the record, so it is the program that will run.
pub(crate) fn program_json(def: &super::schema::Definition, resolved: Result<PathBuf, ResolveError>) -> Value {
    let name = def.binary.names.first().cloned().unwrap_or_default();
    let (path, note) = match resolved {
        Ok(path) => (Some(path), None),
        Err(ResolveError::NotFound { .. }) => (None, Some("not found on this PC")),
        Err(ResolveError::ShimNeedsNative { .. }) => (None, Some("only a script shim was found, not the program itself")),
    };
    json!({
        "name": name,
        "path": path.as_ref().map(|p| p.to_string_lossy().into_owned()),
        "display": path.as_ref().map_or_else(|| name.clone(), |p| super::wizard::locate::display_path(p)),
        "found": path.is_some(),
        "note": note,
        "tui_argv_has_program": def.agent.kind == super::schema::Kind::Generic,
    })
}

/// Flags the approval dialog must show (P5-4): risky argv switches, environment overrides and
/// interpreter binaries. Reported from the definition text only; the argv shown is the real one.
pub fn approval_warnings(def: &super::schema::Definition) -> Vec<String> {
    const RISKY: [(&str, &str); 4] = [
        ("--auto", "passes `--auto`, which usually lets the agent act without asking"),
        ("--dangerously", "passes a `--dangerously...` flag that turns off safety checks"),
        ("--yolo", "passes `--yolo`, which usually skips every confirmation"),
        ("--force", "passes `--force`, which overrides safety checks"),
    ];
    let mut argvs: Vec<&String> = Vec::new();
    if let Some(h) = &def.headless {
        argvs.extend(h.argv.iter());
        argvs.extend(h.optional.iter().flat_map(|s| s.args.iter()));
    }
    if let Some(t) = &def.tui {
        argvs.extend(t.argv.iter());
        argvs.extend(t.optional.iter().flat_map(|s| s.args.iter()));
        if let Some(read_only) = &t.read_only_argv {
            argvs.extend(read_only.iter());
        }
    }
    let mut out = Vec::new();
    for (flag, why) in RISKY {
        if argvs.iter().any(|a| a.to_ascii_lowercase().starts_with(flag)) {
            out.push(format!("The command {why}."));
        }
    }
    let keys: Vec<&String> = def.env.set.keys().collect();
    if !keys.is_empty() {
        let names = keys.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(", ");
        out.push(format!("It sets environment variables for the program: {names}."));
    }
    if def.agent.allow_interpreter {
        out.push("It runs a shell or interpreter (allow_interpreter), which can run anything you type into it.".into());
    }
    out
}

/// Approve exactly the definition the user saw: `fingerprint` must equal the file's current one,
/// and the binary must resolve now (it is pinned into the record, SEC-4). Only files of enabled,
/// consented sources can be found, so nothing outside them is ever approved.
pub fn approve(
    inputs: &LoadInputs<'_>,
    approvals_path: &Path,
    file: &str,
    fingerprint: &str,
) -> Result<Value, ServiceError> {
    approve_as(inputs, approvals_path, file, fingerprint, None)
}

/// Who is approving when it is not a person: the Agent Wizard after every check passed (SPEC 11.19). The
/// record is the same one, with `approved_by` and the labels of the checks next to the usual fields.
pub struct WizardApproval<'a> {
    pub checks: &'a [String],
    /// The folders the wizard looked for the program in (its own search path); `None`: the process `PATH`.
    pub search: Option<&'a [PathBuf]>,
}

/// `approve`, recording `by` when the approval is the wizard's own. Everything else is identical: the
/// fingerprint must be the file's current one, the binary must resolve now and is pinned.
pub fn approve_as(
    inputs: &LoadInputs<'_>,
    approvals_path: &Path,
    file: &str,
    fingerprint: &str,
    by: Option<&WizardApproval<'_>>,
) -> Result<Value, ServiceError> {
    let wanted = canonical_project(file);
    let (files, _) = load_all(inputs);
    let found = files
        .into_iter()
        .filter(|f| f.origin.source != Source::Bundled)
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
    let here = found.path.parent().unwrap_or_else(|| Path::new("."));
    let resolved = match by.and_then(|b| b.search) {
        Some(dirs) => resolve_in(&def.binary, dirs, super::resolve::Platform::current()),
        None => resolve_binary(&def.binary, here),
    };
    let binary = resolved.map_err(|e| ServiceError::new(400, e.code(), e.to_string()))?;
    let (headless_argv, tui_argv) = rendered_argv(def);
    let mut approvals = Approvals::load(approvals_path);
    approvals.approve(Approval {
        id: def.agent.id.clone(),
        source: source_name(found.origin.source).to_string(),
        path: found.path.to_string_lossy().into_owned(),
        fingerprint: current.clone(),
        binary: binary.to_string_lossy().into_owned(),
        approved_at: now_rfc3339(),
        headless_argv,
        tui_argv,
        approved_by: by.map(|_| super::approvals::APPROVED_BY_WIZARD.to_string()),
        checks: by.map(|b| b.checks.to_vec()).unwrap_or_default(),
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

/// The wizard's own approval (SPEC 11.19): the same record as a person's, made for the file as it is NOW (the wizard
/// just wrote it and has checked exactly that text), with `approved_by: "wizard"` and the checks that passed.
pub fn approve_current_as_wizard(
    inputs: &LoadInputs<'_>,
    approvals_path: &Path,
    file: &str,
    checks: &[String],
    search: Option<&[PathBuf]>,
) -> Result<Value, ServiceError> {
    let wanted = canonical_project(file);
    let (files, _) = load_all(inputs);
    let fingerprint = files
        .iter()
        .filter(|f| f.origin.source != Source::Bundled)
        .find(|f| canonical_project(&f.path.to_string_lossy()) == wanted)
        .and_then(|f| f.fingerprint.clone())
        .ok_or_else(|| ServiceError::new(404, "AGENT_DEF_NOT_FOUND", format!("no definition file {file}")))?;
    approve_as(inputs, approvals_path, file, &fingerprint, Some(&WizardApproval { checks, search }))
}

/// The visible-pane command the Approve dialog shows. A generic agent has no `[tui]` table (it is
/// just its binary in a pane), so it renders as the binary name followed by any `[tui]` args.
pub(crate) fn tui_argv_preview(def: &super::schema::Definition) -> Option<Vec<String>> {
    match (&def.tui, def.agent.kind == super::schema::Kind::Generic) {
        (tui, true) => {
            let mut argv = vec![def.binary.names.first()?.clone()];
            argv.extend(tui.as_ref().map(|t| t.argv.clone()).unwrap_or_default());
            Some(argv)
        }
        (Some(t), false) => Some(t.argv.clone()),
        (None, false) => None,
    }
}

/// The argv templates the Approve dialog shows, so the record can later be diffed against them.
fn rendered_argv(def: &super::schema::Definition) -> (Option<Vec<String>>, Option<Vec<String>>) {
    let launch = HeadlessLaunch {
        project: "<project>",
        task: "<task>",
        read_only: false,
        resume_session: Some("<session>"),
        trust_workspace: false,
        allow_shell: false,
        model: Some("<model>"),
    };
    (template::render_headless(def, &launch), tui_argv_preview(def))
}

/// What the Approve dialog needs for one listed file beyond `validate_text`: the argv the user
/// approved earlier (for the diff) and the pinned binary.
pub fn approved_snapshot(approvals_path: &Path, file: &str) -> Value {
    let approvals = Approvals::load(approvals_path);
    match approvals.get(Path::new(file)) {
        Some(record) => json!({
            "headless_argv": record.headless_argv,
            "tui_argv": record.tui_argv,
            "binary": record.binary,
            "approved_at": record.approved_at,
            "fingerprint": record.fingerprint,
        }),
        None => Value::Null,
    }
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

/// Apply `change` to the settings file at `path` and save it. Returns what `change` returned.
fn update_settings<R>(
    path: &Path,
    change: impl FnOnce(&mut SourceSettings) -> Result<R, String>,
) -> Result<R, ServiceError> {
    let mut settings = SourceSettings::load(path);
    let out = change(&mut settings).map_err(|m| ServiceError::new(400, "INVALID_ARGUMENT", m))?;
    settings.save(path).map_err(|e| ServiceError::new(503, "AGENT_DEF_STORE_FAILED", e))?;
    Ok(out)
}

/// `POST /agents/definitions/consent` `{kind: "project"|"extra", path, enabled}`: record or
/// withdraw consent for one canonical path. Consent alone loads nothing: the file still needs
/// approval (SEC-2). Consenting a project also turns the project source on.
pub fn consent_at(settings_file: &Path, body: &Value) -> Result<Value, ServiceError> {
    let kind = body.get("kind").and_then(Value::as_str).unwrap_or("");
    let path = body.get("path").and_then(Value::as_str).map(str::trim).unwrap_or("");
    let enabled = body.get("enabled").and_then(Value::as_bool).unwrap_or(false);
    if path.is_empty() {
        return Err(ServiceError::new(400, "INVALID_ARGUMENT", "path is required"));
    }
    update_settings(settings_file, |settings| match kind {
        "project" => {
            if enabled && !Path::new(path).is_dir() {
                return Err(format!("{path} is not a directory"));
            }
            settings.set_project_consent(path, enabled);
            if enabled {
                settings.sources.project.enabled = true;
            }
            Ok(())
        }
        "extra" => settings.set_extra_consent(path, enabled),
        other => Err(format!("kind must be project or extra, not {other:?}")),
    })?;
    Ok(json!({"ok": true, "kind": kind, "path": canonical_project(path), "consent": enabled}))
}

/// `POST /agents/definitions/source` `{name, enabled}`: SRC-5 switch for user / project / extra.
pub fn set_source_at(settings_file: &Path, body: &Value) -> Result<Value, ServiceError> {
    let name = body.get("name").and_then(Value::as_str).unwrap_or("");
    let Some(enabled) = body.get("enabled").and_then(Value::as_bool) else {
        return Err(ServiceError::new(400, "INVALID_ARGUMENT", "enabled is required"));
    };
    update_settings(settings_file, |settings| settings.set_enabled(name, enabled))?;
    Ok(json!({"ok": true, "name": name, "enabled": enabled}))
}

/// `POST /agents/definitions/extra` `{action: "add"|"remove", path}`. A new directory starts
/// without consent (SRC-4) and loads nothing until the user consents.
pub fn extra_dir_at(settings_file: &Path, body: &Value) -> Result<Value, ServiceError> {
    let action = body.get("action").and_then(Value::as_str).unwrap_or("");
    let path = body.get("path").and_then(Value::as_str).map(str::trim).unwrap_or("");
    update_settings(settings_file, |settings| match action {
        "add" => settings.add_extra(path),
        "remove" => {
            if settings.remove_extra(path) {
                Ok(())
            } else {
                Err(format!("{path} is not in the extra directory list"))
            }
        }
        other => Err(format!("action must be add or remove, not {other:?}")),
    })?;
    Ok(json!({"ok": true, "action": action, "path": path}))
}

/// Consent banner data: the definition files that sit in the project's `.puppet-master/agents`
/// (names only, nothing is read or parsed) and whether this path is consented.
pub fn project_scan_at(settings_file: &Path, project: &str) -> Value {
    let settings = SourceSettings::load(settings_file);
    let files = project_candidates(Path::new(project));
    json!({
        "project": canonical_project(project),
        "files": files,
        "consented": settings.project_consented(project),
        "enabled": settings.sources.project.enabled,
    })
}

// ---- process-wide wrappers ----------------------------------------------------------------

/// Run `f` with the real app inputs (settings file, app data agents dir, approvals file, code ids).
pub fn with_global<R>(f: impl FnOnce(&LoadInputs<'_>, &Path) -> R) -> R {
    let owned = OwnedInputs::current();
    f(&owned.inputs(), &super::approvals::default_path())
}

pub fn listing_global() -> Value {
    with_global(|inputs, approvals| listing_with(inputs, approvals, &SourceSettings::load(&settings_path())))
}

pub fn reload_global_with_diff() -> Value {
    with_global(|inputs, approvals| reload_with_diff(inputs, approvals, &SourceSettings::load(&settings_path())))
}

pub fn validate_global(text: &str, source: &str) -> Result<Value, ServiceError> {
    validate_global_at(text, source, Path::new("."))
}

/// `validate_global` for the definition file in folder `here` (never searched for the program).
pub fn validate_global_at(text: &str, source: &str, here: &Path) -> Result<Value, ServiceError> {
    let code_ids = crate::tui::registry::registry().code_ids();
    validate_text_in(text, source, &code_ids, &path_dirs(here))
}

/// Reload the registry from the current settings and announce what changed.
fn reload_and_announce() {
    let before = super::loader::last_entries();
    let built = with_global(|inputs, approvals| reload_global(inputs, approvals));
    super::watch::announce(&super::watch::diff_entries(&before, &built.entries));
}

/// Approve, then reload the registry so the agent is spawnable at once.
pub fn approve_global(file: &str, fingerprint: &str) -> Result<Value, ServiceError> {
    let result = with_global(|inputs, approvals| approve(inputs, approvals, file, fingerprint))?;
    reload_and_announce();
    Ok(result)
}

pub fn revoke_global(id: &str) -> Result<Value, ServiceError> {
    let result = with_global(|_, approvals| revoke(approvals, id))?;
    reload_and_announce();
    Ok(result)
}

/// After a settings change: reload the registry, re-aim the watcher, announce.
fn settings_changed() {
    reload_and_announce();
    if let Err(error) = super::watch::rewatch() {
        tracing::warn!(%error, "agent definitions watcher could not be re-aimed");
    }
}

pub fn consent(body: &Value) -> Result<Value, ServiceError> {
    let result = consent_at(&settings_path(), body)?;
    settings_changed();
    Ok(result)
}

pub fn set_source(body: &Value) -> Result<Value, ServiceError> {
    let result = set_source_at(&settings_path(), body)?;
    settings_changed();
    Ok(result)
}

pub fn extra_dir(body: &Value) -> Result<Value, ServiceError> {
    let result = extra_dir_at(&settings_path(), body)?;
    settings_changed();
    Ok(result)
}

pub fn project_scan(project: &str) -> Value {
    project_scan_at(&settings_path(), project)
}

/// What was approved before for `file` (the argv diff in the Approve dialog).
pub fn approved_snapshot_global(file: &str) -> Value {
    approved_snapshot(&super::approvals::default_path(), file)
}

// ---- remove / restore (Settings > Agents) ---------------------------------------------------

/// Move one of the user's own definitions (and its fixtures, wizard record and approval) to
/// `.removed/`, then reload the registry so the agent leaves every menu at once.
pub fn remove_global(file: &str) -> Result<Value, ServiceError> {
    let ids = crate::tui::registry::registry().code_ids();
    let result = super::removal::remove_at(
        &crate::app_paths::agents_dir(),
        &super::approvals::default_path(),
        file,
        &ids,
        &now_rfc3339(),
    )?;
    reload_and_announce();
    Ok(result)
}

/// Undo a removal (see `removal::restore_at`), then reload the registry.
pub fn restore_global(removed_id: &str) -> Result<Value, ServiceError> {
    let ids = crate::tui::registry::registry().code_ids();
    let result = super::removal::restore_at(
        &crate::app_paths::agents_dir(),
        &super::approvals::default_path(),
        removed_id,
        &ids,
    )?;
    reload_and_announce();
    Ok(result)
}

/// `{removed: [...], total}`: what `.removed/` holds.
pub fn removed_global() -> Value {
    super::removal::list_removed_at(&crate::app_paths::agents_dir())
}

/// "Delete for good" for one removed agent (see `removal::purge_at`). The registry does not change:
/// `.removed/` is never loaded, so nothing needs a reload.
pub fn purge_global(removed_id: &str) -> Result<Value, ServiceError> {
    super::removal::purge_at(&crate::app_paths::agents_dir(), removed_id)
}

/// "Clear all" for `.removed/` (see `removal::purge_all_at`).
pub fn purge_all_global() -> Result<Value, ServiceError> {
    super::removal::purge_all_at(&crate::app_paths::agents_dir())
}
