//! The install: options, steps, progress lines, the guest archive, and the whole run against a fake guest.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::fakes_tests::{FakeGuest, Scratch};
use super::install::*;
use super::lifecycle::{Lifecycle, Timing};
use super::wsl::SandboxName;

fn main_name() -> SandboxName {
    SandboxName::main()
}

fn options() -> InstallOptions {
    let mut o = InstallOptions::new(main_name(), Desktop::Xfce);
    o.addons = vec!["coding".to_string()];
    o.options = vec![("coding.pkg".to_string(), "pnpm".to_string()), ("coding.languages".to_string(), "python,go".to_string())];
    o
}

/// A small guest folder: the shape of `packages/potato-sandbox/`, with Windows line endings.
fn guest_folder() -> Scratch {
    let scratch = Scratch::new("guest");
    let put = |rel: &str, text: &str| {
        let path = scratch.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    put("guest/desktop/apply-recipe.py", "#!/usr/bin/env python3\r\nprint('hi')\r\n");
    put("guest/desktop/wsl.conf", "[boot]\r\ncommand=/usr/local/sbin/pm-no-wslg.sh\r\n");
    put("guest/desktop/PORTED.md", "provenance");
    put("guest/control/pm-sbx-control", "#!/usr/bin/env python3\n");
    put("guest/control/test_control.py", "tests stay home");
    put("guest/control/PORTED.md", "provenance");
    put("guest/egress/pm-sbx-egress.sh", "#!/usr/bin/env bash\r\necho walls\r\n");
    put("guest/egress/pm-sbx-egress-probe.sh", "#!/usr/bin/env bash\n");
    put("guest/desktop/pm-sbx-start-desktop.sh", "#!/bin/bash\n");
    put("guest/egress/README.md", "readme");
    put("guest/page-browser/pm-sbx-page-browser", "#!/usr/bin/env python3\n");
    put("recipes/base.json", "{\"id\": \"base\"}\r\n");
    put("recipes/SCHEMA.md", "schema");
    scratch
}

struct Tar(Vec<(String, Vec<u8>, u32)>);

/// A tiny reader of what `tar_stream` writes; it checks every header checksum on the way.
fn read_tar(bytes: &[u8]) -> Tar {
    let octal = |field: &[u8]| {
        let text: String = field.iter().take_while(|b| **b != 0 && **b != b' ').map(|b| *b as char).collect();
        u32::from_str_radix(text.trim(), 8).unwrap()
    };
    let mut at = 0;
    let mut files = Vec::new();
    while at + 512 <= bytes.len() {
        let header = &bytes[at..at + 512];
        if header.iter().all(|b| *b == 0) {
            break;
        }
        let name: String = header[..100].iter().take_while(|b| **b != 0).map(|b| *b as char).collect();
        let size = octal(&header[124..136]) as usize;
        let mode = octal(&header[100..108]);
        let stored = octal(&header[148..156]);
        let mut copy = header.to_vec();
        copy[148..156].copy_from_slice(b"        ");
        assert_eq!(copy.iter().map(|b| u32::from(*b)).sum::<u32>(), stored, "checksum of {name}");
        assert_eq!(&header[257..263], b"ustar\0");
        assert_eq!(header[156], b'0');
        files.push((name, bytes[at + 512..at + 512 + size].to_vec(), mode));
        at += 512 + size.div_ceil(512) * 512;
    }
    assert_eq!(at + 1024, bytes.len(), "the archive ends with two empty blocks");
    assert!(bytes[at..].iter().all(|b| *b == 0));
    Tar(files)
}

#[test]
fn the_guest_archive_has_the_right_files_without_windows_line_endings() {
    let folder = guest_folder();
    let tar = GuestFiles::new(&folder.0).tar_stream().unwrap();
    assert_eq!(tar.len() % 512, 0);
    let files = read_tar(&tar).0;
    let names: Vec<&str> = files.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "control/pm-sbx-control",
            "desktop/apply-recipe.py",
            "desktop/pm-sbx-start-desktop.sh",
            "desktop/wsl.conf",
            "egress/pm-sbx-egress-probe.sh",
            "egress/pm-sbx-egress.sh",
            "page-browser/pm-sbx-page-browser",
            "recipes/base.json",
        ],
        "sorted, without provenance notes, readmes, schema notes and the guest's own tests"
    );
    let get = |name: &str| files.iter().find(|(n, _, _)| n == name).unwrap();
    assert_eq!(get("desktop/apply-recipe.py").1, b"#!/usr/bin/env python3\nprint('hi')\n");
    assert_eq!(get("egress/pm-sbx-egress.sh").1, b"#!/usr/bin/env bash\necho walls\n");
    assert_eq!(get("recipes/base.json").1, b"{\"id\": \"base\"}\n");
    assert_eq!(get("desktop/apply-recipe.py").2, 0o755, "scripts can run");
    assert_eq!(get("recipes/base.json").2, 0o644);
    assert_eq!(get("desktop/wsl.conf").2, 0o644);
}

#[test]
fn the_shipped_guest_folder_makes_a_valid_archive() {
    // The real packages/potato-sandbox folder (read only).
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("potato-sandbox");
    let files = GuestFiles::new(&root);
    files.check().expect("the guest folder holds apply-recipe.py and base.json");
    let listed = files.list().unwrap();
    for must in ["desktop/apply-recipe.py", "control/pm-sbx-control", "egress/pm-sbx-egress.sh", "egress/pm-sbx-egress-probe.sh", "desktop/pm-sbx-start-desktop.sh", "recipes/base.json", "page-browser/pm-sbx-page-browser"] {
        assert!(listed.iter().any(|(n, _)| n == must), "{must} is not shipped: {listed:?}");
    }
    assert!(listed.iter().all(|(n, _)| !n.ends_with("PORTED.md") && !n.contains("/test_")));
    let tar = read_tar(&files.tar_stream().unwrap());
    assert_eq!(tar.0.len(), listed.len());
    assert!(tar.0.iter().all(|(_, data, _)| !data.windows(2).any(|w| w == b"\r\n")), "no CRLF in anything shipped");
}

#[test]
fn a_bad_name_in_the_archive_is_refused() {
    let mut out = Vec::new();
    for bad in ["", "/abs", "../up", "a/../b", &"x".repeat(101), "näme"] {
        assert!(write_tar_entry(&mut out, bad, b"x").is_err(), "{bad}");
    }
    assert!(out.is_empty());
}

#[test]
fn missing_guest_files_are_found_before_anything_runs() {
    let empty = Scratch::new("noguest");
    assert_eq!(GuestFiles::new(&empty.0).check(), Err(InstallError::GuestFilesMissing));
    assert!(GuestFiles::new(&empty.0).tar_stream().is_err());
}

#[test]
fn options_are_checked() {
    assert!(options().validate().is_ok());
    assert!(InstallOptions::new(main_name(), Desktop::Light).validate().is_ok());
    let with = |f: &dyn Fn(&mut InstallOptions)| {
        let mut o = options();
        f(&mut o);
        o.validate()
    };
    assert!(with(&|o| o.addons.push("coding".into())).is_err(), "twice");
    assert!(with(&|o| o.addons.push("gaming".into())).is_err(), "unknown add-on");
    assert!(with(&|o| o.addons = vec!["../x".into()]).is_err());
    assert!(with(&|o| o.options.push(("daily.connections".into(), "x".into()))).is_err(), "a recipe that is not chosen");
    assert!(with(&|o| o.options.push(("nodot".into(), "x".into()))).is_err());
    assert!(with(&|o| o.options.push(("coding.pkg;ls".into(), "x".into()))).is_err());
    assert!(with(&|o| o.options.push(("coding.pkg".into(), "a b".into()))).is_err());
    assert!(with(&|o| o.options.push(("coding.pkg".into(), "--evil".into()))).is_ok(), "a value is data after '='");
    assert!(with(&|o| o.options.push(("coding.pkg".into(), "A".into()))).is_err(), "upper case");
    assert!(with(&|o| o.options.push(("coding.pkg".into(), "x".repeat(201)))).is_err());
    assert!(with(&|o| o.options.push(("coding.pkg".into(), "a\nb".into()))).is_err());
}

#[test]
fn the_recipe_command_line_is_built_from_the_choices() {
    assert_eq!(
        options().recipe_args(),
        vec!["--recipe", "xfce", "--recipe", "coding", "--opt", "coding.pkg=pnpm", "--opt", "coding.languages=python,go"]
    );
    assert_eq!(InstallOptions::new(main_name(), Desktop::Light).recipe_args(), vec!["--recipe", "light"]);
}

#[test]
fn the_zorin_look_is_xfce_plus_the_look_recipe_and_the_look_comes_after_the_desktop() {
    let mut zorin = InstallOptions::new(main_name(), Desktop::Zorin);
    zorin.addons.push("coding".into());
    zorin.options.push(("coding.pkg".into(), "npm".into()));
    assert!(zorin.validate().is_ok());
    assert_eq!(zorin.recipe_args(), vec!["--recipe", "xfce", "--recipe", "zorin-look", "--recipe", "coding", "--opt", "coding.pkg=npm"]);
    assert_eq!(Desktop::Zorin.recipe_id(), "xfce", "the desktop (session) recipe is XFCE");
    // an option of the look is a choice of a recipe that is chosen
    zorin.options.push(("zorin-look.x".into(), "1".into()));
    assert!(zorin.validate().is_ok());
    // the look never comes with the light desktop
    assert_eq!(Desktop::Light.recipe_ids(), vec!["light"]);
    assert!(InstallOptions::new(main_name(), Desktop::Light).addons.is_empty());
    let mut light = InstallOptions::new(main_name(), Desktop::Light);
    light.options.push(("zorin-look.x".into(), "1".into()));
    assert!(light.validate().is_err(), "an option of a recipe that is not chosen");
}

#[test]
fn the_wizard_words_for_the_look_are_parsed() {
    assert_eq!(Desktop::parse("ubuntu"), Some(Desktop::Xfce));
    assert_eq!(Desktop::parse("zorin"), Some(Desktop::Zorin));
    assert_eq!(Desktop::parse("light"), Some(Desktop::Light));
    assert_eq!(Desktop::parse("Zorin OS"), None);
    assert_eq!(Desktop::parse(""), None);
}

#[test]
fn the_shipped_recipes_name_the_files_they_install_and_the_look_requires_xfce() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("potato-sandbox");
    let look: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("recipes").join("zorin-look.json")).unwrap()).unwrap();
    assert_eq!(look["id"], "zorin-look");
    assert_eq!(look["requires"], "xfce");
    // the pin that keeps xfdesktop4 (spike item 5): the PPA is low priority, the four look packages are not
    let pre = look["pre"].to_string();
    assert!(pre.contains("Pin-Priority: 100") && pre.contains("Pin-Priority: 600") && pre.contains("LP-PPA-zorinos-stable"));
    let apt: Vec<&str> = look["apt"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(apt, vec!["zorin-desktop-themes", "zorin-icon-themes", "zorin-os-wallpapers-18", "fonts-inter-zorin-os"]);
    // the files its post lines copy are shipped with the guest files
    let files = GuestFiles::new(&root).list().unwrap();
    for needed in ["desktop/pm-sbx-zorin-look.sh", "desktop/pm-sbx-zorin-look.desktop", "recipes/zorin-look.json"] {
        assert!(files.iter().any(|(name, _)| name == needed), "{needed} is not in the guest archive");
    }
}

#[test]
fn progress_lines_are_read_strictly() {
    assert_eq!(parse_progress_line("PROGRESS 3/12 Installing packages"), Some(ApplyLine { done: 3, total: 12, text: "Installing packages".into() }));
    assert_eq!(parse_progress_line("PROGRESS 12/12 Done\r"), Some(ApplyLine { done: 12, total: 12, text: "Done".into() }));
    assert_eq!(parse_progress_line("PROGRESS 0/5 Starting").unwrap().done, 0);
    assert_eq!(parse_progress_line("PROGRESS 1/2").unwrap().text, "");
    assert_eq!(parse_progress_line("  PROGRESS 1/2 padded  ").unwrap().text, "padded");
    for bad in [
        "", "PROGRESS", "PROGRESS ", "PROGRESS x/2 a", "PROGRESS 1/ a", "PROGRESS 1 2 a", "PROGRESS 3/2 over", "PROGRESS 1/0 zero",
        "PROGRESS -1/2 a", "PROGRESS 1/2x a", "PROGRESS 1234567/9999999 big", "progress 1/2 lower", "RESULT {}", "ERROR boom", "  + write x",
        "PROGRESS 1.5/2 a",
    ] {
        assert_eq!(parse_progress_line(bad), None, "{bad:?}");
    }
    // the text is cleaned: control characters out, secrets hidden, cut to 200 characters
    let noisy = parse_progress_line(&format!("PROGRESS 1/2 a\u{7}b sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghij {}", "z".repeat(400))).unwrap();
    assert!(!noisy.text.contains('\u{7}') && !noisy.text.contains("sk-ant-api03-ABCDEF"));
    assert!(noisy.text.chars().count() <= 200);
    assert_eq!(parse_error_line("ERROR apt failed"), Some("apt failed".into()));
    assert_eq!(parse_error_line("PROGRESS 1/2 x"), None);
}

#[test]
fn the_plan_and_the_bar() {
    let with_snapshot = plan(&options());
    assert_eq!(with_snapshot.first(), Some(&Step::CheckWsl));
    assert_eq!(with_snapshot.last(), Some(&Step::Snapshot));
    let mut no_snapshot = options();
    no_snapshot.snapshot = false;
    assert!(!plan(&no_snapshot).contains(&Step::Snapshot));
    // the bar goes 0 to 100, never backwards
    let mut last = 0;
    for index in 0..with_snapshot.len() {
        for tenth in 0..=10 {
            let p = percent_at(&with_snapshot, index, f64::from(tenth) / 10.0);
            assert!(p >= last, "{index} {tenth}: {p} < {last}");
            last = p;
        }
    }
    assert_eq!(percent_at(&with_snapshot, 0, 0.0), 0);
    assert_eq!(percent_at(&with_snapshot, with_snapshot.len() - 1, 1.0), 100);
    assert_eq!(percent_at(&with_snapshot, 99, 1.0), 100, "past the end is 100");
    // every step has a title and an id
    for step in with_snapshot {
        assert!(!step.title().is_empty() && !step.id().is_empty());
    }
}

#[test]
fn everything_lives_under_the_sandbox_folder_of_the_app_data() {
    let layout = Layout::new(Path::new("D:\\data\\potato"));
    let name = main_name();
    assert_eq!(layout.distro_dir(&name), Path::new("D:\\data\\potato").join("sandbox").join("pm-sbx-main"));
    assert!(layout.snapshots_dir(&name).starts_with(layout.root()));
    let base = layout.snapshot_path(&name, "base").unwrap();
    assert!(base.starts_with(layout.root()) && base.ends_with("base.tar"));
    for bad in ["", "../x", "a/b", "A", "x y", "a.b", &"a".repeat(31)] {
        assert!(layout.snapshot_path(&name, bad).is_none(), "{bad}");
    }
    assert!(valid_snapshot_id("before-cleanup-2"));
}

// ---- the whole run -------------------------------------------------------------------------------

struct Rig {
    guest: Arc<FakeGuest>,
    wsl: super::wsl::Wsl,
    layout: Layout,
    files: GuestFiles,
    lifecycle: Lifecycle,
    _folder: Scratch,
    data: Scratch,
}

fn rig() -> Rig {
    let guest = FakeGuest::new();
    let wsl = guest.wsl();
    let folder = guest_folder();
    let data = Scratch::new("data");
    let layout = Layout::new(&data.0);
    let lifecycle = Lifecycle::new(wsl.clone(), main_name()).with_timing(Timing::instant());
    Rig { guest, wsl, layout, files: GuestFiles::new(&folder.0), lifecycle, _folder: folder, data }
}

fn go(rig: &Rig, options: &InstallOptions, cancel: &AtomicBool) -> (Result<(), InstallError>, Vec<Progress>) {
    let env = InstallEnv { wsl: &rig.wsl, layout: &rig.layout, files: &rig.files, lifecycle: &rig.lifecycle };
    let mut seen = Vec::new();
    let result = run(&env, options, cancel, &|| 1_000, &mut |p| seen.push(p));
    (result, seen)
}

#[test]
fn a_full_install_runs_the_steps_in_order() {
    let rig = rig();
    let (result, _) = go(&rig, &options(), &AtomicBool::new(false));
    result.unwrap();

    let joined = rig.guest.joined();
    let at = |needle: &str| joined.iter().position(|c| c.contains(needle)).unwrap_or_else(|| panic!("no call with {needle}: {joined:#?}"));
    assert!(at("--status") < at("--install Ubuntu-24.04"));
    let dir = rig.layout.distro_dir(&main_name());
    assert!(joined[at("--install Ubuntu-24.04")].contains(&format!("--name pm-sbx-main --location {} --no-launch", dir.to_string_lossy())));
    assert!(dir.is_dir(), "the distro folder was made under the app data");
    assert!(dir.starts_with(&rig.data.0));
    assert!(at("--install Ubuntu-24.04") < at("/bin/mkdir -p /usr/local/share/pm-sbx"));
    assert!(at("/bin/mkdir -p") < at("/bin/tar -x"));
    assert!(at("/bin/tar -x") < at("apply-recipe.py"));
    assert!(joined[at("apply-recipe.py")].ends_with("--recipe xfce --recipe coding --opt coding.pkg=pnpm --opt coding.languages=python,go"));
    assert!(joined[at("apply-recipe.py")].contains("-u root --exec /usr/bin/python3 -I /usr/local/share/pm-sbx/desktop/apply-recipe.py"));
    assert!(at("apply-recipe.py") < at("--terminate pm-sbx-main"));
    assert!(at("--terminate pm-sbx-main") < at("sleep infinity"), "the boot check starts after the restart");
    assert!(at("sleep infinity") < at("--export pm-sbx-main"));

    // the guest files went over stdin as a tar, and only that call carried them
    let tar_call = rig.guest.calls().into_iter().find(|c| c.args.iter().any(|a| a == "/bin/tar")).unwrap();
    let archive = read_tar(&tar_call.stdin.clone().unwrap());
    assert!(archive.0.iter().any(|(n, _, _)| n == "desktop/apply-recipe.py"));
    assert!(tar_call.args.iter().all(|a| !a.contains("apply-recipe")), "no file content or name list in the arguments");

    // the reset point goes under the app data too
    let export = &joined[at("--export pm-sbx-main")];
    let base = rig.layout.snapshot_path(&main_name(), "base").unwrap();
    assert!(export.ends_with(&*base.to_string_lossy()));
    assert!(base.parent().unwrap().is_dir());

    // ready to use later, but stopped now
    assert!(rig.guest.st.lock().installed.contains(&"pm-sbx-main".to_string()));
    assert!(rig.guest.st.lock().running.is_empty());
    assert!(!rig.guest.holder_alive());
    assert_eq!(rig.guest.st.lock().elevated_seen, 0, "an install never enables the Linux feature");
}

#[test]
fn the_progress_is_steady_and_carries_the_installer_lines() {
    let rig = rig();
    let (result, progress) = go(&rig, &options(), &AtomicBool::new(false));
    result.unwrap();
    assert!(!progress.is_empty());
    let mut last = 0;
    for p in &progress {
        assert!(p.percent >= last, "{p:?}");
        assert!(p.index >= 1 && p.index <= p.of);
        last = p.percent;
    }
    assert_eq!(progress.last().unwrap().percent, 100);
    assert_eq!(progress.first().unwrap().step, "check_linux");
    let details: Vec<&str> = progress.iter().filter(|p| p.step == "apply").filter_map(|p| p.detail.as_deref()).collect();
    assert_eq!(details, vec!["Installing packages", "Installing Chrome", "Done"]);
    // the step titles are the plain ones
    assert!(progress.iter().any(|p| p.title == "Installing the desktop and tools"));
    // the apply bar moves with the installer's n/total
    let apply: Vec<u8> = progress.iter().filter(|p| p.step == "apply").map(|p| p.percent).collect();
    assert!(apply.windows(2).all(|w| w[0] <= w[1]) && apply.first() < apply.last());
}

#[test]
fn no_snapshot_means_no_export() {
    let rig = rig();
    let mut o = options();
    o.snapshot = false;
    go(&rig, &o, &AtomicBool::new(false)).0.unwrap();
    assert!(!rig.guest.joined().iter().any(|c| c.starts_with("--export")));
}

#[test]
fn a_pc_without_the_linux_feature_stops_at_the_first_step_and_never_turns_it_on() {
    let rig = rig();
    rig.guest.st.lock().wsl_ok = false;
    let (result, progress) = go(&rig, &options(), &AtomicBool::new(false));
    let error = result.unwrap_err();
    assert_eq!(error, InstallError::NeedsWindowsFeature);
    assert_eq!(error.message(), "Needs Windows' Linux feature (admin + restart).");
    assert_eq!(error.code(), "SANDBOX_NEEDS_LINUX_FEATURE");
    assert_eq!(progress.len(), 1);
    assert_eq!(rig.guest.st.lock().elevated_seen, 0);
    assert!(!rig.layout.distro_dir(&main_name()).exists(), "no folder was made");
    assert!(!rig.guest.joined().iter().any(|c| c.contains("--install")));
}

#[test]
fn an_existing_sandbox_is_not_overwritten() {
    let rig = rig();
    rig.guest.st.lock().installed.push("pm-sbx-main".to_string());
    let (result, _) = go(&rig, &options(), &AtomicBool::new(false));
    assert_eq!(result.unwrap_err(), InstallError::AlreadyInstalled);
    assert!(rig.guest.st.lock().installed.contains(&"pm-sbx-main".to_string()), "and it is not unregistered either");
    assert!(!rig.guest.joined().iter().any(|c| c.starts_with("--unregister")));
}

#[test]
fn bad_options_stop_before_any_call() {
    let rig = rig();
    let mut o = options();
    o.addons.push("gaming".to_string());
    let (result, _) = go(&rig, &o, &AtomicBool::new(false));
    assert!(matches!(result, Err(InstallError::BadOptions(_))));
    assert!(rig.guest.calls().is_empty());
}

#[test]
fn missing_guest_files_stop_before_any_call() {
    let mut rig = rig();
    let empty = Scratch::new("noguest");
    rig.files = GuestFiles::new(&empty.0);
    let (result, _) = go(&rig, &options(), &AtomicBool::new(false));
    assert_eq!(result.unwrap_err(), InstallError::GuestFilesMissing);
    assert!(rig.guest.calls().is_empty());
}

#[test]
fn a_failing_recipe_reports_the_installer_error_and_takes_the_new_distro_away() {
    let rig = rig();
    rig.guest.st.lock().apply_lines = vec!["PROGRESS 1/4 Installing packages".into(), "ERROR apt-get install failed".into()];
    rig.guest.st.lock().apply_code = 1;
    let (result, progress) = go(&rig, &options(), &AtomicBool::new(false));
    match result.unwrap_err() {
        InstallError::Step { step, reason } => {
            assert_eq!(step, "Installing the desktop and tools");
            assert_eq!(reason, "apt-get install failed");
        }
        other => panic!("{other:?}"),
    }
    assert!(progress.iter().all(|p| p.percent < 100));
    assert!(rig.guest.st.lock().installed.is_empty(), "a retry starts clean");
    assert!(rig.guest.joined().contains(&"--unregister pm-sbx-main".to_string()));
}

#[test]
fn an_exit_code_without_an_error_line_still_gets_a_reason() {
    for (code, text) in [(2, "choices"), (3, "root"), (4, "rights"), (9, "unexpectedly")] {
        let rig = rig();
        rig.guest.st.lock().apply_lines = vec![];
        rig.guest.st.lock().apply_code = code;
        let (result, _) = go(&rig, &options(), &AtomicBool::new(false));
        match result.unwrap_err() {
            InstallError::Step { reason, .. } => assert!(reason.contains(text), "{code}: {reason}"),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn a_failed_distro_download_is_a_plain_failure_and_cleans_up() {
    let rig = rig();
    rig.guest.st.lock().install_code = 1;
    let (result, _) = go(&rig, &options(), &AtomicBool::new(false));
    match result.unwrap_err() {
        InstallError::Step { step, .. } => assert_eq!(step, "Downloading Ubuntu"),
        other => panic!("{other:?}"),
    }
    assert!(!rig.guest.joined().iter().any(|c| c.contains("/bin/tar")));
}

#[test]
fn a_boot_check_that_fails_fails_the_install() {
    let rig = rig();
    rig.guest.st.lock().egress_probe_passes = false;
    let (result, _) = go(&rig, &options(), &AtomicBool::new(false));
    match result.unwrap_err() {
        InstallError::Step { step, reason } => {
            assert_eq!(step, "Checking it starts safely");
            assert!(reason.contains("network walls"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
    assert!(rig.guest.st.lock().installed.is_empty());
    assert!(!rig.guest.joined().iter().any(|c| c.starts_with("--export")), "no reset point of a sandbox that does not start safely");
}

#[test]
fn a_cancel_before_the_start_does_nothing() {
    let rig = rig();
    let (result, _) = go(&rig, &options(), &AtomicBool::new(true));
    assert_eq!(result.unwrap_err(), InstallError::Cancelled);
    assert!(rig.guest.calls().is_empty());
}

#[test]
fn a_cancel_half_way_stops_at_the_next_step_and_takes_the_new_distro_away() {
    let rig = rig();
    let cancel = AtomicBool::new(false);
    let env = InstallEnv { wsl: &rig.wsl, layout: &rig.layout, files: &rig.files, lifecycle: &rig.lifecycle };
    let result = run(&env, &options(), &cancel, &|| 1_000, &mut |p| {
        if p.step == "copy" {
            cancel.store(true, Ordering::SeqCst);
        }
    });
    assert_eq!(result.unwrap_err(), InstallError::Cancelled);
    assert!(rig.guest.st.lock().installed.is_empty());
    assert!(!rig.guest.joined().iter().any(|c| c.contains("apply-recipe")), "the recipes did not start");
}

#[test]
fn nothing_secret_is_made_or_sent_during_an_install_until_the_boot_check() {
    // The boot check is the one start and it makes the one token; the archive is the one other thing sent over stdin
    // (besides the read-only readiness script).
    let rig = rig();
    go(&rig, &options(), &AtomicBool::new(false)).0.unwrap();
    let with_stdin: Vec<_> = rig.guest.calls().into_iter().filter(|c| c.stdin.is_some()).collect();
    let archives = with_stdin.iter().filter(|c| c.args.iter().any(|a| a == "/bin/tar")).count();
    let tokens = with_stdin.iter().filter(|c| c.args.iter().any(|a| a.contains("control.token"))).count();
    assert_eq!(archives, 1);
    assert_eq!(tokens, 1);
    // no file was written under the app data except by directory creation: only folders exist there
    fn only_dirs(dir: &Path) -> bool {
        std::fs::read_dir(dir).unwrap().flatten().all(|e| e.path().is_dir() && only_dirs(&e.path()))
    }
    assert!(only_dirs(&rig.data.0), "the app wrote no file on Windows");
}

#[test]
fn error_texts_are_plain() {
    for error in [
        InstallError::NeedsWindowsFeature,
        InstallError::AlreadyInstalled,
        InstallError::BadOptions("an add-on is coding or daily"),
        InstallError::GuestFilesMissing,
        InstallError::Cancelled,
        InstallError::Step { step: "Downloading Ubuntu", reason: "it stopped unexpectedly".into() },
    ] {
        assert!(error.code().starts_with("SANDBOX_"));
        assert!(error.message().ends_with('.') || error.message().ends_with("unexpectedly"), "{}", error.message());
    }
    assert_eq!(apply_exit_reason(None), "it stopped unexpectedly");
}
