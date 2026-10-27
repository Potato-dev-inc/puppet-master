//! Codex's folder-trust dialog as an answerable card (Trust / Don't trust), like Claude's and
//! Cursor's. Without it the dialog was only a startup gate (`pane_prompt::gate_prompt`): a card with
//! no choices, so the person had to open the terminal to answer it (live r15, Codex 0.162.1).
//!
//! Three wordings are read:
//! - 0.162+: `Trust this folder? ... › 1. Trust and continue  2. Quit`, footer `enter continue · esc quit`
//!   (real capture `fixtures/trust_folder_live.txt`, `trust_folder_glued_live.txt`)
//! - 0.162+ started from the Agent Command Center: `2. Back to Agent Command Center`, footer
//!   `enter continue · esc back` (`fixtures/trust_folder_back_live.txt`; with this wording the card had no
//!   choices at all, live 2026-10-27)
//! - older: `Do you trust the files in this folder? › 1. Yes, continue  2. No, quit`, footer
//!   `Press enter to continue`
//!
//! Answering moves the `›` pointer with arrow keys; `write_input` then adds the Enter. A number key
//! would select and submit on its own, and the Enter after it could answer the next screen.
//! Puppet Master never answers this dialog by itself: only a person's choice on the card does.

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{json, Value};

use crate::tui::{generic, Reply};

/// Same kind as Claude's and Cursor's: generic code treats it as a startup gate.
const KIND_TRUST: &str = "workspace_trust";
const POINTER: char = '\u{203a}';
const ARROW_DOWN: &str = "\u{1b}[B";
const ARROW_UP: &str = "\u{1b}[A";
/// Only the end of the screen is read: a dialog is always the last thing drawn.
const SCAN_CHARS: usize = 8_000;
/// What a line under the menu starts with when it is Codex's key hints (`enter continue · esc quit`,
/// `enter continue · esc back`, `press enter to continue`). The wording of the second half changes with
/// how the dialog was reached (quit, back, cancel, skip), so the hints are told by their keys.
const FOOTER_STARTS: [&str; 5] = ["enter ", "esc ", "press enter", "press esc", "ctrl+"];

/// A numbered row: `1. Trust and continue`. Rows can be glued onto one line (Codex fills a row to the
/// last column and the emulator joins it to the next), so rows are found by their markers.
static OPTION: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?:^|\s)([1-9])\.\s+").expect("option regex"));

#[derive(Debug)]
struct Opt {
    n: usize,
    label: String,
    choice: Option<&'static str>,
    pointed: bool,
}

fn tail(text: &str) -> &str {
    if text.len() <= SCAN_CHARS {
        return text;
    }
    let mut cut = text.len() - SCAN_CHARS;
    while !text.is_char_boundary(cut) {
        cut += 1;
    }
    &text[cut..]
}

/// A trimmed, lowercased row of key hints under the menu.
fn is_footer(row: &str) -> bool {
    FOOTER_STARTS.iter().any(|start| row.starts_with(start))
}

/// `trust` for the row that trusts the folder, `deny` for the one that leaves without trusting it
/// (`Quit`, `No, quit`, `Back to Agent Command Center`). Other rows (`Open restricted`, ...) are no answer
/// the card offers.
fn classify(label: &str) -> Option<&'static str> {
    let lower = label.to_ascii_lowercase();
    if lower.starts_with("yes") || lower.starts_with("trust") {
        Some("trust")
    } else if matches!(
        lower.split_whitespace().next().map(|word| word.trim_end_matches([',', '.'])),
        Some("no" | "quit" | "exit" | "back" | "cancel" | "don't")
    ) {
        Some("deny")
    } else {
        None
    }
}

/// A row's label: up to the end of its line, or to a run of 3+ spaces (padding before a glued row).
fn label_at(text: &str) -> &str {
    let line = text.lines().next().unwrap_or_default();
    let end = line.find("   ").unwrap_or(line.len());
    line[..end].trim()
}

/// The folder the dialog names: the row under `Folder access` (0.162+) or the end of
/// `You are running Codex in <dir>` (older).
fn folder(text: &str, lower: &str) -> Option<String> {
    let after_heading = lower.find("folder access").and_then(|at| {
        text[at + "folder access".len()..].lines().map(str::trim).find(|row| !row.is_empty()).map(str::to_string)
    });
    let running_in = lower.rfind("you are running codex in ").map(|at| {
        let from = at + "you are running codex in ".len();
        text[from..].lines().next().unwrap_or_default().trim().to_string()
    });
    after_heading.or(running_in).filter(|row| !row.is_empty() && row.len() <= 260)
}

/// The folder-trust dialog on screen, as a prompt with `trust` / `deny` choices and the menu the
/// reply moves through. `None` when it is not (or no longer) the last thing drawn.
pub(super) fn trust_prompt(agent_type: &str, screen: &str) -> Option<Value> {
    let text = tail(screen);
    let lower = text.to_ascii_lowercase();
    let question_at = super::screen::TRUST_PHRASES.iter().filter_map(|phrase| lower.rfind(phrase)).min()?;
    let mut options: Vec<Opt> = Vec::new();
    let mut last_end = question_at;
    for found in OPTION.captures_iter(&text[question_at..]) {
        let (Some(whole), Some(number)) = (found.get(0), found.get(1)) else { continue };
        let n: usize = number.as_str().parse().ok()?;
        if n != options.len() + 1 {
            break;
        }
        let marker_at = question_at + whole.start();
        let label_from = question_at + whole.end();
        let label = label_at(&text[label_from..]);
        let before = text[..marker_at].trim_end_matches([' ', '\t']);
        let pointed = before.ends_with(POINTER) || before.ends_with('>');
        last_end = label_from + label.len();
        options.push(Opt { n, label: label.to_string(), choice: classify(label), pointed });
    }
    if !options.iter().any(|o| o.choice == Some("trust")) {
        return None;
    }
    // A live dialog ends with its footer; once answered, Codex draws its own screen over it.
    if !lower[last_end..].lines().map(str::trim).filter(|row| !row.is_empty()).all(is_footer) {
        return None;
    }
    let selected = options.iter().find(|o| o.pointed).map(|o| o.n).unwrap_or(1);
    let mut choices = vec!["trust".to_string()];
    if options.iter().any(|o| o.choice == Some("deny")) {
        choices.push("deny".to_string());
    }
    let body: Vec<&str> = text[question_at..last_end].lines().map(str::trim).filter(|row| !row.is_empty()).collect();
    let mut prompt = generic::prompt_json(agent_type, KIND_TRUST, &body.join("\n"), choices);
    prompt["menu"] = json!({
        "selected": selected,
        "options": options.iter().map(|o| json!({"n": o.n, "label": o.label, "choice": o.choice})).collect::<Vec<_>>(),
    });
    // One trust dialog per pane at a time: its text must not decide its id (it re-wraps).
    prompt["identity"] = Value::String(KIND_TRUST.into());
    let folder = folder(text, &lower);
    prompt["summary"] = Value::String(match &folder {
        Some(folder) => format!("Codex asks to trust {folder}"),
        None => "Codex asks to trust this folder".into(),
    });
    if let Some(folder) = folder {
        prompt["folder"] = Value::String(folder);
    }
    prompt["labels"] = json!({"trust": "Trust this folder", "deny": "Don't trust"});
    Some(prompt)
}

/// Answer a prompt read by `trust_prompt`: the arrow presses that put the pointer on the chosen
/// row (the caller's Enter selects it). Any other prompt gets the generic answer.
pub(super) fn prompt_reply(prompt: &Value, choice: &str, allow_broad: bool) -> Result<Reply, String> {
    let menu = match prompt.get("menu") {
        Some(menu) if prompt.get("kind").and_then(Value::as_str) == Some(KIND_TRUST) => menu,
        _ => return generic::prompt_reply(prompt, choice, allow_broad),
    };
    let options = menu.get("options").and_then(Value::as_array).cloned().unwrap_or_default();
    let target = options
        .iter()
        .find(|o| o.get("choice").and_then(Value::as_str) == Some(choice))
        .and_then(|o| o.get("n").and_then(Value::as_u64))
        .ok_or_else(|| "unsupported approval choice".to_string())? as usize;
    let from = menu.get("selected").and_then(Value::as_u64).unwrap_or(1) as usize;
    let keys = if target >= from { ARROW_DOWN.repeat(target - from) } else { ARROW_UP.repeat(from - target) };
    Ok(Reply::key(&keys))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIVE_TRUST_FOLDER: &str = include_str!("fixtures/trust_folder_live.txt");
    const OLD_TRUST: &str = "> You are running Codex in ~/sandbox\n\n  Do you trust the files in this folder?\n\n\u{203a} 1. Yes, continue\n  2. No, quit\n\n  Press enter to continue\n";

    #[test]
    fn the_live_trust_dialog_is_a_card_with_trust_and_dont_trust() {
        let prompt = trust_prompt("codex", LIVE_TRUST_FOLDER).expect("the 0.162 trust dialog is read");
        assert_eq!(prompt["kind"], "workspace_trust");
        assert_eq!(prompt["choices"], json!(["trust", "deny"]));
        assert_eq!(prompt["menu"]["selected"], 1);
        assert_eq!(prompt["menu"]["options"][0]["label"], "Trust and continue");
        assert_eq!(prompt["menu"]["options"][1]["label"], "Quit");
        assert_eq!(prompt["summary"], "Codex asks to trust D:\\pm-hunt10\\a");
        assert_eq!(prompt["labels"]["deny"], "Don't trust");
        // The pointer is on "Trust and continue": Enter alone trusts; one Down then Enter quits.
        assert_eq!(prompt_reply(&prompt, "trust", false).unwrap(), Reply::key(""));
        assert_eq!(prompt_reply(&prompt, "deny", false).unwrap(), Reply::key(ARROW_DOWN));
        assert!(prompt_reply(&prompt, "allow_all", true).is_err());
    }

    /// A capture of the standard 0.162.1 dialog as the pane buffer returns it: the question's rows run
    /// together (`Folder settings       can run code`) and every row is padded to the pane width.
    const GLUED_LIVE: &str = include_str!("fixtures/trust_folder_glued_live.txt");
    /// The same dialog when Codex was started from its Agent Command Center (0.162.1, read off a live
    /// pane): row 2 is `Back to Agent Command Center` and the footer says `esc back`, not `Quit` / `esc quit`.
    const BACK_LIVE: &str = include_str!("fixtures/trust_folder_back_live.txt");

    fn lf(text: &str) -> String {
        text.replace("\r\n", "\n")
    }

    #[test]
    fn the_back_wording_is_a_card_with_trust_and_dont_trust() {
        // Live (chat-tab verify, 2026-10-27): the header said "Needs you · Trust this folder" but the pane drew
        // no card, because `esc back` was not a footer and `Back to ...` was not a way out: no choices.
        let prompt = trust_prompt("codex", &lf(BACK_LIVE)).expect("the back wording is read");
        assert_eq!(prompt["kind"], "workspace_trust");
        assert_eq!(prompt["choices"], json!(["trust", "deny"]));
        assert_eq!(prompt["menu"]["options"][0]["label"], "Trust and continue");
        assert_eq!(prompt["menu"]["options"][1]["label"], "Back to Agent Command Center");
        assert_eq!(prompt["menu"]["options"][1]["choice"], "deny");
        assert_eq!(prompt["summary"], "Codex asks to trust C:\\Users\\you\\repo-1");
        assert_eq!(prompt_reply(&prompt, "trust", false).unwrap(), Reply::key(""));
        assert_eq!(prompt_reply(&prompt, "deny", false).unwrap(), Reply::key(ARROW_DOWN));
    }

    #[test]
    fn the_glued_live_capture_is_a_card_too() {
        let prompt = trust_prompt("codex", &lf(GLUED_LIVE)).expect("the buffer as captured");
        assert_eq!(prompt["choices"], json!(["trust", "deny"]));
        assert_eq!(prompt["menu"]["options"][1]["label"], "Quit");
    }

    #[test]
    fn whatever_footer_codex_hangs_under_the_menu_the_dialog_is_still_up() {
        for footer in ["enter continue \u{b7} esc back", "enter continue \u{b7} esc cancel", "enter select \u{b7} esc skip", "press enter to continue"] {
            let screen = lf(BACK_LIVE).replace("enter continue \u{b7} esc back", footer);
            assert!(trust_prompt("codex", &screen).is_some(), "footer {footer:?}");
        }
        // Text that is not a key hint under the menu means Codex drew something else over it.
        let drawn_over = format!("{}\n\u{2022} Working (3s)\n", lf(BACK_LIVE).trim_end());
        assert!(trust_prompt("codex", &drawn_over).is_none());
    }

    #[test]
    fn a_card_from_the_screen_reaches_the_dock_with_its_choices() {
        // Through `pane_prompt`, the way the cockpit reads it: the profile's card, not the choice-less startup gate.
        let card = crate::pane_prompt::detect_in_screen("codex", &lf(BACK_LIVE), false).expect("a card");
        assert_eq!(card["choices"], json!(["trust", "deny"]));
        assert!(card.get("startup_gate").is_none(), "{card}");
    }

    #[test]
    fn the_older_wording_and_a_glued_menu_are_read_too() {
        let prompt = trust_prompt("codex", OLD_TRUST).expect("older trust dialog");
        assert_eq!(prompt["choices"], json!(["trust", "deny"]));
        assert_eq!(prompt["summary"], "Codex asks to trust ~/sandbox");
        // The pointer on row 2: trusting moves up.
        let on_quit = OLD_TRUST.replace("\u{203a} 1. Yes", "  1. Yes").replace("  2. No", "\u{203a} 2. No");
        let prompt = trust_prompt("codex", &on_quit).expect("pointer on quit");
        assert_eq!(prompt_reply(&prompt, "trust", false).unwrap(), Reply::key(ARROW_UP));
        let glued = "  Trust this folder? Continue only if you trust these files.\n\u{203a} 1. Trust and continue        2. Quit\n  enter continue \u{b7} esc quit\n";
        let prompt = trust_prompt("codex", glued).expect("glued rows");
        assert_eq!(prompt["choices"], json!(["trust", "deny"]));
    }

    #[test]
    fn an_answered_dialog_and_other_screens_are_not_trust_cards() {
        // Codex drew its composer under the old dialog: the dialog is gone.
        let answered = format!("{LIVE_TRUST_FOLDER}\n\u{203a} Ask Codex to do anything\n");
        assert!(trust_prompt("codex", &answered).is_none());
        let approval = "  Would you like to run the following command?\n\n    $ cargo test\n\n\u{203a} 1. Yes, proceed (y)\n  2. No, and tell Codex what to do differently (esc)\n";
        assert!(trust_prompt("codex", approval).is_none());
        assert!(trust_prompt("codex", "").is_none());
    }
}
