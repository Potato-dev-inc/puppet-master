//! The foreman side of a companion: it finds what was said (`companion_recall`) and it hires helpers.
//! On the fake host: no terminal, no index file, no thread. The real index is tested in `chat_index`, the real
//! route in `bridge/routes/companion_mcp.rs`, the budget in `budgets`.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde_json::Value;

use super::caller::{decide, CompanionPane};
use super::guard;
use super::host::RecallHit;
use super::model::FeedKind;
use super::store;
use super::test_host::{draft, start_ms, temp_dir, FakeHost};
use super::Service;
use crate::pty::agent_proc::ProcEntry;

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
    assert!(on.contains("run_agent"), "the hiring rule is always there");

    let root2 = temp_dir("pm-companion-foreman-off");
    let host2 = FakeHost::new(start_ms());
    let service2 = Service::new(host2, root2.clone());
    service2.create(draft("Mochi", &root2.join("mochi"))).unwrap();
    let off = brief_of(&root2, "mochi");
    assert!(!off.contains("companion_recall"));
    assert!(off.contains("run_agent"));
    assert!(off.contains("only they can answer it"));
}

// ---- a companion hires helpers -------------------------------------------------------------------

#[test]
fn a_started_helper_becomes_a_delegated_line_in_the_feed() {
    let (service, host, root) = two_companions();
    service.note_delegated(MOCHI, "Codex", "demo-a");
    let feed = store::feed_read(&root, "mochi").unwrap();
    let line = feed.iter().find(|item| item.kind == FeedKind::Delegated).expect("a delegated line");
    assert_eq!(line.text, "Mochi started a Codex helper in demo-a.");
    assert_eq!(line.at_ms, host.now());
    // Only that companion's feed.
    assert!(store::feed_read(&root, "bit").unwrap().iter().all(|item| item.kind != FeedKind::Delegated));
    // An unknown pane writes nothing, and a missing folder name is not printed as "in .".
    service.note_delegated("5f3c-ordinary-pane", "Codex", "demo-a");
    service.note_delegated(BIT, "Codex", "  ");
    let bit = store::feed_read(&root, "bit").unwrap();
    assert_eq!(bit.iter().find(|item| item.kind == FeedKind::Delegated).unwrap().text, "Bit started a Codex helper.");
    assert_eq!(store::feed_read(&root, "mochi").unwrap().iter().filter(|item| item.kind == FeedKind::Delegated).count(), 1);
}

#[test]
fn a_delegated_line_never_carries_the_home_folder_or_a_secret() {
    let (service, _host, root) = two_companions();
    let home = root.join("mochi").display().to_string();
    service.note_delegated(MOCHI, "Codex", &home);
    let line = store::feed_read(&root, "mochi").unwrap().into_iter().find(|i| i.kind == FeedKind::Delegated).unwrap();
    assert!(!line.text.contains(&home), "{}", line.text);
}

#[test]
fn a_helper_may_not_work_in_the_companions_home_folder() {
    let (service, _host, root) = two_companions();
    let home = root.join("mochi");
    assert!(service.is_home_path(MOCHI, &home.display().to_string()));
    assert!(service.is_home_path(MOCHI, &home.join("memory").display().to_string()));
    assert!(!service.is_home_path(MOCHI, &root.join("bit").display().to_string()), "Bit's home is not Mochi's");
    assert!(!service.is_home_path(MOCHI, &root.join("mochi2").display().to_string()));
    assert!(!service.is_home_path("5f3c-ordinary-pane", &home.display().to_string()));
}

// ---- the hard rule: a companion never answers a helper's card ---------------------------------------

fn proc(pid: u32, parent: u32) -> ProcEntry {
    ProcEntry { pid, parent, image: "x.exe".into() }
}

/// Mochi's pane process 100 -> its agent 101 -> its Puppet Master MCP shim 102. The helper it started is a
/// separate pane: 400 -> 401 -> its own shim 402. An ordinary agent of the person: 300 -> 301.
fn table() -> Vec<ProcEntry> {
    vec![proc(100, 1), proc(101, 100), proc(102, 101), proc(300, 1), proc(301, 300), proc(400, 1), proc(401, 400), proc(402, 401)]
}

fn mochi_pane() -> Vec<CompanionPane> {
    vec![CompanionPane { id: MOCHI.into(), roots: vec![100], exited: false }]
}

#[test]
fn the_companion_cannot_answer_its_helpers_card_whatever_it_claims_to_be() {
    let helper_card = ["/agents/answer", "/panes/h-1/key", "/panes/h-1/input", "/panes/h-1/opencode/permissions/p1/reply", "/panes/h-1/opencode/question/reply"];
    // (what the shim's process is, the pane it says it runs in)
    for (label, pid, claim) in [
        ("really in Mochi's pane, says so", 102, Some(MOCHI)),
        ("really in Mochi's pane, says nothing", 102, None),
        ("really in Mochi's pane, claims to be an ordinary pane", 102, Some("5f3c-ordinary-pane")),
        ("elsewhere but claims to be Mochi", 301, Some(MOCHI)),
    ] {
        let verdict = decide(&mochi_pane(), Some(&table()), Some(pid), claim);
        for path in helper_card {
            let refusal = guard::check("POST", path, verdict.suspected.is_some());
            assert_eq!(refusal.unwrap_err().code, "COMPANION_CANNOT_ANSWER", "{label}: {path}");
        }
    }
    // The person's own agents are not touched by the rule.
    let ordinary = decide(&mochi_pane(), Some(&table()), Some(301), Some("5f3c-ordinary-pane"));
    for path in helper_card {
        assert!(guard::check("POST", path, ordinary.suspected.is_some()).is_ok(), "{path}");
    }
}

#[test]
fn the_companion_can_still_see_that_a_helper_waits() {
    // Seeing is reading: the waiting status comes from wait_agents / list_agents / inspect_agent, none of
    // which is an answering route.
    for (method, path) in [("POST", "/agents/wait"), ("GET", "/agents"), ("GET", "/agents/h-1"), ("POST", "/panes/wait"), ("GET", "/panes")] {
        assert!(guard::check(method, path, true).is_ok(), "{method} {path}");
    }
}

#[test]
fn a_helper_is_not_a_companion_even_though_the_companion_started_it() {
    // The helper's own agent (pane 400..402) runs in an ordinary pane: it is not the companion, so it is
    // not the companion's voice. Its own questions go to the person like any agent's.
    let verdict = decide(&mochi_pane(), Some(&table()), Some(402), None);
    assert_eq!(verdict.suspected, None);
    assert_eq!(verdict.verified, None);
}

// ---- the same, over a real pane registry and the real process table ---------------------------------

use crate::pty::registry::PaneRegistry;
use parking_lot::Mutex;

/// A registry with Mochi's pane (whose process is this test process) and an ordinary agent pane.
fn registry_with_mochi() -> (Arc<Mutex<PaneRegistry>>, String) {
    let ordinary = uuid::Uuid::new_v4().to_string();
    let registry = Arc::new(Mutex::new(PaneRegistry::new()));
    {
        let mut reg = registry.lock();
        let mut mochi = PaneRegistry::test_pane_stub(MOCHI);
        mochi.info.pid = std::process::id();
        reg.panes.insert(MOCHI.to_string(), mochi);
        let mut other = PaneRegistry::test_pane_stub(&ordinary);
        other.info.pid = u32::MAX - 11;
        reg.panes.insert(ordinary.clone(), other);
    }
    (registry, ordinary)
}

fn session_in(pane: Option<&str>, pid: u32) -> String {
    let session = uuid::Uuid::new_v4().to_string();
    if let Some(pane) = pane {
        crate::mcp_sessions::note_session_pane(&session, pane);
    }
    crate::mcp_sessions::note_session_pid(&session, pid);
    session
}

#[test]
fn over_the_real_registry_a_companions_session_is_known_and_refused_every_answering_route() {
    let (registry, _ordinary) = registry_with_mochi();
    let session = session_in(Some(MOCHI), std::process::id());
    let verdict = super::caller::of_session(&registry, &session);
    assert_eq!(verdict.suspected.as_deref(), Some(MOCHI));
    if crate::pty::agent_proc::snapshot().is_some() {
        assert_eq!(verdict.verified.as_deref(), Some(MOCHI), "this process sits in Mochi's pane");
    }
    for (method, path) in [("POST", "/agents/answer"), ("POST", "/panes/h1/key"), ("POST", "/panes/h1/input")] {
        let error = guard::check_session(&registry, &session, method, path).unwrap_err();
        assert_eq!(error.code, "COMPANION_CANNOT_ANSWER", "{path}");
    }
    // Starting helpers and watching them is not refused.
    for path in ["/agents/run", "/agents/wait", "/agents"] {
        assert!(guard::check_session(&registry, &session, "POST", path).is_ok(), "{path}");
    }
}

#[test]
fn over_the_real_registry_an_ordinary_agent_still_answers_its_own_cards() {
    let (registry, ordinary) = registry_with_mochi();
    // An agent of the person's, in its own pane: the guard does not touch it.
    let session = session_in(Some(&ordinary), u32::MAX - 11);
    let verdict = super::caller::of_session(&registry, &session);
    assert_eq!(verdict.suspected, None);
    assert!(guard::check_session(&registry, &session, "POST", "/agents/answer").is_ok());
    // A connection that never said where it runs (no pane header, an unknown process): not a companion either.
    let silent = session_in(None, u32::MAX - 13);
    assert!(guard::check_session(&registry, &silent, "POST", "/agents/answer").is_ok());
}

#[test]
fn over_the_real_registry_saying_nothing_does_not_hide_a_companions_session() {
    if crate::pty::agent_proc::snapshot().is_none() {
        return; // The process table is not available here: only the claim can be judged.
    }
    let (registry, _ordinary) = registry_with_mochi();
    // No pane header at all, but the process really is inside Mochi's pane.
    let session = session_in(None, std::process::id());
    let verdict = super::caller::of_session(&registry, &session);
    assert_eq!(verdict.verified.as_deref(), Some(MOCHI));
    assert!(guard::check_session(&registry, &session, "POST", "/agents/answer").is_err());
}
