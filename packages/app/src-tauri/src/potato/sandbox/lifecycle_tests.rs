//! Lazy start, idle stop and the readiness checks, against a fake guest (no WSL, no Linux).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::fakes_tests::FakeGuest;
use super::lifecycle::*;
use super::wsl::SandboxName;

fn rig() -> (Arc<FakeGuest>, Lifecycle) {
    let guest = FakeGuest::with_installed("pm-sbx-main");
    let lifecycle = Lifecycle::new(guest.wsl(), SandboxName::main()).with_timing(Timing::instant());
    (guest, lifecycle)
}

fn pushes(guest: &FakeGuest) -> Vec<String> {
    guest
        .calls()
        .iter()
        .filter(|c| c.args.iter().any(|a| a.contains("control.token")) && c.stdin.is_some())
        .map(|c| String::from_utf8(c.stdin.clone().unwrap()).unwrap())
        .collect()
}

#[test]
fn the_guest_report_is_parsed() {
    let all_up = "boot_done=1\nboot_failed=0\negress_state=passed\negress_reason=\ndesktop_up=1\ncontrol_up=1\n";
    let r = parse_readiness(all_up);
    assert!(r.ready());
    assert_eq!(r.egress, Egress::Passed);
    assert!(r.fatal().is_none());

    let failed = parse_readiness("boot_done=1\nboot_failed=0\negress_state=failed\negress_reason=TCP to 172.20.0.1:445 connected\ndesktop_up=0\ncontrol_up=0\n");
    assert!(!failed.ready());
    assert!(failed.fatal().unwrap().0.contains("network walls"));
    assert!(failed.fatal().unwrap().0.contains("172.20.0.1"));

    let broken = parse_readiness("boot_done=0\nboot_failed=1\n");
    assert!(broken.fatal().unwrap().0.contains("safety check"));

    // nothing printed (an old guest, or a failed script) means nothing is up
    let empty = parse_readiness("");
    assert!(!empty.ready());
    assert_eq!(empty.egress, Egress::Unknown);
    assert_eq!(empty.missing().0, "The sandbox did not finish starting.");
}

#[test]
fn missing_names_the_first_thing_that_is_not_up() {
    let base = Readiness { boot_done: true, boot_failed: false, egress: Egress::Passed, desktop_up: true, control_up: true };
    assert!(base.ready());
    let no_walls = Readiness { egress: Egress::Unknown, ..base.clone() };
    assert!(no_walls.missing().0.contains("network walls"));
    let no_screen = Readiness { desktop_up: false, ..base.clone() };
    assert!(no_screen.missing().0.contains("screen"));
    let no_control = Readiness { control_up: false, ..base };
    assert!(no_control.missing().0.contains("control"));
}

#[test]
fn the_readiness_script_reads_both_boot_flags_and_our_ports() {
    let script = readiness_script(6080, 47801);
    assert!(script.contains("/run/pm-sbx/boot.done"));
    assert!(script.contains("/run/pm-no-wslg.done"));
    assert!(script.contains("/run/pm-sbx/egress-probe.status"));
    assert!(script.contains("listening 6080") && script.contains("listening 47801"));
    // it reads; it never changes anything
    for writer in ["rm ", "kill", "iptables", "tee ", "mkdir", "touch", "chmod", "chown"] {
        assert!(!script.contains(writer), "{writer}");
    }
}

#[test]
fn the_idle_rule_is_pure() {
    let m = 60_000;
    assert!(!should_idle_stop(true, 0, 14 * m, 15, 0));
    assert!(should_idle_stop(true, 0, 15 * m, 15, 0));
    assert!(!should_idle_stop(true, 0, 99 * m, 15, 1), "never while something uses it");
    assert!(!should_idle_stop(false, 0, 99 * m, 15, 0), "nothing to stop when it is not ready");
    // the setting is held between 1 and 240 minutes
    assert_eq!(clamp_idle_minutes(0), 1);
    assert_eq!(clamp_idle_minutes(15), 15);
    assert_eq!(clamp_idle_minutes(100_000), 240);
    assert!(should_idle_stop(true, 0, m, 0, 0), "0 is read as 1 minute, not as never");
}

#[test]
fn nothing_runs_until_a_use_asks() {
    let (guest, lifecycle) = rig();
    assert_eq!(lifecycle.phase(), Phase::Stopped);
    assert!(guest.calls().is_empty(), "making the lifecycle starts nothing");
    assert!(!guest.holder_alive());
}

#[test]
fn a_start_runs_every_check_in_order_and_pushes_the_token_over_stdin() {
    let (guest, lifecycle) = rig();
    let ready = lifecycle.ensure_ready(1_000).unwrap();
    assert_eq!(lifecycle.phase(), Phase::Ready);
    assert_eq!(ready.generation, lifecycle.generation());
    assert!(guest.holder_alive(), "a holder keeps the distro up");

    let joined = guest.joined();
    let at = |needle: &str| joined.iter().position(|c| c.contains(needle)).unwrap_or_else(|| panic!("no call with {needle}: {joined:#?}"));
    assert!(at("--status") < at("--list --verbose"));
    assert!(at("--list --verbose") < at("sleep infinity"));
    assert!(at("sleep infinity") < at("/bin/sh -s"));
    assert!(at("pm-sbx-egress.sh") < at("pm-sbx-egress-probe.sh"));
    assert!(at("pm-sbx-egress-probe.sh") < at("control.token"));
    assert!(at("control.token") < at("pm-sbx-start-desktop"));
    assert!(at("pm-sbx-start-desktop") < at("pm-sbx-control"));

    // the walls and the probe run as root, the desktop and the control service as the pm user
    assert!(joined[at("pm-sbx-egress.sh")].contains("-u root"));
    assert!(joined[at("pm-sbx-start-desktop")].contains("-u pm"));
    assert!(joined[at("pm-sbx-control")].contains("-u pm"));

    // the token: 64 hex characters, in stdin only, never in an argument
    let pushed = pushes(&guest);
    assert_eq!(pushed.len(), 1);
    assert_eq!(pushed[0].len(), 64);
    assert!(pushed[0].bytes().all(|b| b.is_ascii_hexdigit()));
    for call in guest.calls() {
        assert!(call.args.iter().all(|a| !a.contains(&pushed[0])), "the token is in an argument: {:?}", call.args);
    }
    // and it is not in the debug text of what the caller got
    assert!(!format!("{ready:?}").contains(&pushed[0]));
}

#[test]
fn a_second_use_does_not_start_again() {
    let (guest, lifecycle) = rig();
    lifecycle.ensure_ready(1_000).unwrap();
    let before = guest.calls().len();
    let generation = lifecycle.generation();
    lifecycle.ensure_ready(2_000).unwrap();
    assert_eq!(pushes(&guest).len(), 1, "one token for the run");
    assert_eq!(guest.st.lock().holders.len(), 1, "one holder");
    assert_eq!(lifecycle.generation(), generation);
    // it only asked the guest how it is
    let new: Vec<String> = guest.joined()[before..].to_vec();
    assert!(new.iter().all(|c| c.contains("/bin/sh -s")), "{new:?}");
}

#[test]
fn a_distro_left_running_by_an_earlier_app_run_is_stopped_before_the_start() {
    let (guest, lifecycle) = rig();
    guest.st.lock().running.push("pm-sbx-main".to_string());
    guest.st.lock().control_up = true;
    lifecycle.ensure_ready(1_000).unwrap();
    let joined = guest.joined();
    let terminate = joined.iter().position(|c| c == "--terminate pm-sbx-main").expect("terminated first");
    let holder = joined.iter().position(|c| c.contains("sleep infinity")).unwrap();
    assert!(terminate < holder);
}

#[test]
fn stop_ends_the_holder_stops_only_our_distro_and_forgets_the_token() {
    let (guest, lifecycle) = rig();
    lifecycle.ensure_ready(1_000).unwrap();
    let generation = lifecycle.generation();
    lifecycle.stop().unwrap();
    assert_eq!(lifecycle.phase(), Phase::Stopped);
    assert!(!guest.holder_alive());
    assert!(guest.st.lock().running.is_empty());
    assert!(guest.joined().contains(&"--terminate pm-sbx-main".to_string()));
    assert_ne!(lifecycle.generation(), generation, "screen links of the old run die");
    // a start after a stop makes a NEW token
    lifecycle.ensure_ready(5_000).unwrap();
    let pushed = pushes(&guest);
    assert_eq!(pushed.len(), 2);
    assert_ne!(pushed[0], pushed[1]);
    // stopping twice is fine
    lifecycle.stop().unwrap();
    lifecycle.stop().unwrap();
}

#[test]
fn only_wsl_terminate_of_our_name_is_ever_used_to_stop() {
    let (guest, lifecycle) = rig();
    lifecycle.ensure_ready(1).unwrap();
    lifecycle.stop().unwrap();
    for call in guest.calls() {
        let first = call.args.first().map(String::as_str).unwrap_or("");
        assert!(!call.args.iter().any(|a| a == "--shutdown"));
        if first == "--terminate" || first == "-t" {
            assert_eq!(call.args, vec![first.to_string(), "pm-sbx-main".to_string()]);
        }
    }
}

#[test]
fn it_goes_idle_after_the_setting_and_not_while_in_use() {
    let (guest, lifecycle) = rig();
    let m = 60_000;
    lifecycle.ensure_ready(0).unwrap();
    assert!(!lifecycle.idle_check(14 * m).unwrap());
    assert_eq!(lifecycle.phase(), Phase::Ready);
    // a use resets the clock
    lifecycle.touch(10 * m);
    assert!(!lifecycle.idle_check(24 * m).unwrap());
    // something using it holds the stop back
    let guard = lifecycle.begin_use();
    assert!(!lifecycle.idle_check(99 * m).unwrap());
    drop(guard);
    assert!(lifecycle.idle_check(99 * m).unwrap());
    assert_eq!(lifecycle.phase(), Phase::Stopped);
    assert!(!guest.holder_alive());
    // nothing to do when it is already stopped
    assert!(!lifecycle.idle_check(200 * m).unwrap());
}

#[test]
fn the_idle_minutes_setting_is_used() {
    let (_guest, lifecycle) = rig();
    assert_eq!(lifecycle.idle_minutes(), DEFAULT_IDLE_MINUTES);
    lifecycle.set_idle_minutes(2);
    lifecycle.ensure_ready(0).unwrap();
    assert!(!lifecycle.idle_check(119_999).unwrap());
    assert!(lifecycle.idle_check(120_000).unwrap());
    lifecycle.set_idle_minutes(100_000);
    assert_eq!(lifecycle.idle_minutes(), MAX_IDLE_MINUTES);
}

#[test]
fn a_pc_without_the_linux_feature_gets_the_plain_card_and_nothing_starts() {
    let (guest, lifecycle) = rig();
    guest.st.lock().wsl_ok = false;
    let error = lifecycle.ensure_ready(0).unwrap_err();
    assert_eq!(error, LifecycleError::NotAvailable);
    assert_eq!(error.message(), "Needs Windows' Linux feature (admin + restart).");
    assert_eq!(error.code(), "SANDBOX_NO_WSL");
    assert!(!guest.holder_alive());
    assert_eq!(guest.st.lock().elevated_seen, 0, "the feature is never enabled by a start");
}

#[test]
fn a_sandbox_that_was_never_set_up_says_so() {
    let guest = FakeGuest::new();
    let lifecycle = Lifecycle::new(guest.wsl(), SandboxName::main()).with_timing(Timing::instant());
    assert_eq!(lifecycle.ensure_ready(0).unwrap_err(), LifecycleError::NotInstalled);
}

#[test]
fn a_boot_that_never_finishes_fails_with_the_reason_and_is_cleaned_up() {
    let (guest, lifecycle) = rig();
    guest.st.lock().boot_hangs = true;
    let error = lifecycle.ensure_ready(0).unwrap_err();
    assert_eq!(error, LifecycleError::NotReady("The sandbox did not finish starting.".to_string()));
    assert!(!guest.holder_alive(), "the holder is stopped");
    assert!(guest.st.lock().running.is_empty(), "and the distro is stopped again");
    assert!(pushes(&guest).is_empty(), "no token was handed to a sandbox that did not boot");
    assert_eq!(lifecycle.phase(), Phase::Failed("The sandbox did not finish starting.".to_string()));
}

#[test]
fn walls_that_fail_their_probe_stop_the_start_before_any_token_is_pushed() {
    let (guest, lifecycle) = rig();
    guest.st.lock().egress_probe_passes = false;
    let error = lifecycle.ensure_ready(0).unwrap_err();
    match error {
        LifecycleError::NotReady(why) => assert!(why.contains("network walls"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(pushes(&guest).is_empty());
    assert!(!guest.joined().iter().any(|c| c.contains("pm-sbx-start-desktop")), "no screen starts behind failed walls");
    assert!(!guest.holder_alive());
}

#[test]
fn a_boot_hook_that_reports_failure_stops_the_start() {
    let (guest, lifecycle) = rig();
    guest.st.lock().boot_failed = true;
    match lifecycle.ensure_ready(0).unwrap_err() {
        LifecycleError::NotReady(why) => assert!(why.contains("safety check"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(!guest.holder_alive());
}

#[test]
fn a_desktop_that_does_not_start_fails_the_start() {
    let (guest, lifecycle) = rig();
    guest.st.lock().desktop_starts = false;
    assert_eq!(lifecycle.ensure_ready(0).unwrap_err(), LifecycleError::NotReady("The sandbox screen did not start.".to_string()));
    assert!(!guest.joined().iter().any(|c| c.contains("pm-sbx-control")), "the control service is not started without a screen");
    assert!(!guest.holder_alive());
}

struct Visible(AtomicBool);

impl WindowCheck for Visible {
    fn visible_remote_window(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[test]
fn a_visible_remote_desktop_window_fails_the_start() {
    let guest = FakeGuest::with_installed("pm-sbx-main");
    let check = Arc::new(Visible(AtomicBool::new(true)));
    let lifecycle = Lifecycle::new(guest.wsl(), SandboxName::main()).with_timing(Timing::instant()).with_window_check(check.clone());
    assert_eq!(lifecycle.ensure_ready(0).unwrap_err(), LifecycleError::VisibleWindow);
    assert!(!guest.holder_alive());
    check.0.store(false, Ordering::SeqCst);
    assert!(lifecycle.ensure_ready(1).is_ok(), "once the window is gone a start works");
}

#[test]
fn the_error_cards_are_plain() {
    for error in [
        LifecycleError::NotAvailable,
        LifecycleError::NotInstalled,
        LifecycleError::NotReady("The sandbox did not finish starting.".into()),
        LifecycleError::VisibleWindow,
        LifecycleError::Wsl(super::wsl::WslError::Timeout),
        LifecycleError::Wsl(super::wsl::WslError::Failed { code: Some(1), detail: "boom sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghij".into() }),
    ] {
        assert!(error.code().starts_with("SANDBOX_"));
        let message = error.message();
        assert!(message.ends_with('.'), "{message}");
        assert!(!message.contains("sk-ant"), "{message}");
        assert!(!message.to_lowercase().contains("wsl"), "the person sees 'Linux', not 'wsl': {message}");
    }
}
