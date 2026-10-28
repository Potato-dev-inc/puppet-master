//! The pure part: turns and limits in, rows and totals out. No clock, no disk, no agent names.

use super::model::{Files, LimitFact, Limits, PlanRow, Totals, TurnFact};
use std::collections::BTreeMap;

/// How ids become words. The app fills it from the agent registry and the accounts store.
pub struct Labels<'a> {
    pub agent: &'a dyn Fn(&str) -> String,
    /// The account's label, `None` when the account no longer exists.
    pub account: &'a dyn Fn(&str) -> Option<String>,
}

pub const DEFAULT_ACCOUNT: &str = "Default account";
pub const REMOVED_ACCOUNT: &str = "Removed account";

/// The part of `[start, end]` inside `[from, to)`.
fn overlap_ms(start: u64, end: u64, from: u64, to: u64) -> u64 {
    end.min(to).saturating_sub(start.max(from))
}

type Key = (String, Option<String>);

fn new_row(agent: &str, account: &Option<String>, labels: &Labels) -> PlanRow {
    PlanRow {
        agent_id: agent.to_string(),
        agent_label: (labels.agent)(agent),
        account_id: account.clone(),
        account_label: match account {
            None => DEFAULT_ACCOUNT.to_string(),
            Some(id) => (labels.account)(id).unwrap_or_else(|| REMOVED_ACCOUNT.to_string()),
        },
        turns: 0,
        busy_ms: 0,
        files: Files::default(),
        merged: 0,
        limits: Limits::default(),
    }
}

/// Rows (one per agent and account) and their totals for the month `[from, to)`.
///
/// - A turn belongs to the month it started in.
/// - A limit hit belongs to the month it started in; its waiting time is the part inside the month.
pub fn aggregate(turns: &[TurnFact], limits: &[LimitFact], from: u64, to: u64, labels: &Labels) -> (Vec<PlanRow>, Totals) {
    let mut rows: BTreeMap<Key, PlanRow> = BTreeMap::new();

    for turn in turns.iter().filter(|t| t.started_ms >= from && t.started_ms < to) {
        let row = rows
            .entry((turn.agent_id.clone(), turn.account.clone()))
            .or_insert_with(|| new_row(&turn.agent_id, &turn.account, labels));
        row.turns += 1;
        row.busy_ms += turn.busy_ms;
        if turn.merged {
            row.merged += 1;
        }
        if let Some(files) = turn.files {
            row.files.known_turns += 1;
            row.files.changed += files.count;
            row.files.added += files.added;
            row.files.deleted += files.deleted;
        }
    }
    for hit in limits {
        let waited = hit.end_ms.map(|end| overlap_ms(hit.start_ms, end, from, to)).unwrap_or(0);
        let starts_here = hit.start_ms >= from && hit.start_ms < to;
        if !starts_here && waited == 0 {
            continue;
        }
        let row = rows
            .entry((hit.agent_id.clone(), hit.account.clone()))
            .or_insert_with(|| new_row(&hit.agent_id, &hit.account, labels));
        if starts_here {
            row.limits.hits += 1;
        }
        row.limits.limited_ms += waited;
    }

    let mut rows: Vec<PlanRow> = rows.into_values().collect();
    rows.sort_by(|a, b| {
        b.turns
            .cmp(&a.turns)
            .then_with(|| a.agent_label.to_lowercase().cmp(&b.agent_label.to_lowercase()))
            .then_with(|| a.account_label.to_lowercase().cmp(&b.account_label.to_lowercase()))
    });
    let totals = rows.iter().fold(Totals::default(), |mut t, row| {
        t.turns += row.turns;
        t.busy_ms += row.busy_ms;
        t.files_changed += row.files.changed;
        t.added += row.files.added;
        t.deleted += row.files.deleted;
        t.merged += row.merged;
        t.limit_hits += row.limits.hits;
        t.limited_ms += row.limits.limited_ms;
        t
    });
    (rows, totals)
}
