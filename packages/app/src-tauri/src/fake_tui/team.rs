//! The two-person Team end-to-end test (`tests/fake_tui_team.rs`): one Puppet Master per process.
//!
//! The Team service, the audit log, the pane registry and the app-data folder are process-wide, so
//! two people are two processes: the test process is the first person, and a child process (this
//! same test binary, started again) is the second. Each is an isolated world: its own app-data
//! folder under its own temp folder (`world.rs`), its own pane registry with real fake-agent panes,
//! its own in-memory key store (the OS credential store is never touched) and no bridge port. The
//! two share only what real teammates share: a git remote and a relay, both on this computer.
//!
//! A person's app is driven through [`Person::call`], whose operations are the same functions the
//! cockpit's commands call (`team::commands` -> `team::service`), one per click: enable Team,
//! create, invite, join, approve, remove, tell the team, queue my agent, start, apply, discard.
//! The child process serves the same calls over a loopback socket ([`Remote`]).
//!
//! Dev only (`dev-fake-tui`); nothing here exists in the app people run.

use super::world::{Session, World};
use crate::secrets::MemoryBackend;
use crate::team::keystore::KeyStore;
use crate::team::roster::Roster;
use crate::team::secure::{Payload, Session as TeamSession};
use crate::team::service::{self, ConfigInput, CreateInput, JoinInput};
use crate::team::transport::relay::RelayTransport;
use crate::team::transport::TeamTransport;
use pm_relay_protocol::Envelope;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Which of a body's values someone holding `stored` (a relay's every stored byte) could read,
/// looking inside base64 too. The leak check of `team/tests_v1.rs`.
pub fn leak_check<'a>(stored: &str, values: &[&'a str]) -> Vec<&'a str> {
    crate::team::leakcheck::readable_in(stored, values)
}

fn to_json<T: serde::Serialize>(value: Result<T, String>) -> Result<Value, String> {
    value.and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
}

fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key).and_then(Value::as_str).ok_or_else(|| format!("missing {key}"))
}

/// One person's Puppet Master, in this process.
pub struct Person {
    session: Session,
    keys: KeyStore,
    /// Someone who is not on the team: keys for forged envelopes.
    stranger: KeyStore,
}

impl Person {
    /// This process becomes `name`'s Puppet Master: a fake-agent world, and the Team service on it
    /// with an in-memory key store.
    pub fn start(name: &str) -> Person {
        let world = World::get();
        let session = world.session(&format!("team-{}", name.to_lowercase()));
        let keys = KeyStore::with_backend(Arc::new(MemoryBackend::new()), name);
        let (app, registry) = world.handles();
        assert!(service::init_for_e2e(app, registry, keys.clone()), "this process already runs a Team service");
        Person { session, keys, stranger: KeyStore::with_backend(Arc::new(MemoryBackend::new()), "stranger") }
    }

    /// A folder of this person's world for repositories and scratch files.
    pub fn aux(&self) -> &Path {
        self.session.aux()
    }

    /// One click, or one look, in this person's app.
    pub fn call(&self, op: &str, args: &Value) -> Result<Value, String> {
        match op {
            // ---- Settings > Team
            "enable" => {
                let projects = args["projects"].as_array().map(|a| a.iter().filter_map(|p| p.as_str().map(str::to_string)).collect()).unwrap_or_default();
                to_json(service::config_set(ConfigInput { enabled: true, projects }))
            }
            "create" => to_json(service::create(CreateInput {
                team_name: text(args, "team_name")?.into(),
                name: text(args, "name")?.into(),
                relay: text(args, "relay")?.into(),
                folder: String::new(),
            })),
            "invite" => to_json(service::create_invite()),
            "join" => to_json(service::join(JoinInput {
                link: text(args, "link")?.into(),
                code: text(args, "code")?.into(),
                name: text(args, "name")?.into(),
                github: None,
            })),
            "config" => to_json(service::config_get()),
            "approve" => to_json(service::approve_join(text(args, "join_id")?)),
            "decline" => to_json(service::decline_join(text(args, "join_id")?)),
            "remove" => to_json(service::remove_member(text(args, "member_id")?)),
            // ---- the cockpit's Team section
            "view" => to_json(service::view()),
            "send" => to_json(service::send(text(args, "draft_id")?, args.get("note").and_then(Value::as_str).map(str::to_string)).map(|()| Value::Null)),
            "dismiss_draft" => to_json(service::dismiss_draft(text(args, "draft_id")?).map(|()| Value::Null)),
            "dismiss_card" => to_json(service::dismiss_card(text(args, "card_id")?).map(|()| Value::Null)),
            "pull" => to_json(service::pull(text(args, "card_id")?)),
            "queue" => to_json(service::queue(text(args, "card_id")?, text(args, "pane_id")?, text(args, "when")?)),
            "start_job" => to_json(service::start_job(text(args, "job_id")?).map(|()| Value::Null)),
            "apply" => to_json(service::apply(text(args, "job_id")?)),
            "discard" => to_json(service::discard(text(args, "job_id")?).map(|()| Value::Null)),
            "diff" => to_json(service::diff(text(args, "job_id")?)),
            // ---- the team tools of this person's agents (MCP): `team_roster`, `team_events`, `team_wait`, `team_notify`
            "mcp" => self.mcp(args),
            // How many envelopes the relay holds for the team, as this person's device sees them.
            "relay_count" => self.relay_count(text(args, "relay")?, text(args, "team")?),
            // ---- the fake agents of this person
            "script" => {
                self.session.script(text(args, "toml")?);
                Ok(Value::Null)
            }
            "agent_pane" => self.session.spawn_pane_in(text(args, "agent_type")?, Path::new(text(args, "cwd")?)).map(Value::from),
            "pane_status" => Ok(self.session.pane_status(text(args, "pane_id")?).map(Value::from).unwrap_or(Value::Null)),
            "run" => self.run_agent(args),
            "events" => Ok(Value::Array(self.session.events())),
            "screen" => Ok(Value::from(self.session.screen(text(args, "pane_id")?))),
            // ---- test hooks (see `team::service`)
            "watched" => service::e2e_watched().map(|dirs| json!(dirs.iter().map(|d| d.to_string_lossy()).collect::<Vec<_>>())),
            "inject" => {
                let envelope: Envelope = serde_json::from_value(args["envelope"].clone()).map_err(|e| e.to_string())?;
                service::e2e_inject(envelope).map(Value::from)
            }
            // ---- what a hostile relay or a stranger could try (the first person only)
            "envelope" => self.envelope_with_id(text(args, "relay")?, text(args, "team")?, text(args, "id")?),
            "forge" => self.forge(args),
            "stranger_posts" => self.stranger_posts(args),
            other => Err(format!("unknown operation {other}")),
        }
    }

    /// One call of an agent's team tool, through the same bridge route code the MCP shim reaches
    /// (`bridge/routes/team_mcp.rs`): `op` is `roster`, `events`, `wait` or `notify`, `body` its arguments.
    /// By default the caller is an MCP session on this computer; `headers`, `remote` and `session`
    /// let a test be someone else (a phone behind a tunnel). `Err` carries the error JSON as text.
    fn mcp(&self, args: &Value) -> Result<Value, String> {
        let op = text(args, "op")?;
        let body = args.get("body").cloned().unwrap_or_else(|| json!({}));
        let headers = args.get("headers").and_then(Value::as_str).unwrap_or("Host: 127.0.0.1\r\n");
        let remote = args.get("remote").and_then(Value::as_bool).unwrap_or(false);
        let session = match args.get("session") {
            None => Some("e2e-agent"),
            Some(Value::Null) => None,
            Some(other) => other.as_str(),
        };
        match crate::bridge::team_agent_request("POST", &["team", "agent", op], &body, headers, !remote, session, &crate::bridge::LiveTeam) {
            Some(Ok((_, value))) => Ok(value),
            Some(Err((_, error))) => Err(error.to_string()),
            None => Err(format!("no team tool route {op}")),
        }
    }

    /// The envelopes in the team's mailbox on the relay: read from the start until nothing more comes.
    fn relay_count(&self, relay: &str, team: &str) -> Result<Value, String> {
        let wire = self.mailbox(relay, team)?;
        let rx = wire.subscribe(0).map_err(|e| e.0)?;
        let mut count = 0u64;
        while rx.recv_timeout(Duration::from_millis(1500)).is_ok() {
            count += 1;
        }
        wire.stop();
        Ok(Value::from(count))
    }

    /// `run_agent` in `project`, and wait until the run is over.
    fn run_agent(&self, args: &Value) -> Result<Value, String> {
        let project = text(args, "project")?;
        let started = self
            .session
            .run(text(args, "agent_type")?, text(args, "task")?, json!({ "project_path": project }))
            .map_err(|e| e.to_string())?;
        let handle = started["handle"].as_str().ok_or("no handle")?.to_string();
        let waited = self
            .session
            .agents(
                "POST",
                "/agents/wait",
                json!({
                    "project_path": project,
                    "handles": [handle],
                    "until": ["completed", "failed", "cancelled", "needs_input"],
                    "wait_ms": args["wait_s"].as_u64().unwrap_or(60) * 1000,
                }),
            )
            .map_err(|e| e.to_string())?;
        Ok(waited["agents"][0].clone())
    }

    fn mailbox(&self, relay: &str, team: &str) -> Result<RelayTransport, String> {
        RelayTransport::new(relay, team, self.keys.device_sign()?)
    }

    /// The envelope with this id, read off the relay with this person's device (they are on the
    /// team): what a teammate's app is handed.
    fn envelope_with_id(&self, relay: &str, team: &str, id: &str) -> Result<Value, String> {
        let wire = self.mailbox(relay, team)?;
        let rx = wire.subscribe(0).map_err(|e| e.0)?;
        let deadline = Instant::now() + Duration::from_secs(20);
        let found = loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(inbound) if inbound.envelope.id == id => break Ok(inbound.envelope),
                Ok(_) => {}
                Err(_) => break Err(format!("the relay never delivered envelope {id}")),
            }
        };
        wire.stop();
        found.and_then(|e| serde_json::to_value(e).map_err(|e| e.to_string()))
    }

    /// An envelope a teammate's app must refuse. `kind`:
    /// - `stranger`: sealed with the real team key (as if it leaked) and signed by a device that is
    ///   not on the team, claiming to be this person;
    /// - `impersonator`: the same, but its `from` names this person's device;
    /// - `tampered`: `envelope` (a real one) with its ciphertext changed;
    /// - `unsigned`: `envelope` with its signature removed.
    fn forge(&self, args: &Value) -> Result<Value, String> {
        let kind = text(args, "kind")?;
        let genuine = || -> Result<Envelope, String> { serde_json::from_value(args["envelope"].clone()).map_err(|e| e.to_string()) };
        let envelope = match kind {
            "tampered" => {
                let mut env = genuine()?;
                let mut ct: Vec<char> = env.ct.chars().collect();
                let at = ct.len() / 2;
                ct[at] = if ct[at] == 'A' { 'B' } else { 'A' };
                env.ct = ct.into_iter().collect();
                env
            }
            "unsigned" => {
                let mut env = genuine()?;
                env.sig = String::new();
                env
            }
            "stranger" | "impersonator" => {
                let mine = service::e2e_session()?;
                let roster: &Roster = &mine.roster;
                let key = self.keys.epoch_key(&roster.team, roster.epoch)?.ok_or("no team key")?;
                self.stranger.set_epoch_key(&roster.team, roster.epoch, &key)?;
                let me = mine.my_member()?;
                let my_device = mine.my_device()?;
                let mut event = crate::team::event::TeamEvent {
                    v: crate::team::event::SCHEMA_VERSION,
                    id: crate::team::event::new_event_id(crate::team::service::now_ms()),
                    team: roster.team.clone(),
                    project: crate::team::event::ProjectRef { key: text(args, "project_key")?.into(), name: "forged".into() },
                    actor: crate::team::event::Actor { member: me, device: my_device.clone(), display: "Maya".into(), agent: None },
                    kind: crate::team::event::TeamEventKind::Pushed,
                    at_ms: crate::team::service::now_ms(),
                    subject: crate::team::event::Subject {
                        branch: Some("feat/forged".into()),
                        commit: Some("0123456789abcdef0123456789abcdef01234567".into()),
                        ..Default::default()
                    },
                    note: Some("forged".into()),
                    to: Vec::new(),
                    tier_hint: crate::team::event::Tier::Ambient,
                    dedupe_key: String::new(),
                    expires_at_ms: crate::team::service::now_ms() + crate::team::event::DEFAULT_TTL_MS,
                    source: crate::team::event::EventSource::Git,
                };
                event.dedupe_key = crate::team::event::dedupe_key(&event.project.key, event.kind, &event.subject);
                let as_stranger = TeamSession::new(roster.clone(), self.stranger.clone());
                let mut env = as_stranger.seal(&Payload::Event { event }, 1, crate::team::service::now_ms())?;
                if kind == "impersonator" {
                    env.from = my_device;
                    env.sig = pm_relay_protocol::sign(&self.stranger.device_sign()?, &env.signing_bytes());
                }
                env
            }
            other => return Err(format!("unknown forgery {other}")),
        };
        serde_json::to_value(envelope).map_err(|e| e.to_string())
    }

    /// A device that is not on the team tries to post `envelope` to the relay: the relay's answer.
    fn stranger_posts(&self, args: &Value) -> Result<Value, String> {
        let envelope: Envelope = serde_json::from_value(args["envelope"].clone()).map_err(|e| e.to_string())?;
        let wire = RelayTransport::new(text(args, "relay")?, &envelope.team, self.stranger.device_sign()?)?;
        Ok(match wire.send(&envelope) {
            Ok(()) => json!({ "accepted": true }),
            Err(e) => json!({ "accepted": false, "error": e.0, "denied": e.is_denied() }),
        })
    }
}

// ---------------------------------------------------------------------------- the second person

const PARENT_ENV: &str = "PM_TEAM_E2E_PARENT";
const NAME_ENV: &str = "PM_TEAM_E2E_NAME";

/// The second person's process, as the first sees it.
pub struct Remote {
    child: std::process::Child,
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    name: String,
}

impl Remote {
    /// Start this test binary again as `name`'s Puppet Master, running only the test
    /// `module_path::test_name` (which calls [`Remote::serve_if_child`] first thing).
    pub fn spawn(name: &str, module_path: &str, test_name: &str) -> Remote {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port for the second person");
        listener.set_nonblocking(true).expect("non-blocking listener");
        let port = listener.local_addr().expect("listener address").port();
        // libtest names tests without the crate: `team::the_test`.
        let filter = format!("{}::{test_name}", module_path.split_once("::").map(|(_, rest)| rest).unwrap_or(module_path));
        let mut child = std::process::Command::new(std::env::current_exe().expect("this test binary"))
            .args(["--exact", &filter, "--nocapture", "--test-threads=1"])
            .env(PARENT_ENV, format!("127.0.0.1:{port}"))
            .env(NAME_ENV, name)
            .stdin(std::process::Stdio::null())
            .spawn()
            .expect("start the second person's process");
        let deadline = Instant::now() + Duration::from_secs(120);
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(_) => {
                    if let Ok(Some(status)) = child.try_wait() {
                        panic!("{name}'s process ended before it connected ({status})");
                    }
                    assert!(Instant::now() < deadline, "{name}'s process never connected");
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        };
        stream.set_nonblocking(false).expect("blocking stream");
        stream.set_read_timeout(Some(Duration::from_secs(120))).expect("read timeout");
        let writer = stream.try_clone().expect("clone the stream");
        Remote { child, reader: BufReader::new(stream), writer, name: name.to_string() }
    }

    /// One click, or one look, in the second person's app.
    pub fn call(&mut self, op: &str, args: Value) -> Result<Value, String> {
        let line = json!({ "op": op, "args": args }).to_string();
        writeln!(self.writer, "{line}").map_err(|e| format!("{}'s process is gone: {e}", self.name))?;
        let mut reply = String::new();
        match self.reader.read_line(&mut reply) {
            Ok(0) | Err(_) => {
                let status = self.child.try_wait().ok().flatten();
                return Err(format!("{}'s process is gone ({status:?}) while it ran {op}", self.name));
            }
            Ok(_) => {}
        }
        let reply: Value = serde_json::from_str(&reply).map_err(|e| format!("unreadable reply to {op}: {e}"))?;
        match reply.get("err").and_then(Value::as_str) {
            Some(err) => Err(err.to_string()),
            None => Ok(reply["ok"].clone()),
        }
    }

    /// Ask the second person's process to end, and wait for it.
    pub fn quit(mut self) {
        let _ = self.call("quit", Value::Null);
        let _ = self.child.wait();
    }

    /// In the second person's process: serve the first one's calls until it hangs up. False in the
    /// first person's process (the test then carries on as the first person).
    pub fn serve_if_child() -> bool {
        let (Ok(addr), Ok(name)) = (std::env::var(PARENT_ENV), std::env::var(NAME_ENV)) else { return false };
        let stream = TcpStream::connect(&addr).expect("connect to the first person");
        let mut writer = stream.try_clone().expect("clone the stream");
        let reader = BufReader::new(stream);
        let person = Person::start(&name);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            let Ok(request): Result<Value, _> = serde_json::from_str(&line) else { break };
            let op = request["op"].as_str().unwrap_or_default();
            if op == "quit" {
                let _ = writeln!(writer, "{}", json!({ "ok": null }));
                break;
            }
            let reply = match person.call(op, &request["args"]) {
                Ok(value) => json!({ "ok": value }),
                Err(err) => json!({ "err": err }),
            };
            if writeln!(writer, "{reply}").is_err() {
                break;
            }
        }
        true
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        // Only the process this test started, by its handle.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
