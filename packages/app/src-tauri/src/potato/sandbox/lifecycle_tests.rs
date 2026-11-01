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

/// What the app pushed in one start: the control token, the view password and the control password.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Pushed {
    token: String,
    view: String,
    control: String,
}

fn pushes(guest: &FakeGuest) -> Vec<Pushed> {
    guest
        .calls()
        .iter()
        .filter(|c| c.args.iter().any(|a| a.contains("control.token")) && c.stdin.is_some())
        .map(|c| {
            let text = String::from_utf8(c.stdin.clone().unwrap()).unwrap();
            let lines: Vec<&str> = text.lines().collect();
            assert_eq!(lines.len(), 3, "token, view password, control password: {} lines", lines.len());
            Pushed { token: lines[0].to_string(), view: lines[1].to_string(), control: lines[2].to_string() }
        })
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

    // the boot hook's own reason is carried into the card
    let why = parse_readiness("boot_done=0\nboot_failed=1\nboot_reason=pm-no-wslg.sh failed (exit 1)\n");
    assert_eq!(why.boot_reason, "pm-no-wslg.sh failed (exit 1)");
    assert!(why.fatal().unwrap().0.contains("pm-no-wslg.sh failed (exit 1)"), "{:?}", why.fatal());

    // nothing printed (an old guest, or a failed script) means nothing is up
    let empty = parse_readiness("");
    assert!(!empty.ready());
    assert_eq!(empty.egress, Egress::Unknown);
    assert_eq!(empty.missing().0, "The sandbox did not finish starting.");
}

#[test]
fn missing_names_the_first_thing_that_is_not_up() {
    let base = Readiness { boot_done: true, boot_failed: false, boot_reason: String::new(), egress: Egress::Passed, desktop_up: true, control_up: true };
    assert!(base.ready());
    let no_walls = Readiness { egress: Egress::Unknown, ..base.clone() };
    assert!(no_walls.missing().0.contains("network walls"));
    let no_screen = Readiness { desktop_up: false, ..base.clone() };
    assert!(no_screen.missing().0.contains("screen"));
    let no_control = Readiness { control_up: false, ..base };
    assert!(no_control.missing().0.contains("control"));
}

#[test]
fn the_readiness_script_reads_the_one_boot_verdict_and_our_ports() {
    let script = readiness_script(6080, 47801);
    assert!(script.contains("/run/pm-sbx/boot.done"));
    assert!(script.contains("/run/pm-sbx/boot.failed"), "and the reason in it");
    assert!(script.contains("/run/pm-sbx/egress-probe.status"));
    assert!(script.contains("listening 6080") && script.contains("listening 47801"));
    // the duplicate path of the WSLg cut is gone: the boot hook is the only writer of the verdict
    assert!(!script.contains("pm-no-wslg"), "one verdict, not two");
    // it reads; it never changes anything
    for writer in ["rm ", "kill", "iptables", "tee ", "mkdir", "touch", "chmod", "chown"] {
        assert!(!script.contains(writer), "{writer}");
    }
}

#[test]
fn the_app_waits_about_ninety_seconds_for_the_boot_hook() {
    let timing = Timing::default();
    let waited = timing.poll * timing.boot_polls;
    assert!(waited >= std::time::Duration::from_secs(90), "{waited:?}");
    assert!(waited <= std::time::Duration::from_secs(120), "{waited:?}");
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
fn a_start_waits_for_the_boot_verdict_pushes_the_secrets_over_stdin_and_starts_one_script() {
    let (guest, lifecycle) = rig();
    let ready = lifecycle.ensure_ready(1_000).unwrap();
    assert_eq!(lifecycle.phase(), Phase::Ready);
    assert_eq!(ready.generation, lifecycle.generation());
    assert!(guest.holder_alive(), "a holder keeps the distro up");

    let joined = guest.joined();
    let at = |needle: &str| joined.iter().position(|c| c.contains(needle)).unwrap_or_else(|| panic!("no call with {needle}: {joined:#?}"));
    assert!(at("--status") < at("--list --verbose"));
    assert!(at("--list --verbose") < at("sleep infinity"));
    assert!(at("sleep infinity") < at("/bin/sh -s"), "the boot verdict is read after the holder starts the distro");
    assert!(at("/bin/sh -s") < at("control.token"), "secrets only after the boot verdict");
    assert!(at("control.token") < at("pm-sbx-start-desktop"));

    // the app runs no wall and no probe: the guest's boot hook did, and wrote boot.done
    assert!(!joined.iter().any(|c| c.contains("pm-sbx-egress")), "the app must not run the egress scripts: {joined:#?}");
    // and no separate launch of the control service: the desktop script starts it
    assert!(!joined.iter().any(|c| c.contains("pm-sbx-control")), "the control service is started by pm-sbx-start-desktop only: {joined:#?}");
    assert_eq!(joined.iter().filter(|c| c.contains("pm-sbx-start-desktop")).count(), 1);

    // the secrets go in as root; the desktop starts as the pm user, with the sandbox display and without WSLg
    assert!(joined[at("control.token")].contains("-u root"));
    let desktop = &joined[at("pm-sbx-start-desktop")];
    assert!(desktop.contains("-u pm"));
    assert!(desktop.contains("--exec /usr/bin/env -u WAYLAND_DISPLAY"), "{desktop}");
    assert!(desktop.contains("DISPLAY=:99") && desktop.contains("XDG_RUNTIME_DIR=/tmp/pm-runtime"), "{desktop}");
    for call in guest.calls() {
        if call.args.windows(2).any(|w| w[0] == "-u" && w[1] == "pm") {
            assert!(call.args.join(" ").contains("--exec /usr/bin/env -u WAYLAND_DISPLAY"), "{:?}", call.args);
        }
    }

    // the three secrets: in stdin only, never in an argument
    let pushed = pushes(&guest);
    assert_eq!(pushed.len(), 1);
    let p = &pushed[0];
    assert_eq!(p.token.len(), 64);
    assert!(p.token.bytes().all(|b| b.is_ascii_hexdigit()));
    for password in [&p.view, &p.control] {
        assert_eq!(password.len(), 8, "VNC reads 8 characters");
        assert!(password.bytes().all(|b| b.is_ascii_alphanumeric()));
    }
    assert_ne!(p.view, p.control, "view and control must differ or the server cannot tell them apart");
    for call in guest.calls() {
        for secret in [&p.token, &p.view, &p.control] {
            assert!(call.args.iter().all(|a| !a.contains(secret.as_str())), "a secret is in an argument: {:?}", call.args);
        }
    }
    // the script that writes them names the files, owner and mode (0400, pm runs the services)
    let script = guest.calls().iter().find(|c| c.args.iter().any(|a| a.contains("control.token"))).unwrap().args.last().unwrap().clone();
    for file in ["/run/pm-sbx/control.token", "/run/pm-sbx/vnc-view.pass", "/run/pm-sbx/vnc-control.pass"] {
        assert!(script.contains(file), "{file}");
    }
    assert!(script.contains("chown pm:pm") && script.contains("chmod 0400"), "{script}");
    // and none is in the debug text of what the caller got
    for secret in [&p.token, &p.view, &p.control] {
        assert!(!format!("{ready:?}").contains(secret.as_str()));
    }
}

#[test]
fn the_passwords_leave_only_through_the_screen_secrets_and_only_while_it_runs() {
    let (guest, lifecycle) = rig();
    assert!(lifecycle.screen_secrets().is_none(), "nothing before a start");
    lifecycle.ensure_ready(1_000).unwrap();
    let p = pushes(&guest).remove(0);
    let screen = lifecycle.screen_secrets().expect("running");
    assert_eq!(screen.view().expose(), p.view);
    assert_eq!(screen.control().expose(), p.control);
    let shown = format!("{screen:?} {:?} {:?}", screen.view(), screen.control());
    assert!(!shown.contains(&p.view) && !shown.contains(&p.control), "{shown}");
    lifecycle.stop().unwrap();
    assert!(lifecycle.screen_secrets().is_none(), "forgotten at the stop");
}

#[test]
fn a_second_use_does_not_start_again() {
    let (guest, lifecycle) = rig();
    lifecycle.ensure_ready(1_000).unwrap();
    let before = guest.calls().len();
    let generation = lifecycle.generation();
    lifecycle.ensure_ready(2_000).unwrap();
    assert_eq!(pushes(&guest).len(), 1, "one set of secrets for the run");
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
    let joined = guest.joined();
    assert!(joined.contains(&"--terminate pm-sbx-main".to_string()));
    // the desktop is stopped by its own script first (that is what stops Chrome), then the distro
    let stop_script = joined.iter().position(|c| c.contains("pm-sbx-start-desktop") && c.ends_with(" stop")).expect("the desktop script is told to stop");
    assert!(joined[stop_script].contains("-u pm"));
    assert!(stop_script < joined.iter().position(|c| c == "--terminate pm-sbx-main").unwrap());
    assert_ne!(lifecycle.generation(), generation, "screen links of the old run die");
    // a start after a stop makes NEW secrets (all three)
    lifecycle.ensure_ready(5_000).unwrap();
    let pushed = pushes(&guest);
    assert_eq!(pushed.len(), 2);
    assert_ne!(pushed[0].token, pushed[1].token);
    assert_ne!(pushed[0].view, pushed[1].view);
    assert_ne!(pushed[0].control, pushed[1].control);
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
    assert!(pushes(&guest).is_empty(), "no secret was handed to a sandbox that did not boot");
    assert_eq!(lifecycle.phase(), Phase::Failed("The sandbox did not finish starting.".to_string()));
}

#[test]
fn walls_that_fail_their_probe_stop_the_start_before_any_secret_is_pushed() {
    // the guest's boot hook runs the walls and the probe; its failure is read from the guest, not run by the app
    let (guest, lifecycle) = rig();
    guest.st.lock().egress_probe_passes = false;
    let error = lifecycle.ensure_ready(0).unwrap_err();
    match error {
        LifecycleError::NotReady(why) => assert!(why.contains("network walls"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(pushes(&guest).is_empty());
    assert!(!guest.joined().iter().any(|c| c.contains("pm-sbx-start-desktop") && c.ends_with(" start")), "no screen starts behind failed walls");
    assert!(!guest.holder_alive());
}

#[test]
fn a_boot_hook_that_reports_failure_stops_the_start() {
    let (guest, lifecycle) = rig();
    guest.st.lock().boot_failed = true;
    guest.st.lock().boot_reason = "pm-no-wslg.sh failed (exit 1)".to_string();
    match lifecycle.ensure_ready(0).unwrap_err() {
        LifecycleError::NotReady(why) => {
            assert!(why.contains("safety check"), "{why}");
            assert!(why.contains("pm-no-wslg.sh failed (exit 1)"), "the guest's own reason is in the card: {why}");
        }
        other => panic!("{other:?}"),
    }
    assert!(pushes(&guest).is_empty());
    assert!(!guest.holder_alive());
}

#[test]
fn a_desktop_that_does_not_start_fails_the_start() {
    let (guest, lifecycle) = rig();
    guest.st.lock().desktop_starts = false;
    assert_eq!(lifecycle.ensure_ready(0).unwrap_err(), LifecycleError::NotReady("The sandbox screen did not start.".to_string()));
    assert!(!guest.joined().iter().any(|c| c.contains("pm-sbx-control")), "the app never launches the control service itself");
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

#[test]
fn a_use_that_begins_between_the_idle_decision_and_the_stop_prevents_the_stop() {
    // Holds the start lock (what a start or a stop takes) so the idle check is blocked just before its stop, then a
    // bot action begins its use while the check waits. The check must see that use and not stop.
    let (guest, lifecycle) = rig();
    let m = 60_000;
    lifecycle.ensure_ready(0).unwrap();
    let starts = lifecycle.hold_starts_for_test();
    std::thread::scope(|s| {
        let check = s.spawn(|| lifecycle.idle_check(99 * m));
        std::thread::sleep(std::time::Duration::from_millis(150));
        let action = lifecycle.begin_use();
        drop(starts);
        let stopped = check.join().unwrap().unwrap();
        assert!(!stopped, "a bot action began before the stop: the distro must stay up");
        drop(action);
    });
    assert_eq!(lifecycle.phase(), Phase::Ready);
    assert!(guest.holder_alive());
}

#[test]
fn a_person_who_holds_the_screen_keeps_the_sandbox_up_past_the_idle_minutes() {
    let (guest, lifecycle) = rig();
    let m = 60_000;
    lifecycle.ensure_ready(0).unwrap();
    // the person's link or lease runs until 20 minutes; no bot action at all
    lifecycle.hold_for_person(20 * m);
    assert!(!lifecycle.idle_check(30 * m).unwrap(), "15 idle minutes counted from the person's hold, not from start");
    assert!(!lifecycle.idle_check(34 * m).unwrap());
    assert_eq!(lifecycle.phase(), Phase::Ready);
    // after the hold ends, the idle minutes run from its end
    assert!(lifecycle.idle_check(36 * m).unwrap());
    assert!(!guest.holder_alive());
}

fn wait_for_call(guest: &FakeGuest, needle: &str) {
    let began = std::time::Instant::now();
    while !guest.joined().iter().any(|c| c.contains(needle)) {
        assert!(began.elapsed() < std::time::Duration::from_secs(5), "the call {needle} never came");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[test]
fn a_stop_ends_a_start_in_progress_within_two_seconds_and_then_stops() {
    let (guest, lifecycle) = rig();
    guest.st.lock().hang_desktop = true;
    std::thread::scope(|scope| {
        let start = scope.spawn(|| lifecycle.ensure_ready(1_000));
        wait_for_call(&guest, "pm-sbx-start-desktop start");
        let began = std::time::Instant::now();
        assert!(!lifecycle.stopping());
        lifecycle.stop().unwrap();
        assert!(began.elapsed() < std::time::Duration::from_secs(2), "{:?}", began.elapsed());
        assert_eq!(start.join().unwrap().unwrap_err(), LifecycleError::Cancelled);
    });
    assert_eq!(lifecycle.phase(), Phase::Stopped);
    assert!(!guest.holder_alive(), "the holder is gone");
    assert!(guest.st.lock().running.is_empty(), "the distro is stopped");
    assert!(!lifecycle.stopping());
    // and the flag does not stay set: the next start works
    guest.st.lock().hang_desktop = false;
    lifecycle.ensure_ready(2_000).unwrap();
    assert_eq!(lifecycle.phase(), Phase::Ready);
}

#[test]
fn a_stop_ends_a_start_that_is_waiting_for_the_boot_hook() {
    let (guest, lifecycle) = rig();
    guest.st.lock().boot_hangs = true;
    let long = Timing { poll: std::time::Duration::from_millis(10), boot_polls: 100_000, ready_polls: 3 };
    let lifecycle = lifecycle.with_timing(long);
    std::thread::scope(|scope| {
        let start = scope.spawn(|| lifecycle.ensure_ready(1_000));
        wait_for_call(&guest, "sleep infinity");
        let began = std::time::Instant::now();
        lifecycle.stop().unwrap();
        assert!(began.elapsed() < std::time::Duration::from_secs(2), "{:?}", began.elapsed());
        assert_eq!(start.join().unwrap().unwrap_err(), LifecycleError::Cancelled);
    });
    assert_eq!(lifecycle.phase(), Phase::Stopped);
}

#[test]
fn one_failed_readiness_read_does_not_end_the_boot_wait() {
    let (guest, lifecycle) = rig();
    guest.st.lock().readiness_errors = 1;
    lifecycle.ensure_ready(1_000).unwrap();
    assert_eq!(lifecycle.phase(), Phase::Ready);
}

#[test]
fn readiness_reads_that_keep_failing_end_the_wait_after_the_limit() {
    let (guest, lifecycle) = rig();
    guest.st.lock().readiness_errors = 100;
    let lifecycle = lifecycle.with_timing(Timing { poll: std::time::Duration::ZERO, boot_polls: 50, ready_polls: 3 });
    let error = lifecycle.ensure_ready(1_000).unwrap_err();
    assert!(matches!(error, LifecycleError::Wsl(_)), "{error:?}");
    let reads = guest.joined().iter().filter(|c| c.ends_with("/bin/sh -s")).count();
    assert_eq!(reads, MAX_READ_ERRORS as usize, "it gave up after the limit, not before");
    assert!(!guest.holder_alive());
}

#[test]
fn failed_reads_spread_out_do_not_add_up() {
    // errors only count when they come one after the other
    let (guest, lifecycle) = rig();
    guest.st.lock().readiness_errors = MAX_READ_ERRORS - 1;
    let lifecycle = lifecycle.with_timing(Timing { poll: std::time::Duration::ZERO, boot_polls: 20, ready_polls: 3 });
    lifecycle.ensure_ready(1_000).unwrap();
}
