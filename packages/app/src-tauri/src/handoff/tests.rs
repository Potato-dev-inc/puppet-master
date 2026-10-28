//! Detect-only hand-off: the patterns (with negatives), reset parsing, the state machine, the card,
//! and the guards that keep Puppet Master's hands off the agent's pane.
//!
//! Every screen here is SYNTHETIC (built from the vendor texts quoted in the design; see the `#!`
//! headers of `tui/<agent>/fixtures/limits/*`). Nothing in this file has seen a live limit yet.

use super::detect::{LimitEvidence, LimitHit, LimitKind};
use super::limits::{validate_limits, CompiledLimits, LimitsFile};
use super::reset::{parse_reset, when_text, Clock, FixedClock};
use super::watch::{Cleared, Event, Standing, Tracker};
use super::*;
use crate::operations::{OperationStatus, StateSource};
use crate::questions::{QuestionState, QuestionStore};
use crate::tui::profile_for;
use chrono::NaiveDateTime;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

// ---- fixtures -------------------------------------------------------------------------------------

struct Fixture {
    name: String,
    headers: Vec<(String, String)>,
    text: String,
}

impl Fixture {
    fn header(&self, key: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

fn fixtures_in(agent: &str) -> Vec<Fixture> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("tui").join(agent).join("fixtures").join("limits");
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "txt") {
                let raw = std::fs::read_to_string(&path).unwrap();
                let mut headers = Vec::new();
                let mut body = Vec::new();
                for line in raw.lines() {
                    match line.strip_prefix("#! ") {
                        Some(h) => {
                            let (k, v) = h.split_once(": ").unwrap_or((h, ""));
                            headers.push((k.to_string(), v.to_string()));
                        }
                        None => body.push(line),
                    }
                }
                out.push(Fixture {
                    name: format!("{agent}/{}", path.file_stem().unwrap().to_string_lossy()),
                    headers,
                    text: body.join("\n"),
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn fixture(agent: &str, id: &str) -> Fixture {
    fixtures_in(agent).into_iter().find(|f| f.name == format!("{agent}/{id}")).unwrap_or_else(|| panic!("no fixture {agent}/{id}"))
}

/// The screen the way the app reads it: the text through the terminal emulator at `cols` columns, so
/// a row the terminal soft-wrapped arrives as one row.
fn render(text: &str, cols: u16) -> String {
    let mut parser = vt100::Parser::new(40, cols, 0);
    parser.process(text.replace('\n', "\r\n").as_bytes());
    parser.screen().contents()
}

fn cols_of(f: &Fixture) -> u16 {
    f.header("cols").and_then(|c| c.parse().ok()).unwrap_or(120)
}

fn clock_of(f: &Fixture) -> FixedClock {
    // "2026-10-15T12:00 +08:00 Asia/Taipei"
    let now = f.header("now").unwrap_or_else(|| panic!("{}: no `now` header", f.name));
    let mut parts = now.split(' ');
    let local = NaiveDateTime::parse_from_str(parts.next().unwrap(), "%Y-%m-%dT%H:%M").unwrap();
    let offset: i32 = parts.next().unwrap().split(':').next().unwrap().parse().unwrap();
    let zone: &'static str = Box::leak(parts.next().unwrap().to_string().into_boxed_str());
    FixedClock::at(local, offset, Some(zone))
}

/// `key=value key="two words"` pairs.
fn pairs(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    loop {
        while chars.peek() == Some(&' ') {
            chars.next();
        }
        let key: String = std::iter::from_fn(|| chars.next_if(|c| *c != '=' && *c != ' ')).collect();
        if key.is_empty() {
            break;
        }
        chars.next(); // '='
        let value: String = if chars.peek() == Some(&'"') {
            chars.next();
            let v = std::iter::from_fn(|| chars.next_if(|c| *c != '"')).collect();
            chars.next();
            v
        } else {
            std::iter::from_fn(|| chars.next_if(|c| *c != ' ')).collect()
        };
        out.push((key, value));
    }
    out
}

fn kind_name(kind: LimitKind) -> String {
    serde_json::to_value(kind).unwrap().as_str().unwrap().to_string()
}

/// What a fixture's `expect` header says about the detector's answer; empty = as expected.
fn mismatches(agent: &str, f: &Fixture) -> Vec<String> {
    let profile = profile_for(agent);
    let clock = clock_of(f);
    let screen = render(&f.text, cols_of(f));
    let hit = profile.detect_limit(&screen, &clock);
    let expect = f.header("expect").unwrap_or_else(|| panic!("{}: no `expect` header", f.name));
    let mut bad = Vec::new();
    if expect == "none" {
        if let Some(hit) = hit {
            bad.push(format!("{}: expected no limit, got {hit:?}", f.name));
        }
        return bad;
    }
    let Some(hit) = hit else { return vec![format!("{}: expected {expect}, got no limit\n{screen}", f.name)] };
    for (key, want) in pairs(expect) {
        let got = match key.as_str() {
            "kind" => kind_name(hit.kind),
            "rule" => hit.rule_id.clone(),
            "window" => hit.window.clone().unwrap_or_else(|| "none".into()),
            "reset" => hit.reset_text.clone().unwrap_or_else(|| "none".into()),
            "armed" => hit.self_resume_armed.to_string(),
            "menu" => hit.menu_open.to_string(),
            "reset_at" => hit.reset_at_ms.map(|ms| when_iso(ms as i64, &clock)).unwrap_or_else(|| "none".into()),
            other => panic!("{}: unknown expect key {other}", f.name),
        };
        if got != want {
            bad.push(format!("{}: {key}: expected {want:?}, got {got:?} ({hit:?})", f.name));
        }
    }
    bad
}

fn when_iso(ms: i64, clock: &dyn Clock) -> String {
    clock.local_at(ms).format("%Y-%m-%dT%H:%M").to_string()
}

// ---- the tables ------------------------------------------------------------------------------------

#[test]
fn the_embedded_limits_tables_compile_and_validate() {
    for (agent, vendor) in [("claude", "anthropic"), ("codex", "openai")] {
        let profile = profile_for(agent);
        let limits = profile.limits().unwrap_or_else(|| panic!("{agent} has no [limits] table"));
        assert_eq!(limits.vendor(), vendor);
        assert!(!limits.vendor_varies());
        assert!(!limits.usage.is_empty() && !limits.reset_formats.is_empty());
        assert!(limits.table.tail_lines >= 1 && limits.table.stable_ms >= 200);
    }
    // An agent with no rules is never reacted to: generic code asks the profile, never a name.
    assert!(profile_for("shell").limits().is_none());
    assert!(profile_for("opencode").limits().is_none());
}

#[test]
fn claude_declares_that_it_can_wait_by_itself_and_codex_that_it_cannot() {
    use super::limits::SelfResume;
    assert_eq!(profile_for("claude").limits().unwrap().table.self_resume, SelfResume::Setting);
    assert_eq!(profile_for("codex").limits().unwrap().table.self_resume, SelfResume::None);
}

#[test]
fn every_fixture_says_where_it_came_from_and_is_labelled_synthetic_until_it_is_live() {
    for agent in ["claude", "codex"] {
        let all = fixtures_in(agent);
        assert!(all.len() >= 12, "{agent}: {} fixtures", all.len());
        for f in &all {
            let source = f.header("source").unwrap_or_else(|| panic!("{}: no `source` header", f.name));
            assert!(
                source.starts_with("synthetic") || source.starts_with("probe-mock") || source.starts_with("live"),
                "{}: source must start with live / probe-mock / synthetic: {source}",
                f.name
            );
            // Nothing is live yet: a fixture claiming to be live must come with a capture note.
            if source.starts_with("synthetic") {
                assert!(source.contains("NOT a live capture"), "{}: {source}", f.name);
            }
            assert!(f.header("expect").is_some(), "{}: no `expect` header", f.name);
        }
    }
}

#[test]
fn every_limit_fixture_is_classified_as_its_header_says() {
    let mut bad = Vec::new();
    let mut checked = 0;
    for agent in ["claude", "codex"] {
        for f in fixtures_in(agent) {
            bad.extend(mismatches(agent, &f));
            checked += 1;
        }
    }
    assert!(bad.is_empty(), "{} of {checked} fixtures misclassified:\n{}", bad.len(), bad.join("\n"));
    assert!(checked >= 30, "{checked}");
}

#[test]
fn negatives_exist_for_both_agents_and_none_of_them_is_a_limit() {
    for agent in ["claude", "codex"] {
        let negatives: Vec<_> = fixtures_in(agent).into_iter().filter(|f| f.header("expect") == Some("none")).collect();
        assert!(negatives.len() >= 3, "{agent}: {}", negatives.len());
        let quoting = negatives.iter().filter(|f| f.name.contains("neg_reply_quotes_limit")).count();
        assert_eq!(quoting, 1, "{agent}: the reply-quotes-the-limit negative (N1) is missing");
        assert!(negatives.iter().any(|f| f.name.contains("diff")), "{agent}: the test-file-in-a-diff negative (N2) is missing");
        for f in negatives {
            assert!(mismatches(agent, &f).is_empty(), "{}", f.name);
        }
    }
}

#[test]
fn codex_warning_is_read_wrapped_at_52_columns_and_at_200() {
    for id in ["warning_52cols", "warning_200cols"] {
        let f = fixture("codex", id);
        let hit = profile_for("codex").detect_limit(&render(&f.text, cols_of(&f)), &clock_of(&f)).expect(id);
        assert_eq!(hit.kind, LimitKind::Warning, "{id}");
        assert!(!hit.kind.stops_typing(), "a warning never blocks");
    }
    assert_eq!(cols_of(&fixture("codex", "warning_52cols")), 52);
    assert_eq!(cols_of(&fixture("codex", "warning_200cols")), 200);
}

#[test]
fn a_usage_limit_wrapped_by_the_agent_or_by_the_terminal_is_still_one_limit() {
    for (agent, id) in [("claude", "usage_wrapped_52cols"), ("codex", "usage_plus_wrapped_52cols")] {
        let f = fixture(agent, id);
        let screen = render(&f.text, cols_of(&f));
        let hit = profile_for(agent).detect_limit(&screen, &clock_of(&f)).expect(id);
        assert_eq!(hit.kind, LimitKind::Usage, "{id}");
        assert!(hit.reset_at_ms.is_some(), "{id}: {hit:?}");
    }
    // Codex's long Plus line: 190 characters at 120 columns, soft-wrapped by the terminal.
    let f = fixture("codex", "usage_plus_other_day");
    let at_120 = render(&f.text, 120);
    let at_80 = render(&f.text, 80);
    for screen in [at_120, at_80] {
        let hit = profile_for("codex").detect_limit(&screen, &clock_of(&f)).expect("soft-wrapped");
        assert_eq!(hit.reset_text.as_deref(), Some("Oct 16th, 2026 9:45 PM"));
    }
}

#[test]
fn only_a_usage_limit_blocks_typing_and_a_retry_never_does() {
    let blocking: Vec<(&str, &str)> = vec![
        ("claude", "usage_session_window"),
        ("claude", "usage_armed_own_wait"),
        ("claude", "usage_own_wait_off_menu"),
        ("codex", "usage_generic_same_day"),
        ("codex", "unavailable_plan"),
        ("codex", "menu_approaching"),
    ];
    for (agent, id) in blocking {
        let f = fixture(agent, id);
        let screen = render(&f.text, cols_of(&f));
        assert!(limit_gate(profile_for(agent).as_ref(), &screen).is_some(), "{agent}/{id} must stop typing");
    }
    for (agent, id) in [("claude", "transient_retrying"), ("claude", "warning_used_pct"), ("codex", "transient_reconnecting"), ("codex", "warning_52cols"), ("codex", "transient_retry_limit")] {
        let f = fixture(agent, id);
        let screen = render(&f.text, cols_of(&f));
        assert!(limit_gate(profile_for(agent).as_ref(), &screen).is_none(), "{agent}/{id} is not a limit");
    }
}

// ---- reset times -----------------------------------------------------------------------------------

fn formats_of(agent: &str) -> Vec<regex::Regex> {
    profile_for(agent).limits().unwrap().reset_formats.clone()
}

fn local(clock: &FixedClock, text: &str, agent: &str) -> Option<String> {
    parse_reset(text, &formats_of(agent), clock).map(|ms| when_iso(ms, clock))
}

#[test]
fn reset_times_today_tomorrow_another_day_and_another_year() {
    let noon = FixedClock::taipei_noon(); // Thu 2026-10-15 12:00 +08:00, Asia/Taipei
    // Claude: within 24 h the hour alone; it is the next time the clock shows it.
    assert_eq!(local(&noon, "5pm (Asia/Taipei)", "claude").as_deref(), Some("2026-10-15T17:00"));
    assert_eq!(local(&noon, "5:30pm", "claude").as_deref(), Some("2026-10-15T17:30"));
    assert_eq!(local(&noon, "11am", "claude").as_deref(), Some("2026-10-16T11:00"), "11am has passed: tomorrow");
    assert_eq!(local(&noon, "12pm", "claude").as_deref(), Some("2026-10-15T12:00"), "now");
    assert_eq!(local(&noon, "12am (Asia/Taipei)", "claude").as_deref(), Some("2026-10-16T00:00"));
    // Further out: month and day, and the year when it differs.
    assert_eq!(local(&noon, "Oct 18, 5pm (Asia/Taipei)", "claude").as_deref(), Some("2026-10-18T17:00"));
    assert_eq!(local(&noon, "Jan 3, 9:15am", "claude").as_deref(), Some("2027-01-03T09:15"), "a month already past is next year");
    assert_eq!(local(&noon, "Jan 3, 2028, 9:15am", "claude").as_deref(), Some("2028-01-03T09:15"));
    // Codex: `9:45 PM` and `Oct 16th, 2026 9:45 PM`, with the ordinal.
    assert_eq!(local(&noon, "9:45 PM", "codex").as_deref(), Some("2026-10-15T21:45"));
    assert_eq!(local(&noon, "Oct 16th, 2026 9:45 PM", "codex").as_deref(), Some("2026-10-16T21:45"));
    assert_eq!(local(&noon, "Nov 1st, 2026 12:05 AM", "codex").as_deref(), Some("2026-11-01T00:05"));
    assert_eq!(local(&noon, "Oct 22nd, 2026 3:00 PM", "codex").as_deref(), Some("2026-10-22T15:00"));
}

#[test]
fn a_reset_that_has_just_passed_is_still_today_and_a_reset_in_the_past_by_hours_is_tomorrow() {
    let evening = FixedClock::at(NaiveDateTime::parse_from_str("2026-10-15T21:47", "%Y-%m-%dT%H:%M").unwrap(), 8, Some("Asia/Taipei"));
    assert_eq!(local(&evening, "9:45 PM", "codex").as_deref(), Some("2026-10-15T21:45"), "two minutes of leeway");
    let late = FixedClock::at(NaiveDateTime::parse_from_str("2026-10-15T23:30", "%Y-%m-%dT%H:%M").unwrap(), 8, Some("Asia/Taipei"));
    assert_eq!(local(&late, "9:45 PM", "codex").as_deref(), Some("2026-10-16T21:45"));
}

#[test]
fn a_reset_in_another_zone_or_that_cannot_be_read_is_none() {
    let noon = FixedClock::taipei_noon();
    assert_eq!(local(&noon, "5pm (America/Los_Angeles)", "claude"), None, "foreign zone: the printed hour is not ours");
    let unknown_zone = FixedClock { zone: None, ..FixedClock::taipei_noon() };
    assert_eq!(local(&unknown_zone, "5pm (Asia/Taipei)", "claude"), None, "our zone cannot be told: do not guess");
    assert_eq!(local(&unknown_zone, "5pm", "claude").as_deref(), Some("2026-10-15T17:00"), "no zone printed: local time");
    for bad in ["", "soon", "25pm", "0pm", "13:00 PM", "5:75pm", "Feb 31, 5pm", "tomorrow at 5pm", "5pm and 6pm"] {
        assert_eq!(local(&noon, bad, "claude"), None, "{bad:?}");
    }
}

#[test]
fn a_local_time_that_does_not_exist_is_none() {
    // A DST gap: the clock jumps over 02:30. `to_ms` says there is no such moment.
    struct Gap(FixedClock);
    impl Clock for Gap {
        fn now_ms(&self) -> i64 {
            self.0.now_ms()
        }
        fn zone_name(&self) -> Option<String> {
            self.0.zone_name()
        }
        fn local_at(&self, ms: i64) -> NaiveDateTime {
            self.0.local_at(ms)
        }
        fn to_ms(&self, local: NaiveDateTime) -> Option<i64> {
            use chrono::Timelike;
            (!(local.hour() == 2 && local.minute() == 30)).then(|| self.0.to_ms(local)).flatten()
        }
    }
    let gap = Gap(FixedClock::taipei_noon());
    assert!(parse_reset("2:30am", &formats_of("claude"), &gap).is_none());
    assert!(parse_reset("3:30am", &formats_of("claude"), &gap).is_some());
}

#[test]
fn the_reset_shows_as_a_time_today_and_with_the_day_otherwise() {
    let noon = FixedClock::taipei_noon();
    let at = |s: &str| parse_reset(s, &formats_of("claude"), &noon).unwrap();
    assert_eq!(when_text(at("2:05pm"), &noon), "14:05");
    assert_eq!(when_text(at("Oct 18, 5pm"), &noon), "Oct 18 17:00");
    assert_eq!(when_text(at("11am"), &noon), "Oct 16 11:00");
}

// ---- the table's own validation --------------------------------------------------------------------

fn table(extra: &str) -> LimitsFile {
    toml::from_str(&format!("[limits]\nschema = 1\nvendor = \"acme\"\n{extra}")).unwrap()
}

fn codes(extra: &str) -> Vec<&'static str> {
    validate_limits(&table(extra).limits).into_iter().map(|e| e.code).collect()
}

#[test]
fn a_minimal_table_is_valid_and_each_mistake_has_its_code() {
    assert!(codes("").is_empty());
    assert_eq!(codes("[[limits.usage]]\nid = \"a\"\nregex = \"limit\"\n"), Vec::<&str>::new());
    // No vendor: the same-vendor rule keys on it.
    let no_vendor: LimitsFile = toml::from_str("[limits]\nschema = 1\nvendor = \"\"\n").unwrap();
    assert_eq!(validate_limits(&no_vendor.limits).iter().map(|e| e.code).collect::<Vec<_>>(), vec!["AGENT_DEF_LIMITS_NO_VENDOR"]);
    let shouty: LimitsFile = toml::from_str("[limits]\nschema = 1\nvendor = \"Open AI\"\n").unwrap();
    assert!(validate_limits(&shouty.limits).iter().any(|e| e.code == "AGENT_DEF_LIMITS_NO_VENDOR"));
    // `varies` is a valid vendor: the agent's billing vendor is the user's own provider.
    let varies: LimitsFile = toml::from_str("[limits]\nschema = 1\nvendor = \"varies\"\n").unwrap();
    assert!(validate_limits(&varies.limits).is_empty());
    assert!(CompiledLimits::compile(varies.limits).unwrap().vendor_varies());
    // A regex that does not compile, and a usage rule that matches nothing at all.
    assert_eq!(codes("[[limits.usage]]\nid = \"a\"\nregex = \"(unclosed\"\n"), vec!["AGENT_DEF_LIMITS_REGEX"]);
    assert_eq!(codes("[[limits.usage]]\nid = \"a\"\nregex = \".*\"\n"), vec!["AGENT_DEF_LIMITS_REGEX"]);
    assert_eq!(codes("[[limits.usage]]\nid = \"a\"\nregex = \"x?\"\n"), vec!["AGENT_DEF_LIMITS_REGEX"]);
    assert_eq!(codes("[[limits.menu]]\nregex = \"(?:)\"\n"), vec!["AGENT_DEF_LIMITS_REGEX"]);
    // Reset formats need the two groups the parser reads.
    assert!(codes("[limits.reset_formats]\nformats = ['^(?P<hour>\\d+)$']\n").contains(&"AGENT_DEF_INCONSISTENT"));
    assert!(codes("tail_lines = 0\n").contains(&"AGENT_DEF_INCONSISTENT"));
    assert!(codes("stable_ms = 5\n").contains(&"AGENT_DEF_INCONSISTENT"));
    assert!(codes("tested = \"fast\"\n").contains(&"AGENT_DEF_INCONSISTENT"));
}

#[test]
fn a_limits_table_has_no_key_that_presses_anything() {
    // The design reserves `cancel_self_resume` and `reset_gate` for the hand-off step. Until then the
    // schema is closed: a definition that declares keys is refused at parse time.
    for text in [
        "[limits]\nschema = 1\nvendor = \"acme\"\n[limits.cancel_self_resume]\nkeys = \"\\u001b\"\n",
        "[limits]\nschema = 1\nvendor = \"acme\"\n[[limits.reset_gate]]\nid = \"x\"\nregex = \"y\"\nkeys = \"\\r\"\n",
        "[limits]\nschema = 1\nvendor = \"acme\"\n[[limits.usage]]\nid = \"a\"\nregex = \"x\"\nkeys = \"\\r\"\n",
    ] {
        assert!(toml::from_str::<LimitsFile>(text).is_err(), "{text}");
    }
}

#[test]
fn a_toml_agent_definition_can_carry_its_own_limits_table() {
    let text = r##"
schema_version = 1

[agent]
id = "mycoder"
label = "My Coder"
experimental = true

[binary]
names = ["mycoder"]

[tui]
argv = []

[limits]
schema = 1
vendor = "acme"

[[limits.usage]]
id = "out"
regex = '^Out of credits\. Back at (?P<reset>\d{1,2}:\d{2} [AP]M)\.$'

[limits.reset_formats]
formats = ['^(?P<hour>\d{1,2}):(?P<minute>\d{2})\s*(?P<ampm>[ap]m)$']
"##;
    let def = crate::tui::declarative::parse_definition(text).unwrap();
    let ctx = crate::tui::declarative::ValidationContext {
        source: crate::tui::declarative::Source::User,
        code_ids: &["claude", "codex"],
        raw_len: text.len(),
    };
    assert_eq!(crate::tui::declarative::validate(&def, &ctx), vec![]);
    let profile = crate::tui::declarative::DeclarativeProfile::new(def).unwrap();
    let clock = FixedClock::taipei_noon();
    let hit = profile.detect_limit("working...\nOut of credits. Back at 9:45 PM.\n", &clock).expect("the definition's own rule");
    assert_eq!((hit.kind, hit.rule_id.as_str()), (LimitKind::Usage, "out"));
    assert_eq!(when_iso(hit.reset_at_ms.unwrap() as i64, &clock), "2026-10-15T21:45");
    // A definition with a broken table fails validation with the limits code, at the table's path.
    let broken = text.replace("^Out of credits", "(^Out of credits");
    let def = crate::tui::declarative::parse_definition(&broken).unwrap();
    let errors = crate::tui::declarative::validate(&def, &ctx);
    assert!(errors.iter().any(|e| e.code == "AGENT_DEF_LIMITS_REGEX" && e.path == "limits.usage.0.regex"), "{errors:?}");
    // And a definition without the table has no limit rules.
    let plain = text.split("[limits]").next().unwrap();
    let profile = crate::tui::declarative::DeclarativeProfile::new(crate::tui::declarative::parse_definition(plain).unwrap()).unwrap();
    assert!(profile.limits().is_none());
}

// ---- the tracker -----------------------------------------------------------------------------------

fn usage(sha: &str) -> LimitHit {
    LimitHit {
        kind: LimitKind::Usage,
        rule_id: "window".into(),
        window: Some("session limit".into()),
        reset_text: Some("5pm".into()),
        reset_at_ms: Some(1_000),
        self_resume_armed: false,
        menu_open: false,
        evidence: LimitEvidence::Seen,
        stable_ms: 2000,
        screen_sha: sha.into(),
    }
}

#[test]
fn a_limit_counts_only_after_it_has_held_still() {
    let mut t = Tracker::default();
    let (standing, events) = t.observe(10_000, "p", Some(usage("a")), Some(false));
    assert_eq!((standing, events.len()), (Standing::Pending, 0));
    assert!(t.tick(11_999).is_empty(), "1999 ms: not yet");
    let events = t.tick(12_000);
    assert_eq!(events.len(), 1);
    match &events[0] {
        Event::Fired { pane_id, hit } => {
            assert_eq!(pane_id, "p");
            assert_eq!(hit.evidence, LimitEvidence::Stable);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(t.standing_of("p"), Standing::Parked);
    assert!(t.tick(60_000).is_empty(), "fires once");
    // Seeing it again changes nothing.
    let (standing, events) = t.observe(61_000, "p", Some(usage("a")), Some(false));
    assert_eq!((standing, events.len()), (Standing::Parked, 0));
}

#[test]
fn a_changing_limit_restarts_the_clock() {
    let mut t = Tracker::default();
    t.observe(0, "p", Some(usage("a")), Some(false));
    t.observe(1_500, "p", Some(usage("b")), Some(false)); // another reset time: a different limit
    assert!(t.tick(2_500).is_empty(), "1 s since the change");
    assert_eq!(t.tick(3_500).len(), 1);
}

#[test]
fn a_limit_that_goes_away_before_it_holds_never_fires() {
    let mut t = Tracker::default();
    t.observe(0, "p", Some(usage("a")), Some(false));
    let (standing, _) = t.observe(1_000, "p", None, Some(false));
    assert_eq!(standing, Standing::Clear);
    assert!(t.tick(10_000).is_empty());
}

#[test]
fn a_parked_limit_clears_when_the_turn_resumes_and_the_same_old_text_does_not_come_back() {
    let mut t = Tracker::default();
    t.observe(0, "p", Some(usage("a")), Some(false));
    t.tick(2_000);
    // The agent carries on by itself: it is working, so the detector sees no limit (guard 2).
    let (standing, events) = t.observe(5_000, "p", None, Some(true));
    assert_eq!(standing, Standing::Clear);
    assert!(matches!(&events[..], [Event::Cleared { why: Cleared::Resumed, .. }]), "{events:?}");
    // The turn ends with the old notice still in the scrollback rows: that is history.
    let (standing, events) = t.observe(9_000, "p", Some(usage("a")), Some(false));
    assert_eq!((standing, events.len()), (Standing::Clear, 0));
    assert!(t.tick(30_000).is_empty());
    // Once the text has been gone, the same words are a new limit.
    t.observe(31_000, "p", None, Some(false));
    t.observe(32_000, "p", Some(usage("a")), Some(false));
    assert_eq!(t.tick(34_000).len(), 1);
}

#[test]
fn a_limit_whose_text_leaves_the_screen_clears_as_gone() {
    let mut t = Tracker::default();
    t.observe(0, "p", Some(usage("a")), Some(false));
    t.tick(2_000);
    let (_, events) = t.observe(3_000, "p", None, Some(false));
    assert!(matches!(&events[..], [Event::Cleared { why: Cleared::Gone, .. }]), "{events:?}");
}

#[test]
fn a_different_limit_replaces_the_parked_one() {
    let mut t = Tracker::default();
    t.observe(0, "p", Some(usage("a")), Some(false));
    t.tick(2_000);
    let (standing, events) = t.observe(5_000, "p", Some(usage("b")), Some(false));
    assert!(matches!(&events[..], [Event::Cleared { why: Cleared::Replaced, .. }]), "{events:?}");
    assert_eq!(standing, Standing::Pending);
    assert_eq!(t.tick(7_000).len(), 1);
}

#[test]
fn only_a_usage_limit_ever_fires() {
    let mut t = Tracker::default();
    for kind in [LimitKind::Transient, LimitKind::TransientFinal, LimitKind::Warning, LimitKind::Unavailable, LimitKind::Menu] {
        let mut hit = usage("x");
        hit.kind = kind;
        let (standing, events) = t.observe(0, "q", Some(hit), Some(false));
        assert_eq!((standing, events.len()), (Standing::Clear, 0), "{kind:?}");
        assert!(t.tick(1_000_000).is_empty(), "{kind:?}");
    }
    let mut retry = usage("r");
    retry.kind = LimitKind::Transient;
    t.observe(0, "q", Some(retry), Some(true));
    assert!(t.transient("q").is_some(), "a retry is remembered for the status line");
}

// ---- the engine: the run, the card, the answers -----------------------------------------------------

struct TestClock {
    now: Cell<i64>,
    inner: FixedClock,
}

impl Clock for TestClock {
    fn now_ms(&self) -> i64 {
        self.now.get()
    }
    fn zone_name(&self) -> Option<String> {
        self.inner.zone_name()
    }
    fn local_at(&self, ms: i64) -> NaiveDateTime {
        self.inner.local_at(ms)
    }
    fn to_ms(&self, local: NaiveDateTime) -> Option<i64> {
        self.inner.to_ms(local)
    }
}

struct TestEnv {
    clock: TestClock,
    store: QuestionStore,
    project: PathBuf,
    status: RefCell<String>,
    present: Cell<bool>,
    published: RefCell<Vec<OperationSnapshot>>,
    marks: RefCell<Vec<String>>,
    limit_log: RefCell<Vec<crate::plans_report::limit_log::Entry>>,
}

impl TestEnv {
    fn new() -> Self {
        let inner = FixedClock::taipei_noon();
        Self {
            clock: TestClock { now: Cell::new(inner.now_ms), inner },
            store: QuestionStore::new(),
            project: std::env::temp_dir().join(format!("pm-handoff-{}", uuid::Uuid::new_v4())),
            status: RefCell::new("idle".into()),
            present: Cell::new(true),
            published: RefCell::new(Vec::new()),
            marks: RefCell::new(Vec::new()),
            limit_log: RefCell::new(Vec::new()),
        }
    }

    fn advance(&self, ms: i64) {
        self.clock.now.set(self.clock.now.get() + ms);
    }

    fn now(&self) -> u64 {
        self.clock.now.get() as u64
    }

    fn pending(&self) -> Vec<crate::questions::Question> {
        self.store.list_pending(self.now())
    }
}

impl Env for TestEnv {
    fn pane(&self, _pane_id: &str) -> Option<PaneCtx> {
        self.present.get().then(|| PaneCtx {
            agent_id: "claude".into(),
            agent_label: "Claude Code".into(),
            account: Some("acct-work".into()),
            project: self.project.to_string_lossy().into_owned(),
            status: self.status.borrow().clone(),
        })
    }
    fn questions(&self) -> &QuestionStore {
        &self.store
    }
    fn clock(&self) -> &dyn Clock {
        &self.clock
    }
    fn publish(&self, snapshot: &OperationSnapshot) {
        self.published.borrow_mut().push(snapshot.clone());
    }
    fn mark(&self, _pane_id: &str, text: &str) {
        self.marks.borrow_mut().push(text.to_string());
    }
    fn log_limit(&self, entry: crate::plans_report::limit_log::Entry) {
        self.limit_log.borrow_mut().push(entry);
    }
}

const PANE: &str = "pane-1";

/// A run bound to the pane, in the state a mid-turn run is in.
fn running_run(env: &TestEnv) -> OperationSnapshot {
    let request = crate::operations::DelegateWorkRequest {
        project_path: env.project.to_string_lossy().into_owned(),
        task: "Make the cart total round to cents".into(),
        agent_type: "claude".into(),
        pane_id: Some(PANE.into()),
        idempotency_key: uuid::Uuid::new_v4().to_string(),
        timeout_ms: 600_000,
        owner_session_id: Some("handoff-test".into()),
        ..Default::default()
    };
    let (op, _) = crate::operations::create_operation(request).unwrap();
    for status in [OperationStatus::Starting, OperationStatus::Running] {
        crate::operations::mark_operation_state(&op.project_path, &op.operation_id, status, StateSource::Inferred, Some("running".into()), None, None).unwrap();
    }
    crate::operations::get_operation(&op.project_path, &op.operation_id).unwrap()
}

fn stored(op: &OperationSnapshot) -> OperationSnapshot {
    crate::operations::get_operation(&op.project_path, &op.operation_id).unwrap()
}

fn claude_screen(id: &str) -> String {
    let f = fixture("claude", id);
    render(&f.text, cols_of(&f))
}

fn alive(_: &str) -> bool {
    true
}

/// Observe the limit screen, let it hold still, and return the fired state.
fn hit_the_limit(env: &TestEnv, engine: &mut Engine, screen: &str) {
    let claude = profile_for("claude");
    assert_eq!(engine.observe(env, PANE, claude.as_ref(), screen), Standing::Pending);
    env.advance(1_900);
    engine.tick(env, &alive);
    assert!(env.pending().is_empty(), "not before it has held for 2 s");
    env.advance(200);
    engine.tick(env, &alive);
}

#[test]
fn a_usage_limit_mid_turn_parks_the_run_as_waiting_input_and_raises_one_card() {
    let env = TestEnv::new();
    let mut engine = Engine::default();
    let run = running_run(&env);
    hit_the_limit(&env, &mut engine, &claude_screen("usage_session_window"));

    // The run reports a typed state: waiting_input, stage usage_limit, the reset time on the action.
    let parked = stored(&run);
    assert_eq!(parked.status, OperationStatus::WaitingInput);
    assert_eq!(parked.stage.as_deref(), Some("usage_limit"));
    let action = parked.required_action.clone().expect("a required action");
    assert_eq!(action["kind"], "usage_limit");
    assert_eq!(action["window"], "session limit");
    assert_eq!(action["reset_text"], "5pm (Asia/Taipei)");
    assert_eq!(when_iso(action["reset_at_ms"].as_u64().unwrap() as i64, env.clock()), "2026-10-15T17:00");
    assert_eq!(action["self_resume_armed"], false);
    assert_eq!(action["options"], serde_json::json!([]), "nothing to choose, nothing to answer");
    assert_eq!(action["summary"], "Claude Code ran out of usage. It can continue at 17:00.");
    assert_eq!(env.published.borrow().len(), 1, "the new state is published once");
    assert!(env.marks.borrow().iter().any(|m| m.starts_with("limit:detected window session limit")), "{:?}", env.marks.borrow());

    // One Inbox card: what happened, when it can continue, two choices, and the hand-off is later.
    let cards = env.pending();
    assert_eq!(cards.len(), 1);
    let card = &cards[0];
    assert_eq!(card.question, "Claude Code ran out of usage. It can continue at 17:00.");
    assert_eq!(card.choices.iter().map(|c| (c.id.as_str(), c.label.as_str())).collect::<Vec<_>>(), vec![("remind", "Remind me at 17:00"), ("stop", "Stop here")]);
    assert!(card.context.as_deref().unwrap().contains("coming in a later update"), "{:?}", card.context);
    assert!(!card.allow_free_text);
    let info = card.limit.as_ref().expect("a limit card");
    assert!(info.has_run && !info.self_resume_armed);
    assert_eq!(card.pane_id.as_deref(), Some(PANE));
    assert!(card.consent.is_none());
    for choice in &card.choices {
        assert!(!choice.label.to_lowercase().contains("hand"), "no hand-off button in this step: {}", choice.label);
        assert!(!choice.label.to_lowercase().contains("continue with"), "{}", choice.label);
    }
    // Never a suggestion to use another login of the same vendor (kill list: rotating logins at a limit).
    let words = format!("{} {} {}", card.question, card.context.clone().unwrap_or_default(), card.choices.iter().map(|c| c.label.as_str()).collect::<Vec<_>>().join(" ")).to_lowercase();
    for forbidden in ["login", "account", "personal", "work profile", "switch to", "use your"] {
        assert!(!words.contains(forbidden), "the card names `{forbidden}`: {words}");
    }

    // Seeing the same limit again never raises a second card or a second state change.
    let claude = profile_for("claude");
    env.advance(5_000);
    engine.observe(&env, PANE, claude.as_ref(), &claude_screen("usage_session_window"));
    engine.tick(&env, &alive);
    assert_eq!(env.pending().len(), 1);
    assert_eq!(env.published.borrow().len(), 1);
}

#[test]
fn the_card_says_claude_continues_on_its_own_when_its_own_wait_is_armed() {
    let env = TestEnv::new();
    let mut engine = Engine::default();
    running_run(&env);
    hit_the_limit(&env, &mut engine, &claude_screen("usage_armed_own_wait"));
    let cards = env.pending();
    assert_eq!(cards.len(), 1);
    assert!(cards[0].limit.as_ref().unwrap().self_resume_armed);
    assert!(cards[0].context.as_deref().unwrap().starts_with("Claude Code is set to continue on its own."), "{:?}", cards[0].context);
    // Respecting the vendor's wait: the only labels are "Remind me" and "Stop here"; nothing offers to
    // cancel, replace or press its wait.
    let labels: Vec<_> = cards[0].choices.iter().map(|c| c.label.clone()).collect();
    assert_eq!(labels, vec!["Remind me at 17:00".to_string(), "Stop here".to_string()]);
}

#[test]
fn a_limit_with_no_printed_reset_says_so_and_offers_a_reminder_in_an_hour() {
    let env = TestEnv::new();
    let mut engine = Engine::default();
    running_run(&env);
    hit_the_limit(&env, &mut engine, &claude_screen("usage_opus_no_reset"));
    let card = env.pending().remove(0);
    assert_eq!(card.question, "Claude Code ran out of usage. It didn't say when it can continue.");
    assert_eq!(card.choices[0].label, "Remind me in 1 hour");
}

#[test]
fn a_limit_in_a_pane_without_a_run_still_raises_a_card_but_has_nothing_to_stop() {
    let env = TestEnv::new();
    let mut engine = Engine::default(); // no run bound to the pane
    hit_the_limit(&env, &mut engine, &claude_screen("usage_session_window"));
    let card = env.pending().remove(0);
    assert!(!card.limit.as_ref().unwrap().has_run);
    assert_eq!(card.choices[1].label, "Dismiss");
    assert!(env.published.borrow().is_empty(), "no run, nothing published");
}

#[test]
fn an_agent_stays_known_as_limited_after_its_pane_is_gone_until_the_reset() {
    let env = TestEnv::new();
    let mut engine = Engine::default();
    assert_eq!(engine.agent_limit("claude", Some("acct-work"), env.now()), None, "nothing seen yet");
    hit_the_limit(&env, &mut engine, &claude_screen("usage_session_window"));
    let reset = engine.agent_limit("claude", Some("acct-work"), env.now()).expect("limited").expect("it printed a reset time");
    assert_eq!(when_iso(reset as i64, env.clock()), "2026-10-15T17:00");
    assert_eq!(engine.agent_limit("other", Some("acct-work"), env.now()), None, "only that agent");

    // The pane is closed (a companion moved away from it): the tracker forgets the pane, the agent stays known.
    engine.tick(&env, &|_| false);
    assert!(engine.agent_limit("claude", Some("acct-work"), env.now()).is_some());

    // At the reset it is not limited any more.
    assert_eq!(engine.agent_limit("claude", Some("acct-work"), reset), None);
}

#[test]
fn an_agent_is_not_limited_any_more_once_a_pane_of_it_carries_on() {
    let env = TestEnv::new();
    let mut engine = Engine::default();
    hit_the_limit(&env, &mut engine, &claude_screen("usage_session_window"));
    assert!(engine.agent_limit("claude", Some("acct-work"), env.now()).is_some());
    // The limit text is gone from the screen: the agent works again.
    let claude = profile_for("claude");
    env.advance(1_000);
    engine.observe(&env, PANE, claude.as_ref(), "> hello
");
    engine.tick(&env, &alive);
    assert_eq!(engine.agent_limit("claude", Some("acct-work"), env.now()), None);
}

#[test]
fn a_limit_that_prints_no_reset_is_remembered_for_an_hour() {
    let env = TestEnv::new();
    let mut engine = Engine::default();
    hit_the_limit(&env, &mut engine, &claude_screen("usage_opus_no_reset"));
    assert_eq!(engine.agent_limit("claude", Some("acct-work"), env.now()), Some(None));
    assert_eq!(engine.agent_limit("claude", Some("acct-work"), env.now() + card::REMIND_FALLBACK_MS + 1), None);
}

#[test]
fn stop_here_ends_the_run_keeps_the_pane_and_writes_the_receipt_line() {
    let env = TestEnv::new();
    let mut engine = Engine::default();
    let run = running_run(&env);
    hit_the_limit(&env, &mut engine, &claude_screen("usage_session_window"));
    let card = env.pending().remove(0);
    let answered = env.store.answer(&card.id, Some("stop"), None, "desktop", env.now()).unwrap();
    assert!(answered.accepted);
    engine.resolved(&env, &answered.question);

    let ended = stored(&run);
    assert_eq!(ended.status, OperationStatus::Cancelled);
    assert_eq!(ended.stage.as_deref(), Some("stopped_at_usage_limit"));
    assert!(ended.keep_pane, "the agent's pane (its own wait, its screen, its transcript) stays");
    assert!(ended.required_action.is_none());
    let error = ended.error.clone().expect("typed error");
    assert_eq!(error.code, "USAGE_LIMIT_STOPPED");
    assert_eq!(error.context["reset_text"], "5pm (Asia/Taipei)");
    assert!(env.marks.borrow().iter().any(|m| m == "limit:stopped-by-person"));
    // The receipt line: "Stopped at a usage limit (resets 17:00)".
    let limit = crate::agent_runs::handoff_seams::receipt_usage_limit(&ended).expect("the receipt carries the limit");
    assert_eq!(limit.state, "stopped");
    assert_eq!(limit.reset_text.as_deref(), Some("5pm (Asia/Taipei)"));
    assert_eq!(when_iso(limit.reset_at_ms.unwrap() as i64, env.clock()), "2026-10-15T17:00");
    assert_eq!(limit.window.as_deref(), Some("session limit"));
}

#[test]
fn a_run_waiting_at_the_limit_shows_it_on_its_receipt_too() {
    let env = TestEnv::new();
    let mut engine = Engine::default();
    let run = running_run(&env);
    hit_the_limit(&env, &mut engine, &claude_screen("usage_session_window"));
    let limit = crate::agent_runs::handoff_seams::receipt_usage_limit(&stored(&run)).unwrap();
    assert_eq!(limit.state, "waiting");
    // A run that never hit a limit has none.
    let other_env = TestEnv::new();
    let other = running_run(&other_env);
    assert!(crate::agent_runs::handoff_seams::receipt_usage_limit(&stored(&other)).is_none());
}

#[test]
fn remind_me_raises_a_reminder_at_the_reset_and_not_before_and_survives_a_restart() {
    let env = TestEnv::new();
    let file = std::env::temp_dir().join(format!("pm-reminders-{}.json", uuid::Uuid::new_v4()));
    let mut engine = Engine::with_reminders_file(file.clone());
    let run = running_run(&env);
    hit_the_limit(&env, &mut engine, &claude_screen("usage_session_window"));
    let card = env.pending().remove(0);
    let answered = env.store.answer(&card.id, Some("remind"), None, "desktop", env.now()).unwrap();
    engine.resolved(&env, &answered.question);
    assert_eq!(engine.reminders().len(), 1);
    assert_eq!(when_iso(engine.reminders()[0].due_ms as i64, env.clock()), "2026-10-15T17:00");
    assert_eq!(stored(&run).status, OperationStatus::WaitingInput, "remind me does not touch the run");

    // A restart: the reminders are read back from the file.
    let mut engine = Engine::with_reminders_file(file.clone());
    assert_eq!(engine.reminders().len(), 1);
    let before = env.pending().len();
    env.advance(4 * 3_600_000); // 16:00
    engine.tick(&env, &alive);
    assert_eq!(env.pending().len(), before, "an hour early: nothing yet");
    env.advance(3_600_000 + 1_000); // 17:00:01
    engine.tick(&env, &alive);
    let reminder = env.pending().into_iter().find(|q| q.limit.as_ref().is_some_and(|l| l.kind == "reset_reminder")).expect("the reminder card");
    assert_eq!(reminder.question, "Claude Code's usage limit should have reset by now.");
    assert_eq!(reminder.choices.len(), 1);
    assert!(engine.reminders().is_empty(), "raised once");
    assert_eq!(std::fs::read_to_string(&file).unwrap().trim(), "[]");
    let _ = std::fs::remove_file(file);
}

#[test]
fn a_reminder_is_skipped_when_the_agent_already_carried_on_or_the_pane_is_gone() {
    let env = TestEnv::new();
    let mut engine = Engine::default();
    engine.reminders.push(Reminder { pane_id: PANE.into(), agent_label: "Claude Code".into(), window: None, due_ms: env.now() + 10 });
    *env.status.borrow_mut() = "running".into(); // working again, no limit on screen
    env.advance(1_000);
    engine.tick(&env, &alive);
    assert!(env.pending().is_empty(), "it continued by itself: nothing to remind about");
    engine.reminders.push(Reminder { pane_id: PANE.into(), agent_label: "Claude Code".into(), window: None, due_ms: env.now() });
    env.present.set(false);
    engine.tick(&env, &alive);
    assert!(env.pending().is_empty(), "the pane is gone");
}

#[test]
fn when_the_agent_carries_on_by_itself_the_card_goes_and_the_run_is_running_again() {
    let env = TestEnv::new();
    let mut engine = Engine::default();
    let run = running_run(&env);
    hit_the_limit(&env, &mut engine, &claude_screen("usage_armed_own_wait"));
    assert_eq!(stored(&run).status, OperationStatus::WaitingInput);
    let card_id = env.pending()[0].id.clone();

    // Reset time: Claude continues on its own. The detector sees a working screen (guard 2).
    env.advance(5 * 3_600_000);
    *env.status.borrow_mut() = "running".into();
    let claude = profile_for("claude");
    assert_eq!(engine.observe(&env, PANE, claude.as_ref(), &claude_screen("neg_busy_with_notice")), Standing::Clear);
    assert!(env.pending().is_empty(), "the card is withdrawn");
    assert_eq!(env.store.get(&card_id, env.now()).unwrap().state, QuestionState::Cancelled);
    let resumed = stored(&run);
    assert_eq!(resumed.status, OperationStatus::Running, "the run follows the pane again");
    assert!(resumed.required_action.is_none());
    assert!(env.marks.borrow().iter().any(|m| m.contains("limit:cleared") && m.contains("Resumed")), "{:?}", env.marks.borrow());

    // The turn ends with the old notice still above: not a new limit, no new card.
    *env.status.borrow_mut() = "idle".into();
    env.advance(10_000);
    engine.observe(&env, PANE, claude.as_ref(), &claude_screen("usage_armed_own_wait"));
    engine.tick(&env, &alive);
    assert!(env.pending().is_empty());
}

#[test]
fn a_transient_rate_limit_raises_no_card_and_leaves_the_run_alone() {
    let env = TestEnv::new();
    let mut engine = Engine::default();
    let run = running_run(&env);
    let claude = profile_for("claude");
    for id in ["transient_retrying", "warning_used_pct"] {
        engine.observe(&env, PANE, claude.as_ref(), &claude_screen(id));
        env.advance(600_000);
        engine.tick(&env, &alive);
    }
    assert!(env.pending().is_empty());
    assert_eq!(stored(&run).status, OperationStatus::Running);
    assert!(env.published.borrow().is_empty());
    // Only a status line says so, in plain words, and only while the retry is on screen.
    engine.observe(&env, PANE, claude.as_ref(), &claude_screen("transient_retrying"));
    let (kind, text) = engine.status_line(&env, PANE).expect("a status line");
    assert_eq!(kind, crate::tui::ActivityKind::Working);
    assert_eq!(text, "Claude Code is busy at its end · retrying");
    assert!(!text.to_lowercase().contains("rate limit"), "chrome never says rate limit");
    engine.observe(&env, PANE, claude.as_ref(), &claude_screen("warning_used_pct"));
    assert!(engine.status_line(&env, PANE).is_none());
}

#[test]
fn the_pane_status_line_says_where_the_agent_stopped() {
    let env = TestEnv::new();
    let mut engine = Engine::default();
    running_run(&env);
    hit_the_limit(&env, &mut engine, &claude_screen("usage_session_window"));
    let (kind, text) = engine.status_line(&env, PANE).unwrap();
    assert_eq!(kind, crate::tui::ActivityKind::Waiting);
    assert_eq!(text, "Claude Code hit its usage limit · resets 17:00");
}

#[test]
fn an_agent_without_limit_rules_is_never_reacted_to() {
    let env = TestEnv::new();
    let mut engine = Engine::default();
    let shell = profile_for("shell");
    let screen = claude_screen("usage_session_window");
    assert_eq!(engine.observe(&env, PANE, shell.as_ref(), &screen), Standing::Clear);
    env.advance(60_000);
    engine.tick(&env, &alive);
    assert!(env.pending().is_empty());
}

#[test]
fn reply_text_that_quotes_the_limit_never_raises_a_card() {
    let env = TestEnv::new();
    let mut engine = Engine::default();
    let run = running_run(&env);
    let claude = profile_for("claude");
    for id in ["neg_reply_quotes_limit", "neg_prompt_echo_pasted", "neg_tool_output_diff"] {
        engine.observe(&env, PANE, claude.as_ref(), &claude_screen(id));
        env.advance(60_000);
        engine.tick(&env, &alive);
    }
    assert!(env.pending().is_empty());
    assert_eq!(stored(&run).status, OperationStatus::Running);
}

// ---- the never-type guard ---------------------------------------------------------------------------

#[test]
fn a_usage_limit_is_not_an_answerable_prompt() {
    let env = TestEnv::new();
    let mut engine = Engine::default();
    let run = running_run(&env);
    hit_the_limit(&env, &mut engine, &claude_screen("usage_own_wait_off_menu"));
    let action = stored(&run).required_action.unwrap();
    let refused = refuse_answer(&action).expect("answer_prompt is refused for a usage limit");
    assert_eq!(refused.code, "USAGE_LIMIT_NOT_ANSWERABLE");
    assert_eq!(refused.context["sent_input"], false);
    assert!(refuse_answer(&serde_json::json!({"kind": "command_approval"})).is_none(), "other prompts are unchanged");
    // And the MCP view keeps the limit readable for a waiting caller (kind, reset) with no options.
    let compact = crate::agent_runs::handoff_seams::compact_prompt(action);
    assert_eq!(compact["kind"], "usage_limit");
    assert_eq!(compact["reset_text"], "5pm (Asia/Taipei)");
    assert_eq!(compact["options"], serde_json::json!([]));
}

#[test]
fn no_task_is_ever_typed_into_a_pane_at_its_usage_limit_or_its_limit_menu() {
    use crate::agent_runs::readiness::{assess, Observation, Readiness};
    for (agent, id) in [
        ("claude", "usage_session_window"),
        ("claude", "usage_armed_own_wait"),
        ("claude", "usage_own_wait_off_menu"),
        ("codex", "usage_generic_same_day"),
        ("codex", "menu_approaching"),
    ] {
        let f = fixture(agent, id);
        let verdict = assess(profile_for(agent).as_ref(), Observation::screen_only(render(&f.text, cols_of(&f))));
        let Readiness::Gate(gate) = verdict.readiness() else { panic!("{agent}/{id}: {:?}", verdict.readiness()) };
        if !id.contains("menu") {
            // (A numbered menu may also read as the agent's own prompt: either way it is a gate.)
            assert_eq!(gate.id, "usage_limit", "{agent}/{id}");
        }
        assert!(gate.dismiss.is_none(), "no keys are declared to move past a limit");
        assert!(verdict.typing_permit().is_none(), "{agent}/{id}: a task permit was issued");
        assert!(verdict.gate_permit().is_none(), "{agent}/{id}: a gate permit was issued");
    }
    // The same pane without the limit on screen is ready as before.
    let f = fixture("claude", "neg_reply_quotes_limit");
    let verdict = assess(profile_for("claude").as_ref(), Observation::screen_only(render(&f.text, 120)));
    assert_eq!(verdict.readiness(), &Readiness::Ready);
}

/// The detect-only module has no way to reach a pane's input: the `Env` it works through has no
/// write method, and its source names none of the write paths. (A source scan is crude and honest:
/// the day someone adds a key press here, this fails and asks for a design review.)
#[test]
fn the_handoff_module_has_no_way_to_write_to_a_pane() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("handoff");
    let forbidden = [
        "write_input",
        "WriteOrigin",
        "TypingPermit",
        "GatePermit",
        "registry_write",
        "press_key",
        "write_terminal",
        "send_agent",
        "interrupt",
        "\\u{1b}",
        "\\r\"",
    ];
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().is_some_and(|n| n == "tests.rs") || path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for word in forbidden {
            assert!(!text.contains(word), "{} mentions `{word}`: hand-off detect-only never writes to a pane", path.display());
        }
    }
    for agent in ["claude", "codex"] {
        let toml = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("tui").join(agent).join("limits.toml")).unwrap();
        assert!(!toml.contains("keys"), "{agent}/limits.toml declares keys");
    }
}

// ---- the card's own channel -----------------------------------------------------------------------------

#[test]
fn plain_loopback_http_cannot_answer_a_limit_card_but_a_phone_can() {
    let env = TestEnv::new();
    let card = super::card::limit_card(&usage("z"), "Claude Code", PANE, true, env.clock());
    // The routes use the process-wide store.
    let created = crate::questions::store().create(card, crate::questions::now_ms()).unwrap();
    let path = format!("/questions/{}/answer", created.id);
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let body = serde_json::json!({"choice_id": "stop"});
    let (status, reply) = crate::questions::routes::handle("POST", &segments, &body, None, false, &|| {}).unwrap().unwrap_err();
    assert_eq!(status, 403, "{reply}");
    assert_eq!(reply["code"], "CONSENT_NEEDS_PERSON");
    assert_eq!(crate::questions::store().get(&created.id, crate::questions::now_ms()).unwrap().state, QuestionState::Pending);
    let (status, _) = crate::questions::routes::handle("POST", &segments, &body, None, true, &|| {}).unwrap().unwrap();
    assert_eq!(status, 200, "a paired phone is the person");
}

#[test]
fn an_agent_cannot_raise_a_limit_card_through_ask_user() {
    let body = serde_json::json!({"question": "Claude Code ran out of usage.", "limit": {"kind": "usage_limit", "agent_label": "x"}});
    let (status, reply) = crate::questions::routes::handle("POST", &["questions", "ask"], &body, None, false, &|| {}).unwrap().unwrap();
    assert_eq!(status, 200);
    let id = reply["question_id"].as_str().unwrap().to_string();
    let created = crate::questions::store().get(&id, crate::questions::now_ms()).unwrap();
    assert!(created.limit.is_none() && created.consent.is_none());
    let _ = crate::questions::store().dismiss(&id, crate::questions::now_ms());
}

#[test]
fn a_limit_card_outlives_the_hour_an_agent_question_may_wait() {
    let env = TestEnv::new();
    let card = super::card::limit_card(&usage("z"), "Claude Code", PANE, true, env.clock());
    let created = env.store.create(card, env.now()).unwrap();
    assert!(created.expires_at_ms - created.created_at_ms > 3_600_000);
    let plain = crate::questions::NewQuestion { question: "q".into(), timeout_s: Some(7_200), ..Default::default() };
    assert_eq!(env.store.create(plain, env.now()).unwrap_err().code, "INVALID_ARGUMENT");
}

#[test]
fn the_wire_shape_of_a_limit_card_is_what_the_inbox_reads() {
    let env = TestEnv::new();
    let card = super::card::limit_card(&usage("z"), "Claude Code", PANE, true, env.clock());
    let created = env.store.create(card, env.now()).unwrap();
    let json = serde_json::to_value(&created).unwrap();
    assert_eq!(json["limit"]["kind"], "usage_limit");
    assert_eq!(json["limit"]["agent_label"], "Claude Code");
    assert_eq!(json["limit"]["window"], "session limit");
    assert_eq!(json["limit"]["reset_at_ms"], 1_000);
    assert_eq!(json["limit"]["has_run"], true);
    assert!(json.get("consent").is_none());
}

// ---- Claude's own wait is left alone ------------------------------------------------------------------

#[test]
fn the_same_limit_has_one_identity_whichever_line_shows_it() {
    // The notice alone, the notice plus the agent's own "continuing automatically" line, and the
    // wait line alone are one limit (what ran out, when it resets): a stale notice left above a
    // finished turn must not look like a new limit after the agent carried on.
    let clock = clock_of(&fixture("claude", "usage_session_window"));
    let hit = |id: &str| {
        let f = fixture("claude", id);
        profile_for("claude").detect_limit(&render(&f.text, 120), &clock).unwrap()
    };
    let notice = hit("usage_session_window");
    let armed = hit("usage_armed_own_wait");
    assert_ne!(notice.rule_id, armed.rule_id);
    assert_eq!(notice.screen_sha, armed.screen_sha);
    assert_ne!(notice.screen_sha, hit("usage_weekly_other_day").screen_sha, "another window is another limit");
}

#[test]
fn claudes_armed_wait_is_detected_but_nothing_in_the_detect_path_can_cancel_it() {
    // The armed line is how Puppet Master knows Claude will continue by itself. It is read, never
    // pressed: the `self_resume_armed` flag is the only thing that leaves the detector.
    let f = fixture("claude", "usage_armed_own_wait");
    let hit = profile_for("claude").detect_limit(&render(&f.text, 120), &clock_of(&f)).unwrap();
    assert!(hit.self_resume_armed);
    assert_eq!(hit.rule_id, "self_resume_armed", "an armed rule leads, so the card tells the truth");
    let json = serde_json::to_string(&hit).unwrap();
    assert!(!json.contains("keys") && !json.contains("cancel"), "{json}");
}

// ---- the run supervisor's hold -------------------------------------------------------------------------

#[test]
fn the_supervisor_holds_a_run_on_a_limit_screen_instead_of_reading_the_limit_as_its_answer() {
    let limit = claude_screen("usage_session_window");
    // The hazard: judged like any idle screen, the notice is taken for the turn's answer (or the
    // turn is ruled failed). Either way a run that is only waiting out its limit ends wrongly.
    let working = claude_screen("neg_busy_with_notice");
    let baseline = working.replace("esc to interrupt", "");
    let judged = crate::agent_runs::handoff_seams::idle_turn_verdict("claude", &baseline, "Make the cart total round to cents", &working, &limit);
    assert!(
        matches!(judged, "complete" | "ambiguous_idle" | "not_submitted"),
        "the turn watch alone ends the turn on the limit screen: {judged}"
    );
    // The hold (`Supervisor::observe_pane` asks first and returns without judging): a limit on screen
    // is not `Clear`, from its first sight.
    let pane = format!("hold-{}", uuid::Uuid::new_v4());
    assert_ne!(observe_pane(&pane, "claude", &limit), Standing::Clear);
    // A screen that only quotes the limit is not held.
    let quoting = format!("hold-{}", uuid::Uuid::new_v4());
    assert_eq!(observe_pane(&quoting, "claude", &claude_screen("neg_reply_quotes_limit")), Standing::Clear);
    // An agent without limit rules is not held.
    assert_eq!(observe_pane(&format!("hold-{}", uuid::Uuid::new_v4()), "shell", &limit), Standing::Clear);
}

#[test]
fn the_pane_status_observer_keeps_a_run_parked_at_a_limit_while_the_limit_is_on_screen() {
    // `bridge::observe` records the run from the pane's status on every status change. Without the
    // limit it reads "idle" and un-parks the run (back to running); with `active_limit_action` it
    // passes the limit as the required action and the run stays parked at its own stage.
    let env = TestEnv::new();
    let mut engine = Engine::default();
    let run = running_run(&env);
    assert!(engine.parked_action(&env, PANE).is_none(), "nothing parked before the limit holds");
    hit_the_limit(&env, &mut engine, &claude_screen("usage_session_window"));
    let action = engine.parked_action(&env, PANE).expect("parked");
    assert_eq!(action["kind"], "usage_limit");

    // What the observer does on an "idle" status change when it knows nothing about the limit.
    let un_parked = crate::operations::mark_operation_observation(&run.project_path, &run.operation_id, Some("idle".into()), None, StateSource::Inferred).unwrap();
    assert_eq!(un_parked.status, OperationStatus::Running, "without the hook the run is un-parked");

    // Park it again (the engine's own step), then the observer with the hook.
    crate::operations::mark_operation_usage_limit(&run.project_path, &run.operation_id, action.clone()).unwrap();
    let kept = crate::operations::mark_operation_observation(&run.project_path, &run.operation_id, Some("waiting_input".into()), Some(action), StateSource::Inferred).unwrap();
    assert_eq!(kept.status, OperationStatus::WaitingInput);
    assert_eq!(kept.stage.as_deref(), Some("usage_limit"), "the limit keeps its own stage");
    assert_eq!(kept.required_action.as_ref().unwrap()["kind"], "usage_limit");

    // Once the limit is gone from the screen nothing is parked any more.
    let claude = profile_for("claude");
    engine.observe(&env, PANE, claude.as_ref(), &claude_screen("neg_busy_with_notice"));
    assert!(engine.parked_action(&env, PANE).is_none());
}

/// Guard 1 counts non-blank rows: a CLI that draws its input box at the bottom of the screen leaves
/// blank rows between the notice and the box, and they must not push the notice out of the tail. Seen
/// through `pm-fake-tui` (`tests/fake_tui_e2e.rs`, `a_usage_limit_is_detected`): Codex's notice above
/// 18 blank rows and the bottom-pinned box was missed when blank rows counted.
#[test]
fn blank_rows_between_the_notice_and_a_bottom_pinned_box_do_not_hide_the_limit() {
    let f = fixture("codex", "usage_generic_same_day");
    let gap = "\n".repeat(18);
    let text = f.text.replacen("Try again at 9:45 PM.\n", &format!("Try again at 9:45 PM.\n{gap}"), 1);
    assert_ne!(text, f.text, "the premise: the notice row is in the fixture");
    let screen = render(&text, cols_of(&f));
    let hit = profile_for("codex").detect_limit(&screen, &clock_of(&f)).expect("the limit above the gap");
    assert_eq!(hit.kind, LimitKind::Usage);
    assert_eq!(hit.reset_text.as_deref(), Some("9:45 PM"));
    // A notice that is really far back (more than `tail_lines` non-blank rows above the box) is history.
    let history: String = (0..14).map(|i| format!("\u{2022} line {i} of the answer\n")).collect();
    let old = f.text.replacen("Try again at 9:45 PM.\n", &format!("Try again at 9:45 PM.\n{history}"), 1);
    assert!(profile_for("codex").detect_limit(&render(&old, cols_of(&f)), &clock_of(&f)).is_none());
}

// ---- the turn's end --------------------------------------------------------------------------------------

/// A pane that goes idle on its limit has ended its turn: the screen is final and still, so the run is
/// parked at once. Before, only the 2 s hold could park it, and a run settled from the idle screen
/// first (it ended `completed`, the notice read as its answer, no `usage_limit` on its receipt).
#[test]
fn a_limit_on_the_final_screen_of_a_turn_parks_the_run_without_the_two_second_hold() {
    let env = TestEnv::new();
    let mut engine = Engine::default();
    let run = running_run(&env);
    let claude = profile_for("claude");
    let screen = claude_screen("usage_session_window");
    *env.status.borrow_mut() = "idle".into();

    assert_eq!(engine.observe_final(&env, PANE, claude.as_ref(), &screen), Standing::Parked);
    // No clock moved, no tick ran: the run is already parked, with the limit on its action.
    let parked = stored(&run);
    assert_eq!(parked.status, OperationStatus::WaitingInput);
    assert_eq!(parked.stage.as_deref(), Some("usage_limit"));
    assert_eq!(parked.required_action.as_ref().unwrap()["kind"], "usage_limit");
    assert_eq!(parked.required_action.as_ref().unwrap()["reset_text"], "5pm (Asia/Taipei)");
    let limit = crate::agent_runs::handoff_seams::receipt_usage_limit(&parked).expect("the receipt carries the limit");
    assert_eq!(limit.state, "waiting");
    // The same card and the same single state change as the mid-turn path.
    let cards = env.pending();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].question, "Claude Code ran out of usage. It can continue at 17:00.");
    assert_eq!(env.published.borrow().len(), 1);
    assert!(env.marks.borrow().iter().any(|m| m.starts_with("limit:detected window session limit")), "{:?}", env.marks.borrow());

    // Looking again (the supervisor looks every tick, and the ticker promotes later) changes nothing.
    assert_eq!(engine.observe_final(&env, PANE, claude.as_ref(), &screen), Standing::Parked);
    env.advance(5_000);
    engine.tick(&env, &alive);
    assert_eq!(env.pending().len(), 1);
    assert_eq!(env.published.borrow().len(), 1);
}

/// The mid-turn path still waits for the hold: a screen that may yet change is not acted on.
#[test]
fn mid_turn_the_same_limit_screen_is_still_only_pending_until_it_holds() {
    let env = TestEnv::new();
    let mut engine = Engine::default();
    let run = running_run(&env);
    let claude = profile_for("claude");
    assert_eq!(engine.observe(&env, PANE, claude.as_ref(), &claude_screen("usage_session_window")), Standing::Pending);
    assert_eq!(stored(&run).status, OperationStatus::Running);
    assert!(env.pending().is_empty());
}

/// Bypassing the hold bypasses nothing else: an agent quoting the limit sentence in its reply, a pasted
/// prompt echo, a diff and a notice on a screen that is still working are not limits at a turn's end.
#[test]
fn the_other_guards_still_hold_on_the_final_screen_of_a_turn() {
    let claude = profile_for("claude");
    for id in ["neg_reply_quotes_limit", "neg_prompt_echo_pasted", "neg_tool_output_diff", "neg_busy_with_notice"] {
        let env = TestEnv::new();
        let mut engine = Engine::default();
        let run = running_run(&env);
        assert_eq!(engine.observe_final(&env, PANE, claude.as_ref(), &claude_screen(id)), Standing::Clear, "{id}");
        assert_eq!(stored(&run).status, OperationStatus::Running, "{id}");
        assert!(env.pending().is_empty(), "{id}");
    }
    // Only a usage limit parks: a transient retry on the final screen is a status line, never a card.
    let env = TestEnv::new();
    let mut engine = Engine::default();
    running_run(&env);
    assert_eq!(engine.observe_final(&env, PANE, claude.as_ref(), &claude_screen("transient_retrying")), Standing::Clear);
    assert!(env.pending().is_empty());
    // A limit the turn already moved past (scrolled-back text) is not a new limit at the next turn's end.
    let mut tracker = Tracker::default();
    let hit = |screen: &str| profile_for("claude").detect_limit(screen, &FixedClock::taipei_noon());
    let screen = claude_screen("usage_session_window");
    assert_eq!(tracker.observe_final(0, "p", hit(&screen), Some(false)).0, Standing::Parked);
    tracker.observe(1_000, "p", None, Some(true));
    let (standing, events) = tracker.observe_final(2_000, "p", hit(&screen), Some(false));
    assert_eq!((standing, events.len()), (Standing::Clear, 0));
}

/// What `Supervisor::observe_pane` calls on an idle pane: the live entry point, no hold.
#[test]
fn the_live_turn_end_entry_point_parks_where_the_mid_turn_one_only_waits() {
    let limit = claude_screen("usage_session_window");
    let mid_turn = format!("turn-end-{}", uuid::Uuid::new_v4());
    assert_eq!(observe_pane(&mid_turn, "claude", &limit), Standing::Pending);
    let ended = format!("turn-end-{}", uuid::Uuid::new_v4());
    assert_eq!(observe_pane_at_turn_end(&ended, "claude", &limit), Standing::Parked);
    let quoting = format!("turn-end-{}", uuid::Uuid::new_v4());
    assert_eq!(observe_pane_at_turn_end(&quoting, "claude", &claude_screen("neg_reply_quotes_limit")), Standing::Clear);
    // An agent without limit rules is never held.
    assert_eq!(observe_pane_at_turn_end(&format!("turn-end-{}", uuid::Uuid::new_v4()), "shell", &limit), Standing::Clear);
}

#[test]
fn a_limit_is_logged_with_its_agent_and_account_and_logged_again_when_it_is_over() {
    use crate::plans_report::limit_log::Entry;
    let env = TestEnv::new();
    let mut engine = Engine::default();
    running_run(&env);
    hit_the_limit(&env, &mut engine, &claude_screen("usage_session_window"));
    let logged = env.limit_log.borrow().clone();
    match &logged[..] {
        [Entry::Start { pane, agent, account, reset_at_ms, .. }] => {
            assert_eq!(pane, PANE);
            assert_eq!(agent, "claude");
            assert_eq!(account.as_deref(), Some("acct-work"));
            assert!(reset_at_ms.is_some(), "the printed reset time is kept");
        }
        other => panic!("one start line expected, got {other:?}"),
    }

    // The turn resumes: the limit is over.
    env.advance(60_000);
    let claude = profile_for("claude");
    let _ = engine.observe(&env, PANE, claude.as_ref(), "");
    let logged = env.limit_log.borrow().clone();
    assert!(matches!(logged.last(), Some(Entry::End { pane, .. }) if pane == PANE), "{logged:?}");
}

fn hit_the_limit_on(env: &TestEnv, engine: &mut Engine, pane: &str, screen: &str) {
    let claude = profile_for("claude");
    assert_eq!(engine.observe(env, pane, claude.as_ref(), screen), Standing::Pending);
    env.advance(1_900);
    engine.tick(env, &alive);
    env.advance(200);
    engine.tick(env, &alive);
}

#[test]
fn a_limit_on_one_account_does_not_show_for_another_account_of_the_same_agent() {
    let env = TestEnv::new();
    let mut engine = Engine::default();
    hit_the_limit(&env, &mut engine, &claude_screen("usage_session_window"));
    assert!(engine.agent_limit("claude", Some("acct-work"), env.now()).is_some());
    // Another plan of Claude (or the agent's own login) is not at its limit.
    assert_eq!(engine.agent_limit("claude", Some("acct-home"), env.now()), None);
    assert_eq!(engine.agent_limit("claude", None, env.now()), None);
}

#[test]
fn a_pane_that_carries_on_forgets_only_its_own_limit() {
    let env = TestEnv::new();
    let mut engine = Engine::default();
    hit_the_limit_on(&env, &mut engine, "pane-1", &claude_screen("usage_session_window"));
    hit_the_limit_on(&env, &mut engine, "pane-2", &claude_screen("usage_session_window"));
    let reset = engine.agent_limit("claude", Some("acct-work"), env.now()).expect("limited").expect("a reset time");
    // The second pane is back at work: the first pane's limit (same agent, same account) still holds.
    let claude = profile_for("claude");
    env.advance(1_000);
    engine.observe(&env, "pane-2", claude.as_ref(), "> hello\n");
    engine.tick(&env, &alive);
    assert_eq!(engine.agent_limit("claude", Some("acct-work"), env.now()), Some(Some(reset)), "pane-1 is still limited");
    // The first pane carries on too: now nothing is known.
    env.advance(1_000);
    engine.observe(&env, "pane-1", claude.as_ref(), "> hello\n");
    engine.tick(&env, &alive);
    assert_eq!(engine.agent_limit("claude", Some("acct-work"), env.now()), None);
}
