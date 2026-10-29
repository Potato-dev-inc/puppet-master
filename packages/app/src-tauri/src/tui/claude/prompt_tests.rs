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

// ---- the sign-in menu (`/login`) ----

const LOGIN_MENU: &str = include_str!("fixtures/login_menu_2_1_312.txt");

#[test]
fn the_sign_in_menu_is_a_card_with_its_three_options_and_a_way_out() {
    let prompt = detect(LOGIN_MENU, false);
    assert_eq!(prompt["kind"], "question");
    assert_eq!(prompt["summary"], "Claude Code needs you to sign in");
    assert_eq!(prompt["note"], "Pick how you sign in. A browser window opens.");
    assert_eq!(prompt["choices"], json!(["1", "2", "3", "cancel"]));
    assert_eq!(prompt["labels"]["1"], "Claude account with subscription");
    assert_eq!(prompt["labels"]["2"], "Anthropic Console account");
    assert_eq!(prompt["labels"]["3"], "3rd-party platform");
    assert_eq!(prompt["menu"]["selected"], 1);
    assert_eq!(prompt["menu"]["options"].as_array().unwrap().len(), 3);
}

#[test]
fn picking_a_sign_in_option_moves_the_pointer_and_cancel_is_escape() {
    let prompt = detect(LOGIN_MENU, false);
    assert_eq!(key(&prompt, "1", false), "", "the pointer is already on option 1; the route adds Enter");
    assert_eq!(key(&prompt, "2", false), DOWN);
    assert_eq!(key(&prompt, "3", false), format!("{DOWN}{DOWN}"));
    assert_eq!(key(&prompt, "cancel", false), "\u{1b}");
    assert!(prompt_reply(&prompt, "4", false).is_err());
}

#[test]
fn the_sign_in_menu_keeps_its_pointer_row_when_the_person_moved_it() {
    let moved = LOGIN_MENU.replace("❯ 1.", "  1.").replace("  2. Anthropic", "❯ 2. Anthropic");
    let prompt = detect(&moved, false);
    assert_eq!(prompt["menu"]["selected"], 2);
    assert_eq!(key(&prompt, "1", false), UP);
    assert_eq!(key(&prompt, "3", false), DOWN);
}

#[test]
fn the_sign_in_menu_is_a_waiting_pane_but_only_while_it_is_the_last_thing_drawn() {
    assert_eq!(profile_for("claude").classify_screen(LOGIN_MENU), Some(crate::pty::status::PaneStatus::WaitingInput));
    // Anything drawn under it means it was answered or cancelled.
    let later = format!("{LOGIN_MENU}\nOpening your browser to sign in...\n");
    assert!(detect_prompt("claude", &later, false).is_none());
    assert_eq!(profile_for("claude").classify_screen(&later), None);
}

#[test]
fn text_that_only_looks_like_the_sign_in_menu_is_not_a_card() {
    // A reply that quotes the title and a numbered list, with no pointer and no footer.
    let quoted = "The CLI says: Select login method:\n1. Claude account\n2. Console account\nthen asks for a browser.";
    assert!(detect_prompt("claude", quoted, false).is_none());
    // Options that do not count 1, 2, 3.
    let skipped = LOGIN_MENU.replace("2. Anthropic", "5. Anthropic");
    assert!(detect_prompt("claude", &skipped, false).is_none());
}

#[test]
fn the_sign_in_card_leads_with_its_own_line_and_keeps_the_agents_words() {
    let card = crate::pane_prompt::detect_in_screen("claude", LOGIN_MENU, false).expect("a card");
    assert_eq!(card["risk"]["reasons"][0], "Pick how you sign in. A browser window opens.");
    let ui = crate::pane_prompt::PanePrompt::from_detected(&card).expect("a prompt for the UI");
    assert_eq!(ui.summary.as_deref(), Some("Claude Code needs you to sign in"));
    let labels: Vec<&str> = ui.choices.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["Claude account with subscription", "Anthropic Console account", "3rd-party platform", "Cancel"]);
    assert!(ui.choices.iter().all(|c| !c.broad), "no sign-in answer is a broad approval");
}

// ---- the first-run screens (real captures of v2.1.312, a fresh config folder) ----

const FIRST_THEME: &str = include_str!("fixtures/first_run_theme_2_1_312.txt");
const FIRST_LOGIN: &str = include_str!("fixtures/first_run_login_method_2_1_312.txt");
const FIRST_API_KEY: &str = include_str!("fixtures/first_run_api_key_2_1_312.txt");
const FIRST_NOTES: &str = include_str!("fixtures/first_run_security_notes_2_1_312.txt");
const FIRST_IDLE: &str = include_str!("fixtures/first_run_idle_box_2_1_312.txt");
const READY_FRESH: &str = include_str!("fixtures/ready_fresh_2_1_299_live.txt");

fn waiting(screen: &str) -> bool {
    profile_for("claude").classify_screen(screen) == Some(crate::pty::status::PaneStatus::WaitingInput)
}

#[test]
fn the_theme_picker_is_a_waiting_pane_with_a_card_of_seven_rows() {
    assert!(waiting(FIRST_THEME));
    let prompt = detect(FIRST_THEME, false);
    assert_eq!(prompt["kind"], "question");
    assert_eq!(prompt["summary"], "Claude Code: pick a text style");
    assert_eq!(prompt["choices"], json!(["1", "2", "3", "4", "5", "6", "7"]));
    assert_eq!(prompt["labels"]["1"], "Auto (match terminal)");
    assert_eq!(prompt["labels"]["2"], "Dark mode", "the check mark is not part of the name");
    assert_eq!(prompt["labels"]["7"], "Light mode (ANSI colors only)");
    assert_eq!(prompt["menu"]["selected"], 2);
    assert_eq!(prompt["menu"]["options"].as_array().unwrap().len(), 7);
    // The card says what the screen asks, not the code preview under the menu.
    let text = prompt["text"].as_str().unwrap();
    assert!(text.starts_with("Choose the text style"));
    assert!(!text.contains("function greet"));
}

#[test]
fn picking_a_theme_moves_the_pointer_from_dark_mode() {
    let prompt = detect(FIRST_THEME, false);
    assert_eq!(key(&prompt, "2", false), "", "the pointer is already on Dark mode; the route adds Enter");
    assert_eq!(key(&prompt, "1", false), UP);
    assert_eq!(key(&prompt, "4", false), format!("{DOWN}{DOWN}"));
    assert!(prompt_reply(&prompt, "8", false).is_err());
    assert!(prompt_reply(&prompt, "cancel", false).is_err(), "the picker has no way out");
}

#[test]
fn the_theme_picker_keeps_one_id_when_the_pointer_moves() {
    // Normalized first, so a CRLF checkout of the fixture reads the same as an LF one.
    let theme = FIRST_THEME.replace("\r\n", "\n");
    let moved = theme.replace("❯ ✔ Dark mode", "  ✔ Dark mode").replace("     Light mode\n", " ❯   Light mode\n");
    assert_ne!(moved, theme);
    let (a, b) = (detect(&theme, false), detect(&moved, false));
    assert_eq!(b["menu"]["selected"], 3);
    assert_eq!(a["identity"], b["identity"]);
    assert_eq!(b["labels"]["2"], "Dark mode");
}

#[test]
fn the_theme_picker_goes_away_when_the_next_screen_is_drawn() {
    // The picker's words with the login menu drawn under them: no rule and no preview, so it is not the end of the screen.
    let later = format!("{FIRST_THEME}\n\n{}\n", "Opening the next screen...\n".repeat(16));
    assert!(detect_prompt("claude", &later, false).is_none());
    assert!(!waiting(&later));
}

#[test]
fn the_sign_in_menu_of_a_first_run_has_no_title_and_is_still_a_card() {
    assert!(waiting(FIRST_LOGIN));
    let prompt = detect(FIRST_LOGIN, false);
    assert_eq!(prompt["summary"], "Claude Code needs you to sign in");
    assert_eq!(prompt["choices"], json!(["1", "2", "3", "cancel"]));
    assert_eq!(prompt["labels"]["3"], "3rd-party platform");
    assert_eq!(key(&prompt, "2", false), DOWN);
}

#[test]
fn the_api_key_question_is_a_card_with_yes_and_no_and_hides_the_key() {
    assert!(waiting(FIRST_API_KEY));
    let prompt = detect(FIRST_API_KEY, false);
    assert_eq!(prompt["kind"], "question");
    assert_eq!(prompt["summary"], "Claude Code found an API key in your environment");
    assert_eq!(prompt["choices"], json!(["1", "2", "cancel"]));
    assert_eq!(prompt["labels"]["1"], "Yes");
    assert_eq!(prompt["labels"]["2"], "No (recommended)");
    assert_eq!(prompt["menu"]["selected"], 2);
    assert!(!prompt["text"].as_str().unwrap().contains("sk-ant"), "the key stays out of the card");
    assert_eq!(key(&prompt, "1", false), UP);
    assert_eq!(key(&prompt, "2", false), "");
    assert_eq!(key(&prompt, "cancel", false), "\u{1b}");
}

#[test]
fn the_security_notes_are_a_continue_card() {
    assert!(waiting(FIRST_NOTES));
    let prompt = detect(FIRST_NOTES, false);
    assert_eq!(prompt["kind"], "question");
    assert_eq!(prompt["summary"], "Claude Code shows its security notes");
    assert_eq!(prompt["choices"], json!(["continue"]));
    assert_eq!(prompt["labels"]["continue"], "Continue");
    assert!(prompt["text"].as_str().unwrap().starts_with("Security notes"));
    assert_eq!(key(&prompt, "continue", false), "", "the route adds the Enter");
    let ui = crate::pane_prompt::detect_in_screen("claude", FIRST_NOTES, false).and_then(|card| crate::pane_prompt::PanePrompt::from_detected(&card)).expect("a prompt for the UI");
    assert_eq!(ui.choices.len(), 1);
    assert!(ui.choices.iter().all(|c| !c.broad));
}

#[test]
fn any_press_enter_to_continue_screen_is_caught_by_its_footer() {
    let signed_in = "Login successful. Press Enter to continue…";
    assert!(waiting(signed_in));
    let prompt = detect(signed_in, false);
    assert_eq!(prompt["summary"], "Claude Code signed you in");
    assert_eq!(prompt["choices"], json!(["continue"]));
    // Said in the middle of a reply, with the input box under it, it is not a screen to continue.
    let quoted = format!("The wizard says: Press Enter to continue…\n\n{READY_FRESH}");
    assert!(detect_prompt("claude", &quoted, false).is_none());
    assert!(!waiting(&quoted));
}

#[test]
fn the_idle_input_box_is_not_a_first_run_screen() {
    for screen in [FIRST_IDLE, READY_FRESH] {
        assert!(!waiting(screen), "{screen}");
        assert!(detect_prompt("claude", screen, false).is_none());
    }
}

#[test]
fn the_screens_that_look_a_little_like_a_first_run_screen_are_not_cards() {
    // A reply that talks about themes, with no menu under it.
    let talk = "Choose the text style that looks best with your terminal is what the CLI asks on a first run.\nDark mode\nLight mode\n";
    assert!(detect_prompt("claude", talk, false).is_none());
    // The picker's words and a list with no pointer.
    let no_pointer = FIRST_THEME.replace("❯ ✔ Dark mode", "  ✔ Dark mode");
    assert!(detect_prompt("claude", &no_pointer, false).is_none());
}

#[test]
fn every_first_run_screen_reaches_the_card_through_the_shared_detector() {
    // `pane_prompt::detect_in_screen` hands the profile only the last rows (a window around an anchor phrase).
    for (screen, summary) in [
        (FIRST_THEME, "Claude Code: pick a text style"),
        (FIRST_LOGIN, "Claude Code needs you to sign in"),
        (FIRST_API_KEY, "Claude Code found an API key in your environment"),
        (FIRST_NOTES, "Claude Code shows its security notes"),
    ] {
        let card = crate::pane_prompt::detect_in_screen("claude", screen, false).unwrap_or_else(|| panic!("no card for {summary}"));
        let ui = crate::pane_prompt::PanePrompt::from_detected(&card).expect("a prompt for the UI");
        assert_eq!(ui.summary.as_deref(), Some(summary));
        assert!(!ui.choices.is_empty());
        assert!(ui.choices.iter().all(|c| !c.broad));
    }
    // A narrow pane wraps the preview into more rows; the picker is still found.
    let narrow = FIRST_THEME.replace("\r\n", "\n").replace("Hello, World!", "Hello, World! Hello, World! Hello, World!\n  Hello, World! Hello").repeat(1);
    assert!(crate::pane_prompt::detect_in_screen("claude", &narrow, false).is_some());
}
