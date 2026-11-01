//! The `Sandbox` trait: the fake (static picture, recorded calls) and the real one over a fake guest.

use std::sync::Arc;

use serde_json::json;

use super::control::{Action, Button};
use super::control_tests::{observation_json, serve};
use super::fakes_tests::{FakeGuest, Scratch};
use super::install::{Desktop, GuestFiles, InstallOptions, Layout, Progress};
use super::lifecycle::{Lifecycle, Timing};
use super::wsl::SandboxName;
use super::*;

fn click() -> Action {
    Action::Click { x: 10, y: 20, button: Button::Left }
}

// ---- the fake ------------------------------------------------------------------------------------

#[test]
fn the_fake_serves_one_picture_and_records_every_call() {
    let fake = FakeSandbox::new();
    assert_eq!(fake.status().unwrap().state, State::Stopped);
    let shot = fake.observe().unwrap();
    assert_eq!(&shot.png[..4], b"\x89PNG");
    assert_eq!((shot.width, shot.height), (160, 100));
    assert_eq!(fake.status().unwrap().state, State::Running, "a screen call starts it, like the real one");
    let after = fake.act(&[click(), Action::Type { text: "hello".into() }]).unwrap();
    assert_eq!(after, FakeSandbox::picture());
    fake.set_browser_reply("{\"title\":\"Shop\"}");
    assert_eq!(fake.browser(&["snapshot".to_string()]).unwrap(), "{\"title\":\"Shop\"}");
    assert_eq!(
        fake.calls(),
        vec![
            Recorded::Observe,
            Recorded::Act(vec![click(), Action::Type { text: "hello".into() }]),
            Recorded::Browser(vec!["snapshot".to_string()]),
        ]
    );
    assert_eq!(fake.actions().len(), 2);
}

#[test]
fn the_fake_refuses_what_the_guest_would_refuse() {
    let fake = FakeSandbox::new();
    let error = fake.act(&[Action::Move { x: -1, y: 0 }]).unwrap_err();
    assert_eq!(error.code, "SANDBOX_INVALID_ACTION");
    assert!(fake.actions().is_empty(), "nothing was recorded as done");
}

#[test]
fn the_fake_can_be_told_to_fail_once() {
    let fake = FakeSandbox::new();
    fake.fail_next(SandboxError::new("SANDBOX_NOT_READY", "The sandbox screen did not come up."));
    assert_eq!(fake.observe().unwrap_err().code, "SANDBOX_NOT_READY");
    assert!(fake.observe().is_ok());
}

#[test]
fn the_fake_installs_stops_resets_and_removes() {
    let fake = FakeSandbox::not_installed();
    assert_eq!(fake.status().unwrap().state, State::NotInstalled);
    assert_eq!(fake.observe().unwrap_err().code, "SANDBOX_NOT_INSTALLED");
    let mut seen: Vec<Progress> = Vec::new();
    fake.install(&InstallOptions::new(SandboxName::main(), Desktop::Light), &mut |p| seen.push(p)).unwrap();
    assert_eq!(seen.last().unwrap().percent, 100);
    assert_eq!(fake.status().unwrap().state, State::Stopped);
    assert_eq!(fake.install(&InstallOptions::new(SandboxName::main(), Desktop::Light), &mut |_| {}).unwrap_err().code, "SANDBOX_ALREADY_INSTALLED");

    fake.start().unwrap();
    let g1 = fake.screen_target().unwrap().generation;
    fake.stop().unwrap();
    assert_ne!(fake.screen_target().unwrap().generation, g1, "a stop kills the screen links");
    fake.start().unwrap();
    fake.snapshot("before-shop").unwrap();
    assert_eq!(fake.snapshot("Bad Name").unwrap_err().code, "SANDBOX_BAD_SNAPSHOT");
    fake.reset("base").unwrap();
    assert!(!fake.is_running());
    fake.remove().unwrap();
    assert_eq!(fake.status().unwrap().state, State::NotInstalled);
}

#[test]
fn the_fake_rejects_bad_install_options() {
    let fake = FakeSandbox::not_installed();
    let mut options = InstallOptions::new(SandboxName::main(), Desktop::Light);
    options.addons = vec!["gaming".to_string()];
    assert_eq!(fake.install(&options, &mut |_| {}).unwrap_err().code, "SANDBOX_BAD_OPTIONS");
}

#[test]
fn both_kinds_work_as_a_trait_object() {
    let fake: Box<dyn Sandbox> = Box::new(FakeSandbox::new());
    assert!(fake.status().is_ok());
    let shared: Arc<dyn Sandbox> = Arc::new(FakeSandbox::new());
    assert!(shared.observe().is_ok());
}

// ---- the real one over a fake guest --------------------------------------------------------------

struct Rig {
    guest: Arc<FakeGuest>,
    sandbox: WslSandbox,
    layout: Layout,
    _folder: Scratch,
    data: Scratch,
}

/// `control_port` is where the fake control service listens (a test server), or 0 when no call reaches it.
fn rig(installed: bool, control_port: u16) -> Rig {
    let guest = if installed { FakeGuest::with_installed("pm-sbx-main") } else { FakeGuest::new() };
    let wsl = guest.wsl();
    let folder = Scratch::new("sbxguest");
    for rel in [
        "guest/desktop/apply-recipe.py",
        "guest/desktop/pm-sbx-start-desktop.sh",
        "guest/egress/pm-sbx-egress.sh",
        "guest/egress/pm-sbx-egress-probe.sh",
        "guest/control/pm-sbx-control",
        "guest/page-browser/pm-sbx-page-browser",
        "recipes/base.json",
    ] {
        let text = "x";
        let path = folder.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let data = Scratch::new("sbxdata");
    let layout = Layout::new(&data.0);
    let lifecycle = Arc::new(Lifecycle::new(wsl.clone(), SandboxName::main()).with_timing(Timing::instant()).with_ports(6080, control_port));
    let sandbox = WslSandbox::new(wsl, SandboxName::main(), layout.clone(), GuestFiles::new(&folder.0), lifecycle);
    Rig { guest, sandbox, layout, _folder: folder, data }
}

#[test]
fn the_real_sandbox_reports_what_it_is() {
    let rig = rig(false, 0);
    let status = rig.sandbox.status().unwrap();
    assert_eq!((status.state, status.reason), (State::NotInstalled, None));
    assert_eq!(status.name, "pm-sbx-main");
    assert_eq!(status.idle_minutes, 15);

    rig.guest.st.lock().wsl_ok = false;
    let status = rig.sandbox.status().unwrap();
    assert_eq!(status.state, State::NotInstalled);
    assert_eq!(status.reason.as_deref(), Some("Needs Windows' Linux feature (admin + restart)."));
    assert_eq!(rig.guest.st.lock().elevated_seen, 0);
}

#[test]
fn install_then_start_then_stop() {
    let rig = rig(false, 0);
    let mut seen = Vec::new();
    rig.sandbox.install(&InstallOptions::new(SandboxName::main(), Desktop::Xfce), &mut |p| seen.push(p)).unwrap();
    assert_eq!(seen.last().unwrap().percent, 100);
    assert_eq!(rig.sandbox.status().unwrap().state, State::Stopped);
    rig.sandbox.start().unwrap();
    assert_eq!(rig.sandbox.status().unwrap().state, State::Running);
    assert!(rig.guest.holder_alive());
    let target = rig.sandbox.screen_target().unwrap();
    assert_eq!(target.view_port, 6080);
    assert!(target.generation > 0);
    rig.sandbox.stop().unwrap();
    assert_eq!(rig.sandbox.status().unwrap().state, State::Stopped);
    assert_ne!(rig.sandbox.screen_target().unwrap().generation, target.generation);
}

#[test]
fn a_failed_start_shows_as_failed_with_the_reason() {
    let rig = rig(true, 0);
    rig.guest.st.lock().egress_probe_passes = false;
    let error = rig.sandbox.start().unwrap_err();
    assert_eq!(error.code, "SANDBOX_NOT_READY");
    let status = rig.sandbox.status().unwrap();
    assert_eq!(status.state, State::Failed);
    assert!(status.reason.unwrap().contains("network walls"));
}

#[test]
fn observe_starts_the_sandbox_on_first_use_and_reads_the_screen() {
    let (port, seen) = serve(200, json!({ "observation": observation_json() }).to_string());
    let rig = rig(true, port);
    assert!(!rig.guest.holder_alive(), "not running before the first use");
    let shot = rig.sandbox.observe().unwrap();
    assert_eq!((shot.width, shot.height), (160, 100));
    assert!(rig.guest.holder_alive(), "the first use started it");
    assert_eq!(seen.join().unwrap().request_line, "POST /v1/screenshot HTTP/1.1");
}

#[test]
fn act_goes_to_the_control_service_with_the_steps() {
    let (port, seen) = serve(200, json!({ "completed": 1, "observation": observation_json() }).to_string());
    let rig = rig(true, port);
    let shot = rig.sandbox.act(&[click()]).unwrap();
    assert_eq!(shot.pointer, Some((12, 34)));
    let seen = seen.join().unwrap();
    assert_eq!(seen.request_line, "POST /v1/desktop HTTP/1.1");
    assert_eq!(seen.body["steps"][0]["argv"][6], "20");
}

#[test]
fn a_control_error_is_an_error_card_not_a_panic() {
    let (port, _seen) = serve(401, json!({ "error": "unauthorized" }).to_string());
    let rig = rig(true, port);
    let error = rig.sandbox.observe().unwrap_err();
    assert_eq!(error.code, "SANDBOX_UNAUTHORIZED");
    assert!(error.message.ends_with('.'));
}

#[test]
fn the_browser_tool_runs_as_the_pm_user_and_its_output_is_capped() {
    let rig = rig(true, 0);
    let out = rig.sandbox.browser(&["snapshot".to_string(), "--json".to_string()]).unwrap();
    assert_eq!(out, "{\"ok\":true}");
    let call = rig.guest.joined().into_iter().find(|c| c.contains("pm-sbx-page-browser")).unwrap();
    assert_eq!(
        call,
        "-d pm-sbx-main -u pm --exec /usr/bin/env DISPLAY=:99 /usr/bin/python3 -I /usr/local/share/pm-sbx/page-browser/pm-sbx-page-browser snapshot --json"
    );
}

#[test]
fn reset_goes_back_to_the_saved_point_and_leaves_it_stopped() {
    let rig = rig(true, 0);
    let name = SandboxName::main();
    assert_eq!(rig.sandbox.reset("base").unwrap_err().code, "SANDBOX_NO_SNAPSHOT");
    assert!(!rig.guest.joined().iter().any(|c| c.starts_with("--unregister")), "nothing is thrown away without a reset point");
    assert_eq!(rig.sandbox.reset("../x").unwrap_err().code, "SANDBOX_BAD_SNAPSHOT");

    let tar = rig.layout.snapshot_path(&name, "base").unwrap();
    std::fs::create_dir_all(tar.parent().unwrap()).unwrap();
    std::fs::write(&tar, b"tar").unwrap();
    rig.sandbox.start().unwrap();
    rig.sandbox.reset("base").unwrap();
    let joined = rig.guest.joined();
    let at = |needle: &str| joined.iter().rposition(|c| c.starts_with(needle)).unwrap_or_else(|| panic!("no {needle}: {joined:#?}"));
    assert!(at("--terminate") < at("--unregister pm-sbx-main"));
    assert!(at("--unregister pm-sbx-main") < at("--import pm-sbx-main"));
    assert!(joined[at("--import pm-sbx-main")].ends_with(&*tar.to_string_lossy()));
    assert!(rig.layout.distro_dir(&name).is_dir());
    assert_eq!(rig.sandbox.status().unwrap().state, State::Stopped);
}

#[test]
fn a_snapshot_stops_the_sandbox_and_exports_it_under_the_app_data() {
    let rig = rig(true, 0);
    rig.sandbox.start().unwrap();
    rig.sandbox.snapshot("before-shop").unwrap();
    let joined = rig.guest.joined();
    let export = joined.iter().find(|c| c.starts_with("--export")).unwrap();
    assert!(export.contains("before-shop.tar"));
    assert!(export.contains(&*rig.data.0.to_string_lossy()));
    assert!(!rig.guest.holder_alive());
    assert_eq!(rig.sandbox.snapshot("no/slash").unwrap_err().code, "SANDBOX_BAD_SNAPSHOT");
}

#[test]
fn remove_takes_the_distro_and_only_the_reset_point_files() {
    let rig = rig(true, 0);
    let name = SandboxName::main();
    let dir = rig.layout.snapshots_dir(&name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("base.tar"), b"x").unwrap();
    std::fs::write(dir.join("notes.txt"), b"keep me").unwrap();
    rig.sandbox.remove().unwrap();
    assert!(rig.guest.st.lock().installed.is_empty());
    assert!(!dir.join("base.tar").exists());
    assert!(dir.join("notes.txt").exists(), "a file that is not a reset point is left alone");
    assert_eq!(rig.sandbox.status().unwrap().state, State::NotInstalled);
    // removing again is fine
    rig.sandbox.remove().unwrap();
}

#[test]
fn a_cancel_flag_is_cleared_by_the_next_install() {
    let rig = rig(false, 0);
    rig.sandbox.cancel_install();
    rig.sandbox.install(&InstallOptions::new(SandboxName::main(), Desktop::Light), &mut |_| {}).unwrap();
}
