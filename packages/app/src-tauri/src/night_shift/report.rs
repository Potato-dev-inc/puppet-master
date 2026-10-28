//! The morning summary: the counts line, the push body and small time texts (SPEC n32 3.6). Pure.
//!
//! Counts and times only. No task text, project name or path ever comes through here.

use super::model::{Class, NightRun};

/// The push body when "Hide details" is on for the phone.
pub const HIDDEN_PUSH: &str = "Night Shift is ready";

const MS_PER_MIN: u64 = 60_000;
const SECS_PER_DAY: i64 = 86_400;
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Task counts for one run. `done` = Ready + NoChecks; `green` = Ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    pub total: u32,
    pub done: u32,
    pub green: u32,
    pub needs_you: u32,
    pub didnt_run: u32,
    pub no_changes: u32,
}

/// Counts the classified tasks of a run. Tasks without an outcome yet are not counted.
pub fn summary(run: &NightRun) -> Summary {
    let mut s = Summary { total: run.tasks.len() as u32, ..Summary::default() };
    for task in &run.tasks {
        let Some(outcome) = &task.outcome else { continue };
        match outcome.class {
            Class::Ready => {
                s.done += 1;
                s.green += 1;
            }
            Class::NoChecks => s.done += 1,
            Class::NeedsLook => s.needs_you += 1,
            Class::NoChanges => s.no_changes += 1,
            Class::DidntRun => s.didnt_run += 1,
        }
    }
    s
}

/// "4 of 5 done, tests green. 1 needs you." (SPEC n32 3.6 summary line).
pub fn line(s: &Summary) -> String {
    let mut out = format!("{} of {} done", s.done, s.total);
    if s.done > 0 {
        if s.green == s.done {
            out.push_str(", tests green");
        } else if s.green > 0 {
            out.push_str(&format!(", tests green on {}", s.green));
        } else {
            out.push_str(", no tests ran");
        }
    }
    if s.needs_you == 1 {
        out.push_str(". 1 needs you.");
    } else if s.needs_you > 1 {
        out.push_str(&format!(". {} need you.", s.needs_you));
    }
    // The last two clauses end a sentence of their own. When no earlier clause ended one, start with a full stop.
    for (n, text) in [(s.didnt_run, "didn't run"), (s.no_changes, "changed nothing")] {
        if n > 0 {
            out.push_str(if out.ends_with('.') { " " } else { ". " });
            out.push_str(&format!("{n} {text}."));
        }
    }
    out
}

/// The morning push body. Counts only; "Hide details" phones get the fixed sentence.
pub fn push_body(s: &Summary, hidden: bool) -> String {
    if hidden {
        HIDDEN_PUSH.to_string()
    } else {
        line(s)
    }
}

/// "under a minute", "45 min", "2 h", "2 h 5 min".
pub fn duration_text(ms: u64) -> String {
    let mins = ms / MS_PER_MIN;
    if mins == 0 {
        return "under a minute".to_string();
    }
    if mins < 60 {
        return format!("{mins} min");
    }
    let (h, m) = (mins / 60, mins % 60);
    if m == 0 {
        format!("{h} h")
    } else {
        format!("{h} h {m} min")
    }
}

/// "07:00" on the local clock. `offset_min` is the local offset east of UTC in minutes.
pub fn hhmm(at_ms: u64, offset_min: i32) -> String {
    let secs = local_secs(at_ms, offset_min).rem_euclid(SECS_PER_DAY);
    format!("{:02}:{:02}", secs / 3600, (secs % 3600) / 60)
}

/// "today at 07:00", "tomorrow at 07:00" or "on 29 Oct at 07:00", as seen from `now_ms`.
pub fn when_text(at_ms: u64, now_ms: u64, offset_min: i32) -> String {
    let at_day = local_secs(at_ms, offset_min).div_euclid(SECS_PER_DAY);
    let now_day = local_secs(now_ms, offset_min).div_euclid(SECS_PER_DAY);
    let clock = hhmm(at_ms, offset_min);
    match at_day - now_day {
        0 => format!("today at {clock}"),
        1 => format!("tomorrow at {clock}"),
        _ => format!("on {} at {clock}", day_text(at_day)),
    }
}

fn local_secs(ms: u64, offset_min: i32) -> i64 {
    (ms / 1000) as i64 + i64::from(offset_min) * 60
}

/// "29 Oct" for a local day number (days since 1970-01-01).
fn day_text(day: i64) -> String {
    let (_, month, d) = civil_from_days(day);
    format!("{} {}", d, MONTHS[(month - 1) as usize])
}

/// Year, month (1-12) and day of a day number since 1970-01-01 (proleptic Gregorian).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m as u32, d as u32)
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod tests;
