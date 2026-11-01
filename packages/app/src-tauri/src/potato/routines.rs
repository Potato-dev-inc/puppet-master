//! Routines of Potato Bot (phase P9a): a prompt a bot runs on a schedule, in `<app data>/potato/routines.json`.
//!
//! - A schedule is a preset (`hourly`, `daily` at HH:MM, `weekdays` at HH:MM, `everyMinutes` 15 to 1440) or a
//!   one-time `once` at a local date and time. Times are the computer's local clock (no zone is stored).
//! - `next_after` is the pure next-run math. `take_due` is the pure scheduler step: it picks what is due at `now`,
//!   moves each routine on, and returns what to send. A routine due several times while the app was closed runs
//!   ONCE (the missed ticks are coalesced); a run that starts 15 minutes or more after it was due gets a note in
//!   its `context` saying it ran late.
//! - `run_due` is the 30 second tick (started with the live service): it saves the moved routines first, then
//!   hands each one to the bot's brain as a message from `routine`, trusted like the person at this computer.
//!   Helpers still need Start.
//! - Every whole-file write takes the store lock (`store::files_lock`).
//!
//! The clock is passed in (`now_ms`) so the tests need no real time. Nothing here names an agent or a chat app.

use std::path::Path;

use chrono::{Datelike, Local, NaiveDateTime, NaiveTime, TimeZone, Timelike, Weekday};
use serde::{Deserialize, Serialize};

use super::service::{Origin, SendOutcome, Service};
use super::store::{self, StoreError};

pub const FILE_VERSION: u32 = 1;
pub const MIN_EVERY_MINUTES: u32 = 15;
pub const MAX_EVERY_MINUTES: u32 = 1440;
/// A run that starts this many minutes after it was due says so to the brain.
pub const LATE_AFTER_MINUTES: i64 = 15;
pub const MAX_ROUTINES_PER_BOT: usize = 20;
pub const MAX_TITLE_CHARS: usize = 80;
pub const MAX_PROMPT_CHARS: usize = 2000;
const INVALID: &str = "POTATO_ROUTINE_INVALID";
const NEXT_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";
const ONCE_FORMAT: &str = "%Y-%m-%dT%H:%M";

/// When a routine runs. Serialized as `{ "kind": "daily", "time": "09:00" }` and so on.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Schedule {
    /// Every hour, on the hour.
    Hourly,
    /// Every day at `time` ("HH:MM", 24 hours).
    Daily { time: String },
    /// Monday to Friday at `time`.
    Weekdays { time: String },
    /// Every `minutes`, on a grid counted from local midnight.
    EveryMinutes { minutes: u32 },
    /// Once, at a local date and time ("YYYY-MM-DDTHH:MM").
    Once { at: String },
}

/// What the form sends: a name, what to do, and when.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RoutineInput {
    pub title: String,
    pub prompt: String,
    pub schedule: Schedule,
}

/// One routine as stored.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Routine {
    pub id: String,
    pub bot: String,
    pub title: String,
    pub prompt: String,
    pub schedule: Schedule,
    #[serde(default)]
    pub paused: bool,
    /// A one-time routine that has run. It never runs again unless it is edited to a future time.
    #[serde(default)]
    pub done: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_ms: Option<i64>,
    /// The next run as local wall-clock time ("YYYY-MM-DDTHH:MM:SS"). None while paused or done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run: Option<String>,
    pub created_ms: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RoutinesFile {
    pub v: u32,
    pub routines: Vec<Routine>,
}

/// What the page shows for a routine.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RoutineView {
    pub id: String,
    pub bot: String,
    pub title: String,
    pub prompt: String,
    pub schedule: Schedule,
    /// "Weekdays 6am", "Every hour", "Once on Nov 2 at 9am" and so on.
    pub summary: String,
    /// `active`, `paused` or `done`.
    pub status: &'static str,
    pub paused: bool,
    pub done: bool,
    pub last_run_ms: Option<i64>,
    pub next_run_ms: Option<i64>,
}

/// One run the tick sends to a brain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Due {
    pub id: String,
    pub bot: String,
    pub title: String,
    pub prompt: String,
    /// The late note, when the run started 15 minutes or more after it was due.
    pub context: Option<String>,
}

fn invalid(message: &str) -> StoreError {
    StoreError::new(INVALID, message)
}

// ---- time math (pure: local wall-clock values only) -----------------------------------------------

/// "HH:MM" to a time of day. Anything else is None.
pub fn parse_clock(text: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(text.trim(), "%H:%M").ok()
}

fn parse_once(text: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(text.trim(), ONCE_FORMAT).ok()
}

fn parse_next(text: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(text, NEXT_FORMAT).ok()
}

fn format_next(at: NaiveDateTime) -> String {
    at.format(NEXT_FORMAT).to_string()
}

/// The first run strictly after `after`, or None when the schedule has no run left.
pub fn next_after(schedule: &Schedule, after: NaiveDateTime) -> Option<NaiveDateTime> {
    match schedule {
        Schedule::Hourly => {
            let top = after.date().and_hms_opt(after.hour(), 0, 0)?;
            Some(top + chrono::TimeDelta::hours(1))
        }
        Schedule::Daily { time } => {
            let at = parse_clock(time)?;
            let today = after.date().and_time(at);
            Some(if today > after { today } else { today + chrono::TimeDelta::days(1) })
        }
        Schedule::Weekdays { time } => {
            let at = parse_clock(time)?;
            (0..8).map(|offset| after.date() + chrono::TimeDelta::days(offset)).map(|day| day.and_time(at)).find(|candidate| {
                *candidate > after && !matches!(candidate.weekday(), Weekday::Sat | Weekday::Sun)
            })
        }
        Schedule::EveryMinutes { minutes } => {
            if !(MIN_EVERY_MINUTES..=MAX_EVERY_MINUTES).contains(minutes) {
                return None;
            }
            let step = i64::from(*minutes) * 60;
            let seconds = i64::from(after.num_seconds_from_midnight());
            let next = (seconds / step + 1) * step;
            let midnight = after.date().and_hms_opt(0, 0, 0)?;
            Some(midnight + chrono::TimeDelta::seconds(next))
        }
        Schedule::Once { at } => parse_once(at).filter(|at| *at > after),
    }
}

/// "6am", "6:30am", "12pm".
fn clock_words(time: NaiveTime) -> String {
    let hour = time.hour();
    let suffix = if hour < 12 { "am" } else { "pm" };
    let hour12 = if hour % 12 == 0 { 12 } else { hour % 12 };
    if time.minute() == 0 {
        format!("{hour12}{suffix}")
    } else {
        format!("{hour12}:{:02}{suffix}", time.minute())
    }
}

/// The schedule in plain words, for the list.
pub fn summary(schedule: &Schedule) -> String {
    match schedule {
        Schedule::Hourly => "Every hour".to_string(),
        Schedule::Daily { time } => format!("Every day {}", parse_clock(time).map(clock_words).unwrap_or_else(|| time.clone())),
        Schedule::Weekdays { time } => format!("Weekdays {}", parse_clock(time).map(clock_words).unwrap_or_else(|| time.clone())),
        Schedule::EveryMinutes { minutes } => format!("Every {minutes} minutes"),
        Schedule::Once { at } => match parse_once(at) {
            Some(at) => format!("Once on {} at {}", at.format("%b %-d"), clock_words(at.time())),
            None => "Once".to_string(),
        },
    }
}

fn late_note(due: NaiveDateTime, now: NaiveDateTime) -> String {
    format!(
        "This run is late. It was due at {} and started at {}. Adjust anything that depends on the time of day, and mention the delay if it matters.",
        due.format("%b %-d, %-I:%M %p"),
        now.format("%b %-d, %-I:%M %p"),
    )
}

// ---- validation -----------------------------------------------------------------------------------

/// Checks a form before it is saved. `now` is the local wall clock; a one-time run must be in the future.
pub fn validate(input: &RoutineInput, now: NaiveDateTime) -> Result<(), StoreError> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err(invalid("Give the routine a name."));
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(invalid("The name is too long. Keep it under 80 letters."));
    }
    let prompt = input.prompt.trim();
    if prompt.is_empty() {
        return Err(invalid("Write what the bot should do."));
    }
    if prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err(invalid("That is too long. Keep it under 2000 letters."));
    }
    match &input.schedule {
        Schedule::Hourly => Ok(()),
        Schedule::Daily { time } | Schedule::Weekdays { time } => {
            parse_clock(time).map(|_| ()).ok_or_else(|| invalid("Pick a time like 09:00."))
        }
        Schedule::EveryMinutes { minutes } => {
            if (MIN_EVERY_MINUTES..=MAX_EVERY_MINUTES).contains(minutes) {
                Ok(())
            } else {
                Err(invalid("Pick every 15 minutes or more."))
            }
        }
        Schedule::Once { at } => match parse_once(at) {
            Some(at) if at > now => Ok(()),
            Some(_) => Err(invalid("Pick a time in the future.")),
            None => Err(invalid("Pick a date and time.")),
        },
    }
}

// ---- the scheduler step (pure) --------------------------------------------------------------------

/// Picks the routines due at `now` (local wall clock; `now_ms` is the same instant), moves each one on, and
/// returns what to send. One run per routine however many times it was missed. A paused or finished routine
/// never runs. A routine with no next run gets one now (a file from an older build).
pub fn take_due(routines: &mut [Routine], now: NaiveDateTime, now_ms: i64) -> Vec<Due> {
    let mut out = Vec::new();
    for routine in routines.iter_mut() {
        if routine.paused || routine.done {
            continue;
        }
        let Some(due) = routine.next_run.as_deref().and_then(parse_next) else {
            routine.next_run = next_after(&routine.schedule, now).map(format_next);
            continue;
        };
        if due > now {
            continue;
        }
        let context = ((now - due).num_minutes() >= LATE_AFTER_MINUTES).then(|| late_note(due, now));
        if matches!(routine.schedule, Schedule::Once { .. }) {
            routine.done = true;
            routine.next_run = None;
        } else {
            routine.next_run = next_after(&routine.schedule, now).map(format_next);
        }
        routine.last_run_ms = Some(now_ms);
        out.push(Due { id: routine.id.clone(), bot: routine.bot.clone(), title: routine.title.clone(), prompt: routine.prompt.trim().to_string(), context });
    }
    out
}

/// Local wall-clock time of an instant.
pub fn local_naive(ms: i64) -> NaiveDateTime {
    match Local.timestamp_millis_opt(ms).single() {
        Some(at) => at.naive_local(),
        None => chrono::DateTime::<chrono::Utc>::UNIX_EPOCH.naive_utc(),
    }
}

/// An instant of a local wall-clock time (the earlier one in a repeated hour; the clock is read as UTC in a gap).
fn instant_ms(at: NaiveDateTime) -> Option<i64> {
    match Local.from_local_datetime(&at) {
        chrono::LocalResult::Single(t) => Some(t.timestamp_millis()),
        chrono::LocalResult::Ambiguous(first, _) => Some(first.timestamp_millis()),
        chrono::LocalResult::None => Some(at.and_utc().timestamp_millis()),
    }
}

pub fn view(routine: &Routine) -> RoutineView {
    let status = if routine.done {
        "done"
    } else if routine.paused {
        "paused"
    } else {
        "active"
    };
    RoutineView {
        id: routine.id.clone(),
        bot: routine.bot.clone(),
        title: routine.title.clone(),
        prompt: routine.prompt.clone(),
        schedule: routine.schedule.clone(),
        summary: summary(&routine.schedule),
        status,
        paused: routine.paused,
        done: routine.done,
        last_run_ms: routine.last_run_ms,
        next_run_ms: routine.next_run.as_deref().and_then(parse_next).and_then(instant_ms),
    }
}

// ---- the file -------------------------------------------------------------------------------------

pub fn routines_path(root: &Path) -> std::path::PathBuf {
    root.join("routines.json")
}

/// Reads the file under the store lock. A missing file is empty; a damaged one is an error and is never overwritten.
fn load_file(root: &Path) -> Result<RoutinesFile, StoreError> {
    let file: Option<RoutinesFile> = store::read_json(&routines_path(root), "routines.json")?;
    match file {
        None => Ok(RoutinesFile { v: FILE_VERSION, routines: Vec::new() }),
        Some(file) if file.v > FILE_VERSION => Err(invalid("routines.json was written by a newer Puppet Master.")),
        Some(file) => Ok(file),
    }
}

fn save_file(root: &Path, routines: &[Routine]) -> Result<(), StoreError> {
    store::write_json(&routines_path(root), &RoutinesFile { v: FILE_VERSION, routines: routines.to_vec() }, "routines.json")
}

pub fn list(root: &Path, bot: Option<&str>) -> Result<Vec<RoutineView>, StoreError> {
    let _files = store::files_lock();
    Ok(load_file(root)?.routines.iter().filter(|r| bot.is_none_or(|b| r.bot == b)).map(view).collect())
}

fn find_in(routines: &[Routine], id: &str) -> Result<usize, StoreError> {
    routines.iter().position(|r| r.id == id).ok_or_else(|| StoreError::new("POTATO_ROUTINE_NOT_FOUND", "No routine has that name."))
}

fn bot_exists(root: &Path, bot: &str) -> Result<(), StoreError> {
    if store::load_bots(root)?.iter().any(|b| b.id == bot) {
        Ok(())
    } else {
        Err(StoreError::new("POTATO_BOT_NOT_FOUND", "No bot has that name."))
    }
}

/// Makes a routine for a bot. At most 20 per bot.
pub fn create(root: &Path, bot: &str, input: &RoutineInput, now_ms: i64) -> Result<RoutineView, StoreError> {
    let now = local_naive(now_ms);
    validate(input, now)?;
    bot_exists(root, bot)?;
    let _files = store::files_lock();
    let mut file = load_file(root)?;
    if file.routines.iter().filter(|r| r.bot == bot).count() >= MAX_ROUTINES_PER_BOT {
        return Err(invalid("This bot has 20 routines already. Delete one first."));
    }
    let routine = Routine {
        id: format!("r-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]),
        bot: bot.to_string(),
        title: input.title.trim().to_string(),
        prompt: input.prompt.trim().to_string(),
        schedule: input.schedule.clone(),
        paused: false,
        done: false,
        last_run_ms: None,
        next_run: next_after(&input.schedule, now).map(format_next),
        created_ms: now_ms,
    };
    file.routines.push(routine.clone());
    save_file(root, &file.routines)?;
    Ok(view(&routine))
}

/// Changes a routine's name, prompt and schedule. A one-time routine that ran can be set to a new future time.
pub fn update(root: &Path, id: &str, input: &RoutineInput, now_ms: i64) -> Result<RoutineView, StoreError> {
    let now = local_naive(now_ms);
    validate(input, now)?;
    let _files = store::files_lock();
    let mut file = load_file(root)?;
    let index = find_in(&file.routines, id)?;
    let routine = &mut file.routines[index];
    routine.title = input.title.trim().to_string();
    routine.prompt = input.prompt.trim().to_string();
    routine.schedule = input.schedule.clone();
    routine.done = false;
    routine.next_run = if routine.paused { None } else { next_after(&routine.schedule, now).map(format_next) };
    let saved = view(routine);
    save_file(root, &file.routines)?;
    Ok(saved)
}

/// Pause (`paused = true`) or resume. Resuming looks forward from now: the time paused is never run late.
pub fn set_paused(root: &Path, id: &str, paused: bool, now_ms: i64) -> Result<RoutineView, StoreError> {
    let now = local_naive(now_ms);
    let _files = store::files_lock();
    let mut file = load_file(root)?;
    let index = find_in(&file.routines, id)?;
    let routine = &mut file.routines[index];
    routine.paused = paused;
    routine.next_run = if paused || routine.done { None } else { next_after(&routine.schedule, now).map(format_next) };
    let saved = view(routine);
    save_file(root, &file.routines)?;
    Ok(saved)
}

pub fn delete(root: &Path, id: &str) -> Result<(), StoreError> {
    let _files = store::files_lock();
    let mut file = load_file(root)?;
    let index = find_in(&file.routines, id)?;
    file.routines.remove(index);
    save_file(root, &file.routines)
}

/// The 30 second tick. Does nothing while Potato Bot is switched off. Moves the due routines on and saves that
/// first, then sends each one to its brain. Returns how many were due.
pub fn run_due(service: &Service, now_ms: i64) -> usize {
    if !service.enabled() {
        return 0;
    }
    let root = service.root();
    let held = super::hold::is_held(root);
    let dues = {
        let _files = store::files_lock();
        let mut file = match load_file(root) {
            Ok(file) => file,
            Err(e) => {
                tracing::warn!(%e, "could not read the routines; they wait");
                return 0;
            }
        };
        let dues = take_due(&mut file.routines, local_naive(now_ms), now_ms);
        if !dues.is_empty() {
            if let Err(e) = save_file(root, &file.routines) {
                tracing::warn!(%e, "could not save the routines; they will run again later");
                return 0;
            }
        }
        dues
    };
    for due in &dues {
        if held {
            service.note(&due.bot, &super::hold::skipped_note(&due.title));
            continue;
        }
        let origin = Origin::Routine { title: &due.title, context: due.context.as_deref() };
        if let Err(e) = service.send_as(&due.bot, &due.prompt, origin) {
            tracing::warn!(%e, routine = %due.id, "a routine could not reach its bot");
            // A refused run is never silent: its history line says why (the code is stable, the text is for people).
            if let Some(line) = refused_note(&e.code, &due.title) {
                service.note(&due.bot, &line);
            }
        }
    }
    dues.len()
}

/// The history line for a routine run the app refused to send. `None` when the refusal already left its own line.
pub fn refused_note(code: &str, title: &str) -> Option<String> {
    Some(match code {
        "POTATO_TURNS" | "POTATO_BOT_NOT_FOUND" => return None,
        "POTATO_PAUSED" => super::hold::skipped_note(title),
        "POTATO_RATE" => format!("Skipped: too many messages in a minute: {title}"),
        "POTATO_OFF" => format!("Skipped: Potato Bot is switched off: {title}"),
        _ => format!("Skipped: the message could not be sent: {title}"),
    })
}

// ---- runs that are due but not yet taken by a brain (survive a restart) ---------------------------

/// A routine run that was written to the thread and handed to a bot, and that no brain has answered yet.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingRun {
    /// The saved message id: the brain's reply or error names it in `inReplyTo`.
    pub id: String,
    pub bot: String,
    pub title: String,
    pub at_ms: i64,
}

pub fn pending_path(root: &Path) -> std::path::PathBuf {
    root.join("routine_pending.json")
}

/// The pending runs on disk. A damaged file is read as none (logged), so the app still starts.
fn load_pending(root: &Path) -> Vec<PendingRun> {
    match store::read_json::<Vec<PendingRun>>(&pending_path(root), "routine pending runs") {
        Ok(found) => found.unwrap_or_default(),
        Err(e) => {
            tracing::warn!(%e, "could not read the pending routine runs");
            Vec::new()
        }
    }
}

fn save_pending(root: &Path, runs: &[PendingRun]) {
    if let Err(e) = store::write_json(&pending_path(root), &runs.to_vec(), "routine pending runs") {
        tracing::warn!(%e, "could not save the pending routine runs");
    }
}

/// Written BEFORE the run goes to the queue, so a quit before the brain takes it leaves it on disk.
pub fn add_pending(root: &Path, run: PendingRun) {
    let _files = store::files_lock();
    let mut runs = load_pending(root);
    runs.push(run);
    save_pending(root, &runs);
}

/// The brain answered (a reply or an error) for this message: the run was taken.
pub fn clear_pending(root: &Path, id: &str) {
    let _files = store::files_lock();
    let runs = load_pending(root);
    if runs.iter().any(|r| r.id == id) {
        save_pending(root, &runs.into_iter().filter(|r| r.id != id).collect::<Vec<_>>());
    }
}

#[cfg(test)]
pub fn pending_count(root: &Path) -> usize {
    load_pending(root).len()
}

#[cfg(test)]
pub fn pending_ids(root: &Path) -> Vec<String> {
    load_pending(root).into_iter().map(|r| r.id).collect()
}

/// On a fresh start: every run still pending was never answered. It is written as missed (never run again, so it
/// cannot run twice) and the list is cleared.
pub fn recover_pending(service: &Service, _now_ms: i64) {
    let missed = {
        let _files = store::files_lock();
        let runs = load_pending(service.root());
        save_pending(service.root(), &[]);
        runs
    };
    for run in missed {
        service.note(&run.bot, &format!("Missed while the app was closed: {}", run.title));
    }
}

/// "Run now": sends the routine's prompt at once. The schedule does not move.
pub fn run_now(service: &Service, id: &str, now_ms: i64) -> Result<SendOutcome, StoreError> {
    let routine = {
        let _files = store::files_lock();
        let mut file = load_file(service.root())?;
        let index = find_in(&file.routines, id)?;
        file.routines[index].last_run_ms = Some(now_ms);
        let routine = file.routines[index].clone();
        save_file(service.root(), &file.routines)?;
        routine
    };
    service.send_as(&routine.bot, routine.prompt.trim(), Origin::Routine { title: &routine.title, context: None })
}
