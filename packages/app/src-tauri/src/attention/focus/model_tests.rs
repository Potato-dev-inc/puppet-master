use super::model::*;
use super::{is_on_at, set_session};

/// 2026-10-14 12:00 UTC, the same fixed time the audit tests use.
const T0: u64 = 1_791_979_200_000;
const MIN: u64 = MINUTE_MS;

fn id() -> String {
    "fc-0123456789ab".into()
}

#[test]
fn session_ends_at_start_plus_the_asked_minutes() {
    let s = Session::new(T0, 45, By::Desktop, id());
    assert_eq!(s.started_at_ms, T0);
    assert_eq!(s.ends_at_ms, T0 + 45 * MIN);
    assert_eq!(s.minutes, 45);
    assert_eq!(s.extended, 0);
    assert_eq!(s.by, By::Desktop);
}

#[test]
fn session_end_is_clamped_to_240_minutes_from_start() {
    let longest = Session::new(T0, 240, By::Phone, id());
    assert_eq!(longest.ends_at_ms, T0 + 240 * MIN, "240 is allowed and ends exactly at the cap");
    assert_eq!(clamp_end(T0, T0 + 999 * MIN), T0 + 240 * MIN, "a later end is cut to the cap");
    assert_eq!(clamp_end(T0, T0 + 240 * MIN + 15 * MIN), T0 + 240 * MIN, "+15 cannot pass the cap either");
    assert_eq!(clamp_end(T0, T0 + 10 * MIN), T0 + 10 * MIN, "an earlier end is kept");
    assert_eq!(clamp_end(u64::MAX - 5, u64::MAX), u64::MAX, "no overflow near the top of u64");
}

#[test]
fn minutes_are_checked_from_5_to_240() {
    assert_eq!(validate_minutes(5), Ok(5));
    assert_eq!(validate_minutes(240), Ok(240));
    for bad in [0, 4, 241, 9_999] {
        let err = validate_minutes(bad).unwrap_err();
        assert_eq!(err.code, "FOCUS_BAD_MINUTES", "{bad} minutes");
    }
}

#[test]
fn presets_and_defaults_match_the_spec() {
    assert_eq!(PRESETS, [25, 45, 90]);
    assert_eq!(DEFAULT_MINUTES, 45);
    assert!(PRESETS.contains(&DEFAULT_MINUTES));
    assert_eq!(Settings::default(), Settings { default_minutes: 45, quiet_blocking: true });
    assert_eq!(State::default().v, FILE_VERSION);
    assert!(State::default().session.is_none());
}

#[test]
fn held_counts_add_each_class_and_total_them() {
    let mut counts = HeldCounts::default();
    assert_eq!(counts.total(), 0);
    counts.add(Class::RunDone);
    counts.add(Class::RunDone);
    counts.add(Class::RunFailed);
    counts.add(Class::Companion);
    counts.add(Class::Review);
    counts.add(Class::Night);
    counts.add(Class::Night);
    assert_eq!(counts.run_done, 2);
    assert_eq!(counts.run_failed, 1);
    assert_eq!(counts.companion, 1);
    assert_eq!(counts.review, 1);
    assert_eq!(counts.night, 2);
    assert_eq!(counts.total(), 7);
}

#[test]
fn held_counts_never_wrap_around() {
    let mut counts = HeldCounts { run_done: u32::MAX, ..HeldCounts::default() };
    counts.add(Class::RunDone);
    assert_eq!(counts.run_done, u32::MAX);
    assert_eq!(counts.total(), u32::MAX);
}

#[test]
fn tier_class_and_by_have_stable_json_names() {
    let tiers = [(Tier::Hold, "\"hold\""), (Tier::Quiet, "\"quiet\""), (Tier::Through, "\"through\"")];
    for (tier, json) in tiers {
        assert_eq!(serde_json::to_string(&tier).unwrap(), json);
        assert_eq!(serde_json::from_str::<Tier>(json).unwrap(), tier);
    }
    let classes = [
        (Class::RunDone, "\"run_done\""),
        (Class::RunFailed, "\"run_failed\""),
        (Class::Companion, "\"companion\""),
        (Class::Review, "\"review\""),
        (Class::Night, "\"night\""),
    ];
    for (class, json) in classes {
        assert_eq!(serde_json::to_string(&class).unwrap(), json);
        assert_eq!(serde_json::from_str::<Class>(json).unwrap(), class);
    }
    let bys = [(By::Desktop, "\"desktop\""), (By::Tray, "\"tray\""), (By::Phone, "\"phone\""), (By::Local, "\"local\"")];
    for (by, json) in bys {
        assert_eq!(serde_json::to_string(&by).unwrap(), json);
        assert_eq!(serde_json::from_str::<By>(json).unwrap(), by);
    }
}

#[test]
fn an_unknown_tier_or_class_name_is_refused() {
    assert!(serde_json::from_str::<Tier>("\"mute\"").is_err());
    assert!(serde_json::from_str::<Class>("\"dnd\"").is_err());
    assert!(serde_json::from_str::<By>("\"agent\"").is_err(), "an agent is never a starter");
}

#[test]
fn a_full_state_survives_a_json_round_trip() {
    let mut ledger = Ledger::default();
    ledger.held.push(Held { at_ms: T0 + MIN, class: Class::RunDone, agent: "Claude".into(), sentence: Some("Done.".into()) });
    ledger.counts.add(Class::RunDone);
    ledger.waiters.push(Waiter { key: "approval:pane-1".into(), tier: Tier::Quiet, first_seen_ms: T0 + 2 * MIN, nudged: false });
    let state = State {
        v: FILE_VERSION,
        settings: Settings { default_minutes: 90, quiet_blocking: false },
        session: Some(Session::new(T0, 25, By::Tray, id())),
        ledger,
        last_summary: Some(sample_summary()),
    };
    let json = serde_json::to_string(&state).unwrap();
    let back: State = serde_json::from_str(&json).unwrap();
    assert_eq!(serde_json::to_string(&back).unwrap(), json, "nothing is lost or renamed");
    assert_eq!(back.settings.default_minutes, 90);
    assert!(!back.settings.quiet_blocking);
    assert_eq!(back.session.as_ref().unwrap().by, By::Tray);
    assert_eq!(back.ledger.waiters[0].tier, Tier::Quiet);
}

#[test]
fn settings_missing_a_field_take_the_defaults() {
    let settings: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(settings, Settings::default());
    let only_length: Settings = serde_json::from_str(r#"{"default_minutes":25}"#).unwrap();
    assert_eq!(only_length.default_minutes, 25);
    assert!(only_length.quiet_blocking);
}

#[test]
fn error_codes_and_messages_are_stable() {
    let bad = FocusError::bad_minutes();
    assert_eq!((bad.code, bad.message.as_str()), ("FOCUS_BAD_MINUTES", "Pick between 5 and 240 minutes."));
    let off = FocusError::not_on();
    assert_eq!((off.code, off.message.as_str()), ("FOCUS_NOT_ON", "Focus is not on."));
    let person = FocusError::person_only();
    assert_eq!(person.code, "FOCUS_PERSON_ONLY");
}

#[test]
fn is_on_follows_the_deadline_and_stops_when_cleared() {
    // One test owns the global flag: it is set and cleared here only.
    let deadline = T0 + 10 * MIN;
    set_session(Some(deadline));
    assert!(is_on_at(T0), "on at the start");
    assert!(is_on_at(deadline - 1), "on one millisecond before the deadline");
    assert!(!is_on_at(deadline), "off exactly at the deadline");
    assert!(!is_on_at(deadline + 1), "off after the deadline, even with no tick");
    assert!(!is_on_at(u64::MAX), "a far-future clock is still off");

    set_session(None);
    assert!(!is_on_at(T0), "off once the session is cleared");
    assert!(!is_on_at(deadline - 1), "clearing wins over the deadline");
}
