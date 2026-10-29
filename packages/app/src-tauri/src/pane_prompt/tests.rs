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
const TRUST_REWRAPPED: &str = "  Cursor Agent\n  v2026.10.01-e373342\n\n  \u{256d}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256e}\n  \u{2502} \u{26a0} Workspace Trust \u{2502}\n  \u{2502} Required \u{2502}\n  \u{2502} Cursor Agent can \u{2502}\n  \u{2502} execute code and \u{2502}\n  \u{2502} access files in \u{2502}\n  \u{2502} this directory. \u{2502}\n  \u{2502} Do you trust the \u{2502}\n  \u{2502} contents of this \u{2502}\n  \u{2502} directory? \u{2502}\n  \u{2502} C:\\Users\\you-pc\\AppData\\ \u{2502}\n  \u{2502} Local\\Temp\\cur-_b1a_ip3 \u{2502}\n  \u{2502} [a] Trust this \u{2502}\n  \u{2502} workspace \u{2502}\n  \u{2502} [q] Quit \u{2502}\n  \u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256f}\n";

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
    let p = |id: &str| PanePrompt { prompt_id: id.into(), kind: "k".into(), text: String::new(), summary: None, choices: vec![], risk: None, packages: None };
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

// ---- the id is the question's, not the screen's (screen-read agents) ---------------------------------------------------

const ID_PROBE: &str = r##"
schema_version = 1

[agent]
id = "idprobe"
label = "Id probe"
experimental = true

[binary]
names = ["idprobe"]

[tui]
argv = []

[[tui.prompt]]
kind = "workspace_trust"
detect_regex = 'do you trust the files in this (other )?folder'
choices = [
  { id = "allow_once", text = "1" },
  { id = "deny", text = "2" },
]
"##;

fn trust_screen(glyph: &str, seconds: u32, cursor: &str, folder: &str) -> String {
    format!(
        "IDPROBE 1.0\n\nDo you trust the files in this folder?{cursor}\n  {folder}\n\n> 1. Yes, trust this folder\n  2. No, exit\n\nPress Enter to confirm\n\n{glyph} Waiting for your answer...  {seconds}s\n"
    )
}

#[test]
fn a_declared_question_keeps_its_id_while_a_spinner_a_counter_and_a_cursor_redraw_under_it() {
    let profile = std::sync::Arc::new(
        crate::tui::declarative::DeclarativeProfile::new(crate::tui::declarative::parse_definition(ID_PROBE).expect("parses")).expect("builds"),
    );
    crate::tui::registry::registry().add_transient(profile).expect("registered");
    let id_of = |screen: &str| detect_in_screen("idprobe", screen, false).expect("a card")["prompt_id"].as_str().expect("an id").to_string();
    let first = id_of(&trust_screen("\u{280b}", 1, "", "/work/a"));
    for (glyph, seconds, cursor) in [("\u{2819}", 2, " \u{258c}"), ("\u{2839}", 3, ""), ("\u{2838}", 41, " \u{258c}")] {
        assert_eq!(first, id_of(&trust_screen(glyph, seconds, cursor, "/work/a")), "{glyph} {seconds}s {cursor:?}");
    }
    // the output of a previous turn scrolled in above does not change it either
    let scrolled = format!("older output line\nand another one\n{}", trust_screen("\u{280b}", 9, "", "/work/a"));
    assert_eq!(first, id_of(&scrolled));
    // another question is another id
    let other = trust_screen("\u{280b}", 1, "", "/work/a").replace("in this folder?", "in this other folder?");
    assert_ne!(first, id_of(&other));
    crate::tui::registry::registry().remove_transient("idprobe");
}

#[test]
fn a_command_prompt_read_by_the_shared_rules_ignores_what_is_drawn_below_it_but_not_which_command() {
    let a = "Run this command?\n  rm -rf build\n\n> 1. Yes\n  2. No\n";
    let id_of = |text: &str| {
        let mut prompt = json!({"agent_type": "x", "kind": "command_approval", "text": text, "choices": ["allow_once", "deny"]});
        prompt["prompt_id"] = Value::String(prompt_id_for(&prompt).expect("an id"));
        prompt["prompt_id"].as_str().unwrap().to_string()
    };
    let first = id_of(a);
    assert_eq!(first, id_of(&format!("{a}\n\u{280b} waiting 12s\n")), "a spinner row under the choices");
    assert_eq!(first, id_of(&format!("{a}\n\u{2819} waiting 13s\n")));
    assert_ne!(first, id_of(&a.replace("rm -rf build", "rm -rf dist")), "another command is another question");
    assert_ne!(first, id_of(&a.replace("Run this command?", "Run this other command?")));
}

#[test]
fn a_menu_with_allow_once_in_it_is_read_as_a_menu_where_the_shared_guess_would_type_y() {
    let profile = std::sync::Arc::new(
        crate::tui::declarative::DeclarativeProfile::new(
            crate::tui::declarative::parse_definition(&ID_PROBE.replace("idprobe", "menuprobe")).expect("parses"),
        )
        .expect("builds"),
    );
    crate::tui::registry::registry().add_transient(profile).expect("registered");
    let menu = "MENUPROBE\n\nRun this command?\n    echo hi\n\n> 1. Allow once\n  2. Always allow in this project\n  3. Deny\n";
    // a pane that is not waiting is offered nothing new: the shared reading stays what it was
    let shared = detect_in_screen_with_status("menuprobe", menu, false, Some("running")).expect("the shared reading");
    assert_eq!(shared["kind"], "command_approval");
    // a waiting pane gets the menu as it is: options, the keys that pick them, recognised by shape
    let card = detect_in_screen_with_status("menuprobe", menu, false, Some("waiting_input")).expect("a card");
    assert_eq!(card["kind"], "unknown_question");
    assert_eq!(card["choices"], json!(["option_1", "option_2", "option_3"]));
    assert!(card["prompt_id"].as_str().is_some_and(|id| !id.is_empty()));
    // a one-line [y/n] question is what the shared reading answers right: it stays
    let yes_no = "MENUPROBE\n\nAllow once? [y/n]\n";
    for status in [Some("running"), Some("waiting_input")] {
        let found = detect_in_screen_with_status("menuprobe", yes_no, false, status).expect("a card");
        assert_eq!(found["kind"], "command_approval", "{status:?}");
    }
    // and a profile's own table is never replaced
    let trust = trust_screen("\u{280b}", 1, "", "/work/a").replace("IDPROBE", "MENUPROBE");
    assert_eq!(detect_in_screen_with_status("menuprobe", &trust, false, Some("waiting_input")).expect("a card")["kind"], "workspace_trust");
    crate::tui::registry::registry().remove_transient("menuprobe");
}

// ---- Package check: `packages` on the prompt (facts only; the id, the choices and the buttons do not depend on it) ----

const LODAHS_SCREEN: &str = " $ npm install lodahs\n\n Run this command?\n Run once (y)\n Skip (esc or n)\n\n → Add follow-up\n";

#[test]
fn an_install_approval_carries_its_packages_and_the_ui_shape_serializes_them() {
    let prompt = detect_in_screen("cursor_agent", LODAHS_SCREEN, false).unwrap();
    let item = &prompt["packages"]["items"][0];
    assert_eq!(item["name"], "lodahs");
    assert_eq!(item["lookup"], true);
    assert!(item["first"].as_str().unwrap().starts_with("Looks like lodash"), "{item}");
    let ui = PanePrompt::from_detected(&prompt).unwrap();
    let packages = ui.packages.as_ref().expect("packages on the UI prompt");
    assert_eq!(packages.items.len(), 1);
    assert_eq!(serde_json::to_value(&ui).unwrap()["packages"]["items"][0]["name"], "lodahs");
    // The risk is the offline look-alike one, already on the first paint.
    assert_eq!(ui.risk.as_ref().unwrap().headline, "Install a package with a look-alike name");
}

#[test]
fn a_prompt_without_an_install_has_no_packages_key() {
    let prompt = detect_in_screen("cursor_agent", " $ cargo test --lib\n\n Run this command?\n Run once (y)\n Skip (esc or n)\n", false).unwrap();
    assert!(prompt.get("packages").is_none(), "{prompt}");
    assert!(PanePrompt::from_detected(&prompt).unwrap().packages.is_none());
    let json = serde_json::to_value(PanePrompt::from_detected(&prompt).unwrap()).unwrap();
    assert!(json.get("packages").is_none());
}

#[test]
fn a_trust_gate_has_no_packages() {
    let prompt = detect_in_screen("cursor_agent", TRUST_PENDING, false).unwrap();
    assert_eq!(prompt["kind"], "workspace_trust");
    assert!(prompt.get("packages").is_none());
    assert!(PanePrompt::from_detected(&prompt).unwrap().packages.is_none());
}

#[test]
fn a_command_not_fully_drawn_marks_its_last_package_cut() {
    let drawing = " $ npm install lodash lodahs\n\n Run this command?\n";
    let prompt = detect_in_screen("cursor_agent", drawing, false).unwrap();
    assert_eq!(prompt["fully_drawn"], false);
    let items = prompt["packages"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["cut"], false);
    assert_eq!(items[1]["cut"], true);
    assert_eq!(items[1]["lookup"], false);
    assert!(items[1].get("lookalike").is_none(), "a cut name is never judged: {}", items[1]);
    // Once the options are drawn the same command is whole.
    let whole = detect_in_screen("cursor_agent", LODAHS_SCREEN, false).unwrap();
    assert_eq!(whole["packages"]["items"][0]["cut"], false);
}

#[test]
fn a_command_ending_in_an_ellipsis_marks_its_last_package_cut() {
    for command in ["npm install lodash lodahs\u{2026}", "npm install lodash lodahs..."] {
        let mut prompt = json!({"agent_type": "cursor_agent", "kind": "command_approval", "command": command});
        attach_risk(&mut prompt, None);
        let items = prompt["packages"]["items"].as_array().unwrap();
        assert_eq!(items.last().unwrap()["cut"], true, "{command}");
        assert_eq!(items.first().unwrap()["cut"], false, "{command}");
    }
}

#[test]
fn the_prompt_id_and_choices_do_not_depend_on_packages() {
    let prompt = detect_in_screen("cursor_agent", LODAHS_SCREEN, false).unwrap();
    assert!(prompt.get("packages").is_some());
    let mut stripped = prompt.clone();
    stripped.as_object_mut().unwrap().remove("packages");
    assert_eq!(prompt_id_for(&stripped).as_deref(), prompt["prompt_id"].as_str());
    // Reading the same screen again gives the same id and the same answers.
    let again = detect_in_screen("cursor_agent", LODAHS_SCREEN, false).unwrap();
    assert_eq!(again["prompt_id"], prompt["prompt_id"]);
    assert_eq!(again["choices"], prompt["choices"]);
    assert_eq!(PanePrompt::from_detected(&prompt).unwrap().choices, PanePrompt::from_detected(&stripped).unwrap().choices);
}

#[test]
fn a_name_that_looks_like_a_secret_is_counted_not_listed_on_the_prompt() {
    let mut prompt = json!({"prompt_id": "p1", "agent_type": "cursor_agent", "kind": "command_approval", "command": "npm i sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789"});
    attach_risk(&mut prompt, None);
    assert_eq!(prompt["packages"]["hidden"], 1);
    assert!(prompt["packages"]["items"].as_array().unwrap().is_empty());
    let ui = PanePrompt::from_detected(&prompt).unwrap();
    assert_eq!(ui.packages.unwrap().hidden, 1);
    assert!(!prompt["packages"].to_string().contains("sk-ant-api03"), "the secret-looking name must not be in the packages");
}
