//! Live test (SPEC 5): run the verified draft three times against the real program. "pong" must
//! produce `pong`, resuming with the session id must produce `pong2`, and a wrong model name must be
//! classified as an error. Stops at the first failed stage so no plan is wasted. The outputs are kept
//! as samples (replayed for free by the next `verify`) and, when all stages pass, become fixtures.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use serde::Serialize;
use serde_json::{json, Value};

use crate::tui::declarative::resolve::{resolve_in, Platform};
use crate::tui::declarative::schema::{parse_definition, Definition};
use crate::tui::declarative::template;
use crate::tui::declarative::DeclarativeProfile;
use crate::tui::types::HeadlessLaunch;
use crate::tui::TuiProfile;

use super::exec::{self, RunSpec};
use super::report::{render, CheckError, Source as ReportSource, WizardError};
use super::session::{Draft, Sample, Session, Stage, StageSummary, State, Tested, Wizard, WizardEnv};

pub const PONG_TASK: &str = "Reply with exactly the word: pong";
pub const PONG2_TASK: &str = "Reply with exactly the word: pong2";
pub const BAD_MODEL: &str = "pm-wizard-no-such-model/zzz";
/// How much of each stream goes back to the UI (the full capped output is kept in the session).
pub const REPLY_STREAM_CHARS: usize = 8 * 1024;

/// The task and model of a stage.
pub fn stage_inputs(stage: Stage) -> (&'static str, Option<&'static str>) {
    match stage {
        Stage::Pong => (PONG_TASK, None),
        Stage::Resume => (PONG2_TASK, None),
        Stage::BadModel => (PONG_TASK, Some(BAD_MODEL)),
    }
}

/// The argument list the draft renders for a stage (no program name).
pub fn stage_args(def: &Definition, stage: Stage, project: &str, resume: Option<&str>) -> Vec<String> {
    let (task, model) = stage_inputs(stage);
    let launch = HeadlessLaunch {
        project,
        task,
        read_only: false,
        resume_session: resume,
        trust_workspace: false,
        allow_shell: false,
        model,
    };
    template::render_headless(def, &launch).unwrap_or_default()
}

/// Does the draft support continuing a conversation? (A resume segment and a session selector.)
pub fn can_resume(def: &Definition) -> bool {
    let h = def.headless.as_ref();
    h.is_some_and(|h| h.optional.iter().any(|s| s.when == "resume"))
        && h.and_then(|h| h.output.as_ref()).is_some_and(|o| o.session.is_some())
}

pub fn has_model_option(def: &Definition) -> bool {
    def.headless.as_ref().is_some_and(|h| h.optional.iter().any(|s| s.when == "model"))
}

fn same_word(got: &str, want: &str) -> bool {
    let trimmed = got
        .trim()
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | '*' | '_'))
        .trim_end_matches(['.', '!', ' '])
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | '*' | '_'));
    trimmed.eq_ignore_ascii_case(want)
}

/// What the draft made of one stage's output.
#[derive(Debug, Clone)]
pub struct Judged {
    pub result: Option<String>,
    pub session: Option<String>,
    /// `(code, message)` from the draft's own failure classifier.
    pub classified: Option<(String, String)>,
    pub problems: Vec<String>,
}

impl Judged {
    pub fn passed(&self) -> bool {
        self.problems.is_empty()
    }
}

/// The stage rules of SPEC 5.1. `prior_session` is the session id stage 1 produced (for the resume
/// stage); `has_session` is whether the draft has a `session` selector.
pub fn judge(
    profile: &DeclarativeProfile,
    stage: Stage,
    stdout: &str,
    exit: Option<i32>,
    prior_session: Option<&str>,
    has_session: bool,
) -> Judged {
    let code = exit.unwrap_or(0);
    let result = profile.extract_result(stdout, code).filter(|r| !r.trim().is_empty());
    let session = profile.extract_provider_session(stdout);
    let classified = profile
        .classify_headless_failure(stdout, code)
        .map(|(c, m)| (c.to_string(), m));
    let mut problems = Vec::new();
    let said = |classified: &Option<(String, String)>| match classified {
        Some((c, m)) => format!(" The draft's own check says {c}: {m}"),
        None => String::new(),
    };
    match stage {
        Stage::Pong | Stage::Resume => {
            let want = if stage == Stage::Pong { "pong" } else { "pong2" };
            match &result {
                Some(r) if same_word(r, want) => {
                    if classified.as_ref().is_some_and(|(c, _)| c == "AGENT_REPORTED_ERROR") {
                        problems.push(format!(
                            "a result was found but the output also reports an error.{}",
                            said(&classified)
                        ));
                    }
                }
                Some(r) => problems.push(format!("the result read from the output was {r:?}, expected {want:?}.")),
                None => problems.push(format!("no result was found in the output; expected {want:?}.{}", said(&classified))),
            }
            if has_session && session.is_none() {
                problems.push("no session id was found (the `session` selector matched nothing), so a follow-up turn cannot resume.".into());
            }
            if stage == Stage::Resume {
                if let (Some(prior), Some(now)) = (prior_session, &session) {
                    if prior != now {
                        problems.push(format!("the resumed turn reported session {now:?} but {prior:?} was asked for; the resume flag does not continue the conversation."));
                    }
                }
            }
        }
        Stage::BadModel => {
            if let Some(r) = &result {
                problems.push(format!("an answer ({r:?}) was read although the model name was wrong; the wrong model must give an error and no result."));
            }
            let recognised = classified.as_ref().is_some_and(|(c, _)| c == "AGENT_REPORTED_ERROR") || code != 0;
            if !recognised {
                problems.push("the output was not recognised as an error and the program exited with 0. Add a `[headless.output.error]` selector for the error event or message.".into());
            }
        }
    }
    Judged { result, session, classified, problems }
}

/// What one stage did.
#[derive(Debug, Clone, Serialize)]
pub struct StageReport {
    pub name: &'static str,
    pub label: &'static str,
    /// `passed`, `failed` or `skipped`.
    pub status: &'static str,
    pub args: Vec<String>,
    pub exit: Option<i32>,
    pub duration_ms: u64,
    pub timed_out: bool,
    pub result: Option<String>,
    pub session: Option<String>,
    pub error_code: Option<String>,
    pub problems: Vec<String>,
    pub note: Option<String>,
    pub stdout: String,
    pub stderr: String,
}

impl StageReport {
    fn skipped(stage: Stage, note: &str) -> Self {
        Self {
            name: stage.as_str(),
            label: stage.label(),
            status: "skipped",
            args: Vec::new(),
            exit: None,
            duration_ms: 0,
            timed_out: false,
            result: None,
            session: None,
            error_code: None,
            problems: Vec::new(),
            note: Some(note.to_string()),
            stdout: String::new(),
            stderr: String::new(),
        }
    }
}

pub struct LiveOutcome {
    pub stages: Vec<StageReport>,
    pub errors: Vec<CheckError>,
    pub samples: Vec<Sample>,
}

impl LiveOutcome {
    pub fn ok(&self) -> bool {
        self.errors.is_empty() && self.stages.iter().all(|s| s.status != "failed")
    }
}

struct Runner<'a> {
    profile: &'a DeclarativeProfile,
    def: &'a Definition,
    program: PathBuf,
    session: &'a Session,
    env: &'a WizardEnv,
    cancel: &'a AtomicBool,
}

impl Runner<'_> {
    fn has_session(&self) -> bool {
        self.def.headless.as_ref().and_then(|h| h.output.as_ref()).is_some_and(|o| o.session.is_some())
    }

    /// Run one stage; returns the report and, when the program ran, the sample.
    fn stage(&self, stage: Stage, resume: Option<&str>, prior: Option<&str>) -> (StageReport, Option<Sample>, Option<CheckError>) {
        let project = self.session.work_dir.to_string_lossy().into_owned();
        let (task, model) = stage_inputs(stage);
        let launch = HeadlessLaunch {
            project: &project,
            task,
            read_only: false,
            resume_session: resume,
            trust_workspace: false,
            allow_shell: false,
            model,
        };
        let spec = match self.profile.headless_spec(&launch, self.program.clone()) {
            Ok(spec) => spec,
            Err(message) => {
                let mut report = StageReport::skipped(stage, "");
                report.status = "failed";
                report.note = None;
                report.problems = vec![message.clone()];
                let err = CheckError::new("livetest", "LIVETEST_SPAWN_FAILED", stage.as_str(), message);
                return (report, None, Some(err));
            }
        };
        let stdin = self.def.headless.as_ref().and_then(|h| template::render_stdin(h, &launch));
        let early = |text: &str| self.profile.headless_result_ready(text);
        let mut run = RunSpec::new(&self.program, &spec.args, &spec.cwd);
        run.stdin = stdin.as_deref();
        run.env = &spec.env;
        run.env_remove = &spec.env_remove;
        run.timeout = self.env.limits.live_timeout;
        run.max_bytes = self.env.limits.live_max_bytes;
        run.cancel = Some(self.cancel);
        run.early_stop = Some(&early);
        run.grace = self.env.limits.live_grace;
        let out = exec::run(&run);

        let stdout_plain = exec::plain(&out.stdout);
        let stdout = exec::clean(&out.stdout);
        let stderr = exec::clean(&out.stderr);
        let exit = if out.stopped_early { Some(0) } else { out.exit };
        let sample = Sample {
            stage,
            args: spec.args.clone(),
            stdout: stdout.clone(),
            stderr: stderr.clone(),
            exit,
            resume_session: resume.map(str::to_string),
        };
        let mut report = StageReport {
            name: stage.as_str(),
            label: stage.label(),
            status: "passed",
            args: spec.args.clone(),
            exit,
            duration_ms: out.duration.as_millis() as u64,
            timed_out: out.timed_out,
            result: None,
            session: None,
            error_code: None,
            problems: Vec::new(),
            note: None,
            stdout: exec::cut_chars(&stdout, REPLY_STREAM_CHARS),
            stderr: exec::cut_chars(&stderr, REPLY_STREAM_CHARS),
        };
        let code_name = format!("LIVETEST_{}_FAILED", stage.as_str().to_ascii_uppercase());
        let shown = format!("`{}`", spec.args.join(" "));
        let fail = |report: &mut StageReport, code: String, message: String| {
            report.status = "failed";
            report.problems.push(message.clone());
            CheckError::new("livetest", code, stage.as_str(), format!("{message} The command was {shown}."))
                .with_excerpt(if stdout.trim().is_empty() { &stderr } else { &stdout })
        };
        if let Some(e) = &out.spawn_error {
            let err = fail(&mut report, "LIVETEST_SPAWN_FAILED".into(), format!("the program could not be started: {e}."));
            return (report, None, Some(err));
        }
        if out.cancelled {
            report.status = "failed";
            report.note = Some("cancelled".into());
            return (report, Some(sample), None);
        }
        if out.timed_out {
            let err = fail(
                &mut report,
                "LIVETEST_TIMEOUT".into(),
                format!("the program did not finish within {} seconds.", self.env.limits.live_timeout.as_secs()),
            );
            return (report, Some(sample), Some(err));
        }
        if stdout.trim().is_empty() && stage != Stage::BadModel {
            let err = fail(&mut report, "LIVETEST_NO_OUTPUT".into(), "the program printed nothing on stdout.".into());
            return (report, Some(sample), Some(err));
        }
        let judged = judge(self.profile, stage, &stdout_plain, exit, prior, self.has_session());
        report.result = judged.result.clone();
        report.session = judged.session.clone();
        report.error_code = judged.classified.as_ref().map(|(c, _)| c.clone());
        if !judged.passed() {
            let err = fail(&mut report, code_name, judged.problems.join(" "));
            return (report, Some(sample), Some(err));
        }
        (report, Some(sample), None)
    }
}

/// Run the stages against `program`. Pure over the session snapshot and environment.
pub fn execute(session: &Session, env: &WizardEnv, draft: &Draft, cancel: &AtomicBool) -> LiveOutcome {
    let mut outcome = LiveOutcome { stages: Vec::new(), errors: Vec::new(), samples: Vec::new() };
    let def = match parse_definition(&draft.text) {
        Ok(def) => def,
        Err(e) => {
            outcome.errors.push(CheckError::new("livetest", "LIVETEST_SPAWN_FAILED", "", e.to_string()));
            return outcome;
        }
    };
    let profile = match DeclarativeProfile::new(def.clone()) {
        Ok(p) => p,
        Err(e) => {
            outcome.errors.push(CheckError::new("livetest", "LIVETEST_SPAWN_FAILED", "", e.to_string()));
            return outcome;
        }
    };
    let program = match resolve_in(&def.binary, &session.dirs, Platform::current()) {
        Ok(p) => p,
        Err(e) => {
            outcome.errors.push(CheckError::new("livetest", "LIVETEST_SPAWN_FAILED", "binary", e.to_string()));
            return outcome;
        }
    };
    let runner = Runner { profile: &profile, def: &def, program, session, env, cancel };

    let (pong, sample, error) = runner.stage(Stage::Pong, None, None);
    let session_id = pong.session.clone();
    let pong_passed = pong.status == "passed";
    outcome.stages.push(pong);
    outcome.samples.extend(sample);
    outcome.errors.extend(error);
    if !pong_passed {
        outcome.stages.push(StageReport::skipped(Stage::Resume, "not run: the first stage failed"));
        outcome.stages.push(StageReport::skipped(Stage::BadModel, "not run: the first stage failed"));
        return outcome;
    }

    if can_resume(&def) {
        let (report, sample, error) = runner.stage(Stage::Resume, session_id.as_deref(), session_id.as_deref());
        let passed = report.status == "passed";
        outcome.stages.push(report);
        outcome.samples.extend(sample);
        outcome.errors.extend(error);
        if !passed {
            outcome.stages.push(StageReport::skipped(Stage::BadModel, "not run: the resume stage failed"));
            return outcome;
        }
    } else {
        outcome.stages.push(StageReport::skipped(
            Stage::Resume,
            "the draft has no resume segment or no session selector, so follow-up turns start a new conversation",
        ));
    }

    if has_model_option(&def) {
        let (report, sample, error) = runner.stage(Stage::BadModel, None, None);
        outcome.stages.push(report);
        outcome.samples.extend(sample);
        outcome.errors.extend(error);
    } else {
        outcome.stages.push(StageReport::skipped(
            Stage::BadModel,
            "the draft has no model option, so the wrong-model case was not tested",
        ));
    }
    outcome
}

pub fn outcome_json(outcome: &LiveOutcome, state: State, version: Option<&str>, fixtures: &[String]) -> Value {
    json!({
        "ok": outcome.ok(),
        "state": state.as_str(),
        "stages": outcome.stages,
        "errors": outcome.errors,
        "text": render(&outcome.errors, ReportSource::LiveTest),
        "tested_version": version,
        "fixtures": fixtures,
    })
}

/// `POST /agents/wizard/livetest`.
pub fn run(wizard: &Wizard, body: &Value) -> Result<Value, WizardError> {
    let id = super::api::session_id(body)?;
    if body.get("confirm").and_then(Value::as_bool) != Some(true) {
        return Err(WizardError::new(
            400,
            "WIZARD_CONFIRM_REQUIRED",
            "the live test uses your plan (about 3 small messages); send confirm: true after the person agrees",
        ));
    }
    let snap = wizard.begin(&id, "livetest", &[State::Verified])?;
    let env = wizard.env();
    let Some(draft) = snap.draft.clone() else {
        wizard.release(&id);
        return Err(WizardError::new(409, "WIZARD_BAD_STATE", "there is no verified draft; call verify first"));
    };
    let outcome = execute(&snap, &env, &draft, &snap.cancel);
    let ok = outcome.ok() && !outcome.stages.iter().any(|s| s.note.as_deref() == Some("cancelled"));
    let summaries: Vec<StageSummary> = outcome
        .stages
        .iter()
        .map(|s| StageSummary { name: s.name, status: s.status })
        .collect();
    let samples = outcome.samples.clone();
    let version = snap.version.number.clone();
    let sha = draft.sha256.clone();
    let state = wizard.end(&id, |s| {
        s.samples = samples;
        if ok {
            s.tested = Some(Tested { version: version.clone(), stages: summaries, draft_sha256: sha });
            s.state = State::Tested;
        }
        s.state
    })?;
    let names: Vec<String> = if ok {
        let ext = super::finish::draft_extension(&draft.text);
        let mut names: Vec<String> = outcome
            .samples
            .iter()
            .map(|s| format!("{}.{ext}", s.stage.fixture_stem()))
            .collect();
        names.push("headless_garbage.txt".into());
        names
    } else {
        Vec::new()
    };
    Ok(outcome_json(&outcome, state, snap.version.number.as_deref(), &names))
}
