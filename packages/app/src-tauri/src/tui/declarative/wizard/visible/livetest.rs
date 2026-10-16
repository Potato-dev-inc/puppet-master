//! The visible test (SPEC 11.9): start a NEW pane from the drafted profile, through the registry and the
//! profile's own `spawn_command` (nothing special about it), and check with the profile itself that it
//! reaches ready, takes a task through the app's delivery rules, is seen busy, finishes with a reply
//! that holds the random word, and makes an approval card for a permission question.
//!
//! The test answers nothing. A question on the way (trust, sign-in) waits for the person; the
//! permission check ends the moment the card exists and the pane is closed, which refuses the request.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

use crate::agent_runs::completion::{observe_tui_turn, TuiTurnDecision, TuiTurnWatch};
use crate::agent_runs::readiness::{assess, AgentProcess, Observation, Readiness};
use crate::tui::declarative::resolve::{resolve_in, Platform};
use crate::tui::declarative::schema::{parse_definition, Definition};
use crate::tui::declarative::template;
use crate::tui::declarative::DeclarativeProfile;
use crate::tui::types::StandbyScreen;
use crate::tui::TuiProfile;

use super::super::exec;
use super::super::livetest::{first_line, mask_words, EXTERNAL};
use super::super::report::{render, CheckError, Source as ReportSource, WizardError};
use super::super::session::{sha256_hex, Draft, Session, State, VisibleTested, Wizard, WizardEnv};
use super::answer::{scope_ok, AnswerCfg, AnswerKey, Answerer, AnsweredQ, Hint, Input as AnswerInput, Step as AnswerStep};
use super::api::make_project;
use super::capture::{message_task_for, permission_task, reply_rows};
use super::host::{Look, PaneHost, PaneSpec};
use super::runner::TestLimits;
use super::screen::{classify, Flavour, ScreenKind, Stability, GRID_COLS, GRID_ROWS};

/// The commands the visible test would start, written so the person can read them first.
#[derive(Debug, Clone)]
pub struct VisiblePlan {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub env_unset: Vec<String>,
    pub fingerprint: String,
}

fn program_for(session: &Session, def: &Definition) -> Result<PathBuf, String> {
    let mut table = def.binary.clone();
    table.names = vec![session.command.clone()];
    resolve_in(&table, &session.dirs, Platform::current()).map_err(|e| e.to_string())
}

/// What the test pane would start for `def`: the program the typed command resolves to (with the
/// draft's own shim globs), the rendered `[tui]` arguments and the `[env]` changes.
pub fn plan(session: &Session, def: &Definition) -> Result<VisiblePlan, String> {
    let tui = def.tui.as_ref().ok_or("the draft has no [tui] table")?;
    let program = program_for(session, def)?;
    let args = template::render_tui(tui, "<isolated folder>", false, None, None);
    let env: Vec<(String, String)> = def.env.set.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let env_unset = def.env.unset.clone();
    let canonical = json!({
        "program": program.to_string_lossy(),
        "args": args,
        "env": env,
        "env_unset": env_unset,
        "grid": [GRID_COLS, GRID_ROWS],
        "cwd": "<isolated folder>",
    });
    let fingerprint = sha256_hex(canonical.to_string().as_bytes());
    Ok(VisiblePlan { program, args, env, env_unset, fingerprint })
}

pub fn plan_json(plan: &VisiblePlan) -> Value {
    json!({
        "program": {"path": plan.program.display().to_string(), "display": super::super::locate::display_path(&plan.program)},
        "args": plan.args,
        "env": plan.env.iter().map(|(k, v)| json!({"name": k, "value": v})).collect::<Vec<_>>(),
        "env_unset": plan.env_unset,
        "cwd": "<isolated folder>",
        "grid": {"cols": GRID_COLS, "rows": GRID_ROWS},
        "fingerprint": plan.fingerprint,
    })
}

/// The profile of the test pane: the draft, with its id and program names changed to the wizard's.
pub fn test_profile(session: &Session, def: &Definition) -> Result<DeclarativeProfile, String> {
    let mut def = def.clone();
    def.agent.id = format!("wzt_{}", session.id.trim_start_matches("wz_"));
    def.binary.names = vec![session.command.clone()];
    DeclarativeProfile::new(def).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub name: &'static str,
    pub label: &'static str,
    /// `passed`, `failed` or `skipped`.
    pub status: &'static str,
    pub note: String,
    pub ms: u64,
}

pub struct Outcome {
    pub checks: Vec<CheckResult>,
    pub errors: Vec<CheckError>,
    /// The program's own error behind a failure: `{check, message, external}`.
    pub agent_error: Option<Value>,
    /// (check, masked screen) of every failed check, kept for the next round.
    pub screens: Vec<(String, String)>,
    /// The questions the wizard answered for the person in the test pane (SPEC 11.16).
    pub answered: Vec<AnsweredQ>,
    /// The same, masked, for the reply.
    pub answered_json: Vec<Value>,
}

impl Outcome {
    pub fn ok(&self) -> bool {
        self.errors.is_empty() && !self.checks.iter().any(|c| c.status == "failed")
    }

    pub fn passed(&self) -> usize {
        self.checks.iter().filter(|c| c.status == "passed").count()
    }

    pub fn total(&self) -> usize {
        self.checks.iter().filter(|c| c.status != "skipped").count()
    }
}

const CHECKS: [(&str, &str); 8] = [
    ("starts", "The agent starts in a pane"),
    ("startup", "A startup question becomes a card"),
    ("ready", "It reaches its empty input box"),
    ("delivery", "A task is typed and sent"),
    ("busy", "It is seen working"),
    ("done_reply", "It finishes and the reply is read"),
    ("permission", "A permission question becomes a card"),
    ("exit", "The pane closes"),
];

fn label_of(name: &str) -> &'static str {
    CHECKS.iter().find(|(n, _)| *n == name).map_or("", |(_, l)| *l)
}

fn name_of(name: &str) -> &'static str {
    CHECKS.iter().find(|(n, _)| *n == name).map_or("", |(n, _)| *n)
}

/// Why a check failed, and the screen at that moment.
struct Fail {
    note: String,
    screen: String,
}

fn fail(note: impl Into<String>, screen: &str) -> Fail {
    Fail { note: note.into(), screen: screen.to_string() }
}

enum Wait<R> {
    Got(R),
    Timeout(Option<Look>),
    Gone(Option<Look>),
    Cancelled,
}

struct Test<'a> {
    host: &'a dyn PaneHost,
    pane: String,
    profile: Arc<DeclarativeProfile>,
    agent_type: String,
    session: &'a Session,
    limits: TestLimits,
    stable_ms: u64,
    /// See `capture::Config`: the same rules as the capture's.
    frozen_ms: u64,
    idle_done_ms: u64,
    question_ms: u64,
    cancel: &'a AtomicBool,
    project: PathBuf,
    began: Instant,
    status: &'a parking_lot::Mutex<Value>,
    checks: Vec<CheckResult>,
    /// "Agy", for the words shown to the person.
    agent_name: String,
    /// The capture saw a startup question (trust, first run): the draft must make a card of it.
    expects_startup: bool,
    /// The card check of the startup question, once the question has held still: Ok(note) / Err(why).
    startup: parking_lot::Mutex<Option<Result<String, Fail>>>,
    /// Answers setup questions in this pane (off unless the person left the switch on) and where it may.
    auto: Option<parking_lot::Mutex<Answerer>>,
    scope: (PathBuf, PathBuf),
    answered: parking_lot::Mutex<Vec<AnsweredQ>>,
}

impl Test<'_> {
    fn ms(&self) -> u64 {
        self.began.elapsed().as_millis() as u64
    }

    fn masked(&self, text: &str) -> String {
        mask_words(&exec::clean(text), &self.session.words)
    }

    fn publish(&self, current: Option<&str>, needs: Option<(&str, &str)>, screen: &str) {
        let rows: Vec<&str> = screen.lines().collect();
        let from = rows.len().saturating_sub(40);
        let shown = self.masked(&rows[from..].join("\n"));
        *self.status.lock() = json!({
            "running": true,
            "check": current,
            "needs_you": needs.map(|(kind, message)| json!({"kind": kind, "message": message})),
            "checks": self.checks,
            "screen": shown,
            "answered": self.answered_json(),
        });
    }

    fn answered_json(&self) -> Vec<Value> {
        self.answered.lock().iter().map(|a| a.to_json(&|t: &str| self.masked(t))).collect()
    }

    /// The draft's own `allow_once` keys for the question on screen, when its `[[tui.prompt]]` detects it.
    fn hint_for(&self, screen: &str) -> Option<Hint> {
        let prompt = self.profile.detect_prompt(screen, false)?;
        let reply = self.profile.prompt_reply(&prompt, "allow_once", false).ok()?;
        let keys = match (reply.text, reply.key) {
            (Some(t), _) if t.chars().count() <= 3 && t.chars().all(|c| c.is_ascii_alphanumeric()) => vec![AnswerKey::Text(t)],
            (None, Some(k)) => vec![AnswerKey::from_name(&k)?],
            _ => return None,
        };
        Some(Hint { keys, label: "Allow once".into() })
    }

    /// The card check of a startup question (once it holds still): the draft's `[[tui.prompt]]` must detect
    /// it and offer Allow and Deny, or it would never become a card. Recorded before anything answers it.
    fn check_startup_card(&self, screen: &str) {
        let mut slot = self.startup.lock();
        if slot.is_some() {
            return;
        }
        let result = match self.profile.declared_prompt(screen, false) {
            None => Err(fail(
                "A startup question is on the screen but no `[[tui.prompt]]` matches it, so it would not become a card and the agent would sit \"idle\" behind it. Add a `[[tui.prompt]]` (kind `workspace_trust` for a trust-this-folder question) whose `detect_regex` matches the `tui_trust` capture, with an `allow_once` and a `deny` choice.",
                screen,
            )),
            Some(prompt) => {
                let kind = prompt.get("kind").and_then(|k| k.as_str()).unwrap_or("").to_string();
                let missing = ["allow_once", "deny"].into_iter().find(|c| self.profile.prompt_reply(&prompt, c, false).is_err());
                match missing {
                    Some(choice) => Err(fail(
                        format!("The startup question was read as `{kind}` but it has no `{choice}` choice that can be answered. Add `{{ id = \"{choice}\", key = \"...\" }}` to its `choices`."),
                        screen,
                    )),
                    None => Ok(format!("The startup question is read as `{kind}` with Allow and Deny, so it becomes a card.")),
                }
            }
        };
        *slot = Some(result);
    }

    /// Press the answer's keys: only in this wizard's own pane and folder (checked here, and again by the host).
    fn press(&self, keys: &[AnswerKey]) -> Result<(), String> {
        scope_ok(self.host.cwd(&self.pane).as_deref(), &self.scope.0, &self.scope.1)?;
        self.host.answer(&self.pane, keys)
    }

    /// One look of the answerer at a question on the test pane. `None` = not its business.
    fn auto_step(&self, look: &Look, kind: ScreenKind, stable_ms: u64) -> Option<Result<(), String>> {
        let cell = self.auto.as_ref()?;
        let flavour = match kind {
            ScreenKind::Dialog(f) => Some(f),
            _ => None,
        };
        let hint = self.hint_for(&look.screen);
        let step = cell.lock().step(&AnswerInput { screen: &look.screen, flavour, stable_ms, now: self.ms(), hint: hint.as_ref() });
        match step {
            AnswerStep::Nothing => None,
            AnswerStep::Waiting(_) => Some(Ok(())),
            AnswerStep::Press { keys, .. } => match self.press(&keys) {
                Ok(()) => Some(Ok(())),
                Err(why) => {
                    // never again in this pane: the question is the person's
                    cell.lock().reset();
                    Some(Err(format!("pressing was refused: {why}")))
                }
            },
            AnswerStep::Answered(done) => {
                self.answered.lock().push(done);
                Some(Ok(()))
            }
            AnswerStep::Declined(reason) => Some(Err(reason)),
        }
    }

    /// The person pressed "Stop the test" (or Skip) in the wizard window.
    fn stop_requested(&self) -> bool {
        self.session.visible.test_ctl.lock().take().is_some()
    }

    fn look(&self) -> Option<Look> {
        let mut look = self.host.look(&self.pane)?;
        look.hello_written = self.project.join("hello.txt").is_file();
        Some(look)
    }

    /// Poll the pane until `f` says yes, the time is up, the agent is gone or the session is cancelled.
    fn wait<R>(&self, label: Option<&str>, timeout: Duration, mut f: impl FnMut(&Look) -> Option<R>) -> Wait<R> {
        let until = Instant::now() + timeout;
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                return Wait::Cancelled;
            }
            let Some(look) = self.look() else { return Wait::Gone(None) };
            if let Some(label) = label {
                self.publish(Some(label), None, &look.screen);
            }
            if !look.alive {
                return Wait::Gone(Some(look));
            }
            if let Some(found) = f(&look) {
                return Wait::Got(found);
            }
            if Instant::now() >= until {
                return Wait::Timeout(Some(look));
            }
            std::thread::sleep(self.limits.tick);
        }
    }

    fn observation(&self, look: &Look) -> Observation {
        Observation {
            pane_id: self.pane.clone(),
            screen: look.screen.clone(),
            output_pos: look.output_pos,
            agent: if look.alive { AgentProcess::Alive } else { AgentProcess::Exited },
        }
    }

    fn gone(&self, what: &str, look: Option<Look>) -> Fail {
        let screen = look.map(|l| l.screen).unwrap_or_default();
        fail(format!("The agent ended while the test was {what}. Check the `[tui] argv` and that the program starts in a pane."), &screen)
    }

    /// `ready` verdict and `[tui.ready]` agree that the empty input box is showing.
    fn is_ready(&self, look: &Look) -> bool {
        let verdict = assess(self.profile.as_ref(), self.observation(look));
        if !verdict.is_ready() {
            return false;
        }
        let has_table = self.profile.definition().tui.as_ref().is_some_and(|t| t.ready.is_some());
        !has_table || self.profile.standby_screen(&look.screen) == StandbyScreen::Ready
    }

    /// Wait until the pane is ready and the screen has held still. A question on the way (the profile's
    /// own prompt, or anything that looks like one) waits for the person: nothing is typed or pressed.
    fn await_ready(&self, label: &str, timeout: Duration) -> Result<Look, Fail> {
        let began = Instant::now();
        let mut stab = Stability::new();
        let mut asked_since: Option<Instant> = None;
        // The last screen that was a question: if the question is answered before it held still for the card
        // check, the check still looks at it.
        let mut question_screen: Option<String> = None;
        let mut last: Option<Look> = None;
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                return Err(fail("The test was cancelled.", ""));
            }
            let Some(look) = self.look() else { return Err(self.gone(label, last)) };
            if !look.alive {
                return Err(self.gone(label, Some(look)));
            }
            stab.observe(&look.screen, self.ms());
            let kind = classify(&look.screen);
            let verdict = assess(self.profile.as_ref(), self.observation(&look));
            let question = kind.is_dialog() || matches!(verdict.readiness(), Readiness::Gate(_));
            if question {
                let since = *asked_since.get_or_insert_with(Instant::now);
                if self.stop_requested() {
                    return Err(fail("You stopped the test while the agent was asking a question.", &look.screen));
                }
                let flavour = match kind {
                    ScreenKind::Dialog(f) => f,
                    _ => Flavour::Question,
                };
                let stable = stab.stable_for(self.ms());
                // 1. the draft must see the question as a card (checked before anything answers it)
                let startup_phase = label == "ready";
                question_screen = Some(look.screen.clone());
                if startup_phase && stable >= 500 {
                    self.check_startup_card(&look.screen);
                }
                // 2. the wizard may answer a trust or one-time approval question in its own pane
                let mut declined: Option<String> = None;
                let mut busy_answering = false;
                if startup_phase && self.auto.is_some() && self.startup.lock().is_none() {
                    busy_answering = true; // waiting for the card check, which comes first
                } else if let Some(outcome) = self.auto_step(&look, kind, stable) {
                    match outcome {
                        Ok(()) => busy_answering = true,
                        Err(reason) => declined = Some(reason),
                    }
                }
                if busy_answering {
                    self.publish(Some(label), None, &look.screen);
                    last = Some(look);
                    std::thread::sleep(self.limits.tick);
                    continue;
                }
                // 3. the person's: say so plainly
                let mut message = format!(
                    "{} is asking a question in the test pane. Answer it there (it is the wizard's isolated temp folder, not your project).",
                    self.agent_name
                );
                if let Some(reason) = &declined {
                    message.push_str(&format!(" The wizard did not answer it for you: {reason}."));
                }
                let kind_word = if since.elapsed() > self.limits.step {
                    message.push_str(" This is taking long. You can stop the test.");
                    "stuck"
                } else {
                    flavour.as_str()
                };
                self.publish(Some(label), Some((kind_word, &message)), &look.screen);
                if since.elapsed() > self.limits.person {
                    return Err(fail("The agent kept asking a question and nobody answered it.", &look.screen));
                }
                last = Some(look);
                std::thread::sleep(self.limits.tick);
                continue;
            }
            // not a question (any more): the card check of the one that was there, if it was not made yet
            if let Some(screen) = question_screen.take() {
                if label == "ready" {
                    self.check_startup_card(&screen);
                }
            }
            // and if the wizard pressed keys for it, it was answered
            if let Some(cell) = &self.auto {
                if let AnswerStep::Answered(done) = cell.lock().step(&AnswerInput { screen: &look.screen, flavour: None, stable_ms: 0, now: self.ms(), hint: None }) {
                    self.answered.lock().push(done);
                }
            }
            asked_since = None;
            self.publish(Some(label), None, &look.screen);
            if self.is_ready(&look) && stab.stable_for(self.ms()) >= self.stable_ms {
                return Ok(look);
            }
            if began.elapsed() > timeout {
                let standby = self.profile.standby_screen(&look.screen);
                let why = if verdict.why().is_empty() { "the profile did not read the screen as an empty input box" } else { verdict.why() };
                return Err(fail(
                    format!(
                        "Within {} s the profile never read the screen as the empty input box: {why} (`[tui.ready]` says {standby:?}). Check `[tui.ready] regex` and `not_regex` against the idle capture, and `[tui.busy]`.",
                        timeout.as_secs()
                    ),
                    &look.screen,
                ));
            }
            last = Some(look);
            std::thread::sleep(self.limits.tick);
        }
    }

    fn check_starts(&self) -> Result<String, Fail> {
        match self.wait(Some("starts"), self.limits.start, |_| None::<()>) {
            Wait::Timeout(Some(look)) if !look.screen.trim().is_empty() => Ok("The agent is running and drew a screen.".into()),
            Wait::Timeout(look) => Err(fail("The agent was running but drew nothing.", &look.map(|l| l.screen).unwrap_or_default())),
            Wait::Gone(look) => Err(self.gone("starting", look)),
            Wait::Cancelled => Err(fail("The test was cancelled.", "")),
            Wait::Got(()) => Ok(String::new()),
        }
    }

    fn check_delivery(&self, task: &str) -> Result<String, Fail> {
        let mut trace = Vec::new();
        match self.host.deliver(&self.pane, task, &mut trace) {
            super::host::Delivered::Submitted => Ok("The task was typed with the profile's delivery rules and sent.".into()),
            super::host::Delivered::Refused(why) => {
                let screen = self.look().map(|l| l.screen).unwrap_or_default();
                Err(fail(
                    format!("The task could not be delivered: {why}. Check `[tui.delivery]` (input_echo_regex, transcript_echo_regex) and `clear_input_keys`."),
                    &screen,
                ))
            }
        }
    }

    fn check_busy(&self, word: &str) -> Result<String, Fail> {
        let found = self.wait(Some("busy"), self.limits.busy, |look| {
            if look.running() {
                return Some("the pane status is running");
            }
            if self.profile.busy_marker(&crate::pane_activity::visible_tail(&look.watch)) == Some(true) {
                return Some("`[tui.busy] working_regex` matched");
            }
            None
        });
        match found {
            Wait::Got(how) => Ok(format!("Seen working: {how}.")),
            Wait::Gone(look) => Err(self.gone("waiting for the agent to work", look)),
            Wait::Timeout(look) => {
                let screen = look.map(|l| l.screen).unwrap_or_default();
                let answered = reply_rows(&screen, word) > 0;
                let note = if answered {
                    "The agent had already answered, and the profile never saw it working. Make `[tui.busy] working_regex` match what the working screen shows."
                } else {
                    "Within the time the profile never saw the agent working. Make `[tui.busy] working_regex` match what the working screen shows."
                };
                Err(fail(note, &screen))
            }
            Wait::Cancelled => Err(fail("The test was cancelled.", "")),
        }
    }

    fn check_done_reply(&self, task: &str, baseline: &str, word: &str) -> Result<String, Fail> {
        let mut watch = TuiTurnWatch::from_baseline(baseline).with_task(task);
        watch.set_delivery_verified(self.profile.verifies_prompt_delivery());
        let until = Instant::now() + self.limits.done;
        let mut last = Look::default();
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                return Err(fail("The test was cancelled.", ""));
            }
            let Some(look) = self.look() else { return Err(self.gone("waiting for the reply", Some(last))) };
            if !look.alive {
                return Err(self.gone("waiting for the reply", Some(look)));
            }
            self.publish(Some("done_reply"), None, &look.screen);
            let has_prompt = self.host.card(&self.pane).is_some() || look.status == "waiting_input";
            match observe_tui_turn(&mut watch, &look.status, &look.watch, has_prompt, &self.agent_type) {
                TuiTurnDecision::Complete { result, .. } => {
                    let shown = self.masked(&result);
                    if !result.to_lowercase().contains(&word.to_lowercase()) {
                        return Err(fail(format!("The reply that was read does not contain the answer the agent gave. It read: {shown:?}. Check `[tui.chrome]`, `[tui.tool_activity]` and `[tui.dialogs]`."), &look.watch));
                    }
                    if result.chars().count() > 400 {
                        return Err(fail(format!("The reply that was read is {} characters long: it carries screen chrome. Add the banner and footer rows to `[tui.chrome] line_regex`. It read: {shown:?}", result.chars().count()), &look.watch));
                    }
                    return Ok(format!("The reply was read: {shown:?}."));
                }
                TuiTurnDecision::WaitingInput if look.status != "waiting_input" && !has_prompt => {}
                TuiTurnDecision::WaitingInput => {
                    return Err(fail("The agent asked a question while it was answering a plain message. Check `[[tui.prompt]] detect_regex`: it may match text that is not a question.", &look.watch));
                }
                TuiTurnDecision::AmbiguousIdle | TuiTurnDecision::NotSubmitted => {
                    return Err(fail("The agent stopped working but no reply was read from the screen. Check `[tui.chrome]` and `[tui.tool_activity]`: they may be hiding the reply row, or `[tui.busy]` may call the finished screen busy.", &look.watch));
                }
                TuiTurnDecision::Continue | TuiTurnDecision::RetrySubmit => {}
            }
            last = look;
            if Instant::now() >= until {
                return Err(fail(
                    format!("No finished reply was read within {} s. Check `[tui.busy]` (it may call the finished screen busy) and `idle_after_ms`.", self.limits.done.as_secs()),
                    &last.watch,
                ));
            }
            std::thread::sleep(Duration::from_millis(500).min(self.limits.tick.max(Duration::from_millis(10)) * 2));
        }
    }

    /// Returns `Ok(None)` when no question appeared (the check is skipped).
    fn check_permission(&self, task: &str) -> Result<Option<String>, Fail> {
        let mut quiet = Stability::new();
        // how many different pictures the pane showed since before the task went in (the first is the start)
        let mut pictures = 0u32;
        if let Some(before) = self.look() {
            if quiet.observe(&before.screen, self.ms()) {
                pictures += 1;
            }
        }
        self.check_delivery(task)?;
        let until = Instant::now() + self.limits.permission;
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                return Err(fail("The test was cancelled.", ""));
            }
            let Some(look) = self.look() else { return Err(self.gone("waiting for the permission question", None)) };
            if !look.alive {
                return Err(self.gone("waiting for the permission question", Some(look)));
            }
            if quiet.observe(&look.screen, self.ms()) {
                pictures += 1;
            }
            let mut kind = classify(&look.screen);
            // "working" needs the screen to move: the same words on a picture that never changes are not work
            if kind == ScreenKind::Working && quiet.quiet_for(self.ms()) >= self.frozen_ms {
                kind = ScreenKind::Unknown;
            }
            let stable = quiet.stable_for(self.ms());
            let ready = kind != ScreenKind::Working && !kind.is_dialog() && self.is_ready(&look);
            let unrecognised = stable >= self.question_ms && !ready && kind != ScreenKind::Working && !kind.is_dialog();
            let message = format!(
                "{} shows a screen the wizard does not recognise in the test pane. If it is a question, answer it there (it is the wizard's isolated temp folder, not your project).",
                self.agent_name
            );
            let needs = unrecognised.then_some(("question", message.as_str()));
            self.publish(Some("permission"), needs, &look.screen);
            if self.stop_requested() {
                // "Skip this check": nothing more is waited for here
                return Ok(None);
            }
            if let Some(card) = self.host.card(&self.pane) {
                // The card was made from the screen as it is NOW; the look above may be a moment older.
                let mut fresh = self.look().unwrap_or(look);
                let mut prompt = self.profile.detect_prompt(&fresh.screen, false);
                for _ in 0..4 {
                    if prompt.is_some() {
                        break;
                    }
                    std::thread::sleep(self.limits.tick);
                    if let Some(again) = self.look() {
                        fresh = again;
                    }
                    prompt = self.profile.detect_prompt(&fresh.screen, false);
                }
                let look = fresh;
                let Some(prompt) = prompt else {
                    return Err(fail("A card was made for this screen, but the profile's own `detect_prompt` does not read it.", &look.screen));
                };
                let kind = prompt.get("kind").and_then(|k| k.as_str()).unwrap_or("").to_string();
                for choice in ["allow_once", "deny"] {
                    if self.profile.prompt_reply(&prompt, choice, false).is_err() {
                        return Err(fail(
                            format!("The question was read as `{kind}` but it has no `{choice}` choice that can be answered. Add `{{ id = \"{choice}\", key = \"...\" }}` to its `choices`."),
                            &look.screen,
                        ));
                    }
                }
                let offered = card.get("choices").map(|c| c.to_string()).unwrap_or_default();
                return Ok(Some(format!(
                    "A card with Allow and Deny is made for the question (`{kind}`, choices {offered}). Nothing was answered; the pane is closed, which refuses the request."
                )));
            }
            if kind.is_dialog() && stable >= self.stable_ms {
                return Err(fail(
                    "A question is on the screen but no `[[tui.prompt]]` matches it, so it would not become a card. Make `detect_regex` match the permission capture.",
                    &look.screen,
                ));
            }
            // the agent finished without asking: the file is there, or the pane is back at the profile's own
            // empty input box and has held still
            let wrote = look.hello_written && stable >= self.stable_ms && !kind.is_dialog() && kind != ScreenKind::Working;
            let back_at_ready = pictures >= 2 && ready && stable >= self.idle_done_ms;
            if wrote || back_at_ready {
                return Ok(None);
            }
            if Instant::now() >= until {
                return Ok(None);
            }
            std::thread::sleep(self.limits.tick);
        }
    }

    fn check_exit(&self) -> Result<String, Fail> {
        self.host.kill(&self.pane);
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            match self.look() {
                None => return Ok("The pane is closed.".into()),
                Some(look) if !look.alive => return Ok("The agent has ended.".into()),
                Some(look) => {
                    if Instant::now() >= until {
                        return Err(fail("The pane did not close.", &look.screen));
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

struct Guard<'a> {
    host: &'a dyn PaneHost,
    pane: Option<String>,
    profile_id: String,
    session: &'a Session,
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        if let Some(pane) = self.pane.take() {
            self.host.kill(&pane);
        }
        crate::tui::registry::registry().remove_transient(&self.profile_id);
        *self.session.visible.test_pane.lock() = None;
    }
}

/// Run the checks against `host`. Pure over the session snapshot and the environment.
pub fn execute(session: &Session, env: &WizardEnv, draft: &Draft, cancel: &AtomicBool) -> Outcome {
    session.visible.test_ctl.lock().take();
    let mut outcome = Outcome { checks: Vec::new(), errors: Vec::new(), agent_error: None, screens: Vec::new(), answered: Vec::new(), answered_json: Vec::new() };
    let setup_failed = |outcome: &mut Outcome, code: &str, message: String| {
        outcome.errors.push(CheckError::new("visibletest", code, "", message));
    };
    let def = match parse_definition(&draft.text) {
        Ok(def) => def,
        Err(e) => {
            setup_failed(&mut outcome, "VISIBLETEST_SPAWN_FAILED", e.to_string());
            return outcome;
        }
    };
    let profile = match test_profile(session, &def) {
        Ok(p) => Arc::new(p),
        Err(e) => {
            setup_failed(&mut outcome, "VISIBLETEST_SPAWN_FAILED", e);
            return outcome;
        }
    };
    let profile_id = profile.id().to_string();
    let project = session.work_dir.join("test-project");
    let _ = std::fs::remove_dir_all(&project);
    if let Err(e) = make_project(&project) {
        setup_failed(&mut outcome, "VISIBLETEST_SPAWN_FAILED", e.message);
        return outcome;
    }
    if let Err(e) = crate::tui::registry::registry().add_transient(profile.clone()) {
        setup_failed(&mut outcome, "VISIBLETEST_SPAWN_FAILED", e);
        return outcome;
    }
    let host = env.host.clone();
    let spec = PaneSpec { profile: profile.clone(), cwd: project.clone(), cols: GRID_COLS, rows: GRID_ROWS, window: env.visible.window };
    let pane = match host.spawn(&spec) {
        Ok(pane) => pane,
        Err(e) => {
            crate::tui::registry::registry().remove_transient(&profile_id);
            setup_failed(
                &mut outcome,
                "VISIBLETEST_SPAWN_FAILED",
                format!("the test pane could not be started: {e}. Check `[tui] argv` and `[binary]`."),
            );
            return outcome;
        }
    };
    *session.visible.test_pane.lock() = Some((pane.clone(), profile_id.clone()));
    let mut guard = Guard { host: &*host, pane: Some(pane.clone()), profile_id: profile_id.clone(), session };
    if env.visible.window {
        host.show(&pane);
    }

    let mut test = Test {
        host: &*host,
        pane,
        profile,
        agent_type: profile_id,
        session,
        limits: env.visible.test.clone(),
        stable_ms: env.visible.capture.stable_ms,
        frozen_ms: env.visible.capture.frozen_ms,
        idle_done_ms: env.visible.capture.idle_done_ms,
        question_ms: env.visible.capture.question_ms,
        cancel,
        project,
        began: Instant::now(),
        status: &session.visible.test_status,
        checks: Vec::new(),
        agent_name: {
            let mut c = session.command.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        },
        expects_startup: session.visible.captures.lock().as_ref().is_some_and(|set| set.has("tui_trust") || set.has("tui_firstrun")),
        startup: parking_lot::Mutex::new(None),
        auto: session.auto_answer.then(|| parking_lot::Mutex::new(Answerer::new("visibletest", AnswerCfg::default()))),
        scope: (session.work_dir.clone(), env.temp_root.clone()),
        answered: parking_lot::Mutex::new(Vec::new()),
    };
    let words = &session.words;
    let task = message_task_for(&words.two);
    let mut dead_end: Option<&'static str> = None;

    let record = |test: &mut Test, outcome: &mut Outcome, name: &'static str, started: Instant, result: Result<Option<String>, Fail>| {
        let ms = started.elapsed().as_millis() as u64;
        let (status, note) = match result {
            Ok(Some(note)) => ("passed", note),
            Ok(None) => ("skipped", "No permission question appeared, so there was nothing to check.".to_string()),
            Err(f) => {
                let note = test.masked(&f.note);
                let screen = test.masked(&f.screen);
                outcome.errors.push(
                    CheckError::new("visibletest", format!("VISIBLETEST_{}_FAILED", name.to_ascii_uppercase()), name, note.clone()).with_screen(&screen),
                );
                if EXTERNAL.is_match(&f.screen) || EXTERNAL.is_match(&f.note) {
                    let line = first_line(&screen);
                    outcome.agent_error = Some(json!({"check": name, "message": line, "external": EXTERNAL.is_match(&f.screen)}));
                }
                outcome.screens.push((name.to_string(), screen));
                ("failed", note)
            }
        };
        let item = CheckResult { name: name_of(name), label: label_of(name), status, note, ms };
        test.checks.push(item.clone());
        outcome.checks.push(item);
        status
    };
    let skipped = |outcome: &mut Outcome, test: &mut Test, name: &'static str, why: &str| {
        let item = CheckResult { name: name_of(name), label: label_of(name), status: "skipped", note: why.to_string(), ms: 0 };
        test.checks.push(item.clone());
        outcome.checks.push(item);
    };

    // 1. starts
    let t = Instant::now();
    let r = test.check_starts().map(Some);
    if record(&mut test, &mut outcome, "starts", t, r) == "failed" {
        dead_end = Some("the agent did not start");
    }
    // 2. ready
    let mut ready_look: Option<Look> = None;
    if dead_end.is_none() {
        let t = Instant::now();
        let limit = test.limits.ready;
        let waited = test.await_ready("ready", limit);
        // the startup question's card check, if a question came up on the way
        let seen = test.startup.lock().take();
        match (seen, test.expects_startup) {
            (Some(Ok(note)), _) => {
                record(&mut test, &mut outcome, "startup", t, Ok(Some(note)));
            }
            (Some(Err(f)), _) => {
                record(&mut test, &mut outcome, "startup", t, Err(f));
            }
            (None, true) => skipped(&mut outcome, &mut test, "startup", "The startup question the capture saw did not appear in the test pane (the agent did not ask again)."),
            (None, false) => skipped(&mut outcome, &mut test, "startup", "No startup question was captured."),
        }
        match waited {
            Ok(look) => {
                ready_look = Some(look);
                record(&mut test, &mut outcome, "ready", t, Ok(Some("The profile reads the screen as the empty input box.".into())));
            }
            Err(f) => {
                record(&mut test, &mut outcome, "ready", t, Err(f));
                dead_end = Some("the agent never reached its input box");
            }
        }
    } else {
        skipped(&mut outcome, &mut test, "startup", "not run: the agent did not start");
    }
    // 3. delivery, 4. busy, 5. done + reply
    let mut turn_ok = false;
    if dead_end.is_none() {
        let baseline = ready_look.as_ref().map(|l| l.watch.clone()).unwrap_or_default();
        let t = Instant::now();
        let r = test.check_delivery(&task).map(Some);
        let delivered = record(&mut test, &mut outcome, "delivery", t, r) == "passed";
        if delivered {
            let t = Instant::now();
            let r = test.check_busy(&words.two).map(Some);
            record(&mut test, &mut outcome, "busy", t, r);
            let t = Instant::now();
            let r = test.check_done_reply(&task, &baseline, &words.two).map(Some);
            turn_ok = record(&mut test, &mut outcome, "done_reply", t, r) == "passed";
        } else {
            skipped(&mut outcome, &mut test, "busy", "not run: the task was not delivered");
            skipped(&mut outcome, &mut test, "done_reply", "not run: the task was not delivered");
        }
    } else {
        for name in ["delivery", "busy", "done_reply"] {
            skipped(&mut outcome, &mut test, name, &format!("not run: {}", dead_end.unwrap_or("an earlier check failed")));
        }
    }
    // 6. permission
    if turn_ok {
        let limit = test.limits.ready;
        let t = Instant::now();
        match test.await_ready("permission", limit) {
            Ok(_) => {
                let r = test.check_permission(&permission_task(words));
                if matches!(r, Ok(Some(_))) {
                    *test.status.lock() = json!({"running": true, "check": "permission", "needs_you": {"message": "A permission question appeared. The wizard answers nothing: it closes this pane, which refuses the request."}, "checks": test.checks, "screen": ""});
                }
                record(&mut test, &mut outcome, "permission", t, r);
            }
            Err(f) => {
                record(&mut test, &mut outcome, "permission", t, Err(f));
            }
        }
    } else {
        skipped(&mut outcome, &mut test, "permission", "not run: the reply check did not pass");
    }
    // 7. exit
    let t = Instant::now();
    let r = test.check_exit().map(Some);
    record(&mut test, &mut outcome, "exit", t, r);
    guard.pane = None;
    drop(guard);
    outcome.answered = test.answered.lock().clone();
    outcome.answered_json = test.answered_json();
    *session.visible.test_answered.lock() = outcome.answered.clone();
    *session.visible.test_status.lock() = json!({"running": false, "checks": outcome.checks, "answered": test.answered_json()});
    outcome
}

pub fn outcome_json(outcome: &Outcome, state: State, version: Option<&str>) -> Value {
    json!({
        "ok": outcome.ok(),
        "state": state.as_str(),
        "checks": outcome.checks,
        "errors": outcome.errors,
        "text": render(&outcome.errors, ReportSource::VisibleTest),
        "version": version,
        "passed": outcome.passed(),
        "total": outcome.total(),
        "agent_error": outcome.agent_error,
        "answered": outcome.answered_json,
    })
}

/// `POST /agents/wizard/visibletest`.
pub fn run(wizard: &Wizard, body: &Value) -> Result<Value, WizardError> {
    let id = super::super::api::session_id(body)?;
    if body.get("confirm").and_then(Value::as_bool) != Some(true) {
        return Err(WizardError::new(
            400,
            "WIZARD_CONFIRM_REQUIRED",
            "the visible test uses your plan (two small messages); send confirm: true after the person agrees",
        ));
    }
    let given = body.get("plan").and_then(Value::as_str).map(str::trim).filter(|p| !p.is_empty());
    let Some(given) = given else {
        return Err(WizardError::new(
            400,
            "WIZARD_PLAN_REQUIRED",
            "send the fingerprint of the command (the `planned_visible.fingerprint` that verify returned) that the person saw",
        ));
    };
    let snap = wizard.begin(&id, "visibletest", &[State::Probed, State::Verified, State::Tested])?;
    let env = wizard.env();
    let Some(draft) = snap.draft.clone() else {
        wizard.release(&id);
        return Err(WizardError::new(409, "WIZARD_BAD_STATE", "there is no draft the visible test may run; call verify first"));
    };
    let current = parse_definition(&draft.text).map_err(|e| e.to_string()).and_then(|def| plan(&snap, &def));
    match current {
        Ok(p) if p.fingerprint == given => {}
        Ok(_) => {
            wizard.release(&id);
            return Err(WizardError::new(
                409,
                "WIZARD_PLAN_MISMATCH",
                "the command the visible test would start is not the one the person was shown; show it again and ask for a new confirmation",
            ));
        }
        Err(message) => {
            wizard.release(&id);
            return Err(WizardError::new(409, "WIZARD_BINARY_UNRESOLVED", message));
        }
    }
    let mut outcome = execute(&snap, &env, &draft, &snap.cancel);
    // The shared rules and the screen fixtures too, so one reply carries every problem and a draft that
    // fails them can never be called tested (the headless live test does the same).
    let checks = super::super::verify::check_draft_with(&snap, &env, &draft.text, &draft.expects);
    outcome.errors.extend(checks.errors.into_iter().filter(|e| e.stage == "conformance" || e.stage == "fixture"));
    let ok = outcome.ok() && !snap.cancel.load(Ordering::SeqCst);
    let (passed, total) = (outcome.passed(), outcome.total());
    let sha = draft.sha256.clone();
    let version = snap.version.number.clone();
    let screens = outcome.screens.clone();
    let state = wizard.end(&id, |s| {
        *s.visible.test_samples.lock() = screens;
        if ok {
            s.visible_tested = Some(VisibleTested { draft_sha256: sha, passed, total, version: version.clone() });
            super::super::session::settle(s);
        }
        s.state
    })?;
    Ok(outcome_json(&outcome, state, snap.version.number.as_deref()))
}
