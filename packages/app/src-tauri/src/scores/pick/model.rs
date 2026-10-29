//! The types of "Pick for me" (see `scores/pick/mod.rs`). Facts are plain Rust; the answer is
//! camel case JSON. No folder, pane id, run handle, prompt, reply, file name or approval text is in
//! any of these types, except the labels from the agent registry and the accounts store.

use crate::handoff::keep_going::model::Target;
use crate::scores::model::{Rate, Sample};
use serde::Serialize;

/// Days of history a pick reads (the Scores page default).
pub const PICK_DAYS: u32 = 90;
/// Lines in "Why this one" at most.
pub const MAX_CONSIDERED: usize = 24;
/// A label longer than this is cut with "..." inside a sentence.
pub const MAX_LABEL_CHARS: usize = 40;

// ---- facts (internal, not serialised) ----

/// The three measures a pick compares, in decision order.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    ChecksGreen,
    Merged,
    EndedOk,
}

/// The decision order of the measures. The first one with two candidates decides.
pub const METRIC_ORDER: [Metric; 3] = [Metric::ChecksGreen, Metric::Merged, Metric::EndedOk];

/// One login's history in this project: the numbers the pick compares.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Evidence {
    pub turns: u32,
    pub checks_green: Rate,
    pub merged: Rate,
    pub ended_ok: Rate,
}

impl Evidence {
    /// The rate of one measure.
    pub fn rate(&self, m: Metric) -> Rate {
        match m {
            Metric::ChecksGreen => self.checks_green,
            Metric::Merged => self.merged,
            Metric::EndedOk => self.ended_ok,
        }
    }
}

/// One candidate: one agent, with the login that would run.
#[derive(Clone, Debug, PartialEq)]
pub struct Cand {
    /// The agent and the login that would run (`agent` or `agent@account`).
    pub target: Target,
    /// "Codex" or "Codex . Work" (the label with the login).
    pub label: String,
    /// Known vendor only. `""`, `varies` and no vendor are `None`.
    pub vendor: Option<String>,
    /// `Err` holds the plain reason it cannot run.
    pub ready: Result<(), String>,
    /// `Some(Some(ms))` out until that time, `Some(None)` out with no time known.
    pub limited_until: Option<Option<u64>>,
    /// `None` = no row for this login in the window.
    pub evidence: Option<Evidence>,
}

/// The person's usual agents (agent ids): the Ask bar default and the last one used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Usual {
    pub preferred: Option<String>,
    pub last: Option<String>,
}

/// Why a candidate is not picked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Skip {
    /// Cannot run here. The text is the plain reason.
    NotReady(String),
    /// Its own login is out of usage. `Some(ms)` = the time it is out until.
    OutOfUsage(Option<u64>),
    /// Another ready agent of the same known vendor is out of usage.
    SharedPlan { with: String },
}

/// Which of the person's usual agents was used for a fallback.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UsualKind {
    Chosen,
    Last,
    First,
}

/// How the pick was made.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// One clear leader on a measure.
    Clear,
    /// Only one agent is ready.
    OnlyOne,
    /// Too few runs to tell agents apart; the usual one is used.
    TooEarly,
    /// Enough runs, but no clear gap; the usual one is used.
    CloseCall,
    /// No agent is ready.
    NoneReady,
}

/// The result of `select::decide`: an index into the ready candidates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub index: usize,
    pub basis: Basis,
    pub metric: Option<Metric>,
    /// The other candidate that was compared (for `Clear`).
    pub rival: Option<usize>,
    pub usual: Option<UsualKind>,
}

// ---- the command's answer (camel case) ----

/// The agent the menu starts. `account_id` is the opaque id the Scores page already returns.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    pub label: String,
}

/// Whether a line of "Why this one" is a ready agent or a skipped one.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ConsideredState {
    Ready,
    Skipped,
}

/// One line of "Why this one".
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Considered {
    pub label: String,
    pub state: ConsideredState,
    pub line: String,
}

/// The whole answer of `pick_agent`.
#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Pick {
    /// `None` = nothing is ready.
    pub choice: Option<Choice>,
    /// Exactly one sentence, always set, at most 220 characters.
    pub reason: String,
    pub basis: Basis,
    /// The measure that decided (`Clear`) or was compared (`CloseCall`).
    pub metric: Option<Metric>,
    /// The sample of the deciding rate for `Clear`; otherwise `Sample::None`.
    pub sample: Sample,
    /// The chosen one first, then ready, then skipped.
    pub considered: Vec<Considered>,
    /// Plain sentences, shown under "Why this one".
    pub notes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rate(count: u32, total: u32) -> Rate {
        crate::scores::stats::rate(count, total)
    }

    #[test]
    fn basis_uses_snake_case_names() {
        assert_eq!(serde_json::to_value(Basis::Clear).unwrap(), json!("clear"));
        assert_eq!(serde_json::to_value(Basis::OnlyOne).unwrap(), json!("only_one"));
        assert_eq!(serde_json::to_value(Basis::TooEarly).unwrap(), json!("too_early"));
        assert_eq!(serde_json::to_value(Basis::CloseCall).unwrap(), json!("close_call"));
        assert_eq!(serde_json::to_value(Basis::NoneReady).unwrap(), json!("none_ready"));
    }

    #[test]
    fn metric_uses_snake_case_names() {
        assert_eq!(serde_json::to_value(Metric::ChecksGreen).unwrap(), json!("checks_green"));
        assert_eq!(serde_json::to_value(Metric::Merged).unwrap(), json!("merged"));
        assert_eq!(serde_json::to_value(Metric::EndedOk).unwrap(), json!("ended_ok"));
    }

    #[test]
    fn considered_state_uses_snake_case_names() {
        assert_eq!(serde_json::to_value(ConsideredState::Ready).unwrap(), json!("ready"));
        assert_eq!(serde_json::to_value(ConsideredState::Skipped).unwrap(), json!("skipped"));
    }

    #[test]
    fn choice_uses_camel_case_and_omits_a_missing_login() {
        let own = Choice { agent_id: "fake_a".into(), account_id: None, label: "fake_a".into() };
        assert_eq!(serde_json::to_value(&own).unwrap(), json!({ "agentId": "fake_a", "label": "fake_a" }));
        let saved =
            Choice { agent_id: "fake_a".into(), account_id: Some("acc_1".into()), label: "fake_a . Work".into() };
        assert_eq!(
            serde_json::to_value(&saved).unwrap(),
            json!({ "agentId": "fake_a", "accountId": "acc_1", "label": "fake_a . Work" })
        );
    }

    #[test]
    fn pick_uses_camel_case_keys_and_keeps_null_choice() {
        let pick = Pick {
            choice: None,
            reason: "No agent is installed yet.".into(),
            basis: Basis::NoneReady,
            metric: None,
            sample: Sample::None,
            considered: vec![Considered {
                label: "fake_b".into(),
                state: ConsideredState::Skipped,
                line: "Not installed on this computer".into(),
            }],
            notes: vec!["Counted from your own turns in this project, last 90 days.".into()],
        };
        let v = serde_json::to_value(&pick).unwrap();
        assert_eq!(v["choice"], json!(null));
        assert_eq!(v["basis"], json!("none_ready"));
        assert_eq!(v["sample"], json!("none"));
        assert_eq!(v["considered"][0]["state"], json!("skipped"));
        assert_eq!(v["considered"][0]["line"], json!("Not installed on this computer"));
        assert_eq!(v["notes"][0], json!("Counted from your own turns in this project, last 90 days."));
    }

    #[test]
    fn evidence_rate_returns_the_matching_measure() {
        let ev = Evidence { turns: 10, checks_green: rate(9, 10), merged: rate(4, 6), ended_ok: rate(8, 10) };
        assert_eq!(ev.rate(Metric::ChecksGreen), rate(9, 10));
        assert_eq!(ev.rate(Metric::Merged), rate(4, 6));
        assert_eq!(ev.rate(Metric::EndedOk), rate(8, 10));
    }
}
