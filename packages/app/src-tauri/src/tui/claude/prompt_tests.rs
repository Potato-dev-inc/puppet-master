use super::*;
use crate::tui::{profile_for, TuiProfile};

// Real captures from Claude Code v2.1.299 (Haiku 5.5), read from the pane the same way the
// prompt detector reads it (`vt100::Screen::contents()`, rows glued by full-width padding).
const BASH_MULTI_CD: &str = include_str!("fixtures/bash_multi_cd_2_1_299.txt");
const BASH_FOUR_OPTIONS: &str = include_str!("fixtures/bash_simple_4_options_2_1_299.txt");
const EDIT_FILE: &str = include_str!("fixtures/edit_file_2_1_299.txt");
const CREATE_FILE: &str = include_str!("fixtures/create_file_2_1_299.txt");
const TRUST: &str = include_str!("fixtures/trust_dialog_2_1_299.txt");
// Older capture (v2.1.287, 2026-10-06): the trust dialog of an earlier version.
const TRUST_OLD: &str = "\u{2500}\u{2500}\u{2500}\n Accessing workspace:\n\n sandbox\n\n Quick safety check: Is this a project you created or one you trust?\n\n \u{276f} No, exit\n   Yes, I trust this folder\n\n Enter to confirm \u{b7} Esc to cancel";

const DOWN: &str = "\u{1b}[B";
const UP: &str = "\u{1b}[A";

fn detect(screen: &str, allow_broad: bool) -> Value {
    detect_prompt("claude", screen, allow_broad).expect("a prompt")
}

fn key(prompt: &Value, choice: &str, allow_broad: bool) -> String {
    prompt_reply(prompt, choice, allow_broad).expect("a reply").key.expect("a key")
}

#[test]
fn the_multi_cd_bash_dialog_is_a_command_approval() {
    let prompt = detect(BASH_MULTI_CD, false);
    assert_eq!(prompt["kind"], "command_approval");
    assert_eq!(prompt["command"], "cd /d/pm-demo 2>/dev/null || cd \"D:/pm-demo\"; pwd; ls -la; cat package.json 2>/dev/null");
    assert_eq!(prompt["reason"], "Multiple directory changes in one command require approval for clarity");
    assert_eq!(prompt["choices"], json!(["allow_once", "deny"]), "no broad answer unless asked");
    assert_eq!(prompt["menu"]["selected"], 1);
    // Three rows: Yes / Yes, and switch to auto mode / No.
    assert_eq!(prompt["menu"]["options"][1]["choice"], "allow_all");
    assert_eq!(prompt["menu"]["options"][2]["choice"], "deny");
}

#[test]
fn answers_are_arrow_presses_from_the_highlighted_row() {
    let prompt = detect(BASH_MULTI_CD, true);
    assert_eq!(prompt["choices"], json!(["allow_once", "deny", "allow_all"]));
    // The pointer is on `1. Yes`; the caller's Enter submits.
    assert_eq!(key(&prompt, "allow_once", false), "");
    assert_eq!(key(&prompt, "deny", false), format!("{DOWN}{DOWN}"));
    assert_eq!(key(&prompt, "allow_all", true), DOWN);
    assert!(prompt_reply(&prompt, "allow_all", false).is_err(), "broad approval needs the opt-in");
    assert!(prompt_reply(&prompt, "trust", false).is_err());
}

#[test]
fn the_four_option_dialog_moves_no_to_row_four_and_keeps_one_broad_answer() {
    let prompt = detect(BASH_FOUR_OPTIONS, true);
    assert_eq!(prompt["kind"], "command_approval");
    assert_eq!(prompt["command"], "npm test");
    assert_eq!(prompt["reason"], "This command requires approval");
    let options = prompt["menu"]["options"].as_array().unwrap();
    assert_eq!(options.len(), 4);
    assert_eq!(options[1]["label"], "Yes, and don\u{2019}t ask again for: npm test *");
    // "don't ask again" is the most specific widening and takes the allow_all slot; the broader
    // "switch to auto mode" row is never offered.
    assert_eq!(options[1]["choice"], "allow_all");
    assert_eq!(options[2]["choice"], Value::Null);
    assert_eq!(options[3]["choice"], "deny");
    assert_eq!(key(&prompt, "deny", false), DOWN.repeat(3));
    assert_eq!(key(&prompt, "allow_all", true), DOWN);
}

#[test]
fn file_edit_dialogs_name_the_file_and_the_change() {
    let edit = detect(EDIT_FILE, true);
    assert_eq!(edit["kind"], "file_edit");
    assert_eq!(edit["path"], "sum.js");
    assert_eq!(edit["summary"], "Edit sum.js");
    assert_eq!(edit["menu"]["options"].as_array().unwrap().len(), 3);
    assert_eq!(edit["menu"]["options"][1]["choice"], "allow_all", "accept edits for this session is the broad row");
    assert_eq!(key(&edit, "deny", false), format!("{DOWN}{DOWN}"));
    assert_eq!(key(&edit, "allow_once", false), "");

    let create = detect(CREATE_FILE, false);
    assert_eq!(create["kind"], "file_edit");
    assert_eq!(create["path"], "notes.tmp");
    assert_eq!(create["summary"], "Create notes.tmp");
    assert_ne!(edit["identity"], create["identity"]);
}

#[test]
fn two_edits_to_one_file_are_two_prompts_but_a_redraw_is_one() {
    let first = detect(EDIT_FILE, false);
    let again = detect(&format!("older output\n{EDIT_FILE}"), true);
    assert_eq!(first["identity"], again["identity"]);
    let other = detect(&EDIT_FILE.replace("// Sums numbers.", "// Totals the list."), false);
    assert_ne!(first["identity"], other["identity"], "a different change to the same file is a different question");
}

#[test]
fn the_trust_dialog_is_a_startup_gate_with_trust_and_deny() {
    let prompt = detect(TRUST, false);
    assert_eq!(prompt["kind"], KIND_TRUST);
    assert_eq!(prompt["choices"], json!(["trust", "deny"]));
    assert_eq!(prompt["folder"], "D:\\pm-demo-untrusted-1");
    assert_eq!(prompt["summary"], "Claude Code asks to trust D:\\pm-demo-untrusted-1");
    assert_eq!(prompt["labels"]["deny"], "Don't trust");
    // The pointer starts on "No, exit": trusting is one row down, declining is Enter alone.
    assert_eq!(prompt["menu"]["selected"], 1);
    assert_eq!(key(&prompt, "trust", false), DOWN);
    assert_eq!(key(&prompt, "deny", false), "");
    assert!(prompt_reply(&prompt, "allow_once", false).is_err());

    let old = detect(TRUST_OLD, false);
    assert_eq!(old["kind"], KIND_TRUST);
    assert_eq!(old["folder"], "sandbox");
    assert_eq!(key(&old, "trust", false), DOWN);
    assert!(shows_trust_gate(TRUST) && shows_trust_gate(TRUST_OLD));
}

#[test]
fn a_trust_dialog_that_was_answered_is_not_a_gate() {
    // Once accepted, Claude draws its own screen over the dialog.
    let after = "\n \u{2590}\u{259b}\u{2588}\u{2588}\u{2588}\u{259c}\u{258c}   Claude Code v2.1.299\n\n\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} Try \"edit <filepath> to...\"\n\u{2500}\u{2500}\u{2500}\u{2500}\n";
    assert!(detect_prompt("claude", after, true).is_none());
    // The dialog with real output below it is scrollback, not the live dialog.
    let scrolled = format!("{TRUST}\n\u{25cf} Listing the project files now.\nI found three files.");
    assert!(!shows_trust_gate(&scrolled));
}

#[test]
fn a_numbered_list_in_the_reply_is_not_a_permission_dialog() {
    // The model quoting the question and a list: no pointer on an option, no footer.
    let reply = "\u{25cf} Do you want to proceed?\n  1. Yes\n  2. No\n\u{276f} \n";
    assert!(detect_prompt("claude", reply, true).is_none());
    // A real dialog whose only options are not yes/no.
    let other = "Do you want to continue?\n \u{276f} 1. Red\n   2. Blue\n Esc to cancel";
    assert!(detect_prompt("claude", other, true).is_none());
    // A dialog from an earlier turn with plenty of output drawn below it.
    let stale = format!("{EDIT_FILE}\n{}", "\u{25cf} The edit went through and here is a long summary of it.\n".repeat(6));
    assert!(detect_prompt("claude", &stale, true).is_none());
    for screen in ["", "\n\n", "\u{276f} Try \"how does <filepath> work?\"", "\u{2722} Osmosing\u{2026}"] {
        assert!(detect_prompt("claude", screen, true).is_none(), "{screen:?}");
    }
}

#[test]
fn another_wording_of_the_same_structure_still_works() {
    // The pointer on row two (the user moved it) and a tool Puppet Master has no box for.
    let screen = "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n Fetch\n https://example.com/readme\n\n Do you want to allow Claude to fetch this content?\n   1. Yes\n \u{276f} 2. Yes, and don't ask again for example.com\n   3. No, and tell Claude what to do differently (esc)\n\n Esc to cancel";
    let prompt = detect(screen, true);
    assert_eq!(prompt["kind"], "tool_approval");
    assert_eq!(prompt["summary"], "Do you want to allow Claude to fetch this content?");
    assert!(prompt.get("command").is_none());
    assert_eq!(prompt["menu"]["selected"], 2);
    assert_eq!(key(&prompt, "allow_once", false), UP);
    assert_eq!(key(&prompt, "deny", false), DOWN);
    assert_eq!(key(&prompt, "allow_all", true), "");
}

#[test]
fn the_older_allow_once_wording_still_goes_through_the_generic_keys() {
    let claude = profile_for("claude");
    let prompt = claude.detect_prompt("Always allow this command?", true).expect("broad prompt");
    assert_eq!(prompt["kind"], "broad_approval");
    assert_eq!(claude.prompt_reply(&prompt, "allow_all", true).unwrap().key.as_deref(), Some("a"));
    assert!(claude.prompt_reply(&prompt, "allow_all", false).is_err());
    let once = claude.detect_prompt("Allow once?\nDeny", false).expect("allow once");
    assert_eq!(claude.prompt_reply(&once, "allow_once", false).unwrap().key.as_deref(), Some("y"));
    assert_eq!(claude.prompt_reply(&once, "deny", false).unwrap().key.as_deref(), Some("n"));
}

#[test]
fn the_status_reads_the_trust_dialog_and_the_menus_as_waiting() {
    use crate::pty::status::classify_agent_observation;
    use crate::tui::PaneStatus;
    assert_eq!(classify_agent_observation("claude", TRUST, ""), PaneStatus::WaitingInput, "before: running, so the task was typed into it");
    assert_eq!(classify_agent_observation("claude", TRUST_OLD, ""), PaneStatus::WaitingInput);
    for screen in [BASH_MULTI_CD, BASH_FOUR_OPTIONS, EDIT_FILE, CREATE_FILE] {
        assert_eq!(classify_agent_observation("claude", screen, ""), PaneStatus::WaitingInput);
    }
    assert_eq!(classify_agent_observation("claude", "\u{273b} Brewed for 1s \u{b7} done 23:01\n\u{276f} \n", ""), PaneStatus::Running);
}

#[test]
fn a_crlf_checkout_of_the_captures_reads_the_same() {
    // The fixtures are plain text files: git may check them out with CRLF line ends.
    for screen in [BASH_MULTI_CD, BASH_FOUR_OPTIONS, EDIT_FILE, CREATE_FILE, TRUST] {
        let lf = detect(screen, true);
        let crlf = detect(&screen.replace('\n', "\r\n"), true);
        for field in ["kind", "command", "path", "folder", "menu", "choices", "identity"] {
            assert_eq!(lf.get(field), crlf.get(field), "{field}");
        }
    }
}
