//! Reading the reset time a limit screen prints ("5pm", "5:30pm (Asia/Taipei)", "Oct 16, 5pm",
//! "9:45 PM", "Oct 16th, 2026 9:45 PM") into an epoch time.
//!
//! The formats are the profile's data (`[limits.reset_formats]`, regexes with named groups); this
//! file only turns the captured parts into a moment. Time and zone come from a [`Clock`], so every
//! case (today, tomorrow, another year, a foreign zone) runs on a fixed clock in tests.
//!
//! A zone suffix that is not this machine's own zone gives `None`: the printed hour is then in
//! another zone and guessing would show a wrong time.

use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike};
use regex::Regex;

/// Where "now", "today" and the local zone come from.
pub trait Clock {
    fn now_ms(&self) -> i64;
    /// This machine's IANA zone name (`Asia/Taipei`), when it can be told.
    fn zone_name(&self) -> Option<String>;
    /// The local wall-clock time at `ms`.
    fn local_at(&self, ms: i64) -> NaiveDateTime;
    /// The moment a local wall-clock time is. `None` for a time that does not exist (a DST gap).
    fn to_ms(&self, local: NaiveDateTime) -> Option<i64>;
}

/// The machine's own clock and zone.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        chrono::Utc::now().timestamp_millis()
    }

    fn zone_name(&self) -> Option<String> {
        iana_time_zone::get_timezone().ok()
    }

    fn local_at(&self, ms: i64) -> NaiveDateTime {
        Local
            .timestamp_millis_opt(ms)
            .single()
            .map(|t| t.naive_local())
            .unwrap_or_default()
    }

    fn to_ms(&self, local: NaiveDateTime) -> Option<i64> {
        Local.from_local_datetime(&local).earliest().map(|t| t.timestamp_millis())
    }
}

/// "14:05" in this machine's zone, with the day when it is not today ("Oct 16 14:05").
pub fn when_text(ms: i64, clock: &dyn Clock) -> String {
    let at = clock.local_at(ms);
    let today = clock.local_at(clock.now_ms()).date();
    if at.date() == today {
        format!("{:02}:{:02}", at.hour(), at.minute())
    } else {
        format!("{} {} {:02}:{:02}", month_short(at.month()), at.day(), at.hour(), at.minute())
    }
}

fn month_short(month: u32) -> &'static str {
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(month as usize).saturating_sub(1).min(11)]
}

fn month_number(name: &str) -> Option<u32> {
    let head: String = name.chars().take(3).collect::<String>().to_ascii_lowercase();
    ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"]
        .iter()
        .position(|m| *m == head)
        .map(|i| i as u32 + 1)
}

/// Collapse every run of whitespace (a wrapped row, a hanging indent) to one space.
pub fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Split a trailing ` (Area/City)` off a reset text.
fn split_zone(text: &str) -> (&str, Option<&str>) {
    let trimmed = text.trim_end();
    if let Some(open) = trimmed.rfind('(') {
        if trimmed.ends_with(')') {
            let zone = &trimmed[open + 1..trimmed.len() - 1];
            let ok = !zone.is_empty()
                && zone.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '+' | '-'));
            if ok {
                return (trimmed[..open].trim_end(), Some(zone));
            }
        }
    }
    (trimmed, None)
}

/// The moment `text` names, or `None` when it cannot be read or is in another zone.
pub fn parse_reset(text: &str, formats: &[Regex], clock: &dyn Clock) -> Option<i64> {
    let squashed = squash(text);
    let (body, zone) = split_zone(&squashed);
    if let Some(zone) = zone {
        if clock.zone_name().as_deref() != Some(zone) {
            return None;
        }
    }
    let now = clock.now_ms();
    for format in formats {
        let Some(caps) = format.captures(body) else { continue };
        if caps.get(0).map(|m| m.as_str().len()) != Some(body.len()) {
            continue;
        }
        let number = |name: &str| caps.name(name).and_then(|m| m.as_str().parse::<u32>().ok());
        let hour12 = number("hour")?;
        let minute = number("minute").unwrap_or(0);
        if !(1..=12).contains(&hour12) || minute > 59 {
            return None;
        }
        let pm = caps.name("ampm")?.as_str().eq_ignore_ascii_case("pm");
        let hour = hour12 % 12 + if pm { 12 } else { 0 };
        let local_now = clock.local_at(now);
        let at = |date: NaiveDate| date.and_hms_opt(hour, minute, 0);
        let candidate = match caps.name("mon") {
            None => {
                // A time with no date: the next time the clock shows it (a few minutes of leeway
                // for a reset that has just passed).
                let mut ms = clock.to_ms(at(local_now.date())?)?;
                if ms < now - 5 * 60_000 {
                    ms = clock.to_ms(at(local_now.date() + Duration::days(1))?)?;
                }
                ms
            }
            Some(mon) => {
                let month = month_number(mon.as_str())?;
                let day = number("day")?;
                let explicit_year = number("year").map(|y| y as i32);
                let year = explicit_year.unwrap_or(local_now.year());
                let mut ms = clock.to_ms(at(NaiveDate::from_ymd_opt(year, month, day)?)?)?;
                if explicit_year.is_none() && ms < now - 24 * 3_600_000 {
                    ms = clock.to_ms(at(NaiveDate::from_ymd_opt(year + 1, month, day)?)?)?;
                }
                ms
            }
        };
        return Some(candidate);
    }
    None
}

/// A clock fixed at one moment in one zone with a constant offset, for tests.
#[cfg(test)]
pub struct FixedClock {
    pub now_ms: i64,
    pub offset_secs: i32,
    pub zone: Option<&'static str>,
}

#[cfg(test)]
impl FixedClock {
    /// `2026-10-15 12:00` local in `zone` at UTC+8 (the machine the design was written on).
    pub fn taipei_noon() -> Self {
        let local = NaiveDate::from_ymd_opt(2026, 10, 15).unwrap().and_hms_opt(12, 0, 0).unwrap();
        Self { now_ms: local.and_utc().timestamp_millis() - 8 * 3_600_000, offset_secs: 8 * 3600, zone: Some("Asia/Taipei") }
    }

    pub fn at(local: NaiveDateTime, offset_hours: i32, zone: Option<&'static str>) -> Self {
        Self { now_ms: local.and_utc().timestamp_millis() - i64::from(offset_hours) * 3_600_000, offset_secs: offset_hours * 3600, zone }
    }
}

#[cfg(test)]
impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        self.now_ms
    }

    fn zone_name(&self) -> Option<String> {
        self.zone.map(str::to_string)
    }

    fn local_at(&self, ms: i64) -> NaiveDateTime {
        chrono::DateTime::from_timestamp_millis(ms + i64::from(self.offset_secs) * 1000).unwrap().naive_utc()
    }

    fn to_ms(&self, local: NaiveDateTime) -> Option<i64> {
        Some(local.and_utc().timestamp_millis() - i64::from(self.offset_secs) * 1000)
    }
}
