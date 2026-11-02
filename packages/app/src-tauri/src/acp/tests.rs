//! End-to-end tests of the ACP lane against the fake agent (`fake_agent.rs`): real processes, real pipes,
//! real Job Objects. No real agent is started.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::config::{AdapterPin, AgentConfig, HistoryLostPolicy, HostEnv, PinPolicy, StartRetry};
use super::conn::{AcpConnection, ConnEvent, ConnHandler, ConnLimits, ExitCause, SpawnSpec};
use super::error::AcpError;
use super::fake_agent;
use super::session::{AcpSession, Readiness, Resolution, SessionEvent, SessionParams, StartKind, TickAction};
use super::state::{ExitKind, Phase, StopKind, TurnSummary};
use super::supervisor::{Supervisor, SupervisorConfig};
use crate::proc_util::pid_alive;
use crate::pty::status::PaneStatus;

// ---- rig ---------------------------------------------------------------------------------------------

struct TestHost {
    root: PathBuf,
}

impl HostEnv for TestHost {
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

struct Rig {
    session: Arc<AcpSession>,
    log: Arc<Mutex<Vec<SessionEvent>>>,
    dir: PathBuf,
}

impl Rig {
    fn new(opts: &str) -> Rig {
        Rig::with(opts, |_| {})
    }

    fn with(opts: &str, tweak: impl FnOnce(&mut AgentConfig)) -> Rig {
        let dir = std::env::temp_dir().join(format!("pm-acp-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut cfg = fake_agent::agent_config(opts, &dir.join("store"));
        tweak(&mut cfg);
        let host: Arc<dyn HostEnv> = Arc::new(TestHost { root: dir.clone() });
        let session = AcpSession::new(Arc::new(cfg), host, SessionParams { key: format!("t-{}", uuid::Uuid::new_v4()), cwd: dir.clone(), resume_session_id: None });
        let log = Arc::new(Mutex::new(Vec::new()));
        let sink_log = log.clone();
        session.add_sink(Arc::new(move |event| sink_log.lock().push(event.clone())));
        Rig { session, log, dir }
    }

    fn events(&self) -> Vec<SessionEvent> {
        self.log.lock().clone()
    }

    fn mark(&self) -> usize {
        self.log.lock().len()
    }

    /// First event at or after `from` that satisfies `pred`.
    fn wait_event(&self, from: usize, pred: impl Fn(&SessionEvent) -> bool, secs: u64) -> Option<(usize, SessionEvent)> {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            {
                let log = self.log.lock();
                if let Some((at, event)) = log.iter().enumerate().skip(from).find(|(_, e)| pred(e)) {
                    return Some((at, event.clone()));
                }
            }
            if Instant::now() > deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_turn_end(&self, from: usize) -> TurnSummary {
        match self.wait_event(from, |e| matches!(e, SessionEvent::TurnEnded(_)), 30) {
            Some((_, SessionEvent::TurnEnded(summary))) => summary,
            _ => panic!("the turn did not end; events: {:#?}", self.events().iter().map(brief).collect::<Vec<_>>()),
        }
    }

    fn run_turn(&self, text: &str) -> TurnSummary {
        let from = self.mark();
        self.session.prompt_text(text).unwrap_or_else(|e| panic!("prompt {text:?} failed: {e}"));
        self.wait_turn_end(from)
    }

    fn text_since(&self, from: usize) -> String {
        self.events()
            .iter()
            .skip(from)
            .filter_map(|e| match e {
                SessionEvent::Update { update, replay: false } => match serde_json::to_value(update.as_ref()) {
                    Ok(v) if v["sessionUpdate"] == "agent_message_chunk" => v["content"]["text"].as_str().map(str::to_string),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    fn pid(&self) -> u32 {
        self.session.connection().expect("a live connection").pid()
    }

    fn wait_phase(&self, phase: Phase) -> super::state::Snapshot {
        self.session
            .wait_for(|s| s.phase == phase, Duration::from_secs(20))
            .unwrap_or_else(|| panic!("never reached {phase:?}; now {}", self.session.snapshot()))
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.session.close();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn brief(event: &SessionEvent) -> String {
    match event {
        SessionEvent::Update { update, replay } => format!("Update(replay={replay}) {}", serde_json::to_value(update.as_ref()).map(|v| v["sessionUpdate"].to_string()).unwrap_or_default()),
        SessionEvent::Snapshot(s) => format!("Snapshot({s})"),
        other => format!("{other:?}").chars().take(120).collect(),
    }
}

fn wait_until(secs: u64, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    condition()
}

// ---- handshake and streaming -------------------------------------------------------------------------

#[test]
fn the_handshake_opens_a_session_and_reads_what_the_agent_can_do() {
    let rig = Rig::new("");
    assert_eq!(rig.session.snapshot().phase, Phase::Dormant);
    assert_eq!(rig.session.ensure_running().unwrap(), Readiness::Started(StartKind::New));
    let snap = rig.session.snapshot();
    assert_eq!(snap.phase, Phase::Idle);
    assert_eq!(snap.epoch, 1);
    assert!(snap.session_id.as_deref().is_some_and(|id| id.starts_with("fake-")));
    assert_eq!(snap.config_options.len(), 1);
    let caps = rig.session.caps().unwrap();
    assert!(caps.load_session && caps.resume);
    assert_eq!(caps.agent_version.as_deref(), Some("0.0.1"));
    assert_eq!(caps.auth_methods[0].id, "login");
    // The agent's command list arrives after the session answer.
    let snap = rig.session.wait_for(|s| !s.commands.is_empty(), Duration::from_secs(10)).expect("commands");
    assert_eq!(snap.commands[0].name, "review");
    assert_eq!(snap.commands[0].hint.as_deref(), Some("branch"));
    // The test harness printed a header on stdout before the first message: ignored, but counted.
    assert!(rig.session.connection().unwrap().stats().bad_lines >= 1);
    // Already running: nothing happens.
    assert_eq!(rig.session.ensure_running().unwrap(), Readiness::AlreadyRunning);
}

#[test]
fn the_adapter_runs_hidden_inside_a_job_object_on_windows() {
    let rig = Rig::new("");
    rig.session.ensure_running().unwrap();
    let conn = rig.session.connection().unwrap();
    if cfg!(windows) {
        assert_eq!(conn.mechanism(), super::job::Mechanism::JobObject);
        assert!(conn.live_processes().is_some_and(|n| n >= 1), "{:?}", conn.live_processes());
    }
    assert!(pid_alive(conn.pid()));
}

#[test]
fn a_streamed_turn_arrives_in_order_and_the_stop_reason_comes_after_the_last_chunk() {
    let rig = Rig::new("");
    for round in 0..4 {
        let from = rig.mark();
        let summary = rig.run_turn("stream 200");
        assert_eq!(summary.stop, StopKind::EndTurn, "round {round}");
        assert!(!summary.empty);
        let expected: String = (0..200).map(|n| format!("w{n} ")).collect();
        assert_eq!(rig.text_since(from), expected, "round {round}");
        // The end of the turn is the last thing that happened for the turn: no chunk after it.
        let events = rig.events();
        let end = events.iter().rposition(|e| matches!(e, SessionEvent::TurnEnded(_))).unwrap();
        assert!(events[end..].iter().all(|e| !matches!(e, SessionEvent::Update { .. })), "round {round}");
    }
    // 800 chunks, but the viewer-visible state changed only at the start and the end of each turn.
    let snapshots = rig.events().iter().filter(|e| matches!(e, SessionEvent::Snapshot(_))).count();
    assert!(snapshots < 40, "{snapshots} snapshot events");
}

#[test]
fn a_tool_that_runs_shows_in_the_snapshot_and_is_gone_when_it_finishes() {
    let rig = Rig::new("");
    rig.run_turn("tool");
    let saw_running = rig.events().iter().any(|e| matches!(e, SessionEvent::Snapshot(s) if s.running_tools.len() == 1 && s.running_tools[0].kind == "execute"));
    assert!(saw_running, "events: {:#?}", rig.events().iter().map(brief).collect::<Vec<_>>());
    assert!(rig.session.snapshot().running_tools.is_empty());
}

#[test]
fn an_empty_turn_is_flagged() {
    let rig = Rig::new("");
    let summary = rig.run_turn("empty");
    assert_eq!(summary.stop, StopKind::EndTurn);
    assert!(summary.empty);
}

#[test]
fn update_kinds_this_build_does_not_know_are_kept_raw_and_the_turn_still_ends() {
    let rig = Rig::new("");
    let summary = rig.run_turn("weird");
    assert_eq!(summary.stop, StopKind::EndTurn);
    let raw = rig.events().into_iter().find_map(|e| match e {
        SessionEvent::UpdateUnparsed { kind, raw, .. } => Some((kind, raw)),
        _ => None,
    });
    let (kind, raw) = raw.expect("an unparsed update");
    assert_eq!(kind, "future_thing");
    assert_eq!(raw["update"]["x"], json!(1));
    assert_eq!(rig.session.stats().unparsed_updates, 1);
}

#[test]
fn a_second_prompt_while_a_turn_runs_is_refused() {
    let rig = Rig::new("");
    let from = rig.mark();
    rig.session.prompt_text("slow").unwrap();
    assert!(matches!(rig.session.prompt_text("stream 1"), Err(AcpError::Busy)));
    rig.session.cancel().unwrap();
    rig.wait_turn_end(from);
}

#[test]
fn a_slow_consumer_slows_the_agent_instead_of_growing_the_queue() {
    let rig = Rig::new("");
    rig.session.ensure_running().unwrap();
    // A sink that takes a moment per event.
    let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = seen.clone();
    rig.session.add_sink(Arc::new(move |event| {
        if matches!(event, SessionEvent::Update { .. }) {
            let start = Instant::now();
            while start.elapsed() < Duration::from_micros(200) {}
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }));
    let from = rig.mark();
    let summary = rig.run_turn("flood 6000");
    assert_eq!(summary.stop, StopKind::EndTurn);
    let expected: String = (0..6000).map(|n| format!("{n},")).collect();
    assert_eq!(rig.text_since(from), expected, "nothing lost, nothing reordered");
    let stats = rig.session.connection().unwrap().stats();
    assert!(stats.max_queued_events <= 1030, "the queue grew to {}", stats.max_queued_events);
    assert!(stats.max_queued_events > 100, "the queue never filled ({}): the test did not exercise backpressure", stats.max_queued_events);
}

// ---- permissions -------------------------------------------------------------------------------------

#[test]
fn a_permission_question_is_parked_without_a_timeout_and_never_answered_by_the_app() {
    let rig = Rig::with("", |cfg| {
        cfg.timeouts.inactivity_secs = 1;
        cfg.timeouts.progress_secs = 1;
        cfg.timeouts.hung_grace_secs = 1;
    });
    let from = rig.mark();
    rig.session.prompt_text("perm").unwrap();
    let snap = rig.wait_phase(Phase::NeedsInput);
    assert_eq!(snap.pane_status(), PaneStatus::WaitingInput);
    assert_eq!(snap.permission_waits.len(), 1);
    let wait = &snap.permission_waits[0];
    assert_eq!(wait.title.as_deref(), Some("Write notes.txt"));
    assert_eq!(wait.options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), vec!["allow_once", "allow_always", "reject_once"]);
    assert_eq!(wait.options[1].kind, "allow_always");

    // Far longer than every window: still waiting, still nothing decided, the turn clock does not run.
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(rig.session.tick(Instant::now() + Duration::from_secs(3600)), TickAction::Nothing);
    assert_eq!(rig.session.tick(Instant::now() + Duration::from_secs(86_400)), TickAction::Nothing);
    let still = rig.session.snapshot();
    assert_eq!(still.phase, Phase::NeedsInput);
    assert!(rig.events().iter().skip(from).all(|e| !matches!(e, SessionEvent::PermissionResolved { .. })));

    // A late viewer sees the same open question.
    assert_eq!(rig.session.pending_permissions().len(), 1);

    // An option that was not offered is refused and nothing changes.
    let ask_id = wait.ask_id;
    assert!(matches!(rig.session.answer_permission(ask_id, "allow_forever"), Err(AcpError::BadOption(_))));
    assert!(matches!(rig.session.answer_permission(ask_id + 100, "allow_once"), Err(AcpError::NoSuchAsk)));
    assert_eq!(rig.session.snapshot().phase, Phase::NeedsInput);

    rig.session.answer_permission(ask_id, "allow_once").unwrap();
    let summary = rig.wait_turn_end(from);
    assert_eq!(summary.stop, StopKind::EndTurn);
    assert_eq!(rig.text_since(from), "chose:allow_once");
    assert!(rig.events().iter().any(|e| matches!(e, SessionEvent::PermissionResolved { how: Resolution::Selected(id), .. } if id == "allow_once")));
    // Answering again finds nothing.
    assert!(matches!(rig.session.answer_permission(ask_id, "allow_once"), Err(AcpError::NoSuchAsk)));
}

#[test]
fn rejecting_is_a_normal_answer_the_agent_continues_from() {
    let rig = Rig::new("");
    let from = rig.mark();
    rig.session.prompt_text("perm").unwrap();
    let snap = rig.wait_phase(Phase::NeedsInput);
    rig.session.answer_permission(snap.permission_waits[0].ask_id, "reject_once").unwrap();
    let summary = rig.wait_turn_end(from);
    assert_eq!(summary.stop, StopKind::EndTurn);
    assert_eq!(rig.text_since(from), "chose:reject_once");
}

#[test]
fn cancelling_with_a_question_open_answers_it_cancelled() {
    let rig = Rig::new("");
    let from = rig.mark();
    rig.session.prompt_text("perm").unwrap();
    rig.wait_phase(Phase::NeedsInput);
    assert!(rig.session.cancel().unwrap());
    let summary = rig.wait_turn_end(from);
    assert_eq!(summary.stop, StopKind::Cancelled);
    assert!(summary.cancel_requested);
    assert!(rig.events().iter().any(|e| matches!(e, SessionEvent::PermissionResolved { how: Resolution::Cancelled, .. })));
    assert!(rig.session.pending_permissions().is_empty());
    assert_eq!(rig.session.snapshot().phase, Phase::Idle);
}

#[test]
fn a_cancel_sent_right_behind_its_prompt_is_not_lost() {
    // The cancel travels on the priority lane, so it can reach the agent before the prompt it is meant
    // for. The repeats make sure the turn is still stopped.
    let rig = Rig::new("");
    rig.session.ensure_running().unwrap();
    for round in 0..6 {
        let from = rig.mark();
        let started = Instant::now();
        rig.session.prompt_text("quiet").unwrap();
        assert!(rig.session.cancel().unwrap());
        let summary = rig.wait_turn_end(from);
        assert_eq!(summary.stop, StopKind::Cancelled, "round {round}");
        assert!(started.elapsed() < Duration::from_secs(5), "round {round}: {:?}", started.elapsed());
    }
}

#[test]
fn a_cancel_with_no_turn_is_a_no_op() {
    let rig = Rig::new("");
    rig.session.ensure_running().unwrap();
    assert!(!rig.session.cancel().unwrap());
}

#[test]
fn cancelling_a_running_turn_ends_it_quickly_as_cancelled() {
    let rig = Rig::new("");
    let from = rig.mark();
    rig.session.prompt_text("slow").unwrap();
    assert!(rig.wait_event(from, |e| matches!(e, SessionEvent::Update { .. }), 10).is_some());
    let asked = Instant::now();
    assert!(rig.session.cancel().unwrap());
    assert_eq!(rig.session.snapshot().phase, Phase::Cancelling);
    let summary = rig.wait_turn_end(from);
    assert!(asked.elapsed() < Duration::from_secs(3), "{:?}", asked.elapsed());
    assert_eq!(summary.stop, StopKind::Cancelled);
    assert_eq!(summary.stop_reason.as_deref(), Some("cancelled"));
    assert_eq!(rig.session.snapshot().phase, Phase::Idle);
    // The next turn works.
    assert_eq!(rig.run_turn("stream 2").stop, StopKind::EndTurn);
}

// ---- crashes, restore, history -----------------------------------------------------------------------

#[test]
fn a_crash_mid_turn_ends_the_turn_and_the_next_prompt_brings_the_session_back() {
    let rig = Rig::new("");
    rig.run_turn("stream 1");
    let before = rig.session.snapshot();
    let old_pid = rig.pid();

    let from = rig.mark();
    rig.session.prompt_text("crash").unwrap();
    let summary = rig.wait_turn_end(from);
    assert_eq!(summary.stop, StopKind::Failed);
    let snap = rig.wait_phase(Phase::Exited);
    let exit = snap.exit.clone().unwrap();
    assert_eq!(exit.kind, ExitKind::Crashed);
    assert_eq!(exit.code, Some(3));
    assert!(exit.stderr_tail.iter().any(|l| l.contains("boom")), "{:?}", exit.stderr_tail);
    assert_eq!(snap.pane_status(), PaneStatus::Error);
    assert!(wait_until(10, || !pid_alive(old_pid)), "the crashed adapter is gone");
    assert!(rig.text_since(from).contains("partial"), "text sent before the crash is kept");

    let from = rig.mark();
    rig.session.prompt_text("stream 2").unwrap();
    let started = rig.wait_event(from, |e| matches!(e, SessionEvent::Started { .. }), 20).expect("restarted");
    assert!(matches!(started.1, SessionEvent::Started { how: StartKind::Resumed, .. }));
    rig.wait_turn_end(from);
    let after = rig.session.snapshot();
    assert_eq!(after.session_id, before.session_id, "same conversation");
    assert_eq!(after.epoch, before.epoch + 1, "a new adapter process");
    assert_eq!(after.phase, Phase::Idle);
    assert_eq!(rig.text_since(from), "w0 w1 ");
    assert_ne!(rig.pid(), old_pid);
}

#[test]
fn load_is_used_when_there_is_no_resume_and_replayed_history_is_not_live_work() {
    let rig = Rig::new("resume=0");
    rig.run_turn("stream 1");
    let id = rig.session.snapshot().session_id.clone();
    assert!(rig.session.sleep().unwrap());
    assert_eq!(rig.session.snapshot().phase, Phase::Dormant);

    let from = rig.mark();
    rig.session.prompt_text("stream 1").unwrap();
    let started = rig.wait_event(from, |e| matches!(e, SessionEvent::Started { .. }), 20).expect("restarted");
    assert!(matches!(started.1, SessionEvent::Started { how: StartKind::Loaded, .. }));
    rig.wait_turn_end(from);
    assert_eq!(rig.session.snapshot().session_id, id);
    let events = rig.events();
    let replayed: Vec<&SessionEvent> = events.iter().skip(from).filter(|e| matches!(e, SessionEvent::Update { replay: true, .. })).collect();
    assert_eq!(replayed.len(), 3, "user, agent, tool from the history");
    // The replayed tool is old news: never shown as running.
    assert!(events.iter().skip(from).all(|e| !matches!(e, SessionEvent::Snapshot(s) if s.running_tools.iter().any(|t| t.id == "old-tool"))));
    assert_eq!(rig.text_since(from), "w0 ", "only the live answer counts as text");
}

#[test]
fn history_lost_is_signalled_when_neither_resume_nor_load_works() {
    let rig = Rig::new("lose=1");
    rig.run_turn("stream 1");
    let old = rig.session.snapshot().session_id.clone().unwrap();
    rig.session.sleep().unwrap();

    let from = rig.mark();
    rig.session.prompt_text("stream 1").unwrap();
    let lost = rig.wait_event(from, |e| matches!(e, SessionEvent::HistoryLost(_)), 20).expect("history lost event");
    let SessionEvent::HistoryLost(lost) = lost.1 else { unreachable!() };
    assert_eq!(lost.old_session_id, old);
    assert_ne!(lost.new_session_id, old);
    assert!(lost.reason.contains("not found"), "{}", lost.reason);
    let summary = rig.wait_turn_end(from);
    assert_eq!(summary.stop, StopKind::EndTurn, "the default policy carries on in the fresh session");
    let snap = rig.session.snapshot();
    assert_eq!(snap.session_id.as_deref(), Some(lost.new_session_id.as_str()));
    assert_eq!(snap.history_lost.as_ref().map(|h| h.old_session_id.as_str()), Some(old.as_str()));
    rig.session.acknowledge_history_lost();
    assert!(rig.session.snapshot().history_lost.is_none());
}

#[test]
fn the_refuse_policy_holds_the_prompt_until_the_person_decides() {
    let rig = Rig::with("lose=1", |cfg| cfg.history_lost = HistoryLostPolicy::Refuse);
    rig.run_turn("stream 1");
    rig.session.sleep().unwrap();
    assert!(matches!(rig.session.prompt_text("stream 1"), Err(AcpError::HistoryLost)));
    let snap = rig.session.snapshot();
    assert_eq!(snap.phase, Phase::Idle, "a fresh session is ready");
    assert!(snap.history_lost.is_some());
    assert_eq!(snap.turn, 1, "no turn was started");
    // Asked again (the person said go): it goes.
    assert_eq!(rig.run_turn("stream 1").stop, StopKind::EndTurn);
}

#[test]
fn a_session_id_given_at_creation_is_restored_by_the_first_start() {
    let first = Rig::new("");
    first.run_turn("stream 1");
    let id = first.session.snapshot().session_id.clone().unwrap();

    // A second session object (an app restart) with the id and the same agent's session store.
    let dir = std::env::temp_dir().join(format!("pm-acp-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = fake_agent::agent_config("", &first.dir.join("store"));
    let host: Arc<dyn HostEnv> = Arc::new(TestHost { root: dir.clone() });
    let second = AcpSession::new(Arc::new(cfg), host, SessionParams { key: "t2".into(), cwd: dir.clone(), resume_session_id: Some(id.clone()) });
    assert_eq!(second.snapshot().session_id.as_deref(), Some(id.as_str()));
    assert_eq!(second.ensure_running().unwrap(), Readiness::Started(StartKind::Resumed));
    assert_eq!(second.snapshot().session_id.as_deref(), Some(id.as_str()));
    second.close();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_sign_in_error_is_reported_with_the_agents_methods_and_leaves_no_process() {
    let rig = Rig::new("auth=1");
    let error = rig.session.ensure_running().unwrap_err();
    match error {
        AcpError::AuthRequired { methods } => assert_eq!(methods, vec!["login"]),
        other => panic!("{other:?}"),
    }
    let snap = rig.session.snapshot();
    assert_eq!(snap.phase, Phase::Exited);
    assert_eq!(snap.exit.as_ref().unwrap().kind, ExitKind::StartFailed);
    assert!(rig.session.connection().is_none());
}

#[test]
fn a_start_that_fails_with_a_known_message_is_retried() {
    let rig = Rig::with("fail_init_once=1", |cfg| {
        cfg.start_retry = Some(StartRetry { patterns: vec!["sqlite state runtime".into()], delays_ms: vec![100, 100] });
    });
    assert_eq!(rig.session.ensure_running().unwrap(), Readiness::Started(StartKind::New));
    assert_eq!(rig.session.stats().starts, 2);
}

#[test]
fn the_same_failure_without_a_retry_rule_is_an_error() {
    let rig = Rig::new("fail_init_once=1");
    assert!(rig.session.ensure_running().is_err());
    assert_eq!(rig.session.stats().starts, 1);
}

#[test]
fn an_adapter_that_is_not_the_pinned_one_is_never_started() {
    let rig = Rig::with("", |cfg| {
        cfg.pin_policy = PinPolicy::Enforce;
        cfg.pins = vec![AdapterPin { package: "x".into(), version: "1.0.0".into(), entry: "{adapters}/missing/index.js".into(), sha256: None }];
    });
    assert!(matches!(rig.session.ensure_running(), Err(AcpError::Pin(_))));
    assert_eq!(rig.session.stats().starts, 0, "no process was started");
    assert!(rig.session.connection().is_none());
}

// ---- agent requests and ids --------------------------------------------------------------------------

#[test]
fn requests_for_features_we_did_not_offer_get_method_not_found_and_ids_never_collide() {
    let rig = Rig::new("");
    let from = rig.mark();
    // The fake asks to read a file using the very id of the prompt in flight, after sending a stray response.
    let summary = rig.run_turn("fsreq");
    assert_eq!(summary.stop, StopKind::EndTurn, "the prompt was answered by the agent, not by the stray request");
    assert_eq!(rig.text_since(from), "fs-error:-32601");
    assert_eq!(rig.session.stats().unsupported_requests, 1);
    assert_eq!(rig.session.connection().unwrap().stats().stale_responses, 1, "the unknown response id was dropped");
}

// ---- stalls, hangs, idle -----------------------------------------------------------------------------

#[test]
fn an_adapter_that_stops_reading_is_killed_after_the_write_timeout() {
    let rig = Rig::with("deaf_after_new_ms=60000", |cfg| cfg.timeouts.stdin_write_secs = 1);
    rig.session.ensure_running().unwrap();
    let pid = rig.pid();
    let from = rig.mark();
    // A prompt too big for the pipe: the write blocks because nobody reads.
    let started = Instant::now();
    rig.session.prompt_text(&format!("stream {}", "x".repeat(3 * 1024 * 1024))).unwrap();
    let summary = rig.wait_turn_end(from);
    assert_eq!(summary.stop, StopKind::Failed, "{summary:?}");
    assert!(started.elapsed() < Duration::from_secs(15), "{:?}", started.elapsed());
    let snap = rig.wait_phase(Phase::Exited);
    assert_eq!(snap.exit.unwrap().kind, ExitKind::WriteStalled);
    assert!(wait_until(10, || !pid_alive(pid)));
}

#[test]
fn the_write_stall_is_detected_on_a_bare_connection_too() {
    struct Collect(Mutex<Vec<ConnEvent>>);
    impl ConnHandler for Collect {
        fn on_event(&self, event: ConnEvent) {
            self.0.lock().push(event);
        }
    }
    let dir = std::env::temp_dir().join(format!("pm-acp-test-{}", uuid::Uuid::new_v4()));
    let cfg = fake_agent::agent_config("deaf_ms=60000", &dir);
    let host = TestHost { root: dir.clone() };
    let spec: SpawnSpec = super::config::resolve_spawn(&cfg, &host, &std::env::temp_dir()).unwrap();
    let handler = Arc::new(Collect(Mutex::new(Vec::new())));
    let limits = ConnLimits { stdin_write_timeout: Duration::from_millis(800), ..ConnLimits::default() };
    let conn = AcpConnection::spawn(spec, limits, handler.clone()).unwrap();
    let init = conn.request("initialize", super::handshake::initialize_params("t", "0"), Duration::from_secs(20)).unwrap();
    assert_eq!(init["protocolVersion"], json!(1));
    // The agent is now deaf. A waiting request must fail with the stall, not wait for its own timeout.
    let waiting = {
        let conn = conn.clone();
        std::thread::spawn(move || conn.request("session/new", json!({"cwd": "C:\\", "mcpServers": []}), Duration::from_secs(60)))
    };
    std::thread::sleep(Duration::from_millis(100));
    conn.notify("padding", json!({"blob": "y".repeat(3 * 1024 * 1024)})).unwrap();
    let failed = waiting.join().unwrap().unwrap_err();
    assert!(matches!(failed, AcpError::WriteStalled { .. }), "{failed:?}");
    assert!(wait_until(10, || !conn.is_alive()));
    let events = handler.0.lock();
    assert!(matches!(events.last(), Some(ConnEvent::Exited(info)) if info.cause == ExitCause::WriteStalled));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_hung_turn_is_asked_to_stop_and_then_killed() {
    let rig = Rig::with("", |cfg| {
        // The progress window runs out (the quiet window only asks the person).
        cfg.timeouts.progress_secs = 5;
        cfg.timeouts.hung_grace_secs = 5;
    });
    rig.run_turn("stream 1");
    let pid = rig.pid();
    let from = rig.mark();
    rig.session.prompt_text("hang").unwrap();
    rig.wait_phase(Phase::Working);
    let now = Instant::now();
    assert_eq!(rig.session.tick(now), TickAction::Nothing, "not yet");
    assert_eq!(rig.session.tick(now + Duration::from_secs(10)), TickAction::StopAsked);
    assert_eq!(rig.session.snapshot().phase, Phase::Cancelling);
    assert_eq!(rig.session.tick(now + Duration::from_secs(12)), TickAction::Nothing, "still inside the grace period");
    assert_eq!(rig.session.tick(now + Duration::from_secs(20)), TickAction::Killed);
    let summary = rig.wait_turn_end(from);
    assert_eq!(summary.stop, StopKind::Hung);
    let snap = rig.session.snapshot();
    assert_eq!(snap.exit.as_ref().unwrap().kind, ExitKind::Hung);
    assert_eq!(snap.pane_status(), PaneStatus::Error);
    assert!(wait_until(10, || !pid_alive(pid)));
    // And it comes back.
    assert_eq!(rig.run_turn("stream 1").stop, StopKind::EndTurn);
}

#[test]
fn a_quiet_agent_asks_the_person_instead_of_being_stopped_and_keep_waiting_starts_the_window_again() {
    let rig = Rig::with("", |cfg| cfg.timeouts.inactivity_secs = 5);
    rig.run_turn("stream 1");
    let from = rig.mark();
    rig.session.prompt_text("quiet").unwrap();
    rig.wait_phase(Phase::Working);
    let now = Instant::now();
    assert_eq!(rig.session.tick(now), TickAction::Nothing, "not quiet yet");
    assert_eq!(rig.session.tick(now + Duration::from_secs(10)), TickAction::QuietAsked);
    let snap = rig.session.snapshot();
    assert_eq!(snap.quiet_secs, Some(5), "the state says the person is asked");
    assert_eq!(snap.phase, Phase::Working, "nothing was stopped");
    assert_eq!(rig.session.tick(now + Duration::from_secs(60)), TickAction::Nothing, "asked once, then it waits for the person");
    // Keep waiting: the question is over and the window runs again from now.
    assert!(rig.session.keep_waiting());
    assert_eq!(rig.session.snapshot().quiet_secs, None);
    let later = Instant::now() + Duration::from_secs(2);
    assert_eq!(rig.session.tick(later), TickAction::Nothing);
    assert_eq!(rig.session.tick(Instant::now() + Duration::from_secs(10)), TickAction::QuietAsked, "quiet again: asked again");
    // Stop is the other answer.
    assert!(rig.session.cancel().unwrap());
    let summary = rig.wait_turn_end(from);
    assert_eq!(summary.stop, StopKind::Cancelled);
    assert_eq!(rig.session.snapshot().quiet_secs, None);
}

#[test]
fn keep_waiting_with_no_turn_running_does_nothing() {
    let rig = Rig::with("", |_| {});
    assert!(!rig.session.keep_waiting());
}

#[test]
fn an_agent_that_honours_the_stop_request_ends_the_turn_without_being_killed() {
    let rig = Rig::with("", |cfg| cfg.timeouts.progress_secs = 5);
    let from = rig.mark();
    rig.session.prompt_text("quiet").unwrap();
    rig.wait_phase(Phase::Working);
    assert_eq!(rig.session.tick(Instant::now() + Duration::from_secs(10)), TickAction::StopAsked);
    let summary = rig.wait_turn_end(from);
    assert_eq!(summary.stop, StopKind::Cancelled);
    assert!(rig.session.is_live(), "it answered, so it was not killed");
}

#[test]
fn an_idle_session_stops_its_adapter_and_the_next_prompt_brings_it_back() {
    let rig = Rig::with("", |cfg| cfg.timeouts.idle_ttl_secs = 60);
    rig.run_turn("stream 1");
    let pid = rig.pid();
    let id = rig.session.snapshot().session_id.clone();
    assert_eq!(rig.session.tick(Instant::now() + Duration::from_secs(30)), TickAction::Nothing);
    assert_eq!(rig.session.tick(Instant::now() + Duration::from_secs(61)), TickAction::Slept);
    let snap = rig.session.snapshot();
    assert_eq!(snap.phase, Phase::Dormant);
    assert_eq!(snap.pane_status(), PaneStatus::Idle);
    assert!(wait_until(10, || !pid_alive(pid)));
    assert!(rig.session.connection().is_none());

    let from = rig.mark();
    rig.session.prompt_text("stream 1").unwrap();
    let started = rig.wait_event(from, |e| matches!(e, SessionEvent::Started { .. }), 20).unwrap();
    assert!(matches!(started.1, SessionEvent::Started { how: StartKind::Resumed, .. }));
    rig.wait_turn_end(from);
    assert_eq!(rig.session.snapshot().session_id, id);
    assert_eq!(rig.session.snapshot().epoch, 2);
}

#[test]
fn a_session_with_a_turn_or_an_open_question_is_never_put_to_sleep() {
    let rig = Rig::with("", |cfg| cfg.timeouts.idle_ttl_secs = 1);
    let from = rig.mark();
    rig.session.prompt_text("perm").unwrap();
    rig.wait_phase(Phase::NeedsInput);
    assert!(matches!(rig.session.sleep(), Err(AcpError::Busy)));
    assert_eq!(rig.session.tick(Instant::now() + Duration::from_secs(3600)), TickAction::Nothing);
    rig.session.cancel().unwrap();
    rig.wait_turn_end(from);
}

#[test]
fn the_supervisor_stops_the_longest_idle_adapters_over_the_cap_and_spares_a_working_one() {
    let working = Rig::new("");
    let idle_a = Rig::new("");
    let idle_b = Rig::new("");
    let supervisor = Supervisor::new(SupervisorConfig { max_live: 1, sweep_interval: Duration::from_secs(5) });
    idle_a.run_turn("stream 1");
    std::thread::sleep(Duration::from_millis(30));
    idle_b.run_turn("stream 1");
    working.run_turn("stream 1");
    let from = working.mark();
    working.session.prompt_text("slow").unwrap();
    working.wait_phase(Phase::Working);
    supervisor.insert(working.session.clone());
    supervisor.insert(idle_a.session.clone());
    supervisor.insert(idle_b.session.clone());
    assert_eq!(supervisor.live_count(), 3);
    let report = supervisor.sweep(Instant::now());
    assert_eq!(report.evicted.len(), 2, "{report:?}");
    assert!(report.evicted.contains(&idle_a.session.key().to_string()) && report.evicted.contains(&idle_b.session.key().to_string()));
    assert!(working.session.is_live(), "a turn in progress is never evicted");
    assert_eq!(idle_a.session.snapshot().phase, Phase::Dormant);
    assert_eq!(idle_b.session.snapshot().phase, Phase::Dormant);
    working.session.cancel().unwrap();
    working.wait_turn_end(from);
    // A second sweep with room to spare does nothing.
    assert!(supervisor.sweep(Instant::now()).evicted.is_empty());
}

// ---- processes ---------------------------------------------------------------------------------------

#[test]
fn closing_a_session_ends_the_adapter_and_everything_it_started() {
    let rig = Rig::new("grandchild=1");
    rig.session.ensure_running().unwrap();
    let adapter = rig.pid();
    let store = rig.dir.join("store");
    assert!(wait_until(10, || store.join("grandchild.pid").exists()));
    let grandchild: u32 = std::fs::read_to_string(store.join("grandchild.pid")).unwrap().trim().parse().unwrap();
    assert!(pid_alive(grandchild), "the helper the adapter started is running");
    if cfg!(windows) {
        let conn = rig.session.connection().unwrap();
        assert!(wait_until(10, || conn.live_processes().is_some_and(|n| n >= 2)), "both are in the job: {:?}", conn.live_processes());
    }
    rig.session.close();
    assert!(wait_until(15, || !pid_alive(adapter)), "adapter still running");
    assert!(wait_until(15, || !pid_alive(grandchild)), "the adapter's helper outlived it");
    assert_eq!(rig.session.snapshot().pane_status(), PaneStatus::Idle, "closed on purpose is not an error");
    assert!(matches!(rig.session.prompt_text("stream 1"), Err(AcpError::SessionClosed)));
}

#[test]
fn dropping_the_last_reference_to_a_session_kills_the_adapter_and_its_tree() {
    let dir = std::env::temp_dir().join(format!("pm-acp-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = fake_agent::agent_config("grandchild=1", &dir.join("store"));
    let host: Arc<dyn HostEnv> = Arc::new(TestHost { root: dir.clone() });
    let session = AcpSession::new(Arc::new(cfg), host, SessionParams { key: "dropped".into(), cwd: dir.clone(), resume_session_id: None });
    session.ensure_running().unwrap();
    let adapter = session.connection().unwrap().pid();
    let store = dir.join("store");
    assert!(wait_until(10, || store.join("grandchild.pid").exists()));
    let grandchild: u32 = std::fs::read_to_string(store.join("grandchild.pid")).unwrap().trim().parse().unwrap();
    assert!(pid_alive(adapter) && pid_alive(grandchild));
    // Nothing calls close: the last reference goes away, as when the app drops it or exits.
    drop(session);
    assert!(wait_until(15, || !pid_alive(adapter)), "adapter outlived its session");
    assert!(wait_until(15, || !pid_alive(grandchild)), "the adapter's helper outlived its session");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_connection_that_is_dropped_takes_its_process_tree_with_it() {
    struct Quiet;
    impl ConnHandler for Quiet {
        fn on_event(&self, _: ConnEvent) {}
    }
    let dir = std::env::temp_dir().join(format!("pm-acp-test-{}", uuid::Uuid::new_v4()));
    let cfg = fake_agent::agent_config("grandchild=1", &dir.join("store"));
    let host = TestHost { root: dir.clone() };
    let spec = super::config::resolve_spawn(&cfg, &host, &std::env::temp_dir()).unwrap();
    let conn = AcpConnection::spawn(spec, ConnLimits::default(), Arc::new(Quiet)).unwrap();
    conn.request("initialize", super::handshake::initialize_params("t", "0"), Duration::from_secs(20)).unwrap();
    conn.request("session/new", json!({"cwd": "C:\\", "mcpServers": []}), Duration::from_secs(20)).unwrap();
    let adapter = conn.pid();
    let store = dir.join("store");
    assert!(wait_until(10, || store.join("grandchild.pid").exists()));
    let grandchild: u32 = std::fs::read_to_string(store.join("grandchild.pid")).unwrap().trim().parse().unwrap();
    assert!(pid_alive(adapter) && pid_alive(grandchild));
    drop(conn);
    assert!(wait_until(15, || !pid_alive(adapter)), "adapter outlived its connection");
    assert!(wait_until(15, || !pid_alive(grandchild)), "grandchild outlived its connection");
    let _ = std::fs::remove_dir_all(dir);
}

// ---- settings ----------------------------------------------------------------------------------------

#[test]
fn a_setting_changed_over_the_protocol_survives_an_adapter_restart() {
    let rig = Rig::new("");
    rig.session.ensure_running().unwrap();
    rig.session.set_config_option("model", json!("b")).unwrap();
    let current = |snap: &super::state::Snapshot| snap.config_options.iter().find(|o| o["id"] == "model").map(|o| o["currentValue"].clone());
    assert_eq!(current(&rig.session.snapshot()), Some(json!("b")));
    rig.session.sleep().unwrap();
    // The agent forgets it on resume (the fake answers "a"); the session puts it back.
    rig.run_turn("stream 1");
    assert_eq!(current(&rig.session.snapshot()), Some(json!("b")));
}

#[test]
fn a_setting_cannot_be_changed_while_a_turn_runs() {
    let rig = Rig::new("");
    let from = rig.mark();
    rig.session.prompt_text("slow").unwrap();
    assert!(matches!(rig.session.set_config_option("model", json!("b")), Err(AcpError::Busy)));
    rig.session.cancel().unwrap();
    rig.wait_turn_end(from);
}

#[test]
fn restore_order_follows_the_agents_preference_and_what_it_can_do() {
    use super::config::ResumePreference::{PreferLoad, PreferResume};
    use super::handshake::AgentCaps;
    use super::session::restore_order_for_tests as order;
    let both = AgentCaps { load_session: true, resume: true, ..AgentCaps::default() };
    assert_eq!(order(PreferResume, &both), vec!["resume", "load"]);
    assert_eq!(order(PreferLoad, &both), vec!["load", "resume"]);
    let load_only = AgentCaps { load_session: true, ..AgentCaps::default() };
    assert_eq!(order(PreferResume, &load_only), vec!["load"]);
    assert!(order(PreferResume, &AgentCaps::default()).is_empty());
}

#[test]
fn only_slow_or_dead_adapters_count_as_transient_failures() {
    assert!(AcpError::Timeout { method: "m".into(), secs: 1 }.is_transient());
    assert!(AcpError::Closed.is_transient());
    assert!(AcpError::WriteStalled { secs: 1 }.is_transient());
    assert!(!AcpError::Rpc { code: -32002, message: "Session not found".into(), data: None }.is_transient());
    assert!(!AcpError::AuthRequired { methods: vec![] }.is_transient());
    let _: Value = json!(null);
}
