//! Helpers on fakes: the engine of `service_tests.rs`, plus fake `Hands` that record what they were asked to
//! start. What it shows: nothing runs without Start; a request nobody trusted gets no card; the folder comes from
//! a name and never from a path the bot wrote; the home folder and broad folders are refused; the caps; the
//! companion's house rules reach the run (20 turns, the feed line); and a helper's end gives the brain exactly
//! one turn.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::helpers::*;
use super::protocol::HelperStatus;
use super::service_tests::Rig;
use super::store::{FromKind, MsgKind};
use crate::agent_runs::AgentRunRequest;
use crate::companion::broad::Places;

struct FakeHands {
    agent: Mutex<Option<CompanionAgent>>,
    folders: Mutex<Vec<Folder>>,
    places: Places,
    started: Mutex<Vec<AgentRunRequest>>,
    run: Mutex<RunState>,
    fail_start: Mutex<Option<String>>,
    key: Mutex<std::result::Result<(), String>>,
    polls: Mutex<u32>,
    /// The pane the run's record names when it ends.
    pane_at_end: Mutex<Option<String>>,
    /// Whether `start` already knows the pane.
    pane_at_start: Mutex<bool>,
}

impl Hands for FakeHands {
    fn agent_of(&self, _bot: &str) -> Option<CompanionAgent> {
        self.agent.lock().clone()
    }
    fn recent_folders(&self) -> Vec<Folder> {
        self.folders.lock().clone()
    }
    fn places(&self) -> Places {
        self.places.clone()
    }
    fn start(&self, request: AgentRunRequest) -> std::result::Result<Started, String> {
        if let Some(message) = self.fail_start.lock().clone() {
            return Err(message);
        }
        let mut started = self.started.lock();
        started.push(request);
        let pane_id = (*self.pane_at_start.lock()).then(|| format!("pane-{}", started.len()));
        Ok(Started { operation_id: format!("op-{}", started.len()), pane_id })
    }
    fn poll(&self, _project: &str, _operation_id: &str) -> RunState {
        *self.polls.lock() += 1;
        self.run.lock().clone()
    }
    fn check_key(&self, _key: &str) -> std::result::Result<(), String> {
        self.key.lock().clone()
    }
}

struct World {
    rig: Rig,
    hands: Arc<FakeHands>,
    base: PathBuf,
}

fn folder(name: &str, path: &Path) -> Folder {
    Folder { name: name.to_string(), path: path.display().to_string() }
}

fn make_dir(path: &Path) -> PathBuf {
    std::fs::create_dir_all(path).unwrap();
    path.to_path_buf()
}

/// A rig whose brain is running, with hands over real folders: two projects (each its own Git folder), a plain
/// folder, the user home, its Desktop, a folder that holds the projects, and the companion's own home.
fn world(name: &str) -> World {
    let rig = Rig::ready(name);
    let base = rig.root.parent().unwrap().to_path_buf();
    let home = make_dir(&base.join("home"));
    let demo_a = make_dir(&base.join("projects").join("demo-a"));
    make_dir(&demo_a.join(".git"));
    let demo_b = make_dir(&base.join("projects").join("demo-b"));
    make_dir(&demo_b.join(".git"));
    let scratch = make_dir(&base.join("scratch"));
    let desktop = make_dir(&home.join("Desktop"));
    let projects = base.join("projects");
    let companion_home = make_dir(&home.join("PuppetMaster").join("companions").join("nimbus"));
    let hands = Arc::new(FakeHands {
        agent: Mutex::new(Some(CompanionAgent {
            agent_type: "fake-agent".into(),
            label: "Fake Agent".into(),
            pane_id: "companion-nimbus".into(),
            home: Some(companion_home.display().to_string()),
        })),
        folders: Mutex::new(vec![
            folder("demo-a", &demo_a),
            folder("demo-b", &demo_b),
            folder("scratch", &scratch),
            folder("home", &home),
            folder("Desktop", &desktop),
            folder("projects", &projects),
            folder("nimbus", &companion_home),
        ]),
        places: Places::for_home(&home),
        started: Mutex::new(Vec::new()),
        run: Mutex::new(RunState::Running),
        fail_start: Mutex::new(None),
        key: Mutex::new(Ok(())),
        polls: Mutex::new(0),
        pane_at_end: Mutex::new(None),
        pane_at_start: Mutex::new(true),
    });
    rig.svc.set_hands(hands.clone());
    World { rig, hands, base }
}

impl World {
    /// The owner asked for work; the brain answers with a helper request. Returns the request id.
    fn ask(&self, id: &str, task: &str, project: Option<&str>) {
        self.rig.svc.send("nimbus", &format!("work: {task}")).unwrap();
        let mut event = json!({"v":1,"type":"helper_request","requestId":id,"conversationId":"app:dm:nimbus","task":task});
        if let Some(project) = project {
            event["project"] = json!(project);
        }
        self.rig.post(vec![event]);
    }

    fn card(&self, id: &str) -> Option<super::store::Message> {
        self.rig.thread().into_iter().find(|m| m.kind == MsgKind::HelperCard && m.request_id.as_deref() == Some(id))
    }

    fn updates(&self) -> Vec<Value> {
        self.rig.inbox(0).0.into_iter().filter(|i| i["type"] == "helper_update").collect()
    }

    fn started(&self) -> usize {
        self.hands.started.lock().len()
    }

    /// The helper ends: the next watch sees it.
    fn end_with(&self, status: HelperStatus, summary: &str) {
        *self.hands.run.lock() = RunState::Done { status, summary: summary.to_string(), pane_id: self.hands.pane_at_end.lock().clone() };
        self.rig.advance(3_000);
        self.rig.svc.tick();
    }
}

fn code(result: super::store::StoreResult<HelperStartOutcome>) -> &'static str {
    result.expect_err("expected a refusal").code
}

// ---- nothing starts without Start ----------------------------------------------------------------------

#[test]
fn a_request_makes_a_card_and_starts_nothing_until_start() {
    let w = world("h-nostart");
    w.ask("h-1", "add a readme", Some("demo-a"));
    let card = w.card("h-1").expect("a card");
    assert_eq!(card.project.as_deref(), Some("demo-a"));
    for _ in 0..5 {
        w.rig.advance(2_500);
        w.rig.svc.tick();
    }
    assert_eq!(w.started(), 0, "no run without Start");
    assert!(w.rig.svc.running_helpers().is_empty());
    // the brain heard nothing about it either
    assert!(w.updates().is_empty());
    // Start is what runs it
    w.rig.svc.helper_start("nimbus", "h-1", None).unwrap();
    assert_eq!(w.started(), 1);
}

#[test]
fn not_now_closes_the_card_tells_the_brain_once_and_starts_nothing() {
    let w = world("h-notnow");
    w.ask("h-1", "add a readme", Some("demo-a"));
    w.rig.svc.helper_decline("nimbus", "h-1").unwrap();
    assert_eq!(w.started(), 0);
    assert_eq!(w.rig.svc.state().unwrap().bots[0].open_cards, 0);
    assert!(w.rig.thread().iter().any(|m| m.kind == MsgKind::HelperResult && m.text == "Not now."));
    let updates = w.updates();
    assert_eq!(updates.len(), 1);
    assert_eq!((updates[0]["requestId"].as_str(), updates[0]["status"].as_str()), (Some("h-1"), Some("declined")));
    // a closed card cannot be started, or declined again
    assert_eq!(code(w.rig.svc.helper_start("nimbus", "h-1", None)), "POTATO_HELPER_GONE");
    assert_eq!(w.rig.svc.helper_decline("nimbus", "h-1").unwrap_err().code, "POTATO_HELPER_GONE");
    assert_eq!(w.started(), 0);
}

#[test]
fn start_needs_a_card_so_a_made_up_request_id_starts_nothing() {
    let w = world("h-nocard");
    assert_eq!(code(w.rig.svc.helper_start("nimbus", "never-asked", Some("demo-a"))), "POTATO_HELPER_GONE");
    assert_eq!(w.started(), 0);
}

#[test]
fn a_request_from_others_is_refused_with_no_card_and_nothing_to_start() {
    let rig = Rig::new("h-others");
    let hands = world_hands_for(&rig);
    rig.svc.start_bot("nimbus").unwrap();
    rig.hello();
    // No owner message is behind this turn, so the trust is `others`.
    rig.post(vec![json!({"v":1,"type":"helper_request","requestId":"h-x","conversationId":"app:dm:nimbus","task":"delete everything","project":"demo-a"})]);
    assert!(rig.thread().iter().all(|m| m.kind != MsgKind::HelperCard), "no card");
    let (items, _) = rig.inbox(0);
    let update = items.iter().find(|i| i["type"] == "helper_update").expect("told the brain");
    assert_eq!(update["status"], "refused");
    assert_eq!(code(rig.svc.helper_start("nimbus", "h-x", Some("demo-a"))), "POTATO_HELPER_GONE");
    assert!(hands.started.lock().is_empty());
}

/// Hands for a rig without a world around it (the folders are not needed: nothing is asked to start).
fn world_hands_for(rig: &Rig) -> Arc<FakeHands> {
    let hands = Arc::new(FakeHands {
        agent: Mutex::new(None),
        folders: Mutex::new(Vec::new()),
        places: Places::default(),
        started: Mutex::new(Vec::new()),
        run: Mutex::new(RunState::Running),
        fail_start: Mutex::new(None),
        key: Mutex::new(Ok(())),
        polls: Mutex::new(0),
        pane_at_end: Mutex::new(None),
        pane_at_start: Mutex::new(true),
    });
    rig.svc.set_hands(hands.clone());
    hands
}

// ---- the folder ------------------------------------------------------------------------------------------

#[test]
fn the_folder_comes_from_a_name_in_the_recent_list_and_never_from_a_path_the_bot_wrote() {
    let w = world("h-name");
    // The bot wrote a path to somewhere else; only its last part counts, and only as a name.
    w.ask("h-1", "add a readme", Some("D:\\somewhere\\else\\demo-a"));
    assert_eq!(w.card("h-1").unwrap().project.as_deref(), Some("demo-a"));
    w.rig.svc.helper_start("nimbus", "h-1", None).unwrap();
    let request = w.hands.started.lock()[0].clone();
    let expected = w.base.join("projects").join("demo-a").display().to_string();
    assert_eq!(request.project_path.as_deref(), Some(expected.as_str()), "the person's own folder, not the written path");
    assert!(!request.project_path.as_deref().unwrap().contains("somewhere"));
}

#[test]
fn a_name_that_is_not_a_recent_project_leaves_the_card_without_a_folder_and_start_refuses_it() {
    let w = world("h-unknown");
    w.ask("h-1", "add a readme", Some("not-a-project"));
    assert_eq!(w.card("h-1").unwrap().project, None, "the person must choose");
    assert_eq!(code(w.rig.svc.helper_start("nimbus", "h-1", None)), "POTATO_FOLDER_NEEDED");
    // a path from the page is not a name either
    let path = w.base.join("projects").join("demo-a").display().to_string();
    assert_eq!(code(w.rig.svc.helper_start("nimbus", "h-1", Some(&path))), "POTATO_FOLDER_UNKNOWN");
    assert_eq!(code(w.rig.svc.helper_start("nimbus", "h-1", Some("..\\demo-a"))), "POTATO_FOLDER_UNKNOWN");
    assert_eq!(w.started(), 0);
    // the person's own choice by name works
    w.rig.svc.helper_start("nimbus", "h-1", Some("DEMO-B")).unwrap();
    let request = w.hands.started.lock()[0].clone();
    assert!(request.project_path.as_deref().unwrap().ends_with("demo-b"));
}

#[test]
fn with_no_name_the_card_proposes_the_most_recent_project_a_helper_may_use() {
    let w = world("h-default");
    // the recent list starts with demo-a
    w.ask("h-1", "add a readme", None);
    assert_eq!(w.card("h-1").unwrap().project.as_deref(), Some("demo-a"));
    // if the list starts with a folder that is refused, the next fine one is proposed
    {
        let mut folders = w.hands.folders.lock();
        let home = folders.iter().find(|f| f.name == "home").unwrap().clone();
        folders.insert(0, home);
    }
    w.ask("h-2", "add a readme", None);
    assert_eq!(w.card("h-2").unwrap().project.as_deref(), Some("demo-a"));
}

#[test]
fn the_companions_home_the_user_home_and_broad_folders_are_refused() {
    let w = world("h-broad");
    w.ask("h-1", "add a readme", Some("demo-a"));
    for (name, why) in [
        ("nimbus", "the bot's own home"),
        ("home", "the user home"),
        ("Desktop", "a standard folder"),
        ("projects", "a folder that only holds projects"),
    ] {
        assert_eq!(code(w.rig.svc.helper_start("nimbus", "h-1", Some(name))), "POTATO_FOLDER_REFUSED", "{why}");
    }
    assert_eq!(w.started(), 0);
    // "Change folder" does not even offer them
    let names = w.rig.svc.helper_folders("nimbus").unwrap();
    assert_eq!(names, vec!["demo-a".to_string(), "demo-b".to_string(), "scratch".to_string()]);
    // a project folder is fine, and so is a plain folder with no Git
    w.rig.svc.helper_start("nimbus", "h-1", Some("scratch")).unwrap();
    assert_eq!(w.started(), 1);
}

#[test]
fn a_folder_that_is_gone_is_refused_with_a_plain_sentence() {
    let w = world("h-gone");
    w.hands.folders.lock().push(Folder { name: "deleted".into(), path: w.base.join("deleted").display().to_string() });
    w.ask("h-1", "add a readme", Some("demo-a"));
    let e = w.rig.svc.helper_start("nimbus", "h-1", Some("deleted")).unwrap_err();
    assert_eq!(e.code, "POTATO_FOLDER_REFUSED");
    assert!(e.message.contains("not there any more"), "{}", e.message);
}

// ---- the caps ----------------------------------------------------------------------------------------------

#[test]
fn ten_starts_per_day_per_bot_and_a_new_day_starts_counting_again() {
    let w = world("h-day");
    for n in 0..10 {
        w.ask(&format!("h-{n}"), &format!("job {n}"), Some("demo-a"));
        w.rig.svc.helper_start("nimbus", &format!("h-{n}"), None).unwrap();
        // each card is closed by its start, so the open-card cap never gets in the way
        assert_eq!(w.rig.svc.state().unwrap().bots[0].open_cards, 0);
    }
    assert_eq!(w.started(), 10);
    w.ask("h-10", "one more", Some("demo-a"));
    assert_eq!(code(w.rig.svc.helper_start("nimbus", "h-10", None)), "POTATO_HELPER_LIMIT");
    assert_eq!(w.started(), 10, "the eleventh did not run");
    // tomorrow
    w.rig.advance(24 * 60 * 60 * 1000);
    w.rig.svc.helper_start("nimbus", "h-10", None).unwrap();
    assert_eq!(w.started(), 11);
}

#[test]
fn the_count_survives_a_restart_of_the_app() {
    let w = world("h-count");
    for n in 0..10 {
        w.ask(&format!("h-{n}"), "job", Some("demo-a"));
        w.rig.svc.helper_start("nimbus", &format!("h-{n}"), None).unwrap();
    }
    // a new engine over the same folder: it reads the book again
    let fresh = super::service::Service::new(w.rig.root.clone(), w.rig.host.clone(), w.rig.launcher.clone(), w.rig.brains.clone());
    fresh.set_hands(w.hands.clone());
    w.ask("h-late", "job", Some("demo-a"));
    assert_eq!(code(fresh.helper_start("nimbus", "h-late", None)), "POTATO_HELPER_LIMIT");
    assert_eq!(fresh.running_helpers().len(), 10, "and it still knows which helpers are working");
}

#[test]
fn four_cards_at_once_are_not_made_and_the_brain_is_told() {
    let w = world("h-cards");
    w.rig.svc.send("nimbus", "work").unwrap();
    for n in 0..4 {
        w.rig.post(vec![json!({"v":1,"type":"helper_request","requestId":format!("c-{n}"),"conversationId":"app:dm:nimbus","task":format!("job {n}"),"project":"demo-a"})]);
    }
    assert_eq!(w.rig.svc.state().unwrap().bots[0].open_cards, 3);
    assert!(w.card("c-3").is_none());
    assert!(w.updates().iter().any(|u| u["requestId"] == "c-3" && u["status"] == "declined"));
}

#[test]
fn two_clicks_on_start_make_one_run() {
    let w = world("h-twice");
    w.ask("h-1", "add a readme", Some("demo-a"));
    w.rig.svc.helper_start("nimbus", "h-1", None).unwrap();
    assert_eq!(code(w.rig.svc.helper_start("nimbus", "h-1", None)), "POTATO_HELPER_GONE");
    assert_eq!(w.started(), 1);
}

#[test]
fn a_start_that_fails_keeps_the_card_open_and_costs_nothing() {
    let w = world("h-fail");
    w.ask("h-1", "add a readme", Some("demo-a"));
    *w.hands.fail_start.lock() = Some("The agent is not installed.".into());
    let e = w.rig.svc.helper_start("nimbus", "h-1", None).unwrap_err();
    assert_eq!((e.code, e.message.as_str()), ("POTATO_HELPER_FAILED", "The agent is not installed."));
    assert_eq!(w.rig.svc.state().unwrap().bots[0].open_cards, 1, "still open: the person can try again");
    *w.hands.fail_start.lock() = None;
    for n in 0..9 {
        w.ask(&format!("x-{n}"), "job", Some("demo-a"));
        w.rig.svc.helper_start("nimbus", &format!("x-{n}"), None).unwrap();
    }
    w.rig.svc.helper_start("nimbus", "h-1", None).unwrap();
    assert_eq!(w.started(), 10, "the failed try did not count");
}

#[test]
fn the_kill_switch_stops_a_start_and_the_watching() {
    let w = world("h-off");
    w.ask("h-1", "add a readme", Some("demo-a"));
    w.ask("h-2", "and a license", Some("demo-a"));
    w.rig.svc.helper_start("nimbus", "h-2", None).unwrap();
    w.rig.svc.set_enabled(false);
    assert_eq!(code(w.rig.svc.helper_start("nimbus", "h-1", None)), "POTATO_OFF");
    assert_eq!(w.started(), 1, "only the one started before the switch");
    // while it is off nothing is processed; the helper is still remembered
    w.end_with(HelperStatus::Finished, "done");
    assert_eq!(w.rig.svc.running_helpers().len(), 1);
    // back on: the end is seen once
    w.rig.brains.set_enabled(true);
    w.rig.advance(3_000);
    w.rig.svc.tick();
    assert!(w.rig.svc.running_helpers().is_empty());
    assert_eq!(w.rig.thread().iter().filter(|m| m.text == "Helper finished in demo-a.").count(), 1);
}

// ---- the companion's house rules ------------------------------------------------------------------------

#[test]
fn the_run_is_the_companions_agent_in_that_folder_for_its_task_and_carries_the_companions_name() {
    let w = world("h-rules");
    w.ask("h-1", "add a readme", Some("demo-a"));
    w.rig.svc.helper_start("nimbus", "h-1", None).unwrap();
    let request = w.hands.started.lock()[0].clone();
    assert_eq!(request.agent_type, "fake-agent", "the companion's own agent");
    assert_eq!(request.task, "add a readme", "the card's task, nothing added");
    assert!(!request.headless && request.background && request.keep_pane);
    assert_eq!(request.idempotency_key.as_deref(), Some("potato-nimbus-h-1"), "a retry cannot start a second run");
    assert_eq!(request.on_behalf_of_companion.as_deref(), Some("companion-nimbus"));
}

#[test]
fn on_behalf_of_companion_gives_the_twenty_turn_budget_and_the_feed_line() {
    use crate::budgets::{with_companion_default, Defaults};
    use crate::companion::test_host::{draft, start_ms, FakeHost};
    use crate::companion::{model::FeedKind, store as companion_store, Service as CompanionService};

    let w = world("h-house");
    w.ask("h-1", "add a readme", Some("demo-a"));
    w.rig.svc.helper_start("nimbus", "h-1", None).unwrap();
    let request = w.hands.started.lock()[0].clone();

    // 1. The run is a companion's helper: the one line in `run_agent` takes the name from the request, with no
    //    session behind it, and the default budget is then the companion helper's 20 turns.
    let pane = request.companion_pane_with(|| None).expect("a companion's helper");
    let budget = with_companion_default(Defaults::default(), true);
    assert_eq!(budget.turns, Some(crate::companion::delegate::HELPER_TURNS));
    assert_eq!(budget.turns, Some(20));
    // A request without the name, from nowhere, is nobody's helper: no budget by default.
    let mut plain = request.clone();
    plain.on_behalf_of_companion = None;
    assert_eq!(plain.companion_pane_with(|| None), None);
    assert_eq!(with_companion_default(Defaults::default(), false).turns, None);

    // 2. The name is the pane id of a real companion, so its service writes the `delegated` feed line and
    //    refuses its own home folder for a helper.
    let root = std::env::temp_dir().join(format!("pm-potato-helpers-companion-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let host = FakeHost::new(start_ms());
    let service = CompanionService::new(host, root.clone());
    service.create(draft("Nimbus", &root.join("nimbus"))).unwrap();
    service.note_delegated(&pane, "Fake Agent", "demo-a");
    let feed = companion_store::feed_read(&root, "nimbus").unwrap();
    let line = feed.iter().find(|item| item.kind == FeedKind::Delegated).expect("a delegated line in the feed");
    assert_eq!(line.text, "Nimbus started a Fake Agent helper in demo-a.");
    assert!(service.is_home_path(&pane, &root.join("nimbus").display().to_string()));
}

#[test]
fn nothing_but_a_card_and_start_calls_the_hands_to_start() {
    // The only caller of `Hands::start` in the engine is `start_checked`, reached from `helper_start`.
    let source = include_str!("helpers.rs");
    let engine = source.split("#[cfg(test)]").next().unwrap();
    assert_eq!(engine.matches("hands.start(").count(), 1);
    for other in ["service.rs", "live.rs", "rules.rs", "sidecar.rs"] {
        let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("potato").join(other)).unwrap();
        let code = text.split("#[cfg(test)]").next().unwrap();
        assert!(!code.contains("hands.start("), "{other} starts a helper by itself");
        assert!(!code.contains("run_agent(") || other == "live.rs", "{other} runs an agent by itself");
    }
}

// ---- the end of a helper -----------------------------------------------------------------------------------

#[test]
fn start_closes_the_card_with_a_row_tells_the_brain_started_and_remembers_the_helper() {
    let w = world("h-started");
    w.ask("h-1", "add a readme", Some("demo-a"));
    let out = w.rig.svc.helper_start("nimbus", "h-1", None).unwrap();
    assert_eq!(out.folder, "demo-a");
    let rows: Vec<_> = w.rig.thread().into_iter().filter(|m| m.kind == MsgKind::HelperResult).collect();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].text.as_str(), rows[0].request_id.as_deref(), rows[0].pane_id.as_deref()), ("Started a helper in demo-a.", Some("h-1"), Some("pane-1")));
    let updates = w.updates();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0]["status"], "started");
    assert_eq!(w.rig.svc.running_helpers().len(), 1);
    assert_eq!(w.rig.svc.state().unwrap().bots[0].open_cards, 0);
}

#[test]
fn a_helper_end_gives_a_row_and_exactly_one_brain_turn() {
    let w = world("h-end");
    w.ask("h-1", "add a readme", Some("demo-a"));
    w.rig.svc.helper_start("nimbus", "h-1", None).unwrap();
    // still running: the watch looks, and says nothing
    w.rig.advance(3_000);
    w.rig.svc.tick();
    assert_eq!(w.updates().iter().filter(|u| u["status"] != "started").count(), 0);
    w.end_with(HelperStatus::Finished, "I added README.md");
    // many more ticks: nothing more is said
    for _ in 0..6 {
        w.rig.advance(3_000);
        w.rig.svc.tick();
    }
    let ended: Vec<_> = w.updates().into_iter().filter(|u| u["status"] != "started").collect();
    assert_eq!(ended.len(), 1, "exactly one helper_update, which is exactly one brain turn");
    assert_eq!((ended[0]["requestId"].as_str(), ended[0]["status"].as_str()), (Some("h-1"), Some("finished")));
    assert_eq!(ended[0]["summary"], "I added README.md");
    let row = w.rig.thread().into_iter().rev().find(|m| m.kind == MsgKind::HelperResult && m.text.starts_with("Helper")).expect("the end row");
    assert_eq!((row.text.as_str(), row.request_id.as_deref(), row.pane_id.as_deref(), row.from.kind), ("Helper finished in demo-a.", Some("h-1"), Some("pane-1"), FromKind::Note));
    assert!(w.rig.svc.running_helpers().is_empty());
}

#[test]
fn the_open_button_uses_the_pane_the_record_names_when_the_start_did_not_know_one_yet() {
    let w = world("h-pane");
    *w.hands.pane_at_start.lock() = false;
    *w.hands.pane_at_end.lock() = Some("pane-77".into());
    w.ask("h-1", "add a readme", Some("demo-a"));
    w.rig.svc.helper_start("nimbus", "h-1", None).unwrap();
    assert_eq!(w.rig.svc.running_helpers()[0].pane_id, None);
    w.end_with(HelperStatus::Finished, "done");
    let row = w.rig.thread().into_iter().find(|m| m.text == "Helper finished in demo-a.").unwrap();
    assert_eq!(row.pane_id.as_deref(), Some("pane-77"));
}

#[test]
fn a_failed_or_stopped_helper_says_so_in_its_row_and_to_the_brain() {
    for (status, word, line) in [(HelperStatus::Failed, "failed", "Helper could not finish in demo-a."), (HelperStatus::Stopped, "stopped", "Helper stopped in demo-a.")] {
        let w = world(&format!("h-end-{word}"));
        w.ask("h-1", "add a readme", Some("demo-a"));
        w.rig.svc.helper_start("nimbus", "h-1", None).unwrap();
        w.end_with(status, "");
        assert!(w.rig.thread().iter().any(|m| m.text == line), "{line}");
        assert!(w.updates().iter().any(|u| u["status"] == word));
    }
}

#[test]
fn the_summary_is_redacted_and_cut_before_it_reaches_the_brain() {
    let w = world("h-summary");
    w.ask("h-1", "add a readme", Some("demo-a"));
    w.rig.svc.helper_start("nimbus", "h-1", None).unwrap();
    let secret = "sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghij";
    w.end_with(HelperStatus::Finished, &format!("done, key {secret} {}", "x".repeat(4000)));
    let update = w.updates().into_iter().find(|u| u["status"] == "finished").unwrap();
    let summary = update["summary"].as_str().unwrap();
    assert!(!summary.contains(secret) && !summary.contains("sk-ant"), "{summary}");
    assert!(summary.chars().count() <= 1500);
}

#[test]
fn the_row_is_added_but_the_brain_is_not_asked_while_it_is_over_its_limit() {
    let w = world("h-limit");
    w.ask("h-1", "add a readme", Some("demo-a"));
    w.rig.svc.helper_start("nimbus", "h-1", None).unwrap();
    // spend more than the day's limit ($0.50): 1M completion tokens of a priced model
    w.rig.post(vec![json!({"v":1,"type":"usage","model":"deepseek-flash","promptTokens":0,"completionTokens":1_000_000,"cacheHitTokens":0,"cacheMissTokens":0})]);
    assert!(w.rig.notes().iter().any(|n| n.contains("used today's limit")));
    w.end_with(HelperStatus::Finished, "done");
    assert!(w.rig.thread().iter().any(|m| m.text == "Helper finished in demo-a."), "the person still sees the row");
    assert!(w.updates().iter().all(|u| u["status"] == "started"), "no turn is asked of a brain that is paused");
}

#[test]
fn a_helper_that_cannot_be_found_any_more_is_let_go_once() {
    let w = world("h-lost");
    w.ask("h-1", "add a readme", Some("demo-a"));
    w.rig.svc.helper_start("nimbus", "h-1", None).unwrap();
    *w.hands.run.lock() = RunState::Gone;
    for _ in 0..8 {
        w.rig.advance(3_000);
        w.rig.svc.tick();
    }
    assert!(w.rig.svc.running_helpers().is_empty());
    assert_eq!(w.rig.thread().iter().filter(|m| m.text == "Helper stopped in demo-a.").count(), 1);
}

#[test]
fn a_helper_still_working_keeps_the_brain_from_going_idle() {
    let w = world("h-idle");
    w.ask("h-1", "add a readme", Some("demo-a"));
    w.rig.svc.helper_start("nimbus", "h-1", None).unwrap();
    // 45 minutes with the helper working and nobody writing: the brain (idle limit 30 minutes) was never asked to leave
    for _ in 0..900 {
        w.rig.advance(3_000);
        w.rig.svc.tick();
    }
    assert!(w.rig.child(0).running(), "a brain with a helper out is not idle, so it was never asked to leave");
    // and once the helper is over, the usual idle timer runs again
    w.end_with(HelperStatus::Finished, "done");
    for _ in 0..700 {
        w.rig.advance(3_000);
        w.rig.svc.tick();
    }
    assert!(!w.rig.child(0).running(), "idle again: it was asked to leave after 30 quiet minutes, and the fake never leaves by itself, so it was killed");
}

// ---- Check key and the key never leaving ----------------------------------------------------------------

#[test]
fn check_key_says_works_or_one_plain_sentence() {
    let w = world("h-check");
    assert_eq!(w.rig.svc.check_key().unwrap(), "Works.");
    *w.hands.key.lock() = Err("DeepSeek did not accept that key.".into());
    let e = w.rig.svc.check_key().unwrap_err();
    assert_eq!(e.message, "DeepSeek did not accept that key.");
    // no key saved: it says so and asks nothing of the model maker
    w.rig.host.secrets.lock().clear();
    assert_eq!(w.rig.svc.check_key().unwrap_err().code, "POTATO_NO_KEY");
}

#[test]
fn the_state_the_page_reads_never_holds_the_key_only_the_last_four() {
    let w = world("h-mask");
    let state = serde_json::to_string(&w.rig.svc.state().unwrap()).unwrap();
    assert!(!state.contains(super::service_tests::KEY), "{state}");
    assert!(state.contains("\"last4\":\"7890\""), "{state}");
    // and the settings the page can read and write carry no key field at all
    let settings = serde_json::to_value(w.rig.svc.settings().unwrap()).unwrap();
    let mut names: Vec<&str> = settings.as_object().unwrap().keys().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(names, vec!["brain", "dailyLimitCents", "defaultPersona", "v"], "no field of the settings can hold a key");
}

#[test]
fn the_settings_are_saved_in_the_potato_folder_and_clamped() {
    let w = world("h-settings");
    let mut s = w.rig.svc.settings().unwrap();
    assert_eq!(s.daily_limit_cents, 50);
    s.daily_limit_cents = 1;
    let saved = w.rig.svc.set_settings(s.clone()).unwrap();
    assert_eq!(saved.daily_limit_cents, 5, "$0.05 is the lowest");
    s.daily_limit_cents = 999_999;
    assert_eq!(w.rig.svc.set_settings(s).unwrap().daily_limit_cents, 2_000, "$20 is the highest");
    assert!(w.rig.root.join("settings.json").is_file());
}
