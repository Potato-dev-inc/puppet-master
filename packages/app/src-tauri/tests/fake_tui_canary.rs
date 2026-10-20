//! CLI-update safety, end to end, with `pm-fake-tui` as the agent: zero tokens, a real ConPTY.
//!
//! "The CLI updated" is a script rewrite. The fake prints a version line for `--version` (the canary reads
//! it as a process, like a real CLI's) and draws the Codex screens the real Codex profile reads. The check
//! is the real one: a hidden ConPTY, the fake started in a scratch folder and driven to its input box,
//! nothing typed. What these tests pin down:
//!
//! 1. version 2 with the same screens: the check passes, the "Untested version" chip clears for that exact
//!    version, and the Inbox says so ("checked, all good");
//! 2. version 2 with a changed input box: the check does not recognise it, the agent goes into strict mode
//!    (no start-up notice is closed for the person) and the Inbox says the screens changed;
//! 3. a run that hits an unreadable screen (a signal) starts one check even when the version did not change.
//!
//! Included from `fake_tui_e2e.rs` (one test binary, one World).

use puppet_master_app_lib::fake_tui::World;
use serde_json::Value;

const AGENT: &str = "fake_codex";

/// Codex's update menu, a start-up notice its profile closes with a skip key (not in strict mode).
const UPDATE_MENU: &str = include_str!("../src/tui/codex/fixtures/update_prompt_live.txt");

const V1: &str = "agent = \"codex\"\nversion = \"1.0.0\"\nversion_output = \"fake-codex 1.0.0\"\n";
const V2_SAME_SCREENS: &str = "agent = \"codex\"\nversion = \"2.0.0\"\nversion_output = \"fake-codex 2.0.0\"\n";
/// Version 2 draws its input row with another glyph: the profile no longer finds the input box.
const V2_CHANGED_INPUT_BOX: &str =
    "agent = \"codex\"\nversion = \"2.0.0\"\nversion_output = \"fake-codex 2.0.0\"\ncomposer_glyph = \"\\u00bb\"\n";
/// Version 1 again, but the input row changed anyway (a server-side change, no new version).
const V1_CHANGED_INPUT_BOX: &str =
    "agent = \"codex\"\nversion = \"1.0.0\"\nversion_output = \"fake-codex 1.0.0\"\ncomposer_glyph = \"\\u00bb\"\n";

fn text<'a>(value: &'a Value, path: &str) -> &'a str {
    path.split('.').fold(value, |v, key| &v[key]).as_str().unwrap_or("")
}

#[test]
fn an_update_with_the_same_screens_is_checked_and_the_untested_chip_clears() {
    let session = World::get().session("canary-same-screens");
    session.script(V1);
    let canary = session.canary(AGENT);

    // Day one: version 1 is inside the range the fake was "tested" with. Nothing is launched.
    assert!(canary.refresh().is_empty(), "the first sighting inside the range checks nothing");
    let agent = canary.agent();
    assert_eq!(text(&agent, "version"), "1.0.0");
    assert_eq!(agent["strict"], false, "{agent}");
    assert!(!canary.is_strict());
    assert!(!canary.before_run(), "the program is the one the last probe saw: nothing is read again");

    // The CLI updates to version 2. Same screens, but nobody has tested it: the chip is on.
    session.script(V2_SAME_SCREENS);
    assert_eq!(canary.refresh(), vec![AGENT.to_string()], "a new version asks for a check");
    let agent = canary.agent();
    assert_eq!(text(&agent, "version"), "2.0.0");
    assert_eq!((text(&agent, "chip"), text(&agent, "strict_reason")), ("Untested version", "untested_version"), "{agent}");
    assert!(canary.is_strict());
    assert_eq!(canary.dismiss_keys(UPDATE_MENU), None, "strict mode: nothing closes the update menu for the person");

    // The check starts the fake in a scratch folder, reaches its input box and sends nothing.
    let checked = canary.check().expect("the check ran");
    assert_eq!(text(&checked, "last.outcome"), "ready", "{checked}");
    assert_eq!(checked["verified"], true, "{checked}");
    assert_eq!(checked["strict"], false, "{checked}");
    assert!(checked["chip"].is_null(), "the chip is gone for this exact version: {checked}");
    assert!(!canary.is_strict());
    assert!(canary.dismiss_keys(UPDATE_MENU).is_some(), "the update menu is closed for the person again");
    assert!(session.events().iter().all(|e| e["event"] != "prompt"), "the check typed no prompt: {:?}", session.events());

    // The result is shown, not only failures.
    let notices = canary.notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0]["kind"], "good");
    assert_eq!(text(&notices[0], "text"), "Fake Codex 1.0.0 \u{2192} 2.0.0: checked, all good.");
    assert_eq!(text(&checked, "change.text"), text(&notices[0], "text"));
    assert_eq!(text(&checked, "change.verdict"), "good");
}

#[test]
fn an_update_with_a_changed_input_box_is_not_recognised_and_the_agent_asks_before_acting() {
    let session = World::get().session("canary-changed-input-box");
    session.script(V1);
    let canary = session.canary(AGENT);
    assert!(canary.refresh().is_empty());

    session.script(V2_CHANGED_INPUT_BOX);
    assert_eq!(canary.refresh(), vec![AGENT.to_string()]);
    let checked = canary.check().expect("the check ran");
    assert_eq!(text(&checked, "last.outcome"), "unrecognised", "{checked}");
    assert_eq!(checked["verified"], false, "{checked}");
    assert_eq!(text(&checked, "strict_reason"), "changed_screens", "{checked}");
    assert_eq!(text(&checked, "chip"), "Untested version");
    assert!(canary.is_strict());
    assert_eq!(canary.dismiss_keys(UPDATE_MENU), None, "strict mode: the person is asked, nothing is pressed");

    let notices = canary.notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0]["kind"], "changed");
    assert_eq!(
        text(&notices[0], "text"),
        "Fake Codex 1.0.0 \u{2192} 2.0.0: screens changed. Puppet Master will ask you before acting."
    );
    assert_eq!(text(&checked, "change.verdict"), "changed");
    // The screen the check could not read is kept with the result, for the person to look at.
    assert!(
        checked["last"]["screens"].as_array().is_some_and(|s| s.iter().any(|shot| text(shot, "screen").contains('\u{bb}'))),
        "{checked}"
    );

    // The fix arrives as the next version: screens read again, the chip goes for it.
    session.script(
        "agent = \"codex\"\nversion = \"2.0.1\"\nversion_output = \"fake-codex 2.0.1\"\n",
    );
    assert_eq!(canary.refresh(), vec![AGENT.to_string()]);
    let fixed = canary.check().expect("the check ran");
    assert_eq!((fixed["verified"].as_bool(), fixed["strict"].as_bool()), (Some(true), Some(false)), "{fixed}");
    assert!(!canary.is_strict());
}

#[test]
fn a_run_that_meets_an_unreadable_screen_starts_one_check_even_without_a_new_version() {
    let session = World::get().session("canary-signal");
    session.script(V1);
    let canary = session.canary(AGENT);
    assert!(canary.refresh().is_empty());
    assert!(!canary.is_strict());

    // The CLI changes its input box without changing its version; a run cannot find the box.
    session.script(V1_CHANGED_INPUT_BOX);
    let started = (0..20).filter(|_| canary.signal("not_ready")).count();
    assert_eq!(started, 1, "twenty signals start one check");
    let checked = canary.check().expect("the check ran");
    assert_eq!(text(&checked, "last.outcome"), "unrecognised", "{checked}");
    assert_eq!(text(&checked, "strict_reason"), "changed_screens");
    assert!(canary.is_strict());
    assert!(!canary.signal("unreadable_screen"), "the answer is known for this version");

    // With the switch off, a signal starts nothing.
    canary.set_auto_check(false);
    assert!(!canary.signal("ambiguous_idle"));
}
