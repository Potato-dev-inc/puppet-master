//! "Setup didn't finish": the record, the completion marker, the Incomplete state, "Continue setup" and what the app quitting does.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::fakes_tests::{FakeGuest, Scratch};
use super::install::*;
use super::lifecycle::{Lifecycle, Timing};
use super::manager::{Manager, NoEvents};
use super::wsl::SandboxName;
use super::{Sandbox, State, WslSandbox};

fn name() -> SandboxName {
    SandboxName::main()
}

fn answers() -> InstallOptions {
    let mut o = InstallOptions::new(name(), Desktop::Zorin);
    o.addons = vec!["coding".to_string()];
    o.options = vec![("coding.pkg".to_string(), "pnpm".to_string())];
    o
}

struct Rig {
    guest: Arc<FakeGuest>,
    records: Arc<MemoryRecords>,
    sandbox: Arc<WslSandbox>,
    _folder: Scratch,
    _data: Scratch,
}

fn make(installed: bool) -> Rig {
    let guest = if installed { FakeGuest::with_installed("pm-sbx-main") } else { FakeGuest::new() };
    let wsl = guest.wsl();
    let folder = Scratch::new("resume-guest");
    for rel in [
        "guest/desktop/apply-recipe.py",
        "guest/desktop/pm-sbx-start-desktop.sh",
        "guest/egress/pm-sbx-egress.sh",
        "guest/egress/pm-sbx-egress-probe.sh",
        "guest/control/pm-sbx-control",
        "guest/page-browser/pm-sbx-page-browser",
        "recipes/base.json",
    ] {
        let path = folder.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "x").unwrap();
    }
    let data = Scratch::new("resume-data");
    let records = Arc::new(MemoryRecords::default());
    let lifecycle = Arc::new(Lifecycle::new(wsl.clone(), name()).with_timing(Timing::instant()));
    let sandbox = Arc::new(WslSandbox::new(wsl, name(), Layout::new(&data.0), GuestFiles::new(&folder.0), lifecycle).with_records(records.clone()));
    Rig { guest, records, sandbox, _folder: folder, _data: data }
}

fn manager(rig: &Rig) -> Arc<Manager> {
    Manager::new(rig.sandbox.clone(), name(), None, None, Arc::new(NoEvents))
}

fn wait_until_done(m: &Arc<Manager>) -> String {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        let state = m.status().unwrap().install.state;
        if state != "running" && state != "idle" || Instant::now() > end {
            return state.to_string();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_finished_install_leaves_the_marker_and_a_finished_record() {
    let rig = make(false);
    rig.sandbox.install(&answers(), &mut |_| {}).unwrap();
    let saved = rig.records.load(&name()).unwrap();
    assert!(saved.done);
    assert_eq!(saved.options, Some(answers()), "the answers are kept");
    let marker = rig.guest.st.lock().marker.clone().expect("the guest got the completion marker");
    assert!(marker.starts_with("version=1\n") && marker.contains("desktop=zorin\n") && marker.contains("--recipe xfce --recipe zorin-look --recipe coding"), "{marker}");
    let joined = rig.guest.joined();
    let at = |needle: &str| joined.iter().position(|c| c.contains(needle)).unwrap_or_else(|| panic!("no {needle}: {joined:#?}"));
    assert!(at("/etc/pm-sbx/installed") > at("sleep infinity"), "after the boot check");
    assert!(at("/etc/pm-sbx/installed") < at("--export"), "before the reset point, so a restored reset point is finished too");
    assert_eq!(rig.sandbox.status().unwrap().state, State::Stopped);
}

#[test]
fn a_failed_new_install_takes_its_record_away_with_its_distro() {
    let rig = make(false);
    rig.guest.st.lock().apply_code = 1;
    assert!(rig.sandbox.install(&answers(), &mut |_| {}).is_err());
    assert!(rig.records.load(&name()).is_none());
    assert!(rig.guest.st.lock().installed.is_empty());
}

#[test]
fn a_cut_off_setup_is_incomplete_and_nothing_starts_it() {
    let rig = make(true);
    rig.records.save(&name(), &SavedInstall::started(&answers()));
    let status = rig.sandbox.status().unwrap();
    assert_eq!(status.state, State::Incomplete);
    assert_eq!(status.reason.as_deref(), Some("Setup didn't finish."));
    for error in [rig.sandbox.start().unwrap_err(), rig.sandbox.observe().unwrap_err(), rig.sandbox.act(&[]).unwrap_err(), rig.sandbox.browser(&[]).unwrap_err()] {
        assert_eq!(error.code, "SANDBOX_SETUP_UNFINISHED");
    }
    assert!(!rig.guest.holder_alive(), "no start was tried");
    assert_eq!(rig.sandbox.resume_options(), Some(answers()));
    let view = manager(&rig).status().unwrap();
    assert_eq!((view.state, view.can_continue), ("incomplete", true));
}

#[test]
fn continue_setup_runs_the_steps_again_on_the_same_distro_without_a_download() {
    let rig = make(true);
    rig.records.save(&name(), &SavedInstall::started(&answers()));
    let m = manager(&rig);
    m.continue_install().unwrap();
    assert_eq!(wait_until_done(&m), "done");
    let joined = rig.guest.joined();
    let at = |needle: &str| joined.iter().position(|c| c.contains(needle)).unwrap_or_else(|| panic!("no {needle}: {joined:#?}"));
    assert!(!joined.iter().any(|c| c.contains("--install Ubuntu")), "no download: {joined:#?}");
    assert!(!joined.iter().any(|c| c.contains("--unregister")), "the distro is kept");
    assert!(at("--terminate pm-sbx-main") < at("/bin/tar -x"), "whatever the cut-off run left going is stopped first");
    assert!(at("/bin/tar -x") < at("apply-recipe.py"));
    assert!(joined[at("apply-recipe.py")].ends_with("--recipe xfce --recipe zorin-look --recipe coding --opt coding.pkg=pnpm"), "the saved answers are used");
    assert!(at("apply-recipe.py") < at("sleep infinity") && at("sleep infinity") < at("/etc/pm-sbx/installed") && at("/etc/pm-sbx/installed") < at("--export"));
    assert!(rig.records.load(&name()).unwrap().done);
    assert_eq!(m.status().unwrap().state, "stopped");
    assert!(rig.guest.st.lock().marker.is_some());
}

#[test]
fn a_carried_on_setup_that_fails_again_keeps_the_distro_and_stays_unfinished() {
    let rig = make(true);
    rig.records.save(&name(), &SavedInstall::started(&answers()));
    rig.guest.st.lock().apply_code = 1;
    let m = manager(&rig);
    m.continue_install().unwrap();
    assert_eq!(wait_until_done(&m), "failed");
    assert!(rig.guest.st.lock().installed.contains(&"pm-sbx-main".to_string()), "still there to try again");
    assert!(!rig.records.load(&name()).unwrap().done);
    m.dismiss_install();
    assert_eq!(m.status().unwrap().state, "incomplete");
}

#[test]
fn continue_is_refused_when_there_is_nothing_to_continue() {
    let rig = make(true);
    rig.records.save(&name(), &SavedInstall::finished(Some(&answers())));
    assert_eq!(manager(&rig).continue_install().unwrap_err().code, "SANDBOX_NOT_UNFINISHED");
    let rig = rig_none();
    assert_eq!(manager(&rig).continue_install().unwrap_err().code, "SANDBOX_NOT_UNFINISHED");
}

fn rig_none() -> Rig {
    make(false)
}

#[test]
fn a_sandbox_from_before_records_is_looked_at_once() {
    // finished: the guest has what the last recipe step writes
    let rig = make(true);
    assert_eq!(rig.sandbox.status().unwrap().state, State::Stopped);
    assert!(rig.records.load(&name()).unwrap().done, "remembered, so nobody looks again");
    assert_eq!(rig.sandbox.status().unwrap().state, State::Stopped);
    assert_eq!(rig.guest.joined().iter().filter(|c| c.contains("test -f")).count(), 1);

    // cut off (the founder's case): no marker and no desktop.conf, and nothing saved about its answers
    let rig = make(true);
    rig.guest.st.lock().legacy_finished = false;
    assert_eq!(rig.sandbox.status().unwrap().state, State::Incomplete);
    assert_eq!(rig.sandbox.status().unwrap().state, State::Incomplete);
    assert_eq!(rig.guest.joined().iter().filter(|c| c.contains("test -f")).count(), 1, "one look, then it is known");
    let m = manager(&rig);
    let view = m.status().unwrap();
    assert_eq!((view.state, view.can_continue), ("incomplete", false), "its answers are not known: only start over");
    assert_eq!(m.continue_install().unwrap_err().code, "SANDBOX_CANNOT_CONTINUE");
    // start over: remove, then the wizard
    m.remove("remove").unwrap();
    assert_eq!(m.status().unwrap().state, "not_installed");
}

#[test]
fn a_running_sandbox_is_not_looked_at_and_a_failed_look_counts_as_finished() {
    let rig = make(true);
    rig.sandbox.start().unwrap();
    assert_eq!(rig.sandbox.status().unwrap().state, State::Running);
    rig.sandbox.stop().unwrap();
    let rig2 = rig_legacy_wsl_down();
    assert_eq!(rig2.sandbox.status().unwrap().state, State::NotInstalled);
}

fn rig_legacy_wsl_down() -> Rig {
    let rig = make(true);
    rig.guest.st.lock().wsl_ok = false;
    rig
}

#[test]
fn the_record_survives_a_round_trip_and_refuses_nonsense() {
    let saved = SavedInstall::started(&answers());
    assert_eq!(SavedInstall::from_json(&name(), &saved.to_json()), Some(saved.clone()));
    let finished = SavedInstall::finished(None);
    assert_eq!(SavedInstall::from_json(&name(), &finished.to_json()), Some(finished));
    for bad in [serde_json::json!({}), serde_json::json!({"done": "yes"}), serde_json::json!({"done": false, "desktop": "windows", "addons": [], "options": [], "snapshot": true}),
                serde_json::json!({"done": false, "desktop": "ubuntu", "addons": ["rm -rf"], "options": [], "snapshot": true})] {
        assert_eq!(SavedInstall::from_json(&name(), &bad), None, "{bad}");
    }
}

#[test]
fn quitting_the_app_never_cancels_a_set_up() {
    // The set up thread dies with the app; the record stays "not finished", so the next start shows "Setup didn't finish".
    let host = include_str!("../sandbox_host.rs");
    let at = host.find("pub fn stop_on_exit()").expect("stop_on_exit");
    let body = &host[at..host[at..].find("\n}\n").map(|e| at + e).unwrap_or(host.len())];
    assert!(!body.contains("cancel_install"), "{body}");
    assert!(body.contains("manager.stop()"));
}

/// The one real "Continue setup": `PM_SBX_RESUME_LIVE=1 cargo test --lib potato::sandbox::resume_tests::a_cut_off -- --ignored --nocapture`.
/// It needs the distro `pm-sbx-speed` to exist in a cut-off state (made by the speed scripts: a fresh image, the guest files, an
/// apply that was killed half way). It touches that name only and refuses `pm-sbx-main`.
#[test]
#[ignore = "needs WSL and a cut-off pm-sbx-speed distro: PM_SBX_RESUME_LIVE=1"]
fn a_cut_off_install_is_carried_on_for_real() {
    if std::env::var("PM_SBX_RESUME_LIVE").ok().as_deref() != Some("1") {
        println!("[resume-live] skipped: set PM_SBX_RESUME_LIVE=1");
        return;
    }
    use crate::potato::window_guard;
    let wsl = super::wsl::Wsl::live().expect("wsl.exe");
    let name = SandboxName::new("pm-sbx-speed").unwrap();
    assert_ne!(name.as_str(), "pm-sbx-main");
    assert!(wsl.is_installed(&name).unwrap(), "the cut-off pm-sbx-speed distro is there");
    let (before, line) = window_guard::visible_windows(&window_guard::PowerShell).expect("count windows");
    println!("[resume-live] before: {line}");
    assert_eq!(before, 0);

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("potato-sandbox");
    let files = GuestFiles::new(&root);
    files.check().unwrap();
    let layout = Layout::new(std::path::Path::new(r"D:\pm-sandbox\speed\app"));
    let lifecycle = Arc::new(Lifecycle::new(wsl.clone(), name.clone()).with_window_check(Arc::new(window_guard::Msrdc::live())));
    let records = Arc::new(MemoryRecords::default());
    let sandbox = WslSandbox::new(wsl.clone(), name.clone(), layout, files, lifecycle.clone()).with_records(records.clone());

    let mut answers = InstallOptions::new(name.clone(), Desktop::Xfce);
    answers.addons = vec!["coding".to_string()];
    answers.options = vec![("coding.pkg".to_string(), "pnpm".to_string())];
    answers.snapshot = false;
    records.save(&name, &SavedInstall::started(&answers));

    let status = sandbox.status().unwrap();
    println!("[resume-live] status before: {:?} {:?}", status.state, status.reason);
    assert_eq!(status.state, State::Incomplete);
    assert_eq!(sandbox.start().unwrap_err().code, "SANDBOX_SETUP_UNFINISHED");

    let mut resume = sandbox.resume_options().expect("the answers were saved");
    resume.resume = true;
    let started = Instant::now();
    let mut last = String::new();
    sandbox
        .install(&resume, &mut |p| {
            let line = format!("{}% {} {}", p.percent, p.title, p.detail.unwrap_or_default());
            if line != last && !line.contains("Downloading ") && !line.contains("Unpacking ") && !line.contains("Setting up ") {
                println!("[resume-live] {:>5.1}s {line}", started.elapsed().as_secs_f32());
                last = line;
            }
        })
        .expect("continue setup");
    println!("[resume-live] continue took {:?}", started.elapsed());
    assert_eq!(sandbox.status().unwrap().state, State::Stopped);
    assert!(records.load(&name).unwrap().done);

    let ready = lifecycle.ensure_ready(0).expect("the carried-on sandbox starts and passes its checks");
    let shot = ready.client.screenshot().expect("a screenshot through the control service");
    println!("[resume-live] screenshot {}x{} ({} bytes)", shot.width, shot.height, shot.png.len());
    assert!(shot.png.starts_with(&[0x89, b'P', b'N', b'G']));
    let (during, line) = window_guard::visible_windows(&window_guard::PowerShell).expect("count windows");
    println!("[resume-live] while running: {line}");
    assert_eq!(during, 0);
    lifecycle.stop().unwrap();
}
