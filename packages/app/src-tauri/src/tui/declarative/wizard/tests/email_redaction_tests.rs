//! Email addresses never leave the wizard (SPEC 11.21): every screen, prompt, fixture, record and message the wizard
//! saves or sends holds `<email>` where the program showed the person's address. Test addresses are made up
//! (`someone@example.com`).

use std::sync::Arc;

use serde_json::{json, Value};

use crate::tui::declarative::check::check_report;
use crate::tui::declarative::wizard::exec;
use crate::tui::declarative::wizard::session::TestWords;
use crate::tui::declarative::wizard::visible::answer::AnsweredQ;
use crate::tui::declarative::wizard::visible::capture::{Action, Capture, CaptureAction, Config};
use crate::tui::declarative::wizard::visible::host::Look;
use crate::tui::declarative::wizard::Wizard;

use super::fake_host::{run_visible_test, FakeHost};
use super::support::{ok, probed, provenance, visible_draft, with_captures, Tree};

const ADDRESS: &str = "someone@example.com";
const ACCOUNT: &str = "someone@example.com (Nimbus Starter Quota)";

fn words() -> TestWords {
    TestWords { one: "pong-aaaaaa".into(), two: "pong-bbbbbb".into(), sky: "cccccc".into() }
}

#[test]
fn clean_replaces_an_email_address_and_keeps_the_other_redactions() {
    let raw = format!("\u{1b}[2m{ACCOUNT}\u{1b}[0m\nC:\\Users\\Someone\\proj  token=abcd1234efgh\nssh deploy@buildhost\n");
    let cleaned = exec::clean(&raw);
    assert!(!cleaned.contains(ADDRESS), "{cleaned}");
    assert!(cleaned.contains("<email> (Nimbus Starter Quota)"), "{cleaned}");
    assert!(cleaned.contains("C:\\Users\\user\\proj") && !cleaned.contains("abcd1234efgh"), "the path and the secret rules still hold: {cleaned}");
    assert!(cleaned.contains("deploy@buildhost"), "not email-shaped: {cleaned}");
    assert_eq!(exec::clean(&cleaned), cleaned, "idempotent");
}

fn look(screen: &str) -> Look {
    Look { screen: screen.into(), watch: screen.into(), status: "idle".into(), alive: true, output_pos: None, hello_written: false, grid: Some((120, 40)), history: String::new(), styled: None }
}

#[test]
fn the_capture_keeps_the_screens_its_notes_and_its_debug_frames_without_the_address() {
    let account = format!("NIMBUS 0.4\n\n  {ACCOUNT}\n\n+------------------------------+\n| > Type a task...             |\n+------------------------------+\n");
    let mut c = Capture::new(words(), (120, 40), Config::default());
    let mut now = 0;
    let mut actions: Vec<Action> = Vec::new();
    while now < 6000 {
        now += 250;
        actions.extend(c.tick(&look(&account), now));
    }
    // the person says it is ready: the idle screen is kept
    let _ = c.act(None, CaptureAction::Ready, now);
    let debug = c.debug_frames();
    assert!(!debug.is_empty());
    for f in &debug {
        assert!(!f.text.contains(ADDRESS), "{}", f.text);
    }
    assert!(debug.iter().any(|f| f.text.contains("<email> (Nimbus Starter Quota)")));
    let set = c.into_set();
    let all = format!("{:?}", set.frames.iter().map(|f| (f.text.clone(), f.variants.clone())).collect::<Vec<_>>());
    assert!(!all.contains(ADDRESS), "{all}");
    assert!(all.contains("<email>"), "the idle frame shows the row masked: {all}");
    assert!(!format!("{:?}", set.notes).contains(ADDRESS));
    // what the UI and the drafter get of the captures
    let masked = crate::tui::declarative::wizard::visible::fixtures::masked_json(&set, &words()).to_string();
    assert!(!masked.contains(ADDRESS), "{masked}");
}

/// The recorded capture with an account row on the idle and the reply screens: one as a clean capture makes it
/// (`<email>`), one as a raw address that slipped past (the last net must catch it).
fn account_session(tree: &Tree) -> (Arc<Wizard>, Arc<FakeHost>, String) {
    let (wizard, host) = tree.visible_wizard();
    let wizard = Arc::new(wizard);
    let id = probed(&wizard, "fake-agent");
    with_captures(&wizard, &id);
    wizard
        .read(&id, |s| {
            let mut set = s.visible.captures.lock();
            let set = set.as_mut().expect("captures");
            for f in set.frames.iter_mut() {
                match f.name.as_str() {
                    "tui_idle" => f.text = format!("{}  <email> (Nimbus Starter Quota)\n", f.text),
                    "tui_reply" => f.text = format!("{}  {ACCOUNT}\n", f.text),
                    _ => {}
                }
            }
            set.answered.push(AnsweredQ {
                place: "capture".into(),
                flavour: "trust".into(),
                label: "Yes, I trust this folder".into(),
                keys: "Enter".into(),
                screen: format!("Trust this folder?\n{ACCOUNT}\n"),
                via: "rules".into(),
                read: String::new(),
                denied: None,
            });
        })
        .unwrap();
    (wizard, host, id)
}

#[test]
fn saved_fixtures_and_the_wizard_record_hold_no_address_and_the_fixtures_still_pass() {
    let tree = Tree::new("em-finish");
    let (wizard, host, id) = account_session(&tree);
    let v = ok(&wizard, "verify", json!({"session": id, "toml": visible_draft("fake-agent")}));
    assert_eq!(v["ok"], true, "the masked captures pass the draft: {}", v["text"]);
    let live = run_visible_test(&wizard, &host, &id, v["planned_visible"]["fingerprint"].as_str().unwrap()).unwrap();
    assert_eq!(live["ok"], true, "{}\n{}", live["text"], live["checks"]);
    let done = ok(&wizard, "finish", json!({"session": id, "provenance": provenance()}));
    assert_eq!(done["fixtures"], json!({"passed": 5, "total": 5}), "{done}");
    let agents = &tree.agents;
    let folder = agents.join("nimbus");
    for entry in std::fs::read_dir(&folder).unwrap().flatten() {
        let text = std::fs::read_to_string(entry.path()).unwrap();
        assert!(!text.contains(ADDRESS), "{:?} holds the address:\n{text}", entry.file_name());
    }
    let idle = std::fs::read_to_string(folder.join("tui_idle.txt")).unwrap();
    let reply = std::fs::read_to_string(folder.join("tui_reply.txt")).unwrap();
    assert!(idle.contains("<email> (Nimbus Starter Quota)"), "{idle}");
    assert!(reply.contains("<email> (Nimbus Starter Quota)"), "a raw address that slipped past is masked on the way to the file: {reply}");
    let record = std::fs::read_to_string(agents.join("nimbus.wizard.json")).unwrap();
    assert!(!record.contains(ADDRESS), "{record}");
    assert!(record.contains("<email>"), "the answered question's screen is kept, masked: {record}");
    // and the saved folder passes its own fixtures when checked from the file, like "Check with fixtures"
    let report = check_report(&agents.join("nimbus.toml"), None, &["claude", "codex"]).unwrap();
    assert_eq!(report.status, "ran");
    assert!(report.results.iter().all(|r| r.passed), "{:?}", report.results);
}

#[test]
fn a_definition_that_names_the_placeholder_is_sent_back_and_the_screens_the_drafter_sees_are_masked() {
    let tree = Tree::new("em-verify");
    let (wizard, _host, id) = account_session(&tree);
    let bad = format!("{}\n# the account row is <email> (Nimbus Starter Quota)\n", visible_draft("fake-agent"));
    let v = ok(&wizard, "verify", json!({"session": id, "toml": bad}));
    assert_eq!(v["ok"], false);
    let text = v["text"].as_str().unwrap();
    assert!(text.contains("WIZARD_EMAIL_PLACEHOLDER") && text.contains("S+@"), "{text}");
    // what the front end hands the drafter: the captures, masked
    let captures = ok(&wizard, "capture", json!({"session": id, "op": "captures"}));
    let shown: Value = captures["captures"].clone();
    assert!(!shown.to_string().contains(ADDRESS), "{shown}");
    assert!(shown.to_string().contains("<email>"), "{shown}");
}

#[test]
fn a_regex_that_matches_an_address_passes_the_masked_fixtures_because_they_are_read_back_with_an_example_address() {
    let tree = Tree::new("em-readback");
    let (wizard, host, id) = account_session(&tree);
    let draft = visible_draft("fake-agent").replace(r"  '^done in \d+s$',", "  '^done in \\d+s$',\n  '^\\S+@\\S+\\s+\\(.*quota\\)$',");
    assert!(draft.contains("quota"), "the chrome row was added");
    let expects = json!({"tui_idle": "chrome_lines = [\"<email> (nimbus starter quota)\"]\n"});
    let v = ok(&wizard, "verify", json!({"session": id, "toml": draft, "expect": expects}));
    assert_eq!(v["ok"], true, "{}", v["text"]);
    let live = run_visible_test(&wizard, &host, &id, v["planned_visible"]["fingerprint"].as_str().unwrap()).unwrap();
    assert_eq!(live["ok"], true, "{}", live["text"]);
    let done = ok(&wizard, "finish", json!({"session": id, "provenance": provenance()}));
    assert_eq!(done["fixtures"], json!({"passed": 5, "total": 5}), "{done}");
    let expect = std::fs::read_to_string(tree.agents.join("nimbus").join("tui_idle.expect.toml")).unwrap();
    assert!(expect.contains("<email> (nimbus starter quota)") && !expect.contains(ADDRESS), "{expect}");
}
