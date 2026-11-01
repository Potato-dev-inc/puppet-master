//! Tests of the bridge with a fake headless agent: the unit-test executable starts itself again as the agent
//! (the one test `child_entry`, chosen by `PM_BRIDGE_ROLE`), like `acp/fake_agent.rs` does. No real agent runs.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::*;
use crate::proc_util::pid_alive;
use crate::tui::{Features, HeadlessLaunch, LaunchSpec, TuiProfile};

const ROLE: &str = "PM_BRIDGE_ROLE";
const TASK: &str = "PM_BRIDGE_TASK";
const RESUME: &str = "PM_BRIDGE_RESUME";
const DIR: &str = "PM_BRIDGE_DIR";
const MEMORY: &str = "PM_BRIDGE_MEMORY";
const HARNESS: [&str; 4] = ["--exact", "acp::bridge::tests::child_entry", "--nocapture", "--test-threads=1"];
const SECRET: &str = "sk-abcdefghijk123";

/// The agent under test: a headless command that re-runs this test exe in the `headless` role.
struct FakeProfile {
    memory: bool,
    headless: bool,
    dir: PathBuf,
}

impl TuiProfile for FakeProfile {
    fn id(&self) -> &'static str {
        "bridgefake"
    }

    fn features(&self) -> Features {
        Features { resumable_headless: self.memory, ..Features::CLI }
    }

    fn headless_command(&self, launch: &HeadlessLaunch) -> Result<LaunchSpec, String> {
        if !self.headless {
            return Err("headless launch is unsupported for agent type: bridgefake".into());
        }
        let mut env = vec![
            (ROLE.to_string(), "headless".to_string()),
            (TASK.to_string(), launch.task.to_string()),
            (DIR.to_string(), self.dir.to_string_lossy().into_owned()),
        ];
        if let Some(resume) = launch.resume_session {
            env.push((RESUME.to_string(), resume.to_string()));
        }
        Ok(LaunchSpec {
            program: std::env::current_exe().unwrap().to_string_lossy().into_owned(),
            args: HARNESS.iter().map(|a| a.to_string()).collect(),
            cwd: PathBuf::from(launch.project),
            read_only_enforced: false,
            env,
            env_remove: Vec::new(),
        })
    }

    fn extract_result(&self, output: &str, exit_code: i32) -> Option<String> {
        if exit_code != 0 {
            return None;
        }
        let text: Vec<&str> = output.lines().filter(|l| !l.starts_with("SESSION:") && !l.contains("child_entry") && !l.starts_with("running ")).collect();
        let text = text.join("\n").trim().to_string();
        (!text.is_empty()).then_some(text)
    }

    fn extract_provider_session(&self, output: &str) -> Option<String> {
        output.lines().find_map(|l| l.strip_prefix("SESSION:").map(str::to_string))
    }
}

/// Not a test when run normally. As a child (role set) it is the fake headless agent, the fake chat bridge, or
/// a process that only sleeps.
#[test]
fn child_entry() {
    let role = std::env::var(ROLE).unwrap_or_default();
    match role.as_str() {
        "headless" => headless_child(),
        "bridge" => {
            // The harness leaves "test ... " unterminated on stdout; the first JSON line must start fresh.
            println!();
            let profile = FakeProfile {
                memory: std::env::var(MEMORY).as_deref() == Ok("1"),
                headless: true,
                dir: PathBuf::from(std::env::var(DIR).unwrap_or_default()),
            };
            serve(Arc::new(profile), "Fake Chat".into(), std::io::stdin().lock(), std::io::stdout(), ServeOptions::default());
            std::process::exit(0);
        }
        "sleeper" => {
            std::thread::sleep(Duration::from_secs(120));
            std::process::exit(0);
        }
        _ => {}
    }
}

fn headless_child() {
    println!();
    let task = std::env::var(TASK).unwrap_or_default();
    let dir = PathBuf::from(std::env::var(DIR).unwrap_or_default());
    let mut words = task.split_whitespace();
    let verb = words.next().unwrap_or("").to_string();
    let count: usize = words.next().and_then(|w| w.parse().ok()).unwrap_or(3);
    match verb.as_str() {
        "stream" => {
            for n in 0..count {
                println!("line{n} done");
                let _ = std::io::stdout().flush();
                std::thread::sleep(Duration::from_millis(250));
            }
        }
        "slow" => {
            let exe = std::env::current_exe().unwrap();
            let mut grandchild = crate::proc_util::hidden_command(exe);
            grandchild.args(HARNESS).env(ROLE, "sleeper").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
            let grandchild = grandchild.spawn().expect("grandchild");
            let _ = std::fs::write(dir.join("slow.pids"), format!("{} {}", std::process::id(), grandchild.id()));
            for _ in 0..600 {
                println!("tick");
                let _ = std::io::stdout().flush();
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        "fail" => {
            eprintln!("fatal: key {SECRET} was rejected");
            std::process::exit(3);
        }
        "empty" => {}
        _ => {
            let resume = std::env::var(RESUME).unwrap_or_else(|_| "none".into());
            println!("got:{task} resume:{resume}");
            println!("SESSION:s-1");
        }
    }
    std::process::exit(0);
}

// ---- in-process rig: serve() on channel-backed pipes -------------------------------------------------------

struct ChanRead {
    rx: mpsc::Receiver<Vec<u8>>,
    rest: Vec<u8>,
}

impl Read for ChanRead {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.rest.is_empty() {
            match self.rx.recv() {
                Ok(bytes) => self.rest = bytes,
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.rest.len());
        out[..n].copy_from_slice(&self.rest[..n]);
        self.rest.drain(..n);
        Ok(n)
    }
}

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct Rig {
    tx: Option<mpsc::Sender<Vec<u8>>>,
    out: Sink,
    server: Option<std::thread::JoinHandle<()>>,
    dir: PathBuf,
    next_id: u64,
}

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-bridge-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fast_limits() -> TurnLimits {
    TurnLimits { extract_every: Duration::from_millis(20), poll: Duration::from_millis(10), ..TurnLimits::default() }
}

impl Rig {
    fn new(memory: bool) -> Rig {
        Rig::with(FakeProfile { memory, headless: true, dir: temp_dir() }, fast_limits())
    }

    fn with(profile: FakeProfile, limits: TurnLimits) -> Rig {
        let dir = profile.dir.clone();
        let (tx, rx) = mpsc::channel();
        let out = Sink::default();
        let sink = out.clone();
        let server = std::thread::spawn(move || {
            serve(Arc::new(profile), "Fake Chat".into(), BufReader::new(ChanRead { rx, rest: Vec::new() }), sink, ServeOptions { read_only: false, limits });
        });
        Rig { tx: Some(tx), out, server: Some(server), dir, next_id: 0 }
    }

    fn send_raw(&self, value: Value) {
        self.tx.as_ref().unwrap().send(format!("{value}\n").into_bytes()).unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> u64 {
        self.next_id += 1;
        self.send_raw(json!({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params}));
        self.next_id
    }

    fn lines(&self) -> Vec<Value> {
        String::from_utf8_lossy(&self.out.0.lock()).lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
    }

    fn response(&self, id: u64) -> Value {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(found) = self.lines().into_iter().find(|v| v["id"] == id && v.get("method").is_none()) {
                return found;
            }
            assert!(Instant::now() < deadline, "no answer to request {id}; got {:#?}", self.lines());
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn init(&mut self) -> String {
        let id = self.request("initialize", json!({"protocolVersion": 1}));
        assert!(self.response(id).get("result").is_some());
        let id = self.request("session/new", json!({"cwd": self.dir.to_string_lossy(), "mcpServers": []}));
        self.response(id)["result"]["sessionId"].as_str().unwrap().to_string()
    }

    fn prompt(&mut self, session: &str, text: &str) -> u64 {
        self.request("session/prompt", json!({"sessionId": session, "prompt": [{"type": "text", "text": text}]}))
    }

    /// The chunks sent so far for `session`, in order, with the time they were first seen.
    fn chunks(&self) -> Vec<String> {
        self.lines()
            .iter()
            .filter(|v| v["method"] == "session/update" && v["params"]["update"]["sessionUpdate"] == "agent_message_chunk")
            .map(|v| v["params"]["update"]["content"]["text"].as_str().unwrap_or("").to_string())
            .collect()
    }

    fn wait_chunks(&self, secs: u64, pred: impl Fn(&[String]) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while !pred(&self.chunks()) {
            assert!(Instant::now() < deadline, "chunks never matched: {:?}", self.chunks());
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn wait_dead(pid: u32, secs: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if !pid_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    !pid_alive(pid)
}

fn pids(dir: &std::path::Path) -> Vec<u32> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(text) = std::fs::read_to_string(dir.join("slow.pids")) {
            let found: Vec<u32> = text.split_whitespace().filter_map(|p| p.parse().ok()).collect();
            if found.len() == 2 {
                return found;
            }
        }
        assert!(Instant::now() < deadline, "the slow agent never wrote its pids");
        std::thread::sleep(Duration::from_millis(20));
    }
}

// ---- protocol ----------------------------------------------------------------------------------------------

#[test]
fn initialize_says_text_only_no_load_and_the_agents_label() {
    let mut rig = Rig::new(true);
    let id = rig.request("initialize", json!({"protocolVersion": 1, "clientCapabilities": {}}));
    let reply = rig.response(id);
    let result = &reply["result"];
    assert_eq!(result["protocolVersion"], 1);
    assert_eq!(result["agentInfo"]["name"], "Fake Chat");
    assert_eq!(result["agentCapabilities"]["loadSession"], false);
    assert_eq!(result["agentCapabilities"]["promptCapabilities"]["image"], false);
    assert_eq!(result["authMethods"], json!([]));
    let id = rig.request("session/load", json!({"sessionId": "x", "cwd": ".", "mcpServers": []}));
    assert_eq!(rig.response(id)["error"]["code"], -32601);
}

#[test]
fn an_agent_with_no_headless_mode_is_refused_on_initialize() {
    let profile = FakeProfile { memory: false, headless: false, dir: temp_dir() };
    let dir = profile.dir.clone();
    let mut rig = Rig::with(profile, fast_limits());
    let id = rig.request("initialize", json!({"protocolVersion": 1}));
    let reply = rig.response(id);
    assert_eq!(reply["error"]["message"], "Fake Chat has no non-interactive mode, so a chat adapter cannot be made");
    // The bridge ends after refusing.
    let server = rig.server.take().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !server.is_finished() {
        assert!(Instant::now() < deadline, "the bridge kept running");
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = server.join();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_prompt_streams_chunks_while_the_agent_runs_and_ends_with_end_turn() {
    let mut rig = Rig::new(true);
    let session = rig.init();
    let started = Instant::now();
    let id = rig.prompt(&session, "stream 4");
    // The first line is forwarded long before the agent (4 x 250 ms) is done.
    rig.wait_chunks(10, |c| c.concat().contains("line0"));
    assert!(started.elapsed() < Duration::from_millis(900), "first chunk came only at the end: {:?}", started.elapsed());
    let reply = rig.response(id);
    assert_eq!(reply["result"]["stopReason"], "end_turn");
    let chunks = rig.chunks();
    assert!(chunks.len() >= 2, "expected the answer in several chunks: {chunks:?}");
    let text = chunks.concat();
    for n in 0..4 {
        assert!(text.contains(&format!("line{n} done")), "{text:?}");
    }
    // Every chunk went out before the answer to the prompt.
    let lines = rig.lines();
    let answer_at = lines.iter().position(|v| v["id"] == id).unwrap();
    let last_chunk_at = lines.iter().rposition(|v| v["method"] == "session/update").unwrap();
    assert!(last_chunk_at < answer_at);
    // A memory agent has no notice.
    assert!(!text.contains("no conversation memory"));
}

#[test]
fn without_memory_the_first_message_says_so_once() {
    let mut rig = Rig::new(false);
    let session = rig.init();
    let id = rig.prompt(&session, "echo one");
    assert_eq!(rig.response(id)["result"]["stopReason"], "end_turn");
    let first = rig.chunks();
    assert_eq!(first[0], format!("{NO_MEMORY_NOTICE}\n\n"));
    let id = rig.prompt(&session, "echo two");
    assert_eq!(rig.response(id)["result"]["stopReason"], "end_turn");
    let all = rig.chunks().concat();
    assert_eq!(all.matches("no conversation memory").count(), 1, "{all:?}");
    // And turn 2 started fresh although the output carried a session id.
    assert!(all.contains("got:echo two resume:none"), "{all:?}");
}

#[test]
fn with_memory_the_second_turn_resumes_the_first() {
    let mut rig = Rig::new(true);
    let session = rig.init();
    let id = rig.prompt(&session, "echo one");
    rig.response(id);
    assert!(rig.chunks().concat().contains("got:echo one resume:none"));
    let before = rig.chunks().len();
    let id = rig.prompt(&session, "echo two");
    rig.response(id);
    let second = rig.chunks()[before..].concat();
    assert!(second.contains("got:echo two resume:s-1"), "{second:?}");
    // Another session starts its own conversation.
    let other = {
        let id = rig.request("session/new", json!({"cwd": rig.dir.to_string_lossy()}));
        rig.response(id)["result"]["sessionId"].as_str().unwrap().to_string()
    };
    let before = rig.chunks().len();
    let id = rig.prompt(&other, "echo three");
    rig.response(id);
    assert!(rig.chunks()[before..].concat().contains("resume:none"));
}

#[test]
fn cancel_kills_the_whole_process_tree() {
    let mut rig = Rig::new(true);
    let session = rig.init();
    let id = rig.prompt(&session, "slow");
    rig.wait_chunks(15, |c| c.concat().contains("tick"));
    let pids = pids(&rig.dir);
    assert!(pids.iter().all(|p| pid_alive(*p)), "the agent and its helper run before the cancel");
    rig.send_raw(json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": session}}));
    assert_eq!(rig.response(id)["result"]["stopReason"], "cancelled");
    for pid in pids {
        assert!(wait_dead(pid, 10), "process {pid} survived the cancel");
    }
    // The next message works.
    let id = rig.prompt(&session, "echo again");
    assert_eq!(rig.response(id)["result"]["stopReason"], "end_turn");
}

#[test]
fn a_failing_agent_gives_one_plain_sentence_without_secrets() {
    let mut rig = Rig::new(true);
    let session = rig.init();
    let id = rig.prompt(&session, "fail");
    let reply = rig.response(id);
    let message = reply["error"]["message"].as_str().unwrap();
    assert!(message.starts_with("Fake Chat stopped with an error (exit code 3)."), "{message}");
    assert!(message.contains("It said:") && message.contains("was rejected"), "{message}");
    assert!(!message.contains(SECRET), "a secret reached the host: {message}");
    assert_ne!(reply["error"]["code"], -32000, "-32000 means sign in first to the host");
}

#[test]
fn an_agent_that_prints_nothing_is_an_error_not_an_empty_answer() {
    let mut rig = Rig::new(true);
    let session = rig.init();
    let id = rig.prompt(&session, "empty");
    let reply = rig.response(id);
    assert!(reply["error"]["message"].as_str().unwrap_or_default().contains("did not give an answer this app can read"), "{reply} {:?}", rig.lines());
}

#[test]
fn a_turn_past_the_time_limit_is_stopped_with_a_sentence() {
    let limits = TurnLimits { timeout: Duration::from_millis(600), ..fast_limits() };
    let mut rig = Rig::with(FakeProfile { memory: true, headless: true, dir: temp_dir() }, limits);
    let session = rig.init();
    let id = rig.prompt(&session, "slow");
    let reply = rig.response(id);
    assert!(reply["error"]["message"].as_str().unwrap().contains("took longer than"), "{reply}");
    for pid in pids(&rig.dir) {
        assert!(wait_dead(pid, 10), "process {pid} survived the time limit");
    }
}

#[test]
fn a_second_message_while_one_runs_is_refused_and_bad_prompts_are_rejected() {
    let mut rig = Rig::new(true);
    let session = rig.init();
    let first = rig.prompt(&session, "slow");
    rig.wait_chunks(15, |c| c.concat().contains("tick"));
    let second = rig.prompt(&session, "echo x");
    assert!(rig.response(second)["error"]["message"].as_str().unwrap().contains("still being answered"));
    let image_only = rig.request("session/prompt", json!({"sessionId": session, "prompt": [{"type": "image", "data": "x", "mimeType": "image/png"}]}));
    assert_eq!(rig.response(image_only)["error"]["code"], -32602);
    let unknown = rig.request("session/prompt", json!({"sessionId": "nope", "prompt": [{"type": "text", "text": "hi"}]}));
    assert_eq!(rig.response(unknown)["error"]["code"], -32602);
    rig.send_raw(json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": session}}));
    assert_eq!(rig.response(first)["result"]["stopReason"], "cancelled");
}

#[test]
fn closing_the_host_pipe_stops_a_running_agent() {
    let mut rig = Rig::new(true);
    let session = rig.init();
    rig.prompt(&session, "slow");
    rig.wait_chunks(15, |c| c.concat().contains("tick"));
    let pids = pids(&rig.dir);
    rig.tx.take();
    let server = rig.server.take().unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while !server.is_finished() {
        assert!(Instant::now() < deadline, "the bridge outlived its host");
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = server.join();
    for pid in pids {
        assert!(wait_dead(pid, 10), "process {pid} outlived the host");
    }
}

// ---- the API -----------------------------------------------------------------------------------------------

#[test]
fn info_says_whether_there_is_memory_and_refuses_no_headless() {
    let with = info_for_profile(&FakeProfile { memory: true, headless: true, dir: PathBuf::new() }, "Fake Chat").unwrap();
    assert!(with.has_memory && with.note.contains("continue the same conversation"));
    let without = info_for_profile(&FakeProfile { memory: false, headless: true, dir: PathBuf::new() }, "Fake Chat").unwrap();
    assert!(!without.has_memory && without.note.contains("each message starts fresh"));
    let refused = info_for_profile(&FakeProfile { memory: false, headless: false, dir: PathBuf::new() }, "Fake Chat").unwrap_err();
    assert_eq!(refused, "Fake Chat has no non-interactive mode, so a chat adapter cannot be made");
}

#[test]
fn the_real_registry_answers_and_a_shell_is_not_an_agent_to_bridge() {
    let shell = available_for("powershell").unwrap_err();
    assert!(shell.contains("cannot be made"), "{shell}");
    assert!(available_for("definitely-not-an-agent").unwrap_err().contains("not a known agent"));
    // A registered agent with a headless mode gets a spec: this exe, the subcommand, the agent id.
    let ids: Vec<String> = crate::tui::registered_agent_ids();
    let bridgeable = ids.iter().find(|id| available_for(id).is_ok()).expect("some registered agent has a headless mode");
    let spec = spawn_spec(bridgeable).unwrap();
    assert_eq!(PathBuf::from(&spec.program), std::env::current_exe().unwrap());
    assert_eq!(spec.args, ["acp-bridge", "--agent", bridgeable.as_str()].map(std::ffi::OsString::from));
    assert!(spec.env.iter().any(|(k, _)| k.to_string_lossy().eq_ignore_ascii_case("PATH")), "the bridge needs PATH");
    assert!(spawn_spec("powershell").is_err());
}

#[test]
fn the_command_line_is_ours_only_for_the_subcommand() {
    assert_eq!(run_cli(&[]), None);
    assert_eq!(run_cli(&["--new-pane".into(), "x".into()]), None);
    assert_eq!(run_cli(&["acp-bridge".into()]), Some(2));
    assert_eq!(run_cli(&["acp-bridge".into(), "--bogus".into()]), Some(2));
}

// ---- end to end: the real host code (conn.rs + session.rs) talks to the bridge subprocess -------------------

mod host {
    use super::*;
    use crate::acp::config::{AgentConfig, HostEnv, PinPolicy, Timeouts};
    use crate::acp::session::{AcpSession, Readiness, SessionEvent, SessionParams, StartKind};
    use crate::acp::state::StopKind;

    struct Env(PathBuf);

    impl HostEnv for Env {
        fn var(&self, name: &str) -> Option<std::ffi::OsString> {
            std::env::var_os(name)
        }
        fn which(&self, _: &str) -> Option<PathBuf> {
            None
        }
        fn adapters_dir(&self) -> PathBuf {
            self.0.join("adapters")
        }
        fn homes_dir(&self) -> PathBuf {
            self.0.join("homes")
        }
    }

    fn config(dir: &std::path::Path, memory: bool) -> AgentConfig {
        AgentConfig {
            id: "bridgefake".into(),
            label: "Fake Chat".into(),
            command: std::env::current_exe().unwrap().to_string_lossy().into_owned(),
            args: HARNESS.iter().map(|a| a.to_string()).collect(),
            env: [
                (ROLE.to_string(), "bridge".to_string()),
                (DIR.to_string(), dir.to_string_lossy().into_owned()),
                (MEMORY.to_string(), if memory { "1" } else { "0" }.to_string()),
            ]
            .into_iter()
            .collect(),
            pin_policy: PinPolicy::Off,
            timeouts: Timeouts { request_secs: 20, new_session_secs: 20, load_secs: 20, stdin_write_secs: 2, ..Timeouts::default() },
            ..AgentConfig::default()
        }
    }

    struct Chat {
        session: Arc<AcpSession>,
        log: Arc<Mutex<Vec<SessionEvent>>>,
        dir: PathBuf,
    }

    impl Chat {
        fn new(memory: bool) -> Chat {
            let dir = temp_dir();
            let host: Arc<dyn HostEnv> = Arc::new(Env(dir.clone()));
            let session = AcpSession::new(
                Arc::new(config(&dir, memory)),
                host,
                SessionParams { key: format!("bridge-{}", uuid::Uuid::new_v4()), cwd: dir.clone(), resume_session_id: None },
            );
            let log = Arc::new(Mutex::new(Vec::new()));
            let sink = log.clone();
            session.add_sink(Arc::new(move |event| sink.lock().push(event.clone())));
            Chat { session, log, dir }
        }

        fn turn(&self, text: &str) -> crate::acp::state::TurnSummary {
            let from = self.log.lock().len();
            self.session.prompt_text(text).unwrap_or_else(|e| panic!("prompt {text:?}: {e}"));
            self.wait_end(from)
        }

        fn wait_end(&self, from: usize) -> crate::acp::state::TurnSummary {
            let deadline = Instant::now() + Duration::from_secs(40);
            loop {
                if let Some(SessionEvent::TurnEnded(summary)) = self.log.lock().iter().skip(from).find(|e| matches!(e, SessionEvent::TurnEnded(_))) {
                    return summary.clone();
                }
                assert!(Instant::now() < deadline, "the turn never ended");
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        fn text_since(&self, from: usize) -> String {
            self.log
                .lock()
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
    }

    impl Drop for Chat {
        fn drop(&mut self) {
            self.session.close();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn the_host_session_chats_through_the_bridge_with_memory() {
        let chat = Chat::new(true);
        assert_eq!(chat.session.ensure_running().unwrap(), Readiness::Started(StartKind::New));
        let caps = chat.session.caps().unwrap();
        assert_eq!(caps.agent_name.as_deref(), Some("Fake Chat"));
        assert!(!caps.load_session && !caps.resume && !caps.image_prompts);
        let summary = chat.turn("echo hello");
        assert_eq!(summary.stop, StopKind::EndTurn, "{summary:?}");
        assert!(chat.text_since(0).contains("got:echo hello resume:none"), "{}", chat.text_since(0));
        let from = chat.log.lock().len();
        let summary = chat.turn("echo again");
        assert_eq!(summary.stop, StopKind::EndTurn);
        assert!(chat.text_since(from).contains("resume:s-1"), "{}", chat.text_since(from));
        assert!(!chat.text_since(0).contains("no conversation memory"));
    }

    #[test]
    fn the_host_sees_the_no_memory_notice_once() {
        let chat = Chat::new(false);
        chat.turn("echo a");
        let from = chat.log.lock().len();
        chat.turn("echo b");
        let all = chat.text_since(0);
        assert_eq!(all.matches("no conversation memory").count(), 1, "{all}");
        assert!(chat.text_since(from).contains("resume:none"));
    }

    #[test]
    fn the_host_cancel_and_failure_paths_work_through_the_bridge() {
        let chat = Chat::new(true);
        // Failure: the host gets the sentence as the turn's error, no secret in it.
        let summary = chat.turn("fail");
        let error = summary.error.clone().unwrap_or_default();
        assert!(error.contains("exit code 3") && !error.contains(SECRET), "{summary:?}");
        // Cancel: the host's Stop reaches the headless process tree.
        let from = chat.log.lock().len();
        chat.session.prompt_text("slow").unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !chat.text_since(from).contains("tick") {
            assert!(Instant::now() < deadline, "no output from the slow agent");
            std::thread::sleep(Duration::from_millis(20));
        }
        let pids = pids(&chat.dir);
        chat.session.cancel().unwrap();
        let summary = chat.wait_end(from);
        assert_eq!(summary.stop, StopKind::Cancelled, "{summary:?}");
        for pid in pids {
            assert!(wait_dead(pid, 10), "process {pid} survived the host's stop");
        }
        // The chat is still usable.
        assert_eq!(chat.turn("echo after").stop, StopKind::EndTurn);
    }

    /// A conversation started from a `source: "bridge"` overlay agent: the overlay resolves the app exe + `acp-bridge`,
    /// the test swaps in the fake bridge process (the unit-test exe is not the app), the rest of the config is the
    /// overlay's own.
    struct BridgeSource {
        store: crate::acp::overlay::OverlayStore,
        fake: AgentConfig,
    }

    impl crate::acp::conversations::AgentSource for BridgeSource {
        fn config(&self, agent: &str) -> Option<Arc<AgentConfig>> {
            let mut cfg = crate::acp::overlay::resolve_with(&self.store, agent, None, Some("Fake Chat".into()))?;
            assert_eq!(PathBuf::from(&cfg.command), std::env::current_exe().unwrap());
            assert_eq!(cfg.args, ["acp-bridge", "--agent", agent]);
            assert!(cfg.resume_command.is_empty() && cfg.env_allow.is_empty());
            cfg.command = self.fake.command.clone();
            cfg.args = self.fake.args.clone();
            cfg.env = self.fake.env.clone();
            Some(Arc::new(cfg))
        }
    }

    #[test]
    fn a_conversation_starts_from_a_bridge_overlay_agent() {
        use crate::acp::conversations::Conversations;
        use crate::acp::record::RecordStore;
        use crate::acp::supervisor::{Supervisor, SupervisorConfig};
        let dir = temp_dir();
        let store = crate::acp::overlay::OverlayStore::at(dir.join("acp_overlay.json"));
        let agent = crate::tui::registered_agent_ids().into_iter().find(|id| available_for(id).is_ok()).expect("a headless agent");
        store.put_item(&agent, &crate::acp::overlay::OverlayItem::Bridge(crate::acp::overlay::BridgeEntry { raw: json!({"source": "bridge"}) })).unwrap();
        let source = Arc::new(BridgeSource { store: store.clone(), fake: config(&dir, true) });
        let host: Arc<dyn HostEnv> = Arc::new(Env(dir.clone()));
        let conversations = Conversations::new(RecordStore::at(dir.join("conversations.json")), Arc::new(Supervisor::new(SupervisorConfig::default())), host, source);
        let view = conversations.create(&agent, &dir).expect("create from the bridge overlay");
        let session = conversations.session(&view.id).unwrap();
        session.prompt_text("echo hello").unwrap();
        let snap = session.wait_for(|s| s.turn >= 1 && !s.turn_in_flight(), Duration::from_secs(40));
        assert!(snap.is_some(), "the turn did not finish");
        session.close();
        store.remove(&agent).unwrap();
        assert!(conversations.create(&agent, &dir).is_err(), "removing the overlay removes the agent for new chats");
        let _ = std::fs::remove_dir_all(dir);
    }
}
