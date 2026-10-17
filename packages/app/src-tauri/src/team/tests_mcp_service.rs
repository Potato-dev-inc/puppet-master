//! The Team tools against a live `Service` (not the process-wide one): a counting transport proves
//! what leaves this computer, and when. Child of `service`, so it can build its own `Service`.

use super::*;
use crate::team::event::{EventSource, PrRef};
use crate::team::roster::Roster;
use crate::team::secure::SeqTracker;
use crate::team::tests_mcp::repo_with_remote_branch;
use crate::team::tests_v1::{config_for, event_from, folder, temp_dir, world, World, NOW};
use pm_relay_protocol::{Envelope, JoinBox, SignedBlob};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;

/// Counts every envelope handed to the wire.
#[derive(Default)]
struct CountingTransport {
    sent: Mutex<Vec<Envelope>>,
    lost: AtomicBool,
}

impl CountingTransport {
    fn count(&self) -> usize {
        self.sent.lock().len()
    }
}

impl TeamTransport for CountingTransport {
    fn kind(&self) -> &'static str {
        "folder"
    }
    fn send(&self, envelope: &Envelope) -> Result<(), super::super::transport::TransportError> {
        self.sent.lock().push(envelope.clone());
        Ok(())
    }
    fn subscribe(&self, _after: u64) -> Result<Receiver<Inbound>, super::super::transport::TransportError> {
        Err(super::super::transport::TransportError::new("not in this test"))
    }
    fn put_state(&self, _blob: &SignedBlob) -> Result<(), super::super::transport::TransportError> {
        Ok(())
    }
    fn get_state(&self, _slot: &str) -> Result<Option<SignedBlob>, super::super::transport::TransportError> {
        Ok(None)
    }
    fn post_join(&self, _join: &JoinBox) -> Result<(), super::super::transport::TransportError> {
        Ok(())
    }
    fn joins(&self) -> Result<Vec<JoinBox>, super::super::transport::TransportError> {
        Ok(Vec::new())
    }
    fn lost_access(&self) -> bool {
        self.lost.load(Ordering::SeqCst)
    }
    fn stop(&self) {}
}

struct NoGh;

impl Gh for NoGh {
    fn api(&self, _path: &str, _etag: Option<&str>) -> Result<github::GhResponse, String> {
        Err("gh isn't available in this test".into())
    }
    fn run(&self, _args: &[&str]) -> Result<String, String> {
        Err("gh isn't available in this test".into())
    }
}

struct Live {
    svc: Arc<Service>,
    wire: Arc<CountingTransport>,
    world: World,
    dir: PathBuf,
    folder: PathBuf,
    repo: PathBuf,
    repo_tip: String,
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
        let _ = std::fs::remove_dir_all(&self.folder);
        let _ = std::fs::remove_dir_all(&self.repo);
    }
}

/// A service for `who` (0 = Maya, the admin; 1 = Leo) on a two-person team, watching one repository
/// that has `feat/x` on origin.
fn live(tag: &str, who: usize) -> Live {
    let wire_dir = temp_dir(&format!("{tag}-wire"));
    let transport = folder(&wire_dir);
    let world = world(&transport, &["Leo"], NOW);
    let (repo, _first, tip) = repo_with_remote_branch(&format!("{tag}-repo"), "feat/x");
    let (keys, roster, name): (_, Roster, _) = if who == 0 {
        (world.maya.clone(), world.roster.clone(), "Maya")
    } else {
        (world.others[0].1.clone(), world.others[0].2.clone(), "Leo")
    };
    let dir = temp_dir(&format!("{tag}-app"));
    let mut config = config_for(&keys, &roster, name);
    config.projects = vec![repo.to_string_lossy().into_owned()];
    config::save(&config::path_in(&dir), &config).unwrap();
    save_roster(&dir, &roster).unwrap();
    let registry: Registry = Arc::new(Mutex::new(PaneRegistry::new()));
    let svc = Arc::new(Service::make(std::mem::ManuallyDrop::new(None), registry, dir.clone(), keys, Arc::new(NoGh)));
    let counting = Arc::new(CountingTransport::default());
    {
        let mut inner = svc.inner.lock();
        inner.transport = Some(counting.clone());
        // No roster refresh should be due while a test runs.
        inner.needs_refresh = false;
        inner.last_refresh_ms = now_ms();
    }
    Live { svc, wire: counting, world, dir, folder: wire_dir, repo, repo_tip: tip }
}

fn code(result: Result<Value, McpError>) -> &'static str {
    match result {
        Err(error) => error.code,
        Ok(value) => panic!("expected an error, got {value}"),
    }
}

fn leo_opens(live: &Live, env: &Envelope) -> TeamEvent {
    let (_, keys, roster) = &live.world.others[0];
    let session = Session::new(roster.clone(), keys.clone());
    match session.open(env, &mut SeqTracker::default(), now_ms()).expect("Leo's app opens it") {
        Opened::Event(verified) => verified.into_event(),
        other => panic!("{other:?}"),
    }
}

// --------------------------------------------------------------------- nothing leaves by itself

#[test]
fn team_notify_makes_a_draft_and_no_envelope_leaves_until_the_person_sends() {
    let live = live("notify", 0);
    let args = json!({
        "project": live.repo.to_string_lossy(),
        "kind": "pushed",
        "refs": { "branch": "feat/x" },
        "note": "Token refresh is in, careful with login()",
        "agent": "Claude Code",
    });
    let before = crate::team::git::TRACE.lock().map(|t| t.len()).unwrap_or(0);
    let reply = live.svc.mcp_notify(&args, "session-1").expect("a draft");

    // The answer: a draft, with the exact text, and nothing sent.
    assert_eq!(reply["drafted"], true);
    assert_eq!(reply["sent"], false);
    assert_eq!(reply["draft"]["note"], "Token refresh is in, careful with login()");
    assert_eq!(reply["draft"]["branch"], "feat/x");
    assert_eq!(reply["draft"]["commit"], live.repo_tip.as_str(), "the verified tip of origin/feat/x");
    let draft_id = reply["draft_id"].as_str().unwrap().to_string();

    // The state: one unsent draft, no card, no job, nothing on the wire.
    {
        let inner = live.svc.inner.lock();
        assert_eq!(inner.store.drafts.len(), 1);
        assert!(inner.store.cards.is_empty() && inner.store.jobs.is_empty());
        let draft = &inner.store.drafts[0];
        assert_eq!(draft.id, draft_id);
        assert_eq!(draft.drafted_by.as_deref(), Some("Claude Code"));
        assert_eq!(draft.event.note.as_deref(), Some("Token refresh is in, careful with login()"));
        assert_eq!(draft.event.kind, TeamEventKind::Pushed);
        assert_eq!(draft.event.actor.display, "Maya");
        assert_eq!(draft.event.actor.agent.as_ref().unwrap().label, "Claude Code");
    }
    assert_eq!(live.wire.count(), 0, "drafting sends no envelope");
    // Drafting the same thing again keeps the one draft; reading and waiting change nothing on the wire either.
    let again = live.svc.mcp_notify(&args, "session-1").unwrap();
    assert_eq!(again["draft_id"], draft_id.as_str());
    assert_eq!(again["created_new_draft"], false);
    live.svc.mcp_roster().unwrap();
    live.svc.mcp_events(&json!({})).unwrap();
    live.svc.mcp_wait(&json!({}), now_ms(), Duration::from_millis(50)).unwrap();
    assert_eq!(live.wire.count(), 0);
    assert_eq!(live.svc.inner.lock().store.drafts.len(), 1, "the same push is one draft");
    // And git was only ever asked questions: nothing here pushes.
    let trace = crate::team::git::TRACE.lock().unwrap();
    assert!(trace.iter().skip(before).all(|args| args.first().map(String::as_str) != Some("push")), "{:?}", &trace[before..]);
    drop(trace);

    // The person clicks Tell: exactly now, and exactly the text the draft showed.
    send_via(&live.svc, &draft_id, Some("Token refresh is in, careful with login()".into())).expect("sent");
    assert_eq!(live.wire.count(), 1);
    assert!(live.svc.inner.lock().store.drafts.is_empty());
    let sent = leo_opens(&live, &live.wire.sent.lock()[0].clone());
    assert_eq!(sent.kind, TeamEventKind::Pushed);
    assert_eq!(sent.note.as_deref(), Some("Token refresh is in, careful with login()"));
    assert_eq!(sent.subject.branch.as_deref(), Some("feat/x"));
    assert_eq!(sent.actor.agent.as_ref().unwrap().label, "Claude Code");
    assert_eq!(sent.id, draft_id);
}

#[test]
fn what_the_person_edits_is_what_goes_and_a_plain_click_sends_the_drafted_note() {
    let live = live("edit", 0);
    let make = |note: &str| {
        let reply = live
            .svc
            .mcp_notify(&json!({ "project": live.repo.to_string_lossy(), "kind": "note", "note": note }), "s")
            .unwrap();
        reply["draft_id"].as_str().unwrap().to_string()
    };
    let (a, b, c) = (make("as drafted"), make("to be edited"), make("to be removed"));
    send_via(&live.svc, &a, None).unwrap();
    send_via(&live.svc, &b, Some("  edited   text ".into())).unwrap();
    let sent: Vec<Option<String>> = live.wire.sent.lock().iter().map(|e| leo_opens(&live, e).note).collect();
    assert_eq!(sent[0].as_deref(), Some("as drafted"), "no edit: the drafted note goes");
    assert_eq!(sent[1].as_deref(), Some("edited text"));
    // A note with no text is not a note: nothing is sent, and the draft stays for the person to fix.
    let refused = send_via(&live.svc, &c, Some("   ".into())).unwrap_err();
    assert!(refused.contains("needs some text"), "{refused}");
    assert_eq!(live.wire.count(), 2);
    assert!(live.svc.inner.lock().store.drafts.iter().any(|d| d.id == c));
}

// --------------------------------------------------------------------------------- the gate

#[test]
fn every_tool_says_team_off_and_a_notify_leaves_nothing_behind() {
    let live = live("off", 0);
    live.svc.inner.lock().config.enabled = false;
    let args = json!({ "project": live.repo.to_string_lossy(), "kind": "note", "note": "hi" });
    assert_eq!(code(live.svc.mcp_roster()), "TEAM_OFF");
    assert_eq!(code(live.svc.mcp_events(&json!({}))), "TEAM_OFF");
    assert_eq!(code(live.svc.mcp_wait(&json!({}), 0, Duration::from_millis(10))), "TEAM_OFF");
    assert_eq!(code(live.svc.mcp_notify(&args, "s")), "TEAM_OFF");
    assert!(live.svc.inner.lock().store.drafts.is_empty());
    assert_eq!(live.wire.count(), 0);
}

#[test]
fn every_tool_says_no_team_when_this_computer_is_on_none() {
    let live = live("noteam", 0);
    {
        let mut inner = live.svc.inner.lock();
        inner.config = TeamConfig { enabled: true, ..TeamConfig::default() };
        inner.session = None;
    }
    let args = json!({ "project": live.repo.to_string_lossy(), "kind": "note", "note": "hi" });
    assert_eq!(code(live.svc.mcp_roster()), "NO_TEAM");
    assert_eq!(code(live.svc.mcp_events(&json!({}))), "NO_TEAM");
    assert_eq!(code(live.svc.mcp_wait(&json!({}), 0, Duration::from_millis(10))), "NO_TEAM");
    assert_eq!(code(live.svc.mcp_notify(&args, "s")), "NO_TEAM");
}

#[test]
fn a_removed_member_gets_not_a_member_from_every_tool_and_can_draft_nothing() {
    let live = live("gone", 1);
    let args = json!({ "project": live.repo.to_string_lossy(), "kind": "note", "note": "hi" });
    live.svc.mcp_roster().expect("a member before");
    // The refresh found this device missing from the signed list.
    live.svc.inner.lock().removed = true;
    assert_eq!(code(live.svc.mcp_roster()), "NOT_A_MEMBER");
    assert_eq!(code(live.svc.mcp_events(&json!({}))), "NOT_A_MEMBER");
    assert_eq!(code(live.svc.mcp_wait(&json!({}), 0, Duration::from_millis(10))), "NOT_A_MEMBER");
    assert_eq!(code(live.svc.mcp_notify(&args, "s")), "NOT_A_MEMBER");
    assert!(live.svc.inner.lock().store.drafts.is_empty());
    // The refresh that clears it (for example after a new invite) restores access.
    live.svc.inner.lock().removed = false;
    live.svc.mcp_roster().expect("a member again");
    // The relay refusing this device counts at once, before any refresh.
    live.wire.lost.store(true, Ordering::SeqCst);
    assert_eq!(code(live.svc.mcp_roster()), "NOT_A_MEMBER");
    assert_eq!(code(live.svc.mcp_notify(&args, "s")), "NOT_A_MEMBER");
}

#[test]
fn a_wait_in_progress_ends_with_not_a_member_when_the_member_is_removed() {
    let live = live("gone-wait", 1);
    let svc = live.svc.clone();
    let flip = std::thread::spawn({
        let svc = svc.clone();
        move || {
            std::thread::sleep(Duration::from_millis(300));
            svc.inner.lock().removed = true;
        }
    });
    let started = std::time::Instant::now();
    let result = live.svc.mcp_wait(&json!({}), now_ms(), Duration::from_secs(20));
    flip.join().unwrap();
    assert_eq!(code(result), "NOT_A_MEMBER");
    assert!(started.elapsed() < Duration::from_secs(10), "it did not sit out the whole wait");
}

// ------------------------------------------------------------------------------ notify refusals

#[test]
fn notify_refuses_unknown_projects_and_unpushed_branches_and_drafts_nothing() {
    let live = live("refuse", 0);
    let known = live.repo.to_string_lossy().into_owned();
    let (other, _, _) = repo_with_remote_branch("refuse-other", "feat/y");
    let unknown_cases = [
        json!({ "project": "no-such-project", "kind": "note", "note": "x" }),
        json!({ "project": other.to_string_lossy(), "kind": "note", "note": "x" }),
        json!({ "project": "D:\\definitely\\not\\here", "kind": "note", "note": "x" }),
    ];
    for args in unknown_cases {
        assert_eq!(code(live.svc.mcp_notify(&args, "s")), "PROJECT_UNKNOWN", "{args}");
    }
    assert_eq!(code(live.svc.mcp_notify(&json!({ "project": known, "kind": "pushed", "refs": { "branch": "feat/never" } }), "s")), "NOT_PUSHED");
    assert_eq!(code(live.svc.mcp_notify(&json!({ "project": known, "kind": "pushed", "refs": { "branch": "feat/x", "commit": "0123456789abcdef" } }), "s")), "NOT_PUSHED");
    assert_eq!(code(live.svc.mcp_notify(&json!({ "project": known, "kind": "note" }), "s")), "INVALID_ARGUMENT");
    assert!(live.svc.inner.lock().store.drafts.is_empty());
    assert_eq!(live.wire.count(), 0);
    // The project is found by folder, key or short name.
    for project in [known.clone(), "github.com/acme/billing".to_string(), "billing".to_string(), "BILLING".to_string()] {
        live.svc.mcp_notify(&json!({ "project": project, "kind": "note", "note": format!("by {project}") }), "s").unwrap_or_else(|e| panic!("{project}: {e}"));
    }
    let _ = std::fs::remove_dir_all(other);
}

#[test]
fn an_agent_draft_is_logged_as_the_agents_and_the_log_says_nothing_was_sent() {
    let live = live("audit", 0);
    live.svc.mcp_notify(&json!({ "project": live.repo.to_string_lossy(), "kind": "note", "note": "audit me" }), "abcdef0123456789").unwrap();
    let lines = crate::audit::global().recent(500);
    let hit = lines
        .iter()
        .find(|e| e.kind == "team.action" && e.summary == "An agent drafted a Tell the team? card (nothing was sent)" && e.actor.as_deref() == Some("abcdef01"))
        .unwrap_or_else(|| panic!("no audit line: {lines:?}"));
    assert_eq!(hit.who, "agent");
    let text = serde_json::to_string(hit).unwrap();
    assert!(!text.contains("abcdef0123456789"), "only the shortened connection id: {text}");
    assert!(!text.contains("audit me"), "the note text is not in the log: {text}");
}

// ---------------------------------------------------------------------------- reading and waiting

fn deliver(live: &Live, kind: TeamEventKind, branch: &str, at: u64) -> String {
    let mut event = event_from(&live.world.maya, &live.world.roster.team, at);
    event.kind = kind;
    event.subject.branch = Some(branch.into());
    event.dedupe_key = crate::team::event::event_dedupe_key(&event);
    let mut inner = live.svc.inner.lock();
    let config = inner.config.clone();
    match ingest(&mut inner.store, &config, Verified::for_test(event), at) {
        Ingested::Card(id) => id,
        other => panic!("{other:?}"),
    }
}

#[test]
fn events_and_roster_through_the_service() {
    let live = live("read", 1);
    deliver(&live, TeamEventKind::Pushed, "feat/auth", now_ms());
    // By folder: Leo's folder is the project (same origin key as the event's).
    let by_folder = live.svc.mcp_events(&json!({ "project": live.repo.to_string_lossy() })).unwrap();
    assert_eq!(by_folder["events"].as_array().unwrap().len(), 1);
    let elsewhere = live.svc.mcp_events(&json!({ "project": "github.com/acme/other" })).unwrap();
    assert!(elsewhere["events"].as_array().unwrap().is_empty());
    let roster = live.svc.mcp_roster().unwrap();
    assert_eq!(roster["members"].as_array().unwrap().len(), 2);
    assert_eq!(roster["members"].as_array().unwrap().iter().filter(|m| m["me"] == true).count(), 1);
    assert_eq!(live.wire.count(), 0, "reading sends nothing");
}

#[test]
fn a_wait_wakes_when_a_matching_event_arrives_and_times_out_quietly_otherwise() {
    let live = live("wait", 1);
    // Quiet: an empty answer that says where to continue.
    let since = now_ms();
    let quiet = live.svc.mcp_wait(&json!({ "kinds": ["pushed"] }), since, Duration::from_millis(250)).unwrap();
    assert_eq!(quiet["matched"], false);
    assert_eq!(quiet["since"], since);
    // An event that does not fit does not wake it; the matching one does, while it is waiting.
    let svc = live.svc.clone();
    let world_team = live.world.roster.team.clone();
    let maya = live.world.maya.clone();
    let feeder = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        let mut other = event_from(&maya, &world_team, now_ms());
        other.subject.branch = Some("feat/other".into());
        other.dedupe_key = crate::team::event::event_dedupe_key(&other);
        let mut inner = svc.inner.lock();
        let config = inner.config.clone();
        let at = now_ms();
        ingest(&mut inner.store, &config, Verified::for_test(other), at);
        drop(inner);
        std::thread::sleep(Duration::from_millis(300));
        let mut wanted = event_from(&maya, &world_team, now_ms());
        wanted.subject.branch = Some("feat/wanted".into());
        wanted.subject.pr = Some(PrRef { number: 5, title: "t".into(), url: String::new(), state: "open".into(), draft: false, files: None, additions: None, deletions: None });
        wanted.source = EventSource::Git;
        wanted.dedupe_key = crate::team::event::event_dedupe_key(&wanted);
        let mut inner = svc.inner.lock();
        let config = inner.config.clone();
        let at = now_ms();
        ingest(&mut inner.store, &config, Verified::for_test(wanted), at);
    });
    let started = std::time::Instant::now();
    let found = live.svc.mcp_wait(&json!({ "kinds": ["pushed"], "branch": "feat/wanted" }), now_ms().saturating_sub(1), Duration::from_secs(20)).unwrap();
    feeder.join().unwrap();
    assert_eq!(found["matched"], true);
    assert_eq!(found["event"]["teammate_data"]["branch"], "feat/wanted");
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(live.wire.count(), 0);
}
