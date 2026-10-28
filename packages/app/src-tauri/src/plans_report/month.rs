//! Calendar months in a time zone (this computer's, in the app; a fixed offset in tests).

use chrono::{Datelike, LocalResult, NaiveDate, TimeZone};

/// A year and month (1-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct YearMonth {
    pub year: i32,
    pub month: u32,
}

const NAMES: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

impl YearMonth {
    /// `2026-10` (what the page sends). Anything else is `None`.
    pub fn parse(text: &str) -> Option<Self> {
        let (year, month) = text.trim().split_once('-')?;
        if year.len() != 4 || month.len() != 2 {
            return None;
        }
        let (year, month) = (year.parse::<i32>().ok()?, month.parse::<u32>().ok()?);
        (1..=12).contains(&month).then_some(Self { year, month })
    }

    pub fn id(self) -> String {
        format!("{:04}-{:02}", self.year, self.month)
    }

    pub fn label(self) -> String {
        format!("{} {}", NAMES[(self.month - 1) as usize], self.year)
    }

    pub fn next(self) -> Self {
        if self.month == 12 {
            Self { year: self.year + 1, month: 1 }
        } else {
            Self { year: self.year, month: self.month + 1 }
        }
    }

    pub fn previous(self) -> Self {
        if self.month == 1 {
            Self { year: self.year - 1, month: 12 }
        } else {
            Self { year: self.year, month: self.month - 1 }
        }
    }

    /// The month a moment falls in, by the zone's clock.
    pub fn of_ms<Tz: TimeZone>(ms: u64, tz: &Tz) -> Option<Self> {
        let at = tz.timestamp_millis_opt(i64::try_from(ms).ok()?).single()?;
        Some(Self { year: at.year(), month: at.month() })
    }

    /// First moment of the month, as milliseconds since 1970. A midnight that does not exist (a clock
    /// change at midnight) moves to the first moment that does.
    pub fn start_ms<Tz: TimeZone>(self, tz: &Tz) -> Option<u64> {
        let midnight = NaiveDate::from_ymd_opt(self.year, self.month, 1)?.and_hms_opt(0, 0, 0)?;
        let at = match tz.from_local_datetime(&midnight) {
            LocalResult::Single(at) => at,
            LocalResult::Ambiguous(first, _) => first,
            LocalResult::None => tz.from_local_datetime(&(midnight + chrono::Duration::hours(1))).earliest()?,
        };
        u64::try_from(at.timestamp_millis()).ok()
    }

    /// First moment of the next month (the end, not included).
    pub fn end_ms<Tz: TimeZone>(self, tz: &Tz) -> Option<u64> {
        self.next().start_ms(tz)
    }
}

/// The calendar day of a moment, `2026-10-28`, by the zone's clock.
pub fn day_id<Tz: TimeZone>(ms: u64, tz: &Tz) -> Option<String> {
    let at = tz.timestamp_millis_opt(i64::try_from(ms).ok()?).single()?;
    Some(format!("{:04}-{:02}-{:02}", at.year(), at.month(), at.day()))
}

/// "28 October 2026".
pub fn day_label<Tz: TimeZone>(ms: u64, tz: &Tz) -> Option<String> {
    let at = tz.timestamp_millis_opt(i64::try_from(ms).ok()?).single()?;
    Some(format!("{} {} {}", at.day(), NAMES[(at.month() - 1) as usize], at.year()))
}
