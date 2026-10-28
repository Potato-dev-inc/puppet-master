//! Tauri commands for Night Shift (SPEC n32 3.8). Desktop only: the person starts, stops, keeps and
//! throws away from here. Nothing in this file is reachable from MCP, the bridge or a paired phone.
//!
//! Every command runs off the UI thread and answers with an `OperationError` on refusal: a code the
//! sheet can match and one plain sentence to show. The Start checks are in `start.rs`.

mod start;

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

use super::AppState;
use crate::audit;
use crate::missions::store::now_ms;
use crate::night_shift::model::{plan_key, valid_hhmm, valid_task_id, NightRun, RunState, Settings};
use crate::night_shift::review::{self, Live};
use crate::night_shift::store::Store;
use crate::night_shift::view::{self, NightView, PlanView};
use crate::night_shift::{checks, page, report, runner};
use crate::operations::OperationError;
use crate::project_path::comparable_project_path;
use crate::redact;
use crate::secure_file::write_private;
use crate::session_chat::export_save::{check_dest, PageKind};
use start::{Env, StartInput};

/// One Start, Stop or Keep at a time, so two clicks cannot make two runs.
static START: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
/// The event the desktop listens to (`lib/night.ts` `useNight`).
const NIGHT_EVENT: &str = "night://changed";
/// The name the audit trail gives the person for these desktop commands.
const WHO: &str = "you";

/// What Tonight and Morning read when they open.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NightStateView {
    pub enabled: bool,
    /// The current run, or the last one that finished.
    pub run: Option<NightView>,
    pub can_start: bool,
    /// Why Start is off right now, in plain words.
    pub start_blocked: Option<String>,
    pub history: Vec<NightHistoryRow>,
    pub settings: NightSettingsView,
}

/// One finished run in the history list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NightHistoryRow {
    pub id: String,
    pub line: String,
}

/// The Tonight settings, as the sheet reads and writes them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NightSettingsView {
    pub plans: Vec<String>,
    pub stop_hhmm: String,
    pub parallel: u8,
    pub turns: u32,
    pub minutes: u32,
    pub awake_asked: bool,
}

impl From<Settings> for NightSettingsView {
    fn from(settings: Settings) -> Self {
        NightSettingsView {
            plans: settings.plans,
            stop_hhmm: settings.stop_hhmm,
            parallel: settings.parallel,
            turns: settings.turns,
            minutes: settings.minutes,
            awake_asked: settings.awake_asked,
        }
    }
}

impl From<NightSettingsView> for Settings {
    fn from(view: NightSettingsView) -> Self {
        Settings {
            plans: view.plans,
            stop_hhmm: view.stop_hhmm,
            parallel: view.parallel,
            turns: view.turns,
            minutes: view.minutes,
            awake_asked: view.awake_asked,
        }
    }
}

/// One declared check of a project, as the Tests popover shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NightCheckView {
    pub name: String,
    pub argv: Vec<String>,
    pub timeout_min: u32,
}

/// A check Puppet Master found in the project. Only offered: nothing is used until the person saves it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NightSuggestionView {
    pub name: String,
    pub argv: Vec<String>,
    pub source: String,
}

/// The Tests popover for one project.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NightChecksView {
    pub declared: Vec<NightCheckView>,
    pub suggestions: Vec<NightSuggestionView>,
    /// Plain notes, e.g. a relative script an agent could edit.
    pub warnings: Vec<String>,
    /// Always `None` in v1: the baseline is not run before a night.
    pub baseline_red: Option<bool>,
}

/// One check as the person typed it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NightCheckInput {
    pub name: String,
    pub command: String,
    pub timeout_min: u32,
}

#[cfg(not(test))]
fn store() -> Store {
    Store::live()
}

#[cfg(test)]
fn store() -> Store {
    Store::at(std::env::temp_dir().join("pm-ns-commands-unused"))
}

fn now() -> u64 {
    now_ms()
}

/// Minutes east of UTC on this computer's clock.
fn offset_min() -> i32 {
    chrono::Local::now().offset().local_minus_utc() / 60
}

/// The Labs switch (`night_shift_enabled`, off unless the person turned it on).
fn labs_on(public: &Value) -> bool {
    public
        .get("night_shift_enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Undo is on for this computer and git is there, so a copy can be rolled back.
fn undo_on(public: &Value) -> bool {
    public
        .get("checkpoints_enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true)
        && crate::checkpoints::git_available()
}

fn is_unfinished(run: &NightRun) -> bool {
    matches!(run.state, RunState::Scheduled { .. } | RunState::Running)
}

/// The current run, or the last one. A refusal in plain words when there is none.
fn current(store: &Store) -> Result<NightRun, OperationError> {
    store
        .load()
        .ok_or_else(|| OperationError::new("NO_NIGHT", "No Night Shift yet. Plan tonight from the + menu.", false))
}

fn view_of_run(run: &NightRun) -> NightView {
    let ctx = redact::known::context();
    view::view_of(run, &ctx, offset_min())
}

fn save(store: &Store, run: &mut NightRun) -> Result<(), OperationError> {
    store
        .save(run)
        .map_err(|message| OperationError::new("SAVE_FAILED", message, true))
}

/// Tell the desktop the run changed (the same event the runner sends).
fn changed(app: &AppHandle, run: &NightRun) {
    let state = serde_json::to_value(&run.state).unwrap_or(Value::Null);
    let _ = app.emit(NIGHT_EVENT, json!({ "rev": run.rev, "state": state }));
}

fn check_task_id(task_id: &str) -> Result<(), OperationError> {
    if valid_task_id(task_id) {
        Ok(())
    } else {
        Err(OperationError::new(
            "NO_SUCH_TASK",
            "That task is not in tonight's run.",
            false,
        ))
    }
}

/// Runs a command body off the UI thread.
async fn off_thread<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, OperationError> + Send + 'static,
) -> Result<T, OperationError> {
    match tauri::async_runtime::spawn_blocking(job).await {
        Ok(result) => result,
        Err(error) => Err(OperationError::new("INTERNAL", error.to_string(), true)),
    }
}

/// The supervisor thread for a run, unless one already runs for it in this process.
#[cfg(not(test))]
fn supervise(run_id: &str) {
    let _ = runner::spawn(crate::night_shift::io::live(run_id), run_id.to_string());
}

#[cfg(test)]
fn supervise(_run_id: &str) {}

/// Whether a project folder has a commit (what a copy needs). Plain check, no output.
fn has_commit(key: &str) -> bool {
    Path::new(key).is_dir()
        && crate::proc_util::hidden_command("git")
            .args(["rev-parse", "--verify", "-q", "HEAD"])
            .current_dir(key)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
}

fn out_until(limited: Option<Option<u64>>, offset: i32) -> Option<String> {
    match limited {
        None => None,
        Some(Some(at)) => Some(format!("Out of usage until {}", report::hhmm(at, offset))),
        Some(None) => Some("Out of usage. No reset time was shown.".to_string()),
    }
}

/// A hint only: whether a run of this night (the current one) had a try with this plan in the project.
fn tried_here(run: Option<&NightRun>, key: &str, project: &str) -> bool {
    let project = comparable_project_path(Path::new(project.trim()));
    run.is_some_and(|run| {
        run.tasks.iter().any(|task| {
            task.spec.project == project && task.attempts.iter().any(|attempt| plan_key(&attempt.plan) == key)
        })
    })
}

fn plans_now(projects: &[String]) -> Vec<PlanView> {
    let run = store().load();
    let offset = offset_min();
    start::known_plans(projects)
        .into_iter()
        .map(|known| PlanView {
            key: known.key.clone(),
            label: known.label.clone(),
            ready: known.runnable.is_ok(),
            why_not: known.runnable.clone().err(),
            out_until: out_until(known.limited_until, offset),
            tried_here: projects
                .iter()
                .map(|project| (project.clone(), tried_here(run.as_ref(), &known.key, project)))
                .collect::<BTreeMap<_, _>>(),
        })
        .collect()
}

fn check_view(check: &checks::Declared) -> NightCheckView {
    NightCheckView {
        name: check.name.clone(),
        argv: check.argv.clone(),
        timeout_min: check.timeout_min,
    }
}

fn checks_now(project: &str) -> NightChecksView {
    let store = store();
    let declared = checks::declared(&store, project);
    let warnings = declared
        .iter()
        .flat_map(|check| checks::warnings(Path::new(project), check))
        .collect();
    NightChecksView {
        declared: declared.iter().map(check_view).collect(),
        suggestions: checks::suggestions(Path::new(project))
            .into_iter()
            .map(|found| NightSuggestionView {
                name: found.name,
                argv: found.argv,
                source: found.source,
            })
            .collect(),
        warnings,
        baseline_red: None,
    }
}

fn state_now(enabled: bool) -> NightStateView {
    let store = store();
    let run = store.load();
    let start_blocked = if !enabled {
        Some(start::LABS_OFF_SENTENCE)
    } else if run.as_ref().is_some_and(is_unfinished) {
        Some(start::BUSY_SENTENCE)
    } else {
        None
    };
    NightStateView {
        enabled,
        run: run.as_ref().map(view_of_run),
        can_start: start_blocked.is_none(),
        start_blocked: start_blocked.map(str::to_string),
        history: store
            .history()
            .into_iter()
            .map(|row| NightHistoryRow {
                id: row.id,
                line: row.line,
            })
            .collect(),
        settings: store.settings().into(),
    }
}

/// Builds and saves the run, then starts its supervisor. The checks and plans are read here, once.
fn start_now(app: &AppHandle, input: StartInput, public: &Value) -> Result<NightView, OperationError> {
    let _one_at_a_time = START.lock();
    let store = store();
    let saved = store.settings();
    let folders: Vec<String> = input.tasks.iter().map(|task| task.project.trim().to_string()).collect();
    let plans = start::known_plans(&folders);
    let busy = store.load().is_some_and(|run| is_unfinished(&run));
    let key_of = |raw: &str| comparable_project_path(Path::new(raw));
    let is_git = |key: &str| has_commit(key);
    let declared = |key: &str| -> Vec<Vec<String>> {
        checks::declared(&store, key)
            .into_iter()
            .map(|check| check.argv)
            .collect()
    };
    let env = Env {
        now_ms: now(),
        enabled: labs_on(public),
        undo_on: undo_on(public),
        awake_asked: saved.awake_asked,
        busy,
        plans: &plans,
        key_of: &key_of,
        is_git: &is_git,
        declared: &declared,
    };
    let mut run = start::build(&input, &env)?;
    save(&store, &mut run)?;
    // The sheet opens with the same plans, stop time and parallel next time.
    let _ = store.save_settings(&Settings {
        plans: input.plans.clone(),
        stop_hhmm: input.stop_hhmm.clone(),
        parallel: run.parallel,
        ..saved
    });
    supervise(&run.id);
    changed(app, &run);
    Ok(view_of_run(&run))
}

fn stop_now(app: &AppHandle) -> Result<NightView, OperationError> {
    let store = store();
    let run = current(&store)?;
    if is_unfinished(&run) {
        runner::request_stop(&run.id);
        // A supervisor that is not running (after a restart) takes the stop and finishes the run.
        supervise(&run.id);
        changed(app, &run);
    }
    Ok(view_of_run(&run))
}

#[tauri::command]
pub async fn night_state(app: AppHandle) -> Result<NightStateView, OperationError> {
    let enabled = labs_on(&crate::settings_store::read_public_settings(&app));
    off_thread(move || Ok(state_now(enabled))).await
}

#[tauri::command]
pub async fn night_plans(projects: Vec<String>) -> Result<Vec<PlanView>, OperationError> {
    off_thread(move || Ok(plans_now(&projects))).await
}

#[tauri::command]
pub async fn night_checks_get(project: String) -> Result<NightChecksView, OperationError> {
    off_thread(move || Ok(checks_now(&project))).await
}

/// Saves the project's declared checks (the whole list). Each command is parsed as argv first.
#[tauri::command]
pub async fn night_checks_set(
    project: String,
    list: Vec<NightCheckInput>,
) -> Result<Vec<NightCheckView>, OperationError> {
    off_thread(move || {
        let store = store();
        let mut declared = Vec::new();
        for item in list {
            let argv = checks::parse_command(&item.command)
                .map_err(|message| OperationError::new("BAD_CHECK", message, true))?;
            declared.push(checks::Declared {
                name: item.name.trim().to_string(),
                argv,
                timeout_min: item.timeout_min,
                declared_ms: 0,
            });
        }
        checks::declare(&store, &project, declared)
            .map_err(|message| OperationError::new("BAD_CHECK", message, true))?;
        Ok(checks::declared(&store, &project).iter().map(check_view).collect())
    })
    .await
}

#[tauri::command]
pub async fn night_start(app: AppHandle, input: StartInput) -> Result<NightView, OperationError> {
    let public = crate::settings_store::read_public_settings(&app);
    off_thread(move || start_now(&app, input, &public)).await
}

#[tauri::command]
pub async fn night_stop(app: AppHandle) -> Result<NightView, OperationError> {
    off_thread(move || stop_now(&app)).await
}

#[tauri::command]
pub async fn night_keep(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
) -> Result<NightView, OperationError> {
    let registry = state.registry.clone();
    off_thread(move || {
        check_task_id(&task_id)?;
        let store = store();
        review::keep_with(&store, &Live { registry: &registry }, &task_id, WHO, now())?;
        let run = current(&store)?;
        changed(&app, &run);
        Ok(view_of_run(&run))
    })
    .await
}

#[tauri::command]
pub async fn night_throw_away(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
) -> Result<NightView, OperationError> {
    let registry = state.registry.clone();
    off_thread(move || {
        check_task_id(&task_id)?;
        let store = store();
        review::throw_away_with(&store, &Live { registry: &registry }, &task_id, WHO, now())?;
        let run = current(&store)?;
        changed(&app, &run);
        Ok(view_of_run(&run))
    })
    .await
}

#[tauri::command]
pub async fn night_keep_all_ready(app: AppHandle, state: State<'_, AppState>) -> Result<NightView, OperationError> {
    let registry = state.registry.clone();
    off_thread(move || {
        let store = store();
        // Each result is in the record already (kept, or "conflicts"); the view shows them.
        let _ = review::keep_all_ready_with(&store, &Live { registry: &registry }, WHO, now());
        let run = current(&store)?;
        changed(&app, &run);
        Ok(view_of_run(&run))
    })
    .await
}

/// The person opened the Morning page: the held morning push is not sent.
#[tauri::command]
pub async fn night_seen(app: AppHandle) -> Result<(), OperationError> {
    off_thread(move || {
        let store = store();
        let Some(mut run) = store.load() else { return Ok(()) };
        if run.seen_ms.is_none() {
            run.seen_ms = Some(now());
            save(&store, &mut run)?;
            changed(&app, &run);
        }
        Ok(())
    })
    .await
}

/// The saved morning page (HTML) for the current or last run.
#[tauri::command]
pub async fn night_report_html() -> Result<String, OperationError> {
    off_thread(move || {
        let run = current(&store())?;
        Ok(page::page(&run, &redact::known::context()))
    })
    .await
}

/// Writes the morning page to a file the person picked. Returns the path it was written to.
#[tauri::command]
pub async fn night_report_save(path: String) -> Result<String, OperationError> {
    off_thread(move || {
        let run = current(&store())?;
        let dest =
            check_dest(&path, PageKind::Html).map_err(|message| OperationError::new("BAD_PATH", message, true))?;
        let html = page::page(&run, &redact::known::context());
        write_private(&dest, &html).map_err(|message| OperationError::new("SAVE_FAILED", message, true))?;
        Ok(dest.to_string_lossy().into_owned())
    })
    .await
}

#[tauri::command]
pub async fn night_settings_get() -> Result<NightSettingsView, OperationError> {
    off_thread(move || Ok(store().settings().into())).await
}

#[tauri::command]
pub async fn night_settings_set(settings: NightSettingsView) -> Result<NightSettingsView, OperationError> {
    off_thread(move || {
        if !valid_hhmm(&settings.stop_hhmm) {
            return Err(OperationError::new("BAD_TIME", "Pick a time like 07:00.", true));
        }
        let clean: Settings = settings.into();
        let saved = store()
            .save_settings(&clean)
            .map_err(|message| OperationError::new("SAVE_FAILED", message, true))?;
        audit::record(
            audit::Kind::NightShift,
            audit::Who::Desktop,
            "Changed Night Shift settings",
            json!({}),
        );
        Ok(saved.into())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_sheet_sends_camel_case_fields() {
        let input: StartInput = serde_json::from_value(json!({
            "tasks": [{ "project": "/work/app", "text": "Fix the cart.", "who": null, "mayChangeTests": false, "turns": 12, "minutes": 90 }],
            "plans": ["fake_a"],
            "stopHhmm": "07:00",
            "startHhmm": null,
            "parallel": 2,
            "keepAwake": false
        }))
        .expect("the sheet's shape");
        assert_eq!(input.tasks[0].text, "Fix the cart.");
        assert_eq!(input.stop_hhmm, "07:00");
        assert!(input.start_hhmm.is_none());
    }

    #[test]
    fn a_task_without_turns_or_minutes_gets_the_defaults() {
        let input: StartInput = serde_json::from_value(json!({
            "tasks": [{ "project": "/work/app", "text": "Fix." }],
            "plans": ["fake_a"],
            "stopHhmm": "07:00",
            "parallel": 1,
            "keepAwake": false
        }))
        .expect("defaults");
        assert_eq!(input.tasks[0].turns, crate::night_shift::model::DEFAULT_TURNS);
        assert_eq!(input.tasks[0].minutes, crate::night_shift::model::DEFAULT_MINUTES);
        assert!(input.tasks[0].who.is_none());
    }

    #[test]
    fn settings_use_camel_case_and_round_trip() {
        let view = NightSettingsView::from(Settings::default());
        let text = serde_json::to_value(&view).expect("json");
        assert!(
            text.get("stopHhmm").is_some() && text.get("awakeAsked").is_some(),
            "{text}"
        );
        let back: Settings = serde_json::from_value::<NightSettingsView>(text).expect("back").into();
        assert_eq!(back, Settings::default());
    }

    #[test]
    fn the_switches_default_to_off_and_undo_on() {
        assert!(!labs_on(&json!({})), "Labs is off by default");
        assert!(labs_on(&json!({ "night_shift_enabled": true })));
        assert!(
            !labs_on(&json!({ "night_shift_enabled": "yes" })),
            "only a true switch is on"
        );
    }

    #[test]
    fn a_plan_is_tried_here_only_when_the_current_run_had_it_in_that_project() {
        let folder = std::env::temp_dir().join("pm-ns-tried-here");
        let mut run = crate::night_shift::view::morning_fixture();
        // Tasks store their project in the comparable form, so the fixture does too.
        run.tasks[0].spec.project = comparable_project_path(&folder);
        let key = plan_key(&run.tasks[0].attempts[0].plan);
        let here = folder.to_string_lossy().into_owned();
        assert!(tried_here(Some(&run), &key, &here));
        assert!(!tried_here(Some(&run), &key, "/somewhere/else"));
        assert!(!tried_here(None, &key, &here));
    }

    #[test]
    fn an_out_of_usage_plan_shows_its_reset_or_says_none_was_shown() {
        assert_eq!(out_until(None, 0), None);
        assert!(out_until(Some(None), 0).is_some_and(|text| text.contains("No reset time")));
        let text = out_until(Some(Some(3_600_000)), 0).expect("time");
        assert!(text.starts_with("Out of usage until "), "{text}");
    }
}
