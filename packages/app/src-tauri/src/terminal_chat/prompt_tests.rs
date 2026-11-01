//! The card for a terminal chat's question: its shape, its stale id, and the rules that keep it a person's call.
//! The round trip through a real pane (the fake agent) is `tests/fake_tui_terminal_chat_prompt.rs`.

use super::*;
use serde_json::Value;
use std::path::PathBuf;

const CLAUDE_BASH: &str = include_str!("../tui/claude/fixtures/bash_multi_cd_2_1_299.txt");
const CLAUDE_TRUST: &str = include_str!("../tui/claude/fixtures/trust_dialog_2_1_299.txt");
const CLAUDE_EDIT: &str = include_str!("../tui/claude/fixtures/edit_file_2_1_299.txt");

fn reading(agent: &str, screen: &str, generation: u64) -> Option<Reading> {
    let detected: Value = crate::pane_prompt::detect_in_screen(agent, screen, false)?;
    reading_from(&detected, generation)
}

fn card(screen: &str, generation: u64) -> PromptView {
    reading("claude", screen, generation).expect("a card").view
}

#[test]
fn a_permission_dialog_becomes_a_card_with_the_agents_own_option_names() {
    let view = card(CLAUDE_BASH, 3);
    assert_eq!(view.kind, "permission");
    assert_eq!(view.raw_kind, "command_approval");
    assert_eq!(
        view.options,
        vec![
            PromptOption { id: "allow_once".into(), name: "Allow once".into() },
            PromptOption { id: "deny".into(), name: "Deny".into() },
        ]
    );
    assert!(view.prompt_id.starts_with("tcp-"), "{}", view.prompt_id);
    assert!(view.risk.is_some());
    let wire = serde_json::to_value(&view).unwrap();
    assert!(wire.get("promptId").is_some() && wire.get("options").is_some() && wire.get("kind").is_some(), "{wire}");
    for key in wire.as_object().unwrap().keys() {
        assert!(!key.contains("default") && !key.contains("recommend") && !key.contains("selected"), "no option is pre-picked: {key}");
    }
}

#[test]
fn a_folder_trust_dialog_is_a_trust_card_and_never_a_default() {
    let view = card(CLAUDE_TRUST, 1);
    assert_eq!(view.kind, "trust");
    assert_eq!(view.options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["trust", "deny"]);
    assert_eq!(view.options[0].name, "Trust this folder");
}

#[test]
fn nothing_recognisable_is_no_card() {
    assert!(reading("claude", "just some output\nand more output\n", 0).is_none());
    assert!(reading("shell", CLAUDE_BASH, 0).is_none(), "a shell pane has no prompt parser");
}

/// The id follows the question and the occurrence: the same question at the same occurrence keeps its id (a redraw
/// or a different width is not a new question), another question or a new occurrence does not.
#[test]
fn the_id_changes_with_the_question_and_with_each_new_occurrence() {
    let first = card(CLAUDE_BASH, 5).prompt_id;
    assert_eq!(card(CLAUDE_BASH, 5).prompt_id, first, "same screen, same id");
    assert_eq!(card(&format!("older output\n{CLAUDE_BASH}"), 5).prompt_id, first, "scrollback above does not matter");
    assert_ne!(card(CLAUDE_BASH, 6).prompt_id, first, "asked again");
    assert_ne!(card(CLAUDE_EDIT, 5).prompt_id, first, "another question");
}

#[test]
fn a_changed_or_gone_screen_makes_the_card_stale_and_nothing_is_sent() {
    let view = card(CLAUDE_BASH, 2);
    assert!(check(Some(&view), &view.prompt_id, "allow_once").is_ok());
    let newer = card(CLAUDE_BASH, 3);
    let stale = check(Some(&newer), &view.prompt_id, "allow_once").unwrap_err();
    assert_eq!(stale.code, STALE);
    assert!(stale.wire().starts_with("TERMINAL_CHAT_PROMPT_STALE: "), "{}", stale.wire());
    assert_eq!(check(None, &view.prompt_id, "allow_once").unwrap_err().code, STALE);
}

#[test]
fn an_option_the_card_did_not_list_is_refused() {
    let view = card(CLAUDE_BASH, 2);
    for wrong in ["allow_all", "always", "", "ALLOW_ONCE", "trust"] {
        assert_eq!(check(Some(&view), &view.prompt_id, wrong).unwrap_err().code, UNKNOWN_OPTION, "{wrong}");
    }
}

// ---- Guards: only a person's call answers; generic code names no agent. ----

fn src(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src").join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())).replace("\r\n", "\n")
}

fn walk(dir: &str, out: &mut Vec<String>) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src").join(dir);
    for entry in std::fs::read_dir(&root).unwrap_or_else(|e| panic!("{}: {e}", root.display())) {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if path.is_dir() {
            walk(&format!("{dir}/{name}"), out);
        } else if name.ends_with(".rs") {
            out.push(format!("{dir}/{name}"));
        }
    }
}

fn shipped(source: &str) -> String {
    let cut = source.find("#[cfg(test)]").unwrap_or(source.len());
    source[..cut]
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The two commands are Tauri commands only: no route table, no MCP catalog, no shim, no phone path, and no other
/// module (a timer, a rule, a hook) calls them or the functions behind them.
#[test]
fn the_commands_are_not_reachable_from_the_bridge_mcp_or_any_other_module() {
    let names = ["terminal_chat_prompt", "terminal_chat_answer", "terminal_chat::prompt"];
    let allowed = [
        "terminal_chat/prompt.rs",
        "terminal_chat/prompt_tests.rs",
        "commands/terminal_chat_prompt.rs",
        "commands/mod.rs",
        "lib.rs",
        "fake_tui/world.rs",
    ];
    let mut files = Vec::new();
    walk(".", &mut files);
    for file in files {
        let relative = file.trim_start_matches("./");
        if allowed.contains(&relative) {
            continue;
        }
        let code = src(relative);
        for name in names {
            assert!(!code.contains(name), "{relative} reaches `{name}`: a terminal chat question is answered from the desktop command only");
        }
    }
    // The bridge's own route and tool tables, named, so a change to the walk above cannot hide a hole.
    for table in ["bridge/router.rs", "bridge/routes/mod.rs", "bridge/routes/chat.rs", "bridge/routes/panes.rs", "tool_registry/mod.rs", "mcp_shim/dispatch/mod.rs"] {
        let code = src(table);
        assert!(!code.contains("terminal_chat"), "{table} names the terminal chat prompt");
    }
    // The command file holds exactly the two commands, and lib.rs registers them once each.
    let commands = src("commands/terminal_chat_prompt.rs");
    assert_eq!(commands.matches("#[tauri::command]").count(), 2);
    let lib = src("lib.rs");
    assert_eq!(lib.matches("commands::terminal_chat_prompt,").count(), 1);
    assert_eq!(lib.matches("commands::terminal_chat_answer,").count(), 1);
}

/// No timer, no default pick, no auto-answer in the code that answers; the click is required before anything is read.
#[test]
fn the_answer_path_has_no_timer_no_default_and_requires_the_click() {
    let code = shipped(&src("terminal_chat/prompt.rs"));
    for word in ["sleep", "Duration", "Instant", "spawn", "interval", "timeout", "tokio::time", "first()", "[0]", "allow_broad: true"] {
        assert!(!code.contains(word), "prompt.rs must not contain `{word}`");
    }
    let answer = &code[code.find("pub fn answer(").expect("answer fn")..];
    let gesture_check = answer.find("let Some(gesture) = gesture else").expect("the click is required");
    let first_read = answer.find("read(registry").expect("re-reads the screen");
    let first_send = answer.find("answer_for_pane").expect("sends through the shared route");
    assert!(gesture_check < first_read && first_read < first_send, "click, then re-read, then send");
    assert!(answer.contains("Some(gesture)"), "the click travels to the route that checks it is real");
    assert_eq!(code.matches("answer_for_pane(").count(), 1, "one way to press a key");
    // The commands pass the caller's gesture through untouched.
    let commands = shipped(&src("commands/terminal_chat_prompt.rs"));
    assert!(commands.contains("gesture.as_ref()") && !commands.contains("UserGesture {"), "a command must not make its own click");
}

/// What differs between agents lives in `tui/<agent>/`: the generic files here name none.
#[test]
fn generic_files_name_no_agent() {
    let names = ["codex", "claude", "gemini", "cursor", "opencode", "kilo", "copilot", "nuphos", "openab", "nimbus"];
    for file in ["terminal_chat/mod.rs", "terminal_chat/prompt.rs", "commands/terminal_chat_prompt.rs"] {
        let code = shipped(&src(file)).to_lowercase();
        for name in names {
            assert!(!code.contains(name), "{file} names an agent ({name}): agent specifics belong in tui/<agent>/");
        }
    }
}
