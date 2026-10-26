//! The wizard's panes stay in the wizard's folder (the founder's rule, SPEC 11.16). A real wizard run on a new CLI asked
//! the agent to "list the files in this folder"; the agent ran `Get-Process`, `Get-CimInstance Win32_Process`, listed the
//! founder's home folder and read the founder's source files, and the wizard answered "yes" every time, including a menu
//! that says "Yes, allow access / Yes, and always allow non-workspace access / No, deny access".
//!
//! Generic rules under test (nothing here names an agent): a question that names a path outside the wizard's folder, says
//! the access is outside the workspace, or asks to inspect or control other processes or the machine is answered with the
//! plain "No" row; with no such row it is left to the person; a question inside the folder is still answered "yes" once.
//! The screens in `fixtures/agy_questions.txt` are the real questions of that run, in the order the wizard met them.

use std::path::PathBuf;

use crate::tui::declarative::wizard::visible::answer::{can_look, AnswerCfg, AnswerKey, Answerer, Input, Step};
use crate::tui::declarative::wizard::visible::screen::Flavour;

const FOLDER: &str = r"C:\Users\user\AppData\Local\Temp\pm-wizard-wz_b16bc1afe839";

fn answerer() -> Answerer {
    Answerer::new("visibletest", AnswerCfg::default()).with_folder(Some(PathBuf::from(FOLDER)))
}

fn look(a: &mut Answerer, screen: &str, flavour: Option<Flavour>, stable_ms: u64, now: u64) -> Step {
    a.step(&Input { screen, styled: None, flavour, stable_ms, now, hint: None })
}

/// What the answerer does with the question on `screen`: the first key it presses (a number picks the row) or why it
/// declined, with the decision it logged.
fn verdict(a: &mut Answerer, screen: &str) -> Result<String, String> {
    match look(a, screen, Some(Flavour::Permission), 1500, 1500) {
        Step::Press { keys, .. } => Ok(keys.iter().map(AnswerKey::say).collect::<Vec<_>>().join(",")),
        Step::Declined(reason) => Err(reason),
        other => panic!("expected keys or a refusal, got {other:?}"),
    }
}

fn real_questions() -> Vec<&'static str> {
    include_str!("fixtures/agy_questions.txt").split("\n=====\n").collect()
}

/// A command question as that CLI draws it (the broad rows repeat the command and wrap).
fn command_question(command: &str) -> String {
    format!(
        "> List the files in this folder, then reply with exactly: folder listed, code <word-3>.\n\n\u{25cf} Ran (Get-ChildItem) (ctrl+o to expand)\n\nCommand\n{rule}\n\nRequesting permission for:\n   {command}\n\nRun this command?\n> 1. Yes, run command\n  2. Yes, and always allow in this conversation for commands that start with '{command}'\n  3. Yes, and always allow for commands that start with '{command}' (Persist to settings.json)\n  4. No, cancel\n\n  \u{2191}/\u{2193} Navigate \u{b7} tab Amend \u{b7} ctrl+g edit/expand command\nesc to cancel                                                                                    Small One \u{b7} high\n",
        rule = "\u{2500}".repeat(120)
    )
}

#[test]
fn the_non_workspace_menu_gets_the_deny_row_not_yes() {
    let screen = real_questions()[7];
    assert!(screen.contains("1. Yes, allow access") && screen.contains("Reason: outside workspace"));
    let mut a = answerer();
    // "3. No, deny access"
    assert_eq!(verdict(&mut a, screen), Ok("3".to_string()));
    let said = a.decisions().last().expect("a decision")["decision"].as_str().unwrap().to_string();
    assert!(said.starts_with("Denied: the agent asked for access outside the wizard's temp folder:"), "{said}");
    assert!(said.contains("No, deny access") && !said.contains("will answer: Yes"), "{said}");
    // the number only moved the cursor: Enter confirms it, on the deny row and nowhere else
    let moved = screen.replace("> 1. Yes, allow access\n  2.", "  1. Yes, allow access\n  2.").replace("  3. No, deny access", "> 3. No, deny access");
    assert_eq!(look(&mut a, &moved, Some(Flavour::Permission), 2600, 2100), Step::Press { keys: vec![AnswerKey::Enter], note: "Denying the permission question: No, deny access".into() });
    let Step::Answered(done) = look(&mut a, "ready", None, 0, 2400) else { panic!("answered") };
    assert_eq!((done.label.as_str(), done.keys.as_str()), ("No, deny access", "3, Enter"));
    assert!(done.denied.as_deref().is_some_and(|why| why.contains("outside the wizard's temp folder")), "{done:?}");
    assert!(done.to_json(&|t: &str| t.to_string())["denied"].as_str().is_some());
}

#[test]
fn a_command_that_names_a_path_outside_the_folder_gets_the_no_row() {
    for command in [
        r#"Get-ChildItem -Path "C:\Users\user\x""#,
        r#"Get-ChildItem -Path "C:\Users\user\Desktop\work2\puppet-master" -Recurse -Filter "*wizard*""#,
        r"Get-Content D:\secrets\keys.txt",
        r"Get-ChildItem -Path ..\..\",
        r"Get-Content ..\..\..\notes.txt",
        r"Get-ChildItem ~\Desktop",
        r"Get-ChildItem $env:USERPROFILE\Documents",
    ] {
        let mut a = answerer();
        assert_eq!(verdict(&mut a, &command_question(command)), Ok("4".to_string()), "{command}");
        let said = a.decisions().last().unwrap()["decision"].as_str().unwrap().to_string();
        assert!(said.starts_with("Denied:") && said.contains("4. No, cancel") || said.contains("No, cancel"), "{said}");
    }
}

#[test]
fn a_command_that_inspects_or_controls_processes_or_the_machine_gets_the_no_row() {
    for command in [
        r#"Get-Process | Where-Object { $_.ProcessName -match "node" }"#,
        r#"Get-CimInstance Win32_Process -Filter "Name = 'x.exe'" | Select-Object ProcessId, CommandLine"#,
        "Stop-Process -Id 4242",
        "tasklist /v",
        "taskkill /F /IM x.exe",
        "ps aux | grep node",
        "kill -9 4242",
        "pkill node",
        "wmic process list brief",
    ] {
        let mut a = answerer();
        assert_eq!(verdict(&mut a, &command_question(command)), Ok("4".to_string()), "{command}");
        let said = a.decisions().last().unwrap()["decision"].as_str().unwrap().to_string();
        assert!(said.contains("other processes or the machine"), "{said}");
    }
}

#[test]
fn a_command_with_no_path_or_a_path_inside_the_folder_is_still_answered_yes_once() {
    for command in [
        "Get-ChildItem",
        "Get-ChildItem -Force",
        "Get-ChildItem -Recurse -Force .puppet-master",
        r"Get-ChildItem -Path .\sub",
        r"Get-ChildItem sub\..\other",
        "echo hello",
        r#"Get-Content "C:\Users\user\AppData\Local\Temp\pm-wizard-wz_b16bc1afe839\test-project\hello.txt""#,
        r#"Get-ChildItem -Force "C:\Users\user\AppData\Local\Temp\pm-wizard-wz_b16bc1afe839""#,
    ] {
        let mut a = answerer();
        assert_eq!(verdict(&mut a, &command_question(command)), Ok("1".to_string()), "{command}");
        let said = a.decisions().last().unwrap()["decision"].as_str().unwrap().to_string();
        assert!(said.starts_with("will answer: Yes, run command"), "{said}");
    }
}

#[test]
fn a_path_in_the_transcript_above_the_question_does_not_deny_a_harmless_one() {
    let screen = command_question("Get-ChildItem").replace(
        "\u{25cf} Ran (Get-ChildItem) (ctrl+o to expand)",
        "\u{25cf} Ran 3 commands (Select-String -Path \"C:\\Users\\user\\Desktop\\work2\\puppet-master\\x\") (ctrl+o to expand)",
    );
    assert_eq!(verdict(&mut answerer(), &screen), Ok("1".to_string()));
}

#[test]
fn the_real_questions_of_the_run_get_the_answers_they_should() {
    // In the order the wizard met them. `true` = it may answer yes (nothing leaves the folder).
    let expected = [true, true, true, false, false, false, false, false, false, false, false, false];
    let questions = real_questions();
    assert_eq!(questions.len(), expected.len());
    for (i, (screen, yes)) in questions.iter().zip(expected).enumerate() {
        let mut a = answerer();
        let keys = verdict(&mut a, screen).unwrap_or_else(|why| panic!("question {i}: {why}"));
        let want = if screen.contains("allow access") { if yes { "1" } else { "3" } } else if yes { "1" } else { "4" };
        assert_eq!(keys, want, "question {i}:\n{screen}");
    }
}

#[test]
fn with_no_plain_no_row_the_question_goes_to_the_person_and_no_look_may_press_yes() {
    let screen = "Read: C:/Users/user/Desktop/x.txt\nReason: outside workspace\n\nAllow access to this file?\n> 1. Yes, allow access\n  2. Yes, and always allow non-workspace access\n";
    let mut a = answerer();
    let reason = verdict(&mut a, screen).expect_err("declined");
    assert!(reason.contains("outside the wizard's temp folder") && reason.contains("no plain \"No\" row was found"), "{reason}");
    assert!(!can_look(&reason), "an AI look must not be asked to pick for it: {reason}");
    // a menu nobody could read, with the path in it
    let mut b = answerer();
    let reason = verdict(&mut b, "Allow the agent to read C:\\Users\\user\\Desktop\\x.txt?\n").expect_err("declined");
    assert!(reason.contains("outside the wizard's temp folder") && !can_look(&reason), "{reason}");
    // a trust question whose only refusal quits the program has no plain "No" either
    let mut c = answerer();
    let reason = verdict(&mut c, "Do you trust the files in this folder?\n  D:\\other\\place\n\n> Yes, I trust this folder\n  No, exit\n").expect_err("declined");
    assert!(reason.contains("D:\\other\\place") && !can_look(&reason), "{reason}");
}

#[test]
fn a_yes_no_line_that_reaches_outside_gets_n() {
    let mut a = answerer();
    assert_eq!(verdict(&mut a, "Allow the agent to run `Get-Process` ? [y/n]\n"), Ok("n".to_string()));
    let mut b = answerer();
    assert_eq!(verdict(&mut b, "Allow the agent to create hello.txt? [y/n]\n"), Ok("y".to_string()));
}

#[test]
fn with_no_known_folder_nothing_outside_is_assumed_inside() {
    let mut a = Answerer::new("visibletest", AnswerCfg::default());
    assert_eq!(verdict(&mut a, &command_question(r#"Get-ChildItem -Path "C:\Users\user\AppData\Local\Temp\pm-wizard-wz_b16bc1afe839""#)), Ok("4".to_string()));
}

#[test]
fn a_numbered_menu_whose_broad_options_the_pane_wrapped_is_read_whole_down_to_its_no_row() {
    use crate::tui::declarative::wizard::visible::answer::parse_menu;
    // the real question of the run: options 2 and 3 repeat the command and run over onto a second row
    let screen = real_questions()[3];
    assert!(screen.contains("\"puppet|wizard|python|node\" }'"), "{screen}");
    let menu = parse_menu(screen).expect("a menu");
    let texts: Vec<&str> = menu.options.iter().map(|o| o.text.split(',').next().unwrap_or("")).collect();
    assert_eq!(menu.options.len(), 4, "{texts:?}");
    assert_eq!(menu.options[3].text, "No, cancel");
    assert_eq!((menu.highlighted(), menu.options[0].text.as_str()), (Some(0), "Yes, run command"));
}
