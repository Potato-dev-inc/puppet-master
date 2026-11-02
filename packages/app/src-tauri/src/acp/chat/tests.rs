//! The chat of an ACP session against the fake agent (`acp/fake_agent.rs`): real processes, real pipes, the real
//! question store, the real chat watchers and phone answer. No real agent is started.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::start::start;
use super::{get, on_pane_closed, terminal_input, AcpChat, ChatHost, Readiness, SendError, Sent, StartDeps, StartRequest, LANE, NOT_A_TERMINAL};
use crate::acp::config::{AgentConfig, HostEnv};
use crate::acp::fake_agent;
use crate::acp::state::Phase;
use crate::pane_outbox::{Attempt, Host, Look, Outbox, OutboxMessage, PaneKind, State, Timing};
use crate::pty::status::PaneStatus;
use crate::pty::PaneRegistry;
use crate::questions::{self, now_ms, Question, QuestionState, ACP_KIND_PERMISSION};
use crate::session_chat::{phone, ChatItem, Opening, PaneLookup, Sink, Watchers};

// ---- rig -------------------------------------------------------------------------------------------------------

struct TestEnv {
    root: PathBuf,
}

impl HostEnv for TestEnv {
    fn var(&self, name: &str) -> Option<std::ffi::OsString> {
        std::env::var_os(name)
    }
    fn which(&self, _: &str) -> Option<PathBuf> {
        None
    }
    fn adapters_dir(&self) -> PathBuf {
        self.root.join("adapters")
    }
    fn homes_dir(&self) -> PathBuf {
        self.root.join("homes")
    }
}

#[derive(Default)]
struct Recorder {
    statuses: Mutex<Vec<(String, PaneStatus)>>,
    question_changes: AtomicUsize,
    pane_changes: AtomicUsize,
    states: AtomicUsize,
}

impl ChatHost for Recorder {
    fn status_changed(&self, pane_id: &str, status: PaneStatus) {
        self.statuses.lock().push((pane_id.to_string(), status));
    }
    fn state_changed(&self, _: &str, _: &Value) {
        self.states.fetch_add(1, Ordering::SeqCst);
    }
    fn questions_changed(&self, _: Option<&Question>) {
        self.question_changes.fetch_add(1, Ordering::SeqCst);
    }
    fn panes_changed(&self) {
        self.pane_changes.fetch_add(1, Ordering::SeqCst);
    }
}

struct Rig {
    chat: Arc<AcpChat>,
    registry: Arc<Mutex<PaneRegistry>>,
    host: Arc<Recorder>,
    dir: PathBuf,
}

impl Rig {
    fn new(opts: &str) -> Rig {
        Rig::with(opts, |_| {})
    }

    fn with(opts: &str, tweak: impl FnOnce(&mut AgentConfig)) -> Rig {
        let dir = std::env::temp_dir().join(format!("pm-acp-chat-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut cfg = fake_agent::agent_config(opts, &dir.join("store"));
        tweak(&mut cfg);
        let registry = Arc::new(Mutex::new(PaneRegistry::new()));
        let host = Arc::new(Recorder::default());
        let deps = StartDeps { registry: registry.clone(), host: host.clone(), env: Arc::new(TestEnv { root: dir.clone() }), conversations: None };
        let chat = start(&deps, cfg, StartRequest { agent_id: "fake".into(), folder: dir.clone(), explicit: true }).expect("the fake agent starts");
        Rig { chat, registry, host, dir }
    }

    fn id(&self) -> String {
        self.chat.pane_id().to_string()
    }

    fn items(&self) -> Vec<Value> {
        self.chat.items().iter().map(|i| serde_json::to_value(i).unwrap()).collect()
    }

    fn kinds(&self) -> Vec<String> {
        self.items().iter().map(|i| i["kind"].as_str().unwrap().to_string()).collect()
    }

    fn phase(&self) -> Phase {
        self.chat.session().snapshot().phase
    }

    fn wait_phase(&self, phase: Phase) {
        assert!(wait_until(30, || self.phase() == phase), "wanted {phase:?}, the session is {:?}; items: {:#?}", self.phase(), self.kinds());
    }

    /// Sends a message and waits until the turn is over.
    fn say(&self, text: &str) {
        self.chat.send(text).unwrap_or_else(|e| panic!("send {text:?}: {e:?}"));
        assert!(wait_until(30, || self.items().iter().any(|i| i["kind"] == "turnEnd" && i["id"] == format!("end:{}", self.chat.session().snapshot().turn))), "the turn did not end: {:#?}", self.kinds());
    }

    fn pane_status(&self) -> String {
        self.registry.lock().get(&self.id()).expect("the pane").info().status
    }

    /// The pending permission cards of this pane.
    fn cards(&self) -> Vec<Question> {
        let id = self.id();
        questions::store().list_pending(now_ms()).into_iter().filter(|q| q.pane_id.as_deref() == Some(id.as_str())).collect()
    }

    fn wait_card(&self) -> Question {
        assert!(wait_until(30, || !self.cards().is_empty()), "no permission card was raised; items {:#?}", self.kinds());
        self.cards().remove(0)
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        on_pane_closed(&self.id());
    }
}

fn wait_until(secs: u64, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    condition()
}

struct Recorded(Mutex<Vec<(String, usize, bool)>>);
impl Sink for Recorded {
    fn send(&self, pane_id: &str, items: Vec<ChatItem>, reset: bool) {
        self.0.lock().push((pane_id.to_string(), items.len(), reset));
    }
}

struct NoPanes;
impl PaneLookup for NoPanes {
    fn facts(&self, _: &str) -> Option<crate::session_chat::PaneFacts> {
        None
    }
}

fn watchers() -> (Watchers, Arc<Recorded>) {
    let sink = Arc::new(Recorded(Mutex::new(Vec::new())));
    let tuning = crate::session_chat::watch::Tuning {
        poll: Duration::from_millis(10),
        screen_poll: Duration::from_millis(10),
        api_poll: Duration::from_millis(10),
        pushed_poll: Duration::from_millis(10),
        relocate: Duration::from_millis(10),
        max_items: 3000,
    };
    (Watchers::with_tuning(sink.clone(), Arc::new(NoPanes), tuning), sink)
}

// ---- starting ----------------------------------------------------------------------------------------------------

#[test]
fn a_started_chat_is_a_pane_with_no_terminal_that_says_it_is_a_chat() {
    let rig = Rig::new("");
    let listed = rig.registry.lock().list();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, rig.id());
    assert_eq!(listed[0].lane.as_deref(), Some(LANE));
    assert_eq!(listed[0].pid, 0);
    assert_eq!(listed[0].status, "idle");
    assert!(rig.host.pane_changes.load(Ordering::SeqCst) >= 1);
    assert!(get(&rig.id()).is_some());
    // The pane's chat is pushed, found by pane id (not by agent).
    assert!(matches!(Opening::for_pane(&rig.id(), "fake"), Some(Opening::Pushed(_))));
    assert!(!matches!(Opening::for_pane("some-other-pane", "shell"), Some(Opening::Pushed(_))));
}

#[test]
fn an_adapter_that_will_not_start_leaves_nothing_behind_and_offers_the_terminal() {
    let dir = std::env::temp_dir().join(format!("pm-acp-chat-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut cfg = fake_agent::agent_config("", &dir.join("store"));
    cfg.command = dir.join("no-such-adapter.exe").to_string_lossy().into_owned();
    let registry = Arc::new(Mutex::new(PaneRegistry::new()));
    let deps = StartDeps { registry: registry.clone(), host: Arc::new(Recorder::default()), env: Arc::new(TestEnv { root: dir.clone() }), conversations: None };
    let error = start(&deps, cfg, StartRequest { agent_id: "fake".into(), folder: dir, explicit: true }).err().expect("it cannot start");
    assert_eq!(error.code, "ACP_START_FAILED");
    assert!(error.message.starts_with("The chat program for Fake agent was not found"), "{}", error.message);
    assert!(!error.message.contains("no-such-adapter"), "no path in the sentence");
    assert!(error.fallback, "the terminal can run this agent instead");
    assert!(registry.lock().list().is_empty(), "no pane was made");
}

#[test]
fn the_labs_switch_off_starts_nothing() {
    // The switch is off in the test process unless a test of the settings turned it on; this one needs it off.
    if crate::acp::chat_enabled() {
        return;
    }
    let dir = std::env::temp_dir();
    let registry = Arc::new(Mutex::new(PaneRegistry::new()));
    let deps = StartDeps { registry: registry.clone(), host: Arc::new(Recorder::default()), env: Arc::new(TestEnv { root: dir.clone() }), conversations: None };
    let error = super::start_gated(&deps, StartRequest { agent_id: "codex".into(), folder: dir, explicit: true }).err().expect("refused");
    assert_eq!(error.code, "ACP_LABS_OFF");
    assert!(registry.lock().list().is_empty());
}

#[test]
fn a_folder_that_is_not_a_folder_is_refused_before_anything_starts() {
    let dir = std::env::temp_dir().join("pm-acp-chat-no-such-folder-xyz");
    let registry = Arc::new(Mutex::new(PaneRegistry::new()));
    let deps = StartDeps { registry, host: Arc::new(Recorder::default()), env: Arc::new(TestEnv { root: std::env::temp_dir() }), conversations: None };
    let error = start(&deps, fake_agent::agent_config("", &std::env::temp_dir().join("s")), StartRequest { agent_id: "fake".into(), folder: dir, explicit: true }).err().unwrap();
    assert_eq!(error.code, "INVALID_ARGUMENT");
    assert!(!error.fallback);
}

// ---- the stream --------------------------------------------------------------------------------------------------

#[test]
fn a_turn_becomes_the_persons_message_one_growing_answer_and_a_turn_end() {
    let rig = Rig::new("");
    rig.say("stream 6");
    assert_eq!(rig.kinds(), ["session", "user", "assistant", "notice", "turnEnd"]);
    let items = rig.items();
    assert_eq!(items[1]["text"], "stream 6");
    assert_eq!(items[2]["text"], "w0 w1 w2 w3 w4 w5 ");
    assert_eq!(items[2]["finalAnswer"], true);
    assert_eq!(items[3]["noticeKind"], "ending:done");
    assert_eq!(items[4]["outcome"], "completed");
    assert_eq!(rig.pane_status(), "idle");
}

#[test]
fn a_rich_turn_shows_tools_with_their_files_a_diff_a_plan_and_usage() {
    let rig = Rig::new("");
    rig.say("rich");
    let items = rig.items();
    let kinds = rig.kinds();
    assert_eq!(kinds, ["session", "user", "assistant", "toolCall", "toolResult", "plan", "toolCall", "toolResult", "usage", "assistant", "notice", "turnEnd"], "{items:#?}");
    let edit = &items[3];
    assert_eq!(edit["name"], "edit");
    assert_eq!(edit["summary"], "Write notes.txt");
    assert_eq!(edit["writes"], true);
    assert!(edit["paths"][0].as_str().unwrap().ends_with("notes.txt"));
    assert_eq!(edit["input"]["new_string"], "hello\n");
    assert_eq!(items[4]["ok"], true);
    let run = &items[6];
    assert_eq!(run["name"], "run");
    assert_eq!(items[7]["output"], "3 passed", "the code fence is formatting");
    assert_eq!(items[5]["items"][1]["status"], "in_progress");
    assert_eq!(items[8]["contextWindow"], 200000);
    assert_eq!(items[9]["text"], "All done.");
    assert!(items.iter().all(|i| i["kind"] != "thinking"), "the thought is not shown");
}

#[test]
fn the_desktop_reader_the_phone_and_since_revisions_read_the_pushed_chat_unchanged() {
    let rig = Rig::new("");
    let (watchers, sink) = watchers();
    let opening = Opening::for_pane(&rig.id(), "fake").unwrap();
    // Open before anything was said: the desktop view is on.
    let first = opening.open(&watchers, &rig.id(), "desktop#1".into());
    assert_eq!(first.len(), 1, "just the session item");
    rig.say("stream 3");
    assert!(wait_until(10, || watchers.since(&rig.id(), None, 300).is_some_and(|d| d.items.iter().any(|i| i.id() == "end:1"))));
    // The batches the desktop gets are the items, with growing messages sent again under their id.
    let sent: usize = sink.0.lock().iter().map(|(_, n, _)| n).sum();
    assert!(sent >= 3);
    assert_eq!(opening.source_now(&watchers, &rig.id()), "acp");

    // The phone's answer: items, a revision, and the session behind them.
    let answer = phone::read(&watchers, &rig.id(), Some(&opening), None);
    assert_eq!(answer["available"], true);
    assert_eq!(answer["source"], "acp");
    assert_eq!(answer["reset"], false);
    assert_eq!(answer["session"]["lane"], "acp");
    assert_eq!(answer["session"]["phase"], "idle");
    let kinds: Vec<&str> = answer["items"].as_array().unwrap().iter().map(|i| i["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["session", "user", "assistant", "notice", "turnEnd"]);
    let rev = answer["rev"].as_u64().unwrap();

    // Asking again with that revision: nothing new.
    let quiet = phone::read(&watchers, &rig.id(), Some(&opening), Some(rev));
    assert_eq!(quiet["items"].as_array().unwrap().len(), 0);
    assert_eq!(quiet["reset"], false);

    // A second turn: only what changed since.
    rig.say("stream 2");
    assert!(wait_until(10, || watchers.since(&rig.id(), Some(rev), 300).is_some_and(|d| d.items.iter().any(|i| i.id() == "end:2"))));
    let delta = phone::read(&watchers, &rig.id(), Some(&opening), Some(rev));
    let ids: Vec<&str> = delta["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["u2", "a2.1", "n:ending:done:2", "end:2"]);
    assert_eq!(delta["reset"], false);
    assert!(delta["rev"].as_u64().unwrap() > rev);
    watchers.close_all();
}

#[test]
fn a_viewer_that_opens_late_gets_the_whole_conversation() {
    let rig = Rig::new("");
    rig.say("stream 2");
    let (watchers, _sink) = watchers();
    let opening = Opening::for_pane(&rig.id(), "fake").unwrap();
    let items = opening.open(&watchers, &rig.id(), "desktop#late".into());
    let ids: Vec<&str> = items.iter().map(|i| i.id()).collect();
    assert_eq!(ids, ["session", "u1", "a1.1", "n:ending:done:1", "end:1"]);
    watchers.close_all();
}

// ---- permissions -------------------------------------------------------------------------------------------------

#[test]
fn a_permission_request_is_a_card_with_the_agents_real_options_and_the_agent_waits_without_a_timeout() {
    let rig = Rig::new("");
    rig.chat.send("perm").unwrap();
    let card = rig.wait_card();
    assert_eq!(card.kind.as_deref(), Some(ACP_KIND_PERMISSION));
    assert_eq!(card.question, "Fake agent asks permission: Write notes.txt");
    let labels: Vec<&str> = card.choices.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["Allow Once", "Allow for Session", "Reject"]);
    assert_eq!(card.urgency, "high");
    assert!(!card.allow_free_text, "only the agent's options");
    assert!(card.consent.is_none());
    // The pane reads "needs you" and the session shows it.
    rig.wait_phase(Phase::NeedsInput);
    assert_eq!(rig.pane_status(), "waiting_input");
    let state = rig.chat.state();
    assert_eq!(state["permissions"][0]["questionId"], card.id);
    assert_eq!(state["permissions"][0]["options"][2]["name"], "Reject");
    // Nothing answered it: it stays open.
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(rig.phase(), Phase::NeedsInput);
    assert_eq!(questions::store().get(&card.id, now_ms()).unwrap().state, QuestionState::Pending);
    let waiting = rig.items().iter().find(|i| i["noticeKind"] == "permission").cloned().expect("the chat says it waits");
    assert!(waiting["text"].as_str().unwrap().contains("Write notes.txt"));
}

#[test]
fn answering_the_card_sends_that_option_to_the_agent_and_the_turn_goes_on() {
    let rig = Rig::new("");
    rig.chat.send("perm").unwrap();
    let card = rig.wait_card();
    // The phone answers (the store records who).
    let answered = questions::store().answer(&card.id, Some("allow_always"), None, "phone", now_ms()).unwrap();
    assert!(answered.accepted);
    rig.wait_phase(Phase::Idle);
    let items = rig.items();
    let reply = items.iter().find(|i| i["kind"] == "assistant").unwrap();
    assert_eq!(reply["text"], "chose:allow_always");
    let result = items.iter().find(|i| i["kind"] == "toolResult").unwrap();
    assert_eq!(result["ok"], true);
    let notice = items.iter().find(|i| i["noticeKind"] == "permission").unwrap();
    assert_eq!(notice["text"], "Answered: Allow for Session");
    assert!(rig.cards().is_empty());
    assert_eq!(rig.pane_status(), "idle");
}

#[test]
fn rejecting_is_an_answer_the_agent_continues_from() {
    let rig = Rig::new("");
    rig.chat.send("perm").unwrap();
    let card = rig.wait_card();
    questions::store().answer(&card.id, Some("reject_once"), None, "desktop", now_ms()).unwrap();
    rig.wait_phase(Phase::Idle);
    let items = rig.items();
    assert_eq!(items.iter().find(|i| i["kind"] == "assistant").unwrap()["text"], "chose:reject_once");
    assert_eq!(items.iter().find(|i| i["kind"] == "toolResult").unwrap()["ok"], false);
}

#[test]
fn dismissing_the_card_says_no_it_never_says_yes() {
    let rig = Rig::new("");
    rig.chat.send("perm").unwrap();
    let card = rig.wait_card();
    questions::store().dismiss(&card.id, now_ms()).unwrap();
    rig.wait_phase(Phase::Idle);
    assert_eq!(rig.items().iter().find(|i| i["kind"] == "assistant").unwrap()["text"], "chose:reject_once");
}

#[test]
fn only_the_person_can_answer_the_card_a_plain_local_request_is_refused() {
    let rig = Rig::new("");
    rig.chat.send("perm").unwrap();
    let card = rig.wait_card();
    let noop = || {};
    let body = json!({ "choice_id": "allow_once" });
    let path = ["questions", card.id.as_str(), "answer"];
    // Loopback with no pairing token (an agent's shell can do this): refused.
    let refused = questions::routes::handle("POST", &path, &body, None, false, &noop).expect("a questions route").unwrap_err();
    assert_eq!(refused.0, 403);
    assert_eq!(refused.1["code"], "ACP_PERMISSION_NEEDS_PERSON");
    let refused_dismiss = questions::routes::handle("POST", &["questions", card.id.as_str(), "dismiss"], &json!({}), None, false, &noop).unwrap().unwrap_err();
    assert_eq!(refused_dismiss.1["code"], "ACP_PERMISSION_NEEDS_PERSON");
    assert_eq!(rig.phase(), Phase::NeedsInput, "still waiting");
    // The paired phone's request goes through.
    let ok = questions::routes::handle("POST", &path, &body, None, true, &noop).unwrap().unwrap();
    assert_eq!(ok.0, 200);
    rig.wait_phase(Phase::Idle);
    assert_eq!(rig.items().iter().find(|i| i["kind"] == "assistant").unwrap()["text"], "chose:allow_once");
}

#[test]
fn a_reloaded_view_and_a_reconnecting_phone_still_see_the_open_question_and_can_answer_it() {
    let rig = Rig::new("");
    rig.chat.send("perm").unwrap();
    let card = rig.wait_card();
    // A first viewer comes and goes (a view reload).
    let (first, _) = watchers();
    let opening = Opening::for_pane(&rig.id(), "fake").unwrap();
    opening.open(&first, &rig.id(), "desktop#1".into());
    first.close_all();
    drop(first);
    // A new viewer (the phone, a new connection) opens from nothing: the chat says it waits, the session says what
    // waits on the person, and the same card is still pending.
    let (second, _) = watchers();
    let answer = phone::read(&second, &rig.id(), Some(&opening), None);
    let notices: Vec<&Value> = answer["items"].as_array().unwrap().iter().filter(|i| i["noticeKind"] == "permission").collect();
    assert_eq!(notices.len(), 1);
    assert_eq!(answer["session"]["phase"], "needs_input");
    assert_eq!(answer["session"]["permissions"][0]["questionId"], card.id);
    assert_eq!(rig.cards().len(), 1);
    assert_eq!(rig.cards()[0].id, card.id);
    // It answers from the new viewer's side.
    questions::store().answer(&card.id, Some("allow_once"), None, "phone", now_ms()).unwrap();
    rig.wait_phase(Phase::Idle);
    // The reader hands the new items on within one poll.
    assert!(wait_until(10, || {
        let after = phone::read(&second, &rig.id(), Some(&opening), None);
        after["items"].as_array().unwrap().iter().any(|i| i["text"] == "chose:allow_once")
    }));
    second.close_all();
}

#[test]
fn a_card_that_ran_out_of_time_is_raised_again_while_the_agent_still_waits() {
    let rig = Rig::new("");
    // A card lives a day; here one second, so the test can see it run out (the store is shared by every test).
    rig.chat.set_card_timeout(1);
    rig.chat.send("perm").unwrap();
    let card = rig.wait_card();
    assert!(wait_until(10, || {
        questions::store().list_pending(now_ms());
        questions::store().get(&card.id, now_ms()).is_some_and(|q| q.state == QuestionState::TimedOut)
    }));
    // The agent still waits (no timeout of its own); the next look at the chat raises a fresh card.
    assert_eq!(rig.phase(), Phase::NeedsInput);
    rig.chat.reconcile_cards();
    let fresh = rig.wait_card();
    assert_ne!(fresh.id, card.id);
    rig.chat.set_card_timeout(3600);
    questions::store().answer(&fresh.id, Some("allow_once"), None, "desktop", now_ms()).unwrap();
    rig.wait_phase(Phase::Idle);
    assert_eq!(rig.items().iter().find(|i| i["kind"] == "assistant").unwrap()["text"], "chose:allow_once");
}

#[test]
fn turning_the_switch_off_closes_every_chat_and_takes_its_cards_down() {
    let a = Rig::new("");
    let b = Rig::new("");
    a.chat.send("perm").unwrap();
    let card = a.wait_card();
    // What `close_all` does for every chat (a test must not close the chats of the tests running beside it).
    super::close_these(vec![a.chat.clone(), b.chat.clone()]);
    assert!(a.chat.is_closed() && b.chat.is_closed());
    assert_eq!(questions::store().get(&card.id, now_ms()).unwrap().state, QuestionState::TimedOut);
    assert_eq!(a.chat.send("hi"), Err(SendError::Gone));
    // The panes stay listed, idle, so the person still sees the conversation.
    assert!(a.registry.lock().get(&a.id()).is_some());
    assert_eq!(a.pane_status(), "idle");
    // Its conversation is still there to read, as the same pushed chat (not another conversation of the agent).
    assert!(matches!(Opening::for_pane(&a.id(), "fake"), Some(Opening::Pushed(_))));
    assert!(a.items().iter().any(|i| i["kind"] == "user"));
    assert_eq!(a.chat.state()["closed"], true);
}

#[test]
fn stop_answers_the_open_question_cancelled_takes_the_card_down_and_ends_the_turn() {
    let rig = Rig::new("");
    rig.chat.send("perm").unwrap();
    let card = rig.wait_card();
    assert!(rig.chat.cancel().unwrap());
    rig.wait_phase(Phase::Idle);
    assert!(rig.cards().is_empty(), "the card is gone");
    assert_eq!(questions::store().get(&card.id, now_ms()).unwrap().state, QuestionState::Cancelled);
    let items = rig.items();
    assert_eq!(items.last().unwrap()["outcome"], "interrupted");
    let notice = items.iter().find(|i| i["noticeKind"] == "permission").unwrap();
    assert_eq!(notice["text"], "Cancelled.");
    assert!(items.iter().filter(|i| i["kind"] == "toolResult").all(|i| i["ok"] == false));
}

#[test]
fn closing_the_chat_takes_down_an_open_card_without_answering_it_yes() {
    let rig = Rig::new("");
    rig.chat.send("perm").unwrap();
    let card = rig.wait_card();
    on_pane_closed(&rig.id());
    assert!(rig.chat.is_closed());
    assert!(rig.cards().is_empty());
    assert_eq!(questions::store().get(&card.id, now_ms()).unwrap().state, QuestionState::TimedOut, "closed as expired: nobody is waiting any more");
    assert!(get(&rig.id()).is_none());
    assert!(crate::session_chat::pushed::lookup(&rig.id()).is_none());
}

// ---- stop and steer ----------------------------------------------------------------------------------------------

#[test]
fn stop_shows_stopping_until_the_agent_ends_the_turn() {
    let rig = Rig::new("");
    rig.chat.send("slow").unwrap();
    assert!(wait_until(10, || rig.items().iter().any(|i| i["kind"] == "assistant")));
    assert_eq!(rig.chat.state()["turnInFlight"], true);
    assert_eq!(rig.chat.state()["stopping"], false);
    assert!(rig.chat.cancel().unwrap());
    // At once: "stopping" (the pane still reads running).
    let state = rig.chat.state();
    assert_eq!(state["stopping"], true);
    assert_eq!(state["phase"], "cancelling");
    assert_eq!(rig.pane_status(), "running");
    rig.wait_phase(Phase::Idle);
    let state = rig.chat.state();
    assert_eq!(state["stopping"], false);
    assert_eq!(state["turnInFlight"], false);
    assert_eq!(rig.items().last().unwrap()["outcome"], "interrupted");
    assert_eq!(rig.pane_status(), "idle");
    // Nothing runs: a second stop finds no turn.
    assert!(!rig.chat.cancel().unwrap());
}

#[test]
fn ctrl_c_and_esc_on_the_pane_stop_the_turn_and_other_keys_are_not_a_terminal() {
    let rig = Rig::new("");
    rig.chat.send("slow").unwrap();
    assert!(wait_until(10, || rig.items().iter().any(|i| i["kind"] == "assistant")));
    assert_eq!(terminal_input(&rig.id(), "\u{3}", false), Some(Ok(())));
    rig.wait_phase(Phase::Idle);
    assert_eq!(rig.items().last().unwrap()["outcome"], "interrupted");
    // Enter alone is nothing; an arrow key is not a terminal key here.
    assert_eq!(terminal_input(&rig.id(), "", true), Some(Ok(())));
    assert_eq!(terminal_input(&rig.id(), "\u{1b}[A", false), Some(Err(NOT_A_TERMINAL.to_string())));
    // A line of text is a message.
    assert_eq!(terminal_input(&rig.id(), "stream 1", true), Some(Ok(())));
    assert!(wait_until(20, || rig.items().iter().any(|i| i["id"] == "end:2")));
    // A pane that is not a chat session is not ours.
    assert_eq!(terminal_input("some-terminal-pane", "\u{3}", false), None);
}

#[test]
fn a_message_while_the_agent_works_is_steered_in_when_the_agent_takes_it() {
    let rig = Rig::new("steer=1");
    assert!(rig.chat.can_steer());
    rig.chat.send("slow").unwrap();
    assert!(wait_until(10, || rig.items().iter().any(|i| i["kind"] == "assistant")));
    assert_eq!(rig.chat.steer("also check B"), Some(Ok(())));
    let items = rig.items();
    let users: Vec<&str> = items.iter().filter(|i| i["kind"] == "user").map(|i| i["text"].as_str().unwrap()).collect();
    assert_eq!(users, ["slow", "also check B"]);
    assert!(wait_until(10, || rig.items().iter().any(|i| i["text"].as_str().is_some_and(|t| t.contains("steered:also check B")))));
    rig.chat.cancel().unwrap();
    rig.wait_phase(Phase::Idle);
    // Nothing running: nothing to steer.
    assert_eq!(rig.chat.steer("late"), None);
}

#[test]
fn an_agent_that_does_not_take_text_while_it_works_gets_the_message_queued_instead() {
    let rig = Rig::new("");
    assert!(!rig.chat.can_steer());
    rig.chat.send("slow").unwrap();
    assert!(wait_until(10, || rig.items().iter().any(|i| i["kind"] == "assistant")));
    assert_eq!(rig.chat.steer("also check B"), None);
    assert_eq!(rig.chat.send("again"), Err(SendError::Busy));
    rig.chat.cancel().unwrap();
    rig.wait_phase(Phase::Idle);
}

/// The outbox over a chat session, as `LiveHost` does it (without the Tauri app).
struct ChatOutboxHost {
    published: Mutex<Vec<Vec<OutboxMessage>>>,
}

impl Host for ChatOutboxHost {
    fn pane_kind(&self, pane_id: &str) -> Option<PaneKind> {
        get(pane_id).map(|_| PaneKind::Agent)
    }
    fn run_controls(&self, _: &str) -> bool {
        false
    }
    fn look(&self, pane_id: &str) -> Option<Look> {
        crate::pane_outbox::chat::look(&*get(pane_id)?)
    }
    fn type_message(&self, pane_id: &str, text: &str) -> Attempt {
        match get(pane_id) {
            Some(chat) => crate::pane_outbox::chat::type_message(&chat, text),
            None => Attempt::Gone,
        }
    }
    fn publish(&self, _: &str, messages: &[OutboxMessage]) {
        self.published.lock().push(messages.to_vec());
    }
    fn steer(&self, pane_id: &str, text: &str) -> Option<Result<(), String>> {
        get(pane_id)?.steer(text)
    }
}

fn outbox() -> Outbox {
    let timing = Timing { poll: Duration::from_millis(20), after_send: Duration::from_millis(0), ..Timing::default() };
    Outbox::with_timing(Arc::new(ChatOutboxHost { published: Mutex::new(Vec::new()) }), timing)
}

#[test]
fn messages_from_the_chat_tab_queue_behind_a_running_turn_and_go_in_when_it_ends() {
    let rig = Rig::new("");
    let outbox = outbox();
    let first = outbox.send(&rig.id(), "slow").unwrap();
    assert!(wait_until(10, || outbox.list(&rig.id()).iter().any(|m| m.id == first.id && m.state == State::Sent)));
    assert!(wait_until(10, || rig.items().iter().any(|i| i["kind"] == "assistant")));
    // The agent works and does not take text: the second message waits in the outbox.
    let second = outbox.send(&rig.id(), "stream 2").unwrap();
    assert_eq!(second.state, State::Queued);
    std::thread::sleep(Duration::from_millis(150));
    let waiting = outbox.list(&rig.id()).into_iter().find(|m| m.id == second.id).unwrap();
    assert_eq!(waiting.state, State::Queued);
    assert!(waiting.waiting_for.as_deref().unwrap().contains("working"));
    // Stop the first: the queued one goes in by itself.
    rig.chat.cancel().unwrap();
    assert!(wait_until(30, || rig.items().iter().any(|i| i["id"] == "end:2")), "{:#?}", rig.kinds());
    assert!(wait_until(10, || outbox.list(&rig.id()).iter().any(|m| m.id == second.id && m.state == State::Sent)));
    let users: Vec<String> = rig.items().iter().filter(|i| i["kind"] == "user").map(|i| i["text"].as_str().unwrap().to_string()).collect();
    assert_eq!(users, ["slow", "stream 2"]);
}

#[test]
fn a_message_to_a_steering_agent_goes_into_the_running_turn_at_once_and_is_listed_as_sent() {
    let rig = Rig::new("steer=1");
    let outbox = outbox();
    outbox.send(&rig.id(), "slow").unwrap();
    assert!(wait_until(10, || rig.items().iter().any(|i| i["kind"] == "assistant")));
    let steered = outbox.send(&rig.id(), "also B").unwrap();
    assert_eq!(steered.state, State::Sent);
    assert!(outbox.list(&rig.id()).iter().any(|m| m.id == steered.id && m.state == State::Sent));
    assert!(wait_until(10, || rig.items().iter().any(|i| i["text"].as_str().is_some_and(|t| t.contains("steered:also B")))));
    rig.chat.cancel().unwrap();
    rig.wait_phase(Phase::Idle);
}

// ---- crashes -----------------------------------------------------------------------------------------------------

#[test]
fn an_adapter_that_dies_mid_turn_leaves_a_consistent_chat_and_the_next_message_brings_it_back() {
    let rig = Rig::new("");
    rig.chat.send("crashtool").unwrap();
    assert!(wait_until(30, || rig.items().iter().any(|i| i["kind"] == "turnEnd")), "{:#?}", rig.kinds());
    let items = rig.items();
    let kinds = rig.kinds();
    assert_eq!(kinds.iter().filter(|k| *k == "turnEnd").count(), 1);
    assert_eq!(kinds.iter().filter(|k| *k == "toolCall").count(), kinds.iter().filter(|k| *k == "toolResult").count(), "no tool keeps spinning");
    assert_eq!(items.iter().find(|i| i["kind"] == "toolResult").unwrap()["ok"], false);
    assert_eq!(items.iter().find(|i| i["kind"] == "turnEnd").unwrap()["outcome"], "failed");
    assert!(items.iter().any(|i| i["noticeKind"] == "error"), "{kinds:?}");
    assert!(wait_until(10, || rig.pane_status() == "error"), "the pane reads error after a crash");
    assert_eq!(rig.chat.readiness(), Readiness::Ready, "the next message restarts the adapter");

    // The conversation goes on in the same chat, with new ids.
    assert_eq!(rig.chat.send("stream 2"), Ok(Sent::Started));
    assert!(wait_until(30, || rig.items().iter().any(|i| i["id"] == "end:2")), "{:#?}", rig.kinds());
    let ids: Vec<String> = rig.items().iter().map(|i| i["id"].as_str().unwrap().to_string()).collect();
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "ids stay unique: {ids:?}");
    assert_eq!(rig.pane_status(), "idle");
}

#[test]
fn a_closed_pane_stops_the_adapter_through_the_registry() {
    let rig = Rig::new("");
    rig.say("stream 1");
    let pid = rig.chat.session().connection().expect("live").pid();
    assert!(crate::proc_util::pid_alive(pid));
    // What the registry does when a pane is closed.
    let pane = rig.registry.lock().take(&rig.id()).expect("the pane");
    crate::pty::registry::shutdown_pane(pane);
    assert!(rig.chat.is_closed());
    assert!(wait_until(10, || !crate::proc_util::pid_alive(pid)), "the adapter process is gone");
    assert_eq!(rig.chat.send("hi"), Err(SendError::Gone));
}

// ---- the bridge routes -------------------------------------------------------------------------------------------

#[test]
fn the_bridge_refuses_a_start_without_what_it_needs_and_offers_the_terminal() {
    let registry = Arc::new(Mutex::new(PaneRegistry::new()));
    let deps = StartDeps { registry: registry.clone(), host: Arc::new(Recorder::default()), env: Arc::new(TestEnv { root: std::env::temp_dir() }), conversations: None };
    let (status, body) = super::api::start(&json!({ "folder": "work" }), &deps).unwrap_err();
    assert_eq!((status, body["code"].as_str()), (400, Some("INVALID_ARGUMENT")));
    let (status, body) = super::api::start(&json!({ "agent": "codex" }), &deps).unwrap_err();
    assert_eq!((status, body["code"].as_str()), (400, Some("INVALID_ARGUMENT")));
    if !crate::acp::chat_enabled() {
        // Labs off: nothing starts, and the answer says the terminal can run the agent.
        let (status, body) = super::api::start(&json!({ "agent": "codex", "folder": std::env::temp_dir(), "lane": "acp" }), &deps).unwrap_err();
        assert_eq!(status, 409);
        assert_eq!(body["code"], "ACP_LABS_OFF");
        assert_eq!(body["fallback"], "terminal");
        assert!(registry.lock().list().is_empty());
    }
}

#[test]
fn the_bridge_reads_the_state_and_stops_the_turn_of_a_chat_session() {
    let rig = Rig::new("");
    let (status, state) = super::api::state(&rig.id()).unwrap();
    assert_eq!((status, state["lane"].as_str(), state["phase"].as_str()), (200, Some("acp"), Some("idle")));
    // Nothing runs: nothing to stop.
    let (_, stopped) = super::api::cancel(&rig.id()).unwrap();
    assert_eq!(stopped["stopped"], false);
    rig.chat.send("slow").unwrap();
    assert!(wait_until(10, || rig.items().iter().any(|i| i["kind"] == "assistant")));
    let (_, stopped) = super::api::cancel(&rig.id()).unwrap();
    assert_eq!(stopped["stopped"], true);
    let (_, state) = super::api::state(&rig.id()).unwrap();
    assert_eq!(state["stopping"], true);
    rig.wait_phase(Phase::Idle);
    // A pane that is not a chat session.
    assert_eq!(super::api::state("a-terminal-pane").unwrap_err().0, 404);
    assert_eq!(super::api::cancel("a-terminal-pane").unwrap_err().0, 404);
}

// ---- the add-on and the sign-in (A5) ------------------------------------------------------------------------------

#[test]
fn a_missing_add_on_is_its_own_start_error_and_leaves_nothing_behind() {
    let dir = std::env::temp_dir().join(format!("pm-acp-chat-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut cfg = fake_agent::agent_config("", &dir.join("store"));
    cfg.label = "Fake".into();
    cfg.pin_policy = crate::acp::config::PinPolicy::Enforce;
    cfg.addon.size_mb = Some(12);
    cfg.pins = vec![crate::acp::config::AdapterPin {
        package: "@scope/fake-acp".into(),
        version: "1.0.0".into(),
        entry: "{adapters}/fake/node_modules/@scope/fake-acp/dist/index.js".into(),
        sha256: Some("0".repeat(64)),
    }];
    let registry = Arc::new(Mutex::new(PaneRegistry::new()));
    let deps = StartDeps { registry: registry.clone(), host: Arc::new(Recorder::default()), env: Arc::new(TestEnv { root: dir.clone() }), conversations: None };
    let error = start(&deps, cfg, StartRequest { agent_id: "fake".into(), folder: dir, explicit: true }).err().expect("no add-on, no chat");
    assert_eq!(error.code, "ACP_ADDON_MISSING");
    assert!(error.message.contains("add-on for Fake (about 12 MB)"), "{}", error.message);
    assert!(error.fallback);
    assert!(registry.lock().list().is_empty(), "no pane was made");
}

#[test]
fn a_login_that_runs_out_shows_the_sign_in_card_and_try_again_sends_the_same_message() {
    let dir = std::env::temp_dir().join(format!("pm-acp-chat-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let auth_file = dir.join("signed-out");
    let rig = Rig::with(&format!("auth_file={}", auth_file.display()), |cfg| {
        cfg.timeouts.idle_ttl_secs = 60;
        cfg.addon.signin_hint = "Type /login in the terminal.".into();
    });
    rig.say("stream 2");
    assert_eq!(rig.chat.state()["signIn"]["needed"], false);

    // The adapter sleeps; the login ran out meanwhile.
    assert_eq!(rig.chat.session().tick(Instant::now() + Duration::from_secs(61)), crate::acp::session::TickAction::Slept);
    std::fs::write(&auth_file, "x").unwrap();
    let refused = rig.chat.send("stream 3").unwrap_err();
    assert_eq!(refused, SendError::Refused("The agent needs you to sign in.".into()));
    let state = rig.chat.state();
    assert_eq!(state["signIn"]["needed"], true);
    assert_eq!(state["signIn"]["hint"], "Type /login in the terminal.");
    assert!(rig.chat.sign_in_needed());
    assert!(rig.host.states.load(Ordering::SeqCst) >= 1, "viewers are told so the card appears");

    // Still signed out: the card stays, and the message is kept.
    assert!(matches!(rig.chat.retry_after_sign_in(), Err(SendError::Refused(_))));
    assert!(rig.chat.sign_in_needed());

    // Signed in again: the held message goes, the card goes.
    std::fs::remove_file(&auth_file).unwrap();
    assert_eq!(rig.chat.retry_after_sign_in().unwrap(), Sent::Started);
    assert!(wait_until(30, || rig.items().iter().any(|i| i["kind"] == "user" && i["text"] == "stream 3")));
    assert_eq!(rig.chat.state()["signIn"]["needed"], false);
    // Nothing is held now.
    assert!(matches!(rig.chat.retry_after_sign_in(), Err(SendError::Refused(_))));
}

// ---- robustness (R17) --------------------------------------------------------------------------------------------

fn turn_ended(rig: &Rig, n: u64) -> bool {
    rig.items().iter().any(|i| i["kind"] == "turnEnd" && i["id"] == format!("end:{n}"))
}

#[test]
fn a_second_message_while_the_adapter_starts_is_busy_and_does_not_take_the_firsts_text() {
    // The adapter takes ~1.2 s to start: the first message is still bringing it up when the second arrives.
    let rig = Rig::new("deaf_ms=1200");
    assert!(rig.chat.session().sleep().unwrap(), "the adapter naps; the next message starts it again");
    let first = {
        let chat = rig.chat.clone();
        std::thread::spawn(move || chat.send("echo first"))
    };
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(rig.chat.send("echo second"), Err(SendError::Busy), "a message while one is starting is refused at once");
    assert_eq!(first.join().unwrap(), Ok(Sent::Started));
    assert!(wait_until(30, || rig.items().iter().any(|i| i["kind"] == "turnEnd")), "{:#?}", rig.kinds());
    let users: Vec<String> = rig.items().iter().filter(|i| i["kind"] == "user").map(|i| i["text"].as_str().unwrap().to_string()).collect();
    assert_eq!(users, ["echo first"], "the chat shows the message that went, once");
    assert_eq!(rig.items().iter().find(|i| i["kind"] == "assistant").unwrap()["text"], "echo: echo first");
}

#[test]
fn a_permission_card_left_open_when_the_adapter_dies_expires_and_never_answers() {
    let rig = Rig::new("");
    rig.chat.send("permcrash").unwrap();
    let card = rig.wait_card();
    assert!(wait_until(30, || rig.cards().is_empty()), "the card goes when the adapter dies");
    let after = questions::store().get(&card.id, now_ms()).expect("the card is still readable for a while");
    assert_eq!(after.state, QuestionState::TimedOut, "closed as expired, not cancelled and not answered");
    assert!(after.answer.is_none(), "nothing was chosen on the person's behalf");
    assert!(wait_until(30, || rig.items().iter().any(|i| i["kind"] == "turnEnd")), "{:#?}", rig.kinds());
    assert_eq!(rig.chat.state()["turnInFlight"], false);
    assert_eq!(rig.chat.state()["permissions"].as_array().unwrap().len(), 0);
}

#[test]
fn closing_a_chat_expires_its_open_card() {
    let rig = Rig::new("");
    rig.chat.send("perm").unwrap();
    let card = rig.wait_card();
    rig.chat.close();
    assert_eq!(questions::store().get(&card.id, now_ms()).unwrap().state, QuestionState::TimedOut);
}

#[test]
fn restart_chat_brings_a_stopped_adapter_back_in_the_same_chat() {
    let rig = Rig::new("");
    rig.chat.send("crash").unwrap();
    assert!(wait_until(30, || rig.chat.state()["phase"] == "exited"), "{:?}", rig.chat.state()["phase"]);
    let state = rig.chat.state();
    assert_eq!(state["exit"]["kind"], "crashed");
    assert_eq!(state["turnInFlight"], false, "no spinner after a crash");
    rig.chat.restart().expect("the adapter starts again");
    assert_ne!(rig.chat.state()["phase"], "exited");
    rig.say("stream 1");
    assert_eq!(rig.items().iter().filter(|i| i["kind"] == "user").count(), 2, "{:#?}", rig.kinds());
}

#[test]
fn a_closed_chat_cannot_be_restarted() {
    let rig = Rig::new("");
    rig.chat.close();
    assert!(rig.chat.restart().is_err());
}

#[test]
fn stop_during_a_tool_call_ends_the_tool_and_the_turn_with_nothing_left_spinning() {
    let rig = Rig::new("");
    rig.chat.send("toolhang").unwrap();
    assert!(wait_until(10, || rig.kinds().iter().any(|k| k == "toolCall")), "{:#?}", rig.kinds());
    assert!(rig.chat.cancel().unwrap());
    assert!(wait_until(30, || turn_ended(&rig, 1)), "{:#?}", rig.kinds());
    let kinds = rig.kinds();
    assert_eq!(kinds.iter().filter(|k| *k == "toolCall").count(), kinds.iter().filter(|k| *k == "toolResult").count(), "the open tool is settled: {kinds:?}");
    assert_eq!(rig.items().iter().find(|i| i["kind"] == "turnEnd").unwrap()["outcome"], "interrupted");
    rig.wait_phase(Phase::Idle);
    assert_eq!(rig.chat.state()["stopping"], false);
    assert_eq!(rig.chat.send("stream 1"), Ok(Sent::Started), "the chat takes the next message");
}

#[test]
fn bytes_that_are_not_text_and_broken_lines_are_skipped_and_the_turn_goes_on() {
    let rig = Rig::new("");
    rig.say("garbage");
    let items = rig.items();
    assert_eq!(items.iter().find(|i| i["kind"] == "assistant").unwrap()["text"], "before after");
    assert_eq!(items.iter().find(|i| i["kind"] == "turnEnd").unwrap()["outcome"], "completed");
    assert_eq!(rig.chat.state()["phase"], "idle");
}

#[test]
fn an_edit_that_touches_very_many_files_still_shows_its_diff_bounded() {
    let rig = Rig::new("");
    rig.say("bigdiff 60");
    let call = rig.items().into_iter().find(|i| i["kind"] == "toolCall").expect("a tool call");
    let edits = call["input"]["edits"].as_array().expect("the diff is kept, not dropped");
    assert_eq!(edits.len(), crate::acp::chat::mapper::MAX_SHOWN_DIFFS);
    assert_eq!(call["input"]["omitted_edits"], 60 - crate::acp::chat::mapper::MAX_SHOWN_DIFFS as u64);
    assert!(call.to_string().len() < 40_000, "the item stays small: {} bytes", call.to_string().len());
    assert_eq!(rig.chat.state()["phase"], "idle");
}

#[test]
fn two_chats_for_the_same_agent_do_not_share_cards_or_stops() {
    let a = Rig::new("");
    let b = Rig::new("");
    assert_ne!(a.id(), b.id());
    a.chat.send("perm").unwrap();
    let card = a.wait_card();
    assert!(b.cards().is_empty(), "a card belongs to the chat that asked");
    b.say("stream 2");
    assert_eq!(a.chat.state()["permissions"].as_array().unwrap().len(), 1, "the other chat's turn did not touch it");
    assert!(!b.chat.cancel().unwrap(), "stopping the idle chat does nothing to the other");
    assert_eq!(questions::store().get(&card.id, now_ms()).unwrap().state, QuestionState::Pending);
    // Closing one leaves the other running.
    let pid_b = b.chat.session().connection().unwrap().pid();
    a.chat.close();
    assert!(crate::proc_util::pid_alive(pid_b));
    assert_eq!(b.chat.readiness(), Readiness::Ready);
}

#[test]
fn removing_chat_for_an_agent_stops_its_open_chats_and_their_adapters() {
    // Their own agent id, so this does not stop the chats of tests that run beside it.
    let id = format!("fake-rm-{}", uuid::Uuid::new_v4().simple());
    let a = Rig::with("", |cfg| cfg.id = id.clone());
    let b = Rig::with("", |cfg| cfg.id = id.clone());
    a.say("stream 1");
    b.say("stream 1");
    let pids = [a.chat.session().connection().unwrap().pid(), b.chat.session().connection().unwrap().pid()];
    a.chat.send("perm").unwrap();
    let card = a.wait_card();
    assert!(crate::acp::chat::close_agent(&id.to_uppercase()) == 2);
    assert!(a.chat.is_closed() && b.chat.is_closed());
    for pid in pids {
        assert!(wait_until(10, || !crate::proc_util::pid_alive(pid)), "no adapter is left running");
    }
    assert_eq!(questions::store().get(&card.id, now_ms()).unwrap().state, QuestionState::TimedOut);
    assert_eq!(crate::acp::chat::close_agent(&id), 0, "again: nothing left to stop");
    assert_eq!(a.chat.send("hi"), Err(SendError::Gone));
}

#[test]
fn an_adapter_that_never_answers_the_start_fails_in_time_and_leaves_no_process() {
    let dir = std::env::temp_dir().join(format!("pm-acp-chat-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    // initialize is answered, then the agent sleeps: session/new gets no answer inside the limit.
    let mut cfg = fake_agent::agent_config("deaf_ms=60000", &dir.join("store"));
    cfg.timeouts.request_secs = 2;
    cfg.timeouts.new_session_secs = 2;
    let registry = Arc::new(Mutex::new(PaneRegistry::new()));
    let deps = StartDeps { registry: registry.clone(), host: Arc::new(Recorder::default()), env: Arc::new(TestEnv { root: dir.clone() }), conversations: None };
    let started = Instant::now();
    let error = start(&deps, cfg, StartRequest { agent_id: "fake".into(), folder: dir.clone(), explicit: true }).err().expect("the start fails");
    assert!(started.elapsed() < Duration::from_secs(20), "{:?}", started.elapsed());
    assert_eq!(error.code, "ACP_START_FAILED");
    assert!(error.fallback, "the terminal can run the agent instead");
    assert!(registry.lock().list().is_empty(), "no pane is left behind");
}
