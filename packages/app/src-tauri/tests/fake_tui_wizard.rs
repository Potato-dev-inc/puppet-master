//! The Agent Wizard's visible path end to end, with `pm-fake-tui` playing an UNKNOWN agent ("nimbus": no
//! profile in the app knows its screens) on a real ConPTY. Zero tokens: capture -> a stand-in drafter that
//! replays a recorded draft -> verify -> visible test -> finish, and the written definition must pass
//! `check_report` including its `tui_*` fixtures.
//!
//! The test plays the PERSON: it answers the folder question and the permission question by typing into the
//! pane as a user. The wizard itself never presses a key into either (its capture has no way to), and this
//! test checks that too, from the fake agent's own event log.
//!
//! Included from `fake_tui_e2e.rs` (one test binary, one World).

use puppet_master_app_lib::fake_tui::{Session, World};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// The recorded draft a stand-in drafter answers with. `@@COMMAND@@` is the program the person typed.
const DRAFT: &str = include_str!("../src/tui/declarative/wizard/tests/drafts/visible_nimbus.toml");

const SCRIPT: &str = r#"
agent = "nimbus"
boot_ms = 200
inline = true
transcript = "session.jsonl"
summary_row = "▸ Thought for {secs}s, 298 tokens"

[[startup]]
gate = "trust"
after_ms = 150

[[turn]]
on_prompt = "hello.txt"
steps = [
  { do = "think", ms = 1200 },
  { do = "approve", kind = "create", path = "hello.txt", on_allow = [
      { do = "write_file", path = "hello.txt", content = "{last_word}\n" },
      { do = "say", text = "Created hello.txt." },
    ], on_deny = [{ do = "say", text = "Not created." }] },
]

[[turn]]
on_prompt = "shell command"
steps = [
  { do = "think", ms = 1000 },
  { do = "approve", kind = "bash", command = "echo ok", on_allow = [{ do = "say", text = "ok" }], on_deny = [{ do = "say", text = "Not run." }] },
]

[[turn]]
on_prompt = "list the files"
steps = [
  { do = "screen", text = "~ Thinking... (1s)" },
  { do = "tool", label = "ls", detail = "README.md" },
  { do = "think", ms = 3200 },
  { do = "say", text = "folder listed, code {last_word}." },
]

[[turn]]
on_prompt = "whole numbers"
steps = [
  { do = "think", ms = 800 },
  { do = "lines", count = 70, template = "{n}" },
  { do = "say", text = "{last_word}" },
]

[[turn]]
on_prompt = "exactly the word"
steps = [
  { do = "think", ms = 2200 },
  { do = "say", text = "{last_word}" },
]
"#;

fn err(e: &Value) -> String {
    e["message"].as_str().map(str::to_string).unwrap_or_else(|| e.to_string())
}

fn status(rig: &puppet_master_app_lib::fake_tui::WizardRig, id: &str) -> Value {
    rig.call("capture", json!({"session": id, "op": "status"})).unwrap_or_else(|e| panic!("capture status: {}", err(&e)))
}

/// The person at the pane: answers what the pane asks, once each, and asks for `/exit` at the exit step.
/// Returns what it answered (so the test can say what the WIZARD did not do).
fn play_the_person(session: &Session, rig: &puppet_master_app_lib::fake_tui::WizardRig, id: &str, seconds: u64) -> Vec<String> {
    let mut answered: HashSet<String> = HashSet::new();
    let mut log = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        let s = status(rig, id);
        let pane = s["pane"].as_str().unwrap_or_default().to_string();
        if let Some(kind) = s["needs_you"]["kind"].as_str() {
            // the permission step asks twice: the file write (phase 0), then the command run (phase 1)
            let once = format!("{}-{kind}-{}", s["step"].as_str().unwrap_or(""), s["permission_phase"]);
            if !answered.contains(&once) {
                let (keys, what) = match kind {
                    "trust" => ("1", "trusted the folder"),
                    "permission" if s["permission_phase"] == 1 => ("y", "allowed the command"),
                    "permission" => ("y", "allowed the file"),
                    other => panic!("an unexpected question ({other}): {s}"),
                };
                // The person presses the key in the pane: the same function a click or a keystroke in the window uses.
                session.type_as_user(&pane, keys, false).expect("the person types");
                answered.insert(once);
                log.push(what.to_string());
            }
        }
        if s["step"] == "exit" && !answered.contains("exit") {
            answered.insert("exit".into());
            rig.call("capture", json!({"session": id, "op": "act", "action": "try_exit"})).unwrap_or_else(|e| panic!("try_exit: {}", err(&e)));
            log.push("asked the wizard to try /exit".into());
        }
        if s["settled"] == true {
            return log;
        }
        assert!(Instant::now() < deadline, "the capture did not settle in {seconds} s: {s}");
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn events_named(session: &Session, name: &str) -> Vec<Value> {
    session.events().into_iter().filter(|e| e["ev"] == name).collect()
}

#[test]
fn the_wizard_learns_an_unknown_visible_agent_from_its_screens_and_writes_a_definition_that_passes() {
    let session = World::get().session("wizard-visible");
    let rig = session.wizard_rig("nimbus", SCRIPT);

    // 1. Resolve and probe: the typed command is found on PATH.
    let started = rig.call("start", json!({"command": "nimbus"})).unwrap_or_else(|e| panic!("start: {}", err(&e)));
    let id = started["session"].as_str().expect("session id").to_string();
    assert_eq!(started["id_suggestion"], "nimbus");
    rig.call("probe", json!({"session": id})).unwrap_or_else(|e| panic!("probe: {}", err(&e)));
    let (one, two, sky) = rig.words(&id);

    // 2. The person sees the exact command and confirms it; the pane starts at the fixed grid in an isolated folder.
    let plan = rig.call("capture", json!({"session": id, "op": "plan"})).unwrap_or_else(|e| panic!("plan: {}", err(&e)));
    assert_eq!(plan["plan"]["grid"], json!({"cols": 120, "rows": 40}));
    let fingerprint = plan["plan"]["fingerprint"].as_str().unwrap().to_string();
    let first = rig
        .call("capture", json!({"session": id, "op": "start", "confirm": true, "plan": fingerprint}))
        .unwrap_or_else(|e| panic!("capture start: {}", err(&e)));
    let pane = first["pane"].as_str().expect("a pane").to_string();
    assert_eq!(first["grid"], json!({"cols": 120, "rows": 40}));
    let grid = session.pane_ids();
    assert!(grid.contains(&pane));

    // 3. The guided capture, with the test as the person.
    let did = play_the_person(&session, &rig, &id, 120);
    assert_eq!(did, ["trusted the folder", "allowed the file", "allowed the command", "asked the wizard to try /exit"], "{did:?}");
    // The fake's own log is the proof of what the WIZARD typed: exactly the four tasks and `/exit`, as prompts.
    let prompts: Vec<String> = events_named(&session, "prompt").iter().filter_map(|e| e["text"].as_str().map(str::to_string)).collect();
    assert_eq!(prompts.len(), 5, "{prompts:?}");
    assert_eq!(prompts[0], format!("Reply with exactly the word: {one}"));
    assert_eq!(prompts[1], format!("Stay in this folder. List the files here, then reply with exactly: folder listed, code {sky}."));
    assert!(prompts[2].contains("Create a file named hello.txt") && prompts[2].contains(&sky), "{}", prompts[2]);
    assert!(prompts[3].contains("Run this shell command") && prompts[3].ends_with(&format!("echo {sky}")), "{}", prompts[3]);
    assert_eq!(prompts[4], "/exit");
    // It answered no question: the only answers the fake saw were the person's (one gate answer, two approvals).
    assert_eq!(events_named(&session, "gate_answer").len(), 1);
    assert_eq!(events_named(&session, "approval_answer").len(), 2);
    assert_eq!(events_named(&session, "approval_answer")[0]["answer"], "allow");
    assert_eq!(std::fs::read_to_string(session.project().join("hello.txt")).ok(), None, "the agent ran in an isolated folder, not the project");

    let finished = rig.call("capture", json!({"session": id, "op": "finish"})).unwrap_or_else(|e| panic!("capture finish: {}", err(&e)));
    let files = finished["captures"]["files"].as_array().unwrap();
    let names: Vec<&str> = files.iter().filter_map(|f| f["name"].as_str()).collect();
    assert_eq!(names, ["tui_trust", "tui_idle", "tui_working", "tui_reply", "tui_work_working", "tui_work_reply", "tui_permission", "tui_permission_command"], "{finished}");
    // the listing task made the agent think and use a tool: the drafter sees those rows, and the row after the turn
    let screen_of = |name: &str| files.iter().find(|f| f["name"] == name).and_then(|f| f["text"].as_str()).unwrap_or_default().to_string();
    let (work_working, work_reply) = (screen_of("tui_work_working"), screen_of("tui_work_reply"));
    assert!(work_working.contains("Thinking...") && work_working.contains("tool: ls"), "{work_working}");
    assert!(work_reply.contains("reply: folder listed, code <word-3>.") && work_reply.contains("Thought for") && work_reply.contains("298 tokens"), "{work_reply}");
    let shown = finished["captures"].to_string();
    assert!(!shown.contains(&one) && !shown.contains(&two) && !shown.contains(&sky), "the AI would see a test word: {shown}");
    assert_eq!(finished["captures"]["facts"]["exit_method"], "slash_exit");
    // The pane was made narrower after the reply and put back; this agent paints the cells it needs and does not
    // reprint, so the wizard measured `false` and says so (with the rows it counted as evidence).
    assert_eq!(finished["captures"]["facts"]["reprints_on_resize"], false, "{}", finished["captures"]["facts"]);
    assert_eq!(finished["captures"]["facts"]["resize_probe"]["from"], json!({"cols": 120, "rows": 40}));
    assert_eq!(finished["captures"]["facts"]["resize_probe"]["to"], json!({"cols": 100, "rows": 40}));
    assert!(!rig.is_registered(&format!("wzc_{}", id.trim_start_matches("wz_"))), "the capture's transient profile is gone");

    // 4. The stand-in drafter answers with the recorded draft; verify checks it against the captures.
    let draft = DRAFT.replace("@@COMMAND@@", "nimbus");
    let expect = json!({"tui_idle": "chrome_lines = [\"NIMBUS 0.4.2\"]\n"});
    let verified = rig.call("verify", json!({"session": id, "toml": draft, "expect": expect})).unwrap_or_else(|e| panic!("verify: {}", err(&e)));
    assert_eq!(verified["ok"], true, "verify said:\n{}", verified["text"].as_str().unwrap_or_default());
    assert_eq!(verified["checked"]["fixtures"], 8);
    // the wizard wrote the measured flag into the draft it checked and will write
    let stored_flag = rig.draft_text(&id).contains("reprints_on_resize = false");
    assert!(stored_flag, "the measured flag is in the draft");

    // 5. The person confirms the visible test; it starts a NEW pane from the drafted profile. The folder question comes
    //    again (a new isolated folder): the person answers it, the wizard does not.
    let prompts_before = events_named(&session, "prompt").len();
    let plan = verified["planned_visible"]["fingerprint"].as_str().expect("a visible plan").to_string();
    let test = std::thread::scope(|scope| {
        let handle = scope.spawn(|| rig.call("visibletest", json!({"session": id, "confirm": true, "plan": plan})));
        let deadline = Instant::now() + Duration::from_secs(180);
        // The command question is checked in a second pane, which asks the folder question again.
        let mut trusted: HashSet<String> = HashSet::new();
        while !handle.is_finished() {
            assert!(Instant::now() < deadline, "the visible test did not finish");
            let status = rig
                .call("capture", json!({"session": id, "op": "test_status"}))
                .unwrap_or_else(|e| panic!("test_status: {}", err(&e)));
            if status["needs_you"]["kind"] == "trust" {
                if let Some(pane) = session.pane_ids().into_iter().find(|p| !grid.contains(p) && !trusted.contains(p)) {
                    session.type_as_user(&pane, "1", false).expect("the person types");
                    trusted.insert(pane);
                }
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        handle.join().unwrap()
    })
    .unwrap_or_else(|e| panic!("visibletest: {}", err(&e)));
    assert_eq!(test["ok"], true, "the visible test said:\n{}\n{}", test["text"].as_str().unwrap_or_default(), test["checks"]);
    assert_eq!(test["state"], "tested");
    // starts, startup, ready, delivery, busy, done_reply, resize, work, long_reply, permission, permission_command, exit
    // (the model check is skipped: the capture names no model)
    assert_eq!((test["passed"].as_u64(), test["total"].as_u64()), (Some(12), Some(12)), "{}", test["checks"]);
    let status_of = |name: &str| test["checks"].as_array().unwrap().iter().find(|c| c["name"] == name).map(|c| c["status"].clone());
    assert_eq!(status_of("resize"), Some(json!("passed")), "{}", test["checks"]);
    assert_eq!(status_of("permission_command"), Some(json!("passed")), "{}", test["checks"]);
    assert_eq!(status_of("model"), Some(json!("skipped")));
    // the thinking-and-tool task is clean, and a reply of 70 rows in a pane of 40 was read whole from the scrollback
    assert_eq!(status_of("work"), Some(json!("passed")), "{}", test["checks"]);
    assert_eq!(status_of("long_reply"), Some(json!("passed")), "{}", test["checks"]);
    let long_note = test["checks"].as_array().unwrap().iter().find(|c| c["name"] == "long_reply").unwrap()["note"].as_str().unwrap().to_string();
    assert!(long_note.contains("read whole"), "{long_note}");
    // The fake kept the conversation in `session.jsonl` in its folder, as a real CLI would: the test turn is in it, and the
    // wizard found it, inferred the fields and checked them with the real reader.
    assert_eq!(test["transcript"], json!({"found": true, "dir": "{cwd}", "tool_lines": false}), "{}", test["transcript"]);
    let prompts_after: Vec<String> = events_named(&session, "prompt").iter().skip(prompts_before).filter_map(|e| e["text"].as_str().map(str::to_string)).collect();
    assert_eq!(prompts_after.len(), 5, "{prompts_after:?}");
    assert_eq!(prompts_after[0], format!("Reply with exactly the word: {two}"));
    assert!(prompts_after[1].contains("List the files here"));
    assert!(prompts_after[2].starts_with("Write the whole numbers from 1 to 70"));
    assert!(prompts_after[3].contains("hello.txt"));
    assert!(prompts_after[4].contains("Run this shell command"));
    assert_eq!(events_named(&session, "approval_answer").len(), 2, "the permission checks answered nothing: only the person's earlier answers exist");
    assert!(session.pane_ids().iter().all(|p| !p.is_empty()));

    // 6. Finish writes an unapproved definition with its screen fixtures, and the fixtures pass.
    let done = rig
        .call("finish", json!({"session": id, "provenance": {"drafter": "Stand-in drafter", "drafter_kind": "other", "prompt_version": "5", "rounds": 1}}))
        .unwrap_or_else(|e| panic!("finish: {}", err(&e)));
    assert_eq!(done["id"], "nimbus");
    assert_eq!(done["state"], "needs_approval");
    assert_eq!(done["fixtures"], json!({"passed": 8, "total": 8}));
    let file = rig.agents_dir().join("nimbus.toml");
    // The definition carries the table the wizard found, with the comment that tells the person to check it. The wizard
    // does not approve it.
    let written = std::fs::read_to_string(&file).unwrap();
    assert!(written.contains("# found by the wizard: check before approving
[session_chat]"), "{written}");
    assert!(written.contains("dir = \"{cwd}\"") && written.contains("role = \"/role\"") && written.contains("text = \"/content\""), "{written}");
    assert!(!written.contains(&two), "no word of the test is in the table");
    let provenance: Value = serde_json::from_str(&std::fs::read_to_string(rig.agents_dir().join("nimbus.wizard.json")).unwrap()).unwrap();
    assert_eq!(provenance["transcript"]["added"], true, "{provenance}");
    let report = rig.check_definition(&file);
    assert_eq!(report["status"], "ran", "{report}");
    let results = report["results"].as_array().unwrap();
    assert_eq!(results.len(), 8, "{report}");
    assert!(results.iter().all(|r| r["passed"] == true), "{report}");
    let reply = std::fs::read_to_string(rig.agents_dir().join("nimbus").join("tui_reply.txt")).unwrap();
    assert!(reply.contains(&one) && reply.starts_with("# capture: nimbus"), "{reply}");
    assert_eq!(rig.session_count(), 0);
    // For refreshing the worked example in the prompt: PM_WIZARD_E2E_KEEP=<folder> keeps what the wizard wrote.
    if let Some(keep) = std::env::var_os("PM_WIZARD_E2E_KEEP") {
        let keep = std::path::PathBuf::from(keep);
        std::fs::create_dir_all(keep.join("nimbus")).unwrap();
        std::fs::copy(&file, keep.join("nimbus.toml")).unwrap();
        for entry in std::fs::read_dir(rig.agents_dir().join("nimbus")).unwrap().flatten() {
            std::fs::copy(entry.path(), keep.join("nimbus").join(entry.file_name())).unwrap();
        }
    }
    // nothing is left running or registered by the wizard
    session.until(10, || session.pane_ids().is_empty().then_some(())).unwrap_or_else(|| panic!("panes left: {:?}", session.pane_ids()));
}

#[test]
fn a_capture_that_is_cancelled_closes_its_pane() {
    let session = World::get().session("wizard-visible-cancel");
    let rig = session.wizard_rig("nimbus", SCRIPT);
    let started = rig.call("start", json!({"command": "nimbus"})).unwrap();
    let id = started["session"].as_str().unwrap().to_string();
    rig.call("probe", json!({"session": id})).unwrap();
    let plan = rig.call("capture", json!({"session": id, "op": "plan"})).unwrap();
    let fingerprint = plan["plan"]["fingerprint"].as_str().unwrap().to_string();
    rig.call("capture", json!({"session": id, "op": "start", "confirm": true, "plan": fingerprint})).unwrap();
    session.until(20, || (!session.pane_ids().is_empty()).then_some(())).expect("the pane exists");
    // the folder question is on screen, nobody answers it, the person gives up
    session.until(20, || (status(&rig, &id)["needs_you"]["kind"] == "trust").then_some(())).expect("the question is shown");
    assert!(events_named(&session, "gate_answer").is_empty());
    rig.call("cancel", json!({"session": id})).unwrap();
    session.until(20, || session.pane_ids().is_empty().then_some(())).unwrap_or_else(|| panic!("the pane is still open: {:?}", session.pane_ids()));
    assert!(events_named(&session, "gate_answer").is_empty(), "cancel answered nothing");
    assert_eq!(rig.session_count(), 0);
}

// ---- the benchmark helpers (SPEC 11.13) ---------------------------------------------------------------

use puppet_master_app_lib::fake_tui::{Answers, WizardRig};

fn bench_expects(words_hint: &str) -> std::collections::BTreeMap<String, String> {
    let _ = words_hint;
    std::collections::BTreeMap::from([("tui_idle".to_string(), "chrome_lines = [\"NIMBUS 0.4.2\"]\n".to_string())])
}

#[test]
fn the_benchmark_helpers_capture_and_run_rounds_on_the_fake_agent() {
    let session = World::get().session("wizard-bench");
    let rig = session.wizard_rig("nimbus", SCRIPT);
    let out = session.aux().join("bench");

    // Without the person's answers the capture stops and says exactly which one is missing.
    let err = rig.bench_capture(&session, "nimbus", &out, &Answers::parse(""), &[], 60).unwrap_err();
    assert!(err.contains("the capture needs you") && err.contains("PM_WIZARD_BENCH_ANSWERS=trust=<keys>"), "{err}");
    session.until(20, || session.pane_ids().is_empty().then_some(())).expect("the stopped capture closed its pane");

    let answers = Answers::parse("trust=1,permission=y");
    let dump = rig.bench_capture(&session, "nimbus", &out, &answers, &[], 120).expect("the capture");
    assert!(out.join("captures.json").is_file());
    let one = dump["words"]["one"].as_str().unwrap().to_string();
    assert!(dump["raw"].to_string().contains(&one), "the dump keeps the real word");
    assert!(!dump["masked"].to_string().contains(&one), "the masked set (what the drafter sees) never has it");
    assert_eq!(dump["masked"]["files"].as_array().unwrap().len(), 8);

    // A round with the recorded draft passes and writes what the next prompt needs.
    let draft = DRAFT.replace("@@COMMAND@@", "nimbus");
    let round1 = out.join("round-1");
    std::fs::create_dir_all(&round1).unwrap();
    let result = rig.bench_round(&session, "nimbus", &round1, &draft, &bench_expects(""), &dump, &answers, false).expect("the round");
    assert_eq!(result, "PASS");
    assert_eq!(std::fs::read_to_string(round1.join("result.txt")).unwrap(), "PASS\n");
    assert!(std::fs::read_to_string(round1.join("verify.txt")).unwrap().starts_with("OK"));
    let feedback: Value = serde_json::from_str(&std::fs::read_to_string(round1.join("feedback.json")).unwrap()).unwrap();
    assert_eq!(feedback["text"], "");
    assert_eq!(feedback["previous_toml"], draft);
    assert_eq!(feedback["previous_expects"]["tui_idle"], "chrome_lines = [\"NIMBUS 0.4.2\"]\n");
    let visible: Value = serde_json::from_str(&std::fs::read_to_string(round1.join("visible.json")).unwrap()).unwrap();
    assert_eq!((visible["passed"].as_u64(), visible["total"].as_u64()), (Some(12), Some(12)));

    // A draft whose ready pattern never matches fails the round with the checker's text and the screen.
    let wrong = draft.replace("type a task'\nnot_regex", "type a taskX'\nnot_regex");
    assert_ne!(wrong, draft);
    let round2 = out.join("round-2");
    std::fs::create_dir_all(&round2).unwrap();
    let result = rig.bench_round(&session, "nimbus", &round2, &wrong, &bench_expects(""), &dump, &answers, false).expect("the round");
    assert_eq!(result, "FAIL visibletest");
    let feedback: Value = serde_json::from_str(&std::fs::read_to_string(round2.join("feedback.json")).unwrap()).unwrap();
    let text = feedback["text"].as_str().unwrap();
    assert!(text.contains("VISIBLETEST_READY_FAILED") && text.contains("NIMBUS"), "{text}");
    assert!(!text.contains(&one), "the real word never reaches the next prompt");

    // A draft that is not valid never reaches a pane.
    let round3 = out.join("round-3");
    std::fs::create_dir_all(&round3).unwrap();
    let before = events_named(&session, "prompt").len();
    let result = rig.bench_round(&session, "nimbus", &round3, "this is [ not toml", &bench_expects(""), &dump, &answers, false).expect("the round");
    assert_eq!(result, "FAIL verify");
    assert_eq!(events_named(&session, "prompt").len(), before, "nothing was typed for a draft that does not validate");
    let _: Option<&WizardRig> = None;
}

/// An agent that writes hello.txt without asking and leaves a "Working..." line on its screen for good:
/// the permission step must end by itself (the file is the evidence), not wait for a question.
const SILENT_SCRIPT: &str = r#"
agent = "nimbus"
boot_ms = 200

[[startup]]
gate = "trust"
after_ms = 150

[[turn]]
on_prompt = "hello.txt"
steps = [
  { do = "think", ms = 800 },
  { do = "write_file", path = "hello.txt", content = "{last_word}\n" },
  { do = "screen", text = "status: Working..." },
]

[[turn]]
on_prompt = "exactly the word"
steps = [
  { do = "think", ms = 2200 },
  { do = "say", text = "{last_word}" },
]
"#;

/// An agent that shows a picture nothing in the wizard knows (no [y/n], no numbered yes/no) and holds it
/// still until a key arrives.
const ODD_SCRIPT: &str = r#"
agent = "nimbus"
boot_ms = 200

[[startup]]
gate = "trust"
after_ms = 150

[[turn]]
on_prompt = "hello.txt"
steps = [
  { do = "think", ms = 800 },
  { do = "hold", text = "Where should hello.txt go?\n\n   project folder      scratch folder      cancel\n\nuse the arrow keys" },
]

[[turn]]
on_prompt = "exactly the word"
steps = [
  { do = "think", ms = 2200 },
  { do = "say", text = "{last_word}" },
]
"#;

#[test]
fn an_agent_that_writes_the_file_without_asking_ends_the_permission_step_by_itself() {
    let session = World::get().session("wizard-silent");
    let rig = session.wizard_rig("nimbus", SILENT_SCRIPT);
    let out = session.aux().join("silent");
    // no permission answer is given: if the wizard waited for a question, this would stop with an error
    let dump = rig.bench_capture(&session, "nimbus", &out, &Answers::parse("trust=1"), &[], 120).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(dump["raw"]["writes_without_asking"], true, "{}", dump["raw"]);
    assert_eq!(dump["masked"]["facts"]["writes_without_asking"], true);
    assert!(dump["raw"]["notes"].to_string().contains("did not ask before writing a file"), "{}", dump["raw"]["notes"]);
    let names: Vec<String> = dump["masked"]["files"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap().to_string()).collect();
    assert!(!names.contains(&"tui_permission".to_string()), "{names:?}");
    assert_eq!(events_named(&session, "approval_shown").len(), 0, "the agent never asked");
    assert!(!out.join("capture-debug.json").exists(), "a capture that finished leaves no evidence file");
}

#[test]
fn a_picture_nothing_recognises_stops_with_evidence_and_the_person_can_answer_it() {
    let session = World::get().session("wizard-odd");
    let rig = session.wizard_rig("nimbus", ODD_SCRIPT);
    let out = session.aux().join("odd");
    // 1. nobody answers: the bench stops, says which answer it needs, and keeps what it saw
    let err = rig.bench_capture(&session, "nimbus", &out, &Answers::parse("trust=1"), &[], 120).unwrap_err();
    assert!(err.contains("PM_WIZARD_BENCH_ANSWERS=question=<keys>"), "{err}");
    let debug: Value = serde_json::from_str(&std::fs::read_to_string(out.join("capture-debug.json")).expect("capture-debug.json")).unwrap();
    let frames = debug["debug"]["frames"].as_array().expect("frames");
    assert!(frames.len() > 3 && frames.len() <= 40, "{}", frames.len());
    let last = frames.last().unwrap();
    assert_eq!(last["step"], "permission");
    assert!(last["raw"].as_str().unwrap().contains("Where should hello.txt go?"));
    assert!(last["quiet_ms"].as_u64().unwrap() >= 3000, "{last}");
    assert!(frames.iter().all(|f| f.get("classifier").is_some() && f.get("reading").is_some()));
    let partial: Value = serde_json::from_str(&std::fs::read_to_string(out.join("captures.json")).expect("a partial captures.json")).unwrap();
    assert_eq!(partial["partial"], true);
    let names: Vec<&str> = partial["raw"]["frames"].as_array().unwrap().iter().filter_map(|f| f["name"].as_str()).collect();
    assert!(names.contains(&"tui_idle") && names.contains(&"tui_reply"), "{names:?}");
    session.until(20, || session.pane_ids().is_empty().then_some(())).expect("the stopped capture closed its pane");

    // 2. the person answers (the bench types Enter for them): the question is kept as the permission picture
    let dump = rig.bench_capture(&session, "nimbus", &out, &Answers::parse("trust=1,question=<enter>"), &[], 120).unwrap_or_else(|e| panic!("{e}"));
    let permission = dump["raw"]["frames"].as_array().unwrap().iter().find(|f| f["name"] == "tui_permission").expect("tui_permission");
    assert!(permission["text"].as_str().unwrap().contains("Where should hello.txt go?"));
    assert_eq!(dump["raw"]["writes_without_asking"], false);
}

// ---- the wizard answers the setup questions of its own panes (SPEC 11.16) ---------------------------------------

fn with_trust_style(style: &str) -> String {
    SCRIPT.replace("after_ms = 150", &format!("after_ms = 150\nstyle = \"{style}\""))
}

/// Watches a capture settle without answering anything: the only button it presses is the optional `/exit`.
fn settle_without_the_person(rig: &puppet_master_app_lib::fake_tui::WizardRig, id: &str, seconds: u64) -> Value {
    let mut tried_exit = false;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        let s = status(rig, id);
        assert!(s["needs_you"].is_null(), "the wizard should have answered it, but it asked the person: {}", s["needs_you"]);
        if s["step"] == "exit" && !tried_exit {
            tried_exit = true;
            rig.call("capture", json!({"session": id, "op": "act", "action": "try_exit"})).unwrap_or_else(|e| panic!("try_exit: {}", err(&e)));
        }
        if s["settled"] == true {
            return s;
        }
        assert!(Instant::now() < deadline, "the capture did not settle in {seconds} s: {s}");
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn start_capture(rig: &puppet_master_app_lib::fake_tui::WizardRig, auto: bool) -> String {
    let started = rig.call("start", json!({"command": "nimbus", "auto_answer": auto})).unwrap_or_else(|e| panic!("start: {}", err(&e)));
    let id = started["session"].as_str().expect("session id").to_string();
    rig.call("probe", json!({"session": id})).unwrap_or_else(|e| panic!("probe: {}", err(&e)));
    let plan = rig.call("capture", json!({"session": id, "op": "plan"})).unwrap_or_else(|e| panic!("plan: {}", err(&e)));
    let fingerprint = plan["plan"]["fingerprint"].as_str().unwrap().to_string();
    rig.call("capture", json!({"session": id, "op": "start", "confirm": true, "plan": fingerprint})).unwrap_or_else(|e| panic!("capture start: {}", err(&e)));
    id
}

#[test]
fn the_whole_flow_on_an_arrow_menu_agent_needs_no_person_with_the_switch_on() {
    let session = World::get().session("wizard-auto-arrow");
    let rig = session.wizard_rig("nimbus", &with_trust_style("arrow"));
    let id = start_capture(&rig, true);
    let (one, two, _sky) = rig.words(&id);

    // capture: the trust menu (cursor on the safe row) and the permission question are answered by the wizard
    let settled = settle_without_the_person(&rig, &id, 120);
    let answered: Vec<(String, String, String)> = settled["answered"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| (a["flavour"].as_str().unwrap().into(), a["label"].as_str().unwrap().into(), a["keys"].as_str().unwrap().into()))
        .collect();
    assert_eq!(
        answered,
        [
            ("trust".to_string(), "Yes, I trust this folder".to_string(), "Enter".to_string()),
            ("permission".to_string(), "Yes".to_string(), "y".to_string()),
            ("permission".to_string(), "Yes".to_string(), "y".to_string()),
        ],
        "{settled}"
    );
    assert_eq!(events_named(&session, "gate_answer").len(), 1);
    assert_eq!(events_named(&session, "gate_answer")[0]["answer"], "trust");
    assert_eq!(events_named(&session, "approval_answer").len(), 2, "the file question and the command question");
    assert_eq!(events_named(&session, "approval_answer")[0]["answer"], "allow");
    let finished = rig.call("capture", json!({"session": id, "op": "finish"})).unwrap_or_else(|e| panic!("finish: {}", err(&e)));
    let names: Vec<&str> = finished["captures"]["files"].as_array().unwrap().iter().filter_map(|f| f["name"].as_str()).collect();
    assert_eq!(
        names,
        ["tui_trust", "tui_idle", "tui_working", "tui_reply", "tui_work_working", "tui_work_reply", "tui_permission", "tui_permission_command"],
        "the questions were kept before they were answered"
    );
    assert!(finished["captures"]["files"][0]["text"].as_str().unwrap().contains("Navigate"), "the arrow menu was captured");
    assert_eq!(finished["captures"]["facts"]["answered"].as_array().unwrap().len(), 3);
    assert!(!finished.to_string().contains(&one));

    // draft -> verify (the draft's own prompt must detect the startup question) -> visible test, no person
    let draft = DRAFT.replace("@@COMMAND@@", "nimbus");
    let verified = rig
        .call("verify", json!({"session": id, "toml": draft, "expect": {"tui_idle": "chrome_lines = [\"NIMBUS 0.4.2\"]\n"}}))
        .unwrap_or_else(|e| panic!("verify: {}", err(&e)));
    assert_eq!(verified["ok"], true, "verify said:\n{}", verified["text"].as_str().unwrap_or_default());
    let plan = verified["planned_visible"]["fingerprint"].as_str().unwrap().to_string();
    let prompts_before = events_named(&session, "prompt").len();
    let test = rig.call("visibletest", json!({"session": id, "confirm": true, "plan": plan})).unwrap_or_else(|e| panic!("visibletest: {}", err(&e)));
    assert_eq!(test["ok"], true, "{}\n{}", test["text"].as_str().unwrap_or_default(), test["checks"]);
    assert_eq!((test["passed"].as_u64(), test["total"].as_u64()), (Some(12), Some(12)), "{}", test["checks"]);
    let startup = test["checks"].as_array().unwrap().iter().find(|c| c["name"] == "startup").unwrap();
    assert_eq!(startup["status"], "passed", "the draft's prompt saw the question before the wizard answered: {startup}");
    assert_eq!(test["answered"].as_array().unwrap().len(), 2, "the trust question of each of the two test panes");
    assert_eq!(test["answered"][0]["label"], "Yes, I trust this folder");
    assert_eq!(events_named(&session, "gate_answer").len(), 3, "one trust answer per pane");
    assert_eq!(events_named(&session, "approval_answer").len(), 2, "the permission checks answered nothing");
    let prompts_after: Vec<String> = events_named(&session, "prompt").iter().skip(prompts_before).filter_map(|e| e["text"].as_str().map(str::to_string)).collect();
    assert_eq!(prompts_after.len(), 5, "{prompts_after:?}");
    assert_eq!(prompts_after[0], format!("Reply with exactly the word: {two}"));

    // finish records what was answered for the person
    let done = rig
        .call("finish", json!({"session": id, "provenance": {"drafter": "Stand-in drafter", "drafter_kind": "other", "prompt_version": "6", "rounds": 1}}))
        .unwrap_or_else(|e| panic!("finish: {}", err(&e)));
    assert_eq!(done["id"], "nimbus");
    let prov: Value = serde_json::from_str(&std::fs::read_to_string(done["provenance_file"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(prov["auto_answer"]["enabled"], true);
    assert_eq!(prov["auto_answer"]["answered"].as_array().unwrap().len(), 5, "{prov}");
    session.until(10, || session.pane_ids().is_empty().then_some(())).unwrap_or_else(|| panic!("panes left: {:?}", session.pane_ids()));
}

#[test]
fn a_menu_with_the_safe_option_on_the_second_row_is_walked_to_on_a_real_pane() {
    let session = World::get().session("wizard-auto-second");
    let rig = session.wizard_rig("nimbus", &with_trust_style("arrow_no_first"));
    let id = start_capture(&rig, true);
    let settled = settle_without_the_person(&rig, &id, 120);
    let first = &settled["answered"][0];
    assert_eq!((first["label"].as_str(), first["keys"].as_str()), (Some("Yes, I trust this folder"), Some("Down, Enter")), "{settled}");
    assert_eq!(events_named(&session, "gate_answer")[0]["answer"], "trust", "the fake saw a yes, not the default No");
    rig.call("cancel", json!({"session": id})).unwrap();
}

#[test]
fn a_numbered_menu_is_answered_with_its_number_on_a_real_pane() {
    let session = World::get().session("wizard-auto-numbered");
    let rig = session.wizard_rig("nimbus", SCRIPT);
    let id = start_capture(&rig, true);
    let settled = settle_without_the_person(&rig, &id, 120);
    let first = &settled["answered"][0];
    assert_eq!((first["label"].as_str(), first["keys"].as_str()), (Some("Yes, trust this folder"), Some("1")), "{settled}");
    rig.call("cancel", json!({"session": id})).unwrap();
}

#[test]
fn a_menu_whose_only_yes_is_always_trust_is_left_to_the_person_on_a_real_pane() {
    let session = World::get().session("wizard-auto-always");
    let rig = session.wizard_rig("nimbus", &with_trust_style("always"));
    let id = start_capture(&rig, true);
    let deadline = Instant::now() + Duration::from_secs(60);
    let needs = loop {
        let s = status(&rig, &id);
        if s["needs_you"].is_object() {
            break s["needs_you"].clone();
        }
        assert!(Instant::now() < deadline, "the person was never asked: {s}");
        std::thread::sleep(Duration::from_millis(150));
    };
    assert_eq!(needs["kind"], "trust");
    assert!(needs["message"].as_str().unwrap().contains("did not answer it for you"), "{needs}");
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(events_named(&session, "gate_answer").len(), 0, "the fake was never answered");
    rig.call("cancel", json!({"session": id})).unwrap();
}

#[test]
fn with_the_switch_off_the_same_menu_is_left_to_the_person() {
    let session = World::get().session("wizard-auto-off");
    let rig = session.wizard_rig("nimbus", &with_trust_style("arrow"));
    let id = start_capture(&rig, false);
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let s = status(&rig, &id);
        if s["needs_you"]["kind"] == "trust" {
            assert!(!s["needs_you"]["message"].as_str().unwrap().contains("did not answer it for you"));
            break;
        }
        assert!(Instant::now() < deadline, "{s}");
        std::thread::sleep(Duration::from_millis(150));
    }
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(events_named(&session, "gate_answer").len(), 0);
    rig.call("cancel", json!({"session": id})).unwrap();
}

#[test]
fn a_normal_pane_showing_the_same_menu_is_never_answered() {
    let session = World::get().session("wizard-auto-normal");
    // a normal pane: the app's own, not the wizard's, showing a trust question (the fake claude layout)
    session.script("agent = \"claude\"\nboot_ms = 100\n[[startup]]\ngate = \"trust\"\nafter_ms = 100\n");
    let rig = session.wizard_rig("nimbus", &with_trust_style("arrow"));
    session.script("agent = \"claude\"\nboot_ms = 100\n[[startup]]\ngate = \"trust\"\nafter_ms = 100\n");
    let pane = session.spawn_pane("fake_claude").expect("a normal pane");
    session.until(20, || session.screen(&pane).to_lowercase().contains("trust").then_some(())).expect("the question is on the pane");
    // even asked directly, the wizard host presses nothing in a pane it did not start
    let refused = rig.wizard_host_presses_enter(&pane).unwrap_err();
    assert!(refused.contains("not a pane the wizard started"), "{refused}");
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(events_named(&session, "gate_answer").len(), 0, "nothing answered the person's pane");
    session.close_pane(&pane);
}

fn env_or_skip(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("set {name}"))
}

/// `PM_WIZARD_COMMAND=kilo PM_WIZARD_OUT=<dir> PM_WIZARD_BENCH_ANSWERS=permission=<keys>[,trust=<keys>]`
/// `[PM_WIZARD_CAPTURE_ARGS="--model x"]`: runs the guided capture on the REAL program (it spends about
/// two small messages) and writes `<dir>/captures.json`. Plays the person with the answers it was given.
#[test]
#[ignore = "runs the real program and spends real agent messages; see SPEC 11.13"]
fn wizard_bench_capture() {
    let command = env_or_skip("PM_WIZARD_COMMAND");
    let out = std::path::PathBuf::from(env_or_skip("PM_WIZARD_OUT"));
    let session = World::get().session("wizard-bench-capture");
    let rig = WizardRig::real(&out);
    let args: Vec<String> = std::env::var("PM_WIZARD_CAPTURE_ARGS").unwrap_or_default().split_whitespace().map(str::to_string).collect();
    let dump = rig.bench_capture(&session, &command, &out, &Answers::from_env(), &args, 600).unwrap_or_else(|e| panic!("{e}"));
    println!("{}", serde_json::to_string_pretty(&dump["masked"]).unwrap());
}

/// `PM_WIZARD_COMMAND=kilo PM_WIZARD_ROUND_DIR=<dir>\round-N PM_WIZARD_BENCH_ANSWERS=...`, with `draft.toml`
/// (and optionally `expects.json`) in the round folder and `captures.json` one folder up. Verifies the draft
/// and runs the visible test on the REAL program (about two small messages). `PM_WIZARD_FINISH=1` also
/// writes the definition to `<round>\agents`. A failing draft is a normal result, not a test failure.
#[test]
#[ignore = "runs the real program and spends real agent messages; see SPEC 11.13"]
fn wizard_bench_round() {
    let command = env_or_skip("PM_WIZARD_COMMAND");
    let round = std::path::PathBuf::from(env_or_skip("PM_WIZARD_ROUND_DIR"));
    let draft = std::fs::read_to_string(round.join("draft.toml")).unwrap_or_else(|e| panic!("draft.toml: {e} (run the extract step first)"));
    let expects: std::collections::BTreeMap<String, String> = std::fs::read_to_string(round.join("expects.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let dump_path = round.parent().expect("a round folder has a parent").join("captures.json");
    let dump: Value = serde_json::from_str(&std::fs::read_to_string(&dump_path).unwrap_or_else(|e| panic!("{}: {e} (run wizard_bench_capture first)", dump_path.display())))
        .expect("captures.json is JSON");
    let session = World::get().session("wizard-bench-round");
    let rig = WizardRig::real(&round);
    let result = rig
        .bench_round(&session, &command, &round, &draft, &expects, &dump, &Answers::from_env(), std::env::var("PM_WIZARD_FINISH").is_ok())
        .unwrap_or_else(|e| panic!("{e}"));
    println!("{result}");
    println!("{}", std::fs::read_to_string(round.join("feedback.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v["text"].as_str().map(str::to_string)).unwrap_or_default());
}

// ---- the capture measures a reprint on resize; one question per kind of permission --------------------------------------

/// Plays the person for a whole capture, with `keys` for the file question and the command question (`<enter>` is the
/// Enter key). Returns the session id, the masked captures and what the person did.
fn capture_as_the_person(
    session: &Session,
    rig: &puppet_master_app_lib::fake_tui::WizardRig,
    keys: [&str; 2],
) -> (String, Value, Vec<String>) {
    let id = start_capture(rig, false);
    let mut answered: HashSet<String> = HashSet::new();
    let mut log = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let s = status(rig, &id);
        let pane = s["pane"].as_str().unwrap_or_default().to_string();
        if let Some(kind) = s["needs_you"]["kind"].as_str() {
            let once = format!("{}-{kind}-{}", s["step"].as_str().unwrap_or(""), s["permission_phase"]);
            if !answered.contains(&once) {
                let typed = match (kind, s["permission_phase"].as_u64()) {
                    ("trust", _) => "1",
                    ("permission", Some(1)) => keys[1],
                    ("permission", _) => keys[0],
                    (other, _) => panic!("an unexpected question ({other}): {s}"),
                };
                if typed == "<enter>" {
                    session.type_as_user(&pane, "", true).expect("the person presses Enter");
                } else {
                    session.type_as_user(&pane, typed, false).expect("the person types");
                }
                answered.insert(once);
                log.push(format!("{kind}:{typed}"));
            }
        }
        if s["step"] == "exit" && !answered.contains("exit") {
            answered.insert("exit".into());
            rig.call("capture", json!({"session": id, "op": "act", "action": "try_exit"})).unwrap_or_else(|e| panic!("try_exit: {}", err(&e)));
        }
        if s["settled"] == true {
            break;
        }
        assert!(Instant::now() < deadline, "the capture did not settle: {s}");
        std::thread::sleep(Duration::from_millis(150));
    }
    let finished = rig.call("capture", json!({"session": id, "op": "finish"})).unwrap_or_else(|e| panic!("capture finish: {}", err(&e)));
    (id, finished["captures"].clone(), log)
}

/// The visible test, with the person answering the folder question in every new pane it opens.
fn visible_test_as_the_person(session: &Session, rig: &puppet_master_app_lib::fake_tui::WizardRig, id: &str, plan: &str, known: &[String]) -> Value {
    std::thread::scope(|scope| {
        let handle = scope.spawn(|| rig.call("visibletest", json!({"session": id, "confirm": true, "plan": plan})));
        let deadline = Instant::now() + Duration::from_secs(240);
        let mut trusted: HashSet<String> = HashSet::new();
        while !handle.is_finished() {
            assert!(Instant::now() < deadline, "the visible test did not finish");
            let s = rig.call("capture", json!({"session": id, "op": "test_status"})).unwrap_or_else(|e| panic!("test_status: {}", err(&e)));
            if s["needs_you"]["kind"] == "trust" {
                if let Some(pane) = session.pane_ids().into_iter().find(|p| !known.contains(p) && !trusted.contains(p)) {
                    session.type_as_user(&pane, "1", false).expect("the person types");
                    trusted.insert(pane);
                }
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        handle.join().unwrap()
    })
    .unwrap_or_else(|e| panic!("visibletest: {}", err(&e)))
}

fn the_wizard_on_an_inline_agent(reprint: bool, name: &str) {
    let session = World::get().session(name);
    let script = {
        // top-level keys must come before the first table: put them at the head of the script
        let tail = SCRIPT.trim_start();
        format!("reprint_on_resize = {reprint}\nbacklog_rows = 32\n{tail}")
    };
    let rig = session.wizard_rig("nimbus", &script);
    let (id, captures, did) = capture_as_the_person(&session, &rig, ["y", "y"]);
    assert_eq!(did, ["trust:1", "permission:y", "permission:y"], "{did:?}");
    let facts = &captures["facts"];
    assert_eq!(facts["reprints_on_resize"], reprint, "the capture measured what the agent does: {facts}");
    assert_eq!(facts["resize_probe"]["from"], json!({"cols": 120, "rows": 40}));
    assert_eq!(facts["resize_probe"]["to"], json!({"cols": 100, "rows": 40}));
    let after = facts["resize_probe"]["after_rows"].as_array().unwrap().len();
    let before = facts["resize_probe"]["before_rows"].as_array().unwrap().len();
    assert_eq!(after > before, reprint, "evidence: {} -> {} rows\n{}", before, after, facts["resize_probe"]);
    assert!(facts["notes"].as_array().unwrap().iter().all(|n| !n.as_str().unwrap_or("").contains("changed size")), "{}", facts["notes"]);

    // the wizard writes the measured flag into the draft; the drafter did not have to guess it
    let draft = DRAFT.replace("@@COMMAND@@", "nimbus");
    let expect = json!({"tui_idle": "chrome_lines = [\"NIMBUS 0.4.2\"]\n"});
    let verified = rig.call("verify", json!({"session": id, "toml": draft, "expect": expect})).unwrap_or_else(|e| panic!("verify: {}", err(&e)));
    assert_eq!(verified["ok"], true, "verify said:\n{}", verified["text"].as_str().unwrap_or_default());
    assert!(rig.draft_text(&id).contains(&format!("reprints_on_resize = {reprint}")), "{}", rig.draft_text(&id));

    // the visible test resizes its own pane once after the reply: the conversation shows once and the grids agree
    let grid = session.pane_ids();
    let plan = verified["planned_visible"]["fingerprint"].as_str().unwrap().to_string();
    let test = visible_test_as_the_person(&session, &rig, &id, &plan, &grid);
    assert_eq!(test["ok"], true, "the visible test said:\n{}\n{}", test["text"].as_str().unwrap_or_default(), test["checks"]);
    let resize = test["checks"].as_array().unwrap().iter().find(|c| c["name"] == "resize").unwrap();
    assert_eq!(resize["status"], "passed", "{resize}");

    let done = rig
        .call("finish", json!({"session": id, "provenance": {"drafter": "Stand-in drafter", "drafter_kind": "other", "prompt_version": "7", "rounds": 1}}))
        .unwrap_or_else(|e| panic!("finish: {}", err(&e)));
    let written = std::fs::read_to_string(rig.agents_dir().join("nimbus.toml")).unwrap();
    assert!(written.contains(&format!("reprints_on_resize = {reprint}")), "{written}");
    assert_eq!(done["fixtures"], json!({"passed": 8, "total": 8}));
    session.until(10, || session.pane_ids().is_empty().then_some(())).unwrap_or_else(|| panic!("panes left: {:?}", session.pane_ids()));
}

#[test]
fn the_wizard_measures_an_inline_agent_that_reprints_its_conversation_and_writes_the_flag() {
    the_wizard_on_an_inline_agent(true, "wizard-inline-reprint");
}

#[test]
fn the_wizard_measures_an_inline_agent_that_does_not_reprint_and_writes_false() {
    the_wizard_on_an_inline_agent(false, "wizard-inline-plain");
}

/// A command asks with a numbered menu, a file write with an arrow menu: two screens, so two tables.
const MENUS_SCRIPT: &str = r#"
approvals = "menus"
agent = "nimbus"
boot_ms = 200

[[startup]]
gate = "trust"
after_ms = 150

[[turn]]
on_prompt = "hello.txt"
steps = [
  { do = "think", ms = 1200 },
  { do = "approve", kind = "create", path = "hello.txt", on_allow = [
      { do = "write_file", path = "hello.txt", content = "{last_word}\n" },
      { do = "say", text = "Created hello.txt." },
    ], on_deny = [{ do = "say", text = "Not created." }] },
]

[[turn]]
on_prompt = "shell command"
steps = [
  { do = "think", ms = 1000 },
  { do = "approve", kind = "bash", command = "echo ok", on_allow = [{ do = "say", text = "ok" }], on_deny = [{ do = "say", text = "Not run." }] },
]

[[turn]]
on_prompt = "exactly the word"
steps = [
  { do = "think", ms = 2200 },
  { do = "say", text = "{last_word}" },
]
"#;

const COMMAND_AND_FILE_TABLES: &str = r#"
[[tui.prompt]]
kind = "command_approval"
detect_regex = 'run this command\?'
choices = [
  { id = "allow_once", text = "1" },
  { id = "deny", text = "3" },
]

[[tui.prompt]]
kind = "file_edit"
detect_regex = 'write \S+\?'
choices = [
  { id = "allow_once", key = "enter" },
  { id = "deny", key = "escape" },
]
"#;

#[test]
fn a_command_menu_and_a_file_menu_each_need_their_own_table_and_each_becomes_a_card() {
    let session = World::get().session("wizard-two-menus");
    let rig = session.wizard_rig("nimbus", MENUS_SCRIPT);
    // the person presses Enter on the arrow menu (the cursor is on "Yes, write it") and 1 on the numbered menu
    let (id, captures, did) = capture_as_the_person(&session, &rig, ["<enter>", "1"]);
    assert_eq!(did, ["trust:1", "permission:<enter>", "permission:1"], "{did:?}");
    let names: Vec<&str> = captures["files"].as_array().unwrap().iter().filter_map(|f| f["name"].as_str()).collect();
    assert_eq!(names, ["tui_trust", "tui_idle", "tui_working", "tui_reply", "tui_permission", "tui_permission_command"], "{captures}");
    let file = captures["files"].as_array().unwrap().iter().find(|f| f["name"] == "tui_permission").unwrap()["text"].as_str().unwrap().to_string();
    let command = captures["files"].as_array().unwrap().iter().find(|f| f["name"] == "tui_permission_command").unwrap()["text"].as_str().unwrap().to_string();
    assert!(file.contains("Write hello.txt?") && file.contains("up/down to move"), "{file}");
    assert!(command.contains("Run this command?") && command.contains("1. Allow once"), "{command}");

    // a draft that only knows the y/n line does not read either menu: verify names both captures
    let narrow = DRAFT.replace("@@COMMAND@@", "nimbus");
    let expect = json!({"tui_idle": "chrome_lines = [\"NIMBUS 0.4.2\"]\n"});
    let refused = rig.call("verify", json!({"session": id, "toml": narrow, "expect": expect})).unwrap_or_else(|e| panic!("verify: {}", err(&e)));
    assert_eq!(refused["ok"], false);
    let text = refused["text"].as_str().unwrap_or_default();
    assert!(text.contains("tui_permission") && text.contains("tui_permission_command") && text.contains("[[tui.prompt]]"), "{text}");

    // with one table per kind it passes, and the visible test makes a card of each question
    let draft = format!("{}\n{COMMAND_AND_FILE_TABLES}", DRAFT.replace("@@COMMAND@@", "nimbus"));
    let verified = rig.call("verify", json!({"session": id, "toml": draft, "expect": expect})).unwrap_or_else(|e| panic!("verify: {}", err(&e)));
    assert_eq!(verified["ok"], true, "verify said:\n{}", verified["text"].as_str().unwrap_or_default());
    let grid = session.pane_ids();
    let plan = verified["planned_visible"]["fingerprint"].as_str().unwrap().to_string();
    let test = visible_test_as_the_person(&session, &rig, &id, &plan, &grid);
    assert_eq!(test["ok"], true, "the visible test said:\n{}\n{}", test["text"].as_str().unwrap_or_default(), test["checks"]);
    for name in ["permission", "permission_command"] {
        let check = test["checks"].as_array().unwrap().iter().find(|c| c["name"] == name).unwrap();
        assert_eq!(check["status"], "passed", "{check}");
    }
    let done = rig
        .call("finish", json!({"session": id, "provenance": {"drafter": "Stand-in drafter", "drafter_kind": "other", "prompt_version": "7", "rounds": 1}}))
        .unwrap_or_else(|e| panic!("finish: {}", err(&e)));
    assert_eq!(done["fixtures"], json!({"passed": 6, "total": 6}));
    let folder = rig.agents_dir().join("nimbus");
    let kind_of = |file: &str| std::fs::read_to_string(folder.join(file)).unwrap();
    assert!(kind_of("tui_permission.expect.toml").contains("prompt_kind = \"file_edit\""), "{}", kind_of("tui_permission.expect.toml"));
    assert!(kind_of("tui_permission_command.expect.toml").contains("prompt_kind = \"command_approval\""), "{}", kind_of("tui_permission_command.expect.toml"));
    let report = rig.check_definition(&rig.agents_dir().join("nimbus.toml"));
    assert!(report["results"].as_array().unwrap().iter().all(|r| r["passed"] == true), "{report}");
    session.until(10, || session.pane_ids().is_empty().then_some(())).unwrap_or_else(|| panic!("panes left: {:?}", session.pane_ids()));
}

// ---- the model must still be read once a tall reply has pushed the banner away -------------------------------------------

/// A profile whose `[tui.footer_model]` only matches the startup banner reads the model while the banner is on screen
/// and loses it in a longer session. The visible test runs a reply taller than the pane and says so; a regex that also
/// matches the status row passes.
#[test]
fn a_model_regex_that_only_matches_the_banner_fails_the_visible_test_after_a_tall_reply_and_a_status_row_regex_passes() {
    let session = World::get().session("wizard-visible-model");
    let script = format!("model = \"zeta-pro\"\nstatus_model = \"zeta-pro\"\n{}", SCRIPT.trim_start());
    let rig = session.wizard_rig("nimbus", &script);
    let (id, captures, did) = capture_as_the_person(&session, &rig, ["y", "y"]);
    assert_eq!(did, ["trust:1", "permission:y", "permission:y"], "{did:?}");
    assert!(captures["files"].to_string().contains("model: zeta-pro"), "the capture shows the model on the status row");

    let base = DRAFT.replace("@@COMMAND@@", "nimbus").replace("'^/help for commands',", "'^/help for commands', '^model: ',");
    let banner_only = format!("{base}\n[tui.footer_model]\nregex = '^nimbus\\s+[\\d.]+\\s*\\|\\s*(?P<model>[a-z-]+)'\n");
    let status_row = format!("{base}\n[tui.footer_model]\nregex = '(?:^nimbus\\s+[\\d.]+\\s*\\|\\s*|model:\\s*)(?P<model>[a-z-]+)'\n");
    let expect = json!({"tui_idle": "chrome_lines = [\"NIMBUS 0.4.2\"]\n"});
    let grid = session.pane_ids();

    let run = |draft: &str| {
        let verified = rig.call("verify", json!({"session": id, "toml": draft, "expect": expect})).unwrap_or_else(|e| panic!("verify: {}", err(&e)));
        assert_eq!(verified["ok"], true, "verify said:\n{}", verified["text"].as_str().unwrap_or_default());
        let plan = verified["planned_visible"]["fingerprint"].as_str().unwrap().to_string();
        visible_test_as_the_person(&session, &rig, &id, &plan, &grid)
    };
    let check = |test: &Value, name: &str| test["checks"].as_array().unwrap().iter().find(|c| c["name"] == name).cloned().unwrap_or(Value::Null);

    let failed = run(&banner_only);
    assert_eq!(failed["ok"], false, "{}", failed["checks"]);
    assert_eq!(check(&failed, "model")["status"], "passed", "while the banner is on screen the model is read: {}", failed["checks"]);
    let long = check(&failed, "long_reply");
    assert_eq!(long["status"], "failed", "{}", failed["checks"]);
    let text = failed["text"].as_str().unwrap();
    assert!(text.contains("VISIBLETEST_LONG_REPLY_FAILED") && text.contains("zeta-pro") && text.contains("banner scrolls away"), "{text}");

    let passed = run(&status_row);
    assert_eq!(passed["ok"], true, "the visible test said:\n{}\n{}", passed["text"].as_str().unwrap_or_default(), passed["checks"]);
    let long = check(&passed, "long_reply");
    assert_eq!(long["status"], "passed", "{long}");
    assert!(long["note"].as_str().unwrap().contains("zeta-pro"), "{long}");
    session.until(10, || session.pane_ids().is_empty().then_some(())).unwrap_or_else(|| panic!("panes left: {:?}", session.pane_ids()));
}

/// An inline TUI prints a logo, its name with a version and a "Signed in as" row before anything else. A draft whose
/// `[[tui.prompt]]` regex is too broad fails the visible test; that is the draft's fault, so the wizard must hand it
/// back for a fix round and never say "the agent itself returned an error: <the banner>".
#[test]
fn a_too_broad_prompt_regex_on_an_agent_with_a_banner_is_the_drafts_fault_not_the_agents_own_error() {
    let session = World::get().session("wizard-banner-broad");
    let tail = SCRIPT.trim_start();
    let banner = r#"banner_rows = ["\u2584\u2580 Nimbus CLI 1.4.2.", "Signed in as dev@example.com"]"#;
    let script = format!("{banner}\n{tail}");
    let rig = session.wizard_rig("nimbus", &script);
    let (id, _captures, did) = capture_as_the_person(&session, &rig, ["y", "y"]);
    assert_eq!(did, ["trust:1", "permission:y", "permission:y"], "{did:?}");

    // the command-approval table matches the echo of the task (not a question): the profile reads the working screen as one
    let draft = DRAFT.replace("@@COMMAND@@", "nimbus").replace(r"detect_regex = 'allow nimbus to .*\[y/n\]'", "detect_regex = 'reply with exactly'");
    assert!(draft.contains("detect_regex = 'reply with exactly'"));
    let expect = json!({"tui_idle": "chrome_lines = [\"NIMBUS 0.4.2\"]\n"});
    let verified = rig.call("verify", json!({"session": id, "toml": draft, "expect": expect})).unwrap_or_else(|e| panic!("verify: {}", err(&e)));
    let plan = verified["planned_visible"]["fingerprint"].as_str().unwrap_or_else(|| panic!("the draft can still be tested: {verified}")).to_string();
    let grid = session.pane_ids();
    let test = visible_test_as_the_person(&session, &rig, &id, &plan, &grid);
    assert_eq!(test["ok"], false, "{}", test["checks"]);
    assert_eq!(test["agent_error"], json!(null), "the banner is not an error; the failure is the draft's: {}\n{}", test["agent_error"], test["text"]);
}

// ---- an account row shown since startup is status, not an error; a new error row still is (founder run 2026-10-19) -----

const ADDRESS: &str = "someone@example.com";

/// The same agent, with the account row some TUIs draw under their footer from startup on (the person's address and the
/// plan in parentheses: "quota" in it matched the external-error words).
fn with_account_row(script: &str) -> String {
    format!("account_row = \"{ADDRESS} (Nimbus Starter Quota)\"\n{}", script.trim_start())
}

/// The recorded draft, plus a chrome row for the account row (what the drafter writes once it was shown the failure).
fn draft_with_account_chrome() -> String {
    let draft = DRAFT.replace("@@COMMAND@@", "nimbus");
    let at = "  '^done in \\d+s$',";
    assert!(draft.contains(at));
    draft.replace(at, &format!("{at}\n  '^\\S+@\\S+\\s+\\(.*quota\\)$',"))
}

fn run_visible(session: &Session, rig: &puppet_master_app_lib::fake_tui::WizardRig, id: &str, draft: &str, known: &[String]) -> Value {
    let expect = json!({"tui_idle": "chrome_lines = [\"NIMBUS 0.4.2\"]\n"});
    let verified = rig.call("verify", json!({"session": id, "toml": draft, "expect": expect})).unwrap_or_else(|e| panic!("verify: {}", err(&e)));
    let plan = verified["planned_visible"]["fingerprint"].as_str().unwrap_or_else(|| panic!("the draft can still be tested: {verified}")).to_string();
    visible_test_as_the_person(session, rig, id, &plan, known)
}

#[test]
fn an_account_row_shown_since_startup_is_status_the_loop_goes_on_with_a_fix_round_and_no_address_leaves_the_wizard() {
    let session = World::get().session("wizard-account-row");
    let rig = session.wizard_rig("nimbus", &with_account_row(SCRIPT));
    let (id, captures, did) = capture_as_the_person(&session, &rig, ["y", "y"]);
    assert_eq!(did, ["trust:1", "permission:y", "permission:y"], "{did:?}");
    // what the drafter and the UI are shown of the captures: the row, with the address masked
    let shown = captures.to_string();
    assert!(!shown.contains(ADDRESS), "no address in the captures the drafter sees: {shown}");
    assert!(shown.contains("<email> (Nimbus Starter Quota)"), "{shown}");

    // round 1: the draft does not know the account row, so the reply is not clean. That is the draft's fault.
    let grid = session.pane_ids();
    let first = run_visible(&session, &rig, &id, &DRAFT.replace("@@COMMAND@@", "nimbus"), &grid);
    assert_eq!(first["ok"], false, "{}", first["checks"]);
    assert_eq!(first["agent_error"], json!(null), "the account row is status, not the agent's error: {}\n{}", first["agent_error"], first["text"]);
    let text = first["text"].as_str().unwrap_or_default();
    assert!(text.contains("<email> (Nimbus Starter Quota)"), "the drafter is told which row is in the way, masked: {text}");
    assert!(!first.to_string().contains(ADDRESS), "no address in the reply of the visible test: {first}");

    // round 2: the fix round (a chrome row for it) passes, and what is written holds no address
    let grid = session.pane_ids();
    let second = run_visible(&session, &rig, &id, &draft_with_account_chrome(), &grid);
    assert_eq!(second["ok"], true, "{}\n{}", second["text"].as_str().unwrap_or_default(), second["checks"]);
    session.until(10, || session.pane_ids().is_empty().then_some(())).unwrap_or_else(|| panic!("panes left: {:?}", session.pane_ids()));
    let provenance = json!({"drafter": "Test drafter", "drafter_kind": "other", "model": null, "prompt_version": "1", "rounds": 2});
    let done = rig.call("finish", json!({"session": id, "provenance": provenance})).unwrap_or_else(|e| panic!("finish: {}", err(&e)));
    let folder = std::path::PathBuf::from(done["fixtures_dir"].as_str().expect("a fixtures folder"));
    let mut saved = 0;
    for entry in std::fs::read_dir(&folder).unwrap().flatten() {
        let text = std::fs::read_to_string(entry.path()).unwrap();
        assert!(!text.contains(ADDRESS), "{:?} holds the address:\n{text}", entry.file_name());
        saved += 1;
    }
    assert!(saved >= 6, "fixtures were written: {saved}");
    let record = std::fs::read_to_string(done["provenance_file"].as_str().unwrap()).unwrap();
    assert!(!record.contains(ADDRESS), "{record}");
    assert!(std::fs::read_to_string(folder.join("tui_idle.txt")).unwrap().contains("<email> (Nimbus Starter Quota)"));
}

#[test]
fn a_new_error_row_after_the_task_still_stops_the_visible_test_with_that_row_beside_an_account_row() {
    let session = World::get().session("wizard-account-new-error");
    let rig = session.wizard_rig("nimbus", &with_account_row(SCRIPT));
    let (id, _captures, _did) = capture_as_the_person(&session, &rig, ["y", "y"]);
    // the agent now answers every plain message with a new error row (the capture is done: the script is read at start)
    let failing = SCRIPT.replace("{ do = \"say\", text = \"{last_word}\" },\n]\n\"#", "").replace(
        "on_prompt = \"exactly the word\"\nsteps = [\n  { do = \"think\", ms = 2200 },\n  { do = \"say\", text = \"{last_word}\" },\n]",
        "on_prompt = \"exactly the word\"\nsteps = [\n  { do = \"think\", ms = 1500 },\n  { do = \"say\", text = \"Error: quota exceeded\" },\n]",
    );
    assert!(failing.contains("Error: quota exceeded"), "the script was changed");
    session.script(&with_account_row(&failing));
    let grid = session.pane_ids();
    let test = run_visible(&session, &rig, &id, &draft_with_account_chrome(), &grid);
    assert_eq!(test["ok"], false, "{}", test["checks"]);
    let ae = &test["agent_error"];
    assert!(ae["message"].as_str().unwrap_or_default().ends_with("Error: quota exceeded"), "the new error row is the message, not the account row: {test}");
    assert_eq!(ae["external"], true, "{ae}");
    assert!(!test.to_string().contains(ADDRESS), "{test}");
}
