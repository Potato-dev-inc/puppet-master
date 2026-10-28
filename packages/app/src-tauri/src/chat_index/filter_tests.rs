//! The chips' filters of "Search all chats": each one alone, combined, with no typed words, bound as values (a quote
//! or an SQL fragment in a folder name is only text), the facets for the menus, and the switch.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::entry::{Entry, PaneContext};
use super::feeder::ChatIndex;
use super::filter::{Facets, Filters, Role};
use super::store::{Hit, Store};
use super::{ContextLookup, Answer};
use serde_json::json;

const DAY: i64 = 86_400_000;

struct Nobody;

impl ContextLookup for Nobody {
    fn context(&self, _pane_id: &str) -> Option<PaneContext> {
        None
    }
}

fn row_of(id: &str, agent: &str, folder: &str, kind: &'static str, text: &str, at_ms: i64) -> Entry {
    Entry {
        pane_id: "pane-1".into(),
        session: "s".into(),
        item_id: id.into(),
        kind,
        agent: agent.into(),
        folder: folder.into(),
        at_ms,
        text: text.into(),
        imported: false,
    }
}

/// Five items: two agents, two folders, all three kinds, three different days.
fn chats() -> Store {
    let mut store = Store::open_memory(100).unwrap();
    store
        .insert(&[
            row_of("1:0", "codex", "demo-a", "user", "fix the login bug", 10 * DAY),
            row_of("2:0", "codex", "demo-a", "assistant", "the login bug is in the token check", 10 * DAY + 5),
            row_of("3:0", "claude", "demo-a", "assistant", "login bug fixed in the token check", 20 * DAY),
            row_of("4:0", "claude", "shop", "user", "login page copy please", 20 * DAY + 5),
            row_of("5:0", "codex", "shop", "tool", "Bash login test", 30 * DAY),
        ])
        .unwrap();
    store
}

fn ids(hits: &[Hit]) -> Vec<&str> {
    hits.iter().map(|h| h.item_id.as_str()).collect()
}

#[test]
fn each_filter_narrows_the_search_on_its_own() {
    let store = chats();
    let by = |filters: Filters| store.search_filtered("login", 50, &filters).unwrap();
    let agent = Filters { agent: Some("codex".into()), ..Default::default() };
    assert_eq!(ids(&by(agent)), vec!["5:0", "2:0", "1:0"]);
    let folder = Filters { folder: Some("shop".into()), ..Default::default() };
    assert_eq!(ids(&by(folder)), vec!["5:0", "4:0"]);
    let since = Filters { since_ms: Some(15 * DAY), ..Default::default() };
    assert_eq!(ids(&by(since)), vec!["5:0", "4:0", "3:0"]);
    let you = Filters { role: Some(Role::You), ..Default::default() };
    assert_eq!(ids(&by(you)), vec!["4:0", "1:0"]);
    let agent_said = Filters { role: Some(Role::Agent), ..Default::default() };
    assert_eq!(ids(&by(agent_said)), vec!["3:0", "2:0"]);
    let tools = Filters { role: Some(Role::Tool), ..Default::default() };
    assert_eq!(ids(&by(tools)), vec!["5:0"]);
}

#[test]
fn filters_combine_and_every_one_must_hold() {
    let store = chats();
    let codex_in_demo = Filters { agent: Some("codex".into()), folder: Some("demo-a".into()), ..Default::default() };
    assert_eq!(ids(&store.search_filtered("login", 50, &codex_in_demo).unwrap()), vec!["2:0", "1:0"]);
    let all_four = Filters { agent: Some("claude".into()), folder: Some("demo-a".into()), since_ms: Some(15 * DAY), role: Some(Role::Agent) };
    assert_eq!(ids(&store.search_filtered("login", 50, &all_four).unwrap()), vec!["3:0"]);
    // A combination no item has is an empty list, not an error.
    let none = Filters { agent: Some("claude".into()), role: Some(Role::Tool), ..Default::default() };
    assert!(store.search_filtered("login", 50, &none).unwrap().is_empty());
}

#[test]
fn no_filter_is_the_old_search_and_an_empty_value_means_any() {
    let store = chats();
    let plain = store.search("login", 50).unwrap();
    assert_eq!(plain.len(), 5);
    assert_eq!(store.search_filtered("login", 50, &Filters::default()).unwrap(), plain);
    let blank = Filters { agent: Some(String::new()), folder: Some(String::new()), ..Default::default() };
    assert_eq!(store.search_filtered("login", 50, &blank).unwrap(), plain);
}

#[test]
fn filters_without_typed_words_list_the_newest_items_that_pass() {
    let store = chats();
    let codex = Filters { agent: Some("codex".into()), ..Default::default() };
    let hits = store.search_filtered("", 50, &codex).unwrap();
    assert_eq!(ids(&hits), vec!["5:0", "2:0", "1:0"]);
    // Nothing is marked: there is no match to mark.
    assert!(hits.iter().all(|h| h.marks.is_empty()));
    assert_eq!(hits[1].snippet, "the login bug is in the token check");
    // No words and no filter: nothing (an empty box never lists every chat).
    assert!(store.search_filtered("  ", 50, &Filters::default()).unwrap().is_empty());
}

#[test]
fn a_long_text_is_a_short_preview_when_nothing_is_matched() {
    let mut store = Store::open_memory(10).unwrap();
    store.insert(&[row_of("9:0", "agent-x", "shop", "assistant", &"word ".repeat(100), 1)]).unwrap();
    let agent = Filters { agent: Some("agent-x".into()), ..Default::default() };
    let hits = store.search_filtered("", 5, &agent).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].snippet.chars().count(), 201);
    assert!(hits[0].snippet.ends_with('…'));
}

#[test]
fn a_folder_or_agent_with_a_quote_or_sql_is_only_a_value() {
    let mut store = Store::open_memory(100).unwrap();
    let tricky = "it's \"the\" shop'); DROP TABLE chat_items; --";
    store.insert(&[row_of("1:0", "codex", tricky, "user", "login here", 10), row_of("2:0", "codex", "demo-a", "user", "login there", 20)]).unwrap();
    // The exact folder name finds its row, quotes and all.
    let folder = Filters { folder: Some(tricky.into()), ..Default::default() };
    assert_eq!(ids(&store.search_filtered("login", 10, &folder).unwrap()), vec!["1:0"]);
    // An injection attempt matches nothing and drops nothing.
    let attack = Filters { folder: Some("x' OR '1'='1".into()), ..Default::default() };
    assert!(store.search_filtered("login", 10, &attack).unwrap().is_empty());
    let attack_agent = Filters { agent: Some("codex' OR 1=1 --".into()), ..Default::default() };
    assert!(store.search_filtered("", 10, &attack_agent).unwrap().is_empty());
    assert_eq!(store.count().unwrap(), 2);
    // Quotes and operators typed as words stay text.
    assert!(store.search_filtered("login\" OR 1=1", 10, &Filters::default()).is_ok());
}

#[test]
fn the_filters_from_the_app_are_camel_case_and_who_spoke_is_you_agent_or_tool() {
    let filters: Filters = serde_json::from_value(json!({ "agent": "codex", "folder": "demo-a", "sinceMs": 5, "role": "you" })).unwrap();
    assert_eq!(filters, Filters { agent: Some("codex".into()), folder: Some("demo-a".into()), since_ms: Some(5), role: Some(Role::You) });
    let agent: Filters = serde_json::from_value(json!({ "role": "agent" })).unwrap();
    assert_eq!(agent.role, Some(Role::Agent));
    let tool: Filters = serde_json::from_value(json!({ "role": "tool" })).unwrap();
    assert_eq!(tool.role, Some(Role::Tool));
    assert!(serde_json::from_value::<Filters>(json!({ "role": "robot" })).is_err());
    assert!(serde_json::from_value::<Filters>(json!({})).unwrap().is_empty());
}

#[test]
fn facets_list_each_agent_and_folder_once_sorted() {
    let store = chats();
    let facets = store.facets().unwrap();
    assert_eq!(facets.agents, vec!["claude", "codex"]);
    assert_eq!(facets.folders, vec!["demo-a", "shop"]);
    assert_eq!(Store::open_memory(10).unwrap().facets().unwrap(), Facets::default());
}

#[test]
fn the_index_answers_filtered_searches_and_facets_only_while_it_is_on() {
    let on = Arc::new(AtomicBool::new(true));
    let index = ChatIndex::start_switched(chats(), Arc::new(Nobody), on.clone());
    let codex = Filters { agent: Some("codex".into()), ..Default::default() };
    let answer: Answer = index.search_filtered("login", 50, &codex).unwrap();
    assert!(answer.enabled);
    assert_eq!(answer.hits.len(), 3);
    assert_eq!(index.facets().unwrap().folders, vec!["demo-a", "shop"]);
    on.store(false, Ordering::SeqCst);
    assert_eq!(index.search_filtered("login", 50, &codex).unwrap(), Answer { enabled: false, hits: Vec::new() });
    assert_eq!(index.facets().unwrap(), Facets::default());
}
