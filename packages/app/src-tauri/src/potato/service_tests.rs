//! The engine on fakes: a clock that is a number, a launcher that starts no process, a host that holds
//! the key in a map. What it shows: a send is saved redacted and queued; the brain starts lazily; what
//! the brain posts is saved, priced and judged; a crash is handled; the kill switch kills; and a brain
//! can only write into its own bot's thread.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::auth::Brains;
use super::env::HostEnv;
use super::protocol::{parse_event, Event};
use super::service::*;
use super::sidecar::{Child, Launcher, LineSink, NodeState, SpawnSpec};
use super::store::{self, BrainKind, FromKind, MsgKind, Settings};

pub(super) const KEY: &str = "sk-test-deepseek-key-1234567890";
const RUNNING: i32 = i32::MIN;

pub(super) struct FakeHost {
    pub now: AtomicI64,
    pub secrets: Mutex<HashMap<String, String>>,
    pub changes: Mutex<Vec<String>>,
    pub resources: Mutex<Option<PathBuf>>,
    pub url: Mutex<Option<String>>,
}

impl Host for FakeHost {
    fn now_ms(&self) -> i64 {
        self.now.load(Ordering::SeqCst)
    }
    /// One "day" is 24 hours of the fake clock.
    fn local_day(&self, ms: i64) -> String {
        format!("2026-11-{:02}", 1 + ms / 86_400_000)
    }
    fn bridge_url(&self) -> Option<String> {
        self.url.lock().clone()
    }
    fn profile(&self, bot: &str) -> Option<BotProfile> {
        match bot {
            "nimbus" => Some(BotProfile { name: "Nimbus".into(), folder: Some("C:\\Users\\someone\\PuppetMaster\\companions\\nimbus".into()) }),
            "mochi" => Some(BotProfile { name: "Mochi".into(), folder: None }),
            _ => None,
        }
    }
    fn host_env(&self) -> HostEnv {
        HostEnv { values: vec![("PATH".into(), "C:\\bin".into()), ("SystemRoot".into(), "C:\\Windows".into()), ("ANTHROPIC_API_KEY".into(), "sk-ant-never".into())] }
    }
    fn secret(&self, name: &str) -> Option<String> {
        self.secrets.lock().get(name).cloned()
    }
    fn resources(&self) -> Option<PathBuf> {
        self.resources.lock().clone()
    }
    fn changed(&self, bot: &str) {
        self.changes.lock().push(bot.to_string());
    }
}

pub(super) struct ChildState {
    pub pid: u32,
    pub exit: AtomicI32,
    pub killed: AtomicBool,
}

impl ChildState {
    pub fn exit_with(&self, code: i32) {
        self.exit.store(code, Ordering::SeqCst);
    }
    pub fn running(&self) -> bool {
        self.exit.load(Ordering::SeqCst) == RUNNING
    }
}

struct FakeChild(Arc<ChildState>);

impl Child for FakeChild {
    fn pid(&self) -> u32 {
        self.0.pid
    }
    fn try_wait(&mut self) -> Option<i32> {
        let code = self.0.exit.load(Ordering::SeqCst);
        (code != RUNNING).then_some(code)
    }
    fn kill_tree(&mut self) {
        self.0.killed.store(true, Ordering::SeqCst);
        self.0.exit.store(-1, Ordering::SeqCst);
    }
}

pub(super) struct Spawned {
    pub child: Arc<ChildState>,
    pub env_names: Vec<String>,
    pub token: String,
    pub pack: String,
    pub provider: String,
    pub cwd: PathBuf,
    pub script: PathBuf,
    pub has_key: bool,
    pub sink: LineSink,
}

pub(super) struct FakeLauncher {
    pub node: Mutex<NodeState>,
    pub spawned: Mutex<Vec<Spawned>>,
    pub fail_spawn: AtomicBool,
    next_pid: AtomicU32,
}

impl Launcher for FakeLauncher {
    fn node(&self) -> NodeState {
        self.node.lock().clone()
    }
    fn spawn(&self, spec: &SpawnSpec, sink: LineSink) -> Result<Box<dyn Child>, String> {
        if self.fail_spawn.load(Ordering::SeqCst) {
            return Err("no".into());
        }
        let child = Arc::new(ChildState { pid: self.next_pid.fetch_add(1, Ordering::SeqCst), exit: AtomicI32::new(RUNNING), killed: AtomicBool::new(false) });
        self.spawned.lock().push(Spawned {
            child: child.clone(),
            env_names: spec.env.names().iter().map(|s| s.to_string()).collect(),
            token: spec.env.get("PM_POTATO_TOKEN").unwrap_or("").to_string(),
            pack: spec.env.get("FISHD_PACK").unwrap_or("").to_string(),
            provider: spec.env.get("FISHD_PROVIDER").unwrap_or("").to_string(),
            cwd: spec.cwd.clone(),
            script: spec.script.clone(),
            has_key: spec.env.get("DEEPSEEK_API_KEY").is_some(),
            sink,
        });
        Ok(Box::new(FakeChild(child)))
    }
}

pub(super) struct Rig {
    pub svc: Arc<Service>,
    pub host: Arc<FakeHost>,
    pub launcher: Arc<FakeLauncher>,
    pub brains: Arc<Brains>,
    pub root: PathBuf,
}

impl Rig {
    pub fn new(name: &str) -> Rig {
        let root = std::env::temp_dir().join(format!("pm-potato-svc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let resources = root.join("resources");
        std::fs::create_dir_all(resources.join("persona")).unwrap();
        std::fs::write(resources.join("brain.mjs"), "// the brain").unwrap();
        std::fs::write(resources.join("persona").join("core.md"), "persona text").unwrap();
        let host = Arc::new(FakeHost {
            now: AtomicI64::new(1_000),
            secrets: Mutex::new(HashMap::from([(SECRET_DEEPSEEK.to_string(), KEY.to_string())])),
            changes: Mutex::new(Vec::new()),
            resources: Mutex::new(Some(resources)),
            url: Mutex::new(Some("http://127.0.0.1:17999".into())),
        });
        let launcher = Arc::new(FakeLauncher {
            node: Mutex::new(NodeState::Ready { path: PathBuf::from("node"), major: 22 }),
            spawned: Mutex::new(Vec::new()),
            fail_spawn: AtomicBool::new(false),
            next_pid: AtomicU32::new(40_000),
        });
        let brains = Arc::new(Brains::new());
        brains.set_enabled(true);
        let data = root.join("potato");
        let svc = Service::new(data.clone(), host.clone(), launcher.clone(), brains.clone());
        store::add_bot(&data, "nimbus", "fish", 1).unwrap();
        Rig { svc, host, launcher, brains, root: data }
    }

    pub fn advance(&self, ms: i64) {
        self.host.now.fetch_add(ms, Ordering::SeqCst);
    }

    pub fn spawned(&self) -> usize {
        self.launcher.spawned.lock().len()
    }

    pub fn child(&self, n: usize) -> Arc<ChildState> {
        self.launcher.spawned.lock()[n].child.clone()
    }

    /// What the brain would fetch next, without waiting.
    pub fn inbox(&self, cursor: u64) -> (Vec<Value>, u64) {
        let body = self.svc.inbox_poll("nimbus", cursor, 0);
        (body["items"].as_array().cloned().unwrap_or_default(), body["cursor"].as_u64().unwrap())
    }

    /// The brain posts events.
    pub fn post(&self, events: Vec<Value>) {
        let parsed: Vec<Event> = events.iter().filter_map(parse_event).collect();
        assert_eq!(parsed.len(), events.len(), "every event in the test must parse");
        self.svc.ingest("nimbus", parsed);
    }

    /// The brain answers every message of the person that is still waiting.
    pub fn answer_all(&self) {
        let waiting: Vec<String> = self.thread().into_iter().filter(|m| m.from.kind == FromKind::You).map(|m| m.id).collect();
        for id in waiting {
            self.post(vec![reply("app:dm:nimbus", &id, &["ok"])]);
        }
    }

    pub fn hello(&self) {
        self.post(vec![json!({"v":1,"type":"hello","protocol":1,"version":"t","provider":"deepseek","packs":["fish","potato"],"surfaces":["app"]})]);
    }

    pub fn thread(&self) -> Vec<store::Message> {
        self.svc.messages("nimbus", 500).unwrap()
    }

    pub fn notes(&self) -> Vec<String> {
        self.thread().into_iter().filter(|m| m.kind == MsgKind::Note).map(|m| m.text).collect()
    }

    pub fn state(&self) -> BrainState {
        self.svc.state().unwrap().bots.iter().find(|b| b.id == "nimbus").unwrap().brain_state
    }

    /// A running, ready brain with one answered message behind it.
    pub fn ready(name: &str) -> Rig {
        let rig = Rig::new(name);
        rig.svc.send("nimbus", "hello").unwrap();
        rig.hello();
        rig
    }
}

fn reply(conversation: &str, in_reply_to: &str, segments: &[&str]) -> Value {
    json!({"v":1,"type":"reply","conversationId":conversation,"inReplyTo":in_reply_to,"segments":segments})
}

fn usage(completion: u64) -> Value {
    json!({"v":1,"type":"usage","model":"deepseek-flash","promptTokens":0,"completionTokens":completion,"cacheHitTokens":0,"cacheMissTokens":0})
}

#[test]
fn a_send_is_saved_starts_the_brain_and_waits_in_its_inbox() {
    let rig = Rig::new("send");
    assert_eq!(rig.spawned(), 0, "nothing starts until a message asks");
    let out = rig.svc.send("nimbus", "  hello there  ").unwrap();
    assert!(out.delivered);
    assert_eq!(rig.spawned(), 1, "lazy start on the first message");
    assert_eq!(out.state, BrainState::Starting);
    // saved as the person's own line
    let thread = rig.thread();
    assert_eq!(thread.len(), 1);
    assert_eq!((thread[0].from.kind, thread[0].kind, thread[0].text.as_str()), (FromKind::You, MsgKind::Text, "hello there"));
    assert_eq!(thread[0].id, out.id);
    // the inbox has the send, with the id the reply will name
    let (items, cursor) = rig.inbox(0);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["type"], "send");
    assert_eq!(items[0]["id"], out.id.as_str());
    assert_eq!(items[0]["conversationId"], "app:dm:nimbus");
    assert_eq!(items[0]["surface"], "app");
    assert_eq!(items[0]["from"]["trust"], "owner_pc");
    assert_eq!(items[0]["tools"], "full");
    assert_eq!(items[0]["text"], "hello there");
    assert_eq!(cursor, 1);
    // asked again from the new cursor: nothing new
    assert!(rig.inbox(cursor).0.is_empty());
    // the window was told
    assert!(rig.host.changes.lock().contains(&"nimbus".to_string()));
}

#[test]
fn the_text_is_redacted_on_its_way_to_the_store_and_to_the_brain() {
    let rig = Rig::new("redact");
    let secret = "sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghij";
    let home = "C:\\Users\\someone\\PuppetMaster\\companions\\nimbus";
    rig.svc.send("nimbus", &format!("use {secret} in {home}")).unwrap();
    let (items, _) = rig.inbox(0);
    let sent = items[0]["text"].as_str().unwrap();
    assert!(!sent.contains(secret) && !sent.contains("someone"), "{sent}");
    let saved = &rig.thread()[0].text;
    assert!(!saved.contains(secret) && !saved.contains("someone"), "{saved}");
}

#[test]
fn hello_makes_the_brain_ready_and_the_token_is_new_for_every_start() {
    let rig = Rig::ready("hello");
    assert_eq!(rig.state(), BrainState::Ready);
    // the brain has a token for this start, in the registry as a hash; and a fresh one next time
    let first_token = rig.launcher.spawned.lock()[0].token.clone();
    assert_eq!(first_token.len(), 64);
    rig.svc.stop_all(Duration::ZERO);
    assert_eq!(rig.brains.any_running(), false);
    rig.svc.send("nimbus", "again").unwrap();
    assert_eq!(rig.spawned(), 2);
    let second_token = rig.launcher.spawned.lock()[1].token.clone();
    assert_ne!(first_token, second_token);
}

#[test]
fn the_brain_gets_its_own_folder_the_persona_and_the_allowlisted_environment() {
    let rig = Rig::ready("spawn");
    let spawned = rig.launcher.spawned.lock();
    let s = &spawned[0];
    assert_eq!(s.cwd, rig.root.join("brain").join("nimbus"));
    assert!(s.script.ends_with("brain.mjs"));
    assert_eq!((s.pack.as_str(), s.provider.as_str(), s.has_key), ("fish", "deepseek", true));
    assert!(!s.env_names.iter().any(|n| n == "ANTHROPIC_API_KEY"), "{:?}", s.env_names);
    assert!(s.env_names.iter().any(|n| n == "PM_POTATO_TOKEN"));
    // the persona was seeded into the bot's own folder
    let seeded = std::fs::read_to_string(rig.root.join("brain").join("nimbus").join("persona").join("core.md")).unwrap();
    assert_eq!(seeded, "persona text");
}

#[test]
fn a_reply_is_saved_redacted_one_bubble_per_segment_with_the_mood() {
    let rig = Rig::ready("reply");
    let secret = "sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghij";
    let sent = rig.svc.send("nimbus", "work please").unwrap();
    rig.post(vec![
        json!({"v":1,"type":"typing","conversationId":"app:dm:nimbus","on":true}),
        json!({"v":1,"type":"mood","mood":"genki","intensity":2,"energy":60,"hunger":30}),
    ]);
    assert!(rig.svc.state().unwrap().bots[0].typing);
    rig.post(vec![
        json!({"v":1,"type":"reply","conversationId":"app:dm:nimbus","inReplyTo":sent.id,"segments":["first", format!("second {secret}")],"kaomoji":"(^_^)"}),
        json!({"v":1,"type":"typing","conversationId":"app:dm:nimbus","on":false}),
    ]);
    let thread = rig.thread();
    let bot_lines: Vec<_> = thread.iter().filter(|m| m.from.kind == FromKind::Bot).collect();
    assert_eq!(bot_lines.len(), 2, "each segment is its own bubble");
    assert_eq!(bot_lines[0].text, "first");
    assert!(bot_lines[1].text.starts_with("second ") && bot_lines[1].text.ends_with("(^_^)"));
    assert!(!bot_lines[1].text.contains(secret));
    assert_eq!(bot_lines[1].mood.as_deref(), Some("genki"));
    assert_eq!(bot_lines[0].from.name.as_deref(), Some("Nimbus"));
    let view = &rig.svc.state().unwrap().bots[0];
    assert!(!view.typing);
    assert_eq!(view.mood.as_ref().map(|m| (m.mood.as_str(), m.energy)), Some(("genki", 60)));
    assert_eq!(view.unread, 2);
    rig.svc.mark_read("nimbus").unwrap();
    assert_eq!(rig.svc.state().unwrap().bots[0].unread, 0);
}

#[test]
fn a_brain_can_only_write_into_its_own_bots_thread() {
    let rig = Rig::ready("scope");
    store::add_bot(&rig.root, "mochi", "potato", 2).unwrap();
    rig.post(vec![
        reply("app:dm:mochi", "x", &["into another bot"]),
        reply("outer:dm:nimbus:42", "x", &["into another surface"]),
        reply("app:dm:../../etc", "x", &["traversal"]),
        reply("nonsense", "x", &["no surface"]),
        json!({"v":1,"type":"typing","conversationId":"app:dm:mochi","on":true}),
        json!({"v":1,"type":"helper_request","requestId":"h-9","conversationId":"app:dm:mochi","task":"do it"}),
    ]);
    assert!(rig.svc.messages("mochi", 50).unwrap().is_empty());
    assert!(rig.thread().iter().all(|m| m.from.kind != FromKind::Bot));
    assert!(!rig.svc.state().unwrap().bots.iter().any(|b| b.typing));
    assert!(!rig.root.join("conversations").join("outer").exists());
}

#[test]
fn an_event_from_a_bot_that_is_not_a_bot_is_dropped() {
    let rig = Rig::ready("ghost");
    rig.svc.ingest("ghost", vec![parse_event(&reply("app:dm:ghost", "x", &["boo"])).unwrap()]);
    assert!(!rig.root.join("conversations").join("app").join("app%3Adm%3Aghost.jsonl").exists());
}

#[test]
fn usage_is_priced_by_puppet_master_warned_at_80_and_paused_at_100() {
    let rig = Rig::ready("limit");
    // the limit is 50 cents: 450_000 tokens of output at 1100 nano a token = 49.5 cents... so go in steps
    rig.post(vec![usage(300_000)]); // 33 cents
    assert!(rig.notes().is_empty(), "{:?}", rig.notes());
    rig.post(vec![usage(100_000)]); // 44 cents: 88% of 50
    assert_eq!(rig.notes().len(), 1);
    assert!(rig.notes()[0].contains("80%"), "{:?}", rig.notes());
    rig.post(vec![usage(100_000)]); // 55 cents: over
    let notes = rig.notes();
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[1], "Nimbus has used today's limit ($0.50). Raise it in Settings or wait until tomorrow.");
    // the brain is told to pause
    let (items, _) = rig.inbox(0);
    assert!(items.iter().any(|i| i["type"] == "pause" && i["reason"] == "daily_limit"), "{items:?}");
    assert_eq!(rig.state(), BrainState::DailyLimit);
    // a send is refused, saved with a note, until local midnight
    let out = rig.svc.send("nimbus", "are you there").unwrap();
    assert!(!out.delivered);
    assert_eq!(out.state, BrainState::DailyLimit);
    let (items, _) = rig.inbox(0);
    assert!(!items.iter().any(|i| i["type"] == "send" && i["text"] == "are you there"));
    // the limit is per day: the next morning it is lifted and the brain gets `resume`
    rig.advance(86_400_000);
    rig.svc.tick();
    let (items, _) = rig.inbox(0);
    assert!(items.iter().any(|i| i["type"] == "resume"), "{items:?}");
    let out = rig.svc.send("nimbus", "good morning").unwrap();
    assert!(out.delivered);
}

#[test]
fn raising_the_limit_lifts_the_pause_at_once() {
    let rig = Rig::ready("raise");
    rig.post(vec![usage(500_000)]);
    assert_eq!(rig.state(), BrainState::DailyLimit);
    rig.svc.set_settings(Settings { daily_limit_cents: 2_000, ..Settings::default() }).unwrap();
    let (items, _) = rig.inbox(0);
    assert!(items.iter().any(|i| i["type"] == "resume"), "{items:?}");
    assert_eq!(rig.state(), BrainState::Ready);
}

#[test]
fn an_unknown_model_is_charged_at_the_highest_price() {
    let rig = Rig::ready("unknown-model");
    rig.post(vec![json!({"v":1,"type":"usage","model":"brand-new-model","promptTokens":0,"completionTokens":1_000_000,"cacheHitTokens":0,"cacheMissTokens":0})]);
    let used = rig.svc.state().unwrap().bots[0].used_today_nano;
    assert_eq!(used, 1_000_000 * super::cost::highest_price().output);
    // the free test model costs nothing
    let free = Rig::ready("free-model");
    free.post(vec![json!({"v":1,"type":"usage","model":"fake","promptTokens":10,"completionTokens":5,"cacheHitTokens":0,"cacheMissTokens":10})]);
    assert_eq!(free.svc.state().unwrap().bots[0].used_today_nano, 0);
}

#[test]
fn a_provider_error_becomes_a_plain_sentence_in_the_thread() {
    let rig = Rig::ready("error");
    let sent = rig.svc.send("nimbus", "hi").unwrap();
    rig.post(vec![json!({"v":1,"type":"error","code":"PROVIDER_AUTH","conversationId":"app:dm:nimbus","inReplyTo":sent.id})]);
    assert_eq!(rig.notes().last().unwrap(), "Nimbus could not use the model key. Check it in Settings.");
    assert_eq!(rig.svc.state().unwrap().bots[0].last_error.as_deref(), Some("PROVIDER_AUTH"));
    // a good reply clears it
    rig.post(vec![reply("app:dm:nimbus", &sent.id, &["ok now"])]);
    assert_eq!(rig.svc.state().unwrap().bots[0].last_error, None);
}

#[test]
fn the_brain_text_in_a_log_line_never_shows_a_key() {
    let rig = Rig::ready("log");
    let sink = rig.launcher.spawned.lock()[0].sink.clone();
    sink(&format!("trouble with {KEY} and sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghij"));
    sink(&"y".repeat(1000));
    let token = rig.launcher.spawned.lock()[0].token.clone();
    sink(&format!("token is {token}"));
    let lines = rig.svc.log_lines("nimbus");
    assert_eq!(lines.len(), 3);
    for line in &lines {
        assert!(!line.contains(KEY) && !line.contains(&token) && !line.contains("sk-ant-api03"), "{line}");
        assert!(line.chars().count() <= 300);
    }
    assert!(lines[0].contains("[hidden]"));
}

#[test]
fn without_a_key_the_message_is_saved_with_a_note_and_nothing_starts() {
    let rig = Rig::new("nokey");
    rig.host.secrets.lock().clear();
    let out = rig.svc.send("nimbus", "hi").unwrap();
    assert!(!out.delivered);
    assert_eq!(out.state, BrainState::NeedsKey);
    assert_eq!(rig.spawned(), 0);
    assert_eq!(rig.notes(), ["Add your DeepSeek key in Settings to talk with Nimbus."]);
    assert_eq!(rig.thread().len(), 2, "the person's line is still there");
    assert_eq!(rig.state(), BrainState::NeedsKey);
}

#[test]
fn without_node_the_card_says_so_and_nothing_starts() {
    let rig = Rig::new("nonode");
    *rig.launcher.node.lock() = NodeState::Missing;
    let out = rig.svc.send("nimbus", "hi").unwrap();
    assert_eq!((out.delivered, out.state), (false, BrainState::NeedsNode));
    assert_eq!(rig.notes(), ["Potato Bot needs Node.js 20 or newer."]);
    assert_eq!(rig.spawned(), 0);
    *rig.launcher.node.lock() = NodeState::TooOld { found: "v18.0.0".into() };
    assert_eq!(rig.svc.send("nimbus", "hi again").unwrap().state, BrainState::NeedsNode);
    let state = rig.svc.state().unwrap();
    assert!(!state.node.ready);
    assert_eq!(state.node.found.as_deref(), Some("v18.0.0"));
    // Node arrives: the next message starts the brain
    *rig.launcher.node.lock() = NodeState::Ready { path: PathBuf::from("node"), major: 22 };
    assert!(rig.svc.send("nimbus", "now?").unwrap().delivered);
    assert_eq!(rig.spawned(), 1);
}

#[test]
fn a_missing_program_or_bridge_is_reported_not_retried() {
    let rig = Rig::new("noprogram");
    *rig.host.resources.lock() = None;
    assert_eq!(rig.svc.send("nimbus", "hi").unwrap().state, BrainState::MissingProgram);
    assert_eq!(rig.spawned(), 0);
}

#[test]
fn while_potato_bot_is_off_a_send_is_refused_and_the_routes_do_not_exist() {
    let rig = Rig::new("off");
    rig.svc.set_enabled(false);
    let err = rig.svc.send("nimbus", "hi").unwrap_err();
    assert_eq!(err.code, "POTATO_OFF");
    assert!(rig.thread().is_empty(), "nothing is saved while it is off");
    assert_eq!(rig.spawned(), 0);
    assert_eq!(rig.svc.state().unwrap().bots[0].brain_state, BrainState::Off);
    assert!(!rig.svc.state().unwrap().enabled);
}

#[test]
fn the_kill_switch_kills_every_brain_forgets_the_tokens_and_withdraws_the_cards() {
    let rig = Rig::ready("kill");
    let sent = rig.svc.send("nimbus", "work: add a readme").unwrap();
    rig.post(vec![json!({"v":1,"type":"helper_request","requestId":"h-1","conversationId":"app:dm:nimbus","task":"add a readme","project":"demo-a"})]);
    let _ = sent;
    assert_eq!(rig.svc.state().unwrap().bots[0].open_cards, 1);
    assert!(rig.brains.any_running());
    rig.svc.set_enabled(false);
    assert!(!rig.child(0).running(), "the process is gone");
    assert!(rig.child(0).killed.load(Ordering::SeqCst), "it did not leave by itself, so it was killed");
    assert!(!rig.brains.any_running(), "its token is forgotten");
    assert!(!rig.brains.enabled());
    assert_eq!(rig.svc.state().unwrap().bots[0].open_cards, 0, "the card is withdrawn");
    assert!(rig.thread().iter().any(|m| m.kind == MsgKind::HelperResult && m.text.contains("Withdrawn")));
    // turned on again: it works again, with a new token
    rig.svc.set_enabled(true);
    assert!(rig.svc.send("nimbus", "back").unwrap().delivered);
    assert_eq!(rig.spawned(), 2);
}

#[test]
fn stop_asks_the_brain_to_leave_and_the_next_message_starts_it_again() {
    let rig = Rig::ready("stop");
    rig.answer_all();
    rig.svc.stop_bot("nimbus");
    let (items, _) = rig.inbox(0);
    assert!(items.iter().any(|i| i["type"] == "shutdown"), "{items:?}");
    // it leaves by itself
    rig.child(0).exit_with(0);
    rig.svc.tick();
    assert!(!rig.brains.any_running());
    assert_eq!(rig.state(), BrainState::Idle);
    assert!(!rig.child(0).killed.load(Ordering::SeqCst));
    assert!(rig.notes().is_empty(), "a stop the person asked for is not an error");
    assert!(rig.svc.send("nimbus", "back").unwrap().delivered);
    assert_eq!(rig.spawned(), 2);
}

#[test]
fn a_brain_that_ignores_shutdown_is_killed_after_two_seconds() {
    let rig = Rig::ready("grace");
    rig.svc.stop_bot("nimbus");
    rig.advance(1_900);
    rig.svc.tick();
    assert!(rig.child(0).running());
    rig.advance(200);
    rig.svc.tick();
    assert!(rig.child(0).killed.load(Ordering::SeqCst));
    assert!(!rig.brains.any_running());
    assert_eq!(rig.state(), BrainState::Idle);
}

#[test]
fn thirty_idle_minutes_stop_the_brain_without_an_error() {
    let rig = Rig::ready("idle");
    rig.answer_all();
    rig.advance(30 * 60 * 1000 + 10);
    rig.svc.tick();
    let (items, _) = rig.inbox(0);
    assert!(items.iter().any(|i| i["type"] == "shutdown"));
    rig.child(0).exit_with(0);
    rig.svc.tick();
    assert_eq!(rig.state(), BrainState::Idle);
    assert!(rig.notes().is_empty());
    assert!(rig.svc.send("nimbus", "morning").unwrap().delivered);
    assert_eq!(rig.spawned(), 2);
}

#[test]
fn a_crash_is_restarted_after_a_wait_and_what_was_never_fetched_goes_to_the_next_brain() {
    let rig = Rig::ready("crash");
    // fetched and answered... then a new message that the brain never fetched
    let (_, cursor) = rig.inbox(0);
    let waiting = rig.svc.send("nimbus", "second").unwrap();
    // the brain dies without fetching it
    rig.child(0).exit_with(1);
    rig.advance(100);
    rig.svc.tick();
    assert_eq!(rig.state(), BrainState::Restarting);
    assert!(!rig.brains.any_running());
    assert_eq!(rig.spawned(), 1, "it waits before it starts again");
    rig.advance(1_000);
    rig.svc.tick();
    assert_eq!(rig.spawned(), 2);
    let (items, _) = rig.inbox(0);
    let sends: Vec<_> = items.iter().filter(|i| i["type"] == "send").collect();
    assert!(sends.iter().any(|i| i["id"] == waiting.id.as_str()), "the unfetched message is delivered to the new brain: {items:?} {cursor}");
}

#[test]
fn a_message_the_brain_had_fetched_but_not_answered_is_reported_not_repeated() {
    let rig = Rig::ready("lost");
    let lost = rig.svc.send("nimbus", "crash").unwrap();
    let (items, _) = rig.inbox(0);
    assert!(items.iter().any(|i| i["id"] == lost.id.as_str()), "the brain fetched it");
    rig.child(0).exit_with(1);
    rig.svc.tick();
    assert!(rig.notes().iter().any(|n| n == "Nimbus's brain stopped before it answered."), "{:?}", rig.notes());
    rig.advance(2_000);
    rig.svc.tick();
    let (items, _) = rig.inbox(0);
    assert!(!items.iter().any(|i| i["id"] == lost.id.as_str()), "a message that may have crashed it is not sent again");
}

#[test]
fn five_crashes_stop_it_for_good_until_start_again() {
    let rig = Rig::new("stopped");
    rig.svc.send("nimbus", "go").unwrap();
    for n in 0..5 {
        rig.hello();
        rig.child(n).exit_with(1);
        rig.advance(100);
        rig.svc.tick();
        rig.advance(10_000);
        rig.svc.tick();
    }
    assert_eq!(rig.state(), BrainState::Stopped);
    assert_eq!(rig.spawned(), 5);
    assert!(rig.notes().iter().any(|n| n == "Nimbus's brain stopped. Press Start again."), "{:?}", rig.notes());
    // a message does not start it; the person's "Start again" does
    let out = rig.svc.send("nimbus", "still there?").unwrap();
    assert_eq!((out.delivered, out.state), (false, BrainState::Stopped));
    assert_eq!(rig.spawned(), 5);
    rig.svc.start_bot("nimbus").unwrap();
    assert_eq!(rig.spawned(), 6);
}

#[test]
fn a_brain_that_never_says_hello_is_killed_after_ten_seconds() {
    let rig = Rig::new("nohello");
    rig.svc.send("nimbus", "hi").unwrap();
    rig.advance(9_000);
    rig.svc.tick();
    assert!(rig.child(0).running());
    rig.advance(1_500);
    rig.svc.tick();
    assert!(rig.child(0).killed.load(Ordering::SeqCst));
    assert_eq!(rig.state(), BrainState::Restarting);
}

#[test]
fn a_start_that_fails_is_a_crash_with_a_wait_not_a_loop() {
    let rig = Rig::new("spawnfail");
    rig.launcher.fail_spawn.store(true, Ordering::SeqCst);
    rig.svc.send("nimbus", "hi").unwrap();
    assert_eq!(rig.state(), BrainState::Restarting);
    assert!(!rig.brains.any_running(), "no token is left behind");
    rig.svc.tick();
    assert_eq!(rig.spawned(), 0);
}

#[test]
fn twenty_sends_a_minute_is_the_limit() {
    let rig = Rig::ready("rate");
    for n in 0..19 {
        rig.svc.send("nimbus", &format!("m{n}")).unwrap();
    }
    // 20 in the minute counting "hello"
    assert_eq!(rig.svc.send("nimbus", "one too many").unwrap_err().code, "POTATO_RATE");
    rig.advance(61_000);
    assert!(rig.svc.send("nimbus", "later").is_ok());
}

#[test]
fn an_empty_message_is_refused_and_an_unknown_bot_is_not_found() {
    let rig = Rig::new("empty");
    assert_eq!(rig.svc.send("nimbus", "   ").unwrap_err().code, "POTATO_EMPTY");
    assert_eq!(rig.svc.send("ghost", "hi").unwrap_err().code, "POTATO_BOT_NOT_FOUND");
    assert_eq!(rig.svc.messages("ghost", 5).unwrap_err().code, "POTATO_BOT_NOT_FOUND");
}

#[test]
fn a_helper_request_from_the_owner_becomes_a_card_with_the_folder_name_only() {
    let rig = Rig::ready("helper");
    rig.svc.send("nimbus", "work: add a readme to demo-a").unwrap();
    rig.post(vec![json!({"v":1,"type":"helper_request","requestId":"h-1","conversationId":"app:dm:nimbus","task":"add a readme","project":"C:\\Users\\someone\\demo-a"})]);
    let card = rig.thread().into_iter().find(|m| m.kind == MsgKind::HelperCard).expect("a card");
    assert_eq!(card.request_id.as_deref(), Some("h-1"));
    assert_eq!(card.text, "add a readme");
    assert_eq!(card.from.kind, FromKind::Bot);
    assert_eq!(card.project.as_deref(), Some("demo-a"), "a name only, never the path the bot wrote");
    assert_eq!(rig.svc.state().unwrap().bots[0].open_cards, 1);
    // no helper was started: nothing here can start one
    assert_eq!(rig.spawned(), 1);
}

#[test]
fn a_helper_request_from_a_turn_nobody_trusted_is_refused_with_no_card() {
    let rig = Rig::new("others");
    // The brain is running but no send from the owner came in this conversation, so no trusted turn is behind the request.
    rig.svc.start_bot("nimbus").unwrap();
    rig.hello();
    rig.post(vec![json!({"v":1,"type":"helper_request","requestId":"h-x","conversationId":"app:dm:nimbus","task":"delete everything"})]);
    assert!(rig.thread().iter().all(|m| m.kind != MsgKind::HelperCard));
    let (items, _) = rig.inbox(0);
    let update = items.iter().find(|i| i["type"] == "helper_update").expect("a helper_update");
    assert_eq!((update["requestId"].as_str(), update["status"].as_str()), (Some("h-x"), Some("refused")));
}

#[test]
fn at_most_three_helper_cards_are_open_per_bot() {
    let rig = Rig::ready("cards");
    rig.svc.send("nimbus", "work").unwrap();
    for n in 0..4 {
        rig.post(vec![json!({"v":1,"type":"helper_request","requestId":format!("h-{n}"),"conversationId":"app:dm:nimbus","task":format!("job {n}")})]);
    }
    assert_eq!(rig.svc.state().unwrap().bots[0].open_cards, 3);
    let (items, _) = rig.inbox(0);
    let update = items.iter().find(|i| i["type"] == "helper_update" && i["requestId"] == "h-3").expect("the fourth is declined");
    assert_eq!(update["status"], "declined");
}

#[test]
fn switching_the_persona_keeps_the_thread_adds_a_note_and_restarts_the_brain_with_the_new_pack() {
    let rig = Rig::ready("persona");
    rig.svc.set_persona("nimbus", "potato").unwrap();
    assert!(rig.notes().contains(&"Now talking as Potato.".to_string()));
    assert!(rig.thread().iter().any(|m| m.text == "hello"), "the thread stays");
    assert_eq!(store::load_bots(&rig.root).unwrap()[0].talk_persona, "potato");
    let (items, _) = rig.inbox(0);
    assert!(items.iter().any(|i| i["type"] == "shutdown"), "the brain restarts: its persona prefix is fixed at start");
    rig.child(0).exit_with(0);
    rig.svc.tick();
    rig.svc.send("nimbus", "hi as potato").unwrap();
    assert_eq!(rig.launcher.spawned.lock()[1].pack, "potato");
    // the same value again changes nothing and adds no note
    let before = rig.notes().len();
    rig.svc.set_persona("nimbus", "potato").unwrap();
    assert_eq!(rig.notes().len(), before);
    assert!(rig.svc.set_persona("nimbus", "wizard").is_err());
}

#[test]
fn forgetting_talk_memory_stops_the_brain_and_moves_its_folder_aside() {
    let rig = Rig::ready("forget");
    std::fs::write(rig.root.join("brain").join("nimbus").join("vault").join("journal.md"), "memory").unwrap();
    rig.svc.forget_talk_memory("nimbus").unwrap();
    assert!(!rig.child(0).running());
    assert!(!rig.root.join("brain").join("nimbus").exists());
    let removed: Vec<_> = std::fs::read_dir(rig.root.join(".removed")).unwrap().flatten().collect();
    assert_eq!(removed.len(), 1);
    assert!(rig.thread().iter().any(|m| m.text == "hello"), "the thread stays");
}

#[test]
fn a_new_bot_needs_a_companion_and_takes_the_default_persona() {
    let rig = Rig::new("addbot");
    assert_eq!(rig.svc.add_bot("ghost").unwrap_err().code, "POTATO_BOT_NOT_FOUND");
    rig.svc.set_settings(Settings { default_persona: "potato".into(), ..Settings::default() }).unwrap();
    let bot = rig.svc.add_bot("mochi").unwrap();
    assert_eq!(bot.talk_persona, "potato");
    assert_eq!(rig.svc.state().unwrap().bots.len(), 2);
}

#[test]
fn through_my_agents_starts_no_process() {
    let rig = Rig::new("agents");
    rig.svc.set_settings(Settings { brain: BrainKind::Agents, ..Settings::default() }).unwrap();
    let out = rig.svc.send("nimbus", "hi").unwrap();
    assert_eq!((out.delivered, out.state), (false, BrainState::Agents));
    assert_eq!(rig.spawned(), 0);
}

#[test]
fn state_never_carries_the_key_only_the_mask() {
    let rig = Rig::new("mask");
    let state = rig.svc.state().unwrap();
    let text = serde_json::to_string(&state).unwrap();
    assert!(!text.contains(KEY), "{text}");
    assert!(state.key.set);
    assert_eq!(state.key.last4.as_deref(), Some("7890"));
}

#[test]
fn the_go_brain_needs_no_key_and_gets_the_go_provider() {
    let rig = Rig::new("go");
    rig.host.secrets.lock().clear();
    rig.svc.set_settings(Settings { brain: BrainKind::Go, ..Settings::default() }).unwrap();
    assert!(rig.svc.send("nimbus", "hi").unwrap().delivered);
    let spawned = rig.launcher.spawned.lock();
    assert_eq!((spawned[0].provider.as_str(), spawned[0].has_key), ("opencode", false));
}
