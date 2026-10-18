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
use super::session::{tests_complete, Stage};
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

/// What the front end tells `finish` about an autonomous run ("Finish without me", SPEC 11.19): the decisions it took
/// (plain short sentences, for the record) and whether it wants the approval. Ignored unless the session is autonomous.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutoFinish {
    pub decisions: Vec<String>,
    pub approve: bool,
}

const MAX_DECISIONS: usize = 200;
const MAX_DECISION_CHARS: usize = 240;

pub fn parse_auto(body: &Value) -> Result<AutoFinish, WizardError> {
    let Some(a) = body.get("autonomous").filter(|a| a.is_object()) else { return Ok(AutoFinish::default()) };
    let mut decisions = Vec::new();
    if let Some(list) = a.get("decisions").filter(|d| !d.is_null()) {
        let list = list.as_array().ok_or_else(|| WizardError::bad_request("autonomous.decisions must be a list of text"))?;
        if list.len() > MAX_DECISIONS {
            return Err(WizardError::bad_request(format!("at most {MAX_DECISIONS} decisions")));
        }
        for item in list {
            let text = item.as_str().ok_or_else(|| WizardError::bad_request("every decision must be text"))?;
            decisions.push(super::exec::cut_chars(&super::exec::clean(text), MAX_DECISION_CHARS));
        }
    }
    Ok(AutoFinish { decisions, approve: a.get("approve").and_then(Value::as_bool).unwrap_or(false) })
}

fn stage_label(name: &str) -> String {
    [Stage::Pong, Stage::Sentence, Stage::Resume, Stage::BadModel]
        .into_iter()
        .find(|s| s.as_str() == name)
        .map_or_else(|| name.to_string(), |s| format!("One-task mode: {}", s.label().to_lowercase()))
}

/// Every check that passed for this session, in plain words, or why the wizard may NOT approve by itself. The
/// wizard approves only when all of these hold: it was asked to finish without the person, the checker passed
/// this exact draft with no problem, every test that applies to it passed for exactly this text, and every saved
/// sample and screen passes. One failure anywhere is a no.
pub fn passed_checks(s: &Session, fixtures: (usize, usize)) -> Result<Vec<String>, String> {
    if !s.autonomous {
        return Err("this session was not started with 'Finish without me'".into());
    }
    let draft = s.draft.as_ref().ok_or("there is no draft")?;
    if s.verified_sha256.as_deref() != Some(draft.sha256.as_str()) {
        return Err("the checker still reports a problem with this draft".into());
    }
    if !tests_complete(s) {
        return Err("a test that applies to this draft has not passed for exactly this text".into());
    }
    if fixtures.1 == 0 || fixtures.0 != fixtures.1 {
        return Err(format!("only {} of {} saved samples and screens pass", fixtures.0, fixtures.1));
    }
    let mut checks = vec![
        "The definition is valid".to_string(),
        "The wizard's own rules hold".to_string(),
        "The shared conformance rules hold".to_string(),
        format!("All {} saved samples and screens pass", fixtures.1),
    ];
    if s.visible.has_captures() {
        let v = s.visible_tested.as_ref().filter(|v| v.draft_sha256 == draft.sha256).ok_or("the visible test has not passed")?;
        if v.total == 0 || v.passed != v.total {
            return Err(format!("only {} of {} visible checks passed", v.passed, v.total));
        }
        checks.extend(v.passed_labels.iter().map(|l| format!("Visible pane: {l}")));
    }
    let needs_headless = parse_definition(&draft.text).ok().and_then(|d| d.headless).is_some_and(|h| h.output.is_some());
    if needs_headless {
        let t = s.tested.as_ref().filter(|t| t.draft_sha256 == draft.sha256).ok_or("the one-task test has not passed")?;
        if t.stages.iter().any(|st| st.status == "failed") {
            return Err("a stage of the one-task test failed".into());
        }
        checks.extend(t.stages.iter().filter(|st| st.status == "passed").map(|st| stage_label(st.name)));
    }
    Ok(checks)
}

/// The wizard approves the definition it just wrote, through the normal approval path, when every check passed.
/// Returns the `approval` object of the `finish` reply (and of `<id>.wizard.json`).
fn auto_approve(session: &Session, env: &super::session::WizardEnv, written: &Value, requested: bool) -> Value {
    if !requested {
        return json!({"approved": false, "reason": "approval was not asked for"});
    }
    let fixtures = (
        written["fixtures"]["passed"].as_u64().unwrap_or(0) as usize,
        written["fixtures"]["total"].as_u64().unwrap_or(0) as usize,
    );
    let checks = match passed_checks(session, fixtures) {
        Ok(c) => c,
        Err(reason) => return json!({"approved": false, "reason": reason}),
    };
    let file = written["file"].as_str().unwrap_or("");
    let code_ids = env.code_id_refs();
    let inputs = crate::tui::declarative::loader::LoadInputs {
        bundled: &[],
        user_dir: Some(env.agents_dir.as_path()),
        code_ids: &code_ids,
        projects: &[],
        extra: &[],
    };
    match crate::tui::declarative::service::approve_current_as_wizard(&inputs, &env.approvals_path, file, &checks, env.path_dirs.as_deref()) {
        Ok(_) => json!({"approved": true, "approved_by": crate::tui::declarative::approvals::APPROVED_BY_WIZARD, "count": checks.len(), "checks": checks, "at": now_rfc3339()}),
        Err(e) => json!({"approved": false, "reason": e.message}),
    }
}

fn provenance_json(session: &Session, id: &str, p: &Provenance, sha: &str, passed: usize, total: usize, build: &str, auto: &AutoFinish) -> Value {
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
    if session.autonomous {
        // "Finish without me": what ran without anyone reading it first, and what the wizard decided (SPEC 11.19).
        body["autonomous"] = json!({
            "enabled": true,
            "decisions": auto.decisions,
            "ai_looks": session.visible.auto.verdicts.lock().clone(),
            "planned": session.visible.auto.plans.lock().clone(),
            "approval": {"approved": null},
        });
    }
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
    write_session_with(session, agents_dir, provenance, build, &AutoFinish::default())
}

/// `write_session` with what an autonomous run adds to `<id>.wizard.json`.
pub fn write_session_with(session: &Session, agents_dir: &Path, provenance: &Provenance, build: &str, auto: &AutoFinish) -> Result<Value, WizardError> {
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

    let prov = provenance_json(session, &id, provenance, &draft.sha256, passed, total, build, auto);
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

/// Put the approval's outcome into `<id>.wizard.json` (it was written before the approval was tried).
fn record_approval(written: &Value, approval: &Value) {
    let Some(path) = written["provenance_file"].as_str() else { return };
    let Ok(text) = std::fs::read_to_string(path) else { return };
    let Ok(mut doc) = serde_json::from_str::<Value>(&text) else { return };
    if doc.get("autonomous").is_none() {
        return;
    }
    doc["autonomous"]["approval"] = approval.clone();
    if let Ok(out) = serde_json::to_string_pretty(&doc) {
        let _ = std::fs::write(path, out);
    }
}

/// `POST /agents/wizard/finish`. The caller reloads the registry afterwards.
pub fn run(wizard: &Wizard, body: &Value) -> Result<Value, WizardError> {
    let id = super::api::session_id(body)?;
    let provenance = parse_provenance(body)?;
    let auto = parse_auto(body)?;
    let snap = wizard.begin(&id, "finish", &[State::Tested])?;
    let env = wizard.env();
    match write_session_with(&snap, &env.agents_dir, &provenance, &env.app_build, &auto) {
        Ok(mut body) => {
            if snap.autonomous {
                let approval = auto_approve(&snap, &env, &body, auto.approve);
                record_approval(&body, &approval);
                body["approval"] = approval;
            }
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
