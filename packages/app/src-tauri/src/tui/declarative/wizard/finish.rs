//! Finish (SPEC 7): write the tested draft as an UNAPPROVED definition, its fixtures folder (named
//! after the file, where `check::resolve_fixtures_dir` looks) and `<id>.wizard.json` with provenance.
//! Never overwrites anything: every file is created with create-new, the TOML last (the watcher loads
//! it the moment it appears, with its fixtures already in place). Anything created by a failed call
//! is removed again.

use std::path::Path;

use serde_json::{json, Value};

use crate::tui::declarative::approvals::now_rfc3339;
use crate::tui::declarative::check::check_dir;
use crate::tui::declarative::schema::{parse_definition, OutputFormat};
use crate::tui::declarative::DeclarativeProfile;

use super::fixtures::{self, FixtureFile};
use super::visible::fixtures as visible_fixtures;
use super::report::WizardError;
use super::session::{Session, State, Wizard};

/// Who wrote the draft (SPEC 4.5). Short plain strings only; nothing here ever holds a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub drafter: String,
    pub drafter_kind: String,
    pub model: Option<String>,
    pub prompt_version: String,
    pub rounds: u32,
}

fn short_text(body: &Value, key: &str) -> Result<Option<String>, WizardError> {
    let Some(raw) = body.get(key) else { return Ok(None) };
    if raw.is_null() {
        return Ok(None);
    }
    let text = raw
        .as_str()
        .ok_or_else(|| WizardError::bad_request(format!("provenance.{key} must be text")))?
        .trim();
    let ok = text.chars().count() <= 80
        && text.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | ':' | '/' | '(' | ')' | '+' | '-'));
    if !ok {
        return Err(WizardError::bad_request(format!(
            "provenance.{key} may hold at most 80 characters of letters, digits and . _ : / ( ) + -"
        )));
    }
    Ok(Some(text.to_string()).filter(|t| !t.is_empty()))
}

pub fn parse_provenance(body: &Value) -> Result<Provenance, WizardError> {
    let p = body.get("provenance").filter(|p| p.is_object()).ok_or_else(|| WizardError::bad_request("provenance is required"))?;
    let drafter = short_text(p, "drafter")?.ok_or_else(|| WizardError::bad_request("provenance.drafter is required"))?;
    let kind = short_text(p, "drafter_kind")?.unwrap_or_else(|| "other".into());
    if !["local_agent", "openrouter", "other"].contains(&kind.as_str()) {
        return Err(WizardError::bad_request("provenance.drafter_kind must be local_agent, openrouter or other"));
    }
    let prompt_version =
        short_text(p, "prompt_version")?.ok_or_else(|| WizardError::bad_request("provenance.prompt_version is required"))?;
    let rounds = p.get("rounds").and_then(Value::as_u64).filter(|r| (1..=50).contains(r)).ok_or_else(|| {
        WizardError::bad_request("provenance.rounds must be a number from 1 to 50")
    })? as u32;
    Ok(Provenance { drafter, drafter_kind: kind, model: short_text(p, "model")?, prompt_version, rounds })
}

/// File extension of the live-test captures for this draft.
pub fn draft_extension(text: &str) -> &'static str {
    match parse_definition(text).ok().and_then(|d| d.headless).and_then(|h| h.output).map(|o| o.format) {
        Some(format) => fixtures::extension(format),
        None => "jsonl",
    }
}

fn provenance_json(session: &Session, id: &str, p: &Provenance, sha: &str, passed: usize, total: usize, build: &str) -> Value {
    let tested = session.tested.as_ref();
    let visible = session.visible_tested.as_ref();
    let capture = session.visible.captures.lock().clone();
    let mut body = json!({
        "wizard_schema": 1,
        "id": id,
        "command": session.command,
        "created_at": now_rfc3339(),
        "drafter": {"name": p.drafter, "kind": p.drafter_kind, "model": p.model},
        "prompt_version": p.prompt_version,
        "rounds": p.rounds,
        "tested_version": tested.and_then(|t| t.version.clone()).or_else(|| visible.and_then(|v| v.version.clone())),
        "definition_sha256": sha,
        "results": {
            "stages": tested.map(|t| t.stages.iter().map(|s| json!({"name": s.name, "status": s.status})).collect::<Vec<_>>()).unwrap_or_default(),
            "fixtures": {"passed": passed, "total": total},
        },
        "app_build": build,
    });
    if let Some(v) = visible {
        body["results"]["visible"] = json!({"passed": v.passed, "total": v.total});
    }
    // The questions the wizard answered for the person in its own panes (SPEC 11.16), masked like every screen.
    let mask = |t: &str| super::livetest::mask_words(t, &session.words);
    let mut answered: Vec<Value> = capture.as_ref().map(|set| set.answered.iter().map(|a| a.to_json(&mask)).collect()).unwrap_or_default();
    answered.extend(session.visible.test_answered.lock().iter().map(|a| a.to_json(&mask)));
    body["auto_answer"] = json!({"enabled": session.auto_answer, "answered": answered});
    if let Some(set) = capture.filter(|set| !set.frames.is_empty()) {
        body["capture"] = json!({"grid": {"cols": set.grid.0, "rows": set.grid.1}, "partial": set.partial, "exit_method": set.exit_method, "writes_without_asking": set.writes_without_asking});
    }
    body
}

/// Removes what `finish` created if it does not reach `commit`.
struct Created<'a> {
    files: Vec<&'a Path>,
    dir: Option<&'a Path>,
    armed: bool,
}

impl Drop for Created<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        for f in self.files.iter().rev() {
            let _ = std::fs::remove_file(f);
        }
        if let Some(dir) = self.dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

fn create_new(path: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    std::io::Write::write_all(&mut f, bytes)
}

/// Write everything for a tested session. Returns the response body (without a registry listing).
pub fn write_session(session: &Session, agents_dir: &Path, provenance: &Provenance, build: &str) -> Result<Value, WizardError> {
    let draft = session.draft.as_ref().ok_or_else(|| WizardError::new(409, "WIZARD_BAD_STATE", "there is no verified draft"))?;
    if !super::session::tests_complete(session) {
        return Err(WizardError::new(
            409,
            "WIZARD_BAD_STATE",
            "the draft was not tested; run the live test (one-task mode) and the visible test (visible pane) that apply to it first",
        ));
    }
    let def = parse_definition(&draft.text).map_err(|e| WizardError::new(409, "AGENT_DEF_INVALID", e.to_string()))?;
    let id = def.agent.id.clone();
    let has_headless = def.headless.as_ref().is_some_and(|h| h.output.is_some());
    let format = def.headless.as_ref().and_then(|h| h.output.as_ref()).map_or(OutputFormat::JsonLines, |o| o.format);
    let profile = DeclarativeProfile::new(def).map_err(|e| WizardError::new(409, "AGENT_DEF_INVALID", e.to_string()))?;
    let mut files: Vec<FixtureFile> = if has_headless { fixtures::final_files(&profile, format, &session.samples) } else { Vec::new() };
    let captured = session.visible.captures.lock().clone().filter(|set| !set.frames.is_empty());
    if let Some(set) = &captured {
        let head = visible_fixtures::header(&session.command, session.version.number.as_deref(), set.grid);
        let (screens, problems) = visible_fixtures::files(set, &profile, &session.words, &draft.expects, &head);
        if !problems.is_empty() {
            let list: Vec<String> = problems.iter().map(|p| format!("{}: {}", p.name, p.message)).collect();
            return Err(WizardError::new(
                409,
                "WIZARD_FIXTURES_FAILED",
                format!("the screen fixtures are not valid, so nothing was written: {}", list.join(" | ")),
            ));
        }
        files.extend(screens);
    }

    // Stage and check first: nothing reaches the agents folder unless every fixture passes.
    let staging = session.work_dir.join("finish").join(&id);
    let _ = std::fs::remove_dir_all(&staging);
    fixtures::write_all(&staging, &files).map_err(|e| WizardError::new(500, "WIZARD_IO", e))?;
    let results = check_dir(&profile, &staging);
    let failed: Vec<String> = results
        .iter()
        .filter(|r| !r.passed)
        .map(|r| format!("{}: {}", r.fixture, r.failures.join("; ")))
        .collect();
    if !failed.is_empty() {
        return Err(WizardError::new(
            409,
            "WIZARD_FIXTURES_FAILED",
            format!("the fixtures do not pass, so nothing was written: {}", failed.join(" | ")),
        ));
    }
    let (passed, total) = (results.iter().filter(|r| r.passed).count(), results.len());

    let toml_path = agents_dir.join(format!("{id}.toml"));
    let fixtures_dir = agents_dir.join(&id);
    let provenance_path = agents_dir.join(format!("{id}.wizard.json"));
    if toml_path.exists() {
        return Err(WizardError::new(
            409,
            "AGENT_DEF_DUPLICATE_ID",
            format!("{} already exists; the wizard never overwrites a definition", toml_path.display()),
        ));
    }
    if fixtures_dir.exists() {
        return Err(WizardError::new(409, "WIZARD_FIXTURES_DIR_EXISTS", format!("{} already exists", fixtures_dir.display())));
    }
    if provenance_path.exists() {
        return Err(WizardError::new(409, "WIZARD_PROVENANCE_EXISTS", format!("{} already exists", provenance_path.display())));
    }
    std::fs::create_dir_all(agents_dir)
        .map_err(|e| WizardError::new(500, "WIZARD_IO", format!("cannot create the agents folder: {e}")))?;

    let io = |what: &str, e: std::io::Error| WizardError::new(500, "WIZARD_IO", format!("cannot write {what}: {e}"));
    let mut created = Created { files: Vec::new(), dir: None, armed: true };
    std::fs::create_dir(&fixtures_dir).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => {
            WizardError::new(409, "WIZARD_FIXTURES_DIR_EXISTS", format!("{} already exists", fixtures_dir.display()))
        }
        _ => io("the fixtures folder", e),
    })?;
    created.dir = Some(&fixtures_dir);
    fixtures::write_all(&fixtures_dir, &files).map_err(|e| WizardError::new(500, "WIZARD_IO", e))?;

    let prov = provenance_json(session, &id, provenance, &draft.sha256, passed, total, build);
    let prov_text = serde_json::to_string_pretty(&prov).unwrap_or_default();
    create_new(&provenance_path, prov_text.as_bytes()).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => {
            WizardError::new(409, "WIZARD_PROVENANCE_EXISTS", format!("{} already exists", provenance_path.display()))
        }
        _ => io("the provenance file", e),
    })?;
    created.files.push(&provenance_path);
    create_new(&toml_path, draft.text.as_bytes()).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => WizardError::new(
            409,
            "AGENT_DEF_DUPLICATE_ID",
            format!("{} already exists; the wizard never overwrites a definition", toml_path.display()),
        ),
        _ => io("the definition", e),
    })?;
    created.armed = false;
    Ok(json!({
        "ok": true,
        "id": id,
        "file": toml_path.display().to_string(),
        "fixtures_dir": fixtures_dir.display().to_string(),
        "provenance_file": provenance_path.display().to_string(),
        "state": "needs_approval",
        "fixtures": {"passed": passed, "total": total},
    }))
}

/// `POST /agents/wizard/finish`. The caller reloads the registry afterwards.
pub fn run(wizard: &Wizard, body: &Value) -> Result<Value, WizardError> {
    let id = super::api::session_id(body)?;
    let provenance = parse_provenance(body)?;
    let snap = wizard.begin(&id, "finish", &[State::Tested])?;
    let env = wizard.env();
    match write_session(&snap, &env.agents_dir, &provenance, &env.app_build) {
        Ok(body) => {
            wizard.release(&id);
            wizard.drop_finished(&id);
            Ok(body)
        }
        Err(e) => {
            wizard.release(&id);
            Err(e)
        }
    }
}
