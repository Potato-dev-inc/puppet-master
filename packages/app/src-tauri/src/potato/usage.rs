//! The numbers of Settings > Potato Bot > Usage (spec 6.2). Read only: it adds up what `cost.rs` already keeps in
//! `usage/YYYY-MM.json` for the last 30 local days, per bot and in total.
//!
//! - A message is one brain turn the brain reported usage for (`cost::SurfaceUsage::turns`).
//! - Tokens are prompt tokens plus completion tokens.
//! - Cost is in nano-dollars, as everywhere else; the page turns it into dollars.
//! - Sandbox actions are the day's count per bot, judged by the cap in the settings.
//!
//! Nothing here writes and nothing reads a clock: the caller says what today is. Dates are `YYYY-MM-DD` strings.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use super::cost::{self, BotDay, MonthFile};

/// How many local days the report covers, today included.
pub const DAYS: usize = 30;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub messages: u64,
    pub tokens: u64,
    pub cost_nano: u64,
    pub sandbox_actions: u64,
}

impl Totals {
    fn of(day: &BotDay) -> Totals {
        let mut totals = Totals { cost_nano: day.cost_nano(), sandbox_actions: day.sandbox_actions, ..Totals::default() };
        for surface in day.surfaces.values() {
            totals.messages = totals.messages.saturating_add(surface.turns);
            totals.tokens = totals.tokens.saturating_add(surface.prompt_tokens.saturating_add(surface.completion_tokens));
        }
        totals
    }

    fn add(&mut self, other: &Totals) {
        self.messages = self.messages.saturating_add(other.messages);
        self.tokens = self.tokens.saturating_add(other.tokens);
        self.cost_nano = self.cost_nano.saturating_add(other.cost_nano);
        self.sandbox_actions = self.sandbox_actions.saturating_add(other.sandbox_actions);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayUsage {
    /// `YYYY-MM-DD`.
    pub date: String,
    #[serde(flatten)]
    pub totals: Totals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BotUsage {
    pub id: String,
    pub name: String,
    /// All the days of the report.
    pub total: Totals,
    /// Today only (the last day of `days`).
    pub today: Totals,
    /// Oldest first, one per day of the report.
    pub days: Vec<DayUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageReport {
    pub today: String,
    /// The daily limit of one bot, in cents (clamped as the settings clamp it).
    pub limit_cents: u32,
    /// The daily cap of sandbox actions of one bot (clamped as the settings clamp it).
    pub sandbox_actions_per_day: u32,
    /// All bots, all the days of the report.
    pub total: Totals,
    /// All bots, today.
    pub today_total: Totals,
    /// All bots, one per day, oldest first.
    pub days: Vec<DayUsage>,
    pub bots: Vec<BotUsage>,
}

fn sum<'a>(items: impl Iterator<Item = &'a Totals>) -> Totals {
    let mut total = Totals::default();
    for item in items {
        total.add(item);
    }
    total
}

/// Parses `YYYY-MM-DD` into (year, month, day).
fn parse_day(day: &str) -> Option<(i64, i64, i64)> {
    let bytes = day.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year: i64 = day.get(0..4)?.parse().ok()?;
    let month: i64 = day.get(5..7)?.parse().ok()?;
    let date: i64 = day.get(8..10)?.parse().ok()?;
    ((1..=12).contains(&month) && (1..=31).contains(&date)).then_some((year, month, date))
}

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date of a day count since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The `count` local days that end with `today`, oldest first. Empty when `today` is not a date.
pub fn days_ending(today: &str, count: usize) -> Vec<String> {
    let Some((year, month, day)) = parse_day(today) else { return Vec::new() };
    let end = days_from_civil(year, month, day);
    (0..count as i64)
        .rev()
        .map(|back| {
            let (y, m, d) = civil_from_days(end - back);
            format!("{y:04}-{m:02}-{d:02}")
        })
        .collect()
}

/// The report for `bots` (id and name, as the page shows them), over the 30 days that end with `today`.
/// A damaged usage file is an error; a missing one is an empty month.
pub fn build(root: &Path, today: &str, bots: &[(String, String)], limit_cents: u32, sandbox_actions_per_day: u32) -> Result<UsageReport, String> {
    let dates = days_ending(today, DAYS);
    if dates.is_empty() {
        return Err(format!("{today} is not a day Potato Bot can count"));
    }
    // One read per month the report touches.
    let mut months: BTreeMap<String, MonthFile> = BTreeMap::new();
    for date in &dates {
        let key = date[..7].to_string();
        if !months.contains_key(&key) {
            months.insert(key, cost::load_month(root, date)?);
        }
    }
    let per_day: Vec<BTreeMap<String, BotDay>> = dates.iter().map(|date| months.get(&date[..7]).and_then(|file| file.days.get(date)).cloned().unwrap_or_default()).collect();

    let bot_rows: Vec<BotUsage> = bots
        .iter()
        .map(|(id, name)| {
            let days: Vec<DayUsage> = dates
                .iter()
                .zip(&per_day)
                .map(|(date, day)| DayUsage { date: date.clone(), totals: day.get(id).map(Totals::of).unwrap_or_default() })
                .collect();
            BotUsage {
                id: id.clone(),
                name: name.clone(),
                total: sum(days.iter().map(|d| &d.totals)),
                today: days.last().map(|d| d.totals).unwrap_or_default(),
                days,
            }
        })
        .collect();

    let days: Vec<DayUsage> = dates
        .iter()
        .enumerate()
        .map(|(i, date)| DayUsage { date: date.clone(), totals: sum(bot_rows.iter().map(|bot| &bot.days[i].totals)) })
        .collect();

    Ok(UsageReport {
        today: today.to_string(),
        limit_cents: cost::clamp_limit_cents(limit_cents),
        sandbox_actions_per_day: cost::clamp_sandbox_actions(sandbox_actions_per_day),
        total: sum(days.iter().map(|d| &d.totals)),
        today_total: days.last().map(|d| d.totals).unwrap_or_default(),
        days,
        bots: bot_rows,
    })
}
