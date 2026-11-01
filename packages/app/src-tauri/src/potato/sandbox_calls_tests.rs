//! The bot's computer on fakes: a `FakeSandbox` that serves one picture and records what it was asked, a clock
//! that is a number, and the real screen lease. What it shows: bad arguments cost nothing and touch nothing; the
//! Labs switches, the screen lease, the trust rules and the day's cap are checked in order; a model that cannot see
//! gets words; the answer is redacted; and through the engine a `sandbox_call` comes back as a `sandbox_result`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::protocol::{parse_event, Trust};
use super::sandbox::control::{Action, Button, Direction};
use super::sandbox::lease::ScreenLease;
use super::sandbox::{FakeSandbox, Observation, Recorded, SandboxError};
use super::sandbox_calls::*;
use super::service_tests::{FakeHost, Rig};
use super::store::{self, Settings};

const OWNER: Trust = Trust::OwnerPc;
const MIN: i64 = 60_000;

struct Fixture {
    calls: Calls,
    port: Port,
    fake: Arc<FakeSandbox>,
    host: Arc<FakeHost>,
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!("pm-potato-calls-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let host = Arc::new(FakeHost {
            now: AtomicI64::new(1_000),
            secrets: Mutex::new(HashMap::new()),
            changes: Mutex::new(Vec::new()),
            resources: Mutex::new(None),
            url: Mutex::new(None),
        });
        let fake = Arc::new(FakeSandbox::new());
        let port = Port { sandbox: fake.clone(), lease: Arc::new(Mutex::new(ScreenLease::new())) };
        Fixture { calls: Calls::new(root.clone(), host.clone()), port, fake, host, root }
    }

    fn now(&self) -> i64 {
        self.host.now.load(Ordering::SeqCst)
    }

    fn advance(&self, ms: i64) {
        self.host.now.fetch_add(ms, Ordering::SeqCst);
    }

    fn ctx(&self, bot: &str) -> Context {
        Context { bot: bot.to_string(), conversation: format!("app:dm:{bot}"), trust: OWNER, want_image: false }
    }

    fn call_as(&self, ctx: &Context, tool: &str, args: Value) -> Result<Value, CallError> {
        self.calls.execute(Some(&self.port), Gates { bot_on: true, sandbox_on: true }, ctx, tool, &args)
    }

    fn call(&self, tool: &str, args: Value) -> Result<Value, CallError> {
        self.call_as(&self.ctx("nimbus"), tool, args)
    }

    fn used_today(&self, bot: &str) -> u64 {
        let day = super::service::Host::local_day(&*self.host, self.now());
        super::cost::day_of(&self.root, &day, bot).unwrap().sandbox_actions
    }

    fn set_cap(&self, cap: u32) {
        store::save_settings(&self.root, &Settings { sandbox_actions_per_day: cap, ..Settings::default() }).unwrap();
    }
}

fn code(result: Result<Value, CallError>) -> &'static str {
    result.expect_err("the call must fail").code
}

// ---- reading the arguments -------------------------------------------------------------------------

fn parse(args: Value) -> Result<Request, CallError> {
    validate(Tool::ComputerAct, &args, None, &Options::default())
}

fn actions_of(args: Value) -> Vec<Action> {
    match parse(args).unwrap() {
        Request::Act { actions, .. } => actions,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_five_tools_are_the_five_names() {
    let names: Vec<&str> = [Tool::ComputerObserve, Tool::ComputerAct, Tool::BrowserNavigate, Tool::BrowserSnapshot, Tool::BrowserAct, Tool::AskPersonForScreen].iter().map(|t| t.name()).collect();
    assert_eq!(names, TOOL_NAMES);
    for name in TOOL_NAMES {
        assert_eq!(Tool::parse(name).unwrap().name(), name);
    }
    for other in ["", "computer", "shell", "ask_helper", "COMPUTER_ACT", "computer_act "] {
        assert!(Tool::parse(other).is_none(), "{other}");
    }
}

#[test]
fn every_kind_of_desktop_action_is_read_into_one_the_sandbox_takes() {
    let actions = actions_of(json!({ "actions": [
        { "kind": "click", "x": 10, "y": 20 },
        { "kind": "click", "x": 10.6, "y": 20.4, "button": "right" },
        { "kind": "click", "x": 1, "y": 2, "double": true },
        { "kind": "move", "x": 5, "y": 6 },
        { "kind": "drag", "x": 1, "y": 2, "to_x": 30, "to_y": 40 },
        { "kind": "type", "text": "hello world" },
        { "kind": "key", "key": "Return" },
        { "kind": "key", "key": "l", "modifiers": ["Control"] },
        { "kind": "key", "key": "t", "modifiers": ["ctrl", "SHIFT"] },
        { "kind": "scroll", "x": 100, "y": 200, "direction": "up", "amount": 5 },
        { "kind": "scroll", "x": 100, "y": 200 },
        { "kind": "wait", "ms": 250 },
        { "kind": "focus", "application": "Chrome", "uri": "https://example.com" },
        { "kind": "focus", "application": "terminal" },
    ]}));
    assert_eq!(
        actions,
        vec![
            Action::Click { x: 10, y: 20, button: Button::Left },
            Action::Click { x: 11, y: 20, button: Button::Right },
            Action::DoubleClick { x: 1, y: 2 },
            Action::Move { x: 5, y: 6 },
            Action::Drag { from: (1, 2), to: (30, 40) },
            Action::Type { text: "hello world".into() },
            Action::Key { combo: "Return".into() },
            Action::Key { combo: "ctrl+l".into() },
            Action::Key { combo: "ctrl+shift+t".into() },
            Action::Scroll { x: 100, y: 200, direction: Direction::Up, amount: 5 },
            Action::Scroll { x: 100, y: 200, direction: Direction::Down, amount: 3 },
            Action::Wait { ms: 250 },
            Action::Launch { app: "chrome".into(), arg: Some("https://example.com".into()) },
            Action::Launch { app: "terminal".into(), arg: None },
        ]
    );
}

#[test]
fn bad_desktop_actions_are_refused_with_a_reason_before_anything_is_touched() {
    let bad = |args: Value| -> CallError { parse(args).expect_err("must be refused") };
    // counts
    assert_eq!(bad(json!({ "actions": [] })).code, "BAD_ARGS");
    assert_eq!(bad(json!({})).code, "BAD_ARGS");
    assert_eq!(bad(json!({ "actions": "click" })).code, "BAD_ARGS");
    let list = |n: usize, step: Value| Value::Array(vec![step; n]);
    let one = json!({ "kind": "wait", "ms": 1 });
    assert!(parse(json!({ "actions": list(24, one.clone()) })).is_ok());
    assert_eq!(bad(json!({ "actions": list(25, one.clone()) })).code, "BAD_ARGS");
    // kinds
    for kind in ["down", "up", "press", "teleport", "", "Click"] {
        assert_eq!(bad(json!({ "actions": [{ "kind": kind, "x": 1, "y": 1 }] })).code, "BAD_ARGS", "{kind}");
    }
    assert!(bad(json!({ "actions": [{ "kind": "down", "x": 1, "y": 1 }] })).message.contains("drag"));
    // positions
    for step in [
        json!({ "kind": "click" }),
        json!({ "kind": "click", "x": 1 }),
        json!({ "kind": "click", "x": -1, "y": 0 }),
        json!({ "kind": "click", "x": 0, "y": 16385 }),
        json!({ "kind": "click", "x": "1", "y": 1 }),
        json!({ "kind": "click", "x": 1.0e300, "y": 1 }),
        json!({ "kind": "drag", "x": 1, "y": 1 }),
        json!({ "kind": "drag", "x": 1, "y": 1, "to_x": 5 }),
        json!({ "kind": "scroll", "x": 1 }),
        json!({ "kind": "move", "x": 1 }),
    ] {
        assert!(parse(json!({ "actions": [step.clone()] })).is_err(), "{step}");
    }
    // buttons, directions, modifiers, keys, text, apps
    for step in [
        json!({ "kind": "click", "x": 1, "y": 1, "button": "middle" }),
        json!({ "kind": "click", "x": 1, "y": 1, "button": "right", "double": true }),
        json!({ "kind": "scroll", "x": 1, "y": 1, "direction": "diagonal" }),
        json!({ "kind": "key" }),
        json!({ "kind": "key", "key": "" }),
        json!({ "kind": "key", "key": "a", "modifiers": ["hyper"] }),
        json!({ "kind": "key", "key": "a", "modifiers": "ctrl" }),
        json!({ "kind": "key", "key": "a b" }),
        json!({ "kind": "key", "key": "a;rm" }),
        json!({ "kind": "type", "text": "" }),
        json!({ "kind": "type" }),
        json!({ "kind": "type", "text": "a\u{0}b" }),
        json!({ "kind": "type", "text": "x".repeat(2001) }),
        json!({ "kind": "focus" }),
        json!({ "kind": "focus", "application": "" }),
        json!({ "kind": "focus", "application": "../bin/sh" }),
        json!({ "kind": "focus", "application": "Chrome Beta" }),
        json!({ "kind": "focus", "application": "chrome", "uri": "file:///etc/passwd" }),
        json!({ "kind": "focus", "application": "chrome", "uri": "http://127.0.0.1:47801/" }),
        json!({ "kind": "focus", "application": "files", "uri": "-rf" }),
    ] {
        assert!(parse(json!({ "actions": [step.clone()] })).is_err(), "{step}");
    }
    // too many steps for one request (a double click is three)
    let doubles = list(24, json!({ "kind": "click", "x": 1, "y": 1, "double": true }));
    assert_eq!(bad(json!({ "actions": doubles })).code, "BAD_ACTIONS");
    // the whole call is capped
    let big = list(24, json!({ "kind": "type", "text": "x".repeat(1990) }));
    assert_eq!(bad(json!({ "actions": big })).code, "BAD_ARGS");
}

#[test]
fn a_scroll_with_no_position_uses_the_pointer_of_the_last_look_and_needs_one() {
    let args = json!({ "actions": [{ "kind": "scroll", "direction": "down" }] });
    assert!(validate(Tool::ComputerAct, &args, None, &Options::default()).is_err());
    match validate(Tool::ComputerAct, &args, Some((7, 8)), &Options::default()).unwrap() {
        Request::Act { actions, .. } => assert_eq!(actions, vec![Action::Scroll { x: 7, y: 8, direction: Direction::Down, amount: 3 }]),
        other => panic!("{other:?}"),
    }
    // a move before it counts as the pointer
    let args = json!({ "actions": [{ "kind": "move", "x": 50, "y": 60 }, { "kind": "scroll", "amount": 100 }] });
    match validate(Tool::ComputerAct, &args, None, &Options::default()).unwrap() {
        Request::Act { actions, .. } => assert_eq!(actions[1], Action::Scroll { x: 50, y: 60, direction: Direction::Down, amount: 20 }),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_page_tools_check_the_address_and_the_actions() {
    let opts = Options::default();
    let nav = |url: &str| validate(Tool::BrowserNavigate, &json!({ "url": url }), None, &opts);
    assert_eq!(nav("https://example.com/a").unwrap(), Request::Navigate { url: "https://example.com/a".into() });
    for url in ["file:///etc/passwd", "http://localhost:3000", "http://127.0.0.1/", "http://192.168.0.1/", "ftp://a.example", "javascript:1", "chrome://settings"] {
        assert_eq!(nav(url).unwrap_err().code, "URL_NOT_ALLOWED", "{url}");
    }
    assert_eq!(validate(Tool::BrowserNavigate, &json!({}), None, &opts).unwrap_err().code, "BAD_ARGS");
    assert_eq!(validate(Tool::BrowserNavigate, &json!({ "url": 5 }), None, &opts).unwrap_err().code, "BAD_ARGS");
    // a test page inside the sandbox, only when the caller allows it
    let dev = Options { allow_local_pages: true };
    assert!(validate(Tool::BrowserNavigate, &json!({ "url": "http://127.0.0.1:8000/t.html" }), None, &dev).is_ok());
    assert!(validate(Tool::BrowserNavigate, &json!({ "url": "file:///etc/passwd" }), None, &dev).is_err());

    let act = |actions: Value| validate(Tool::BrowserAct, &json!({ "actions": actions }), None, &opts);
    match act(json!([{ "kind": "fill", "ref": "e1", "text": "x", "origin": "https://a.example", "roots": ["/"] }, { "kind": "click", "ref": "e2" }])).unwrap() {
        Request::BrowserAct { actions } => {
            assert_eq!(actions, vec![json!({ "kind": "fill", "ref": "e1", "text": "x" }), json!({ "kind": "click", "ref": "e2" })]);
        }
        other => panic!("{other:?}"),
    }
    for kind in ["fill_secret", "fill_card", "fill_pay_token"] {
        assert_eq!(act(json!([{ "kind": kind, "ref": "e1" }])).unwrap_err().code, "BAD_ACTIONS", "{kind}");
    }
    assert_eq!(validate(Tool::BrowserAct, &json!({}), None, &opts).unwrap_err().code, "BAD_ACTIONS");
}

#[test]
fn the_cost_of_a_call_is_its_actions_and_at_least_one() {
    assert_eq!(Request::Observe.cost(), 1);
    assert_eq!(Request::Snapshot.cost(), 1);
    assert_eq!(Request::Navigate { url: "https://example.com".into() }.cost(), 1);
    assert_eq!(Request::Act { actions: vec![Action::Wait { ms: 1 }; 7], show: true }.cost(), 7);
    assert_eq!(Request::BrowserAct { actions: vec![json!({}); 3] }.cost(), 3);
}

// ---- running a call --------------------------------------------------------------------------------

#[test]
fn observe_returns_words_for_a_model_that_cannot_see_and_a_picture_for_one_that_can() {
    let f = Fixture::new("observe");
    let text = f.call("computer_observe", json!({})).unwrap();
    assert!(text.get("image").is_none(), "no picture for a model without eyes");
    assert_eq!(text["width"], 160);
    assert_eq!(text["height"], 100);
    assert_eq!(text["pointer"], json!({ "x": 10, "y": 10 }));
    assert_eq!(text["activeWindow"], "Fake desktop");
    assert!(text["frameId"].as_str().unwrap().starts_with("f-"));
    assert_eq!(text["changed"], Value::Null, "the first look has nothing to compare with");
    assert!(text["note"].as_str().unwrap().contains("cannot see pictures"));
    assert!(text["note"].as_str().unwrap().contains("browser_snapshot"), "{}", text["note"]);

    let mut seeing = f.ctx("nimbus");
    seeing.want_image = true;
    let picture = f.call_as(&seeing, "computer_observe", json!({})).unwrap();
    assert_eq!(picture["image"]["mime"], "image/png");
    let png = base64_decode(picture["image"]["data"].as_str().unwrap());
    assert_eq!(png, FakeSandbox::picture().png);
    assert_eq!(picture["changed"], false, "the fake serves the same picture");
    assert_eq!(picture["frameId"], text["frameId"]);
    assert!(picture.get("note").is_none());
    assert_eq!(f.fake.calls(), vec![Recorded::Observe, Recorded::Observe], "no page tool for a window that is not a browser");
}

fn base64_decode(text: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(text).unwrap()
}

#[test]
fn the_front_chrome_tab_is_read_as_page_text_when_there_is_one() {
    let f = Fixture::new("page");
    let chrome = Observation { active_window_title: Some("Inbox - Google Chrome".into()), ..FakeSandbox::picture() };
    f.fake.set_browser_reply("{\"ok\":true,\"title\":\"Inbox\",\"text\":\"3 new messages\"}");
    let page = front_page_text(&*f.fake, &chrome).expect("a page");
    assert_eq!(page["text"], "3 new messages");
    assert_eq!(f.fake.calls(), vec![Recorded::Browser(vec!["snapshot".to_string()])]);
    // the page tool says no: words from the note instead
    f.fake.set_browser_reply("{\"ok\":false,\"fallback\":\"computer_act\"}");
    assert!(front_page_text(&*f.fake, &chrome).is_none());
    // not a browser: the page tool is not even asked
    let before = f.fake.calls().len();
    assert!(front_page_text(&*f.fake, &FakeSandbox::picture()).is_none());
    assert!(front_page_text(&*f.fake, &Observation { active_window_title: None, ..FakeSandbox::picture() }).is_none());
    assert_eq!(f.fake.calls().len(), before);
}

#[test]
fn a_picture_that_is_too_big_is_not_sent() {
    let f = Fixture::new("big");
    let big = Observation { png: vec![0u8; MAX_IMAGE_BYTES + 1], ..FakeSandbox::picture() };
    let out = f.calls.describe("nimbus@app:dm:nimbus", &big, true);
    assert!(!out.contains_key("image"));
    assert!(out["note"].as_str().unwrap().contains("too big"));
    let ok = Observation { png: vec![0u8; MAX_IMAGE_BYTES], ..FakeSandbox::picture() };
    assert!(f.calls.describe("nimbus@app:dm:nimbus", &ok, true).contains_key("image"));
}

#[test]
fn computer_act_runs_the_actions_in_order_and_says_whether_the_screen_changed() {
    let f = Fixture::new("act");
    let out = f
        .call("computer_act", json!({ "actions": [{ "kind": "focus", "application": "chrome" }, { "kind": "click", "x": 5, "y": 6 }, { "kind": "type", "text": "hi" }] }))
        .unwrap();
    assert_eq!(out["done"], 3);
    assert_eq!(out["activeWindow"], "Fake desktop");
    assert_eq!(
        f.fake.actions(),
        vec![Action::Launch { app: "chrome".into(), arg: None }, Action::Click { x: 5, y: 6, button: Button::Left }, Action::Type { text: "hi".into() }]
    );
    assert!(out.get("image").is_none());
    // observe:false leaves the picture out (the actions still ran)
    let quiet = f.call("computer_act", json!({ "actions": [{ "kind": "wait", "ms": 10 }], "observe": false })).unwrap();
    assert_eq!(quiet, json!({ "done": 1 }));
    assert_eq!(f.fake.actions().len(), 4);
    // the pointer the screen reported is remembered for a scroll with no position
    let scrolled = f.call("computer_act", json!({ "actions": [{ "kind": "scroll", "direction": "down" }] })).unwrap();
    assert_eq!(scrolled["done"], 1);
    assert_eq!(f.fake.actions().last().unwrap(), &Action::Scroll { x: 10, y: 10, direction: Direction::Down, amount: 3 });
}

#[test]
fn the_page_tools_go_to_the_sandbox_as_words_and_come_back_as_the_tools_answer() {
    let f = Fixture::new("browser");
    f.fake.set_browser_reply("{\"ok\":true,\"url\":\"https://example.com/\",\"title\":\"Example\"}");
    let nav = f.call("browser_navigate", json!({ "url": "https://example.com/" })).unwrap();
    assert_eq!(nav["title"], "Example");
    f.fake.set_browser_reply("{\"ok\":true,\"elements\":[{\"ref\":\"e1\"}]}");
    let snap = f.call("browser_snapshot", json!({})).unwrap();
    assert_eq!(snap["elements"][0]["ref"], "e1");
    f.fake.set_browser_reply("{\"ok\":false,\"error\":\"stale ref\",\"fallback\":\"computer_act\"}");
    let act = f.call("browser_act", json!({ "actions": [{ "kind": "click", "ref": "e1" }] })).unwrap();
    assert_eq!(act["ok"], false, "the page tool's own failure is its answer, not a failed call");
    assert_eq!(act["fallback"], "computer_act");
    let calls = f.fake.calls();
    assert_eq!(calls[0], Recorded::Browser(vec!["navigate".into(), "{\"url\":\"https://example.com/\"}".into()]));
    assert_eq!(calls[1], Recorded::Browser(vec!["snapshot".into()]));
    assert_eq!(calls[2], Recorded::Browser(vec!["act".into(), "{\"actions\":[{\"kind\":\"click\",\"ref\":\"e1\"}]}".into()]));
    // a refused address never reaches the sandbox
    assert_eq!(code(f.call("browser_navigate", json!({ "url": "http://localhost:47801/" }))), "URL_NOT_ALLOWED");
    assert_eq!(f.fake.calls().len(), 3);
}

#[test]
fn what_the_sandbox_says_is_redacted_but_the_picture_is_left_alone() {
    let f = Fixture::new("redact");
    let secret = "sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghij";
    f.fake.set_browser_reply(&format!("{{\"ok\":true,\"text\":\"your key is {secret}\"}}"));
    let snap = f.call("browser_snapshot", json!({})).unwrap();
    assert!(!snap.to_string().contains(secret), "{snap}");
    let mut seeing = f.ctx("nimbus");
    seeing.want_image = true;
    let picture = f.call_as(&seeing, "computer_observe", json!({})).unwrap();
    assert_eq!(base64_decode(picture["image"]["data"].as_str().unwrap()), FakeSandbox::picture().png);
}

#[test]
fn a_failed_call_is_an_error_with_the_sandbox_code() {
    let f = Fixture::new("fail");
    f.fake.fail_next(SandboxError::new("SANDBOX_UNREACHABLE", "The sandbox screen is not answering."));
    let err = f.call("computer_observe", json!({})).unwrap_err();
    assert_eq!((err.code, err.needs_you), ("SANDBOX_UNREACHABLE", false));
    assert_eq!(err.message, "The sandbox screen is not answering.");
    // and the next call works (the failure was once)
    assert!(f.call("computer_observe", json!({})).is_ok());
}

// ---- the switches, the rules, the lease, the cap -----------------------------------------------------

#[test]
fn the_labs_switches_and_the_wiring_are_checked_first() {
    let f = Fixture::new("gates");
    let ctx = f.ctx("nimbus");
    let run = |gates: Gates, port: Option<&Port>, tool: &str| f.calls.execute(port, gates, &ctx, tool, &json!({}));
    let on = Gates { bot_on: true, sandbox_on: true };
    assert_eq!(code(run(Gates { bot_on: false, sandbox_on: true }, Some(&f.port), "computer_observe")), "POTATO_OFF");
    assert_eq!(code(run(Gates { bot_on: true, sandbox_on: false }, Some(&f.port), "computer_observe")), "SANDBOX_OFF");
    assert_eq!(code(run(Gates { bot_on: false, sandbox_on: false }, Some(&f.port), "computer_observe")), "POTATO_OFF");
    assert_eq!(code(run(on, None, "computer_observe")), "SANDBOX_NOT_SET_UP");
    assert_eq!(code(run(on, Some(&f.port), "shell")), "UNKNOWN_TOOL");
    assert_eq!(code(run(on, Some(&f.port), "ask_helper")), "UNKNOWN_TOOL");
    assert!(f.fake.calls().is_empty(), "nothing touched the sandbox");
    assert_eq!(f.used_today("nimbus"), 0, "and nothing was counted");
    // a bad argument is refused before the lease is taken or an action is counted
    assert_eq!(code(f.call("computer_act", json!({ "actions": [] }))), "BAD_ARGS");
    assert_eq!(f.used_today("nimbus"), 0);
    assert!(!f.calls.bot_is_using(&f.port, "nimbus"));
    // arguments that are not an object
    let weird = f.calls.execute(Some(&f.port), on, &ctx, "computer_observe", &json!("hello"));
    assert_eq!(code(weird), "BAD_ARGS");
    let huge = json!({ "pad": "x".repeat(30_000) });
    assert_eq!(code(f.calls.execute(Some(&f.port), on, &ctx, "computer_observe", &huge)), "BAD_ARGS");
}

#[test]
fn anyone_may_use_the_sandbox_and_only_the_owner_may_start_a_helper() {
    use super::rules::tool_access;
    for trust in [Trust::OwnerPc, Trust::OwnerRemote] {
        let access = tool_access(trust);
        assert!(access.helpers && access.sandbox, "{trust:?}");
    }
    let others = tool_access(Trust::Others);
    assert!(!others.helpers, "an outside voice can never start a helper");
    assert!(!others.sandbox, "and the computer is never offered to an outside voice (H6b-8)");

    let f = Fixture::new("others");
    for trust in [Trust::OwnerPc, Trust::OwnerRemote] {
        let mut ctx = f.ctx(&format!("bot-{trust:?}").to_lowercase());
        ctx.trust = trust;
        assert!(f.call_as(&ctx, "computer_observe", json!({})).is_ok(), "{trust:?}");
    }
}

#[test]
fn the_bot_takes_the_screen_and_keeps_it_for_the_run() {
    let f = Fixture::new("lease");
    f.call("computer_observe", json!({})).unwrap();
    assert!(f.calls.bot_is_using(&f.port, "nimbus"));
    let lease = f.port.lease.lock().current(f.now()).cloned().unwrap();
    assert_eq!(lease.run_id, "nimbus@app:dm:nimbus");
    // using it again renews the same lease, it does not take a new one
    f.advance(10 * MIN);
    f.call("computer_observe", json!({})).unwrap();
    f.advance(10 * MIN);
    f.call("computer_observe", json!({})).unwrap();
    assert_eq!(f.port.lease.lock().current(f.now()).unwrap().id, lease.id);
    // the turn ending lets go of it
    f.calls.end_run(Some(&f.port), "nimbus", "app:dm:nimbus");
    assert!(!f.calls.bot_is_using(&f.port, "nimbus"));
    assert!(f.port.lease.lock().current(f.now()).is_none());
}

#[test]
fn the_person_holding_the_screen_stops_the_bot_and_costs_it_nothing() {
    let f = Fixture::new("person");
    f.port.lease.lock().take_over(f.now());
    let err = f.call("computer_observe", json!({})).unwrap_err();
    assert_eq!(err.code, "LEASE_HELD_BY_PERSON");
    assert!(err.needs_you, "the answer carries the Needs you option");
    assert!(err.message.contains("waiting"));
    assert!(f.fake.calls().is_empty());
    assert_eq!(f.used_today("nimbus"), 0);
    // the person hands it back: the bot goes on
    let handed = f.port.lease.lock().hand_back(f.now());
    assert!(handed.is_none(), "nobody was displaced");
    assert!(f.call("computer_observe", json!({})).is_ok());
    assert_eq!(f.used_today("nimbus"), 1);
}

#[test]
fn a_take_over_while_the_bot_works_is_lease_lost_once_then_the_person_holds_it() {
    let f = Fixture::new("takeover");
    f.call("computer_observe", json!({})).unwrap();
    // the person takes over in the middle of the run
    let (_, displaced) = f.port.lease.lock().take_over(f.now());
    assert_eq!(displaced.unwrap().bot, "nimbus");
    let err = f.call("computer_act", json!({ "actions": [{ "kind": "click", "x": 1, "y": 1 }] })).unwrap_err();
    assert_eq!(err.code, "LEASE_HELD_BY_PERSON", "the bot is told the person has it, not that it may carry on");
    assert!(f.fake.actions().is_empty());
    // the person hands it back; the bot's old lease is dead: it must look again before acting
    f.port.lease.lock().hand_back(f.now());
    let lost = f.call("computer_act", json!({ "actions": [{ "kind": "click", "x": 1, "y": 1 }] })).unwrap_err();
    assert_eq!(lost.code, "LEASE_LOST");
    assert!(!lost.needs_you);
    assert!(lost.message.contains("again"));
    assert!(f.fake.actions().is_empty(), "the stale action did not run");
    assert_eq!(f.used_today("nimbus"), 1, "and was not counted");
    // the next call goes on with the new lease
    assert!(f.call("computer_act", json!({ "actions": [{ "kind": "click", "x": 1, "y": 1 }] })).is_ok());
    assert_eq!(f.fake.actions().len(), 1);
}

#[test]
fn a_lease_that_ran_out_is_lease_lost_once() {
    let f = Fixture::new("expiry");
    f.call("computer_observe", json!({})).unwrap();
    f.advance(16 * MIN);
    assert_eq!(code(f.call("computer_observe", json!({}))), "LEASE_LOST");
    assert!(f.call("computer_observe", json!({})).is_ok());
}

#[test]
fn a_newer_run_takes_the_screen_from_an_older_bot_run_but_never_from_the_person() {
    let f = Fixture::new("two");
    f.call_as(&f.ctx("nimbus"), "computer_observe", json!({})).unwrap();
    f.call_as(&f.ctx("mochi"), "computer_observe", json!({})).unwrap();
    assert!(f.calls.bot_is_using(&f.port, "mochi") && !f.calls.bot_is_using(&f.port, "nimbus"));
    // the brain of the first bot going away ends only its own runs
    f.calls.end_bot(Some(&f.port), "nimbus");
    assert!(f.calls.bot_is_using(&f.port, "mochi"));
    f.calls.end_bot(Some(&f.port), "mochi");
    assert!(f.port.lease.lock().current(f.now()).is_none());
}

#[test]
fn the_needs_you_card_is_raised_by_the_bot_that_holds_the_screen() {
    let f = Fixture::new("needs");
    assert_eq!(f.calls.needs_you(&f.port, "nimbus", "app:dm:nimbus", "a code is needed").unwrap_err().code, "LEASE_NOT_HOLDER");
    f.call("computer_observe", json!({})).unwrap();
    let card = f.calls.needs_you(&f.port, "nimbus", "app:dm:nimbus", "a code is needed").unwrap();
    assert_eq!((card.bot.as_str(), card.reason.as_str()), ("nimbus", "a code is needed"));
    assert!(matches!(f.port.lease.lock().banner(f.now()), super::sandbox::lease::Banner::NeedsYou { .. }));
}

#[test]
fn the_day_has_an_action_cap_and_a_call_that_would_pass_it_is_refused_whole() {
    let f = Fixture::new("cap");
    f.set_cap(10);
    let six = Value::Array((0..6).map(|i| json!({ "kind": "move", "x": i, "y": i })).collect());
    f.call("computer_act", json!({ "actions": six.clone() })).unwrap();
    assert_eq!(f.used_today("nimbus"), 6);
    // 6 more would be 12: refused, nothing ran, nothing counted
    let err = f.call("computer_act", json!({ "actions": six })).unwrap_err();
    assert_eq!(err.code, "SANDBOX_DAILY_CAP");
    assert!(err.message.contains("6 of its 10"), "{}", err.message);
    assert_eq!(f.fake.actions().len(), 6);
    assert_eq!(f.used_today("nimbus"), 6);
    // a smaller call still fits, up to the cap exactly
    assert!(f.call("computer_observe", json!({})).is_ok());
    f.call("computer_act", json!({ "actions": [{ "kind": "wait", "ms": 1 }, { "kind": "wait", "ms": 1 }, { "kind": "wait", "ms": 1 }] })).unwrap();
    assert_eq!(f.used_today("nimbus"), 10);
    assert_eq!(code(f.call("computer_observe", json!({}))), "SANDBOX_DAILY_CAP");
    // the cap is per bot
    assert!(f.call_as(&f.ctx("mochi"), "computer_observe", json!({})).is_ok());
    // and it starts again with the day
    f.advance(24 * 60 * MIN);
    assert_eq!(code(f.call("computer_observe", json!({}))), "LEASE_LOST", "a day later the old hold has run out");
    assert!(f.call("computer_observe", json!({})).is_ok());
    assert_eq!(f.used_today("nimbus"), 1);
}

#[test]
fn the_default_cap_is_500_and_the_setting_is_kept_inside_its_limits() {
    assert_eq!(Settings::default().sandbox_actions_per_day, 500);
    let f = Fixture::new("cap-setting");
    store::save_settings(&f.root, &Settings { sandbox_actions_per_day: 1, ..Settings::default() }).unwrap();
    assert_eq!(store::load_settings(&f.root).unwrap().sandbox_actions_per_day, super::cost::MIN_SANDBOX_ACTIONS);
    store::save_settings(&f.root, &Settings { sandbox_actions_per_day: 1_000_000, ..Settings::default() }).unwrap();
    assert_eq!(store::load_settings(&f.root).unwrap().sandbox_actions_per_day, super::cost::MAX_SANDBOX_ACTIONS);
    // an old settings file with no such field reads as the default
    std::fs::write(store::settings_path(&f.root), "{\"v\":1,\"brain\":\"deepseek\",\"defaultPersona\":\"fish\",\"dailyLimitCents\":50}").unwrap();
    assert_eq!(store::load_settings(&f.root).unwrap().sandbox_actions_per_day, 500);
}

#[test]
fn the_count_survives_a_restart() {
    let f = Fixture::new("restart");
    f.set_cap(10);
    f.call("computer_act", json!({ "actions": Value::Array((0..8).map(|i| json!({ "kind": "move", "x": i, "y": 0 })).collect()) })).unwrap();
    // a new dispatcher on the same folder (the app restarted) still knows
    let again = Calls::new(f.root.clone(), f.host.clone());
    let ctx = f.ctx("nimbus");
    let args = json!({ "actions": [{ "kind": "move", "x": 1, "y": 1 }, { "kind": "move", "x": 2, "y": 2 }, { "kind": "move", "x": 3, "y": 3 }] });
    let err = again.execute(Some(&f.port), Gates { bot_on: true, sandbox_on: true }, &ctx, "computer_act", &args).unwrap_err();
    assert_eq!(err.code, "SANDBOX_DAILY_CAP");
}

// ---- through the engine ------------------------------------------------------------------------------

/// A rig whose sandbox calls run at once (no thread), with the computer wired in and switched on.
fn engine(name: &str) -> (Rig, Arc<FakeSandbox>, Arc<Mutex<ScreenLease>>) {
    let rig = Rig::new(name);
    rig.svc.set_spawner(Arc::new(|job| job()));
    let fake = Arc::new(FakeSandbox::new());
    let lease = Arc::new(Mutex::new(ScreenLease::new()));
    rig.svc.set_sandbox_port(Some(Port { sandbox: fake.clone(), lease: lease.clone() }));
    rig.svc.set_sandbox_enabled(true);
    (rig, fake, lease)
}

fn call_event(id: &str, tool: &str, args: Value, image: bool) -> Value {
    json!({"v":1,"type":"sandbox_call","callId":id,"conversationId":"app:dm:nimbus","tool":tool,"args":args,"image":image})
}

fn results(rig: &Rig) -> Vec<Value> {
    rig.inbox(0).0.into_iter().filter(|i| i["type"] == "sandbox_result").collect()
}

#[test]
fn a_send_tells_the_brain_whether_it_has_a_computer() {
    // off by default: the field is there and false
    let rig = Rig::new("caps-off");
    rig.svc.send("nimbus", "hi").unwrap();
    assert_eq!(rig.inbox(0).0[0]["capabilities"], json!({ "sandbox": false }));
    // on, but no sandbox wired in: still false (the tools would only fail)
    let rig = Rig::new("caps-nowire");
    rig.svc.set_sandbox_enabled(true);
    assert!(!rig.svc.sandbox_offered());
    rig.svc.send("nimbus", "hi").unwrap();
    assert_eq!(rig.inbox(0).0[0]["capabilities"]["sandbox"], false);
    // wired in but the switch is off: false
    let (rig, _, _) = engine("caps-switch");
    rig.svc.set_sandbox_enabled(false);
    rig.svc.send("nimbus", "hi").unwrap();
    assert_eq!(rig.inbox(0).0[0]["capabilities"]["sandbox"], false);
    // both: true, and the kill switch beats it
    let (rig, _, _) = engine("caps-on");
    assert!(rig.svc.sandbox_offered());
    rig.svc.send("nimbus", "hi").unwrap();
    assert_eq!(rig.inbox(0).0[0]["capabilities"], json!({ "sandbox": true }));
    rig.brains.set_enabled(false);
    assert!(!rig.svc.sandbox_offered());
}

#[test]
fn a_sandbox_call_comes_back_as_a_sandbox_result_in_the_inbox() {
    let (rig, fake, _) = engine("roundtrip");
    rig.svc.send("nimbus", "look at the screen").unwrap();
    rig.hello();
    let before = rig.inbox(0).1;
    rig.post(vec![call_event("s-1", "computer_observe", json!({}), false)]);
    let (items, _) = rig.inbox(before);
    assert_eq!(items.len(), 1, "{items:?}");
    let item = &items[0];
    assert_eq!((item["v"].clone(), item["type"].clone(), item["callId"].clone(), item["ok"].clone()), (json!(1), json!("sandbox_result"), json!("s-1"), json!(true)));
    assert_eq!(item["result"]["activeWindow"], "Fake desktop");
    assert_eq!(fake.calls(), vec![Recorded::Observe]);

    // a refusal is a result too, so the brain never waits for nothing
    let at = rig.inbox(0).1;
    rig.post(vec![call_event("s-2", "browser_navigate", json!({ "url": "file:///etc/passwd" }), false)]);
    let (items, _) = rig.inbox(at);
    assert_eq!(items[0]["callId"], "s-2");
    assert_eq!(items[0]["ok"], false);
    assert_eq!(items[0]["error"]["code"], "URL_NOT_ALLOWED");
    assert!(items[0]["error"].get("needsYou").is_none());
    assert_eq!(fake.calls().len(), 1);
}

#[test]
fn the_image_flag_of_the_call_decides_whether_a_picture_comes_back() {
    let (rig, _, _) = engine("image");
    rig.svc.send("nimbus", "look").unwrap();
    rig.hello();
    rig.post(vec![call_event("s-1", "computer_observe", json!({}), true), call_event("s-2", "computer_observe", json!({}), false)]);
    let all = results(&rig);
    let by = |id: &str| all.iter().find(|i| i["callId"] == id).unwrap().clone();
    assert_eq!(by("s-1")["result"]["image"]["mime"], "image/png");
    assert!(by("s-2")["result"].get("image").is_none());
}

#[test]
fn the_person_holding_the_screen_is_an_error_result_with_needs_you() {
    let (rig, fake, lease) = engine("person");
    rig.svc.send("nimbus", "go").unwrap();
    rig.hello();
    lease.lock().take_over(rig.host.now.load(Ordering::SeqCst));
    rig.post(vec![call_event("s-1", "computer_act", json!({ "actions": [{ "kind": "click", "x": 1, "y": 1 }] }), false)]);
    let item = results(&rig).remove(0);
    assert_eq!((item["ok"].clone(), item["error"]["code"].clone(), item["error"]["needsYou"].clone()), (json!(false), json!("LEASE_HELD_BY_PERSON"), json!(true)));
    assert!(fake.calls().is_empty());
}

#[test]
fn the_switches_are_judged_when_the_call_runs_not_when_the_send_was_made() {
    let (rig, fake, _) = engine("late-off");
    rig.svc.send("nimbus", "go").unwrap();
    rig.hello();
    rig.svc.set_sandbox_enabled(false);
    rig.post(vec![call_event("s-1", "computer_observe", json!({}), false)]);
    let item = results(&rig).remove(0);
    assert_eq!(item["error"]["code"], "SANDBOX_OFF");
    assert!(fake.calls().is_empty());
    // unwired
    rig.svc.set_sandbox_enabled(true);
    rig.svc.set_sandbox_port(None);
    rig.post(vec![call_event("s-2", "computer_observe", json!({}), false)]);
    let all = results(&rig);
    assert_eq!(all.iter().find(|i| i["callId"] == "s-2").unwrap()["error"]["code"], "SANDBOX_NOT_SET_UP");
}

#[test]
fn a_call_for_another_bots_conversation_gets_no_answer() {
    let (rig, fake, _) = engine("foreign");
    rig.svc.send("nimbus", "go").unwrap();
    rig.hello();
    rig.post(vec![json!({"v":1,"type":"sandbox_call","callId":"s-1","conversationId":"app:dm:mochi","tool":"computer_observe","args":{},"image":false})]);
    assert!(results(&rig).is_empty());
    assert!(fake.calls().is_empty());
}

#[test]
fn the_trust_of_a_call_is_the_one_the_engine_recorded_for_the_turn() {
    let (rig, fake, _) = engine("trust");
    rig.svc.send("nimbus", "go").unwrap();
    rig.hello();
    // the app sent this turn as the person at the PC; the call cannot claim anything else (it has no such field)
    let event = json!({"v":1,"type":"sandbox_call","callId":"s-1","conversationId":"app:dm:nimbus","tool":"computer_observe","args":{},"trust":"others"});
    rig.post(vec![event]);
    assert_eq!(results(&rig)[0]["ok"], true);
    assert_eq!(fake.calls().len(), 1);
    let parsed = parse_event(&call_event("s-9", "computer_act", json!({}), false)).unwrap();
    assert!(matches!(parsed, super::protocol::Event::SandboxCall { .. }));
}

#[test]
fn the_reply_ends_the_run_and_a_gone_brain_lets_go_of_the_screen() {
    let (rig, _, lease) = engine("ends");
    let sent = rig.svc.send("nimbus", "go").unwrap();
    rig.hello();
    rig.post(vec![call_event("s-1", "computer_observe", json!({}), false)]);
    let now = rig.host.now.load(Ordering::SeqCst);
    assert!(lease.lock().current(now).is_some());
    rig.post(vec![json!({"v":1,"type":"reply","conversationId":"app:dm:nimbus","inReplyTo":sent.id,"segments":["done"]})]);
    assert!(lease.lock().current(now).is_none(), "the turn is over");
    // again, then the brain goes away
    rig.svc.send("nimbus", "more").unwrap();
    rig.post(vec![call_event("s-2", "computer_observe", json!({}), false)]);
    assert!(lease.lock().current(now).is_some());
    rig.svc.stop_all(std::time::Duration::ZERO);
    assert!(lease.lock().current(now).is_none());
    assert_eq!(rig.svc.calls().bot_is_using(&Port { sandbox: Arc::new(FakeSandbox::new()), lease: lease.clone() }, "nimbus"), false);
}

#[test]
fn a_call_is_answered_off_the_posting_thread_by_default() {
    // The default spawner is a thread: the post returns at once and the result shows up in the inbox soon after.
    let rig = Rig::new("thread");
    let fake = Arc::new(FakeSandbox::new());
    rig.svc.set_sandbox_port(Some(Port { sandbox: fake.clone(), lease: Arc::new(Mutex::new(ScreenLease::new())) }));
    rig.svc.set_sandbox_enabled(true);
    rig.svc.send("nimbus", "go").unwrap();
    rig.hello();
    let at = rig.inbox(0).1;
    rig.post(vec![call_event("s-1", "computer_observe", json!({}), false)]);
    let started = std::time::Instant::now();
    loop {
        let (items, _) = rig.inbox(at);
        if items.iter().any(|i| i["type"] == "sandbox_result") {
            break;
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "no result");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn an_older_brain_that_does_not_know_sandbox_results_is_not_harmed_and_a_newer_event_is_ignored_by_an_older_app() {
    // protocol side: the field `capabilities` is extra on a send; the result is a new type
    let rig = Rig::new("compat");
    rig.svc.send("nimbus", "hi").unwrap();
    let send = &rig.inbox(0).0[0];
    for key in ["id", "conversationId", "surface", "from", "text", "tools", "v", "type"] {
        assert!(send.get(key).is_some(), "{key} stays");
    }
    // a post with a sandbox_call whose fields are missing is dropped on its own, not the whole post
    let events: Vec<Value> = vec![json!({"v":1,"type":"sandbox_call"}), json!({"v":1,"type":"sandbox_call","callId":"","conversationId":"c","tool":"t"}), json!({"v":1,"type":"typing","conversationId":"app:dm:nimbus","on":true})];
    let parsed: Vec<_> = events.iter().filter_map(parse_event).collect();
    assert_eq!(parsed.len(), 1);
}

// ---- guards ------------------------------------------------------------------------------------------

fn production(file: &str) -> String {
    let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("potato").join(file)).unwrap();
    text.split("#[cfg(test)]").next().unwrap_or("").lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n")
}

#[test]
fn the_dispatcher_starts_no_process_and_reaches_the_sandbox_only_through_the_trait() {
    for file in ["sandbox_calls.rs", "sandbox/browser.rs"] {
        let code = production(file);
        for needle in ["Command::", "hidden_command", "std::process", "wsl.exe", "powershell", "tracing::", "println!", "eprintln!", "dbg!", "fs::write", "File::create"] {
            assert!(!code.contains(needle), "{file} uses {needle}");
        }
    }
    // the sandbox is only ever called as the trait, never as the real one
    let calls = production("sandbox_calls.rs");
    assert!(!calls.contains("WslSandbox") && !calls.contains("Wsl::") && !calls.contains("Lifecycle"), "the dispatcher knows the trait only");
}

#[test]
fn the_trust_of_a_call_never_comes_from_the_brain_and_the_brain_cannot_name_a_caller_other_than_itself() {
    // The event has no trust field, and the engine reads the recorded trust of the turn.
    let protocol = production("protocol.rs");
    let at = protocol.find("SandboxCall {").expect("the event");
    let decl = &protocol[at..at + 200];
    assert!(!decl.contains("trust"), "{decl}");
    // (service.rs has test-only helpers early on, so its whole text is read)
    let service = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("potato").join("service.rs")).unwrap();
    assert!(service.contains("rt.turn_trust.get(&conversation_id)"), "the engine looks the trust up");
    assert!(service.contains("if !self.own_conversation(bot, &conversation_id)"), "a call is for the brain's own bot only");
    // anything the model could make the dispatcher do goes through the same tool list
    assert_eq!(TOOL_NAMES.len(), 6);
}

// ---- the first navigation of a fresh sandbox starts Chrome

const NO_CHROME: &str = "{\"ok\":false,\"error\":\"CDP not ready on port 9320: <urlopen error [Errno 111] Connection refused>\",\"fallback\":\"computer_act\"}";

#[test]
fn the_first_navigation_starts_chrome_and_asks_again() {
    let f = Fixture::new("nav-start");
    f.fake.queue_browser_replies(&[NO_CHROME, "{\"ok\":true,\"url\":\"https://example.com/\",\"title\":\"Example Domain\"}"]);
    let answer = f.call("browser_navigate", json!({ "url": "https://example.com" })).unwrap();
    assert_eq!(answer["title"], "Example Domain");
    let kinds: Vec<&str> = f.fake.calls().iter().map(|c| match c { Recorded::Browser(_) => "browser", Recorded::Act(_) => "act", _ => "other" }).collect();
    assert_eq!(kinds, ["browser", "act", "browser"], "ask, start Chrome, ask again");
    assert_eq!(f.fake.actions(), vec![Action::Launch { app: "chrome".to_string(), arg: None }, Action::Wait { ms: 3000 }]);
}

#[test]
fn a_navigation_that_works_starts_nothing_and_a_second_failure_is_told_as_it_is() {
    let f = Fixture::new("nav-plain");
    f.fake.set_browser_reply("{\"ok\":true,\"title\":\"Fine\"}");
    f.call("browser_navigate", json!({ "url": "https://example.com" })).unwrap();
    assert!(f.fake.actions().is_empty(), "Chrome was up: nothing was started");
    // still no Chrome after the start: the bot gets the page tool's own answer (with its fallback), not an endless loop
    let g = Fixture::new("nav-twice");
    g.fake.set_browser_reply(NO_CHROME);
    let answer = g.call("browser_navigate", json!({ "url": "https://example.com" })).unwrap();
    assert_eq!((answer["ok"].clone(), answer["fallback"].clone()), (json!(false), json!("computer_act")));
    assert_eq!(g.fake.calls().iter().filter(|c| matches!(c, Recorded::Browser(_))).count(), 2, "asked twice, no more");
    // other page errors do not start anything
    let h = Fixture::new("nav-other");
    h.fake.set_browser_reply("{\"ok\":false,\"error\":\"navigation timed out\"}");
    h.call("browser_navigate", json!({ "url": "https://example.com" })).unwrap();
    assert!(h.fake.actions().is_empty());
}

#[test]
fn only_navigation_starts_chrome_a_snapshot_with_no_browser_says_so() {
    let f = Fixture::new("snap-nochrome");
    f.fake.set_browser_reply(NO_CHROME);
    let answer = f.call("browser_snapshot", json!({})).unwrap();
    assert_eq!(answer["fallback"], "computer_act");
    assert!(f.fake.actions().is_empty());
}

// ---- ask_person_for_screen: the sixth tool --------------------------------------------------------------

#[test]
fn ask_person_for_screen_needs_one_short_reason() {
    let opts = Options::default();
    for args in [json!({}), json!({ "reason": "" }), json!({ "reason": "   " }), json!({ "reason": 5 })] {
        assert_eq!(validate(Tool::AskPersonForScreen, &args, None, &opts).unwrap_err().code, "BAD_ARGS", "{args}");
    }
    let long = "x".repeat(MAX_ASK_REASON_CHARS + 1);
    assert_eq!(validate(Tool::AskPersonForScreen, &json!({ "reason": long }), None, &opts).unwrap_err().code, "BAD_ARGS");
    match validate(Tool::AskPersonForScreen, &json!({ "reason": "  A login code is needed  " }), None, &opts).unwrap() {
        Request::AskPerson { reason } => assert_eq!(reason, "A login code is needed"),
        other => panic!("{other:?}"),
    }
    assert_eq!(Request::AskPerson { reason: "r".into() }.cost(), 0);
}

#[test]
fn a_bot_that_is_stuck_raises_the_needs_you_card_and_it_costs_nothing() {
    let f = Fixture::new("ask");
    f.call("computer_observe", json!({})).unwrap();
    assert_eq!(f.used_today("nimbus"), 1);
    let answer = f.call("ask_person_for_screen", json!({ "reason": "The site wants a code sent to your phone" })).unwrap();
    assert_eq!((answer["asked"].clone(), answer["expiresInMinutes"].clone()), (json!(true), json!(30)));
    assert!(answer["cardId"].as_str().is_some_and(|id| id.starts_with("needs-")), "{answer}");
    // the Computer panel's lease table now says "Needs you"
    match f.port.lease.lock().banner(f.now()) {
        super::sandbox::lease::Banner::NeedsYou { .. } => {}
        other => panic!("{other:?}"),
    }
    assert_eq!(f.port.lease.lock().card(f.now()).unwrap().reason, "The site wants a code sent to your phone");
    // it ran no program and spent no action
    assert_eq!(f.fake.calls(), vec![Recorded::Observe]);
    assert_eq!(f.used_today("nimbus"), 1);
}

#[test]
fn skip_turns_the_card_down_and_the_bot_goes_on() {
    let f = Fixture::new("ask-skip");
    f.call("computer_observe", json!({})).unwrap();
    let answer = f.call("ask_person_for_screen", json!({ "reason": "Please solve the check" })).unwrap();
    let card_id = answer["cardId"].as_str().unwrap().to_string();
    f.port.lease.lock().skip(&card_id).unwrap();
    assert!(f.port.lease.lock().card(f.now()).is_none());
    assert!(f.call("computer_observe", json!({})).is_ok(), "the bot keeps its hold after a Skip");
}

#[test]
fn asking_with_no_hold_yet_takes_the_hold_first() {
    let f = Fixture::new("ask-first");
    let answer = f.call("ask_person_for_screen", json!({ "reason": "I cannot find the app" })).unwrap();
    assert_eq!(answer["asked"], true);
    assert!(f.calls.bot_is_using(&f.port, "nimbus"));
    assert!(f.fake.calls().is_empty());
}

#[test]
fn when_the_person_already_has_the_screen_nothing_is_raised() {
    let f = Fixture::new("ask-person");
    f.port.lease.lock().take_over(f.now());
    let answer = f.call("ask_person_for_screen", json!({ "reason": "Please help" })).unwrap();
    assert_eq!(answer["asked"], false);
    assert!(answer["note"].as_str().unwrap().contains("already has the screen"));
    assert!(f.port.lease.lock().card(f.now()).is_none());
    assert!(f.port.lease.lock().current(f.now()).is_some_and(|l| l.holder == super::sandbox::lease::Holder::Person), "the person still holds it");
}

#[test]
fn ask_person_for_screen_follows_the_same_gates_as_the_other_tools() {
    let f = Fixture::new("ask-gates");
    let args = json!({ "reason": "Please help" });
    let ctx = f.ctx("nimbus");
    let off = f.calls.execute(Some(&f.port), Gates { bot_on: true, sandbox_on: false }, &ctx, "ask_person_for_screen", &args);
    assert_eq!(code(off), "SANDBOX_OFF");
    let none = f.calls.execute(None, Gates { bot_on: true, sandbox_on: true }, &ctx, "ask_person_for_screen", &args);
    assert_eq!(code(none), "SANDBOX_NOT_SET_UP");
    assert_eq!(code(f.call("ask_person_for_screen", json!({}))), "BAD_ARGS");
}

#[test]
fn the_reason_on_the_card_is_cleaned() {
    let f = Fixture::new("ask-clean");
    f.call("ask_person_for_screen", json!({ "reason": "The key is sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghij please" })).unwrap();
    let card = f.port.lease.lock().card(f.now()).cloned().unwrap();
    assert!(!card.reason.contains("sk-ant"), "{}", card.reason);
}

#[test]
fn through_the_engine_the_card_comes_back_as_a_result() {
    let (rig, _, lease) = engine("ask-engine");
    rig.svc.send("nimbus", "go").unwrap();
    rig.hello();
    rig.post(vec![call_event("s-1", "computer_observe", json!({}), false), call_event("s-2", "ask_person_for_screen", json!({ "reason": "Please log in for me" }), false)]);
    let all = results(&rig);
    let item = all.iter().find(|i| i["callId"] == "s-2").unwrap();
    assert_eq!((item["ok"].clone(), item["result"]["asked"].clone()), (json!(true), json!(true)));
    assert!(lease.lock().card(rig.host.now.load(Ordering::SeqCst)).is_some());
}

#[test]
fn the_card_outlives_the_turn_that_raised_it() {
    // The bot asks, tells the person what it needs and ends its turn: the card must still be there to answer.
    let f = Fixture::new("ask-parked");
    f.call("ask_person_for_screen", json!({ "reason": "A code is needed" })).unwrap();
    f.calls.end_run(Some(&f.port), "nimbus", "app:dm:nimbus");
    assert!(f.port.lease.lock().card(f.now()).is_some(), "the card is still open after the turn ended");
    assert!(matches!(f.port.lease.lock().banner(f.now()), super::sandbox::lease::Banner::NeedsYou { .. }));
    assert!(f.calls.bot_is_using(&f.port, "nimbus"));
    // the person takes over: the card is answered, and the bot is told to wait if it tries the screen
    f.port.lease.lock().take_over(f.now());
    assert!(f.port.lease.lock().card(f.now()).is_none());
    assert_eq!(code(f.call("computer_observe", json!({}))), "LEASE_HELD_BY_PERSON");
}

#[test]
fn the_next_turn_of_the_bot_that_asked_clears_its_old_card() {
    let f = Fixture::new("ask-next-turn");
    f.call("ask_person_for_screen", json!({ "reason": "A code is needed" })).unwrap();
    f.calls.end_run(Some(&f.port), "nimbus", "app:dm:nimbus");
    f.advance(2 * MIN);
    f.call("computer_observe", json!({})).unwrap();
    assert!(f.port.lease.lock().card(f.now()).is_none(), "working again: nobody is being waited for");
    assert!(f.calls.bot_is_using(&f.port, "nimbus"));
}

#[test]
fn a_card_that_nobody_answers_ends_with_its_thirty_minutes() {
    let f = Fixture::new("ask-expires");
    f.call("ask_person_for_screen", json!({ "reason": "A code is needed" })).unwrap();
    f.calls.end_run(Some(&f.port), "nimbus", "app:dm:nimbus");
    f.advance(31 * MIN);
    f.port.lease.lock().tick(f.now());
    assert!(f.port.lease.lock().card(f.now()).is_none());
    assert!(f.port.lease.lock().current(f.now()).is_none());
}
