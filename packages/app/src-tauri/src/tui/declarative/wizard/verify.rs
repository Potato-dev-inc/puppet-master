//! Verify (SPEC 6): check a draft and say what is wrong in words an AI can act on. Order: size,
//! the definition validator, the wizard's own rules, the shared conformance rules, then fixtures
//! (the generated garbage case, and a free replay of any earlier live-test outputs). Every stage that
//! can run does, so one round trip reports every problem.

use serde_json::{json, Value};

use crate::tui::conformance_rules as rules;
use crate::tui::declarative::check::check_dir;
use crate::tui::declarative::resolve::{resolve_in, Platform, ResolveError};
use crate::tui::declarative::schema::{parse_definition, DefError, Definition, Kind};
use crate::tui::declarative::service::approval_warnings;
use crate::tui::declarative::template;
use crate::tui::declarative::validate::{validate, Source, ValidationContext, MAX_DEFINITION_BYTES};
use crate::tui::declarative::DeclarativeProfile;
use crate::tui::types::HeadlessLaunch;

use super::fixtures;
use super::report::{render, CheckError, Source as ReportSource, WizardError};
use super::session::{sha256_hex, Draft, Session, State, Wizard, WizardEnv};

/// What `check_draft` found. `profile` is present when the draft built.
pub struct Outcome {
    pub errors: Vec<CheckError>,
    pub warnings: Vec<String>,
    pub rendered: Option<Value>,
    pub fixtures_checked: usize,
    pub samples_replayed: usize,
    pub profile: Option<DeclarativeProfile>,
    pub definition: Option<Definition>,
}

impl Outcome {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// May the live test run for this draft? Only a draft that is not valid at all (the file does not
    /// parse or validate, or breaks a wizard rule such as the program name) is kept from it; a draft
    /// that fails only the shared conformance rules or the fixture checks is tested anyway, so the
    /// drafter gets the program's real output together with those problems.
    pub fn can_livetest(&self) -> bool {
        self.definition.is_some()
            && !self.errors.iter().any(|e| matches!(e.stage, "size" | "validate" | "wizard"))
    }
}

fn def_error(stage: &'static str, e: &DefError) -> CheckError {
    CheckError::new(stage, e.code, e.path.clone(), e.message.clone()).at(e.line, e.col)
}

/// `kiro-cli.exe` and `Kiro-CLI` both compare as `kiro-cli`.
fn bare(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    match lower.rsplit_once('.') {
        Some((stem, ext)) if ["exe", "cmd", "bat", "ps1", "com"].contains(&ext) => stem.to_string(),
        _ => lower,
    }
}

fn wizard_rules(def: &Definition, session: &Session, env: &WizardEnv, errors: &mut Vec<CheckError>) {
    if def.agent.kind == Kind::Generic {
        errors.push(CheckError::new(
            "wizard",
            "WIZARD_KIND",
            "agent.kind",
            "kind = \"generic\" is the visible-pane path without a headless table",
        ));
    }
    if def.headless.is_none() {
        errors.push(CheckError::new(
            "wizard",
            "WIZARD_NO_HEADLESS",
            "headless",
            "the file has no [headless] table; this wizard writes headless definitions",
        ));
    }
    let id = &def.agent.id;
    let taken = [
        env.agents_dir.join(format!("{id}.toml")),
        env.agents_dir.join(format!("{id}.wizard.json")),
        env.agents_dir.join(id),
    ]
    .into_iter()
    .find(|p| p.exists());
    if let Some(path) = taken {
        let what = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        errors.push(CheckError::new(
            "wizard",
            "WIZARD_ID_TAKEN",
            "agent.id",
            format!("the id `{id}` is already used: `{what}` exists in the agents folder (the wizard never overwrites)"),
        ));
    }
    // The program that was tested is the one the person typed; the definition must start with it, so
    // the approved definition runs the same program (names are tried in order).
    let typed = bare(&session.command);
    if def.binary.names.first().map(|n| bare(n)) != Some(typed.clone()) {
        errors.push(CheckError::new(
            "wizard",
            "WIZARD_BINARY_MISMATCH",
            "binary.names",
            format!("binary.names is {:?} but the first name must be the command the person typed, `{}`", def.binary.names, session.command),
        ));
    }
    match resolve_in(&def.binary, &session.dirs, Platform::current()) {
        Ok(_) => {}
        Err(e) => {
            let mut message = format!("the program was not found with this [binary] table: {e}");
            if matches!(e, ResolveError::ShimNeedsNative { .. }) {
                if session.native_candidates.is_empty() {
                    message.push_str(". No executable was found beside the wrapper");
                } else {
                    message.push_str(&format!(
                        ". Executables found beside the wrapper (relative to its folder): {}",
                        session.native_candidates.join(", ")
                    ));
                }
            }
            errors.push(CheckError::new("wizard", "WIZARD_BINARY_UNRESOLVED", "binary", message));
        }
    }
}

/// Does the draft text contain one of the session's live-test words, or the bare word `pong`? (A result
/// regex built around the test answer would pass the live test and be useless.)
pub fn leaks_test_words(text: &str, words: &super::session::TestWords) -> bool {
    use once_cell::sync::Lazy;
    static PONG: Lazy<regex::Regex> = Lazy::new(|| regex::Regex::new(r"(?i)\bpong(?:\d*\b|-)").expect("pong regex"));
    let lower = text.to_ascii_lowercase();
    words.all().iter().any(|w| lower.contains(w)) || PONG.is_match(text)
}

fn rendered(def: &Definition) -> Option<Value> {
    def.headless.as_ref()?;
    let launch = |resume: Option<&'static str>, model: Option<&'static str>| HeadlessLaunch {
        project: "<project>",
        task: "<task>",
        read_only: false,
        resume_session: resume,
        trust_workspace: false,
        allow_shell: false,
        model,
    };
    Some(json!({
        "headless_argv": template::render_headless(def, &launch(None, None)),
        "resume_argv": template::render_headless(def, &launch(Some("<session>"), Some("<model>"))),
    }))
}

/// Run every check on `text` for the session `session`.
pub fn check_draft(session: &Session, env: &WizardEnv, text: &str) -> Outcome {
    let mut out = Outcome {
        errors: Vec::new(),
        warnings: Vec::new(),
        rendered: None,
        fixtures_checked: 0,
        samples_replayed: 0,
        profile: None,
        definition: None,
    };
    if text.len() > MAX_DEFINITION_BYTES {
        out.errors.push(CheckError::new(
            "size",
            "AGENT_DEF_TOO_LARGE",
            "",
            format!("the file is {} bytes; the limit is {MAX_DEFINITION_BYTES}", text.len()),
        ));
        return out;
    }
    let def = match parse_definition(text) {
        Ok(def) => def,
        Err(e) => {
            out.errors.push(def_error("validate", &e));
            return out;
        }
    };
    let code_ids = env.code_id_refs();
    let ctx = ValidationContext { source: Source::User, code_ids: &code_ids, raw_len: text.len() };
    let validation = validate(&def, &ctx);
    out.errors.extend(validation.iter().map(|e| def_error("validate", e)));
    wizard_rules(&def, session, env, &mut out.errors);
    if leaks_test_words(text, &session.words) {
        out.errors.push(CheckError::new(
            "wizard",
            "WIZARD_TEST_WORD",
            "",
            "the file contains a word the live test uses as its expected answer, or the bare word `pong`. A definition must read any answer, not one the test asks for",
        ));
    }
    out.warnings = approval_warnings(&def);
    if !def.headless.as_ref().is_some_and(|h| h.optional.iter().any(|s| s.when == "resume")) {
        out.warnings.push("The draft has no resume segment, so follow-up turns will start a new conversation.".into());
    }
    out.rendered = rendered(&def);
    if !validation.is_empty() || def.headless.is_none() {
        out.definition = Some(def);
        return out;
    }
    let profile = match DeclarativeProfile::new(def.clone()) {
        Ok(p) => p,
        Err(e) => {
            out.errors.push(def_error("validate", &e));
            out.definition = Some(def);
            return out;
        }
    };
    for (name, rule) in rules::ALL {
        if let Err(message) = rule(&profile) {
            out.errors.push(CheckError::new("conformance", format!("CONFORMANCE_{}", name.to_ascii_uppercase()), "", message));
        }
    }
    check_fixtures(session, &profile, &mut out);
    out.definition = Some(def);
    out.profile = Some(profile);
    out
}

fn check_fixtures(session: &Session, profile: &DeclarativeProfile, out: &mut Outcome) {
    let dir = session.work_dir.join("preview");
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = fixtures::write_all(&dir, &fixtures::garbage_files()) {
        out.errors.push(CheckError::new("fixture", "FIXTURE_FAILED", "", e));
        return;
    }
    let results = check_dir(profile, &dir);
    out.fixtures_checked = results.len();
    for r in results.iter().filter(|r| !r.passed) {
        out.errors.push(CheckError::new(
            "fixture",
            "FIXTURE_FAILED",
            r.fixture.clone(),
            r.failures.join("; "),
        ));
    }
    let _ = std::fs::remove_dir_all(&dir);
    let has_session = profile
        .definition()
        .headless
        .as_ref()
        .and_then(|h| h.output.as_ref())
        .is_some_and(|o| o.session.is_some());
    let project = session.work_dir.to_string_lossy().into_owned();
    for sample in &session.samples {
        // An output made by a different command says nothing about this draft's selectors.
        let args = super::livetest::stage_args(profile.definition(), sample.stage, &project, sample.resume_session.as_deref(), &session.words);
        if args != sample.args {
            continue;
        }
        let judged = super::livetest::judge(profile, sample.stage, &sample.want, &sample.stdout, sample.exit, None, has_session);
        let mask = |t: &str| super::livetest::mask_words(t, &session.words);
        out.samples_replayed += 1;
        for problem in judged.problems {
            out.errors.push(
                CheckError::new("fixture", "FIXTURE_FAILED", format!("{}.{}", sample.stage.fixture_stem(), "sample"), mask(&problem))
                    .with_excerpt(&mask(&sample.stdout)),
            );
        }
    }
}

/// JSON of an outcome for the route (SPEC 4.3).
pub fn outcome_json(outcome: &Outcome, state: State, sha: Option<&str>, rules_run: usize, planned: Option<Value>) -> Value {
    let mut body = json!({
        "ok": outcome.ok(),
        "state": state.as_str(),
        "errors": outcome.errors,
        "text": render(&outcome.errors, ReportSource::Verify),
        "warnings": outcome.warnings,
        "checked": {"fixtures": outcome.fixtures_checked, "rules": rules_run},
        "samples_replayed": outcome.samples_replayed,
    });
    if let Some(rendered) = &outcome.rendered {
        body["rendered"] = rendered.clone();
    }
    if let Some(sha) = sha {
        body["sha256"] = json!(sha);
    }
    if let Some(planned) = planned {
        // present for a draft the live test may run, also when other checks failed
        body["planned"] = planned;
        body["can_livetest"] = json!(true);
    }
    body
}

/// `POST /agents/wizard/verify`.
pub fn run(wizard: &Wizard, body: &Value) -> Result<Value, WizardError> {
    let id = super::api::session_id(body)?;
    let text = body
        .get("toml")
        .and_then(Value::as_str)
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| WizardError::bad_request("toml is required"))?
        .to_string();
    if text.len() > MAX_DEFINITION_BYTES {
        return Err(WizardError::new(
            400,
            "AGENT_DEF_TOO_LARGE",
            format!("the draft is {} bytes; the limit is {MAX_DEFINITION_BYTES}", text.len()),
        ));
    }
    let snap = wizard.begin(&id, "verify", &[State::Probed, State::Verified, State::Tested])?;
    let env = wizard.env();
    let outcome = check_draft(&snap, &env, &text);
    let ok = outcome.ok();
    let sha = ok.then(|| sha256_hex(text.as_bytes()));
    // The commands the live test would run for this draft; the caller shows them and sends the
    // fingerprint back with the live-test request.
    let planned_run = if outcome.can_livetest() {
        outcome.definition.as_ref().and_then(|def| super::livetest::plan(&snap, def).ok())
    } else {
        None
    };
    let testable = ok || planned_run.is_some();
    let state = wizard.end(&id, |s| {
        if testable {
            s.draft = Some(Draft { text: text.clone(), sha256: sha256_hex(text.as_bytes()) });
            s.tested = None;
            // a draft that failed some checks is not a verified one
            s.state = if ok { State::Verified } else { State::Probed };
        }
        s.state
    })?;
    let planned = planned_run.as_ref().map(super::livetest::plan_json);
    Ok(outcome_json(&outcome, state, sha.as_deref(), rules::ALL.len(), planned))
}

/// Used by tests and the benchmark: the folder `check_draft` would write previews to.
pub fn preview_dir(session: &Session) -> std::path::PathBuf {
    session.work_dir.join("preview")
}
