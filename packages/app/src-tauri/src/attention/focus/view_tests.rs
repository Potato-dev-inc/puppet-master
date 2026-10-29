use super::model::*;
use super::summary::{EndedBy, FocusSummary};
use super::view::{PhoneSummary, PhoneView, View};

/// 2026-10-14 12:00 UTC, the same fixed time the other focus tests use.
const T0: u64 = 1_791_979_200_000;
const MIN: u64 = MINUTE_MS;

fn running(minutes: u32) -> State {
    State { session: Some(Session::new(T0, minutes, By::Desktop, "fc-0123456789ab".into())), ..State::default() }
}

fn waiter(key: &str, tier: Tier) -> Waiter {
    Waiter { key: key.into(), tier, first_seen_ms: T0 + MIN, nudged: false }
}

fn keys(value: &serde_json::Value) -> Vec<String> {
    let mut out: Vec<String> = value.as_object().expect("an object").keys().cloned().collect();
    out.sort();
    out
}

#[test]
fn off_reads_off_with_the_default_length_and_nothing_held() {
    let state = State::default();
    let view = View::from(&state, T0);
    assert!(!view.desktop.on);
    assert_eq!(view.desktop.session_id, None);
    assert_eq!((view.desktop.started_at_ms, view.desktop.ends_at_ms), (None, None));
    assert_eq!(view.desktop.minutes, DEFAULT_MINUTES);
    assert_eq!((view.desktop.held_total, view.desktop.waiting_quiet, view.desktop.waiting_urgent), (0, 0, 0));
    assert_eq!(view.desktop.settings, Settings::default());
    assert_eq!(view.desktop.last_summary, None);
    assert_eq!(view.phone, PhoneView { on: false, ends_at_ms: None, minutes: DEFAULT_MINUTES, held_total: 0, waiting: 0, last: None });
}

#[test]
fn on_reads_the_session_and_the_held_total() {
    let mut state = running(25);
    state.ledger.counts.companion = 2;
    state.ledger.counts.night = 1;
    let view = View::from(&state, T0 + 10 * MIN);
    assert!(view.desktop.on);
    assert_eq!(view.desktop.session_id.as_deref(), Some("fc-0123456789ab"));
    assert_eq!(view.desktop.started_at_ms, Some(T0));
    assert_eq!(view.desktop.ends_at_ms, Some(T0 + 25 * MIN));
    assert_eq!(view.desktop.minutes, 25);
    assert_eq!((view.desktop.extended, view.desktop.held_total), (0, 3));
    assert_eq!(view.desktop.held, HeldCounts { companion: 2, night: 1, ..Default::default() });
    assert!(view.phone.on);
    assert_eq!(view.phone.ends_at_ms, Some(T0 + 25 * MIN));
    assert_eq!((view.phone.minutes, view.phone.held_total), (25, 3));
}

#[test]
fn the_deadline_itself_is_already_off() {
    let mut state = running(25);
    state.ledger.counts.review = 3;
    let at_end = View::from(&state, T0 + 25 * MIN);
    assert!(!at_end.desktop.on, "now equal to the end reads off");
    assert_eq!(at_end.desktop.held_total, 0, "nothing is held once it reads off");
    assert!(!at_end.phone.on);
}

#[test]
fn a_session_past_its_end_that_no_tick_ended_reads_off() {
    let mut state = running(25);
    state.ledger.waiters.push(waiter("k1", Tier::Quiet));
    let view = View::from(&state, T0 + 26 * MIN);
    assert!(!view.desktop.on, "fails open: the clock decides, not the tick");
    assert_eq!((view.desktop.waiting_quiet, view.desktop.waiting_urgent), (0, 0));
    assert_eq!(view.phone.waiting, 0);
}

#[test]
fn waiting_cards_are_split_into_quiet_and_urgent() {
    let mut state = running(45);
    state.ledger.waiters = vec![waiter("k1", Tier::Quiet), waiter("k2", Tier::Quiet), waiter("k3", Tier::Through)];
    let view = View::from(&state, T0 + MIN);
    assert_eq!((view.desktop.waiting_quiet, view.desktop.waiting_urgent), (2, 1));
    assert_eq!(view.phone.waiting, 3);
}

#[test]
fn the_last_summary_reaches_the_desktop_and_a_counts_only_line_reaches_the_phone() {
    let mut state = State::default();
    let summary = super::model::sample_summary();
    state.last_summary = Some(summary.clone());
    let view = View::from(&state, T0 + 30 * MIN);
    assert_eq!(view.desktop.last_summary, Some(summary.clone()));
    assert_eq!(
        view.phone.last,
        Some(PhoneSummary { ended_at_ms: summary.ended_at_ms, line: summary.line.clone(), ended_by: EndedBy::Timer })
    );
}

#[test]
fn the_desktop_json_uses_the_names_focus_ts_reads() {
    let mut state = running(45);
    state.last_summary = Some(super::model::sample_summary());
    let value = serde_json::to_value(View::from(&state, T0 + MIN).desktop).expect("serializes");
    assert_eq!(
        keys(&value),
        [
            "endsAtMs",
            "extended",
            "held",
            "heldTotal",
            "lastSummary",
            "minutes",
            "on",
            "sessionId",
            "settings",
            "startedAtMs",
            "waitingQuiet",
            "waitingUrgent",
        ]
    );
    assert_eq!(keys(&value["settings"]), ["default_minutes", "quiet_blocking"]);
    assert_eq!(keys(&value["held"]), ["companion", "night", "review", "run_done", "run_failed"]);
    let last = &value["lastSummary"];
    assert_eq!(last["session_id"], "fc-0123456789ab");
    assert_eq!(last["ended_by"], "timer");
    assert_eq!(last["counts"]["still_waiting"], 1);
}

#[test]
fn the_phone_json_has_counts_and_one_line_only() {
    let mut state = running(45);
    state.ledger.counts.companion = 1;
    state.ledger.waiters.push(waiter("k1", Tier::Quiet));
    state.last_summary = Some(FocusSummary { ended_by: EndedBy::Early, ..super::model::sample_summary() });
    let value = serde_json::to_value(View::from(&state, T0 + MIN).phone).expect("serializes");
    assert_eq!(keys(&value), ["endsAtMs", "heldTotal", "last", "minutes", "on", "waiting"]);
    assert_eq!(keys(&value["last"]), ["ended_at_ms", "ended_by", "line"]);
    assert_eq!(value["last"]["ended_by"], "early");
    assert_eq!((value["heldTotal"].as_u64(), value["waiting"].as_u64()), (Some(1), Some(1)));
}

#[test]
fn the_phone_view_carries_no_pane_id_agent_name_or_sentence() {
    let mut state = running(45);
    state.ledger.held.push(Held {
        at_ms: T0 + MIN,
        class: Class::Companion,
        agent: "Helper Bot".into(),
        sentence: Some("Secret sentence about C:\\Users\\you\\project".into()),
    });
    state.ledger.counts.companion = 1;
    state.ledger.waiters.push(waiter("pane-7:approval:k9", Tier::Through));
    state.last_summary = Some(super::model::sample_summary());
    let text = serde_json::to_string(&View::from(&state, T0 + MIN).phone).expect("serializes");
    for hidden in ["Helper Bot", "Secret", "C:\\", "pane-7", "k9", "sentence", "agent", "sessionId", "session_id"] {
        assert!(!text.contains(hidden), "phone JSON must not contain {hidden:?}: {text}");
    }
}
