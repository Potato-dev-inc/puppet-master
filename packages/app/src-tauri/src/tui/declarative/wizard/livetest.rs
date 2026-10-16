//! Live test (SPEC 5): run the verified draft against the real program. A one-word answer, a short
//! sentence (spaces and punctuation, so a one-word regex fails), a resumed turn with the session id, and
//! a wrong model name that must be classified as an error. The expected answers are random per session
//! (`session::TestWords`) and are masked in everything shown to the drafter. Stops at the first failed stage so no plan is wasted. The outputs are kept
//! as samples (replayed for free by the next `verify`) and, when all stages pass, become fixtures.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use once_cell::sync::Lazy;
use regex::Regex;
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
use super::session::{Draft, Sample, Session, Stage, StageSummary, State, TestWords, Tested, Wizard, WizardEnv};

pub const BAD_MODEL: &str = "pm-wizard-no-such-model/zzz";
/// How much of each stream goes back to the UI (the full capped output is kept in the session).
pub const REPLY_STREAM_CHARS: usize = 8 * 1024;

/// The task text of a stage, with this session's random words in it.
pub fn stage_task(stage: Stage, words: &TestWords) -> String {
    match stage {
        Stage::Pong | Stage::BadModel => format!("Reply with exactly the word: {}", words.one),
        Stage::Sentence => format!("Reply with exactly: {}", words.sentence()),
        Stage::Resume => format!("Reply with exactly the word: {}", words.two),
    }
}

/// The answer a stage expects (empty for the wrong-model stage, which must give no answer).
pub fn stage_want(stage: Stage, words: &TestWords) -> String {
    match stage {
        Stage::Pong => words.one.clone(),
        Stage::Sentence => words.sentence(),
        Stage::Resume => words.two.clone(),
        Stage::BadModel => String::new(),
    }
}

/// The model of a stage.
pub fn stage_model(stage: Stage) -> Option<&'static str> {
    (stage == Stage::BadModel).then_some(BAD_MODEL)
}

/// Replace the live-test answers in text that goes to the drafter or the UI (`<word-1>` ...).
pub fn mask_words(text: &str, words: &TestWords) -> String {
    text.replace(&words.one, "<word-1>")
        .replace(&words.two, "<word-2>")
        .replace(&words.sky, "<word-3>")
        // a streamed answer arrives in pieces: the random part on its own is masked too
        .replace(&words.one[5..], "<w1>")
        .replace(&words.two[5..], "<w2>")
}

/// The argument list the draft renders for a stage (no program name).
pub fn stage_args(def: &Definition, stage: Stage, project: &str, resume: Option<&str>, words: &TestWords) -> Vec<String> {
    let task = stage_task(stage, words);
    let model = stage_model(stage);
    let launch = HeadlessLaunch {
        project,
        task: &task,
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

/// The answer with quotes, markdown marks and a final full stop or "!" taken off, lowercase, spaces folded.
fn plain_answer(got: &str) -> String {
    let quotes = |c: char| matches!(c, '"' | '\'' | '`' | '*' | '_');
    let trimmed = got.trim().trim_matches(quotes).trim_end_matches(['.', '!', ' ']).trim_matches(quotes);
    trimmed.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// The sentence stage: the same words in the same order (case, quotes and the final stop do not matter).
fn same_sentence(got: &str, want: &str) -> bool {
    plain_answer(got) == plain_answer(want)
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
    want: &str,
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
        Stage::Pong | Stage::Sentence | Stage::Resume => {
            let matches = |r: &str| if stage == Stage::Sentence { same_sentence(r, want) } else { same_word(r, want) };
            match &result {
                Some(r) if matches(r) => {
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
            if has_session && session.is_none() && stage != Stage::Sentence {
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

/// What the live test is about to run, written so the person can read it before agreeing. The task is
/// shown as `<test message>`, the resumed session as `<session id>` and `{project}` as `<work folder>`.
#[derive(Debug, Clone, Serialize)]
pub struct PlannedStage {
    pub name: &'static str,
    pub label: &'static str,
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub program: PathBuf,
    pub stages: Vec<PlannedStage>,
    /// The task goes to the program's standard input instead of an argument.
    pub stdin: Option<String>,
    pub env: Vec<(String, String)>,
    pub env_unset: Vec<String>,
    /// SHA-256 of everything above: the live test refuses to run unless the caller sends this back.
    pub fingerprint: String,
}

fn planned_args(def: &Definition, stage: Stage) -> Vec<String> {
    let model = stage_model(stage);
    let launch = HeadlessLaunch {
        project: "<work folder>",
        task: "<test message>",
        read_only: false,
        resume_session: (stage == Stage::Resume).then_some("<session id>"),
        trust_workspace: false,
        allow_shell: false,
        model,
    };
    template::render_headless(def, &launch).unwrap_or_default()
}

/// The commands the live test will run for `def`, exactly as `execute` will build them (same program
/// resolution, same stages), with the test message and session id left as placeholders.
pub fn plan(session: &Session, def: &Definition) -> Result<Plan, String> {
    if !def.headless.as_ref().is_some_and(|h| h.output.is_some()) {
        return Err("the draft has no [headless] table with an output, so there is no one-task mode to test".into());
    }
    let mut table = def.binary.clone();
    table.names = vec![session.command.clone()];
    let program = resolve_in(&table, &session.dirs, Platform::current()).map_err(|e| e.to_string())?;
    let mut stages = vec![Stage::Pong, Stage::Sentence];
    if can_resume(def) {
        stages.push(Stage::Resume);
    }
    if has_model_option(def) {
        stages.push(Stage::BadModel);
    }
    let stages: Vec<PlannedStage> = stages
        .into_iter()
        .map(|st| PlannedStage { name: st.as_str(), label: st.label(), args: planned_args(def, st) })
        .collect();
    let stdin = def.headless.as_ref().and_then(|h| {
        let launch = HeadlessLaunch {
            project: "<work folder>",
            task: "<test message>",
            read_only: false,
            resume_session: None,
            trust_workspace: false,
            allow_shell: false,
            model: None,
        };
        template::render_stdin(h, &launch)
    });
    let env: Vec<(String, String)> = def.env.set.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let env_unset = def.env.unset.clone();
    let canonical = json!({
        "program": program.to_string_lossy(),
        "stages": stages.iter().map(|st| json!({"name": st.name, "args": st.args})).collect::<Vec<_>>(),
        "stdin": stdin,
        "env": env,
        "env_unset": env_unset,
    });
    let fingerprint = super::session::sha256_hex(canonical.to_string().as_bytes());
    Ok(Plan { program, stages, stdin, env, env_unset, fingerprint })
}

pub fn plan_json(plan: &Plan) -> Value {
    json!({
        "program": {"path": plan.program.display().to_string(), "display": super::locate::display_path(&plan.program)},
        "stages": plan.stages,
        "stdin": plan.stdin,
        "env": plan.env.iter().map(|(k, v)| json!({"name": k, "value": v})).collect::<Vec<_>>(),
        "env_unset": plan.env_unset,
        "fingerprint": plan.fingerprint,
    })
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
    /// The program was still running after its `done` event and was stopped (so `exit` is not its own).
    pub stopped_early: bool,
    /// First line of what the program itself said went wrong (its own error event, or the first line
    /// it printed when it exited with an error). Masked like the output.
    pub error_message: Option<String>,
    /// The failure is the program's own (sign-in, quota, provider, network ...), not the profile's.
    pub agent_side: bool,
    pub problems: Vec<String>,
    pub note: Option<String>,
    pub stdout: String,
    pub stderr: String,
}

/// Words in a program's own error text that point at its setup, not at the profile.
pub(crate) static EXTERNAL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)log(?:ged)? ?in|sign(?:ed)?[- ]?in|unauthori[sz]ed|authenticat|api[ _-]?key|credential|token (?:expired|invalid)|quota|rate[- ]?limit|too many requests|insufficient|billing|payment|credits?\b|subscription|no endpoints|provider|network|connection (?:refused|reset|error)|econn|enotfound|getaddrinfo|dns|certificate|offline|\b(?:401|402|403|429|50\d)\b|model not found|unknown model",
    )
    .expect("external error regex")
});

/// The first line that says something, at most 300 characters.
pub fn first_line(text: &str) -> String {
    exec::cut_chars(text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or(""), 300)
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
            stopped_early: false,
            error_message: None,
            agent_side: false,
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
        let words = &self.session.words;
        let task = stage_task(stage, words);
        let want = stage_want(stage, words);
        let launch = HeadlessLaunch {
            project: &project,
            task: &task,
            read_only: false,
            resume_session: resume,
            trust_workspace: false,
            allow_shell: false,
            model: stage_model(stage),
        };
        let mask = |text: &str| mask_words(text, words);
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
        // Always the isolated work folder, whatever `cwd` the draft asks for.
        let mut run = RunSpec::new(&self.program, &spec.args, &self.session.work_dir);
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
            want: want.clone(),
            resume_session: resume.map(str::to_string),
        };
        let mut report = StageReport {
            name: stage.as_str(),
            label: stage.label(),
            status: "passed",
            args: spec.args.iter().map(|a| mask(a)).collect(),
            exit,
            duration_ms: out.duration.as_millis() as u64,
            timed_out: out.timed_out,
            result: None,
            session: None,
            error_code: None,
            stopped_early: out.stopped_early,
            error_message: None,
            agent_side: false,
            problems: Vec::new(),
            note: None,
            stdout: exec::cut_chars(&mask(&stdout), REPLY_STREAM_CHARS),
            stderr: exec::cut_chars(&mask(&stderr), REPLY_STREAM_CHARS),
        };
        let code_name = format!("LIVETEST_{}_FAILED", stage.as_str().to_ascii_uppercase());
        let shown = mask(&format!("`{}`", spec.args.join(" ")));
        let fail = |report: &mut StageReport, code: String, message: String| {
            let message = mask(&message);
            report.status = "failed";
            report.problems.push(message.clone());
            CheckError::new("livetest", code, stage.as_str(), format!("{message} The command was {shown}."))
                .with_excerpt(&mask(if stdout.trim().is_empty() { &stderr } else { &stdout }))
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
        // A program that exits with an error says why on its first line: that is its own problem when the
        // text points at sign-in, quota, provider or network, and worth remembering either way.
        if !out.timed_out && stage != Stage::BadModel && exit.is_some_and(|c| c != 0) {
            let said = if stderr.trim().is_empty() { &stdout } else { &stderr };
            let line = first_line(said);
            if !line.is_empty() {
                report.agent_side = EXTERNAL.is_match(&format!("{stderr}\n{stdout}"));
                report.error_message = Some(mask(&line));
            }
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
        let judged = judge(self.profile, stage, &want, &stdout_plain, exit, prior, self.has_session());
        report.result = judged.result.as_deref().map(mask);
        report.session = judged.session.clone();
        report.error_code = judged.classified.as_ref().map(|(c, _)| c.clone());
        if !judged.passed() && stage != Stage::BadModel {
            // Is it the program that is broken (its own error, no answer) rather than the profile?
            let reported = judged
                .classified
                .as_ref()
                .filter(|(c, _)| c == "AGENT_REPORTED_ERROR")
                .map(|(_, m)| first_line(m))
                .filter(|m| !m.is_empty());
            if judged.result.is_none() && reported.is_some() {
                report.error_message = reported.map(|m| mask(&m));
                report.agent_side = true;
            }
        }
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
    // Only the program the person typed ever runs here, whatever other names the draft lists.
    let mut table = def.binary.clone();
    table.names = vec![session.command.clone()];
    let program = match resolve_in(&table, &session.dirs, Platform::current()) {
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
        outcome.stages.push(StageReport::skipped(Stage::Sentence, "not run: the first stage failed"));
        outcome.stages.push(StageReport::skipped(Stage::Resume, "not run: the first stage failed"));
        outcome.stages.push(StageReport::skipped(Stage::BadModel, "not run: the first stage failed"));
        return outcome;
    }

    let (sentence, sample, error) = runner.stage(Stage::Sentence, None, None);
    let sentence_passed = sentence.status == "passed";
    outcome.stages.push(sentence);
    outcome.samples.extend(sample);
    outcome.errors.extend(error);
    if !sentence_passed {
        outcome.stages.push(StageReport::skipped(Stage::Resume, "not run: the sentence stage failed"));
        outcome.stages.push(StageReport::skipped(Stage::BadModel, "not run: the sentence stage failed"));
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

/// The program's own error behind a failed live test: `{stage, message, external}`. `external` means
/// "its sign-in, model or provider is broken", so another draft cannot help; a message that is not
/// external is still reported so the caller can see the same error twice in a row.
pub fn agent_error(outcome: &LiveOutcome) -> Option<Value> {
    outcome
        .stages
        .iter()
        .find(|s| s.status == "failed" && s.error_message.is_some())
        .map(|s| json!({"stage": s.name, "message": s.error_message, "external": s.agent_side}))
}

pub fn outcome_json(outcome: &LiveOutcome, state: State, version: Option<&str>, fixtures: &[String]) -> Value {
    json!({
        "agent_error": agent_error(outcome),
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
    let given = body.get("plan").and_then(Value::as_str).map(str::trim).filter(|p| !p.is_empty());
    let Some(given) = given else {
        return Err(WizardError::new(
            400,
            "WIZARD_PLAN_REQUIRED",
            "send the fingerprint of the planned commands (the `planned.fingerprint` that verify returned) that the person saw",
        ));
    };
    let snap = wizard.begin(&id, "livetest", &[State::Probed, State::Verified, State::Tested])?;
    let env = wizard.env();
    let Some(draft) = snap.draft.clone() else {
        wizard.release(&id);
        return Err(WizardError::new(
            409,
            "WIZARD_BAD_STATE",
            "there is no draft the live test may run; call verify first (a draft that does not validate cannot be tested)",
        ));
    };
    let parsed = parse_definition(&draft.text).map_err(|e| e.to_string());
    if parsed.as_ref().is_ok_and(|def| !def.headless.as_ref().is_some_and(|h| h.output.is_some())) {
        wizard.release(&id);
        return Err(WizardError::new(
            409,
            "WIZARD_NO_HEADLESS_TEST",
            "the draft has no [headless] table with an output, so there is no one-task mode to test; use the visible test",
        ));
    }
    let current = parsed.and_then(|def| plan(&snap, &def));
    match current {
        Ok(p) if p.fingerprint == given => {}
        Ok(_) => {
            wizard.release(&id);
            return Err(WizardError::new(
                409,
                "WIZARD_PLAN_MISMATCH",
                "the commands the live test would run are not the ones the person was shown; show them again and ask for a new confirmation",
            ));
        }
        Err(message) => {
            wizard.release(&id);
            return Err(WizardError::new(409, "WIZARD_BINARY_UNRESOLVED", message));
        }
    }
    let mut outcome = execute(&snap, &env, &draft, &snap.cancel);
    // Then the shared rules and the fixture checks, now that the real output is in the session, so one
    // reply carries every problem. (The live stages already report a replayed sample's failure.)
    let mut with_samples = snap.clone();
    with_samples.samples = outcome.samples.clone();
    let checks = super::verify::check_draft_with(&with_samples, &env, &draft.text, &draft.expects);
    outcome.errors.extend(
        checks
            .errors
            .into_iter()
            .filter(|e| e.stage == "conformance" || (e.stage == "fixture" && !e.path.ends_with(".sample"))),
    );
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
            super::session::settle(s);
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
