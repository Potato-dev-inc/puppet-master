//! Night Shift: the views the UI and the bridge read (SPEC n32 3.8 and 3.10). Pure: no I/O, no clock.
//!
//! Every string goes through `cut_paths` first (strict redaction, e-mails masked, paths cut to their last
//! names), so no secret and no absolute path reaches a view. A task is shown by its first line, cut to
//! `TITLE_CHARS`. `NightView::redacted_for_bridge` drops what the phone never shows: project names, pane ids,
//! turn sentences and agent questions. It also cuts event text to `EVENT_BRIDGE_CHARS`.
//!
//! Camel-case JSON, because the UI reads these shapes as they are.

use serde::Serialize;

use super::model::{Attempt, AttemptEnd, Class, Counts, Event, NightRun, Review, RunState, Task, TaskState};
use super::report;
use crate::redact::RedactContext;
use crate::session_chat::export_text::{cut_paths, folder_of};

pub const TITLE_CHARS: usize = 80;
pub const SENTENCE_CHARS: usize = 600;
pub const EVENT_BRIDGE_CHARS: usize = 80;
pub const MAX_SUMMARIES: usize = 8;
pub const MAX_CHANGE_LINES: usize = 20;
const MS_PER_MIN: u64 = 60_000;

/// The Morning page and the Tonight header for one run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NightView {
    pub id: String,
    pub rev: u64,
    /// "scheduled" | "running" | "done" | "stopped"
    pub state: String,
    /// The summary line, e.g. "4 of 5 done, tests green. 1 needs you."
    pub line: String,
    pub total: u32,
    pub done: u32,
    pub green: u32,
    pub needs_you: u32,
    pub didnt_run: u32,
    pub no_changes: u32,
    /// How many tasks "Keep all that pass" would keep.
    pub keep_all: u32,
    /// Local start time, "23:10".
    pub from: String,
    /// Local finish time, "05:22", once the run has finished.
    pub to: Option<String>,
    pub turns: u32,
    /// "6 h 12 min", or "under a minute".
    pub duration: String,
    pub by_plan: Vec<PlanCount>,
    pub tasks: Vec<TaskView>,
    pub events: Vec<EventView>,
}

/// How many tasks ended on one plan (by the label of their last try).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanCount {
    pub label: String,
    pub tasks: u32,
}

/// One task row on the Morning page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub id: String,
    /// The first line of the person's text, cut to 80 characters.
    pub title: String,
    /// The project's folder name only. None on the bridge.
    pub project: Option<String>,
    /// The label of the last try's plan, e.g. "Codex".
    pub plan: Option<String>,
    /// "queued" | "waiting" | "working" | "finished"
    pub state: String,
    pub state_text: String,
    /// Local time the wait ends, "01:10", when known.
    pub until: Option<String>,
    /// "ready" | "no_checks" | "needs_look" | "no_changes" | "didnt_run"; None until the task is classified.
    pub class: Option<String>,
    pub headline: Option<String>,
    pub reason: Option<String>,
    pub tests: Option<Counts>,
    pub tests_before: Option<Counts>,
    pub test_change_lines: Vec<String>,
    pub files: Option<FilesView>,
    pub turns: u32,
    pub duration: Option<String>,
    pub summaries: Vec<String>,
    pub attempts: Vec<AttemptView>,
    /// "pending" | "kept" | "discarded" | "keep_failed"
    pub review: String,
    pub review_text: Option<String>,
    /// The branch a kept copy merged into.
    pub into: Option<String>,
    pub can_keep: bool,
    pub can_throw_away: bool,
    /// The pane of the last try, for "Open its terminal". None on the bridge.
    pub pane_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesView {
    pub count: u32,
    pub added: u32,
    pub removed: u32,
}

/// One try of a task, for "Tries" on the row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptView {
    pub n: u8,
    pub label: String,
    /// "running" | "completed" | "out_of_usage" | "gate" | "asked" | "needs_you" | "failed" | "cut_off" | "stopped_by_person"
    pub end: String,
    /// The plain words for the end, when there are some. None on the bridge.
    pub sentence: Option<String>,
    pub started: String,
    pub ended: Option<String>,
}

/// One line of "The night" timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventView {
    pub at: String,
    pub kind: String,
    pub glyph: String,
    pub text: String,
    pub task: Option<String>,
}

/// One installed plan (agent x login) as the Tonight sheet lists it. Built by the plan lister, not here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    pub key: String,
    pub label: String,
    pub ready: bool,
    pub why_not: Option<String>,
    pub out_until: Option<String>,
    /// Projects where a finished run had a copy of this plan. A hint, never a promise.
    pub tried_here: std::collections::BTreeMap<String, bool>,
}

/// The view of a run for the UI. `offset_min` is the local offset east of UTC in minutes.
pub fn view_of(run: &NightRun, ctx: &RedactContext, offset_min: i32) -> NightView {
    let summary = report::summary(run);
    let tasks: Vec<TaskView> = run.tasks.iter().map(|task| task_view(task, ctx, offset_min)).collect();

    let mut by_plan: Vec<PlanCount> = Vec::new();
    for label in tasks.iter().filter_map(|task| task.plan.as_ref()) {
        match by_plan.iter_mut().find(|plan| &plan.label == label) {
            Some(plan) => plan.tasks += 1,
            None => by_plan.push(PlanCount { label: label.clone(), tasks: 1 }),
        }
    }

    let (turns, minutes) = run
        .tasks
        .iter()
        .filter_map(|task| task.outcome.as_ref())
        .fold((0u32, 0u32), |(turns, minutes), outcome| (turns + outcome.turns, minutes + outcome.minutes));

    NightView {
        id: run.id.clone(),
        rev: run.rev,
        state: state_tag(&run.state).to_string(),
        line: report::line(&summary),
        total: summary.total,
        done: summary.done,
        green: summary.green,
        needs_you: summary.needs_you,
        didnt_run: summary.didnt_run,
        no_changes: summary.no_changes,
        keep_all: tasks.iter().filter(|task| task.can_keep).count() as u32,
        from: report::hhmm(run.start_at_ms, offset_min),
        to: run.finished_ms.map(|ms| report::hhmm(ms, offset_min)),
        turns,
        duration: report::duration_text(u64::from(minutes) * MS_PER_MIN),
        by_plan,
        tasks,
        events: run.events.iter().map(|event| event_view(event, ctx, offset_min)).collect(),
    }
}

impl NightView {
    /// The same view for `GET /night`: no project names, no pane ids, no turn sentences, no agent
    /// questions, titles already cut to 80 characters, event text cut to 80.
    pub fn redacted_for_bridge(&self) -> NightView {
        let mut view = self.clone();
        for task in &mut view.tasks {
            task.project = None;
            task.pane_id = None;
            task.summaries.clear();
            task.test_change_lines.clear();
            for attempt in &mut task.attempts {
                attempt.sentence = None;
            }
        }
        for event in &mut view.events {
            event.text = cut_chars(&event.text, EVENT_BRIDGE_CHARS);
        }
        view
    }
}

/// The title of a task: the first non-empty line of its text, redacted, paths cut, cut to 80 characters.
pub fn title_of(raw: &str, ctx: &RedactContext) -> String {
    let cleaned = clean(raw, ctx);
    let first = cleaned.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    cut_chars(first, TITLE_CHARS)
}

/// Plain text that is safe to show: redacted (strict), e-mails masked, paths cut to their last names.
/// Not escaped: the JSON views keep plain text, and the page escapes it itself.
pub fn clean(raw: &str, ctx: &RedactContext) -> String {
    cut_paths(raw, ctx)
}

/// An agent's sentence or a question: `clean`, trimmed and cut to `SENTENCE_CHARS`.
pub fn turn_text(raw: &str, ctx: &RedactContext) -> String {
    cut_chars(clean(raw, ctx).trim(), SENTENCE_CHARS)
}

/// The first `max` characters of `text`; when cut, the last one is an ellipsis, so the result is `max` characters.
pub fn cut_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn task_view(task: &Task, ctx: &RedactContext, offset_min: i32) -> TaskView {
    let outcome = task.outcome.as_ref();
    let last = task.attempts.last();
    let (state, state_text, until) = state_parts(&task.state, ctx, offset_min);
    let (review, review_text, into) = review_parts(&task.review, ctx);
    let can_keep = matches!(task.review, Review::Pending)
        && outcome.is_some_and(|o| {
            o.copy.is_some() && matches!(o.class, Class::Ready | Class::NoChecks | Class::NeedsLook)
        });
    TaskView {
        id: task.spec.id.clone(),
        title: title_of(&task.spec.text, ctx),
        project: project_name(&task.spec.project, ctx),
        plan: last.map(|a| clean(&a.label, ctx)).filter(|label| !label.is_empty()),
        state,
        state_text,
        until,
        class: outcome.map(|o| class_tag(o.class).to_string()),
        headline: outcome.map(|o| clean(&o.headline, ctx)),
        reason: outcome.and_then(|o| o.reason.as_deref()).map(|reason| clean(reason, ctx)),
        tests: outcome.and_then(|o| o.tests),
        tests_before: outcome.and_then(|o| o.tests_before),
        test_change_lines: outcome.map_or(Vec::new(), |o| {
            o.test_change_lines.iter().take(MAX_CHANGE_LINES).map(|line| clean(line, ctx)).collect()
        }),
        files: outcome.and_then(|o| o.files).map(|(count, added, removed)| FilesView { count, added, removed }),
        turns: outcome.map_or(0, |o| o.turns),
        duration: outcome.map(|o| report::duration_text(u64::from(o.minutes) * MS_PER_MIN)),
        summaries: outcome.map_or(Vec::new(), |o| {
            o.summaries.iter().take(MAX_SUMMARIES).map(|s| turn_text(s, ctx)).collect()
        }),
        attempts: task.attempts.iter().map(|attempt| attempt_view(attempt, ctx, offset_min)).collect(),
        review,
        review_text,
        into,
        can_keep,
        can_throw_away: matches!(task.review, Review::Pending) && matches!(task.state, TaskState::Finished),
        pane_id: last.and_then(|a| a.pane_ids.first().cloned()),
    }
}

/// The folder name of the project, never the home folder's name. None when there is no name.
fn project_name(project: &str, ctx: &RedactContext) -> Option<String> {
    let name = clean(&folder_of(project, ctx.home.as_deref()), ctx);
    (!name.is_empty()).then_some(name)
}

fn state_parts(state: &TaskState, ctx: &RedactContext, offset_min: i32) -> (String, String, Option<String>) {
    match state {
        TaskState::Queued => ("queued".into(), "In the queue".into(), None),
        TaskState::Waiting { until_ms, why } => (
            "waiting".into(),
            format!("Waiting: {}", clean(why, ctx)),
            until_ms.map(|ms| report::hhmm(ms, offset_min)),
        ),
        TaskState::Working => ("working".into(), "Working".into(), None),
        TaskState::Finished => ("finished".into(), "Finished".into(), None),
    }
}

fn review_parts(review: &Review, ctx: &RedactContext) -> (String, Option<String>, Option<String>) {
    match review {
        Review::Pending => ("pending".into(), None, None),
        Review::Kept { into, .. } => {
            let into = clean(into, ctx);
            (
                "kept".into(),
                Some(format!("Kept · merged into {into}")),
                Some(into),
            )
        }
        Review::Discarded { .. } => ("discarded".into(), Some("Thrown away".into()), None),
        Review::KeepFailed { sentence, .. } => ("keep_failed".into(), Some(clean(sentence, ctx)), None),
    }
}

fn attempt_view(attempt: &Attempt, ctx: &RedactContext, offset_min: i32) -> AttemptView {
    let (end, sentence) = end_parts(attempt.end.as_ref(), ctx);
    AttemptView {
        n: attempt.n,
        label: clean(&attempt.label, ctx),
        end,
        sentence,
        started: report::hhmm(attempt.started_ms, offset_min),
        ended: attempt.ended_ms.map(|ms| report::hhmm(ms, offset_min)),
    }
}

fn end_parts(end: Option<&AttemptEnd>, ctx: &RedactContext) -> (String, Option<String>) {
    let Some(end) = end else { return ("running".into(), None) };
    match end {
        AttemptEnd::Completed => ("completed".into(), None),
        AttemptEnd::OutOfUsage { .. } => ("out_of_usage".into(), None),
        AttemptEnd::Gate { sentence, .. } => ("gate".into(), Some(turn_text(sentence, ctx))),
        AttemptEnd::Asked { question } => ("asked".into(), Some(turn_text(question, ctx))),
        AttemptEnd::NeedsYou { sentence, .. } => ("needs_you".into(), Some(turn_text(sentence, ctx))),
        AttemptEnd::Failed { sentence, .. } => ("failed".into(), Some(turn_text(sentence, ctx))),
        AttemptEnd::CutOff => ("cut_off".into(), None),
        AttemptEnd::StoppedByPerson => ("stopped_by_person".into(), None),
    }
}

fn event_view(event: &Event, ctx: &RedactContext, offset_min: i32) -> EventView {
    EventView {
        at: report::hhmm(event.at_ms, offset_min),
        kind: event.kind.clone(),
        glyph: event.glyph.clone(),
        text: turn_text(&event.text, ctx),
        task: event.task.clone(),
    }
}

fn class_tag(class: Class) -> &'static str {
    match class {
        Class::Ready => "ready",
        Class::NoChecks => "no_checks",
        Class::NeedsLook => "needs_look",
        Class::NoChanges => "no_changes",
        Class::DidntRun => "didnt_run",
    }
}

fn state_tag(state: &RunState) -> &'static str {
    match state {
        RunState::Scheduled { .. } => "scheduled",
        RunState::Running => "running",
        RunState::Done => "done",
        RunState::Stopped => "stopped",
    }
}

/// A fixed run for the view and page tests: one Ready task, one needs-a-look task, one that didn't run.
#[cfg(test)]
pub fn morning_fixture() -> NightRun {
    use super::model::{sample_run, sample_task, AttemptKind, CopyRef, Outcome};
    use crate::missions::model::AgentChoice;

    let codex = AgentChoice { agent_type: "codex".into(), account: None, model: None };
    let attempt = |pane: &str, end: AttemptEnd| Attempt {
        n: 1,
        plan: codex.clone(),
        label: "Codex".into(),
        kind: AttemptKind::Single,
        mission_id: None,
        handle: Some("run-1".into()),
        pane_ids: vec![pane.into()],
        started_ms: 3_600_000,
        ended_ms: Some(7_200_000),
        end: Some(end),
    };
    let copy = |pane: &str| CopyRef {
        project: "C:\\Users\\you\\work\\app".into(),
        handle: "run-1".into(),
        branch: "ns/nt-00000001".into(),
        base: "main".into(),
        pane_id: Some(pane.into()),
    };
    let counts = |passed, total| Some(Counts { passed, total });

    let mut ready = sample_task("nt-00000001");
    ready.spec.text = "Fix the cart total rounding.".into();
    ready.spec.project = "C:\\Users\\you\\work\\app".into();
    ready.state = TaskState::Finished;
    ready.attempts = vec![attempt("pane-42", AttemptEnd::Completed)];
    ready.outcome = Some(Outcome {
        class: Class::Ready,
        headline: "Tests pass 42 of 42".into(),
        reason: None,
        tests: counts(42, 42),
        tests_before: counts(40, 42),
        checks_declared: true,
        test_change_lines: Vec::new(),
        files: Some((5, 120, 34)),
        turns: 3,
        minutes: 38,
        summaries: vec!["Fixed the rounding in cart.ts".into()],
        copy: Some(copy("pane-42")),
    });

    let mut needs = sample_task("nt-00000002");
    needs.spec.text = "Rename the config loader.\nThe second line is not the title.".into();
    needs.spec.project = "C:\\Users\\you\\work\\docs".into();
    needs.state = TaskState::Finished;
    needs.attempts = vec![attempt(
        "pane-43",
        AttemptEnd::Gate { code: "WORKSPACE_TRUST_DECLINED".into(), sentence: "Codex asked to trust the folder.".into() },
    )];
    needs.outcome = Some(Outcome {
        class: Class::NeedsLook,
        headline: "Tests fail: 9 of 12 pass".into(),
        reason: Some("Look at the failing tests before you keep it.".into()),
        tests: counts(9, 12),
        tests_before: counts(7, 12),
        checks_declared: true,
        test_change_lines: vec!["Changed tests/cart.test.ts".into()],
        files: Some((2, 14, 3)),
        turns: 12,
        minutes: 90,
        summaries: vec!["<script>alert(1)</script> at C:\\Users\\you\\work\\app\\src\\lib.rs".into()],
        copy: Some(copy("pane-43")),
    });

    let mut skipped = sample_task("nt-00000003");
    skipped.spec.text = "Fix broken links in /guides".into();
    skipped.spec.project = "D:\\pm-x\\docs-site".into();
    skipped.state = TaskState::Finished;
    skipped.outcome = Some(Outcome {
        class: Class::DidntRun,
        headline: "Didn't run: no time left before 07:00.".into(),
        reason: None,
        tests: None,
        tests_before: None,
        checks_declared: false,
        test_change_lines: Vec::new(),
        files: None,
        turns: 0,
        minutes: 0,
        summaries: Vec::new(),
        copy: None,
    });

    let mut run = sample_run("ns-0123456789ab");
    run.tasks = vec![ready, needs, skipped];
    run.start_at_ms = 3_600_000;
    run.finished_ms = Some(7_200_000);
    run.events = vec![Event {
        at_ms: 3_600_000,
        kind: "started".into(),
        glyph: "~".into(),
        text: "Started in C:\\Users\\you\\work\\app".into(),
        task: None,
    }];
    run
}

#[cfg(test)]
#[path = "view_tests.rs"]
mod tests;
