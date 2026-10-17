//! The capture's work step (SPEC 11.18): after the plain message the agent is asked to list the folder and answer with a
//! sentence, so the drafter sees the rows an agent prints while it thinks, uses a tool, and after the turn.

use crate::tui::declarative::wizard::session::TestWords;
use crate::tui::declarative::wizard::visible::capture::{
    message_task, permission_task, work_task, Action, Capture, CaptureAction, Config, StepId, StepStatus,
};
use crate::tui::declarative::wizard::visible::host::Look;

fn words() -> TestWords {
    TestWords { one: "pong-aaaaaa".into(), two: "pong-bbbbbb".into(), sky: "cccccc".into() }
}

const IDLE: &str = "NIMBUS 0.4 ready\n\n+------------------------------+\n| > Type a task...             |\n+------------------------------+\n";
const PERMISSION: &str = "NIMBUS\n\nAllow nimbus to read the folder? [y/n]\n";

fn message_working(n: usize) -> String {
    let spinner = ["\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}"][n % 4];
    format!("NIMBUS 0.4\n\n> Reply with exactly the word: pong-aaaaaa\n\n{spinner} Thinking...  ctrl+c to stop\n\n| > Type a task...             |\n")
}

fn message_reply() -> String {
    "NIMBUS 0.4\n\n> Reply with exactly the word: pong-aaaaaa\nreply: pong-aaaaaa\n\n+------------------------------+\n| > Type a task...             |\n+------------------------------+\n".into()
}

fn work_working(n: usize) -> String {
    let spinner = ["\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}"][n % 4];
    format!("NIMBUS 0.4\n\n> {}\n\n~ Thinking... ({n}s)\ntool: ls\n  -> README.md\n{spinner} Generating...  ctrl+c to stop\n\n| > Type a task...             |\n", work_task(&words()))
}

fn work_reply() -> String {
    format!(
        "NIMBUS 0.4\n\n> {}\n\n~ Reflected for 3s\ntool: ls\n  -> README.md\nreply: folder listed, code cccccc.\n\u{25b8} Thought for 3s, 298 tokens\n\n+------------------------------+\n| > Type a task...             |\n+------------------------------+\n",
        work_task(&words())
    )
}

struct Drive {
    c: Capture,
    now: u64,
}

impl Drive {
    fn new(work_step: bool) -> Self {
        Self { c: Capture::new(words(), (120, 40), Config { work_step, ..Config::default() }), now: 0 }
    }

    fn look(&self, screen: &str) -> Look {
        Look { screen: screen.into(), watch: screen.into(), status: "idle".into(), alive: true, output_pos: None, hello_written: false, grid: Some((120, 40)), history: String::new() }
    }

    fn show(&mut self, screen: &str, ms: u64) -> Vec<Action> {
        let mut got = Vec::new();
        let end = self.now + ms;
        while self.now < end {
            self.now += 250;
            let look = self.look(screen);
            got.extend(self.c.tick(&look, self.now));
        }
        got
    }

    fn animate(&mut self, screens: impl Fn(usize) -> String, ms: u64) -> Vec<Action> {
        let mut got = Vec::new();
        let end = self.now + ms;
        let mut n = 0;
        while self.now < end {
            self.now += 250;
            n += 1;
            let look = self.look(&screens(n));
            got.extend(self.c.tick(&look, self.now));
        }
        got
    }

    fn deliver_message_then_reach_the_work_step(&mut self) -> Vec<Action> {
        let got = self.show(IDLE, 6000);
        assert_eq!(got, vec![Action::Deliver { text: message_task(&words()) }]);
        self.c.on_delivered(true, "", self.now);
        assert!(self.animate(message_working, 3000).is_empty());
        self.show(&message_reply(), 3500)
    }
}

#[test]
fn the_work_task_asks_the_agent_to_list_the_folder_and_answer_with_a_sentence() {
    let task = work_task(&words());
    assert_eq!(task, "List the files in this folder, then reply with exactly: folder listed, code cccccc.");
}

#[test]
fn with_the_step_on_the_message_is_followed_by_the_listing_task_and_both_screens_are_kept() {
    let mut d = Drive::new(true);
    let got = d.deliver_message_then_reach_the_work_step();
    assert_eq!(got, vec![Action::Deliver { text: work_task(&words()) }], "the listing task goes in next");
    assert_eq!(d.c.step(StepId::Message).status, StepStatus::Done);
    assert_eq!(d.c.current(), StepId::Work);
    d.c.on_delivered(true, "", d.now);
    assert!(d.animate(work_working, 3000).is_empty());
    let got = d.show(&work_reply(), 3500);
    assert_eq!(d.c.step(StepId::Work).status, StepStatus::Done);
    // the working screen shows the thinking and tool rows, the reply screen the answer and the row after the turn
    let working = d.c.frame("tui_work_working").expect("the working screen of the listing task");
    assert!(working.text.contains("Thinking...") && working.text.contains("tool: ls"), "{}", working.text);
    let reply = d.c.frame("tui_work_reply").expect("the reply screen of the listing task");
    assert!(reply.text.contains("reply: folder listed, code cccccc.") && reply.text.contains("Thought for 3s, 298 tokens"), "{}", reply.text);
    // and the permission step is next, with its own task
    assert_eq!(got, vec![Action::Deliver { text: permission_task(&words()) }]);
    assert_eq!(d.c.current(), StepId::Permission);
    // the message screens were not replaced by the work screens
    assert!(d.c.frame("tui_reply").unwrap().text.contains("reply: pong-aaaaaa"));
    let status = d.c.status_json(d.now);
    let ids: Vec<&str> = status["steps"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["start", "ready", "message", "work", "permission", "exit"]);
    assert_eq!(status["steps"][3]["label"], "Thinking and a tool");
}

#[test]
fn with_the_step_off_it_is_not_asked_not_listed_and_not_called_skipped() {
    let mut d = Drive::new(false);
    let got = d.deliver_message_then_reach_the_work_step();
    assert_eq!(got, vec![Action::Deliver { text: permission_task(&words()) }], "the permission task follows the message");
    let status = d.c.status_json(d.now);
    assert_eq!(status["steps"].as_array().unwrap().len(), 5);
    assert!(status["steps"].to_string().find("\"work\"").is_none());
    assert!(!d.c.into_set().skipped.contains(&"work".to_string()));
}

#[test]
fn a_question_raised_by_the_listing_task_is_the_persons_and_is_not_kept_as_the_permission_capture() {
    let mut d = Drive::new(true);
    d.deliver_message_then_reach_the_work_step();
    d.c.on_delivered(true, "", d.now);
    d.animate(work_working, 1500);
    let got = d.show(PERMISSION, 15_000);
    assert!(got.is_empty(), "no key goes into the question: {got:?}");
    assert_eq!(d.c.needs_you().map(|n| n.kind.as_str()), Some("permission"));
    assert!(d.c.frame("tui_permission").is_none(), "the permission step makes that capture, from its own task");
    assert!(d.c.frame("tui_work_question").is_some());
    // the person answers; the reply comes
    d.show(&work_reply(), 4000);
    assert_eq!(d.c.step(StepId::Work).status, StepStatus::Done);
}

#[test]
fn skip_and_retry_work_on_the_step() {
    let mut d = Drive::new(true);
    d.deliver_message_then_reach_the_work_step();
    d.c.on_delivered(true, "", d.now);
    d.animate(work_working, 2000);
    d.show(&work_reply(), 3500);
    assert!(d.c.frame("tui_work_reply").is_some());
    // (the permission task is on its way: the buttons wait for the typing to end)
    d.c.on_delivered(false, "the test stops it", d.now);
    // retry the step: its screens are gone and the task is typed again once the box is ready
    d.c.act(Some(StepId::Work), CaptureAction::Retry, d.now).unwrap();
    assert!(d.c.frame("tui_work_reply").is_none() && d.c.frame("tui_work_working").is_none());
    assert!(d.c.frame("tui_reply").is_some(), "the earlier step's screens stay");
    let got = d.show(IDLE, 4000);
    assert_eq!(got, vec![Action::Deliver { text: work_task(&words()) }]);
    // skip it instead
    let mut s = Drive::new(true);
    s.deliver_message_then_reach_the_work_step();
    s.c.on_delivered(false, "the test stops it", s.now);
    s.c.act(Some(StepId::Work), CaptureAction::Skip, s.now).unwrap();
    assert_eq!(s.c.step(StepId::Work).status, StepStatus::Skipped);
    assert_eq!(s.c.current(), StepId::Permission);
    assert!(s.c.into_set().skipped.contains(&"work".to_string()));
}
