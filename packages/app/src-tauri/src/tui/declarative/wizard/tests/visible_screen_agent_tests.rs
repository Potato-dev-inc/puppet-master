//! What the wizard learns about a screen-read agent beyond its screens (SPEC 11.17):
//!
//! - the reply the profile reads must be CLEAN (the one answer row, nothing else), and a dirty one names its rows;
//! - a model name the capture showed must be read by the draft (`[tui.footer_model]`);
//! - the capture resizes the pane once after the reply and MEASURES whether the agent reprints its conversation;
//!   the wizard writes `[features] reprints_on_resize` from that measurement, and the visible test checks the viewer
//!   shows the conversation once after a resize and that the viewer's grid is the PTY's.

use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{json, Value};

use crate::tui::declarative::wizard::session::TestWords;
use crate::tui::declarative::wizard::visible::capture::{Action, Capture, CaptureAction, Config, ResizeProbe, StepId, StepStatus};
use crate::tui::declarative::wizard::visible::host::{Delivered, Look};
use crate::tui::declarative::wizard::{Wizard, WizardError};

use super::fake_host::{screens, wait_for, FakeHost};
use super::support::{call, ok, visible_draft, with_captures, words_of, Tree};

// ---- the capture engine measures the reprint ---------------------------------------------------------------------------

fn words() -> TestWords {
    TestWords { one: "pong-aaaaaa".into(), two: "pong-bbbbbb".into(), sky: "cccccc".into() }
}

const IDLE: &str = "NIMBUS 0.4 ready\n\n+------------------------------+\n| > Type a task...             |\n+------------------------------+\n";

fn working(n: usize) -> String {
    let spinner = ["a", "b", "c", "d"][n % 4];
    format!("NIMBUS 0.4\n\n> Reply with exactly the word: pong-aaaaaa\n\n{spinner} Thinking...  ctrl+c to stop\n\n| > Type a task...             |\n")
}

fn reply() -> String {
    "NIMBUS 0.4\n\n> Reply with exactly the word: pong-aaaaaa\nreply: pong-aaaaaa\n\n+------------------------------+\n| > Type a task...             |\n+------------------------------+\n".into()
}

struct Drive {
    c: Capture,
    now: u64,
    grid: (u16, u16),
}

impl Drive {
    fn look(&self, screen: &str, history: &str) -> Look {
        Look { screen: screen.into(), watch: screen.into(), status: "idle".into(), alive: true, output_pos: None, hello_written: false, grid: Some(self.grid), history: history.into() }
    }

    fn show(&mut self, screen: &str, history: &str, ms: u64) -> Vec<Action> {
        let mut got = Vec::new();
        let end = self.now + ms;
        while self.now < end {
            self.now += 250;
            let look = self.look(screen, history);
            got.extend(self.c.tick(&look, self.now));
        }
        got
    }
}

/// A capture that has typed the test message and watched the agent answer; the reply is on screen and held still.
fn drive_to_the_reply() -> Drive {
    let mut d = Drive { c: Capture::new(words(), (120, 40), Config::default()), now: 0, grid: (120, 40) };
    let got = d.show(IDLE, "", 6000);
    assert!(matches!(got.as_slice(), [Action::Deliver { .. }]), "{got:?}");
    d.c.on_delivered(true, "", d.now);
    for n in 0..12 {
        d.now += 250;
        let look = d.look(&working(n), "");
        d.c.tick(&look, d.now);
    }
    d
}

fn history_with(copies: usize) -> String {
    let turn = "> Reply with exactly the word: pong-aaaaaa\nreply: pong-aaaaaa\n";
    format!("{}{}", turn.repeat(copies), IDLE)
}

#[test]
fn the_capture_resizes_the_pane_after_the_reply_and_says_the_agent_reprints_when_the_turn_shows_twice() {
    let mut d = drive_to_the_reply();
    let reply = reply();
    // the reply is held still: the step does not end yet, the pane is made narrower (and nothing is typed)
    let got = d.show(&reply, &history_with(1), 3500);
    assert_eq!(got, vec![Action::Resize { cols: 100, rows: 40 }], "{got:?}");
    assert_eq!(d.c.step(StepId::Message).status, StepStatus::Running, "not done until the measurement is");
    // the agent redraws at the new size and prints its whole conversation again
    d.grid = (100, 40);
    let got = d.show(&reply, &history_with(2), 3500);
    assert_eq!(got, vec![Action::Resize { cols: 120, rows: 40 }], "put back: {got:?}");
    d.grid = (120, 40);
    d.show(&reply, &history_with(2), 3000);
    assert_eq!(d.c.step(StepId::Message).status, StepStatus::Done);
    let set = d.c.into_set();
    let probe = set.resize.expect("it was measured");
    assert!(probe.reprints, "{probe:?}");
    assert_eq!((probe.from, probe.to), ((120, 40), (100, 40)));
    assert_eq!((probe.before.len(), probe.after.len()), (2, 4));
    assert!(set.notes.iter().all(|n| !n.contains("changed size")), "the wizard's own resize is no surprise: {:?}", set.notes);
}

#[test]
fn an_agent_that_redraws_only_what_is_visible_is_measured_as_not_reprinting() {
    let mut d = drive_to_the_reply();
    let reply = reply();
    d.show(&reply, &history_with(1), 3500);
    d.grid = (100, 40);
    d.show(&reply, &history_with(1), 3500);
    d.grid = (120, 40);
    d.show(&reply, &history_with(1), 3000);
    let probe = d.c.into_set().resize.expect("measured");
    assert!(!probe.reprints, "{probe:?}");
    assert_eq!(probe.before, probe.after);
}

#[test]
fn a_host_that_cannot_show_a_history_measures_nothing_and_the_capture_goes_on() {
    let mut d = drive_to_the_reply();
    let got = d.show(&reply(), "", 3500);
    assert!(got.iter().all(|a| !matches!(a, Action::Resize { .. })), "{got:?}");
    assert_eq!(d.c.step(StepId::Message).status, StepStatus::Done);
    assert!(d.c.into_set().resize.is_none());
}

#[test]
fn a_host_that_refuses_the_resize_ends_the_probe_with_a_note() {
    let mut d = drive_to_the_reply();
    let got = d.show(&reply(), &history_with(1), 3500);
    assert!(matches!(got.as_slice(), [Action::Resize { .. }]));
    d.c.on_resize_refused("not a pane the wizard started", d.now);
    assert_eq!(d.c.step(StepId::Message).status, StepStatus::Done);
    let set = d.c.into_set();
    assert!(set.resize.is_none());
    assert!(set.notes.iter().any(|n| n.contains("could not be resized")), "{:?}", set.notes);
    let _ = CaptureAction::Skip;
}

#[test]
fn the_probe_travels_in_the_capture_json_masked_for_the_drafter() {
    let probe = ResizeProbe {
        reprints: true,
        from: (120, 40),
        to: (100, 40),
        before: vec!["reply: pong-aaaaaa".into()],
        after: vec!["reply: pong-aaaaaa".into(), "reply: pong-aaaaaa".into()],
    };
    let shown = probe.to_json(&|t: &str| t.replace("pong-aaaaaa", "<word-1>"));
    assert_eq!(shown["reprints"], true);
    assert_eq!(shown["after_rows"], json!(["reply: <word-1>", "reply: <word-1>"]));
    let mut set = crate::tui::declarative::wizard::visible::capture::CaptureSet::default();
    set.resize = Some(probe.clone());
    let back = crate::tui::declarative::wizard::visible::capture::CaptureSet::from_json(&set.to_json()).expect("round trip");
    assert_eq!(back.resize, Some(probe));
}

// ---- verify writes the measured flag -----------------------------------------------------------------------------------

fn session_with_measurement(name: &str, reprints: Option<bool>) -> (Wizard, String, Tree) {
    let tree = Tree::new(name);
    let (wizard, _host) = tree.visible_wizard_with(|_| ());
    let id = ok(&wizard, "start", json!({"command": "fake-agent"}))["session"].as_str().unwrap().to_string();
    ok(&wizard, "probe", json!({"session": id}));
    with_captures(&wizard, &id);
    wizard
        .read(&id, |s| {
            if let Some(r) = reprints {
                s.visible.captures.lock().as_mut().unwrap().resize =
                    Some(ResizeProbe { reprints: r, from: (120, 40), to: (100, 40), before: vec![], after: vec![] });
            }
        })
        .unwrap();
    (wizard, id, tree)
}

#[test]
fn verify_writes_the_measured_flag_whatever_the_drafter_wrote() {
    for (measured, drafter, expect) in [(Some(true), "", true), (Some(false), "[features]\nreprints_on_resize = true\n", false)] {
        let (wizard, id, _tree) = session_with_measurement("sa-flag", measured);
        let text = format!("{}\n{drafter}", visible_draft("fake-agent"));
        let v = ok(&wizard, "verify", json!({"session": id, "toml": text}));
        assert_eq!(v["ok"], true, "{}", v["text"]);
        let stored = wizard.read(&id, |s| s.draft.as_ref().unwrap().text.clone()).unwrap();
        let def = crate::tui::declarative::schema::parse_definition(&stored).unwrap();
        assert_eq!(def.features.reprints_on_resize, expect, "{stored}");
        assert_eq!(stored.matches("reprints_on_resize").count(), 1, "{stored}");
    }
}

#[test]
fn an_unmeasured_capture_leaves_the_drafters_text_alone() {
    let (wizard, id, _tree) = session_with_measurement("sa-flag-none", None);
    let text = visible_draft("fake-agent");
    ok(&wizard, "verify", json!({"session": id, "toml": text}));
    let stored = wizard.read(&id, |s| s.draft.as_ref().unwrap().text.clone()).unwrap();
    assert_eq!(stored, text);
}

// ---- the visible test: a clean reply, the model, a resize ---------------------------------------------------------------

struct Fixture {
    wizard: Arc<Wizard>,
    host: Arc<FakeHost>,
    id: String,
    plan: String,
    words: TestWords,
    _tree: Tree,
}

/// `idle`: the idle screen the capture holds (and the pane shows); `draft`: the draft text.
fn fixture(name: &str, draft: String, idle: &str, reprints: Option<bool>) -> Fixture {
    let tree = Tree::new(name);
    let (wizard, host) = tree.visible_wizard_with(|env| {
        env.visible.test.ready = Duration::from_millis(400);
        env.visible.test.busy = Duration::from_millis(400);
        env.visible.test.done = Duration::from_millis(800);
        env.visible.test.permission = Duration::from_millis(400);
    });
    let id = ok(&wizard, "start", json!({"command": "fake-agent"}))["session"].as_str().unwrap().to_string();
    ok(&wizard, "probe", json!({"session": id}));
    with_captures(&wizard, &id);
    wizard
        .read(&id, |s| {
            let mut guard = s.visible.captures.lock();
            let set = guard.as_mut().unwrap();
            for frame in set.frames.iter_mut().filter(|f| f.name == "tui_idle") {
                frame.text = idle.to_string();
            }
            if let Some(r) = reprints {
                set.resize = Some(ResizeProbe { reprints: r, from: (120, 40), to: (100, 40), before: vec![], after: vec![] });
            }
        })
        .unwrap();
    let v = ok(&wizard, "verify", json!({"session": id, "toml": draft}));
    assert_eq!(v["ok"], true, "{}", v["text"]);
    let plan = v["planned_visible"]["fingerprint"].as_str().expect("a planned visible test").to_string();
    let words = words_of(&wizard, &id);
    Fixture { wizard: Arc::new(wizard), host, id, plan, words, _tree: tree }
}

impl Fixture {
    fn run(&self) -> JoinHandle<Result<Value, WizardError>> {
        let (wizard, id, plan) = (self.wizard.clone(), self.id.clone(), self.plan.clone());
        std::thread::spawn(move || call(&wizard, "visibletest", json!({"session": id, "confirm": true, "plan": plan})))
    }

    /// The made-up agent: it answers with `rows_before_reply` between the task's echo and its reply. `tall`: the
    /// conversation is taller than the pane, so the echo of the task has scrolled into the history.
    fn agent(&self, idle: &'static str, rows_before_reply: &'static str) {
        self.agent_with(idle, rows_before_reply, false)
    }

    fn agent_with(&self, idle: &'static str, rows_before_reply: &'static str, tall: bool) {
        let host = self.host.clone();
        self.host.on_deliver(move |h, pane, text| {
            if text.contains("hello.txt") {
                h.set_screen_of(pane, screens::PERMISSION);
                h.set_card(Some(json!({"kind": "command_approval", "choices": ["allow_once", "deny"]})));
                return Delivered::Submitted;
            }
            let word = text.rsplit(' ').next().unwrap_or("").to_string();
            h.set_screen_of(pane, &screens::working(&word, 0));
            let (host, pane) = (host.clone(), pane.to_string());
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(120));
                host.set_screen_of(&pane, &screens::working(&word, 1));
                std::thread::sleep(Duration::from_millis(120));
                let echo = if tall { String::new() } else { format!("> Reply with exactly the word: {word}\n") };
                let screen = format!(
                    "NIMBUS 0.4\n\n{echo}{rows_before_reply}reply: {word}\n\ndone in 2s\n\n{}",
                    idle.split_once("\n\n").map_or(idle, |(_, rest)| rest)
                );
                if tall {
                    host.push_scrollback(&pane, &[format!("> Reply with exactly the word: {word}").as_str()]);
                }
                host.set_screen_of(&pane, &screen);
            });
            Delivered::Submitted
        });
    }

    fn start(&self, idle: &str) -> JoinHandle<Result<Value, WizardError>> {
        let before = self.host.spawned().len();
        let run = self.run();
        wait_for(10, || (self.host.spawned().len() > before).then_some(())).expect("the test pane is started");
        self.host.set_screen(idle);
        run
    }
}

fn status_of(v: &Value, name: &str) -> String {
    v["checks"].as_array().unwrap().iter().find(|c| c["name"] == name).map(|c| c["status"].as_str().unwrap().to_string()).unwrap_or_default()
}

fn note_of(v: &Value, name: &str) -> String {
    v["checks"].as_array().unwrap().iter().find(|c| c["name"] == name).map(|c| c["note"].as_str().unwrap().to_string()).unwrap_or_default()
}

#[test]
fn a_reply_with_a_thinking_row_in_it_fails_and_the_row_is_named() {
    let f = fixture("sa-dirty", visible_draft("fake-agent"), screens::IDLE, None);
    f.agent(screens::IDLE, "~ Thinking... (2s)\n");
    let v = f.start(screens::IDLE).join().unwrap().unwrap();
    assert_eq!(v["ok"], false, "{}", v["checks"]);
    assert_eq!(status_of(&v, "done_reply"), "failed");
    let text = v["text"].as_str().unwrap();
    assert!(text.contains("not clean") && text.contains("~ Thinking... (2s)"), "{text}");
    assert!(text.contains("[tui.thinking]") && text.contains("[tui.chrome]"), "{text}");
    assert!(!text.contains(&f.words.two), "the word stays masked: {text}");
}

#[test]
fn the_same_reply_passes_once_the_draft_names_the_thinking_rows() {
    let draft = format!("{}\n[tui.thinking]\nline_regex = ['^~ thinking']\n", visible_draft("fake-agent"));
    let f = fixture("sa-clean", draft, screens::IDLE, None);
    f.agent(screens::IDLE, "~ Thinking... (2s)\n");
    let v = f.start(screens::IDLE).join().unwrap().unwrap();
    assert_eq!(status_of(&v, "done_reply"), "passed", "{}\n{}", v["text"], v["checks"]);
}

const IDLE_WITH_MODEL: &str = "NIMBUS 0.4 ready model: zeta-1\n\n+------------------------------+\n| > Type a task...             |\n+------------------------------+\n";

#[test]
fn a_model_the_capture_showed_must_be_read_by_the_draft() {
    let f = fixture("sa-model-missing", visible_draft("fake-agent"), IDLE_WITH_MODEL, None);
    f.agent(IDLE_WITH_MODEL, "");
    let v = f.start(IDLE_WITH_MODEL).join().unwrap().unwrap();
    assert_eq!(status_of(&v, "model"), "failed", "{}", v["checks"]);
    let text = v["text"].as_str().unwrap();
    assert!(text.contains("[tui.footer_model]") && text.contains("zeta-1"), "{text}");
}

#[test]
fn the_model_is_read_when_the_draft_has_a_footer_model_table() {
    let draft = format!("{}\n[tui.footer_model]\nregex = 'model:\\s*(?P<model>[\\w.-]+)'\n", visible_draft("fake-agent"));
    let f = fixture("sa-model-ok", draft, IDLE_WITH_MODEL, None);
    f.agent(IDLE_WITH_MODEL, "");
    let v = f.start(IDLE_WITH_MODEL).join().unwrap().unwrap();
    assert_eq!(status_of(&v, "model"), "passed", "{}", v["checks"]);
    assert!(note_of(&v, "model").contains("zeta-1"));
}

#[test]
fn no_model_in_the_capture_means_nothing_to_check() {
    let f = fixture("sa-model-none", visible_draft("fake-agent"), screens::IDLE, None);
    f.agent(screens::IDLE, "");
    let v = f.start(screens::IDLE).join().unwrap().unwrap();
    assert_eq!(status_of(&v, "model"), "skipped");
}

/// What the viewer does: the pane's emulator wipes its history at a resize when the profile says the agent reprints.
/// The agent here reprints its whole turn into the scrollback.
fn reprinting_agent(f: &Fixture) {
    let word = f.words.two.clone();
    f.host.on_resize(move |h, pane, _cols, _rows| {
        h.push_scrollback(pane, &[format!("> Reply with exactly the word: {word}").as_str()]);
    });
}

#[test]
fn a_resize_that_reprints_the_turn_fails_a_draft_without_the_flag() {
    let f = fixture("sa-resize-bad", visible_draft("fake-agent"), screens::IDLE, None);
    f.agent_with(screens::IDLE, "", true);
    reprinting_agent(&f);
    let v = f.start(screens::IDLE).join().unwrap().unwrap();
    assert_eq!(status_of(&v, "resize"), "failed", "{}", v["checks"]);
    let text = v["text"].as_str().unwrap();
    assert!(text.contains("reprints_on_resize") && text.contains("shows again"), "{text}");
    assert!(f.host.resizes().len() >= 2, "resized and put back: {:?}", f.host.resizes());
    assert_eq!(f.host.resizes().last().map(|r| (r.1, r.2)), Some((120, 40)), "the pane is put back");
}

#[test]
fn the_same_agent_passes_when_the_wizard_wrote_the_measured_flag() {
    let f = fixture("sa-resize-ok", visible_draft("fake-agent"), screens::IDLE, Some(true));
    f.agent_with(screens::IDLE, "", true);
    reprinting_agent(&f);
    let v = f.start(screens::IDLE).join().unwrap().unwrap();
    assert_eq!(status_of(&v, "resize"), "passed", "{}\n{}", v["text"], v["checks"]);
    assert!(note_of(&v, "resize").contains("viewer's grid is the PTY's"), "{}", note_of(&v, "resize"));
}

#[test]
fn an_agent_that_does_not_reprint_passes_without_a_flag() {
    let f = fixture("sa-resize-plain", visible_draft("fake-agent"), screens::IDLE, Some(false));
    f.agent_with(screens::IDLE, "", true);
    f.host.on_resize(|_, _, _, _| ());
    let v = f.start(screens::IDLE).join().unwrap().unwrap();
    assert_eq!(status_of(&v, "resize"), "passed", "{}\n{}", v["text"], v["checks"]);
}

#[test]
fn a_host_that_cannot_resize_skips_the_resize_check() {
    let f = fixture("sa-resize-skip", visible_draft("fake-agent"), screens::IDLE, None);
    f.agent(screens::IDLE, "");
    let v = f.start(screens::IDLE).join().unwrap().unwrap();
    assert_eq!(status_of(&v, "resize"), "skipped");
    assert_eq!(v["ok"], true, "{}", v["text"]);
}

// ---- two permission captures: a file write and a command run ------------------------------------------------------------

const COMMAND_QUESTION: &str = "NIMBUS\n\nRun this command?\n    echo cccccc\n\n> 1. Allow once\n  2. Always allow in this project\n  3. Deny\n";

#[test]
fn the_permission_step_captures_the_file_question_and_then_the_command_question() {
    use crate::tui::declarative::wizard::visible::capture::{command_task, permission_task};
    let mut d = drive_to_the_reply();
    // no history: no resize measurement; the reply step ends and the file task goes in
    let got = d.show(&reply(), "", 4000);
    let file_task = match got.as_slice() {
        [Action::Deliver { text }] => text.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(file_task, permission_task(&words()));
    d.c.on_delivered(true, "", d.now);
    assert!(d.show(screens::PERMISSION, "", 15_000).is_empty(), "nothing is pressed into the question");
    assert!(d.c.frame("tui_permission").unwrap().text.contains("create hello.txt"));
    assert_eq!(d.c.step(StepId::Permission).status, StepStatus::NeedsYou);
    // the person answers; the agent is back at its box: the SECOND question is asked, not the end of the step
    let got = d.show(IDLE, "", 4000);
    assert_eq!(d.c.step(StepId::Permission).status, StepStatus::Running, "one question is not the whole step");
    assert_eq!(got, vec![Action::Deliver { text: command_task(&words()) }], "{got:?}");
    d.c.on_delivered(true, "", d.now);
    assert!(d.show(COMMAND_QUESTION, "", 15_000).is_empty());
    assert!(d.c.frame("tui_permission_command").unwrap().text.contains("Run this command?"));
    assert!(d.c.frame("tui_permission").unwrap().text.contains("create hello.txt"), "the first capture is kept");
    d.show(IDLE, "", 4000);
    assert_eq!(d.c.step(StepId::Permission).status, StepStatus::Done);
    let set = d.c.into_set();
    assert!(set.has("tui_permission") && set.has("tui_permission_command"));
    assert!(!set.runs_without_asking);
}

#[test]
fn an_agent_that_runs_the_command_without_asking_is_a_fact_not_a_failure() {
    let mut d = drive_to_the_reply();
    d.show(&reply(), "", 4000);
    d.c.on_delivered(true, "", d.now);
    d.show(screens::PERMISSION, "", 15_000);
    d.show(IDLE, "", 4000);
    d.c.on_delivered(true, "", d.now);
    // it works for a moment, prints the output and is back at its box, with no question in between
    for n in 0..8 {
        d.now += 250;
        let look = d.look(&working(n), "");
        d.c.tick(&look, d.now);
    }
    d.show("NIMBUS 0.4\n\n> Run this shell command: echo cccccc\ncccccc\n\n+------------------------------+\n| > Type a task...             |\n+------------------------------+\n", "", 12_000);
    let set = d.c.into_set();
    assert!(set.runs_without_asking, "{:?}", set.notes);
    assert!(!set.has("tui_permission_command"));
    assert!(set.notes.iter().any(|n| n.contains("did not ask before running a command")), "{:?}", set.notes);
}

/// The visible test checks the command question in a second pane (the first is stuck behind the file question).
#[test]
fn the_command_question_becomes_a_card_in_a_second_test_pane() {
    let draft = format!(
        "{}\n[[tui.prompt]]\nkind = \"command_run\"\ndetect_regex = 'run this command\\?'\nchoices = [\n  {{ id = \"allow_once\", text = \"1\" }},\n  {{ id = \"deny\", text = \"3\" }},\n]\n",
        visible_draft("fake-agent")
    );
    let f = fixture("sa-command", draft, screens::IDLE, None);
    wizard_with_command_frame(&f);
    let host = f.host.clone();
    let inner = f.host.clone();
    f.host.on_deliver(move |h, pane, text| {
        if text.contains("echo ") {
            h.set_screen_of(pane, COMMAND_QUESTION);
            h.set_card(Some(json!({"kind": "command_run", "choices": ["allow_once", "deny"]})));
            return Delivered::Submitted;
        }
        if text.contains("hello.txt") {
            h.set_screen_of(pane, screens::PERMISSION);
            h.set_card(Some(json!({"kind": "command_approval", "choices": ["allow_once", "deny"]})));
            return Delivered::Submitted;
        }
        let word = text.rsplit(' ').next().unwrap_or("").to_string();
        let (host, pane) = (inner.clone(), pane.to_string());
        host.set_screen_of(&pane, &screens::working(&word, 0));
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            host.set_screen_of(&pane, &screens::reply(&word));
        });
        Delivered::Submitted
    });
    let run = f.start(screens::IDLE);
    // the second pane draws its input box when it starts
    assert!(wait_for(20, || (host.spawned().len() >= 2).then_some(())).is_some(), "a second pane was started");
    host.set_screen(screens::IDLE);
    let v = run.join().unwrap().unwrap();
    assert_eq!(status_of(&v, "permission"), "passed", "{}", v["checks"]);
    assert_eq!(status_of(&v, "permission_command"), "passed", "{}\n{}", v["text"], v["checks"]);
    let typed = host.delivered();
    assert!(typed.iter().any(|t| t.contains("hello.txt")) && typed.iter().any(|t| t.contains("echo ")), "{typed:?}");
    assert!(host.pressed().is_empty(), "no question was answered");
    assert_eq!(host.killed().len(), 2, "both panes are closed");
}

fn wizard_with_command_frame(f: &Fixture) {
    f.wizard
        .read(&f.id, |s| {
            let mut guard = s.visible.captures.lock();
            guard.as_mut().unwrap().frames.push(crate::tui::declarative::wizard::visible::capture::CapFrame {
                name: "tui_permission_command".into(),
                text: COMMAND_QUESTION.into(),
                variants: Vec::new(),
            });
        })
        .unwrap();
}

#[test]
fn a_command_capture_with_no_table_that_matches_it_fails_verify() {
    // the draft only knows the y/n line; the numbered command menu is a different screen
    let tree = Tree::new("sa-verify-command");
    let (wizard, _host) = tree.visible_wizard_with(|_| ());
    let id = ok(&wizard, "start", json!({"command": "fake-agent"}))["session"].as_str().unwrap().to_string();
    ok(&wizard, "probe", json!({"session": id}));
    with_captures(&wizard, &id);
    wizard
        .read(&id, |s| {
            s.visible.captures.lock().as_mut().unwrap().frames.push(crate::tui::declarative::wizard::visible::capture::CapFrame {
                name: "tui_permission_command".into(),
                text: COMMAND_QUESTION.into(),
                variants: Vec::new(),
            });
        })
        .unwrap();
    let v = call(&wizard, "verify", json!({"session": id, "toml": visible_draft("fake-agent")})).unwrap();
    assert_eq!(v["ok"], false);
    let text = v["text"].as_str().unwrap();
    assert!(text.contains("tui_permission_command") && text.contains("[[tui.prompt]]"), "{text}");
}
