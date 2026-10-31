//! What the bot costs (spec section 6.2). Puppet Master is the judge, not the brain: it prices the
//! tokens the brain reports in `usage` with its own table, keeps the day's total per bot and surface,
//! and says when to warn (80%) and when to stop (100%).
//!
//! Money is counted in nano-dollars (a billionth of a dollar) as whole numbers, so nothing drifts.
//! A model that is not in the table is charged at the highest price in the table.
//! The usage file is `usage/YYYY-MM.json`: per day, per bot, per surface.
//! The clock is passed in as a local date string (`YYYY-MM-DD`); nothing here reads the time.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::secure_file;

pub const USAGE_VERSION: u32 = 1;
pub const NANO_PER_CENT: u64 = 10_000_000;
pub const DEFAULT_LIMIT_CENTS: u32 = 50;
pub const MIN_LIMIT_CENTS: u32 = 5;
pub const MAX_LIMIT_CENTS: u32 = 2_000;
/// The warning note appears at this share of the daily limit.
pub const WARN_PERCENT: u64 = 80;

/// Nano-dollars per token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Price {
    pub input_hit: u64,
    pub input_miss: u64,
    pub output: u64,
}

/// The price table. Matched by model name prefix, longest first. A free model (the offline stub,
/// the test brain) is listed with zeros so it is not charged at the highest price.
const TABLE: &[(&str, Price)] = &[
    ("deepseek-reasoner", Price { input_hit: 140, input_miss: 550, output: 2_190 }),
    ("deepseek-chat", Price { input_hit: 70, input_miss: 270, output: 1_100 }),
    ("deepseek-flash", Price { input_hit: 70, input_miss: 270, output: 1_100 }),
    ("deepseek-v4", Price { input_hit: 70, input_miss: 270, output: 1_100 }),
    ("stub", Price { input_hit: 0, input_miss: 0, output: 0 }),
    ("fake", Price { input_hit: 0, input_miss: 0, output: 0 }),
];

/// The most expensive row of the table, per field.
pub fn highest_price() -> Price {
    TABLE.iter().fold(Price { input_hit: 0, input_miss: 0, output: 0 }, |top, (_, p)| Price {
        input_hit: top.input_hit.max(p.input_hit),
        input_miss: top.input_miss.max(p.input_miss),
        output: top.output.max(p.output),
    })
}

pub fn price_of(model: &str) -> Price {
    let model = model.trim().to_ascii_lowercase();
    TABLE
        .iter()
        .filter(|(name, _)| model.starts_with(name))
        .max_by_key(|(name, _)| name.len())
        .map(|(_, price)| *price)
        .unwrap_or_else(highest_price)
}

/// One `usage` event: the 4 token counts the brain reports.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tokens {
    pub prompt: u64,
    pub completion: u64,
    pub cache_hit: u64,
    pub cache_miss: u64,
}

/// The price of the tokens, in nano-dollars. Input tokens the brain does not split into hit and miss
/// are all charged as misses; a split that is bigger than the prompt count is charged as reported.
pub fn price_tokens(model: &str, t: Tokens) -> u64 {
    let p = price_of(model);
    let split = t.cache_hit.saturating_add(t.cache_miss);
    let unsplit = t.prompt.saturating_sub(split);
    t.cache_hit
        .saturating_mul(p.input_hit)
        .saturating_add(t.cache_miss.saturating_add(unsplit).saturating_mul(p.input_miss))
        .saturating_add(t.completion.saturating_mul(p.output))
}

pub fn clamp_limit_cents(cents: u32) -> u32 {
    cents.clamp(MIN_LIMIT_CENTS, MAX_LIMIT_CENTS)
}

pub fn limit_nano(cents: u32) -> u64 {
    u64::from(clamp_limit_cents(cents)) * NANO_PER_CENT
}

/// Where a bot stands against its daily limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Budget {
    Under,
    /// 80% or more, below 100%.
    Warn,
    /// 100% or more: sends are refused until local midnight.
    Over,
}

pub fn budget(used_nano: u64, limit_cents: u32) -> Budget {
    let limit = limit_nano(limit_cents);
    if used_nano >= limit {
        Budget::Over
    } else if used_nano.saturating_mul(100) >= limit.saturating_mul(WARN_PERCENT) {
        Budget::Warn
    } else {
        Budget::Under
    }
}

/// Dollars as text, for the copy ("$0.50").
pub fn dollars(cents: u32) -> String {
    format!("${}.{:02}", cents / 100, cents % 100)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SurfaceUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_hit_tokens: u64,
    pub cache_miss_tokens: u64,
    pub cost_nano: u64,
    pub turns: u64,
}

/// One bot on one day: per surface, plus the flags (the 80% note appears once a day).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BotDay {
    pub surfaces: BTreeMap<String, SurfaceUsage>,
    pub warned: bool,
    pub limit_hit: bool,
}

impl BotDay {
    pub fn cost_nano(&self) -> u64 {
        self.surfaces.values().fold(0u64, |sum, s| sum.saturating_add(s.cost_nano))
    }
}

/// `usage/YYYY-MM.json`: `{ v, days: { "2026-11-01": { "<bot>": BotDay } } }`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MonthFile {
    pub v: u32,
    pub days: BTreeMap<String, BTreeMap<String, BotDay>>,
}

/// `<root>/usage/YYYY-MM.json` for a local date `YYYY-MM-DD`.
pub fn month_path(root: &Path, day: &str) -> PathBuf {
    let month = day.get(..7).filter(|m| m.as_bytes().get(4) == Some(&b'-')).unwrap_or("0000-00");
    root.join("usage").join(format!("{month}.json"))
}

/// A missing file is empty; a damaged file is an error and is never overwritten.
pub fn load_month(root: &Path, day: &str) -> Result<MonthFile, String> {
    let path = month_path(root, day);
    match fs::read_to_string(&path) {
        Ok(text) => {
            let file: MonthFile = serde_json::from_str(&text).map_err(|e| format!("{} is damaged: {e}", path.display()))?;
            if file.v > USAGE_VERSION {
                return Err(format!("{} was written by a newer Puppet Master", path.display()));
            }
            Ok(file)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(MonthFile { v: USAGE_VERSION, days: BTreeMap::new() }),
        Err(e) => Err(format!("could not read {}: {e}", path.display())),
    }
}

fn save_month(root: &Path, day: &str, file: &MonthFile) -> Result<(), String> {
    let text = serde_json::to_string_pretty(file).map_err(|e| format!("could not save usage: {e}"))?;
    secure_file::write_private(&month_path(root, day), &text)
}

/// One bot's total for the day, over all its surfaces.
pub fn day_of(root: &Path, day: &str, bot: &str) -> Result<BotDay, String> {
    Ok(load_month(root, day)?.days.get(day).and_then(|bots| bots.get(bot)).cloned().unwrap_or_default())
}

/// What `charge` found out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Charged {
    pub cost_nano: u64,
    pub used_nano: u64,
    pub budget: Budget,
    /// The 80% note is due now (first time today).
    pub warn_now: bool,
    /// The 100% pause is due now (first time today).
    pub limit_now: bool,
}

/// Adds one `usage` event to the day's total and judges it against the limit.
pub fn charge(root: &Path, day: &str, bot: &str, surface: &str, model: &str, tokens: Tokens, limit_cents: u32) -> Result<Charged, String> {
    let _files = super::store::files_lock();
    let mut file = load_month(root, day)?;
    file.v = USAGE_VERSION;
    let cost = price_tokens(model, tokens);
    let entry = file.days.entry(day.to_string()).or_default().entry(bot.to_string()).or_default();
    let surf = entry.surfaces.entry(surface.to_string()).or_default();
    surf.prompt_tokens = surf.prompt_tokens.saturating_add(tokens.prompt);
    surf.completion_tokens = surf.completion_tokens.saturating_add(tokens.completion);
    surf.cache_hit_tokens = surf.cache_hit_tokens.saturating_add(tokens.cache_hit);
    surf.cache_miss_tokens = surf.cache_miss_tokens.saturating_add(tokens.cache_miss);
    surf.cost_nano = surf.cost_nano.saturating_add(cost);
    surf.turns = surf.turns.saturating_add(1);
    let used = entry.cost_nano();
    let verdict = budget(used, limit_cents);
    let warn_now = verdict != Budget::Under && !entry.warned;
    let limit_now = verdict == Budget::Over && !entry.limit_hit;
    if verdict != Budget::Under {
        entry.warned = true;
    }
    if verdict == Budget::Over {
        entry.limit_hit = true;
    }
    save_month(root, day, &file)?;
    Ok(Charged { cost_nano: cost, used_nano: used, budget: verdict, warn_now, limit_now })
}
