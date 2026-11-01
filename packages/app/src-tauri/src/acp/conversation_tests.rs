//! Restart, history-lost, and hand-over tests against the fake agent (real processes, no real agent).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use super::config::{AgentConfig, HostEnv};
use super::conversations::{AgentSource, ConversationError, Conversations};
use super::error::AcpError;
use super::fake_agent;
use super::handover::{HandOverError, PaneActivity, PaneHost, TerminalStart};
use super::record::{LastState, NoticeKind, RecordStore};
use super::session::{Readiness, SessionEvent, StartKind};
use super::supervisor::{Supervisor, SupervisorConfig};
use super::writers::Writer;

struct TestHost {
    root: PathBuf,
}

impl HostEnv for TestHost {
    fn var(&self, name: &str) -> Option<OsString> {
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

/// Panes that exist only as a table.
#[derive(Default)]
struct FakePanes {
    state: Mutex<BTreeMap<String, PaneActivity>>,
    started: Mutex<Vec<TerminalStart>>,
    fail_start: Mutex<bool>,
    fail_stop: Mutex<bool>,
    next: Mutex<u32>,
}

impl FakePanes {
    fn set(&self, pane: &str, activity: PaneActivity) {
        self.state.lock().insert(pane.to_string(), activity);
    }
}

impl PaneHost for FakePanes {
    fn start(&self, start: &TerminalStart) -> Result<String, String> {
        if *self.fail_start.lock() {
            return Err("no window".into());
        }
        self.started.lock().push(start.clone());
        let mut next = self.next.lock();
        *next += 1;
        let id = format!("pane-{next}");
        self.state.lock().insert(id.clone(), PaneActivity::Idle);
        Ok(id)
    }
    fn activity(&self, pane_id: &str) -> PaneActivity {
        self.state.lock().get(pane_id).copied().unwrap_or(PaneActivity::Gone)
    }
    fn stop(&self, pane_id: &str) -> Result<(), String> {
        if *self.fail_stop.lock() {
            return Err("it would not stop".into());
        }
        self.state.lock().insert(pane_id.to_string(), PaneActivity::Gone);
        Ok(())
    }
}

struct Rig {
    dir: PathBuf,
    opts: String,
    conversations: Conversations,
    panes: FakePanes,
}

fn agents(opts: &str, dir: &Path) -> Arc<Agents> {
    let mut cfg = fake_agent::agent_config(opts, &dir.join("agent-sessions"));
    cfg.resume_command = vec!["resume".into(), "{session}".into()];
    Arc::new(Agents(BTreeMap::from([("fake".to_string(), Arc::new(cfg))])))
}

fn conversations_in(dir: &Path, opts: &str) -> Conversations {
    let host: Arc<dyn HostEnv> = Arc::new(TestHost { root: dir.to_path_buf() });
    Conversations::new(RecordStore::at(dir.join("conversations.json")), Arc::new(Supervisor::new(SupervisorConfig::default())), host, agents(opts, dir))
}

impl Rig {
    fn new(opts: &str) -> Rig {
        let dir = std::env::temp_dir().join(format!("pm-acp-conv-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Rig { conversations: conversations_in(&dir, opts), dir, opts: opts.to_string(), panes: FakePanes::default() }
    }

    /// The app, started again: a new supervisor and a new ledger over the same record file and agent store.
    fn restarted(&self) -> Conversations {
        conversations_in(&self.dir, &self.opts)
    }

    fn new_started(&self) -> String {
        let view = self.conversations.create("fake", &self.dir).unwrap();
        let session = self.conversations.session(&view.id).unwrap();
        session.prompt_text("stream 2").unwrap();
        wait_idle(&session);
        view.id
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        for view in self.conversations.list() {
            if let Some(session) = self.conversations.session(&view.id) {
                session.close();
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn wait_idle(session: &super::session::AcpSession) {
    let snap = session.wait_for(|s| s.turn >= 1 && !s.turn_in_flight(), Duration::from_secs(30));
    assert!(snap.is_some(), "the turn did not finish");
}

fn events_of(session: &super::session::AcpSession) -> Arc<Mutex<Vec<SessionEvent>>> {
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = log.clone();
    session.add_sink(Arc::new(move |e| sink.lock().push(e.clone())));
    log
}

// ---- the record ---------------------------------------------------------------------------------------

#[test]
fn the_record_learns_the_agents_session_id_and_survives_a_restart() {
    let rig = Rig::new("");
    let id = rig.new_started();
    let record = rig.conversations.record(&id).unwrap();
    let session_id = record.session_id.clone().expect("the id is written as soon as the agent made it");
    assert_eq!((record.agent.as_str(), record.last_known_state), ("fake", LastState::Live));
    assert_eq!(record.cwd, rig.dir.to_string_lossy());
    assert_eq!(rig.conversations.session(&id).unwrap().snapshot().session_id.as_deref(), Some(session_id.as_str()));

    let again = rig.restarted();
    assert_eq!(again.record(&id).unwrap().session_id.as_deref(), Some(session_id.as_str()));
}

#[test]
fn after_a_restart_a_live_conversation_comes_back_lazily_and_replays() {
    let rig = Rig::new("");
    let id = rig.new_started();
    let session_id = rig.conversations.record(&id).unwrap().session_id.unwrap();
    rig.conversations.session(&id).unwrap().close();

    let app = rig.restarted();
    let report = app.restore_lazily(&rig.panes);
    assert_eq!(report.restored, vec![id.clone()]);
    let session = app.session(&id).expect("the session is back in the supervisor");
    assert!(!session.is_live(), "nothing is started until the chat is used");
    assert_eq!(app.writer(&id), Some(Writer::Chat));

    let log = events_of(&session);
    let ready = app.open_chat(&id).unwrap();
    assert_eq!(ready, Readiness::Started(StartKind::Loaded), "session/load, so the history is replayed for a view that has none");
    assert!(log.lock().iter().any(|e| matches!(e, SessionEvent::Update { replay: true, .. })));
    assert_eq!(session.snapshot().session_id.as_deref(), Some(session_id.as_str()));
    session.close();
}

#[test]
fn closed_conversations_are_not_brought_back() {
    let rig = Rig::new("");
    let id = rig.new_started();
    rig.conversations.close(&id).unwrap();
    assert!(rig.conversations.session(&id).is_none());
    let app = rig.restarted();
    let report = app.restore_lazily(&rig.panes);
    assert!(report.restored.is_empty() && report.failed.is_empty());
    assert_eq!(app.record(&id).unwrap().last_known_state, LastState::Closed);
}

#[test]
fn removing_chat_stops_a_live_conversation_and_keeps_its_record_with_a_notice() {
    let rig = Rig::new("");
    let id = rig.new_started();
    let other = rig.new_started();
    let pid = rig.conversations.session(&id).unwrap().connection().expect("live").pid();
    assert_eq!(rig.conversations.chat_removed("FAKE").len(), 2);
    assert!(rig.conversations.session(&id).is_none() && rig.conversations.session(&other).is_none());
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while crate::proc_util::pid_alive(pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!crate::proc_util::pid_alive(pid), "the adapter process is gone");
    let record = rig.conversations.record(&id).expect("the record stays");
    assert_eq!(record.last_known_state, LastState::Closed);
    assert!(record.session_id.is_some(), "the agent's session id stays, so the conversation can open in a terminal");
    assert!(record.notices.iter().any(|n| n.kind == NoticeKind::ChatRemoved && n.text.contains("terminal")));
    // Again: nothing to do, no second notice. After a restart it is not brought back.
    assert!(rig.conversations.chat_removed("fake").is_empty());
    assert_eq!(rig.conversations.record(&id).unwrap().notices.iter().filter(|n| n.kind == NoticeKind::ChatRemoved).count(), 1);
    let report = rig.restarted().restore_lazily(&rig.panes);
    assert!(report.restored.is_empty());
    // Another agent's conversations are left alone.
    let third = rig.new_started();
    assert!(rig.conversations.chat_removed("someone-else").is_empty());
    assert!(rig.conversations.session(&third).is_some());
}

#[test]
fn history_lost_becomes_a_notice_and_the_record_follows_the_new_session() {
    let rig = Rig::new("");
    let id = rig.new_started();
    let old = rig.conversations.record(&id).unwrap().session_id.unwrap();
    rig.conversations.session(&id).unwrap().close();

    // The agent in the new app run does not know the old session.
    let app = conversations_in(&rig.dir, "lose=1");
    app.restore_lazily(&rig.panes);
    let ready = app.open_chat(&id).unwrap();
    assert_eq!(ready, Readiness::Started(StartKind::HistoryLost));
    let record = app.record(&id).unwrap();
    assert_ne!(record.session_id.as_deref(), Some(old.as_str()), "the record points at the new session");
    assert!(record.session_id.is_some());
    let notice = record.notices.iter().find(|n| n.kind == NoticeKind::HistoryLost).expect("a notice item");
    assert!(!notice.seen && notice.text.contains("could not be brought back"));
    app.mark_notices_seen(&id);
    assert!(app.record(&id).unwrap().notices.iter().all(|n| n.seen));
    app.session(&id).unwrap().close();
}

#[test]
fn a_conversation_whose_agent_is_gone_is_closed_with_a_notice() {
    let rig = Rig::new("");
    let id = rig.new_started();
    rig.conversations.session(&id).unwrap().close();
    let host: Arc<dyn HostEnv> = Arc::new(TestHost { root: rig.dir.clone() });
    let none = Arc::new(Agents(BTreeMap::new()));
    let app = Conversations::new(RecordStore::at(rig.dir.join("conversations.json")), Arc::new(Supervisor::new(SupervisorConfig::default())), host, none);
    let report = app.restore_lazily(&rig.panes);
    assert_eq!(report.failed, vec![id.clone()]);
    let record = app.record(&id).unwrap();
    assert_eq!(record.last_known_state, LastState::Closed);
    assert!(record.notices.iter().any(|n| n.kind == NoticeKind::RestoreFailed));
}

#[test]
fn a_disabled_agent_cannot_start_a_conversation() {
    let dir = std::env::temp_dir().join(format!("pm-acp-conv-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut cfg = fake_agent::agent_config("", &dir.join("s"));
    cfg.disabled = true;
    cfg.disabled_note = "waiting".into();
    let host: Arc<dyn HostEnv> = Arc::new(TestHost { root: dir.clone() });
    let agents = Arc::new(Agents(BTreeMap::from([("fake".to_string(), Arc::new(cfg))])));
    let app = Conversations::new(RecordStore::at(dir.join("c.json")), Arc::new(Supervisor::new(SupervisorConfig::default())), host, agents);
    assert!(matches!(app.create("fake", &dir), Err(ConversationError::Disabled { .. })));
    assert!(matches!(app.create("other", &dir), Err(ConversationError::NoDefinition(_))));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_old_session_imported_becomes_a_conversation_on_that_session() {
    let rig = Rig::new("");
    let view = rig.conversations.import("fake", &rig.dir, "old-session-1").unwrap();
    assert_eq!(view.session_id.as_deref(), Some("old-session-1"));
    assert!(rig.conversations.session(&view.id).is_some());
    // One writer: the chat now holds the session, so a second way in is refused by the writer lookup.
    assert!(rig.conversations.writer_of_session("fake", "old-session-1").is_some());
    // It survives a restart as a conversation on the same session.
    let again = rig.restarted();
    assert_eq!(again.record(&view.id).unwrap().session_id.as_deref(), Some("old-session-1"));
    assert!(matches!(rig.conversations.import("other", &rig.dir, "x"), Err(ConversationError::NoDefinition(_))));
}

// ---- hand-over ------------------------------------------------------------------------------------------

#[test]
fn open_in_terminal_stops_the_adapter_and_starts_the_resume_command_for_the_same_session() {
    let rig = Rig::new("");
    let id = rig.new_started();
    let session_id = rig.conversations.record(&id).unwrap().session_id.unwrap();
    let session = rig.conversations.session(&id).unwrap();
    let pid = session.connection().unwrap().pid();

    let handed = rig.conversations.open_in_terminal(&id, false, &rig.panes).unwrap();
    assert_eq!(handed.session_id, session_id);
    assert!(!session.is_live(), "the adapter is stopped");
    assert!(!crate::proc_util::pid_alive(pid), "and its process is gone before the terminal starts");

    let started = rig.panes.started.lock().clone();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].agent_type, "fake");
    assert_eq!(started[0].extra_args, vec!["resume".to_string(), session_id.clone()]);
    assert_eq!(started[0].cwd, rig.dir.to_string_lossy());

    assert_eq!(rig.conversations.writer(&id), Some(Writer::Terminal { pane_id: Some(handed.pane_id.clone()) }));
    let record = rig.conversations.record(&id).unwrap();
    assert_eq!((record.last_known_state, record.terminal_pane.as_deref()), (LastState::Terminal, Some(handed.pane_id.as_str())));
    assert!(record.notices.iter().any(|n| n.kind == NoticeKind::HandedToTerminal));
}

#[test]
fn while_the_terminal_has_it_a_second_writer_is_refused_everywhere() {
    let rig = Rig::new("");
    let id = rig.new_started();
    let session = rig.conversations.session(&id).unwrap();
    let handed = rig.conversations.open_in_terminal(&id, false, &rig.panes).unwrap();

    // The session itself will not start an adapter.
    assert!(matches!(session.prompt_text("stream 1"), Err(AcpError::Held(_))));
    assert!(matches!(session.ensure_running(), Err(AcpError::Held(_))));
    assert!(matches!(rig.conversations.open_chat(&id), Err(ConversationError::Chat(AcpError::Held(_)))));
    assert!(!session.is_live());
    // The ledger refuses a chat claim, a second terminal, and a second hand-over.
    assert!(rig.conversations.claim_writer(&id, Writer::Chat).is_err());
    assert!(rig.conversations.claim_writer(&id, Writer::Terminal { pane_id: Some("other".into()) }).is_err());
    assert_eq!(rig.conversations.open_in_terminal(&id, true, &rig.panes), Err(HandOverError::AlreadyInTerminal));
    assert_eq!(rig.panes.started.lock().len(), 1, "no second pane was started");
    // An entry point that resumes the session by itself can ask who has it.
    let session_id = rig.conversations.record(&id).unwrap().session_id.unwrap();
    let (conversation, writer) = rig.conversations.writer_of_session("fake", &session_id).unwrap();
    assert_eq!((conversation.as_str(), writer), (id.as_str(), Writer::Terminal { pane_id: Some(handed.pane_id) }));
}

#[test]
fn a_turn_in_the_chat_is_a_reason_to_ask_and_force_stops_it() {
    let rig = Rig::new("");
    let id = rig.new_started();
    let session = rig.conversations.session(&id).unwrap();
    session.prompt_text("slow").unwrap();
    assert!(session.wait_for(|s| s.turn_in_flight(), Duration::from_secs(10)).is_some());

    let err = rig.conversations.open_in_terminal(&id, false, &rig.panes).unwrap_err();
    assert_eq!(err, HandOverError::TurnRunning { waiting_for_answer: false });
    assert!(session.is_live(), "asking changed nothing");
    assert_eq!(rig.conversations.writer(&id), Some(Writer::Chat));
    assert!(rig.panes.started.lock().is_empty());

    rig.conversations.open_in_terminal(&id, true, &rig.panes).unwrap();
    assert!(!session.snapshot().turn_in_flight());
    assert!(!session.is_live());
    assert!(rig.conversations.writer(&id).unwrap().is_terminal());
}

#[test]
fn a_question_waiting_for_an_answer_is_reported_as_such() {
    let rig = Rig::new("");
    let id = rig.new_started();
    let session = rig.conversations.session(&id).unwrap();
    session.prompt_text("perm").unwrap();
    assert!(session.wait_for(|s| !s.permission_waits.is_empty(), Duration::from_secs(10)).is_some());
    assert_eq!(rig.conversations.open_in_terminal(&id, false, &rig.panes), Err(HandOverError::TurnRunning { waiting_for_answer: true }));
    session.cancel().unwrap();
}

#[test]
fn a_pane_that_will_not_start_gives_the_conversation_back_to_the_chat() {
    let rig = Rig::new("");
    let id = rig.new_started();
    *rig.panes.fail_start.lock() = true;
    let err = rig.conversations.open_in_terminal(&id, false, &rig.panes).unwrap_err();
    assert!(matches!(err, HandOverError::StartFailed(_)));
    assert_eq!(rig.conversations.writer(&id), Some(Writer::Chat), "one writer, as before");
    assert_eq!(rig.conversations.record(&id).unwrap().last_known_state, LastState::Live);
    let session = rig.conversations.session(&id).unwrap();
    assert!(session.held_by().is_none());
    session.prompt_text("stream 1").unwrap();
    session.wait_for(|s| s.turn >= 2 && !s.turn_in_flight(), Duration::from_secs(30)).expect("the chat works again");
}

#[test]
fn no_resume_command_or_no_session_hands_nothing_over() {
    let dir = std::env::temp_dir().join(format!("pm-acp-conv-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut cfg = fake_agent::agent_config("", &dir.join("s"));
    cfg.resume_command = Vec::new();
    let host: Arc<dyn HostEnv> = Arc::new(TestHost { root: dir.clone() });
    let agents = Arc::new(Agents(BTreeMap::from([("fake".to_string(), Arc::new(cfg))])));
    let app = Conversations::new(RecordStore::at(dir.join("c.json")), Arc::new(Supervisor::new(SupervisorConfig::default())), host, agents);
    let panes = FakePanes::default();
    let id = app.create("fake", &dir).unwrap().id;
    assert_eq!(app.open_in_terminal(&id, false, &panes), Err(HandOverError::NoSessionYet), "nothing has been said yet");
    let session = app.session(&id).unwrap();
    session.prompt_text("stream 1").unwrap();
    wait_idle(&session);
    assert_eq!(app.open_in_terminal(&id, false, &panes), Err(HandOverError::NoResumeCommand));
    assert!(session.is_live(), "the adapter was not touched");
    assert_eq!(app.writer(&id), Some(Writer::Chat));
    session.close();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn back_to_chat_stops_the_terminal_and_replays_the_conversation() {
    let rig = Rig::new("");
    let id = rig.new_started();
    let session_id = rig.conversations.record(&id).unwrap().session_id.unwrap();
    let session = rig.conversations.session(&id).unwrap();
    let log = events_of(&session);
    let handed = rig.conversations.open_in_terminal(&id, false, &rig.panes).unwrap();
    let mark = log.lock().len();

    let back = rig.conversations.back_to_chat(&id, false, &rig.panes).unwrap();
    assert_eq!(back.how, "loaded");
    assert_eq!(back.session_id.as_deref(), Some(session_id.as_str()), "the same conversation");
    assert_eq!(rig.panes.activity(&handed.pane_id), PaneActivity::Gone, "the terminal is stopped");
    assert_eq!(rig.conversations.writer(&id), Some(Writer::Chat));
    assert!(session.is_live());
    assert!(log.lock().iter().skip(mark).any(|e| matches!(e, SessionEvent::Update { replay: true, .. })), "session/load replayed it");
    let record = rig.conversations.record(&id).unwrap();
    assert_eq!((record.last_known_state, record.terminal_pane), (LastState::Live, None));
    assert!(record.notices.iter().any(|n| n.kind == NoticeKind::BackInChat));

    session.prompt_text("stream 1").unwrap();
    session.wait_for(|s| s.turn >= 2 && !s.turn_in_flight(), Duration::from_secs(30)).expect("the chat works again");
    // And it can go round again.
    rig.conversations.open_in_terminal(&id, false, &rig.panes).unwrap();
    assert_eq!(rig.panes.started.lock().len(), 2);
}

#[test]
fn a_terminal_that_will_not_stop_keeps_the_conversation() {
    let rig = Rig::new("");
    let id = rig.new_started();
    let handed = rig.conversations.open_in_terminal(&id, false, &rig.panes).unwrap();
    *rig.panes.fail_stop.lock() = true;
    let err = rig.conversations.back_to_chat(&id, false, &rig.panes).unwrap_err();
    assert!(matches!(err, HandOverError::StopFailed(_)));
    assert_eq!(rig.conversations.writer(&id), Some(Writer::Terminal { pane_id: Some(handed.pane_id) }));
    assert!(!rig.conversations.session(&id).unwrap().is_live(), "no adapter next to a running terminal");
}

#[test]
fn a_terminal_that_is_working_is_a_reason_to_ask() {
    let rig = Rig::new("");
    let id = rig.new_started();
    let handed = rig.conversations.open_in_terminal(&id, false, &rig.panes).unwrap();
    rig.panes.set(&handed.pane_id, PaneActivity::Working);
    assert_eq!(rig.conversations.back_to_chat(&id, false, &rig.panes), Err(HandOverError::TerminalBusy { activity: PaneActivity::Working }));
    assert_eq!(rig.panes.activity(&handed.pane_id), PaneActivity::Working, "asking changed nothing");
    rig.conversations.back_to_chat(&id, true, &rig.panes).unwrap();
    assert_eq!(rig.panes.activity(&handed.pane_id), PaneActivity::Gone);
}

#[test]
fn back_to_chat_without_a_terminal_is_refused() {
    let rig = Rig::new("");
    let id = rig.new_started();
    assert_eq!(rig.conversations.back_to_chat(&id, false, &rig.panes), Err(HandOverError::NotInTerminal));
}

#[test]
fn a_terminal_closed_by_the_person_gives_the_chat_the_conversation_again() {
    let rig = Rig::new("");
    let id = rig.new_started();
    let handed = rig.conversations.open_in_terminal(&id, false, &rig.panes).unwrap();
    assert_eq!(rig.conversations.terminal_closed("some-other-pane"), None);
    assert_eq!(rig.conversations.terminal_closed(&handed.pane_id), Some(id.clone()));
    assert_eq!(rig.conversations.writer(&id), Some(Writer::Chat));
    let session = rig.conversations.session(&id).unwrap();
    assert!(session.held_by().is_none() && !session.is_live(), "lazy: the adapter starts with the next use");
    assert_eq!(rig.conversations.open_chat(&id).unwrap(), Readiness::Started(StartKind::Loaded));
}

#[test]
fn after_a_restart_a_conversation_in_a_surviving_terminal_stays_there_and_one_without_comes_back() {
    let rig = Rig::new("");
    let id = rig.new_started();
    let handed = rig.conversations.open_in_terminal(&id, false, &rig.panes).unwrap();
    rig.conversations.session(&id).unwrap().close();

    // The pane survived the restart.
    let app = rig.restarted();
    let report = app.restore_lazily(&rig.panes);
    assert_eq!(report.in_terminal, vec![id.clone()]);
    assert_eq!(app.writer(&id), Some(Writer::Terminal { pane_id: Some(handed.pane_id.clone()) }));
    assert!(matches!(app.open_chat(&id), Err(ConversationError::Chat(AcpError::Held(_)))));
    app.session(&id).unwrap().close();

    // The pane did not.
    rig.panes.stop(&handed.pane_id).unwrap();
    let app = rig.restarted();
    let report = app.restore_lazily(&rig.panes);
    assert_eq!(report.restored, vec![id.clone()]);
    assert_eq!(app.writer(&id), Some(Writer::Chat));
    let record = app.record(&id).unwrap();
    assert_eq!((record.last_known_state, record.terminal_pane), (LastState::Live, None));
    assert_eq!(app.open_chat(&id).unwrap(), Readiness::Started(StartKind::Loaded));
    app.session(&id).unwrap().close();
}
