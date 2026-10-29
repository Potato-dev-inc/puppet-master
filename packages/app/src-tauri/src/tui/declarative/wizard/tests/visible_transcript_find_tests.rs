//! "The wizard finds the transcript" through the whole visible path: the made-up agent writes a JSONL transcript while it
//! answers the test message, the visible test finds it, and `finish` writes the definition with a `[session_chat]` table
//! that the real reader reads the same lines with. The scratch tree plays the person's home; no real folder is touched.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};

use crate::session_chat::{ChatItem, DeclaredSource, Located, Names, PaneFacts, SessionFormat, SessionSource};
use crate::tui::declarative::schema::parse_definition;
use crate::tui::declarative::wizard::transcript::{Homes, MARK};
use crate::tui::declarative::wizard::visible::host::Delivered;
use crate::tui::declarative::wizard::Wizard;

use super::fake_host::{screens, wait_for, working_then_reply, FakeHost};
use super::support::{ok, probed, provenance, visible_draft, with_captures, Tree};

const FLAT: &str = r#"{"role":"user","content":"@@TASK@@"}
{"role":"assistant","content":"@@WORD@@"}
"#;

/// A made-up agent that, on the plain test message, answers on the screen and appends `lines` (with `@@TASK@@` and
/// `@@WORD@@` filled in) to `<place(cwd)>/session.jsonl`.
fn agent_writing(host: &Arc<FakeHost>, place: impl Fn(&Path) -> PathBuf + Send + Sync + 'static, lines: &'static str) {
    host.on_deliver(move |h, pane, text| {
        if text.contains("hello.txt") {
            h.set_screen_of(pane, screens::PERMISSION);
            h.set_card(Some(json!({"kind": "command_approval", "choices": ["allow_once", "deny"]})));
            return Delivered::Submitted;
        }
        let word = text.rsplit(' ').next().unwrap_or("").to_string();
        let cwd = h.spawned()[0].cwd.clone();
        let file = place(&cwd).join("session.jsonl");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let body = lines.replace("@@TASK@@", text).replace("@@WORD@@", &word);
        std::fs::write(&file, body).unwrap();
        h.set_screen_of(pane, &screens::working(&word, 0));
        h.schedule(pane, &working_then_reply(&word, &word));
        Delivered::Submitted
    });
}

struct Run {
    tree: Tree,
    wizard: Arc<Wizard>,
    id: String,
    visible: Value,
}

fn run(name: &str, home: bool, place: impl Fn(&Path) -> PathBuf + Send + Sync + 'static, lines: &'static str) -> Run {
    let tree = Tree::new(name);
    let root = tree.root.join("person");
    std::fs::create_dir_all(&root).unwrap();
    let (wizard, host) = tree.visible_wizard_with(|env| {
        if home {
            env.homes = Homes { home: Some(root.clone()), appdata: None, localappdata: None };
        }
    });
    let wizard = Arc::new(wizard);
    let id = probed(&wizard, "fake-agent");
    with_captures(&wizard, &id);
    let v = ok(&wizard, "verify", json!({"session": id, "toml": visible_draft("fake-agent")}));
    let plan = v["planned_visible"]["fingerprint"].as_str().unwrap().to_string();
    agent_writing(&host, place, lines);
    let (w, i) = (wizard.clone(), id.clone());
    let before = host.spawned().len();
    let thread = std::thread::spawn(move || {
        crate::tui::declarative::wizard::api::handle(&w, "visibletest", &json!({"session": i, "confirm": true, "plan": plan}))
    });
    wait_for(super::fake_host::PANE_WAIT_SECS, || (host.spawned().len() > before).then_some(())).expect("the test pane is started");
    host.set_screen(screens::IDLE);
    let visible = thread.join().unwrap().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(visible["ok"], true, "{}\n{}", visible["text"], visible["checks"]);
    Run { tree, wizard, id, visible }
}

/// The items the real reader makes of the transcript, through the table of the written definition.
fn read_back(written: &str, cwd: &Path, home: Option<&Path>, file: &Path) -> Vec<ChatItem> {
    let def = parse_definition(written).expect("the written definition parses");
    let table = def.session_chat.expect("the written definition has the table");
    let source = DeclaredSource::new(table).expect("the table is valid");
    let names = Names { home, cwd, env: &|_| None };
    let pane = PaneFacts { cwd: cwd.to_path_buf(), started_at: std::time::SystemTime::now() - std::time::Duration::from_secs(60), provider_session_id: None, home: None, launch: Default::default(), linux: None };
    let Located::Found(found) = source.locate_with(&pane, &names) else { panic!("the table does not lead to the file") };
    assert_eq!(std::fs::canonicalize(found).unwrap(), std::fs::canonicalize(file).unwrap());
    let text = std::fs::read_to_string(file).unwrap();
    text.lines().enumerate().flat_map(|(at, line)| source.format().parse_line(line, at as u64 * 1000)).collect()
}

#[test]
fn a_transcript_in_the_test_folder_is_found_written_into_the_definition_and_read_back_by_the_real_reader() {
    let r = run("tf-cwd", false, |cwd| cwd.to_path_buf(), FLAT);
    assert_eq!(r.visible["transcript"], json!({"found": true, "dir": "{cwd}", "tool_lines": false}));
    let done = ok(&r.wizard, "finish", json!({"session": r.id, "provenance": provenance()}));
    assert_eq!(done["state"], "needs_approval");
    let toml_path = r.tree.agents.join("nimbus.toml");
    let written = std::fs::read_to_string(&toml_path).unwrap();
    assert!(written.starts_with(&visible_draft("fake-agent")), "the tested draft is first and unchanged");
    assert!(written.contains(MARK) && written.contains("[session_chat]") && written.contains("dir = \"{cwd}\""), "{written}");
    // the wizard never approves it: the person does, and the table is part of what they approve
    assert!(!r.tree.agents.join("agent-approvals.json").exists());
    let prov: Value = serde_json::from_str(&std::fs::read_to_string(r.tree.agents.join("nimbus.wizard.json")).unwrap()).unwrap();
    assert_eq!(prov["transcript"], json!({"found": true, "dir": "{cwd}", "tool_lines": false, "added": true}));
    let shown = prov.to_string();
    assert!(!shown.contains("Reply with exactly"), "no line of the transcript is kept");
}

#[test]
fn the_written_table_reads_the_same_file_with_the_real_reader() {
    let r = run("tf-read", false, |cwd| cwd.to_path_buf(), FLAT);
    let slot = r.tree.root.join("copy");
    std::fs::create_dir_all(&slot).unwrap();
    let done = ok(&r.wizard, "finish", json!({"session": r.id, "provenance": provenance()}));
    let written = std::fs::read_to_string(done["file"].as_str().unwrap()).unwrap();
    // a project folder of the person: the table says {cwd}, so the same table finds a transcript kept in any project folder
    let project = slot.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let file = project.join("session.jsonl");
    std::fs::write(&file, FLAT.replace("@@TASK@@", "Hello there").replace("@@WORD@@", "Hi!")).unwrap();
    let items = read_back(&written, &project, None, &file);
    assert!(items.iter().any(|i| matches!(i, ChatItem::User { text, .. } if text == "Hello there")), "{items:?}");
    assert!(items.iter().any(|i| matches!(i, ChatItem::Assistant { text, .. } if text == "Hi!")), "{items:?}");
}

#[test]
fn a_transcript_under_the_program_folder_in_home_is_written_with_home_and_the_slug() {
    let r = run("tf-home", true, |cwd| {
        let slug = crate::session_chat::cwd_placeholders(cwd).into_iter().find(|(n, _)| *n == "cwd_slug").unwrap().1;
        // the scratch "person" folder is the home of this test
        cwd.ancestors().find(|a| a.join("person").is_dir()).unwrap().join("person").join(".fake-agent").join("sessions").join(slug)
    }, FLAT);
    assert_eq!(r.visible["transcript"], json!({"found": true, "dir": "{home}/.fake-agent/sessions/{cwd_slug}", "tool_lines": false}));
    let done = ok(&r.wizard, "finish", json!({"session": r.id, "provenance": provenance()}));
    let written = std::fs::read_to_string(done["file"].as_str().unwrap()).unwrap();
    assert!(written.contains("dir = \"{home}/.fake-agent/sessions/{cwd_slug}\""), "{written}");
}

#[test]
fn nothing_found_writes_the_draft_as_tested_and_says_why() {
    let tree = Tree::new("tf-none");
    let (wizard, host) = tree.visible_wizard();
    let wizard = Arc::new(wizard);
    let id = probed(&wizard, "fake-agent");
    with_captures(&wizard, &id);
    let v = ok(&wizard, "verify", json!({"session": id, "toml": visible_draft("fake-agent")}));
    let live = super::fake_host::run_visible_test(&wizard, &host, &id, v["planned_visible"]["fingerprint"].as_str().unwrap()).unwrap();
    assert_eq!(live["transcript"]["found"], false);
    assert!(live["transcript"]["why"].as_str().unwrap().contains("no .jsonl file"), "{}", live["transcript"]);
    ok(&wizard, "finish", json!({"session": id, "provenance": provenance()}));
    assert_eq!(std::fs::read_to_string(tree.agents.join("nimbus.toml")).unwrap(), visible_draft("fake-agent"), "written exactly as tested");
    let prov: Value = serde_json::from_str(&std::fs::read_to_string(tree.agents.join("nimbus.wizard.json")).unwrap()).unwrap();
    assert_eq!(prov["transcript"]["found"], false);
    assert!(prov["transcript"]["why"].is_string());
}

#[test]
fn a_transcript_that_does_not_hold_the_test_turn_adds_nothing_and_records_why() {
    let r = run("tf-wrong", false, |cwd| cwd.to_path_buf(), "{\"event\":\"boot\"}\n{\"event\":\"idle\"}\n");
    assert_eq!(r.visible["transcript"]["found"], false);
    assert!(r.visible["transcript"]["why"].as_str().unwrap().contains("holds the message"), "{}", r.visible["transcript"]);
    ok(&r.wizard, "finish", json!({"session": r.id, "provenance": provenance()}));
    assert_eq!(std::fs::read_to_string(r.tree.agents.join("nimbus.toml")).unwrap(), visible_draft("fake-agent"));
}
