//! Collision Alert, end to end (SPEC n33 section 5, CA-16): zero tokens, `pm-fake-tui` as both agents on real
//! ConPTYs, the real pane list, the real `collision-watch` thread with its real git probe, the real attention list.
//! Only the agents are fake. `PM_FAKE_COLLISION_POLL_MS` makes the polls fast (the app waits 8 s and then 20 s).
//!
//! 1. Two agents, each in its own separate copy (`isolation: worktree`), both write line 50 of `src/auth.ts`: one
//!    `clash` alert names both panes and `Line 50`, a Ready-lane item `collision:<id>` exists, the number of things
//!    that block an agent is unchanged, and the fakes get no key, no prompt and no interrupt after the alert.
//! 2. One writes line 50, the other line 90: `same_file`, "No clash yet", no attention item.
//! 3. They write different files: no alert at all.
//! 4. One agent ends its turn: the advice names the stopped one first.
//! 5. Two agents in ONE folder, both working, a file written meanwhile: `shared_folder`.
//! 6. `Ignore` hides the alert and its phone item; a second clashing file brings it back.
//! 7. The setting off: the alert and its item go, and no git call is made any more.
//! 8. A hostile `.git/config` (an fsmonitor command that writes a marker file): the marker never appears.
//!
//! Included from `fake_tui_e2e.rs` (one test binary, one World per process).

use puppet_master_app_lib::fake_tui::{Session, World};
use serde_json::{json, Value};
use std::time::Duration;

const A: &str = "fake_claude";
const B: &str = "fake_codex";
const AUTH: &str = "src/auth.ts";
const DB: &str = "src/db.ts";
/// How long a test waits for the watch to see something. A look costs a second or two on a busy machine, the next gap
/// is 20 times that (at most 2 minutes), and the first test of a run pays for cold caches.
const ALERT_SECS: u64 = 240;
const FOOTER: &str = "Nothing was paused. You decide.";

/// Fast polls for the length of one test, and the setting back on when the test ends, however it ends.
struct Fast<'a>(&'a Session);

impl<'a> Fast<'a> {
    fn on(session: &'a Session) -> Fast<'a> {
        std::env::set_var("PM_FAKE_COLLISION_POLL_MS", "500");
        session.collision_set_enabled(true);
        Fast(session)
    }
}

impl Drop for Fast<'_> {
    fn drop(&mut self) {
        std::env::remove_var("PM_FAKE_COLLISION_POLL_MS");
        self.0.collision_set_enabled(true);
    }
}

// ---- the project and the scripts ---------------------------------------------------------------------------------

fn git(dir: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(["-c", "user.name=pm", "-c", "user.email=pm@example.invalid"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A file of 100 numbered lines, so a line number in a test is easy to read. `edits` replace whole lines.
fn lines(name: &str, edits: &[(usize, &str)]) -> String {
    let mut text = String::new();
    for n in 1..=100 {
        match edits.iter().find(|(at, _)| *at == n) {
            Some((_, replacement)) => text.push_str(replacement),
            None => text.push_str(&format!("export const {name}{n} = {n};")),
        }
        text.push('\n');
    }
    text
}

/// `src/auth.ts` and `src/db.ts` are committed: a separate copy starts from this commit.
fn seed(session: &Session) {
    let src = session.project().join("src");
    std::fs::create_dir_all(&src).expect("src dir");
    std::fs::write(session.project().join(AUTH), lines("auth", &[])).expect("auth.ts");
    std::fs::write(session.project().join(DB), lines("db", &[])).expect("db.ts");
    git(session.project(), &["add", "."]);
    git(session.project(), &["commit", "-q", "-m", "auth and db"]);
}

/// One thing the fake does: write a whole file, then work for `then_ms`.
struct Write {
    path: &'static str,
    content: String,
    then_ms: u64,
}

fn write(path: &'static str, content: String, then_ms: u64) -> Write {
    Write { path, content, then_ms }
}

/// The script of one fake: think for `first_ms`, make the writes, then either end the turn or work for a long time
/// (the session closes the pane when the test ends).
fn script(agent: &str, first_ms: u64, writes: &[Write], ends: bool) -> String {
    let mut steps = vec![format!(r#"{{ do = "think", ms = {first_ms} }}"#)];
    for w in writes {
        let content = w.content.replace('\n', "\\n");
        steps.push(format!(r#"{{ do = "write_file", path = "{}", content = "{content}" }}"#, w.path));
        steps.push(format!(r#"{{ do = "think", ms = {} }}"#, w.then_ms));
    }
    steps.push(if ends {
        r#"{ do = "say", text = "Done." }"#.to_string()
    } else {
        r#"{ do = "think", ms = 900000 }"#.to_string()
    });
    let layout = if agent == A { "claude" } else { "codex" };
    format!("agent = \"{layout}\"\nboot_ms = 100\n[[turn]]\nsteps = [\n  {}\n]\n", steps.join(",\n  "))
}

/// Like `script`, with a shell command in the middle (the agent runs it and waits for it to finish).
fn script_with(agent: &str, first_ms: u64, before: &[Write], command: &str, after: &[Write]) -> String {
    let base = script(agent, first_ms, before, false);
    let tail = after
        .iter()
        .map(|w| {
            let content = w.content.replace('\n', "\\n");
            format!(r#"{{ do = "write_file", path = "{}", content = "{content}" }}"#, w.path)
        })
        .collect::<Vec<_>>()
        .join(",\n  ");
    let run = format!(r#"{{ do = "run", command = "{}" }}"#, command.replace('"', "\\\""));
    // `script` ends with the long think; the run and the second writes go in front of it.
    let long = r#"{ do = "think", ms = 900000 }"#;
    base.replace(long, &format!("{run},\n  {tail},\n  {long}"))
}

/// Both agents start in their own separate copy of the project. Returns the two pane ids.
fn start_pair(session: &Session, a: &str, b: &str, isolation: bool) -> (String, String) {
    session.script_for(A, a);
    session.script_for(B, b);
    let extra = if isolation { json!({ "isolation": "worktree" }) } else { json!({}) };
    let first = session.run(A, "Fix the sign-in code.", extra.clone()).unwrap_or_else(|e| panic!("run_agent refused: {e}"));
    let second = session.run(B, "Fix the sign-in code too.", extra).unwrap_or_else(|e| panic!("run_agent refused: {e}"));
    let pane_of = |run: &Value| {
        let handle = run["handle"].as_str().expect("a handle").to_string();
        session
            .until(90, || session.view(&handle)["pane_id"].as_str().filter(|p| !p.is_empty()).map(str::to_string))
            .expect("the run opened a pane")
    };
    let pa = pane_of(&first);
    let pb = pane_of(&second);
    if isolation {
        for pane in [&pa, &pb] {
            let cwd = session.pane_cwd(pane).expect("pane folder");
            assert!(cwd.contains(".puppet-master"), "the pane works in its own copy: {cwd}");
        }
        assert_ne!(session.pane_cwd(&pa), session.pane_cwd(&pb), "two separate copies");
    }
    (pa, pb)
}

// ---- reading the screen ------------------------------------------------------------------------------------------

/// Nothing of an earlier test is still on the list (its panes are gone, its alerts resolve within a few polls).
fn settle(session: &Session) {
    session
        .until(60, || (session.collision_list().is_empty() && session.collision_attention_items().is_empty()).then_some(()))
        .unwrap_or_else(|| panic!("an earlier test's alerts did not go: {:#?}", session.collision_list()));
}

fn explain(session: &Session, what: &str, panes: &(String, String)) -> ! {
    panic!(
        "{what}\n-- alerts: {:#?}\n-- items: {:#?}\n-- statuses: {:?} / {:?}\n-- probe calls: {}\n-- {A} events: {:#?}\n-- {B} events: {:#?}",
        session.collision_list(),
        session.collision_attention_items(),
        session.pane_status(&panes.0),
        session.pane_status(&panes.1),
        session.collision_probe_calls(),
        session.events_for(A),
        session.events_for(B),
    )
}

fn alert_with(session: &Session, secs: u64, what: &str, panes: &(String, String), keep: impl Fn(&Value) -> bool) -> Value {
    session
        .until(secs, || session.collision_list().into_iter().find(|alert| keep(alert)))
        .unwrap_or_else(|| explain(session, what, panes))
}

fn level(level: &'static str) -> impl Fn(&Value) -> bool {
    move |alert| alert["level"] == level
}

fn has_pane(alert: &Value, pane: &str) -> bool {
    alert["sides"].as_array().is_some_and(|sides| sides.iter().any(|side| side["paneId"] == pane))
}

fn label_of(alert: &Value, pane: &str) -> String {
    alert["sides"]
        .as_array()
        .and_then(|sides| sides.iter().find(|side| side["paneId"] == pane))
        .and_then(|side| side["label"].as_str())
        .unwrap_or_default()
        .to_string()
}

fn file_paths(alert: &Value) -> Vec<String> {
    alert["files"].as_array().map(|files| files.iter().filter_map(|f| f["path"].as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn item_for(session: &Session, id: &str) -> Option<Value> {
    session.collision_attention_items().into_iter().find(|item| item["key"].as_str().is_some_and(|key| key.contains(id)))
}

fn events_named(events: &[Value], name: &str) -> usize {
    events.iter().filter(|event| event["ev"] == name).count()
}

/// What may reach a fake after its task: nothing. The task itself is the one and only prompt.
const NEVER: &[&str] = &["interrupted", "typed_at_gate", "gate_answer", "approval_answer"];

fn assert_untouched(session: &Session, what: &str) {
    for agent in [A, B] {
        let events = session.events_for(agent);
        assert_eq!(events_named(&events, "prompt"), 1, "{what}: {agent} got more than its task: {events:#?}");
        for ev in NEVER {
            assert_eq!(events_named(&events, ev), 0, "{what}: {agent} saw `{ev}`: {events:#?}");
        }
        // Keys before the prompt are the delivery's own (clear the composer, Enter that sends the task).
        let prompt_at = events.iter().find(|e| e["ev"] == "prompt").and_then(|e| e["t_ms"].as_u64()).unwrap_or(0);
        let strays: Vec<&Value> =
            events.iter().filter(|e| e["ev"] == "key" && e["t_ms"].as_u64().unwrap_or(0) > prompt_at).collect();
        assert!(strays.is_empty(), "{what}: {agent} was sent a key after its task: {strays:#?}");
    }
}

/// An alert's text holds no folder of the machine, whoever reads it.
fn assert_private(session: &Session, alert: &Value, item: Option<&Value>) {
    let mut texts = vec![alert.to_string()];
    texts.extend(item.map(Value::to_string));
    let project = session.project().to_string_lossy().replace('\\', "/").to_lowercase();
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default().replace('\\', "/").to_lowercase();
    for text in texts {
        let flat = text.replace("\\\\", "/").replace('\\', "/").to_lowercase();
        assert!(!flat.contains(&project), "a folder of the project is in {text}");
        assert!(home.is_empty() || !flat.contains(&home), "the home folder is in {text}");
        assert!(!flat.contains(".puppet-master"), "the copy's folder is in {text}");
    }
}

// ---- 1. a clash in two separate copies ----------------------------------------------------------------------------

/// Both write line 50 of `src/auth.ts` in their own copy. See the file header.
#[test]
fn two_copies_changing_the_same_line_raise_one_clash_and_nothing_is_paused() {
    let session = World::get().session("ca-clash");
    let _fast = Fast::on(&session);
    seed(&session);
    settle(&session);
    let (blocking_before, _) = session.attention_counts();
    let a = script(A, 2500, &[write(AUTH, lines("auth", &[(50, "export const line50 = 'edited by A';")]), 0)], false);
    let b = script(B, 2500, &[write(AUTH, lines("auth", &[(50, "export const line50 = 'edited by B';")]), 0)], false);
    let panes = start_pair(&session, &a, &b, true);

    let alert = alert_with(&session, ALERT_SECS, "no clash alert", &panes, level("clash"));
    assert!(has_pane(&alert, &panes.0) && has_pane(&alert, &panes.1), "{alert}");
    assert_eq!(alert["relation"], "copies", "{alert}");
    assert_eq!(alert["place"], "In separate copies.", "{alert}");
    assert_eq!(file_paths(&alert), vec![AUTH.to_string()], "{alert}");
    let (name_a, name_b) = (label_of(&alert, &panes.0), label_of(&alert, &panes.1));
    assert!(!name_a.is_empty() && !name_b.is_empty() && name_a != name_b, "{alert}");
    let title = alert["title"].as_str().unwrap_or_default();
    assert!(title.contains(&name_a) && title.contains(&name_b) && title.ends_with(&format!("both changed {AUTH}")), "{title}");
    assert!(alert["detail"].as_str().unwrap_or_default().starts_with("Line 50 overlaps."), "{alert}");
    let advice = alert["advice"].as_str().unwrap_or_default();
    assert!(advice.starts_with("Finish ") && advice.contains("can build on it."), "{alert}");
    assert_eq!(alert["footer"], FOOTER, "{alert}");
    assert_eq!(session.collision_list().len(), 1, "one alert for the pair: {:#?}", session.collision_list());

    // The phone's item: Ready lane, a review, no buttons. The numbers that block an agent did not move.
    let id = alert["id"].as_str().expect("an alert id").to_string();
    let item = session.until(20, || item_for(&session, &id)).unwrap_or_else(|| explain(&session, "no attention item", &panes));
    assert_eq!(item["lane"], "ready", "{item}");
    assert_eq!(item["kind"], "review", "{item}");
    assert_eq!(item["actions"].as_array().map(Vec::len), Some(0), "no buttons on the phone: {item}");
    assert_eq!(item["title"], alert["title"], "{item}");
    assert_eq!(session.attention_counts().0, blocking_before, "the Blocking count is unchanged: {item}");
    assert_private(&session, &alert, Some(&item));

    // Nothing was paused, stopped or typed: both are still working, and the fakes saw nothing but their task.
    let before: Vec<usize> = [A, B].iter().map(|agent| session.events_for(agent).len()).collect();
    std::thread::sleep(Duration::from_millis(4000));
    for pane in [&panes.0, &panes.1] {
        assert_eq!(session.pane_status(pane).as_deref(), Some("running"), "{pane} was not left alone");
    }
    assert_untouched(&session, "after the alert");
    let after: Vec<usize> = [A, B].iter().map(|agent| session.events_for(agent).len()).collect();
    assert_eq!(before, after, "the fakes logged new events after the alert");
    assert_eq!(session.collision_list().len(), 1, "still one alert");
}

// ---- 2. the same file, far apart ----------------------------------------------------------------------------------

/// One writes line 50, the other line 90: the quiet row, and nothing for the phone.
#[test]
fn the_same_file_in_different_places_is_a_quiet_row_with_no_phone_item() {
    let session = World::get().session("ca-apart");
    let _fast = Fast::on(&session);
    seed(&session);
    settle(&session);
    let a = script(A, 2500, &[write(AUTH, lines("auth", &[(50, "export const line50 = 'edited by A';")]), 0)], false);
    let b = script(B, 2500, &[write(AUTH, lines("auth", &[(90, "export const line90 = 'edited by B';")]), 0)], false);
    let panes = start_pair(&session, &a, &b, true);

    let alert = alert_with(&session, ALERT_SECS, "no same_file alert", &panes, level("same_file"));
    assert!(has_pane(&alert, &panes.0) && has_pane(&alert, &panes.1), "{alert}");
    assert_eq!(file_paths(&alert), vec![AUTH.to_string()], "{alert}");
    assert_eq!(alert["detail"], "Different places in the file. No clash yet.", "{alert}");
    assert!(alert["advice"].is_null(), "a quiet row gives no advice: {alert}");
    assert_eq!(alert["footer"], FOOTER, "{alert}");

    // It stays quiet: several more polls, still no clash and never a phone item.
    std::thread::sleep(Duration::from_millis(4000));
    assert!(session.collision_list().iter().all(|a| a["level"] == "same_file"), "{:#?}", session.collision_list());
    assert!(session.collision_attention_items().is_empty(), "{:#?}", session.collision_attention_items());
    assert_untouched(&session, "quiet row");
}

// ---- 3. different files -------------------------------------------------------------------------------------------

/// One writes `auth.ts`, the other `db.ts`: no alert at all.
#[test]
fn different_files_raise_nothing() {
    let session = World::get().session("ca-different");
    let _fast = Fast::on(&session);
    seed(&session);
    settle(&session);
    let a = script(A, 2000, &[write(AUTH, lines("auth", &[(50, "export const line50 = 'edited by A';")]), 0)], false);
    let b = script(B, 2000, &[write(DB, lines("db", &[(50, "export const line50 = 'edited by B';")]), 0)], false);
    let panes = start_pair(&session, &a, &b, true);

    // Both really wrote their file ...
    let wrote = |agent: &str| {
        session
            .until(60, || session.events_for(agent).iter().any(|e| e["ev"] == "write_file" && e["ok"] == true).then_some(()))
            .is_some()
    };
    assert!(wrote(A) && wrote(B), "the fakes did not write: {:#?} / {:#?}", session.events_for(A), session.events_for(B));
    // ... and the watch had time for several looks (the poll is 0.5 s, the thread ticks every second).
    // (A look on a busy machine costs a second or two and the next gap is 20 times that.)
    let calls = session.collision_probe_calls();
    session.until(120, || (session.collision_probe_calls() >= calls + 4).then_some(())).unwrap_or_else(|| explain(&session, "the watch did not look", &panes));
    assert!(session.collision_list().is_empty(), "{:#?}", session.collision_list());
    assert!(session.collision_attention_items().is_empty(), "{:#?}", session.collision_attention_items());
}

// ---- 4. one agent has stopped ---------------------------------------------------------------------------------------

/// A ends its turn, B keeps working: the advice says to finish the stopped one first.
#[test]
fn when_one_agent_ends_its_turn_the_advice_names_it_first() {
    let session = World::get().session("ca-stopped");
    let _fast = Fast::on(&session);
    seed(&session);
    settle(&session);
    let a = script(A, 2500, &[write(AUTH, lines("auth", &[(50, "export const line50 = 'edited by A';")]), 5000)], true);
    let b = script(B, 2500, &[write(AUTH, lines("auth", &[(50, "export const line50 = 'edited by B';")]), 0)], false);
    let panes = start_pair(&session, &a, &b, true);

    let alert = alert_with(&session, ALERT_SECS, "no clash alert", &panes, level("clash"));
    let name_a = label_of(&alert, &panes.0);
    let stopped = |alert: &Value| {
        alert["level"] == "clash"
            && alert["advice"].as_str().is_some_and(|text| text.starts_with(&format!("Finish {name_a} first. It has stopped working.")))
    };
    let alert = alert_with(&session, ALERT_SECS, "the advice never named the stopped agent first", &panes, stopped);
    let advice = alert["advice"].as_str().unwrap_or_default();
    let name_b = label_of(&alert, &panes.1);
    assert!(advice.contains("Look at its changes and merge them first."), "{advice}");
    assert!(advice.ends_with(&format!("Then {name_b} can build on it.")), "{advice}");
    let side = |pane: &str| alert["sides"].as_array().and_then(|s| s.iter().find(|x| x["paneId"] == pane)).cloned().unwrap_or(Value::Null);
    assert_eq!(side(&panes.0)["working"], false, "{alert}");
    assert_eq!(side(&panes.1)["working"], true, "{alert}");
    assert_eq!(session.pane_status(&panes.1).as_deref(), Some("running"), "B was left alone");
    let events = session.events_for(A);
    assert_eq!(events_named(&events, "turn_done"), 1, "A's turn ended by itself: {events:#?}");
    assert_untouched(&session, "advice");
}

// ---- 5. two agents in one folder -----------------------------------------------------------------------------------

/// Both work in the project folder itself; one keeps writing a file. They can overwrite each other: the softer alert,
/// worded "changed while both worked" (nobody can tell who wrote what).
#[test]
fn two_agents_in_one_folder_get_the_shared_folder_alert() {
    let session = World::get().session("ca-same-folder");
    let _fast = Fast::on(&session);
    seed(&session);
    settle(&session);
    // A writes again and again (a file written before B started working would not count), B only works.
    let rewrites: Vec<Write> = (0..15)
        .map(|n| write(AUTH, lines("auth", &[(50, &format!("export const line50 = 'edit {n} by A';"))]), 4000))
        .collect();
    let a = script(A, 3000, &rewrites, false);
    let b = script(B, 500, &[], false);
    let panes = start_pair(&session, &a, &b, false);
    assert_eq!(session.pane_cwd(&panes.0), session.pane_cwd(&panes.1), "one folder");

    let alert = alert_with(&session, ALERT_SECS, "no shared_folder alert", &panes, level("shared_folder"));
    assert!(has_pane(&alert, &panes.0) && has_pane(&alert, &panes.1), "{alert}");
    assert_eq!(alert["relation"], "same_folder", "{alert}");
    assert_eq!(alert["place"], "In the same folder.", "{alert}");
    let (name_a, name_b) = (label_of(&alert, &panes.0), label_of(&alert, &panes.1));
    assert!(alert["title"].as_str().unwrap_or_default().ends_with("are working in the same folder"), "{alert}");
    assert!(alert["title"].as_str().unwrap_or_default().contains(&name_a) && alert["title"].as_str().unwrap_or_default().contains(&name_b), "{alert}");
    assert!(alert["detail"].as_str().unwrap_or_default().contains(&format!("Changed while both worked: {AUTH}")), "{alert}");
    assert_eq!(alert["footer"], FOOTER, "{alert}");
    assert!(file_paths(&alert).contains(&AUTH.to_string()), "{alert}");
    let id = alert["id"].as_str().expect("an alert id").to_string();
    let item = session.until(20, || item_for(&session, &id)).unwrap_or_else(|| explain(&session, "no attention item", &panes));
    assert_eq!(item["lane"], "ready", "{item}");
    assert_private(&session, &alert, Some(&item));
    assert_untouched(&session, "same folder");
}

// ---- 6. ignore ----------------------------------------------------------------------------------------------------

/// `Ignore` hides the alert and its phone item. A second file that clashes brings it back.
#[test]
fn ignore_hides_the_alert_and_a_new_clashing_file_brings_it_back() {
    let session = World::get().session("ca-ignore");
    let _fast = Fast::on(&session);
    seed(&session);
    settle(&session);
    // Both write auth.ts, then wait for the test's go-ahead file (the test is slower or faster than any clock), then db.ts.
    let go = session.aux().join("go.txt");
    let go_sh = go.to_string_lossy().replace('\\', "/");
    let wait_for_go = format!(r#"powershell -NoProfile -Command "while (-not (Test-Path '{go_sh}')) {{ Start-Sleep -Milliseconds 200 }}""#);
    let one = |who: &str| write(AUTH, lines("auth", &[(50, &format!("export const line50 = 'edited by {who}';"))]), 0);
    let two = |who: &str| write(DB, lines("db", &[(10, &format!("export const line10 = 'edited by {who}';"))]), 0);
    let a = script_with(A, 2500, &[one("A")], &wait_for_go, &[two("A")]);
    let b = script_with(B, 2500, &[one("B")], &wait_for_go, &[two("B")]);
    let panes = start_pair(&session, &a, &b, true);

    let alert = alert_with(&session, ALERT_SECS, "no clash alert", &panes, level("clash"));
    let id = alert["id"].as_str().expect("an alert id").to_string();
    assert_eq!(file_paths(&alert), vec![AUTH.to_string()], "only auth.ts so far: {alert}");
    session.until(20, || item_for(&session, &id)).unwrap_or_else(|| explain(&session, "no attention item", &panes));

    session.collision_ignore(&id).unwrap_or_else(|e| panic!("ignore refused: {e}"));
    assert!(session.collision_list().is_empty(), "the alert left at once: {:#?}", session.collision_list());
    session.until(10, || item_for(&session, &id).is_none().then_some(())).unwrap_or_else(|| explain(&session, "the phone item stayed", &panes));
    // The same file again (the polls go on) does not bring it back.
    std::thread::sleep(Duration::from_millis(4000));
    assert!(session.collision_list().is_empty(), "still hidden: {:#?}", session.collision_list());
    // An unknown id changes nothing and is refused.
    let refused = session.collision_ignore("ca-000000000000").expect_err("an unknown id");
    assert!(refused.starts_with("COLLISION_NOT_FOUND"), "{refused}");

    // db.ts is written by both once the go-ahead is there: a new clash, so the alert is back.
    std::fs::write(&go, "go").expect("go-ahead file");
    let back = alert_with(&session, 120, "the alert did not come back for db.ts", &panes, |a| a["level"] == "clash" && file_paths(a).contains(&DB.to_string()));
    assert_eq!(back["id"], id.as_str(), "the same pair, the same id");
    session.until(20, || item_for(&session, &id)).unwrap_or_else(|| explain(&session, "no attention item after it came back", &panes));
    assert_untouched(&session, "ignore");
}

// ---- 7. the setting off ----------------------------------------------------------------------------------------------

/// "Warn me when two agents change the same file" off: the alert and its phone item go, and no git call is made while
/// it stays off. On again, the alert comes back.
#[test]
fn switching_the_setting_off_clears_the_alert_and_stops_every_git_call() {
    let session = World::get().session("ca-off");
    let _fast = Fast::on(&session);
    seed(&session);
    settle(&session);
    let a = script(A, 2500, &[write(AUTH, lines("auth", &[(50, "export const line50 = 'edited by A';")]), 0)], false);
    let b = script(B, 2500, &[write(AUTH, lines("auth", &[(50, "export const line50 = 'edited by B';")]), 0)], false);
    let panes = start_pair(&session, &a, &b, true);
    let alert = alert_with(&session, ALERT_SECS, "no clash alert", &panes, level("clash"));
    let id = alert["id"].as_str().expect("an alert id").to_string();
    session.until(20, || item_for(&session, &id)).unwrap_or_else(|| explain(&session, "no attention item", &panes));
    assert!(session.collision_probe_calls() > 0, "the watch ran git");

    session.collision_set_enabled(false);
    assert!(session.collision_list().is_empty(), "the list is empty at once");
    session
        .until(10, || (session.collision_list().is_empty() && item_for(&session, &id).is_none()).then_some(()))
        .unwrap_or_else(|| explain(&session, "the alert or its item stayed after the switch", &panes));
    // A look that was running when the switch was thrown may still finish: let it, then the count must stand still.
    std::thread::sleep(Duration::from_millis(2500));
    let calls = session.collision_probe_calls();
    std::thread::sleep(Duration::from_millis(6000));
    assert_eq!(session.collision_probe_calls(), calls, "git was started while the setting was off");
    assert!(session.collision_list().is_empty() && session.collision_attention_items().is_empty());
    assert_untouched(&session, "setting off");

    session.collision_set_enabled(true);
    alert_with(&session, 60, "the alert did not come back after the switch went on", &panes, level("clash"));
}

// ---- 8. a hostile .git/config -------------------------------------------------------------------------------------

/// An agent can write `.git/config`. An fsmonitor command that writes a marker file must never run because of a poll.
#[test]
fn a_hostile_git_config_is_never_run_by_the_watch() {
    let session = World::get().session("ca-hostile");
    let _fast = Fast::on(&session);
    seed(&session);
    settle(&session);
    let a = script(A, 7000, &[write(AUTH, lines("auth", &[(50, "export const line50 = 'edited by A';")]), 0)], false);
    let b = script(B, 7000, &[write(AUTH, lines("auth", &[(50, "export const line50 = 'edited by B';")]), 0)], false);
    let panes = start_pair(&session, &a, &b, true);

    // The copies exist and both agents work; now the config turns hostile (the copies share the project's config).
    let marker = session.aux().join("marker.txt");
    let marker_sh = marker.to_string_lossy().replace('\\', "/");
    let hostile = format!("sh -c 'echo ran > \"{marker_sh}\"'");
    git(session.project(), &["config", "core.fsmonitor", &hostile]);
    git(session.project(), &["config", "diff.evil.textconv", &hostile]);
    git(session.project(), &["config", "core.hooksPath", &session.aux().to_string_lossy().replace('\\', "/")]);
    std::fs::write(session.project().join(".git").join("info").join("attributes"), "*.ts diff=evil\n").expect("attributes");
    // `git(...)` above ran plain git in the project: if that made the marker, the trap is live, and it is ours, so reset.
    let _ = std::fs::remove_file(&marker);

    // The polls go on with the hostile config in place and still see the clash.
    let calls = session.collision_probe_calls();
    alert_with(&session, ALERT_SECS, "no clash alert with the hostile config in place", &panes, level("clash"));
    session.until(30, || (session.collision_probe_calls() >= calls + 6).then_some(())).unwrap_or_else(|| explain(&session, "the watch did not keep looking", &panes));
    assert!(!marker.exists(), "the poll ran a command from .git/config (marker written)");

    // The control: plain git in a scratch repo with the same config does run it. So the silence above means something.
    // (Not under `aux`: Windows treats a folder named `aux` as a device and git cannot make `.git` inside it.)
    let scratch = session.project().parent().expect("the test's folder").join("scratch");
    std::fs::create_dir_all(&scratch).expect("scratch");
    git(&scratch, &["init", "-q"]);
    std::fs::write(scratch.join("a.ts"), "a\n").expect("file");
    git(&scratch, &["add", "."]);
    git(&scratch, &["commit", "-q", "-m", "i"]);
    git(&scratch, &["config", "core.fsmonitor", &hostile]);
    git(&scratch, &["status"]);
    assert!(marker.exists(), "the control repo did not run its fsmonitor, so this machine cannot show the trap");
}
