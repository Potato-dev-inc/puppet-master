//! Verify for the visible path (SPEC 11.8): the wizard rules for a draft with `[tui]`, the captured
//! screens as fixtures with merged expectations, masking, and the plan of the visible test.

use serde_json::{json, Value};

use super::support::{call, ok, probed, visible_draft, with_captures, words_of, Tree, GOOD};

fn verify(wizard: &crate::tui::declarative::wizard::Wizard, id: &str, toml: &str, expect: Value) -> Value {
    ok(wizard, "verify", json!({"session": id, "toml": toml, "expect": expect}))
}

fn codes(v: &Value) -> Vec<String> {
    v["errors"].as_array().unwrap().iter().map(|e| e["code"].as_str().unwrap().to_string()).collect()
}

fn no_real_word(v: &Value, words: &crate::tui::declarative::wizard::session::TestWords) {
    let shown = v.to_string();
    for w in [&words.one, &words.two, &words.sky] {
        assert!(!shown.contains(w.as_str()), "the real word {w} reached the drafter: {shown}");
    }
}

#[test]
fn a_good_visible_draft_passes_and_plans_the_visible_test() {
    let tree = Tree::new("vv-good");
    let (wizard, _host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    with_captures(&wizard, &id);
    let v = verify(&wizard, &id, &visible_draft("fake-agent"), json!({}));
    assert_eq!(v["ok"], true, "{}", v["text"]);
    assert_eq!(v["state"], "verified");
    assert_eq!(v["checked"]["fixtures"], 5, "trust, idle, working, reply, permission");
    assert_eq!(v["rendered"]["tui_argv"], json!([]));
    let planned = &v["planned_visible"];
    assert_eq!(planned["cwd"], "<isolated folder>");
    assert_eq!(planned["grid"], json!({"cols": 120, "rows": 40}));
    assert_eq!(planned["args"], json!([]));
    assert!(planned["program"]["path"].as_str().unwrap().to_lowercase().contains("fake-agent"));
    assert_eq!(v["can_visibletest"], true);
    assert!(v.get("planned").is_none() || v["planned"].is_null(), "a draft without [headless] has no headless plan");
    // the same text again gives the same plan
    let again = verify(&wizard, &id, &visible_draft("fake-agent"), json!({}));
    assert_eq!(again["planned_visible"]["fingerprint"], planned["fingerprint"]);
    no_real_word(&v, &words_of(&wizard, &id));
}

#[test]
fn a_session_without_captures_still_needs_a_headless_table() {
    let tree = Tree::new("vv-nocap");
    let wizard = tree.wizard();
    let id = probed(&wizard, "fake-agent");
    let v = verify(&wizard, &id, &visible_draft("fake-agent"), json!({}));
    assert_eq!(v["ok"], false);
    assert!(codes(&v).contains(&"WIZARD_NO_HEADLESS".to_string()), "{:?}", codes(&v));
    assert!(v.get("planned_visible").is_none());
}

#[test]
fn a_session_with_captures_needs_a_tui_table_and_the_tables_that_make_it_readable() {
    let tree = Tree::new("vv-rules");
    let (wizard, _host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    with_captures(&wizard, &id);
    // a headless-only draft (phase 1) is not enough once the screens were captured
    let v = verify(&wizard, &id, GOOD, json!({}));
    assert_eq!(v["ok"], false);
    assert!(codes(&v).contains(&"WIZARD_NO_TUI".to_string()), "{:?}", codes(&v));
    let draft = visible_draft("fake-agent");
    let no_ready = draft.replace("[tui.ready]\nregex = '\\|\\s*>\\s*type a task'\nnot_regex", "[tui.ready]\nnot_regex");
    assert_ne!(no_ready, draft, "the test edit applied");
    assert!(codes(&verify(&wizard, &id, &no_ready, json!({}))).contains(&"WIZARD_TUI_NO_READY".to_string()));
    let no_busy = draft.replace("working_regex = ['ctrl\\+c to stop']", "working_regex = []");
    assert_ne!(no_busy, draft);
    assert!(codes(&verify(&wizard, &id, &no_busy, json!({}))).contains(&"WIZARD_TUI_NO_BUSY".to_string()));
    let cut = draft.find("[[tui.prompt]]").unwrap();
    let no_prompt = &draft[..cut];
    assert!(codes(&verify(&wizard, &id, no_prompt, json!({}))).contains(&"WIZARD_TUI_NO_PROMPT".to_string()));
}

#[test]
fn the_text_for_the_drafter_names_the_capture_and_shows_the_screen_masked() {
    let tree = Tree::new("vv-mask");
    let (wizard, _host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    with_captures(&wizard, &id);
    // [tui.ready] that never matches the idle screen
    let draft = visible_draft("fake-agent").replace("regex = '\\|\\s*>\\s*type a task'\nnot_regex", "regex = 'zzz never'\nnot_regex");
    let v = verify(&wizard, &id, &draft, json!({}));
    assert_eq!(v["ok"], false);
    let text = v["text"].as_str().unwrap();
    assert!(text.contains("tui_idle") && text.contains("ready: expected true, got false"), "{text}");
    assert!(text.contains("The screen:"), "the failing capture is shown: {text}");
    assert!(text.contains("| > Type a task..."), "{text}");
    // the reply capture is shown with the word masked
    assert!(text.contains("reply: <word-1>"), "{text}");
    no_real_word(&v, &words_of(&wizard, &id));
}

#[test]
fn a_busy_pattern_that_also_matches_the_idle_screen_is_caught() {
    let tree = Tree::new("vv-busy");
    let (wizard, _host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    with_captures(&wizard, &id);
    let draft = visible_draft("fake-agent").replace("working_regex = ['ctrl\\+c to stop']", "working_regex = ['type a task']");
    let v = verify(&wizard, &id, &draft, json!({}));
    assert_eq!(v["ok"], false);
    assert!(codes(&v).contains(&"FIXTURE_EXPECT_INVALID".to_string()), "{:?}\n{}", codes(&v), v["text"]);
    assert!(v["text"].as_str().unwrap().contains("busy"), "{}", v["text"]);
}

#[test]
fn the_reply_row_may_never_be_chrome() {
    let tree = Tree::new("vv-chrome");
    let (wizard, _host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    with_captures(&wizard, &id);
    let draft = visible_draft("fake-agent").replace("'^done in \\d+s$',", "'^done in \\d+s$',\n  '^reply:',");
    let v = verify(&wizard, &id, &draft, json!({}));
    assert_eq!(v["ok"], false);
    let text = v["text"].as_str().unwrap();
    assert!(text.contains("not_chrome_lines") && text.contains("reply: <word-1>"), "{text}");
    no_real_word(&v, &words_of(&wizard, &id));
}

#[test]
fn a_dialog_the_profile_does_not_read_is_caught() {
    let tree = Tree::new("vv-prompt");
    let (wizard, _host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    with_captures(&wizard, &id);
    let draft = visible_draft("fake-agent").replace("detect_regex = 'allow nimbus to .*\\[y/n\\]'", "detect_regex = 'nothing like it'");
    assert_ne!(draft, visible_draft("fake-agent"));
    let v = verify(&wizard, &id, &draft, json!({}));
    assert_eq!(v["ok"], false);
    assert!(codes(&v).contains(&"FIXTURE_EXPECT_INVALID".to_string()), "{:?}", codes(&v));
    assert!(v["text"].as_str().unwrap().contains("tui_permission"), "{}", v["text"]);
}

#[test]
fn the_drafters_extra_expectations_are_checked_against_the_capture() {
    let tree = Tree::new("vv-extras");
    let (wizard, _host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    with_captures(&wizard, &id);
    let draft = visible_draft("fake-agent");
    // valid extras: a chrome row that is on the idle screen, the model on the reply screen
    let good = json!({"tui_idle": "chrome_lines = [\"NIMBUS 0.4 ready\"]\n", "tui_reply": "not_chrome_lines = [\"reply: <word-1>\"]\nchrome_lines = [\"done in 2s\"]\n"});
    let v = verify(&wizard, &id, &draft, good);
    assert_eq!(v["ok"], true, "{}", v["text"]);
    // a row that is not on the capture
    let v = verify(&wizard, &id, &draft, json!({"tui_idle": "chrome_lines = [\"a row nobody drew\"]\n"}));
    assert_eq!(v["ok"], false);
    assert!(codes(&v).contains(&"FIXTURE_EXPECT_INVALID".to_string()));
    assert!(v["text"].as_str().unwrap().contains("a row nobody drew"));
    // keys the drafter may not use, and capture names that do not exist
    for (name, text) in [("tui_idle", "busy = true\n"), ("tui_idle", "ready = false\n"), ("tui_nothing", "chrome_lines = []\n"), ("tui_idle", "this is not toml [")] {
        let v = verify(&wizard, &id, &draft, json!({name: text}));
        assert_eq!(v["ok"], false, "{name}: {text}");
        assert!(codes(&v).contains(&"FIXTURE_EXPECT_INVALID".to_string()), "{name}: {text}: {:?}", codes(&v));
    }
    // a chrome row that is really reply text (it would be filed as chrome) is caught by the check, not trusted
    let v = verify(&wizard, &id, &draft, json!({"tui_reply": "chrome_lines = [\"reply: <word-1>\"]\n"}));
    assert_eq!(v["ok"], false);
}

#[test]
fn a_failed_verify_keeps_a_testable_draft_for_the_visible_test() {
    let tree = Tree::new("vv-testable");
    let (wizard, _host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    with_captures(&wizard, &id);
    // fails a screen fixture only: the visible test may still run (it gives the drafter the real behaviour)
    let draft = visible_draft("fake-agent").replace("detect_regex = 'allow nimbus to .*\\[y/n\\]'", "detect_regex = 'nothing like it'");
    let v = verify(&wizard, &id, &draft, json!({}));
    assert_eq!(v["ok"], false);
    assert_eq!(v["can_visibletest"], true);
    assert!(v["planned_visible"]["fingerprint"].is_string());
    assert_eq!(v["state"], "probed");
    // a draft that does not parse is not testable
    let v = verify(&wizard, &id, "this is [ not toml", json!({}));
    assert_eq!(v["ok"], false);
    assert!(v.get("planned_visible").is_none());
    let err = call(&wizard, "verify", json!({"session": id, "toml": draft, "expect": "nope"})).unwrap_err();
    assert_eq!(err.status, 400);
}

#[test]
fn the_wizard_rules_still_hold_for_a_visible_draft() {
    let tree = Tree::new("vv-rules2");
    let (wizard, _host) = tree.visible_wizard();
    let id = probed(&wizard, "fake-agent");
    with_captures(&wizard, &id);
    // the program must be the one the person typed
    let v = verify(&wizard, &id, &visible_draft("other-agent"), json!({}));
    assert!(codes(&v).contains(&"WIZARD_BINARY_MISMATCH".to_string()), "{:?}", codes(&v));
    // the drafter cannot bake a test word into a regex
    let words = words_of(&wizard, &id);
    let draft = visible_draft("fake-agent").replace("'^done in \\d+s$',", &format!("'^done in \\d+s$', '{}',", words.one));
    let v = verify(&wizard, &id, &draft, json!({}));
    assert!(codes(&v).contains(&"WIZARD_TEST_WORD".to_string()), "{:?}", codes(&v));
}
