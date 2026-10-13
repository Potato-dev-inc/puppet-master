#[test]
fn a_command_approval_keeps_its_id_through_redraws_and_scrolling() {
    // Real capture (2026-10-04): the answer route used to hash the whole screen and so never
    // matched the id it had handed out.
    let screen = include_str!("../tui/cursor/fixtures/command_approval_visible.txt");
    let first = super::detect_in_screen("cursor_agent", screen, true).expect("prompt");
    assert_eq!(first["kind"], "command_approval");
    assert!(first["command"]
        .as_str()
        .unwrap()
        .contains("python todo.py add buy-milk"));
    let id = first["prompt_id"].as_str().unwrap().to_string();
    // The same dialog with more scrollback above it, a spinner tick, and without allow_broad.
    let scrolled = format!(
        "old output line
another line
{screen}"
    );
    let again = super::detect_in_screen("cursor_agent", &scrolled, false).expect("prompt");
    assert_eq!(again["prompt_id"].as_str(), Some(id.as_str()));
    // A different command is a different prompt.
    let other = screen.replace("add buy-milk", "rm buy-milk");
    let changed = super::detect_in_screen("cursor_agent", &other, true).expect("prompt");
    assert_ne!(changed["prompt_id"].as_str(), Some(id.as_str()));
}

use super::*;

const TRUST_PENDING: &str =
    include_str!("../tui/cursor/fixtures/workspace_trust_visible_pending.txt");
const TRUST_REWRAPPED: &str = "  Cursor Agent\n  v2026.10.01-e373342\n\n  \u{256d}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256e}\n  \u{2502} \u{26a0} Workspace Trust \u{2502}\n  \u{2502} Required \u{2502}\n  \u{2502} Cursor Agent can \u{2502}\n  \u{2502} execute code and \u{2502}\n  \u{2502} access files in \u{2502}\n  \u{2502} this directory. \u{2502}\n  \u{2502} Do you trust the \u{2502}\n  \u{2502} contents of this \u{2502}\n  \u{2502} directory? \u{2502}\n  \u{2502} C:\\Users\\Ren-pc\\AppData\\ \u{2502}\n  \u{2502} Local\\Temp\\cur-_b1a_ip3 \u{2502}\n  \u{2502} [a] Trust this \u{2502}\n  \u{2502} workspace \u{2502}\n  \u{2502} [q] Quit \u{2502}\n  \u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256f}\n";

#[test]
fn a_trust_prompt_keeps_its_id_when_the_dialog_is_rewrapped() {
    // Live (2026-10-04): the dialog re-rendered with different wrapping ~45s later and the
    // id handed out earlier hashed the screen text, so answer_prompt said "no longer current".
    let first = detect_in_screen("cursor_agent", TRUST_PENDING, true).expect("trust prompt");
    let again = detect_in_screen("cursor_agent", TRUST_REWRAPPED, false).expect("trust prompt");
    assert_eq!(first["kind"], "workspace_trust");
    assert_eq!(again["kind"], "workspace_trust");
    assert_eq!(first["prompt_id"], again["prompt_id"]);
}

#[test]
fn a_task_is_never_typed_over_a_dialog() {
    for dialog in [TRUST_PENDING, TRUST_REWRAPPED, APPROVAL] {
        let blocker = dispatch_blocker("cursor_agent", dialog).expect("blocked");
        assert_eq!(blocker.code, "PANE_BLOCKED_BY_PROMPT");
    }
    assert!(dispatch_blocker(
        "cursor_agent",
        "  Cursor Agent\n  \u{2192} Plan, search, build anything\n  Composer 2.5\n"
    )
    .is_none());
}

#[test]
fn prompts_without_a_command_ignore_wrapping_and_box_art() {
    let a = detect_in_screen("claude", "Always allow this command in the repo?\n", true).unwrap();
    let b = detect_in_screen(
        "claude",
        "  Always allow this\n  command in the repo?\n",
        true,
    )
    .unwrap();
    assert_eq!(a["prompt_id"], b["prompt_id"]);
}

const APPROVAL: &str = "\
 $ cargo test --lib\n\n Run this command?\n Run once (y)\n Skip (esc or n)\n\n → Add follow-up\n";

#[test]
fn cursor_approval_becomes_a_structured_prompt_with_a_stable_id() {
    let prompt = detect_in_screen("cursor_agent", APPROVAL, false).unwrap();
    assert_eq!(prompt["kind"], "command_approval");
    assert_eq!(prompt["choices"], json!(["allow_once", "deny"]));
    let again = detect_in_screen("cursor_agent", APPROVAL, false).unwrap();
    assert_eq!(prompt["prompt_id"], again["prompt_id"]);
    let other = APPROVAL.replace("cargo test --lib", "rm -rf build");
    let changed = detect_in_screen("cursor_agent", &other, false).unwrap();
    assert_ne!(
        prompt["prompt_id"], changed["prompt_id"],
        "a different command is a different prompt"
    );
}

#[test]
fn output_far_above_the_prompt_does_not_change_its_id() {
    let dialog = "step one
step two
 $ cargo test --lib

 Run this command?
 Run once (y)
 Skip (esc or n)
";
    let short = format!(
        "earlier output
{dialog}"
    );
    let long = format!(
        "{}{dialog}",
        "earlier output
"
        .repeat(200)
    );
    let a = detect_in_screen("cursor_agent", &short, false).unwrap();
    let b = detect_in_screen("cursor_agent", &long, false).unwrap();
    assert_eq!(a["prompt_id"], b["prompt_id"]);
}

const CD_SCREEN: &str = "\
 $ cd \"C:\\x\" && python todo.py add \"buy milk\"\n\n Run this command?\n Run once (y)\n Skip (esc or n)\n\n → Add follow-up\n";

#[test]
fn prompt_names_the_command_cwd_and_summary() {
    let prompt = detect_in_screen("cursor_agent", CD_SCREEN, false).unwrap();
    assert_eq!(
        prompt["command"],
        "cd \"C:\\x\" && python todo.py add \"buy milk\""
    );
    assert_eq!(prompt["cwd"], "C:\\x");
    assert!(prompt["summary"]
        .as_str()
        .unwrap()
        .starts_with("Run command: cd"));
    assert_eq!(prompt["fully_drawn"], true);
    let action = required_action_from_prompt(prompt.clone());
    assert_eq!(action["prompt_id"], prompt["prompt_id"]);
    assert_eq!(action["command"], prompt["command"]);
    assert!(action["summary"]
        .as_str()
        .unwrap()
        .contains("python todo.py add"));
}

#[test]
fn id_is_stable_while_the_option_list_is_still_drawing() {
    let partial = " $ cargo test --lib\n\n Run this command?\n";
    let full = " $ cargo test --lib\n\n Run this command?\n Run once (y)\n Skip (esc or n)\n";
    let a = detect_in_screen("cursor_agent", partial, false).unwrap();
    let b = detect_in_screen("cursor_agent", full, false).unwrap();
    assert_eq!(a["prompt_id"], b["prompt_id"]);
    assert_eq!(a["fully_drawn"], false);
    assert_eq!(b["fully_drawn"], true);
    assert!(a["prompt_id"].is_string());
}

#[test]
fn multi_line_command_survives_the_context_window() {
    let screen = " $ python -c \"x\" &&\n   python a.py &&\n   python b.py &&\n   python c.py &&\n   python d.py\n\n Run this command?\n Run once (y)\n";
    let prompt = detect_in_screen("cursor_agent", screen, false).unwrap();
    assert!(prompt["command"].as_str().unwrap().starts_with("python -c"));
    assert!(prompt["command"].as_str().unwrap().ends_with("python d.py"));
}

#[test]
fn auto_approval_end_to_end_on_a_real_screen() {
    use crate::agent_runs::approval_policy::decide;
    let allowed = vec!["python todo.py add *".to_string()];
    let prompt = detect_in_screen("cursor_agent", CD_SCREEN, false).unwrap();
    assert!(decide(&prompt, CD_SCREEN, &allowed, Some("C:\\x")).is_some());
    let evil = CD_SCREEN.replace("add \"buy milk\"", "add x && rm -rf /");
    let prompt = detect_in_screen("cursor_agent", &evil, false).unwrap();
    assert!(decide(&prompt, &evil, &allowed, Some("C:\\x")).is_none());
    let partial = " $ python todo.py add x\n\n Run this command?\n";
    let prompt = detect_in_screen("cursor_agent", partial, false).unwrap();
    assert!(
        decide(&prompt, partial, &allowed, None).is_none(),
        "never answer a dialog whose options are not drawn yet"
    );
}

#[test]
fn shell_and_idle_screens_have_no_prompt() {
    assert!(detect_in_screen("powershell", "PS C:\repo>", true).is_none());
    assert!(detect_in_screen("cursor_agent", " → Add follow-up\n", true).is_none());
}

#[test]
fn choices_map_to_keys_and_broad_needs_opt_in() {
    let prompt = detect_in_screen("cursor_agent", APPROVAL, false).unwrap();
    assert_eq!(
        reply_key("cursor_agent", &prompt, "allow_once", false)
            .unwrap()
            .0,
        "y"
    );
    assert_eq!(
        reply_key("cursor_agent", &prompt, "deny", false).unwrap().0,
        "n"
    );
    assert_eq!(
        reply_key("cursor_agent", &prompt, "allow_all", false)
            .unwrap_err()
            .code,
        "BROAD_APPROVAL_REQUIRES_OPT_IN"
    );
    assert_eq!(
        reply_key("cursor_agent", &prompt, "sudo", false)
            .unwrap_err()
            .code,
        "INVALID_PROMPT_CHOICE"
    );
}

#[test]
fn pane_prompt_serializes_camel_case_and_prettifies_choice_labels() {
    let screen = include_str!("../tui/cursor/fixtures/command_approval_visible.txt");
    let detected = detect_in_screen("cursor_agent", screen, false).expect("prompt");
    let prompt = PanePrompt::from_detected(&detected).expect("ui prompt");
    let json = serde_json::to_value(&prompt).unwrap();
    assert!(json.get("promptId").and_then(Value::as_str).is_some());
    assert!(json.get("prompt_id").is_none());
    assert_eq!(json["kind"], "command_approval");
    assert!(json["choices"].as_array().unwrap().iter().all(|c| c["id"].is_string() && c["label"].is_string()));
    assert_eq!(choice_label("allow_once"), "Allow once");
}

#[test]
fn prompt_events_fire_on_enter_change_and_leave_only() {
    let p = |id: &str| PanePrompt { prompt_id: id.into(), kind: "k".into(), text: String::new(), summary: None, choices: vec![] };
    let mut t = PromptTrack::default();
    assert_eq!(next_prompt_event(&mut t, false, || panic!("no detect when not waiting")), None);
    assert_eq!(next_prompt_event(&mut t, true, || Some(p("a"))), Some(Some(p("a"))));
    assert_eq!(next_prompt_event(&mut t, true, || Some(p("a"))), None, "same prompt: silent");
    assert_eq!(next_prompt_event(&mut t, true, || Some(p("b"))), Some(Some(p("b"))), "changed prompt");
    assert_eq!(next_prompt_event(&mut t, false, || panic!()), Some(None), "leaving clears");
    assert_eq!(next_prompt_event(&mut t, false, || panic!()), None);
    assert_eq!(next_prompt_event(&mut t, true, || None), Some(None), "waiting with no parsed prompt still announces");
    assert_eq!(next_prompt_event(&mut t, true, || None), None);
}
