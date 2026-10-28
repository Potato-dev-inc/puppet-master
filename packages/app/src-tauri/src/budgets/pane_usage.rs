//! Usage per helper pane: the turns and busy time of each pane's run, rolled up through
//! `parent_pane_id`. A helper of a helper counts into the top parent too.
//!
//! Turns and busy time come from the operation records (`accounting::counts_as_turn`), never from
//! provider usage. Tokens are shown only when the pane's chat already reports them (its `Usage`
//! item): nothing is read off the screen and no chat reader is opened for them.

use super::accounting::counts_as_turn;
use crate::agent_runs::pane_api::run_of_pane;
use crate::operations::{self, OperationSnapshot};
use crate::pty::PaneRegistry;
use crate::session_chat::{ChatItem, Watchers};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

/// Newest chat items scanned for a pane's latest `Usage` item.
const TOKEN_SCAN_ITEMS: usize = 300;

/// One pane as the roll-up sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneLink {
    pub id: String,
    /// `parent_pane_id`: the pane that started this one.
    pub parent: Option<String>,
    /// The pane's account label, only when it runs under a known account.
    pub account_label: Option<String>,
}

/// Turns and busy time of one pane's run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Own {
    pub turns: u32,
    pub busy_ms: u64,
}

/// Tokens the pane's chat reported for its latest reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenCount {
    pub input: u64,
    pub output: u64,
}

/// One direct helper: its own turns (nested helpers are in the parent's roll-up only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperUsage {
    pub pane_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_label: Option<String>,
    pub turns: u32,
    pub busy_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenCount>,
}

/// A pane's own usage and the roll-up of every helper under it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneUsage {
    pub own: Own,
    /// Every pane under this one, nested helpers included.
    pub helper_count: u32,
    pub helper_turns: u32,
    pub helper_busy_ms: u64,
    /// The direct helpers, in list order.
    pub helpers: Vec<HelperUsage>,
}

/// A turn record cut down to what usage needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnRecord {
    pub counted: bool,
    pub started_at_ms: Option<u64>,
    pub finished_at_ms: Option<u64>,
}

impl TurnRecord {
    pub fn of(turn: &OperationSnapshot) -> Self {
        Self { counted: counts_as_turn(turn), started_at_ms: turn.started_at_ms, finished_at_ms: turn.finished_at_ms }
    }
}

/// The turns of a run, and the busy time of its finished turns. A turn still running adds its time
/// once it ends.
pub fn summarize(turns: &[TurnRecord]) -> Own {
    let mut own = Own::default();
    for turn in turns.iter().filter(|turn| turn.counted) {
        own.turns += 1;
        if let (Some(started), Some(finished)) = (turn.started_at_ms, turn.finished_at_ms) {
            own.busy_ms += finished.saturating_sub(started);
        }
    }
    own
}

/// Every pane under `pane_id`, nearest first. A loop of parents lists each pane once.
pub fn descendants<'a>(links: &'a [PaneLink], pane_id: &str) -> Vec<&'a PaneLink> {
    let mut children: HashMap<&str, Vec<&PaneLink>> = HashMap::new();
    for link in links {
        if let Some(parent) = link.parent.as_deref().filter(|parent| *parent != link.id) {
            children.entry(parent).or_default().push(link);
        }
    }
    let mut seen: HashSet<&str> = HashSet::from([pane_id]);
    let mut out = Vec::new();
    let mut queue: VecDeque<&str> = VecDeque::from([pane_id]);
    while let Some(at) = queue.pop_front() {
        for child in children.get(at).into_iter().flatten() {
            if seen.insert(child.id.as_str()) {
                out.push(*child);
                queue.push_back(child.id.as_str());
            }
        }
    }
    out
}

/// The roll-up for `pane_id`, from the panes' own usage and the tokens each one reported.
pub fn roll_up(links: &[PaneLink], own: &HashMap<String, Own>, tokens: &HashMap<String, TokenCount>, pane_id: &str) -> PaneUsage {
    let usage_of = |id: &str| own.get(id).copied().unwrap_or_default();
    let below = descendants(links, pane_id);
    let helpers = below
        .iter()
        .filter(|link| link.parent.as_deref() == Some(pane_id))
        .map(|link| {
            let usage = usage_of(&link.id);
            HelperUsage {
                pane_id: link.id.clone(),
                account_label: link.account_label.clone(),
                turns: usage.turns,
                busy_ms: usage.busy_ms,
                tokens: tokens.get(&link.id).copied(),
            }
        })
        .collect();
    PaneUsage {
        own: usage_of(pane_id),
        helper_count: below.len() as u32,
        helper_turns: below.iter().map(|link| usage_of(&link.id).turns).sum(),
        helper_busy_ms: below.iter().map(|link| usage_of(&link.id).busy_ms).sum(),
        helpers,
    }
}

/// The usage of `pane_id` and of the helpers under it. Reads the registry, the operation records of
/// each pane's run and the chat readers that are already open; it opens none.
pub fn usage_of_pane(registry: &Arc<Mutex<PaneRegistry>>, watchers: Option<&Watchers>, pane_id: &str) -> PaneUsage {
    let links: Vec<PaneLink> = registry
        .lock()
        .list()
        .into_iter()
        .map(|info| PaneLink { id: info.id, parent: info.parent_pane_id, account_label: info.account_label })
        .collect();
    let below: Vec<String> = descendants(&links, pane_id).iter().map(|link| link.id.clone()).collect();
    let mut own = HashMap::new();
    let mut tokens = HashMap::new();
    for id in std::iter::once(pane_id.to_string()).chain(below.iter().cloned()) {
        own.insert(id.clone(), own_of_pane(registry, &id));
        if let Some(count) = watchers.and_then(|watchers| latest_tokens(watchers, &id)) {
            tokens.insert(id, count);
        }
    }
    roll_up(&links, &own, &tokens, pane_id)
}

/// Turns and busy time of the run this pane belongs to (none for a pane with no run).
fn own_of_pane(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> Own {
    let Some((project, handle)) = run_of_pane(registry, pane_id) else { return Own::default() };
    match operations::turns_for_handle(&project, &handle) {
        Ok(turns) => summarize(&turns.iter().map(TurnRecord::of).collect::<Vec<_>>()),
        Err(_) => Own::default(),
    }
}

/// The newest `Usage` item the pane's open chat reader has, if the chat reports one.
fn latest_tokens(watchers: &Watchers, pane_id: &str) -> Option<TokenCount> {
    let delta = watchers.since(pane_id, None, TOKEN_SCAN_ITEMS)?;
    delta.items.iter().rev().find_map(|item| match item {
        ChatItem::Usage { input_tokens, output_tokens, .. } => Some(TokenCount { input: *input_tokens, output: *output_tokens }),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(id: &str, parent: Option<&str>) -> PaneLink {
        PaneLink { id: id.into(), parent: parent.map(String::from), account_label: None }
    }

    fn own(turns: u32, busy_ms: u64) -> Own {
        Own { turns, busy_ms }
    }

    #[test]
    fn a_pane_with_no_helpers_rolls_up_to_zero() {
        let links = [link("p", None), link("other", None)];
        let usage = roll_up(&links, &HashMap::from([("p".to_string(), own(4, 90_000))]), &HashMap::new(), "p");
        assert_eq!(usage.own, own(4, 90_000));
        assert_eq!((usage.helper_count, usage.helper_turns, usage.helper_busy_ms), (0, 0, 0));
        assert!(usage.helpers.is_empty());
    }

    #[test]
    fn the_helpers_turns_and_busy_time_add_up_for_the_parent() {
        let links = [link("p", None), link("h1", Some("p")), link("h2", Some("p")), link("stray", None)];
        let own_map = HashMap::from([("h1".to_string(), own(3, 60_000)), ("h2".to_string(), own(2, 30_000)), ("stray".to_string(), own(9, 9))]);
        let usage = roll_up(&links, &own_map, &HashMap::new(), "p");
        assert_eq!((usage.helper_count, usage.helper_turns, usage.helper_busy_ms), (2, 5, 90_000));
        let ids: Vec<&str> = usage.helpers.iter().map(|h| h.pane_id.as_str()).collect();
        assert_eq!(ids, ["h1", "h2"]);
        assert_eq!(usage.helpers[0].turns, 3);
    }

    #[test]
    fn a_helper_of_a_helper_counts_into_the_top_parent_but_not_its_own_row() {
        let links = [link("p", None), link("h1", Some("p")), link("h2", Some("h1"))];
        let own_map = HashMap::from([("h1".to_string(), own(1, 10)), ("h2".to_string(), own(4, 40))]);
        let usage = roll_up(&links, &own_map, &HashMap::new(), "p");
        assert_eq!((usage.helper_count, usage.helper_turns, usage.helper_busy_ms), (2, 5, 50));
        assert_eq!(usage.helpers.len(), 1);
        assert_eq!(usage.helpers[0].turns, 1);
    }

    #[test]
    fn a_loop_of_parents_lists_each_pane_once_and_terminates() {
        let looped = [link("a", Some("b")), link("b", Some("a"))];
        assert_eq!(roll_up(&looped, &HashMap::new(), &HashMap::new(), "a").helper_count, 1);
        let itself = [link("s", Some("s"))];
        assert_eq!(roll_up(&itself, &HashMap::new(), &HashMap::new(), "s").helper_count, 0);
    }

    #[test]
    fn summarize_counts_turns_and_only_the_busy_time_of_finished_turns() {
        let turns = [
            TurnRecord { counted: true, started_at_ms: Some(1_000), finished_at_ms: Some(61_000) },
            TurnRecord { counted: true, started_at_ms: Some(70_000), finished_at_ms: None },
            TurnRecord { counted: false, started_at_ms: None, finished_at_ms: None },
        ];
        assert_eq!(summarize(&turns), own(2, 60_000));
        assert_eq!(summarize(&[]), own(0, 0));
    }

    #[test]
    fn tokens_and_account_show_only_when_known() {
        let mut worker = link("h", Some("p"));
        worker.account_label = Some("Work".into());
        let links = [link("p", None), worker];
        let bare = roll_up(&links, &HashMap::new(), &HashMap::new(), "p");
        let bare_json = serde_json::to_value(&bare.helpers[0]).unwrap();
        assert_eq!(bare_json["accountLabel"], "Work");
        assert!(bare_json.get("tokens").is_none());

        let reported = HashMap::from([("h".to_string(), TokenCount { input: 12_000, output: 3_000 })]);
        let with = roll_up(&links, &HashMap::new(), &reported, "p");
        let with_json = serde_json::to_value(&with.helpers[0]).unwrap();
        assert_eq!(with_json["tokens"]["input"], 12_000);
        assert_eq!(with_json["tokens"]["output"], 3_000);

        let plain = roll_up(&[link("p", None), link("h", Some("p"))], &HashMap::new(), &HashMap::new(), "p");
        let plain_json = serde_json::to_value(&plain.helpers[0]).unwrap();
        assert!(plain_json.get("accountLabel").is_none());
    }

    #[test]
    fn the_roll_up_serialises_in_camel_case() {
        let usage = roll_up(&[link("p", None), link("h", Some("p"))], &HashMap::new(), &HashMap::new(), "p");
        let json = serde_json::to_value(&usage).unwrap();
        assert_eq!(json["helperCount"], 1);
        assert_eq!(json["helperBusyMs"], 0);
        assert_eq!(json["own"]["busyMs"], 0);
    }
}
