//! Budget types: the stored record, the defaults from Settings and what a caller asked for.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Helpers may start helpers, but only this many levels below the run that holds the first
/// budget: a root run is depth 0, its helper depth 1, a helper's helper depth 2, and that one
/// cannot start another.
pub const MAX_DEPTH: u32 = 2;
pub const MAX_TURNS: u32 = 10_000;
/// One week.
pub const MAX_MINUTES: u32 = 10_080;
/// From this share of the turns on, the header chip turns bold (no push, no card).
pub const NEARLY_PERCENT: u32 = 80;

/// Environment variable a budgeted run's agent process gets, naming the run (its handle). The MCP
/// shim forwards it as `budget_parent` on `run_agent`, so helpers draw from the run's budget.
pub const PARENT_ENV: &str = "PUPPET_MASTER_PARENT_RUN";

/// Settings keys. `0` (or no value) means off, which is the default and today's behaviour.
pub const SETTING_TURNS: &str = "budget_default_turns";
pub const SETTING_MINUTES: &str = "budget_default_minutes";

/// Where a run's limits came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// `budget_turns` / `budget_minutes` on `run_agent`.
    Argument,
    /// The defaults in Settings.
    Default,
    /// A share of the parent run's budget.
    Share,
}

/// The budget of one run (a handle). Usage is never stored here: turns are counted from the
/// operation records, time from `started_ms`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetRecord {
    pub handle: String,
    /// The project this run belongs to (the key its operations are stored under).
    pub project: String,
    /// The run that started this one, when that run holds a budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// The parent's project, when it differs from this run's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_project: Option<String>,
    /// 0 for the run that holds the first budget.
    #[serde(default)]
    pub depth: u32,
    /// Current turn limit including turns the person added. `None`: not limited by turns.
    #[serde(default)]
    pub turns: Option<u32>,
    /// Current time limit in minutes including added time. `None`: not limited by time.
    #[serde(default)]
    pub minutes: Option<u32>,
    /// The limits as first set; the "Add" step is half of these.
    #[serde(default)]
    pub first_turns: Option<u32>,
    #[serde(default)]
    pub first_minutes: Option<u32>,
    /// When the clock started: the moment the run was admitted.
    pub started_ms: u64,
    pub source: Source,
    /// The person chose "Stop here": no more turns, whatever is left.
    #[serde(default)]
    pub stopped: bool,
}

/// Default budgets from Settings (`None` = off).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Defaults {
    pub turns: Option<u32>,
    pub minutes: Option<u32>,
}

impl Defaults {
    pub fn from_settings(settings: &Value) -> Self {
        let read = |key: &str, max: u32| {
            settings
                .get(key)
                .and_then(Value::as_u64)
                .filter(|value| *value > 0)
                .map(|value| value.min(u64::from(max)) as u32)
        };
        Self { turns: read(SETTING_TURNS, MAX_TURNS), minutes: read(SETTING_MINUTES, MAX_MINUTES) }
    }

    pub fn is_off(&self) -> bool {
        self.turns.is_none() && self.minutes.is_none()
    }
}

/// What the caller of `run_agent` asked for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Asked {
    pub turns: Option<u32>,
    pub minutes: Option<u32>,
    /// Handle of the run this caller is (set by the MCP shim from its environment, or passed
    /// by the agent itself as `budget_parent`).
    pub parent: Option<String>,
}

impl Asked {
    pub fn any(&self) -> bool {
        self.turns.is_some() || self.minutes.is_some()
    }
}
