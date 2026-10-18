use super::redact::{redact, secret_env_values, test_ctx, MASK};
use super::snapshots::*;
use crate::pty::status::PaneStatus;

mod redact_table {
    use super::*;

    #[test]
    fn table() {
        let ctx = test_ctx();
        let cases: &[(&str, &str)] = &[
            ("key sk-ant-api03-AbCdEf123456 here", "sk-ant"),
            ("sk-proj-1234567890abcdef", "sk-proj"),
            ("token ghp_abcdefghijklmnopqrstuvwxyz0123456789", "ghp_"),
            ("id AKIAIOSFODNN7EXAMPLE end", "AKIA"),
            ("hex 0123456789abcdef0123456789abcdef01234567 x", "0123456789abcdef"),
            ("b64 QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVowMTIzNDU2Nzg5YWJj== x", "QUJDREVG"),
            ("env hunter2-secret value", "hunter2"),
            ("Authorization: Bearer abcdef1234567890xyz", "abcdef1234"),
            ("API_KEY=abcd1234", "abcd1234"),
            ("xoxb-123456-abcdef", "xoxb-"),
        ];
        for (input, needle) in cases {
            let out = redact(input, &ctx);
            assert!(!out.contains(needle), "{input:?} -> {out:?}");
            assert!(out.contains(MASK), "{input:?} -> {out:?}");
        }
    }

    #[test]
    fn home_path_and_user_names() {
        let ctx = test_ctx();
        let out = redact(r"cwd C:\Users\you-pc\Desktop\x and c:/users/you-pc/y and C:\Users\Someone\z", &ctx);
        assert!(!out.to_lowercase().contains("you-pc"), "{out}");
        assert!(!out.contains("Someone"), "{out}");
        assert!(out.contains("USER") || out.contains('~'));
    }

    #[test]
    fn ordinary_screen_text_survives() {
        let ctx = test_ctx();
        let text = "esc interrupt   tab agents  ctrl+p commands\n┃  Ask anything...\n╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀\n=========================================================\n";
        assert_eq!(redact(text, &ctx), text);
    }

    #[test]
    fn env_values_pick_secret_named_and_opaque_only() {
        let env = vec![
            ("OPENAI_API_KEY".to_string(), "abcd".to_string()),
            ("PATH".to_string(), r"C:\a;C:\b".to_string()),
            ("LANG".to_string(), "en_US".to_string()),
            ("SOMETHING".to_string(), "Zm9vYmFyYmF6cXV4".to_string()),
        ];
        let v = secret_env_values(&env);
        assert!(v.contains(&"abcd".to_string()) && v.contains(&"Zm9vYmFyYmF6cXV4".to_string()));
        assert!(!v.iter().any(|x| x.contains("C:") || x == "en_US"));
    }
}

#[test]
fn standby_pane_never_recorded() {
    let err = start("rec-standby-1", "kilo", true, None);
    assert!(err.unwrap_err().starts_with("RECORDER_STANDBY_PANE"));
    assert!(!is_recording("rec-standby-1"));
    assert!(session("rec-standby-1").is_none());
}

#[test]
fn a_recorded_pane_that_becomes_standby_stops_being_recorded() {
    start("rec-standby-2", "kilo", false, None).unwrap();
    assert!(is_recording("rec-standby-2"));
    observe("rec-standby-2", true, PaneStatus::Running, "anything");
    assert!(!is_recording("rec-standby-2"));
    assert!(session("rec-standby-2").unwrap().snapshots.is_empty());
}

#[test]
fn off_unless_started() {
    observe("rec-never-started", false, PaneStatus::Running, "screen");
    assert!(session("rec-never-started").is_none());
}

#[test]
fn start_stop_keeps_the_session_readable() {
    start("rec-ss-1", "kilo", false, Some((PaneStatus::Idle, "ready"))).unwrap();
    assert!(start("rec-ss-1", "kilo", false, None).is_err());
    observe("rec-ss-1", false, PaneStatus::Running, "busy");
    let done = stop("rec-ss-1").unwrap();
    assert_eq!(done.snapshots.len(), 2);
    assert!(!is_recording("rec-ss-1"));
    assert_eq!(session("rec-ss-1").unwrap().snapshots.len(), 2);
    observe("rec-ss-1", false, PaneStatus::WaitingInput, "after stop");
    assert_eq!(session("rec-ss-1").unwrap().snapshots.len(), 2);
}

#[test]
fn snapshot_on_transitions() {
    let mut s = Session::new("p", "kilo", 0);
    let c = test_ctx();
    observe_into(&mut s, PaneStatus::Idle, "ready screen", 0, &c);
    observe_into(&mut s, PaneStatus::Idle, "ready screen 2", 100, &c); // same state: no new snapshot
    observe_into(&mut s, PaneStatus::Running, "busy 1", 200, &c);
    observe_into(&mut s, PaneStatus::Running, "busy 2", 400, &c); // too soon
    observe_into(&mut s, PaneStatus::Running, "busy 3", 2000, &c); // refresh
    observe_into(&mut s, PaneStatus::WaitingInput, "permission required", 2500, &c);
    observe_into(&mut s, PaneStatus::Running, "busy 4", 2600, &c);
    observe_into(&mut s, PaneStatus::Idle, "the reply", 6000, &c);
    let kinds: Vec<_> = s.snapshots.iter().map(|x| x.kind).collect();
    assert_eq!(
        kinds,
        vec![SnapKind::Idle, SnapKind::Working, SnapKind::Working, SnapKind::Prompt, SnapKind::Working, SnapKind::Reply]
    );
    assert_eq!(s.of_kind(SnapKind::Reply), vec!["the reply"]);
}

#[test]
fn snapshots_are_redacted_before_storage() {
    let mut s = Session::new("p", "kilo", 0);
    observe_into(&mut s, PaneStatus::Idle, r"hello sk-ant-api03-AbCdEf123456 in C:\Users\you-pc\x", 0, &test_ctx());
    let text = &s.snapshots[0].text;
    assert!(!text.contains("sk-ant") && !text.contains("you-pc"), "{text}");
}

#[test]
fn per_kind_cap() {
    let mut s = Session::new("p", "kilo", 0);
    for i in 0..40 {
        observe_into(&mut s, PaneStatus::Running, &format!("busy {i}"), i * 2000, &test_ctx());
    }
    assert_eq!(s.snapshots.len(), 8);
}
