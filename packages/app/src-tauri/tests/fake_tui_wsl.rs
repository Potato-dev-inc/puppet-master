//! Linux Inside end to end (SPEC n33, WSL-17): an agent set to run in Linux starts through `pm-fake-wsl`, a stand-in
//! for `wsl.exe` (`tools/pm-fake-tui/src/bin/fake_wsl.rs`), and the fake agent plays inside it. Everything else is the
//! app's own code: settings, probe, place, argv, env, pane, chat, cards, undo, stop. The real `wsl.exe` is never
//! started. The fake logs every call it gets (`Session::fake_wsl_log`), so what crossed the boundary can be read back.
//! Included from `fake_tui_e2e.rs` (one test binary, one World).

use puppet_master_app_lib::fake_tui::{Session, World};
use serde_json::{json, Value};

const DISTRO: &str = "Ubuntu-Test";
const STOPPED: &[&str] = &["completed", "failed", "cancelled", "needs_input"];

const SAY: &str = r#"
agent = "claude"
boot_ms = 200
[[turn]]
steps = [{ do = "think", ms = 300 }, { do = "say", text = "Hello from Linux." }]
"#;

/// Every launch the fake `wsl.exe` was asked for.
fn launches(session: &Session) -> Vec<Value> {
    session.fake_wsl_log().into_iter().filter(|entry| entry["ev"] == "launch").collect()
}

fn launch_of(session: &Session, pane: &str) -> Option<Value> {
    launches(session).into_iter().find(|entry| entry["pane"] == pane)
}

fn run(session: &Session, task: &str, extra: Value) -> Result<Value, Value> {
    let started = session.run("fake_claude", task, extra)?;
    let handle = started["handle"].as_str().expect("handle").to_string();
    Ok(session.wait(&handle, STOPPED, 120_000).unwrap_or_else(|e| panic!("wait failed: {e}")))
}

fn pane_of(done: &Value) -> String {
    done["pane_id"].as_str().unwrap_or_else(|| panic!("the run had no pane: {done}")).to_string()
}

fn code_of(refused: &Value) -> &str {
    refused["code"].as_str().unwrap_or_default()
}

/// The name Claude gives a project folder: every character that is not a letter or digit becomes `-`.
fn encoded(linux_cwd: &str) -> String {
    linux_cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

// ---- 1. the pane opens in Linux --------------------------------------------------------------------------------

/// The pane runs through the fake `wsl.exe`: `PaneInfo.place` is set, the folder is the `/mnt/...` form, the argv
/// holds the program and its flags as separate elements, and of PM's own environment only the two listed names
/// cross. A secret in the test process's environment stays out.
#[test]
fn a_linux_pane_opens_with_its_place_and_only_the_listed_names_cross() {
    let session = World::get().session("wsl-open");
    session.script(SAY);
    session.set_linux("fake_claude", DISTRO);
    std::env::set_var("PM_TEST_SECRET_TOKEN", "do-not-cross");
    let spawned = session.spawn_pane("fake_claude");
    std::env::remove_var("PM_TEST_SECRET_TOKEN");
    let pane = spawned.unwrap_or_else(|e| panic!("the Linux pane did not open: {e}"));

    let launch = session
        .until(60, || launch_of(&session, &pane))
        .unwrap_or_else(|| panic!("the fake wsl.exe was never asked to launch: {:#?}", session.fake_wsl_log()));
    let linux_cwd = session.linux_project_cwd();
    assert!(linux_cwd.starts_with("/mnt/"), "{linux_cwd}");
    assert_eq!(launch["distro"], DISTRO, "{launch}");
    assert_eq!(launch["cd"], linux_cwd.as_str(), "{launch}");
    // The program is the absolute path the probe found; its flags follow as separate elements.
    assert_eq!(launch["program"], "/usr/bin/claude", "{launch}");
    let args: Vec<&str> = launch["args"].as_array().expect("args").iter().filter_map(Value::as_str).collect();
    assert_eq!(args.len(), 2, "{launch}");
    assert_eq!(args[0], "--session-id", "{launch}");
    assert_eq!(args[1].len(), 36, "a session id: {launch}");

    // Only the names PM lists cross, and WSLENV is PM's own string, whatever the process had.
    assert_eq!(launch["wslenv"], "PM_WSL_PATH/u:PUPPET_MASTER_PANE_ID", "{launch}");
    assert_eq!(launch["env"], json!(["PM_WSL_PATH", "PUPPET_MASTER_PANE_ID"]), "{launch}");
    assert!(!launch.to_string().contains("PM_TEST_SECRET_TOKEN") && !launch.to_string().contains("do-not-cross"), "{launch}");

    // The probe asked for the agent's program. (The distro list is cached for 30 s in the process, so a call to `--list` is
    // not asserted: an earlier test may have read it.)
    let log = session.fake_wsl_log();
    let probe = log.iter().find(|entry| entry["ev"] == "probe").unwrap_or_else(|| panic!("no probe: {log:#?}"));
    assert!(probe["programs"].as_array().is_some_and(|p| p.iter().any(|x| x == "claude")), "{probe}");

    // The pane lists its place; its folder stays the Windows one (checkpoints and git keep working there).
    let info = session.pane_info(&pane).expect("the pane is listed");
    assert_eq!(info["place"]["kind"], "linux", "{info}");
    assert_eq!(info["place"]["distro"], DISTRO, "{info}");
    assert_eq!(info["place"]["cwd"], linux_cwd.as_str(), "{info}");
    assert_eq!(info["place"]["mount_root"], "/mnt/", "{info}");
    assert_eq!(info["place"]["home_windows"], session.fake_wsl_home().to_string_lossy().as_ref(), "{info}");
    assert_eq!(
        info["cwd"].as_str().map(str::to_lowercase),
        Some(session.project().to_string_lossy().to_lowercase()),
        "{info}"
    );

    // The real profile reads the fake agent's screen through the fake wsl.exe like any other.
    let ready = session.until(60, || {
        let read = session.read_screen("claude", &session.input_screen(&pane));
        (read["ready"] == "Empty").then_some(read)
    });
    assert!(ready.is_some(), "the composer never showed up:\n{}", session.screen(&pane));
}

// ---- 2. a visible run completes ------------------------------------------------------------------------------

/// `run_agent` with a visible turn: the task is delivered once, the reply is read, the receipt says completed, and the
/// pane the run used is a Linux pane.
#[test]
fn a_visible_run_completes_in_linux_with_a_receipt() {
    let session = World::get().session("wsl-run");
    session.script(SAY);
    session.set_linux("fake_claude", DISTRO);
    let done = run(&session, "say hello", json!({})).unwrap_or_else(|e| panic!("run_agent refused: {e}"));
    assert_eq!(done["status"], "completed", "{done}\n{:#?}", session.fake_wsl_log());
    assert!(done["result"].as_str().unwrap_or_default().contains("Hello from Linux."), "{done}");
    assert_eq!(done["receipt"]["backend"], "fake_claude", "{done}");
    assert_eq!(done["receipt"]["outcome"], "completed_unchecked", "{done}");
    assert!(done["receipt"]["fallback"].is_null(), "no fallback was needed: {done}");

    let pane = pane_of(&done);
    assert_eq!(session.pane_info(&pane).expect("pane")["place"]["kind"], "linux");
    assert!(launch_of(&session, &pane).is_some(), "the run's pane started through the fake wsl.exe: {:#?}", session.fake_wsl_log());
    let prompts: Vec<Value> = session.events().into_iter().filter(|e| e["ev"] == "prompt").collect();
    assert_eq!(prompts.len(), 1, "the task reached the agent once: {prompts:?}");
}

// ---- 3. the Chat tab reads the Linux home ----------------------------------------------------------------------

/// The fake writes its conversation where Claude would inside Linux: `<home>/.claude/projects/<-mnt-...>/<id>.jsonl`
/// (the home is the temp folder the probe reported). The Chat tab finds the file by the folder name of the Linux path
/// and the session id the pane was started with.
#[test]
fn the_chat_items_come_from_the_session_file_in_the_linux_home() {
    let session = World::get().session("wsl-chat");
    let folder = encoded(&session.linux_project_cwd());
    session.script(&format!(
        r#"
agent = "claude"
boot_ms = 200
transcript = ".claude/projects/{folder}/{{arg:--session-id}}.jsonl"
transcript_shape = "nested"
[[turn]]
steps = [{{ do = "think", ms = 300 }}, {{ do = "say", text = "Hello from Linux." }}]
"#
    ));
    session.set_linux("fake_claude", DISTRO);
    let done = run(&session, "say hello", json!({})).unwrap_or_else(|e| panic!("run_agent refused: {e}"));
    assert_eq!(done["status"], "completed", "{done}");
    let pane = pane_of(&done);

    // The file is in the fake Linux home, under the encoded Linux folder, and nowhere in the project.
    let project_dir = session.fake_wsl_home().join(".claude").join("projects").join(&folder);
    let files: Vec<_> = std::fs::read_dir(&project_dir)
        .unwrap_or_else(|e| panic!("no session folder {}: {e}", project_dir.display()))
        .filter_map(Result::ok)
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(folder.starts_with("-mnt-"), "the folder is named from the Linux path: {folder}");
    assert!(!session.project().join(".claude").exists(), "nothing was written into the project");

    let chat = session.open_file_chat(&pane, "claude");
    let seen = session.until(40, || {
        let text = serde_json::to_string(&chat.items()).unwrap_or_default();
        (text.contains("Hello from Linux.") && text.contains("say hello")).then_some(text)
    });
    assert!(seen.is_some(), "the chat never showed the conversation: {:#?}", chat.items());
}

// ---- 4. a permission card ------------------------------------------------------------------------------------------

/// A permission question in the Linux pane is a card on the desktop like any other; the person's answer reaches the
/// agent as its key press (logged by the fake), and the audit line says `desktop`.
#[test]
fn a_permission_screen_raises_the_card_and_the_desktop_answer_reaches_the_agent() {
    let session = World::get().session("wsl-card");
    session.script(
        r#"
agent = "claude"
boot_ms = 200
[[turn]]
steps = [
  { do = "think", ms = 400 },
  { do = "approve", kind = "bash", command = "npm test", on_allow = [{ do = "say", text = "The tests pass." }], on_deny = [{ do = "say", text = "Stopped." }] },
]
"#,
    );
    session.set_linux("fake_claude", DISTRO);
    let parked = run(&session, "run the tests", json!({})).unwrap_or_else(|e| panic!("run_agent refused: {e}"));
    assert_eq!(parked["status"], "needs_input", "{parked}\n{:#?}", session.events());
    let pane = pane_of(&parked);
    assert_eq!(parked["prompt"]["kind"], "command_approval", "{parked}");
    assert_eq!(parked["prompt"]["command"], "npm test", "{parked}");
    assert!(session.events().iter().all(|e| e["ev"] != "approval_answer"), "nobody has answered yet");

    let shown = session.card(&pane).expect("the pane shows the card");
    let prompt_id = shown["id"].as_str().or_else(|| parked["prompt"]["id"].as_str()).expect("a prompt id").to_string();
    session.answer_card(&pane, &prompt_id, "allow_once").unwrap_or_else(|e| panic!("answer refused: {e}"));

    let handle = parked["handle"].as_str().expect("handle").to_string();
    let done = session.wait(&handle, &["completed", "failed", "cancelled"], 90_000).expect("wait");
    assert_eq!(done["status"], "completed", "{done}\n{}", session.screen(&pane));
    let answers: Vec<Value> = session.events().into_iter().filter(|e| e["ev"] == "approval_answer").collect();
    assert_eq!(answers.len(), 1, "{:#?}", session.events());
    assert_eq!((answers[0]["answer"].as_str(), answers[0]["option"].as_u64()), (Some("allow"), Some(1)), "{answers:?}");
    let lines = session.approvals_for(&pane);
    assert_eq!(lines.len(), 1, "{lines:#?}");
    assert_eq!(lines[0]["who"], "desktop", "{lines:#?}");
}

// ---- 5. undo ------------------------------------------------------------------------------------------------------

/// Undo works on the Windows folder, so a Linux pane gets checkpoints and restores exactly as a Windows one: the
/// file the agent wrote through `/mnt/...` is in the checkpoint and comes back after it was deleted.
#[test]
fn a_checkpoint_is_taken_at_the_turn_and_a_restore_brings_the_file_back() {
    let session = World::get().session("wsl-undo");
    session.script(
        r#"
agent = "claude"
boot_ms = 200
[[turn]]
steps = [{ do = "think", ms = 500 }, { do = "write_file", path = "hello.txt", content = "hi\n" }, { do = "say", text = "Created hello.txt." }]
"#,
    );
    session.set_linux("fake_claude", DISTRO);
    let done = run(&session, "create hello.txt", json!({})).unwrap_or_else(|e| panic!("run_agent refused: {e}"));
    assert_eq!(done["status"], "completed", "{done}");
    let pane = pane_of(&done);
    let file = session.project().join("hello.txt");
    assert_eq!(std::fs::read_to_string(&file).expect("the agent wrote through /mnt"), "hi\n");

    // The checkpoint that holds the agent's file. Which kind it is depends on when the pane's status moved (a busy machine
    // can make the pane read as running again after the write, and then "before" already has the file); what counts is
    // that Undo has a point with the file in it.
    let point = session
        .until(300, || {
            let view = session.checkpoints(&pane);
            let items = view["items"].as_array().cloned().unwrap_or_default();
            items.into_iter().find(|c| {
                c["id"].as_str().is_some_and(|id| session.checkpoint_paths(&pane, id).iter().any(|p| p == "hello.txt"))
            })
        })
        .unwrap_or_else(|| panic!("no checkpoint holds the file: {}", session.checkpoints(&pane)));
    assert_eq!(session.checkpoints(&pane)["state"], "ok");

    std::fs::remove_file(&file).expect("delete the file");
    let id = point["id"].as_str().expect("checkpoint id").to_string();
    let report = session.restore_checkpoint(&pane, &id).unwrap_or_else(|e| panic!("restore failed: {e}"));
    assert_eq!(report["failed"], json!([]), "{report}");
    assert_eq!(std::fs::read_to_string(&file).expect("the file is back"), "hi\n");
}

// ---- 6. closing the pane ---------------------------------------------------------------------------------------------

/// Closing the pane ends the `wsl.exe` tree and then runs the stop call for this pane id inside the distro.
#[test]
fn closing_the_pane_runs_the_stop_for_its_id() {
    let session = World::get().session("wsl-stop");
    session.script(SAY);
    session.set_linux("fake_claude", DISTRO);
    let done = run(&session, "say hello", json!({})).unwrap_or_else(|e| panic!("run_agent refused: {e}"));
    assert_eq!(done["status"], "completed", "{done}");
    let pane = pane_of(&done);
    // (A stop for an earlier test's pane may still arrive late in this log; only this pane's counts.)
    assert!(
        session.fake_wsl_log().iter().all(|entry| !(entry["ev"] == "stop" && entry["pane"] == pane.as_str())),
        "nothing is stopped while the pane lives: {:#?}",
        session.fake_wsl_log()
    );

    session.close_pane(&pane);
    let stop = session.until(60, || session.fake_wsl_log().into_iter().find(|entry| entry["ev"] == "stop" && entry["pane"] == pane.as_str()));
    let stop = stop.unwrap_or_else(|| panic!("no stop for {pane}: {:#?}", session.fake_wsl_log()));
    assert_eq!(stop["distro"], DISTRO, "{stop}");
    assert!(!session.pane_ids().contains(&pane), "the pane is gone");
}

// ---- 7. refusals -----------------------------------------------------------------------------------------------------

/// What Linux cannot do yet is refused before any pane or run exists: a saved login, a separate copy, an explicit
/// one-task run, a guarded launch. Nothing reaches the fake `wsl.exe` launcher.
#[test]
fn what_linux_cannot_do_yet_is_refused_before_anything_starts() {
    let session = World::get().session("wsl-refuse");
    session.script(SAY);
    session.set_linux("fake_claude", DISTRO);

    let account = run(&session, "go", json!({ "account": "work" })).expect_err("a saved login in Linux");
    assert_eq!(code_of(&account), "WSL_ACCOUNT_NOT_YET", "{account}");
    let copy = run(&session, "go", json!({ "isolation": "worktree" })).expect_err("a separate copy in Linux");
    assert_eq!(code_of(&copy), "WSL_COPY_NOT_YET", "{copy}");
    let headless = run(&session, "go", json!({ "headless": true })).expect_err("an explicit one-task run in Linux");
    assert_eq!(code_of(&headless), "WSL_HEADLESS_NOT_YET", "{headless}");
    // The refusals are sentences a person can read (simple English, no code names).
    assert!(account["message"].as_str().unwrap_or_default().contains("Saved logins don't work in Linux yet"), "{account}");
    assert!(copy["message"].as_str().unwrap_or_default().contains("Separate copies don't work in Linux yet"), "{copy}");
    assert!(headless["message"].as_str().unwrap_or_default().contains("One-task runs don't work in Linux yet"), "{headless}");

    let guarded = session.spawn_pane_guarded("fake_claude", "no_push").expect_err("a guarded launch in Linux");
    assert!(guarded.contains("runs agents on Windows only"), "{guarded}");

    assert!(session.pane_ids().is_empty(), "no pane was opened");
    assert!(launches(&session).is_empty(), "nothing was launched: {:#?}", session.fake_wsl_log());
}

// ---- 8. WSL is not installed -------------------------------------------------------------------------------------------

/// With no `wsl.exe` to run (the test points `PM_FAKE_WSL_EXE` at a file that is not there) a run for an agent set to
/// Linux is refused with `WSL_NOT_INSTALLED`, and the same for a plain pane.
#[test]
fn a_missing_wsl_is_refused_with_wsl_not_installed() {
    let session = World::get().session("wsl-missing");
    session.script(SAY);
    session.set_linux("fake_claude", DISTRO);
    std::env::set_var("PM_FAKE_WSL_EXE", session.aux().join("no-such-wsl.exe"));

    let refused = run(&session, "go", json!({})).expect_err("no wsl.exe");
    assert_eq!(code_of(&refused), "WSL_NOT_INSTALLED", "{refused}");
    assert!(refused["message"].as_str().unwrap_or_default().contains("wsl --install"), "{refused}");
    let pane = session.spawn_pane("fake_claude");
    assert!(pane.is_err(), "a pane is not opened on Windows instead: {pane:?}");
    assert!(session.pane_ids().is_empty());
    let started = |entry: &Value| matches!(entry["ev"].as_str(), Some("list" | "probe" | "launch"));
    assert!(!session.fake_wsl_log().iter().any(started), "the fake was never asked anything: {:#?}", session.fake_wsl_log());
}

// ---- 9. back to Windows -------------------------------------------------------------------------------------------------

/// Setting the agent back to Windows gives today's pane again: no place, no call to the fake `wsl.exe`.
#[test]
fn setting_the_agent_back_to_windows_gives_a_windows_pane_again() {
    let session = World::get().session("wsl-back");
    session.script(SAY);
    session.set_linux("fake_claude", DISTRO);
    let linux = session.spawn_pane("fake_claude").expect("a Linux pane");
    session.until(60, || launch_of(&session, &linux)).expect("it launched through the fake wsl.exe");
    session.close_pane(&linux);
    let calls = session.fake_wsl_log().len();

    session.set_windows("fake_claude");
    let windows = session.spawn_pane("fake_claude").expect("a Windows pane");
    let ready = session.until(60, || {
        let read = session.read_screen("claude", &session.input_screen(&windows));
        (read["ready"] == "Empty").then_some(read)
    });
    assert!(ready.is_some(), "the Windows pane never showed its composer:\n{}", session.screen(&windows));
    let info = session.pane_info(&windows).expect("the pane is listed");
    assert!(info.get("place").is_none(), "a Windows pane has no place: {info}");
    assert!(
        session.fake_wsl_log().iter().skip(calls).all(|entry| entry["ev"] != "launch" && entry["ev"] != "probe" && entry["ev"] != "list"),
        "the fake wsl.exe was not used: {:#?}",
        session.fake_wsl_log()
    );
}
