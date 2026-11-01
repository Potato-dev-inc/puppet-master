//! A4: a chat session is a saved conversation (restart, hand-over to a terminal and back), its settings, and what an
//! agent can do to it through the agent tools. Fake agent, real processes and pipes; no real agent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::start::start;
use super::surface::{self, intercept};
use super::{get, on_pane_closed, AcpChat, AgentAnswerError, QuietHost, SendError, StartDeps, StartRequest};
use crate::acp::config::{AgentConfig, HostEnv};
use crate::acp::conversations::{AgentSource, Conversations};
use crate::acp::fake_agent;
use crate::acp::handover::{PaneActivity, PaneHost, TerminalStart};
use crate::acp::record::{LastState, RecordStore};
use crate::acp::supervisor::{Supervisor, SupervisorConfig};
use crate::audit::Who;
use crate::pty::PaneRegistry;
use crate::session_chat::PushedSource;

struct Env {
    root: PathBuf,
}

impl HostEnv for Env {
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

struct Agents(BTreeMap<String, Arc<AgentConfig>>);

impl AgentSource for Agents {
    fn config(&self, agent: &str) -> Option<Arc<AgentConfig>> {
        self.0.get(agent).cloned()
    }
}

/// Terminal panes that exist only as a table.
#[derive(Default)]
struct Panes {
    state: Mutex<BTreeMap<String, PaneActivity>>,
    started: Mutex<Vec<TerminalStart>>,
}

impl PaneHost for Panes {
    fn start(&self, start: &TerminalStart) -> Result<String, String> {
        let mut state = self.state.lock();
        let id = format!("term-{}", state.len() + 1);
        state.insert(id.clone(), PaneActivity::Idle);
        self.started.lock().push(start.clone());
        Ok(id)
    }
    fn activity(&self, pane_id: &str) -> PaneActivity {
        self.state.lock().get(pane_id).copied().unwrap_or(PaneActivity::Gone)
    }
    fn stop(&self, pane_id: &str) -> Result<(), String> {
        self.state.lock().insert(pane_id.to_string(), PaneActivity::Gone);
        Ok(())
    }
}

struct Rig {
    dir: PathBuf,
    opts: String,
    conversations: Conversations,
    deps: StartDeps,
    chat: Arc<AcpChat>,
    panes: Panes,
}

fn agents(opts: &str, dir: &Path) -> Arc<Agents> {
    let mut cfg = fake_agent::agent_config(opts, &dir.join("agent-sessions"));
    cfg.resume_command = vec!["resume".into(), "{session}".into()];
    Arc::new(Agents(BTreeMap::from([("fake".to_string(), Arc::new(cfg))])))
}

fn conversations_in(dir: &Path, opts: &str) -> Conversations {
    let env: Arc<dyn HostEnv> = Arc::new(Env { root: dir.to_path_buf() });
    Conversations::new(RecordStore::at(dir.join("conversations.json")), Arc::new(Supervisor::new(SupervisorConfig::default())), env, agents(opts, dir))
}

fn deps_for(dir: &Path, conversations: &Conversations) -> StartDeps {
    StartDeps { registry: Arc::new(Mutex::new(PaneRegistry::new())), host: Arc::new(QuietHost), env: Arc::new(Env { root: dir.to_path_buf() }), conversations: Some(conversations.clone()) }
}

impl Rig {
    fn new(opts: &str) -> Rig {
        let dir = std::env::temp_dir().join(format!("pm-acp-ws-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let conversations = conversations_in(&dir, opts);
        let deps = deps_for(&dir, &conversations);
        let cfg = agents(opts, &dir).0["fake"].as_ref().clone();
        let chat = start(&deps, cfg, StartRequest { agent_id: "fake".into(), folder: dir.clone(), explicit: true }).expect("the fake agent starts");
        Rig { dir, opts: opts.to_string(), conversations, deps, chat, panes: Panes::default() }
    }

    fn id(&self) -> String {
        self.chat.pane_id().to_string()
    }

    fn say(&self, text: &str) {
        self.chat.send(text).unwrap_or_else(|e| panic!("send {text:?}: {e:?}"));
        let turn = self.chat.session().snapshot().turn;
        assert!(wait_until(30, || self.chat.items().iter().any(|i| serde_json::to_value(i).unwrap()["id"] == format!("end:{turn}"))), "the turn did not end: {:#?}", self.ids());
    }

    fn ids(&self) -> Vec<String> {
        self.chat.items().iter().map(|i| i.id().to_string()).collect()
    }

    fn texts(&self) -> Vec<String> {
        self.chat.items().iter().filter_map(|i| serde_json::to_value(i).unwrap().get("text").and_then(Value::as_str).map(str::to_string)).collect()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        // Not `on_pane_closed`: that would mark the record closed; the registry entry is gone with the test.
        self.chat.retire();
        let _ = self.opts.len();
        let _ = std::fs::remove_dir_all(&self.dir);
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

fn wait_for_ask(chat: &AcpChat) -> (String, Vec<String>) {
    assert!(wait_until(30, || !chat.state()["permissions"].as_array().unwrap().is_empty()), "no permission question appeared");
    let state = chat.state();
    let first = &state["permissions"][0];
    let options = first["options"].as_array().unwrap().iter().map(|o| o["id"].as_str().unwrap().to_string()).collect();
    (first["questionId"].as_str().unwrap().to_string(), options)
}

// ---- the conversation behind the pane ---------------------------------------------------------------------------

#[test]
fn a_chat_session_is_a_saved_conversation_with_the_panes_id() {
    let rig = Rig::new("");
    let id = rig.id();
    let record = rig.conversations.record(&id).expect("the conversation is saved");
    assert_eq!((record.agent.as_str(), record.last_known_state), ("fake", LastState::Live));
    let state = rig.chat.state();
    assert_eq!(state["conversation"], json!(id));
    assert_eq!(state["inTerminal"], Value::Null);
    assert_eq!(state["agentAnswering"], json!(false), "people answer by default");
    assert!(rig.deps.registry.lock().get(&id).is_some(), "the pane is in the registry under the conversation's id");
    rig.say("stream 2");
    assert!(rig.conversations.record(&id).unwrap().session_id.is_some());
}

#[test]
fn closing_the_pane_ends_the_conversation_but_keeps_its_record() {
    let rig = Rig::new("");
    let id = rig.id();
    on_pane_closed(&id);
    assert!(get(&id).is_none());
    assert_eq!(rig.conversations.record(&id).unwrap().last_known_state, LastState::Closed);
    assert!(rig.conversations.open_ids().is_empty(), "a closed conversation is not brought back");
}

#[test]
fn a_failed_first_start_leaves_no_conversation_behind() {
    let dir = std::env::temp_dir().join(format!("pm-acp-ws-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let conversations = conversations_in(&dir, "auth=1");
    let deps = deps_for(&dir, &conversations);
    let cfg = agents("auth=1", &dir).0["fake"].as_ref().clone();
    let failed = start(&deps, cfg, StartRequest { agent_id: "fake".into(), folder: dir.clone(), explicit: true });
    assert!(failed.is_err());
    assert!(conversations.list().is_empty(), "{:?}", conversations.list());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- hand-over -------------------------------------------------------------------------------------------------

#[test]
fn open_in_terminal_pauses_the_chat_and_back_to_chat_rebuilds_it_from_the_agents_own_history() {
    let rig = Rig::new("");
    let id = rig.id();
    rig.say("stream 2");
    assert!(rig.ids().iter().any(|i| i == "u1"), "{:?}", rig.ids());

    let handed = rig.conversations.open_in_terminal(&id, false, &rig.panes).expect("opens in a terminal");
    let state = rig.chat.state();
    assert_eq!(state["inTerminal"]["paneId"], json!(handed.pane_id));
    assert_eq!(rig.panes.started.lock()[0].extra_args, vec!["resume".to_string(), rig.conversations.record(&id).unwrap().session_id.unwrap()]);
    match rig.chat.send("hello") {
        Err(SendError::Refused(why)) => assert!(why.contains("terminal"), "{why}"),
        other => panic!("a chat that is open in a terminal takes no message: {other:?}"),
    }
    // The terminal's pane knows the conversation it holds.
    assert_eq!(rig.conversations.of_terminal(&handed.pane_id).map(|v| v.id), Some(id.clone()));

    let back = rig.conversations.back_to_chat(&id, false, &rig.panes).expect("back to chat");
    assert_eq!(back.how, "loaded");
    // The chat started over from the replay: the agent's history is in, the first live items are not (they came back as history).
    assert!(wait_until(30, || rig.texts().iter().any(|t| t == "earlier question")), "{:?}", rig.texts());
    assert!(!rig.ids().iter().any(|i| i == "u1"), "the old live items were dropped: {:?}", rig.ids());
    assert_eq!(rig.chat.state()["inTerminal"], Value::Null);
    rig.say("stream 1");
    assert!(rig.of_texts_contain("w0"));
}

impl Rig {
    fn of_texts_contain(&self, needle: &str) -> bool {
        self.texts().iter().any(|t| t.contains(needle))
    }
}

// ---- restart ----------------------------------------------------------------------------------------------------

#[test]
fn after_a_restart_the_conversation_is_a_chat_pane_that_loads_when_it_is_read() {
    let rig = Rig::new("");
    let id = rig.id();
    rig.say("stream 2");
    assert!(rig.conversations.record(&id).unwrap().session_id.is_some());
    // The app ends: its sessions go; the records and the agent's own session files stay.
    rig.chat.retire();
    rig.conversations.forget_session(&id);

    let app = conversations_in(&rig.dir, &rig.opts);
    let report = app.restore_lazily(&rig.panes);
    assert_eq!(report.restored, vec![id.clone()]);
    let deps = deps_for(&rig.dir, &app);
    let panes = super::restore_panes(&deps);
    assert_eq!(panes, vec![id.clone()]);
    assert!(deps.registry.lock().get(&id).is_some(), "the chat is listed as a pane");

    let chat = get(&id).expect("the chat is back");
    assert!(chat.items().is_empty());
    assert!(!chat.session().is_live(), "nothing starts by itself");
    // A viewer reads it: the adapter starts and loads the conversation.
    let known = None;
    let mut waited = Instant::now();
    assert!(
        wait_until(30, || {
            let _ = chat.changes(known);
            waited = Instant::now();
            chat.items().iter().any(|i| matches!(i, crate::session_chat::ChatItem::User { text, .. } if text == "earlier question"))
        }),
        "the history was not replayed into the chat: {:?} ({waited:?})",
        chat.items().iter().map(|i| i.id().to_string()).collect::<Vec<_>>()
    );
    // And the conversation goes on in the same session.
    chat.send("stream 1").expect("a message goes in");
    assert!(wait_until(30, || chat.items().iter().any(|i| serde_json::to_value(i).unwrap()["text"].as_str().is_some_and(|t| t.contains("w0")))));
    chat.retire();
    app.forget_session(&id);
}

// ---- settings and slash commands --------------------------------------------------------------------------------

#[test]
fn a_setting_is_changed_through_the_agent_not_typed_into_the_chat() {
    let rig = Rig::new("");
    let offered = rig.chat.state()["configOptions"].clone();
    assert_eq!(offered[0]["id"], "model");
    rig.chat.set_config("model", "b").expect("the agent takes it");
    assert_eq!(rig.chat.state()["configOptions"][0]["currentValue"], "b");
    assert!(rig.chat.set_config("model", "nope").is_err(), "a value the agent does not list is refused");
    assert!(rig.chat.set_config("colour", "b").is_err(), "so is an option it does not have");
    assert!(rig.chat.items().iter().all(|i| !matches!(i, crate::session_chat::ChatItem::User { .. })), "nothing was typed into the chat");
    // Not while a turn runs.
    rig.chat.send("slow").unwrap();
    assert!(wait_until(10, || rig.chat.state()["turnInFlight"] == json!(true)));
    assert_eq!(rig.chat.set_config("model", "a"), Err(crate::acp::error::AcpError::Busy));
    rig.chat.cancel().unwrap();
    assert!(wait_until(30, || rig.chat.state()["turnInFlight"] == json!(false)));
}

#[test]
fn the_agents_slash_commands_are_in_the_state() {
    let rig = Rig::new("");
    assert!(wait_until(10, || !rig.chat.state()["commands"].as_array().unwrap().is_empty()));
    assert_eq!(rig.chat.state()["commands"][0]["name"], "review");
}

// ---- answering questions: the person's switch ---------------------------------------------------------------------

#[test]
fn an_agent_cannot_answer_a_permission_question_unless_the_person_allowed_it() {
    let rig = Rig::new("");
    rig.chat.send("perm").unwrap();
    let (question, options) = wait_for_ask(&rig.chat);
    assert!(options.contains(&"reject_once".to_string()));
    assert_eq!(rig.chat.answer_for_agent(&question, "allow_once", Who::Local), Err(AgentAnswerError::NotAllowed));
    assert!(rig.chat.state()["permissions"].as_array().is_some_and(|p| p.len() == 1), "the question is still open");

    rig.chat.set_agent_answering(true);
    assert_eq!(rig.chat.state()["agentAnswering"], json!(true));
    assert_eq!(rig.chat.answer_for_agent("no-such-question", "reject_once", Who::Local), Err(AgentAnswerError::NoSuchQuestion));
    assert!(matches!(rig.chat.answer_for_agent(&question, "yes please", Who::Local), Err(AgentAnswerError::BadChoice(offered)) if offered.len() == 3));
    let name = rig.chat.answer_for_agent(&question, "reject_once", Who::Agent("session-1".into())).expect("allowed now");
    assert_eq!(name, "Reject");
    assert!(wait_until(30, || rig.of_texts_contain("chose:reject_once")), "the choice reached the agent: {:?}", rig.texts());
    // The question is closed; answering twice is not possible.
    assert_eq!(rig.chat.answer_for_agent(&question, "reject_once", Who::Local), Err(AgentAnswerError::NoSuchQuestion));
}

// ---- the agent tools ----------------------------------------------------------------------------------------------

fn call(rig: &Rig, method: &str, path: &str, body: Value, session: Option<&str>) -> Option<Result<Value, crate::operations::OperationError>> {
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    intercept(method, &segments, "", body.to_string().as_bytes(), &rig.deps, session)
}

#[test]
fn the_agent_tools_see_a_chat_by_its_handle_and_read_the_chat_not_a_screen() {
    let rig = Rig::new("");
    let id = rig.id();
    rig.say("stream 3");
    let inspected = call(&rig, "GET", &format!("agents/{id}"), Value::Null, None).expect("a chat handle is ours").unwrap();
    assert_eq!(inspected["status"], "completed");
    assert_eq!(inspected["lane"], "acp");
    assert_eq!(inspected["pane_id"], json!(id));
    assert_eq!(inspected["result"], "w0 w1 w2 ");
    assert_eq!(inspected["capabilities"]["terminal_screen"], json!(false));
    assert!(inspected["recent"].as_array().unwrap().iter().any(|r| r["kind"] == "assistant"));
    // Not ours: an ordinary handle falls through to the terminal runs.
    assert!(call(&rig, "GET", "agents/run-123", Value::Null, None).is_none());
    assert!(call(&rig, "POST", "agents/wait", json!({ "handles": ["run-123"] }), None).is_none());

    let waited = call(&rig, "POST", "agents/wait", json!({ "handles": [id], "wait_ms": 2000 }), None).unwrap().unwrap();
    assert_eq!(waited["reason"], "status");
    assert_eq!(waited["agents"][0]["status"], "completed");
    let mixed = call(&rig, "POST", "agents/wait", json!({ "handles": [id, "run-123"] }), None).unwrap();
    assert_eq!(mixed.unwrap_err().code, "MIXED_HANDLES");

    let transcript = call(&rig, "GET", &format!("agents/{id}/transcript"), Value::Null, None).unwrap().unwrap();
    assert!(transcript["items"].as_array().unwrap().iter().any(|i| i["kind"] == "user" && i["text"] == "stream 3"));
}

#[test]
fn followup_runs_the_next_turn_and_answer_prompt_needs_the_switch() {
    let rig = Rig::new("");
    let id = rig.id();
    rig.chat.set_started_by(Some("session-A"));
    let done = call(&rig, "POST", "agents/followup", json!({ "handle": id, "task": "stream 2", "wait_ms": 20000 }), Some("session-A")).unwrap().unwrap();
    assert_eq!(done["status"], "completed");
    assert_eq!(done["result"], "w0 w1 ");

    // A permission question: visible to the caller as a prompt with option ids, but not answerable.
    let asked = call(&rig, "POST", "agents/followup", json!({ "handle": id, "task": "perm", "wait_ms": 20000 }), Some("session-A")).unwrap().unwrap();
    assert_eq!(asked["status"], "waiting_input");
    assert_eq!(asked["prompt"]["answerable_by_agent"], json!(false));
    let prompt = asked["prompt"]["prompt_id"].as_str().unwrap().to_string();
    let refused = call(&rig, "POST", "agents/answer", json!({ "handle": id, "prompt_id": prompt, "choice": "allow_once" }), Some("session-A")).unwrap();
    assert_eq!(refused.unwrap_err().code, "AGENT_ANSWER_NOT_ALLOWED");

    // The person flips the switch (the Settings window's command; no agent route does).
    rig.chat.set_agent_answering(true);
    let answered = call(&rig, "POST", "agents/answer", json!({ "handle": id, "prompt_id": prompt, "choice": "reject_once" }), Some("session-A")).unwrap().unwrap();
    assert_eq!(answered["answered"]["name"], "Reject");
    let finished = call(&rig, "POST", "agents/wait", json!({ "handles": [id], "wait_ms": 20000 }), Some("session-A")).unwrap().unwrap();
    assert_eq!(finished["agents"][0]["status"], "completed");
}

#[test]
fn an_agent_may_steer_only_a_chat_it_started() {
    let rig = Rig::new("");
    let id = rig.id();
    // Started by the person: another agent reads it but cannot change it.
    for (path, body) in [
        ("agents/followup", json!({ "handle": id, "task": "stream 1" })),
        ("agents/send", json!({ "handle": id, "message": "hi" })),
        ("agents/interrupt", json!({ "handle": id })),
        ("agents/close", json!({ "handle": id })),
        ("agents/answer", json!({ "handle": id, "prompt_id": "x", "choice": "y" })),
    ] {
        let result = call(&rig, "POST", path, body, Some("session-B")).unwrap();
        assert_eq!(result.unwrap_err().code, "NOT_YOUR_CHAT", "{path}");
    }
    assert!(call(&rig, "GET", &format!("agents/{id}"), Value::Null, Some("session-B")).unwrap().is_ok(), "reading is open");
    // Its own chat: allowed.
    rig.chat.set_started_by(Some("session-B"));
    assert!(call(&rig, "POST", "agents/interrupt", json!({ "handle": id }), Some("session-B")).unwrap().is_ok());
}

#[test]
fn the_lane_word_and_the_saved_choice_pick_the_way() {
    use crate::acp::lane::RunLane;
    assert_eq!(surface::wants(Some("acp"), RunLane::Terminal, true, true), Ok(surface::Want::Chat { fallback: false }));
    assert_eq!(surface::wants(None, RunLane::Acp, true, true), Ok(surface::Want::Chat { fallback: true }));
}
