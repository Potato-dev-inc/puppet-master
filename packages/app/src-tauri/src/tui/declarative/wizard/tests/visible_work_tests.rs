//! The visible test's two checks added by SPEC 11.18, against a scripted pane: a task that makes the agent think and use
//! a tool must still come out as exactly the one answer row, and a reply taller than the pane must be read whole while
//! the model is still read once the banner has scrolled away.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::tui::declarative::wizard::session::TestWords;
use crate::tui::declarative::wizard::visible::capture::{work_sentence, CapFrame};
use crate::tui::declarative::wizard::visible::host::Delivered;
use crate::tui::declarative::wizard::Wizard;

use super::fake_host::{screens, wait_for, FakeHost};
use super::support::{capture_set, ok, visible_draft_without_work_tables, words_of, Tree};

const BOX: &str = "+------------------------------+\n| > Type a task...             |\n+------------------------------+\n";

/// The made-up agent of these tests: its banner names the model, and its footer does too when `footer_model` is set.
#[derive(Clone, Copy)]
struct Agent {
    model: &'static str,
    footer_model: bool,
}

impl Agent {
    fn banner(&self) -> String {
        format!("NIMBUS 0.4.2 | {} | C:\\x\n", self.model)
    }

    fn footer(&self) -> String {
        if self.footer_model {
            format!("  model: {} | /help for commands | ctrl+c twice to quit\n", self.model)
        } else {
            "  /help for commands | ctrl+c twice to quit\n".into()
        }
    }

    /// The agent at rest with `body` above its input box.
    fn at_rest(&self, body: &str) -> String {
        format!("{}\n{body}{BOX}{}", self.banner(), self.footer())
    }
}

fn sentence_of(words: &TestWords) -> String {
    work_sentence(words)
}

/// What the agent draws while it thinks and uses its tool, and after it answered with a summary row.
fn work_working(a: &Agent, words: &TestWords, n: usize) -> String {
    let spinner = ["\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}"][n % 4];
    a.at_rest("").replace(
        BOX,
        &format!(
            "> List the files in this folder, then reply with exactly: {}\n\n~ Thinking... ({n}s)\ntool: ls\n  -> README.md\n{spinner} Generating...  ctrl+c to stop\n\n{BOX}",
            sentence_of(words)
        ),
    )
}

fn work_reply(a: &Agent, words: &TestWords) -> String {
    a.at_rest(&format!(
        "> List the files in this folder, then reply with exactly: {}\n\n~ Thinking... (3s)\ntool: ls\n  -> README.md\nreply: {}\n\u{25b8} Thought for 3s, 298 tokens\n\n",
        sentence_of(words),
        sentence_of(words)
    ))
}

/// The capture the wizard would have made of this agent: the usual frames and the two of the work step.
fn with_work_captures(wizard: &Wizard, id: &str, a: &Agent) {
    wizard
        .read(id, |s| {
            let mut set = capture_set(&s.words);
            for (name, text) in [("tui_work_working", work_working(a, &s.words, 1)), ("tui_work_reply", work_reply(a, &s.words))] {
                set.frames.push(CapFrame { name: name.into(), text, variants: Vec::new() });
            }
            *s.visible.captures.lock() = Some(set);
        })
        .unwrap();
}

/// The recorded draft, with the tables of rows an agent prints while it works and after a turn.
fn draft_with(extra_tables: &str) -> String {
    // a footer row that names the model is chrome
    let base = visible_draft_without_work_tables("fake-agent").replace("'^/help for commands',", "'^/help for commands', '^model: ',");
    format!("{base}\n{extra_tables}")
}

const WORK_TABLES: &str = "[tui.thinking]\nline_regex = ['^~\\s*thinking']\n\n[tui.summary]\nline_regex = ['^thought for \\d+s']\n";
const THINKING_ONLY: &str = "[tui.thinking]\nline_regex = ['^~\\s*thinking']\n";

struct Fx {
    wizard: Arc<Wizard>,
    host: Arc<FakeHost>,
    id: String,
    plan: String,
    words: TestWords,
    _tree: Tree,
}

fn fixture(name: &str, draft: String, agent: Agent) -> Fx {
    let tree = Tree::new(name);
    let (wizard, host) = tree.visible_wizard_with(|env| {
        env.visible.test.ready = Duration::from_secs(3);
        env.visible.test.busy = Duration::from_secs(2);
        env.visible.test.done = Duration::from_secs(4);
        env.visible.test.permission = Duration::from_secs(1);
    });
    let id = ok(&wizard, "start", json!({"command": "fake-agent"}))["session"].as_str().expect("session").to_string();
    ok(&wizard, "probe", json!({"session": id}));
    with_work_captures(&wizard, &id, &agent);
    let v = ok(&wizard, "verify", json!({"session": id, "toml": draft}));
    let plan = v["planned_visible"]["fingerprint"].as_str().expect("a planned visible test").to_string();
    let words = words_of(&wizard, &id);
    host.enable_history();
    script(&host, &words, agent);
    Fx { wizard: Arc::new(wizard), host, id, plan, words, _tree: tree }
}

/// The agent's behaviour for each task the visible test types.
fn script(host: &Arc<FakeHost>, words: &TestWords, agent: Agent) {
    let for_thread = host.clone();
    let words = words.clone();
    host.on_deliver(move |h, pane, text| {
        if text.contains("hello.txt") {
            h.set_screen_of(pane, screens::PERMISSION);
            h.set_card(Some(json!({"kind": "command_approval", "choices": ["allow_once", "deny"]})));
            return Delivered::Submitted;
        }
        let (host, pane, text, words) = (for_thread.clone(), pane.to_string(), text.to_string(), words.clone());
        if text.starts_with("List the files") {
            h.set_screen_of(&pane, &work_working(&agent, &words, 1));
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(150));
                host.set_screen_of(&pane, &work_working(&agent, &words, 2));
                std::thread::sleep(Duration::from_millis(150));
                host.set_screen_of(&pane, &work_reply(&agent, &words));
            });
        } else if text.starts_with("Write the whole numbers") {
            // 70 rows into a 40-row pane: the banner, the echo and the first rows are in the scrollback
            let word = text.rsplit(' ').next().unwrap_or("").to_string();
            h.set_screen_of(&pane, &agent.at_rest(&format!("> {text}\n\n")).replace(BOX, &format!("reply: 1\n\n  ctrl+c to stop\n\n{BOX}")));
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(150));
                let mut above: Vec<String> = vec![agent.banner().trim_end().to_string(), String::new(), format!("> {text}"), "reply: 1".into()];
                above.extend((2..=40).map(|n| n.to_string()));
                let refs: Vec<&str> = above.iter().map(String::as_str).collect();
                host.push_scrollback(&pane, &refs);
                let tail: String = (41..=70).map(|n| format!("{n}\n")).collect();
                // the screen: only the tail of the reply, no banner
                host.set_screen_of(&pane, &format!("{tail}{word}\n\n{BOX}{}", agent.footer()));
            });
        } else {
            let word = text.rsplit(' ').next().unwrap_or("").to_string();
            h.set_screen_of(&pane, &screens::working(&word, 0));
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(150));
                host.set_screen_of(&pane, &agent.at_rest(&format!("> {text}\nreply: {word}\n\ndone in 2s\n\n")));
            });
        }
        Delivered::Submitted
    });
}

impl Fx {
    fn run(&self) -> Value {
        let (wizard, id, plan) = (self.wizard.clone(), self.id.clone(), self.plan.clone());
        let handle = std::thread::spawn(move || crate::tui::declarative::wizard::api::handle(&wizard, "visibletest", &json!({"session": id, "confirm": true, "plan": plan})));
        let host = self.host.clone();
        wait_for(10, || host.spawned().first().map(|_| ())).expect("the test pane is started");
        self.host.set_screen(&Agent { model: "zeta", footer_model: false }.at_rest(""));
        handle.join().unwrap().unwrap_or_else(|e| panic!("{e}"))
    }
}

fn status_of(v: &Value, name: &str) -> String {
    v["checks"].as_array().unwrap().iter().find(|c| c["name"] == name).map(|c| c["status"].as_str().unwrap().to_string()).unwrap_or_default()
}

fn note_of(v: &Value, name: &str) -> String {
    v["checks"].as_array().unwrap().iter().find(|c| c["name"] == name).map(|c| c["note"].as_str().unwrap().to_string()).unwrap_or_default()
}

#[test]
fn a_clean_profile_passes_the_work_task_and_reads_a_tall_reply_whole() {
    let agent = Agent { model: "zeta", footer_model: false };
    let f = fixture("vw-good", draft_with(WORK_TABLES), agent);
    let v = f.run();
    assert_eq!(status_of(&v, "work"), "passed", "{}\n{}", v["text"], v["checks"]);
    assert_eq!(status_of(&v, "long_reply"), "passed", "{}\n{}", v["text"], v["checks"]);
    assert!(note_of(&v, "long_reply").contains("read whole"), "{}", note_of(&v, "long_reply"));
    // the tasks that were typed: the plain message, the listing task (with the third word) and the tall reply
    let typed = f.host.delivered();
    assert_eq!(typed[0], format!("Reply with exactly the word: {}", f.words.two));
    assert_eq!(typed[1], format!("List the files in this folder, then reply with exactly: {}", work_sentence(&f.words)));
    assert!(typed[2].starts_with("Write the whole numbers from 1 to 70") && typed[2].ends_with(&f.words.two), "{}", typed[2]);
    // the sentence's word never reaches the drafter's text
    assert!(!v.to_string().contains(&f.words.sky), "{v}");
}

#[test]
fn a_summary_row_no_table_marks_fails_the_work_task_and_names_the_row_and_the_table() {
    let agent = Agent { model: "zeta", footer_model: false };
    let f = fixture("vw-summary", draft_with(THINKING_ONLY), agent);
    let v = f.run();
    assert_eq!(status_of(&v, "done_reply"), "passed", "the plain message is clean: {}", v["checks"]);
    assert_eq!(status_of(&v, "work"), "failed", "{}", v["checks"]);
    assert_eq!(status_of(&v, "long_reply"), "skipped");
    let text = v["text"].as_str().unwrap();
    assert!(text.contains("VISIBLETEST_WORK_FAILED"), "{text}");
    assert!(text.contains("Thought for 3s, 298 tokens") && text.contains("[tui.summary]"), "{text}");
    // verify says the same from the capture alone: the screen of the work reply is not read as exactly the answer
    let verified = ok(&f.wizard, "verify", json!({"session": f.id, "toml": draft_with(THINKING_ONLY)}));
    assert_eq!(verified["ok"], false);
    assert!(verified["text"].as_str().unwrap().contains("tui_work_reply") && verified["text"].as_str().unwrap().contains("reply_rows"), "{}", verified["text"]);
    // and passes with the table
    let verified = ok(&f.wizard, "verify", json!({"session": f.id, "toml": draft_with(WORK_TABLES)}));
    assert_eq!(verified["ok"], true, "{}", verified["text"]);
}

#[test]
fn a_table_that_eats_rows_of_a_long_answer_fails_the_tall_reply_check() {
    let agent = Agent { model: "zeta", footer_model: false };
    let draft = draft_with(WORK_TABLES).replace("'^done in \\d+s$',", "'^done in \\d+s$', '^\\d+$',");
    assert!(draft.contains("'^\\d+$'"));
    let f = fixture("vw-eaten", draft, agent);
    let v = f.run();
    assert_eq!(status_of(&v, "work"), "passed", "{}", v["checks"]);
    assert_eq!(status_of(&v, "long_reply"), "failed", "{}", v["checks"]);
    let text = v["text"].as_str().unwrap();
    assert!(text.contains("VISIBLETEST_LONG_REPLY_FAILED") && text.contains("holds only") && text.contains("[tui.chrome]"), "{text}");
}

const BANNER_ONLY: &str = "[tui.footer_model]\nregex = '^nimbus\\s+[\\d.]+\\s*\\|\\s*(?P<model>[a-z]+)'\n";
const STATUS_ROW: &str = "[tui.footer_model]\nregex = '(?:^nimbus\\s+[\\d.]+\\s*\\|\\s*|model:\\s*)(?P<model>[a-z]+)'\n";

#[test]
fn a_model_regex_that_only_matches_the_banner_fails_once_a_long_reply_pushed_the_banner_away() {
    let agent = Agent { model: "zeta", footer_model: true };
    let f = fixture("vw-banner-model", draft_with(&format!("{WORK_TABLES}\n{BANNER_ONLY}")), agent);
    let v = f.run();
    assert_eq!(status_of(&v, "model"), "passed", "the banner was on screen right after the reply: {}", v["checks"]);
    assert_eq!(status_of(&v, "long_reply"), "failed", "{}", v["checks"]);
    let text = v["text"].as_str().unwrap();
    assert!(text.contains("VISIBLETEST_LONG_REPLY_FAILED") && text.contains("banner scrolls away") && text.contains("zeta"), "{text}");
}

#[test]
fn a_model_regex_that_also_matches_a_status_row_keeps_reading_it_after_the_banner_is_gone() {
    let agent = Agent { model: "zeta", footer_model: true };
    let f = fixture("vw-status-model", draft_with(&format!("{WORK_TABLES}\n{STATUS_ROW}")), agent);
    let v = f.run();
    assert_eq!(status_of(&v, "long_reply"), "passed", "{}\n{}", v["text"], v["checks"]);
    assert!(note_of(&v, "long_reply").contains("still read once the reply had pushed the banner away"), "{}", note_of(&v, "long_reply"));
}

#[test]
fn an_agent_that_shows_its_model_only_on_the_banner_is_not_failed_for_it() {
    // the capture names the model (its footer did), but this agent's screens after a long reply show it nowhere
    let named = Agent { model: "zeta", footer_model: true };
    let bare = Agent { model: "zeta", footer_model: false };
    let f = fixture("vw-banner-only-agent", draft_with(&format!("{WORK_TABLES}\n{BANNER_ONLY}")), named);
    // the agent under test is the one without the footer row
    script(&f.host, &f.words, bare);
    let v = f.run();
    assert_eq!(status_of(&v, "long_reply"), "passed", "{}\n{}", v["text"], v["checks"]);
    assert!(note_of(&v, "long_reply").contains("only on the banner"), "{}", note_of(&v, "long_reply"));
}
