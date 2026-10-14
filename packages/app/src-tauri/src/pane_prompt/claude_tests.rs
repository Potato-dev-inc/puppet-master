// Claude Code v2.1.299, real captures (tui/claude/fixtures). Included from `tests.rs`.

use super::*;

const CLAUDE_BASH_MULTI_CD: &str = include_str!("../tui/claude/fixtures/bash_multi_cd_2_1_299.txt");
const CLAUDE_BASH_FOUR: &str = include_str!("../tui/claude/fixtures/bash_simple_4_options_2_1_299.txt");
const CLAUDE_EDIT: &str = include_str!("../tui/claude/fixtures/edit_file_2_1_299.txt");
const CLAUDE_CREATE: &str = include_str!("../tui/claude/fixtures/create_file_2_1_299.txt");
const CLAUDE_TRUST: &str = include_str!("../tui/claude/fixtures/trust_dialog_2_1_299.txt");
const CLAUDE_READY: &str = "\n \u{2590}\u{259b}\u{2588}\u{2588}\u{2588}\u{259c}\u{258c}   Claude Code v2.1.299\n\u{259d}\u{259c}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{259b}\u{2580}  Haiku 5.5 \u{b7} Claude Pro\n\n\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} Try \"edit <filepath> to...\"\n\u{2500}\u{2500}\u{2500}\u{2500}\n";

#[test]
fn claudes_bash_dialog_becomes_a_card_with_the_command_and_a_risk() {
    // Live (2026-10-14): the pane read waiting_input but detect returned null, so the Inbox only
    // offered "Open terminal".
    let prompt = detect_in_screen("claude", CLAUDE_BASH_MULTI_CD, false).expect("the dialog is detected");
    assert_eq!(prompt["kind"], "command_approval");
    assert_eq!(prompt["command"], "cd /d/pm-demo 2>/dev/null || cd \"D:/pm-demo\"; pwd; ls -la; cat package.json 2>/dev/null");
    assert!(prompt["summary"].as_str().unwrap().starts_with("Run command: cd /d/pm-demo"));
    assert_eq!(prompt["choices"], json!(["allow_once", "deny"]));
    let risk = &prompt["risk"];
    assert!(risk["headline"].as_str().is_some_and(|h| !h.is_empty()), "{risk}");
    assert_ne!(risk["level"], "high", "a listing and a cat inside the project: {risk}");

    // The same dialog read for the answer (broad answers allowed) and with scrollback above it is
    // the same prompt.
    let id = prompt["prompt_id"].as_str().unwrap();
    let broad = detect_in_screen("claude", CLAUDE_BASH_MULTI_CD, true).unwrap();
    assert_eq!(broad["prompt_id"].as_str(), Some(id));
    let scrolled = detect_in_screen("claude", &format!("older output\n{CLAUDE_BASH_MULTI_CD}"), false).unwrap();
    assert_eq!(scrolled["prompt_id"].as_str(), Some(id));

    // The UI shape and the run's required_action carry the card.
    let ui = PanePrompt::from_detected(&prompt).expect("ui prompt");
    assert_eq!(ui.choices.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(), ["Allow once", "Deny"]);
    assert!(ui.risk.is_some());
    let action = required_action_from_prompt(prompt.clone());
    assert_eq!(action["kind"], "command_approval");
    assert_eq!(action["command"], prompt["command"]);
    assert!(action["risk"]["headline"].is_string());
}

#[test]
fn claudes_dangerous_command_is_rated_high_and_the_safe_one_is_not() {
    let rm = CLAUDE_BASH_FOUR.replace("npm test", "rm -rf /");
    let prompt = detect_in_screen("claude", &rm, false).expect("prompt");
    assert_eq!(prompt["command"], "rm -rf /");
    assert_eq!(prompt["risk"]["level"], "high");
    let test = detect_in_screen("claude", CLAUDE_BASH_FOUR, false).expect("prompt");
    assert_eq!(test["command"], "npm test");
    assert_ne!(test["risk"]["level"], "high");
}

#[test]
fn claudes_answers_move_the_pointer_and_broad_answers_need_the_opt_in() {
    let prompt = detect_in_screen("claude", CLAUDE_BASH_MULTI_CD, true).unwrap();
    assert_eq!(reply_key("claude", &prompt, "deny", false).unwrap().0, "\u{1b}[B\u{1b}[B");
    assert_eq!(reply_key("claude", &prompt, "allow_once", false).unwrap().0, "");
    assert_eq!(reply_key("claude", &prompt, "reject", false).unwrap().0, "\u{1b}[B\u{1b}[B", "the long names map to the same ids");
    assert_eq!(reply_key("claude", &prompt, "allow_all", false).unwrap_err().code, "BROAD_APPROVAL_REQUIRES_OPT_IN");
    assert_eq!(reply_key("claude", &prompt, "allow_all", true).unwrap().0, "\u{1b}[B");
    assert_eq!(reply_key("claude", &prompt, "trust", true).unwrap_err().code, "INVALID_PROMPT_CHOICE");
}

#[test]
fn claudes_edit_dialog_is_a_file_card_rated_by_where_the_file_is() {
    let edit = detect_in_screen("claude", CLAUDE_EDIT, false).expect("prompt");
    assert_eq!(edit["kind"], "file_edit");
    assert_eq!(edit["path"], "sum.js");
    assert_eq!(edit["summary"], "Edit sum.js");
    assert_eq!(edit["risk"]["level"], "low");
    assert_eq!(edit["risk"]["headline"], "Change a file in this project");
    // The broad row is only listed on request, and that must not change the id.
    let broad = detect_in_screen("claude", CLAUDE_EDIT, true).unwrap();
    assert_eq!(edit["prompt_id"], broad["prompt_id"]);
    assert_eq!(edit["choices"], json!(["allow_once", "deny"]));
    assert_eq!(broad["choices"], json!(["allow_once", "deny", "allow_all"]));

    let create = detect_in_screen("claude", CLAUDE_CREATE, false).expect("prompt");
    assert_eq!(create["summary"], "Create notes.tmp");
    assert_ne!(create["prompt_id"], edit["prompt_id"]);

    // Outside the project, or a file that holds secrets, is high; the pane's folder decides.
    let outside = CLAUDE_EDIT.replace("edit to sum.js?", "edit to C:\\Windows\\hosts?");
    let mut prompt = detect_in_screen("claude", &outside, false).expect("prompt");
    attach_risk(&mut prompt, Some("D:/pm-demo"));
    assert_eq!(prompt["risk"]["level"], "high");
    let secrets = CLAUDE_EDIT.replace("edit to sum.js?", "edit to .env?");
    let mut prompt = detect_in_screen("claude", &secrets, false).expect("prompt");
    attach_risk(&mut prompt, Some("D:/pm-demo"));
    assert_eq!(prompt["risk"]["level"], "high");
}

#[test]
fn claudes_trust_dialog_is_a_startup_gate_the_user_answers() {
    let prompt = startup_gate("claude", CLAUDE_TRUST).expect("the trust dialog is a startup gate");
    assert_eq!(prompt["kind"], "workspace_trust");
    assert_eq!(prompt["summary"], "Claude Code asks to trust D:\\pm-demo-untrusted-1");
    assert_eq!(prompt["risk"]["headline"], "Trust this folder");
    let ui = PanePrompt::from_detected(&prompt).expect("ui prompt");
    assert_eq!(ui.kind, "workspace_trust");
    assert_eq!(
        ui.choices.iter().map(|c| (c.id.as_str(), c.label.as_str())).collect::<Vec<_>>(),
        [("trust", "Trust this folder"), ("deny", "Don't trust")]
    );
    let action = required_action_from_prompt(prompt.clone());
    assert_eq!(action["options"], json!([{"id": "trust", "label": "Trust this folder"}, {"id": "deny", "label": "Don't trust"}]));
    // The id does not depend on how the dialog wraps.
    let wide = CLAUDE_TRUST.replace("Is this a project you created or one you trust?", "Is this a project you created\n or one you trust?");
    assert_eq!(detect_in_screen("claude", &wide, false).unwrap()["prompt_id"], prompt["prompt_id"]);
    // Trusting is a deliberate answer: one Down from "No, exit", then Enter.
    assert_eq!(reply_key("claude", &prompt, "trust", false).unwrap().0, "\u{1b}[B");
    assert_eq!(reply_key("claude", &prompt, "deny", false).unwrap().0, "");
    // A permission dialog is a prompt, but not a startup gate.
    assert!(startup_gate("claude", CLAUDE_BASH_MULTI_CD).is_none());
    assert!(startup_gate("claude", CLAUDE_READY).is_none());
}

#[test]
fn denying_the_trust_gate_is_recorded_as_a_final_decline_and_trusting_it_is_not() {
    let prompt = detect_in_screen("claude", CLAUDE_TRUST, false).expect("trust card");
    let (declined, trusted, other) = ("decline-pane-1", "decline-pane-2", "decline-pane-3");
    note_gate_answer(trusted, &prompt, "trust");
    note_gate_answer(declined, &prompt, "deny");
    assert!(gate_was_declined(declined));
    assert!(!gate_was_declined(trusted));
    // Denying a command approval is not a gate decline.
    let approval = detect_in_screen("claude", CLAUDE_BASH_MULTI_CD, false).expect("bash approval");
    note_gate_answer(other, &approval, "deny");
    assert!(!gate_was_declined(other));
}

#[test]
fn a_task_is_never_typed_over_claudes_dialogs() {
    for dialog in [CLAUDE_TRUST, CLAUDE_BASH_MULTI_CD, CLAUDE_BASH_FOUR, CLAUDE_EDIT, CLAUDE_CREATE] {
        let blocker = dispatch_blocker("claude", dialog).expect("blocked");
        assert_eq!(blocker.code, "PANE_BLOCKED_BY_PROMPT");
    }
    assert!(dispatch_blocker("claude", CLAUDE_READY).is_none(), "once the dialog is gone the task can be typed");
}
