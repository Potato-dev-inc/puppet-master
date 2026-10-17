//! Two screen-reading details a wizard-made profile needs: a model name a status bar clipped at the window's edge, and the
//! row an agent prints once after a turn.

use crate::tui::declarative::build_for_test;
use crate::tui::{resolve_model, ModelReading, TuiProfile};

const DEFINITION: &str = r##"
schema_version = 1

[agent]
id = "zz"
label = "Zz"
experimental = true

[binary]
names = ["zz-fixture-not-installed"]

[tui]
argv = []

[tui.footer_model]
regex = '(?:^nimbus\s+[\d.]+\s*\|\s*|model:\s*)(?P<model>[\w.-]+)'

[tui.thinking]
line_regex = ['^thinking']

[tui.summary]
line_regex = ['^thought for \d+s', '^worked for \d+s']
"##;

fn profile() -> impl TuiProfile {
    build_for_test(DEFINITION).unwrap()
}

fn reading(name: &str, cut: bool) -> ModelReading {
    ModelReading { name: name.into(), cut }
}

#[test]
fn a_reading_that_ends_its_row_is_marked_as_possibly_cut() {
    let p = profile();
    let readings = p.footer_model_readings("NIMBUS 0.4.2 | zeta-pro | D:\\x\n\nhint | model: zeta-pro\nmodel: zeta-p\u{2026}\nmodel: zeta | extra");
    let seen: Vec<(&str, bool)> = readings.iter().map(|r| (r.name.as_str(), r.cut)).collect();
    assert_eq!(seen, [("zeta-pro", false), ("zeta-pro", true), ("zeta-p", true), ("zeta", false)]);
}

#[test]
fn a_clipped_footer_name_is_the_longer_name_read_above_it() {
    let p = profile();
    // the banner names it whole, the status row at the bottom was clipped by the window
    let screen = "NIMBUS 0.4.2 | zeta-banner-pro | D:\\x\n...\nrows\n...\n/help | model: zeta-ban";
    assert_eq!(p.detect_footer_model(screen).as_deref(), Some("zeta-banner-pro"));
    // without the banner on the screen the clipped name is all there is
    assert_eq!(p.detect_footer_model("rows\n/help | model: zeta-ban").as_deref(), Some("zeta-ban"));
    // a name with something after it on its row is not clipped: it is the newest word
    assert_eq!(p.detect_footer_model("NIMBUS 0.4.2 | zeta-banner-pro | D:\\x\nrows\nmodel: zeta-ban | /help").as_deref(), Some("zeta-ban"));
    // a longer name that does not START with it is another model
    assert_eq!(p.detect_footer_model("NIMBUS 0.4.2 | other-model | D:\\x\nrows\n/help | model: zeta-ban").as_deref(), Some("zeta-ban"));
}

#[test]
fn resolve_model_keeps_what_the_pane_showed_before_over_a_clipped_start_of_it() {
    let cut = [reading("zeta-ban", true)];
    assert_eq!(resolve_model(Some("zeta-banner-pro"), &cut).as_deref(), Some("zeta-banner-pro"));
    // the pane switched model: the new name is not a start of the old one
    assert_eq!(resolve_model(Some("zeta-banner-pro"), &[reading("sonnet", true)]).as_deref(), Some("sonnet"));
    // a whole (not cut) reading is believed even when it is shorter than what was known
    assert_eq!(resolve_model(Some("zeta-banner-pro"), &[reading("zeta", false)]).as_deref(), Some("zeta"));
    assert_eq!(resolve_model(None, &[]), None);
    assert_eq!(resolve_model(None, &cut).as_deref(), Some("zeta-ban"));
    // case does not matter for "starts with"
    assert_eq!(resolve_model(Some("Zeta-Banner"), &[reading("zeta-b", true)]).as_deref(), Some("Zeta-Banner"));
}

#[test]
fn a_summary_row_matches_with_or_without_a_leading_glyph() {
    let p = profile();
    assert!(p.is_summary_line("thought for 3s, 298 tokens"));
    assert!(p.is_summary_line("\u{25b8} thought for 3s, 298 tokens"));
    assert!(p.is_summary_line("\u{2022} worked for 9s \u{2022} 8:26 pm"));
    assert!(p.is_summary_line("> * worked for 9s"));
    assert!(!p.is_summary_line("i thought for 3s about it"));
    assert!(!p.is_summary_line("the thought for 3s"));
    assert!(!p.is_summary_line(""));
    assert!(!p.is_summary_line("\u{25b8}"));
    // the thinking table is a different table, and does not take the glyph
    assert!(p.is_thinking_line("thinking about it") && !p.is_thinking_line("\u{25b8} thinking about it"));
    // a definition without the table marks nothing
    let none = build_for_test(&DEFINITION.replace("[tui.summary]\nline_regex = ['^thought for \\d+s', '^worked for \\d+s']\n", "")).unwrap();
    assert!(!none.is_summary_line("thought for 3s"));
}

#[test]
fn the_reply_cleaner_drops_a_summary_row_the_profile_marks() {
    use crate::agent_runs::completion::clean_extracted_result;
    // registered under the id the cleaner looks the profile up by
    let _serial = crate::tui::registry::test_serial();
    let registry = crate::tui::registry::registry();
    let saved = registry.snapshot();
    let mut all: Vec<std::sync::Arc<dyn TuiProfile>> = saved.iter().cloned().collect();
    all.push(std::sync::Arc::new(build_for_test(DEFINITION).unwrap()));
    registry.swap(all);
    let evidence = "reply: All good.\n\u{25b8} Thought for 3s, 298 tokens";
    let cleaned = clean_extracted_result(evidence, "Say it is fine", "zz");
    registry.swap(saved.iter().cloned().collect());
    assert_eq!(cleaned.as_deref(), Some("reply: All good."));
}
