//! `derive`: everything that needs the person right now, one entry per thing.
//!
//! This is an exact port of `deriveNeedsYou` in `packages/app/src/lib/attention.ts`: the same five
//! kinds in the same order (approvals in pane order, questions, errors in pane order, stuck panes,
//! Ask-bar answers), the same ids, and the same quirks (an errored pane that is also stuck yields
//! both items). `attention/tests/parity.rs` runs the shared corpus
//! (`packages/shared/fixtures/attention/*.json`) through both implementations. Rule changes land
//! in later, separate commits; this function does not change with them.

use super::model::{Derived, Facts, Kind, Level};
use std::collections::HashSet;

pub fn derive(facts: &Facts) -> Vec<Derived> {
    let mut items = Vec::new();
    let known: HashSet<&str> = facts.panes.iter().map(|p| p.id.as_str()).collect();
    let since = |pane_id: &str| facts.status_since.get(pane_id).copied();

    for pane in &facts.panes {
        if pane.status == "waiting_input" {
            let risk = facts
                .prompts
                .get(&pane.id)
                .and_then(|p| p.as_ref())
                .and_then(|p| p.risk.as_ref())
                .and_then(|r| r.level.as_deref())
                .and_then(Level::parse);
            items.push(Derived {
                id: format!("approval:{}", pane.id),
                kind: Kind::Approval,
                pane_id: Some(pane.id.clone()),
                risk,
                since: since(&pane.id),
            });
        }
    }
    for q in &facts.questions {
        let pane_id = q.pane_id.as_deref().filter(|id| !id.is_empty() && known.contains(id)).map(str::to_string);
        // A consent card is not a question the agent asked: it is a decision about the agent.
        if q.consent.as_ref().is_some_and(truthy) {
            items.push(Derived { id: format!("consent:{}", q.id), kind: Kind::Consent, pane_id: None, risk: Some(Level::High), since: q.created_at_ms });
        } else {
            items.push(Derived { id: format!("question:{}", q.id), kind: Kind::Question, pane_id, risk: None, since: q.created_at_ms });
        }
    }
    for pane in &facts.panes {
        if pane.status == "error" {
            items.push(Derived {
                id: format!("error:{}", pane.id),
                kind: Kind::Error,
                pane_id: Some(pane.id.clone()),
                risk: None,
                since: since(&pane.id),
            });
        }
    }
    for id in &facts.stuck_pane_ids {
        if known.contains(id.as_str()) {
            items.push(Derived { id: format!("stuck:{id}"), kind: Kind::Stuck, pane_id: Some(id.clone()), risk: None, since: None });
        }
    }
    for n in &facts.notices {
        items.push(Derived { id: format!("answer:{}", n.id), kind: Kind::Answer, pane_id: None, risk: None, since: n.at_ms });
    }
    items
}

/// JavaScript truthiness of a JSON value (`if (q.consent)`).
fn truthy(value: &serde_json::Value) -> bool {
    use serde_json::Value;
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// Panes that have something waiting on them, for the sidebar's "Needs you" group.
pub fn pane_ids(items: &[Derived]) -> HashSet<&str> {
    items.iter().filter_map(|i| i.pane_id.as_deref()).collect()
}
