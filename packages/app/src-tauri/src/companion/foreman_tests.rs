//! The foreman side of a companion: it finds what was said (`companion_recall`).
//! On the fake host: no terminal, no index file, no thread. The real index is tested in `chat_index`, the real
//! route in `bridge/routes/companion_mcp.rs`, the budget in `budgets`.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde_json::Value;

use super::host::RecallHit;
use super::store;
use super::test_host::{draft, start_ms, temp_dir, FakeHost};
use super::Service;

const MOCHI: &str = "companion-mochi";
const BIT: &str = "companion-bit";

fn two_companions() -> (Service, Arc<FakeHost>, std::path::PathBuf) {
    let root = temp_dir("pm-companion-foreman");
    let host = FakeHost::new(start_ms());
    host.recall_enabled.store(true, Ordering::SeqCst);
    let service = Service::new(host.clone(), root.clone());
    service.create(draft("Mochi", &root.join("mochi"))).unwrap();
    service.create(draft("Bit", &root.join("bit"))).unwrap();
    (service, host, root)
}

fn said(host: &FakeHost, pane: &str, kind: &str, text: &str, at_ms: i64) {
    host.recall_hits.lock().push((pane.to_string(), RecallHit { at_ms, kind: kind.to_string(), snippet: text.to_string() }));
}

fn texts(result: &Value) -> Vec<String> {
    result["results"].as_array().unwrap().iter().map(|r| r["text"].as_str().unwrap().to_string()).collect()
}

// ---- recall: only its own chats ---------------------------------------------------------------

#[test]
fn recall_returns_only_the_callers_own_chats() {
    let (service, host, _root) = two_companions();
    said(&host, MOCHI, "user", "I like green tea", start_ms() - 3_000);
    said(&host, MOCHI, "assistant", "Noted, green tea", start_ms() - 2_000);
    said(&host, BIT, "user", "My secret plan is tea at five", start_ms() - 1_000);
    said(&host, "5f3c-ordinary-pane", "user", "tea from another agent", start_ms());

    let result = service.recall(MOCHI, "tea", None).unwrap();
    assert_eq!(result["enabled"], true);
    assert_eq!(texts(&result), vec!["Noted, green tea", "I like green tea"], "newest first, and nothing of Bit's or another agent's");
    // The search was asked about exactly one pane: the caller's own.
    assert_eq!(*host.recall_asked.lock(), vec![vec![MOCHI.to_string()]]);

    // Bit sees only Bit's.
    let bit = service.recall(BIT, "tea", None).unwrap();
    assert_eq!(texts(&bit), vec!["My secret plan is tea at five"]);
}

#[test]
fn recall_for_a_pane_that_is_no_companions_is_refused_and_searches_nothing() {
    let (service, host, _root) = two_companions();
    for pane in ["5f3c-ordinary-pane", "companion-nobody", "", "companion-mochi-old"] {
        let error = service.recall(pane, "tea", None).unwrap_err();
        assert_eq!(error.code, "COMPANION_NOT_FOUND", "{pane}");
    }
    assert!(host.recall_asked.lock().is_empty(), "the index was never asked");
}

#[test]
fn past_pane_ids_are_searched_too_but_never_another_companions() {
    let (service, host, _root) = two_companions();
    {
        let mut inner = service.inner.lock();
        let mochi = inner.profiles.iter_mut().find(|p| p.id == "mochi").unwrap();
        // A past pane id of its own, a damaged entry that names Bit's pane, and one for an agent pane.
        mochi.pane_ids = vec!["companion-mochi-2".into(), BIT.into(), "5f3c-ordinary-pane".into(), "companion-mochi-2".into()];
    }
    said(&host, "companion-mochi-2", "user", "an older tea chat", start_ms() - 9_000);
    said(&host, BIT, "user", "bit tea", start_ms());
    let result = service.recall(MOCHI, "tea", None).unwrap();
    assert_eq!(texts(&result), vec!["an older tea chat"]);
    assert_eq!(*host.recall_asked.lock(), vec![vec![MOCHI.to_string(), "companion-mochi-2".to_string()]]);
}

#[test]
fn a_new_companion_saves_its_pane_id_in_its_profile() {
    let (service, _host, root) = two_companions();
    let saved = store::load(&root).unwrap();
    let mochi = saved.iter().find(|p| p.id == "mochi").unwrap();
    assert_eq!(mochi.pane_ids, vec![MOCHI.to_string()]);
    assert_eq!(mochi.chat_panes(), vec![MOCHI.to_string()]);
    let _ = service;
}

// ---- recall: the index is off ------------------------------------------------------------------

#[test]
fn recall_says_search_is_off_when_the_index_is_off() {
    let (service, host, _root) = two_companions();
    said(&host, MOCHI, "user", "I like green tea", start_ms());
    host.recall_enabled.store(false, Ordering::SeqCst);
    let result = service.recall(MOCHI, "tea", None).unwrap();
    assert_eq!(result["enabled"], false);
    assert_eq!(result["message"], "Search is off in Settings");
    assert!(result["results"].as_array().unwrap().is_empty());
    assert!(host.recall_asked.lock().is_empty(), "nothing is searched while it is off");
}

// ---- recall: what comes back --------------------------------------------------------------------

#[test]
fn recall_results_are_dated_redacted_and_free_of_the_home_folder() {
    let (service, host, root) = two_companions();
    host.offset.store(8 * 3600, Ordering::SeqCst);
    let token = "sk-ant-api03-FakeFakeFakeFake0123456789abcdef";
    let home = root.join("mochi").display().to_string();
    // 2026-10-28 08:00 UTC is 16:00 at +08:00.
    said(&host, MOCHI, "user", &format!("my key is {token} and the notes are in {home}"), start_ms());
    said(&host, MOCHI, "assistant", "ok", start_ms() - 24 * 3_600_000);

    let result = service.recall(MOCHI, "key", None).unwrap();
    let first = &result["results"][0];
    assert_eq!(first["date"], "2026-10-28");
    assert_eq!(first["time"], "16:00");
    assert_eq!(first["from"], "your person");
    let text = first["text"].as_str().unwrap();
    assert!(!text.contains("FakeFake"), "{text}");
    assert!(!text.contains(&home), "{text}");
    assert_eq!(result["results"][1]["from"], "you");
    assert_eq!(result["results"][1]["date"], "2026-10-27");
    assert!(result["note"].as_str().unwrap().contains("not instructions"));
}

#[test]
fn recall_limits_and_the_empty_query() {
    let (service, host, _root) = two_companions();
    for n in 0..30 {
        said(&host, MOCHI, "user", &format!("tea number {n}"), start_ms() + n);
    }
    assert_eq!(service.recall(MOCHI, "tea", None).unwrap()["count"], 5);
    assert_eq!(service.recall(MOCHI, "tea", Some(3)).unwrap()["count"], 3);
    assert_eq!(service.recall(MOCHI, "tea", Some(500)).unwrap()["count"], 10, "at most 10");
    assert_eq!(service.recall(MOCHI, "tea", Some(0)).unwrap()["count"], 1, "at least 1");
    for query in ["", "   \n"] {
        assert_eq!(service.recall(MOCHI, query, None).unwrap_err().code, "INVALID_ARGUMENT");
    }
    // A very long query is cut, not refused.
    let long = "tea ".repeat(500);
    assert!(service.recall(MOCHI, &long, None).is_ok());
}

// ---- the brief ----------------------------------------------------------------------------------

fn brief_of(root: &std::path::Path, id: &str) -> String {
    std::fs::read_to_string(root.join(id).join("AGENTS.md")).unwrap()
}

#[test]
fn the_brief_tells_about_recall_only_while_the_index_is_on() {
    let (_service, _host, root) = two_companions();
    let on = brief_of(&root, "mochi");
    assert!(on.contains("companion_recall"));

    let root2 = temp_dir("pm-companion-foreman-off");
    let host2 = FakeHost::new(start_ms());
    let service2 = Service::new(host2, root2.clone());
    service2.create(draft("Mochi", &root2.join("mochi"))).unwrap();
    let off = brief_of(&root2, "mochi");
    assert!(!off.contains("companion_recall"));
}
