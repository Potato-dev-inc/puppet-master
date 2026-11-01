//! The manager the settings page talks to: answers to options, status, a threaded install with events, cancel, busy, the typed
//! remove, and the confirm token for Windows' Linux feature. A fake sandbox stands in for Linux; no wsl.exe runs.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::control::Action;
use super::fakes_tests::Scratch;
use super::install::{percent_at, plan, Desktop, InstallError, InstallOptions, Layout, Progress};
use super::manager::*;
use super::wsl::SandboxName;
use super::*;

fn recipes_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("potato-sandbox").join("recipes")
}

fn name() -> SandboxName {
    SandboxName::main()
}

#[derive(Default)]
struct Log {
    progress: Mutex<Vec<InstallView>>,
    changed: Mutex<u32>,
}

impl Events for Log {
    fn install_progress(&self, view: &InstallView) {
        self.progress.lock().push(view.clone());
    }
    fn changed(&self) {
        *self.changed.lock() += 1;
    }
}

fn answers(look: &str, uses: &[&str], options: &[(&str, &str)]) -> WizardAnswers {
    WizardAnswers {
        look: look.to_string(),
        uses: uses.iter().map(|s| s.to_string()).collect(),
        options: options.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        reset_point: true,
    }
}

fn manager_over(sandbox: Arc<dyn Sandbox>, log: Arc<Log>) -> Arc<Manager> {
    Manager::new(sandbox, name(), None, Some(recipes_dir()), log)
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < Duration::from_secs(10), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A sandbox that records what it was asked, can hold an install until released, and can fail it.
struct Spy {
    installed: AtomicBool,
    calls: Mutex<Vec<String>>,
    options: Mutex<Option<InstallOptions>>,
    hold: AtomicBool,
    cancel: AtomicBool,
    fail_with: Mutex<Option<SandboxError>>,
    idle: Mutex<u32>,
    no_linux: AtomicBool,
}

impl Spy {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            installed: AtomicBool::new(false),
            calls: Mutex::new(Vec::new()),
            options: Mutex::new(None),
            hold: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            fail_with: Mutex::new(None),
            idle: Mutex::new(15),
            no_linux: AtomicBool::new(false),
        })
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().clone()
    }
}

impl Sandbox for Spy {
    fn status(&self) -> SbxResult<Status> {
        let state = if self.installed.load(Ordering::SeqCst) { State::Stopped } else { State::NotInstalled };
        Ok(Status { state, reason: None, name: name().to_string(), idle_minutes: *self.idle.lock() })
    }
    fn install(&self, options: &InstallOptions, progress: &mut dyn FnMut(Progress)) -> SbxResult<()> {
        self.cancel.store(false, Ordering::SeqCst);
        self.calls.lock().push("install".into());
        *self.options.lock() = Some(options.clone());
        let steps = plan(options);
        for (i, step) in steps.iter().enumerate() {
            while self.hold.load(Ordering::SeqCst) && !self.cancel.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(2));
            }
            if self.cancel.load(Ordering::SeqCst) {
                return Err(InstallError::Cancelled.into());
            }
            if let Some(error) = self.fail_with.lock().clone() {
                if i == 2 {
                    return Err(error);
                }
            }
            progress(Progress { step: step.id(), title: step.title(), index: i + 1, of: steps.len(), percent: percent_at(&steps, i, 0.5), detail: Some(format!("line {i}")) });
        }
        self.installed.store(true, Ordering::SeqCst);
        Ok(())
    }
    fn start(&self) -> SbxResult<()> {
        self.calls.lock().push("start".into());
        Ok(())
    }
    fn stop(&self) -> SbxResult<()> {
        self.calls.lock().push("stop".into());
        Ok(())
    }
    fn snapshot(&self, snapshot: &str) -> SbxResult<()> {
        self.calls.lock().push(format!("snapshot {snapshot}"));
        Ok(())
    }
    fn reset(&self, snapshot: &str) -> SbxResult<()> {
        self.calls.lock().push(format!("reset {snapshot}"));
        Ok(())
    }
    fn remove(&self) -> SbxResult<()> {
        self.calls.lock().push("remove".into());
        self.installed.store(false, Ordering::SeqCst);
        Ok(())
    }
    fn observe(&self) -> SbxResult<Observation> {
        Err(SandboxError::new("SANDBOX_NOT_READY", "no screen"))
    }
    fn act(&self, _actions: &[Action]) -> SbxResult<Observation> {
        Err(SandboxError::new("SANDBOX_NOT_READY", "no screen"))
    }
    fn browser(&self, _args: &[String]) -> SbxResult<String> {
        Ok(String::new())
    }
    fn screen_target(&self) -> SbxResult<ScreenTarget> {
        Err(SandboxError::new("SANDBOX_NOT_READY", "no screen"))
    }
    fn cancel_install(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
    fn linux_feature(&self) -> bool {
        !self.no_linux.load(Ordering::SeqCst)
    }
    fn set_idle_minutes(&self, minutes: u32) -> u32 {
        let used = super::lifecycle::clamp_idle_minutes(minutes);
        *self.idle.lock() = used;
        used
    }
}

// ---- the recipe menu and the answers -----------------------------------------------------------

#[test]
fn the_menu_lists_the_shipped_recipes_without_base_and_with_their_options() {
    let recipes = menu(&recipes_dir()).unwrap();
    let ids: Vec<&str> = recipes.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, vec!["coding", "daily", "light", "xfce", "zorin-look"]);
    let coding = recipes.iter().find(|r| r.id == "coding").unwrap();
    let option = |id: &str| coding.options.iter().find(|o| o.id == id).unwrap();
    assert_eq!(option("pkg").kind, "single");
    assert_eq!(option("pkg").choices.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["npm", "pnpm", "yarn", "bun"]);
    assert_eq!(option("pkg").default, serde_json::json!("npm"));
    assert_eq!(option("languages").kind, "multi");
    assert_eq!(option("languages").default, serde_json::json!([]));
    assert!(option("languages").choices.iter().any(|c| c.id == "rust"));
    assert!(option("extras").choices.len() >= 4, "the extras checklist comes from the recipe");
    let daily = recipes.iter().find(|r| r.id == "daily").unwrap();
    assert_eq!(daily.options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), vec!["media", "images"]);
    let look = recipes.iter().find(|r| r.id == "zorin-look").unwrap();
    assert_eq!(look.requires.as_deref(), Some("xfce"));
    assert_eq!(recipes.iter().find(|r| r.id == "xfce").unwrap().exclusive.as_deref(), Some("desktop"));
    assert!(menu(&recipes_dir().join("nope")).unwrap_err().code == "SANDBOX_GUEST_FILES_MISSING");
}

#[test]
fn answers_become_install_options_checked_against_the_recipes() {
    let recipes = menu(&recipes_dir()).unwrap();
    let a = answers(
        "zorin",
        &["coding", "daily", "coding"],
        &[("coding.pkg", "pnpm"), ("coding.languages", "python,go"), ("coding.extras", ""), ("daily.media", "mpv")],
    );
    let options = to_options(&name(), &a, Some(&recipes)).unwrap();
    assert_eq!(options.desktop, Desktop::Zorin);
    assert_eq!(options.addons, vec!["coding".to_string(), "daily".to_string()], "listed once");
    assert!(options.snapshot);
    assert_eq!(
        options.recipe_args(),
        vec![
            "--recipe", "xfce", "--recipe", "zorin-look", "--recipe", "coding", "--recipe", "daily",
            "--opt", "coding.languages=python,go", "--opt", "coding.pkg=pnpm", "--opt", "daily.media=mpv"
        ]
    );
    let mut light = answers("light", &["daily"], &[]);
    light.reset_point = false;
    let options = to_options(&name(), &light, Some(&recipes)).unwrap();
    assert_eq!(options.desktop, Desktop::Light);
    assert!(!options.snapshot);
}

#[test]
fn bad_answers_are_refused_with_a_plain_sentence() {
    let recipes = menu(&recipes_dir()).unwrap();
    let refused = |a: WizardAnswers| to_options(&name(), &a, Some(&recipes)).unwrap_err();
    assert_eq!(refused(answers("", &["coding"], &[])).code, "SANDBOX_BAD_OPTIONS");
    assert_eq!(refused(answers("windows", &["coding"], &[])).message, "Pick how the desktop should look.");
    assert_eq!(refused(answers("ubuntu", &[], &[])).message, "Pick coding, daily use, or both.");
    assert_eq!(refused(answers("ubuntu", &["gaming"], &[])).message, "Pick coding, daily use, or both.");
    // a choice the recipe does not have, a list with one unknown id, an unknown option
    assert!(refused(answers("ubuntu", &["coding"], &[("coding.pkg", "pip")])).message.contains("not one this version"));
    assert!(refused(answers("ubuntu", &["coding"], &[("coding.languages", "python,cobol")])).message.contains("not one this version"));
    assert!(refused(answers("ubuntu", &["coding"], &[("coding.nope", "x")])).message.contains("not one this version"));
    assert!(refused(answers("ubuntu", &["coding"], &[("nodot", "x")])).message.contains("not one this version"));
    // an option of a recipe that is not part of the answers (the menu knows it, the install does not accept it)
    assert_eq!(refused(answers("ubuntu", &["coding"], &[("daily.media", "mpv")])).code, "SANDBOX_BAD_OPTIONS");
    // a value the install would not pass on to the installer
    assert_eq!(refused(answers("ubuntu", &["coding"], &[("coding.pkg", "pnpm;ls")])).code, "SANDBOX_BAD_OPTIONS");
}

#[test]
fn without_the_recipe_files_the_install_checks_still_apply() {
    let bad = answers("ubuntu", &["coding"], &[("coding.pkg", "A B")]);
    assert!(to_options(&name(), &bad, None).is_err());
    assert!(to_options(&name(), &answers("ubuntu", &["coding"], &[("coding.pkg", "pnpm")]), None).is_ok());
}

// ---- status -------------------------------------------------------------------------------------------

#[test]
fn status_tells_what_the_page_needs() {
    let fake = Arc::new(FakeSandbox::not_installed());
    let m = manager_over(fake.clone(), Arc::new(Log::default()));
    let view = m.status().unwrap();
    assert_eq!(view.state, "not_installed");
    assert!(view.linux_feature);
    assert_eq!(view.idle_minutes, 15);
    assert_eq!(view.install.state, "idle");
    assert_eq!((view.disk_bytes, view.has_reset_point), (None, false));

    let missing = Arc::new(FakeSandbox::not_installed().without_linux_feature());
    let view = manager_over(missing, Arc::new(Log::default())).status().unwrap();
    assert!(!view.linux_feature, "the page shows the 'turn on Windows' Linux feature' button first");
}

#[test]
fn status_reads_the_disk_and_the_reset_point_from_the_sandbox_folder() {
    let data = Scratch::new("manager-disk");
    let layout = Layout::new(&data.0);
    let dir = layout.distro_dir(&name());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ext4.vhdx"), vec![0u8; 3000]).unwrap();
    let snap = layout.snapshot_path(&name(), "base").unwrap();
    std::fs::create_dir_all(snap.parent().unwrap()).unwrap();
    std::fs::write(&snap, vec![0u8; 700]).unwrap();
    let m = Manager::new(Arc::new(FakeSandbox::new()), name(), Some(layout), None, Arc::new(NoEvents));
    let view = m.status().unwrap();
    assert_eq!(view.state, "stopped");
    assert_eq!(view.disk_bytes, Some(3000));
    assert!(view.has_reset_point);
    assert_eq!(view.reset_point_bytes, Some(700));
}

// ---- install -------------------------------------------------------------------------------------------

#[test]
fn an_install_reports_each_step_and_ends_done() {
    let log = Arc::new(Log::default());
    let spy = Spy::new();
    let m = manager_over(spy.clone(), log.clone());
    let options = m.prepare_install(&answers("ubuntu", &["coding"], &[("coding.pkg", "bun"), ("coding.languages", "rust")])).unwrap();
    m.run_install(&options);
    let seen = log.progress.lock().clone();
    assert!(seen.len() > plan(&options).len() / 2);
    assert!(seen.windows(2).all(|w| w[0].percent <= w[1].percent), "the bar never goes back");
    assert!(seen.iter().any(|v| v.detail.as_deref() == Some("line 3")), "the installer's line is shown");
    assert!(seen.iter().take(seen.len() - 1).all(|v| v.state == "running"));
    let last = seen.last().unwrap();
    assert_eq!((last.state, last.percent), ("done", 100));
    assert_eq!(*log.changed.lock(), 1);
    assert_eq!(m.status().unwrap().install.state, "done");
    assert_eq!(m.status().unwrap().state, "stopped");
    // what the install got: the wizard's answers
    let got = spy.options.lock().clone().unwrap();
    assert_eq!(got.options, vec![("coding.languages".to_string(), "rust".to_string()), ("coding.pkg".to_string(), "bun".to_string())]);
    m.dismiss_install();
    assert_eq!(m.status().unwrap().install.state, "idle");
}

#[test]
fn start_install_runs_on_a_thread_and_the_page_follows_the_events() {
    let log = Arc::new(Log::default());
    let spy = Spy::new();
    spy.hold.store(true, Ordering::SeqCst);
    let m = manager_over(spy.clone(), log.clone());
    m.start_install(&answers("light", &["daily"], &[])).unwrap();
    assert_eq!(m.status().unwrap().state, "installing", "the page shows the bar while it runs");
    assert_eq!(log.progress.lock().first().unwrap().state, "running", "an event goes out at once");
    // while it runs, nothing else may change the sandbox
    for result in [m.start(), m.stop(), m.reset(), m.remove("remove")] {
        let error = result.unwrap_err();
        assert_eq!(error.code, "SANDBOX_BUSY");
        assert!(error.message.contains("Set up is still running"));
    }
    assert_eq!(m.start_install(&answers("light", &["daily"], &[])).unwrap_err().code, "SANDBOX_BUSY");
    wait_for("the install to reach the sandbox", || !spy.calls().is_empty());
    assert!(spy.calls().iter().all(|c| c == "install"), "none of them reached the sandbox: {:?}", spy.calls());
    spy.hold.store(false, Ordering::SeqCst);
    wait_for("the install to end", || m.status().unwrap().install.state == "done");
    wait_for("the lane to be free", || m.start().is_ok());
    assert!(spy.calls().contains(&"start".to_string()));
}

#[test]
fn cancel_stops_the_install_and_the_page_hears_cancelled() {
    let log = Arc::new(Log::default());
    let spy = Spy::new();
    spy.hold.store(true, Ordering::SeqCst);
    let m = manager_over(spy.clone(), log.clone());
    assert_eq!(m.cancel_install().unwrap_err().code, "SANDBOX_NOT_INSTALLING");
    m.start_install(&answers("ubuntu", &["coding"], &[])).unwrap();
    wait_for("the install to start", || !spy.calls().is_empty());
    m.cancel_install().unwrap();
    wait_for("the cancel", || m.status().unwrap().install.state == "cancelled");
    let view = m.status().unwrap();
    assert_eq!(view.install.error.as_ref().unwrap().code, "SANDBOX_INSTALL_CANCELLED");
    assert!(!view.install.cancelling);
    assert_eq!(view.state, "not_installed");
    // a new try is allowed after a cancel
    spy.hold.store(false, Ordering::SeqCst);
    wait_for("the lane to be free", || m.start_install(&answers("ubuntu", &["coding"], &[])).is_ok());
    wait_for("the second install", || m.status().unwrap().install.state == "done");
}

#[test]
fn a_failed_install_keeps_its_plain_error_and_can_be_tried_again() {
    let log = Arc::new(Log::default());
    let spy = Spy::new();
    *spy.fail_with.lock() = Some(SandboxError::new("SANDBOX_INSTALL_FAILED", "Set up failed at \"Downloading Ubuntu\": no internet."));
    let m = manager_over(spy.clone(), log.clone());
    m.start_install(&answers("ubuntu", &["daily"], &[])).unwrap();
    wait_for("the failure", || m.status().unwrap().install.state == "failed");
    let view = m.status().unwrap();
    assert_eq!(view.install.error.as_ref().unwrap().message, "Set up failed at \"Downloading Ubuntu\": no internet.");
    assert!(!view.install.needs_linux_feature);
    assert_eq!(view.state, "not_installed");
    *spy.fail_with.lock() = None;
    wait_for("the lane to be free", || m.start_install(&answers("ubuntu", &["daily"], &[])).is_ok());
    wait_for("the second try", || m.status().unwrap().install.state == "done");
}

#[test]
fn without_windows_linux_feature_the_install_is_refused_at_once_with_the_feature_error() {
    let spy = Spy::new();
    spy.no_linux.store(true, Ordering::SeqCst);
    let m = manager_over(spy.clone(), Arc::new(Log::default()));
    let error = m.start_install(&answers("ubuntu", &["coding"], &[])).unwrap_err();
    assert_eq!(error.code, "SANDBOX_NEEDS_LINUX_FEATURE");
    assert!(spy.calls().is_empty());
    assert_eq!(m.status().unwrap().install.state, "idle");
    // and an install that finds the feature missing while it runs says so to the page
    let fake = Arc::new(FakeSandbox::not_installed().without_linux_feature());
    let m = manager_over(fake, Arc::new(Log::default()));
    m.run_install(&InstallOptions::new(name(), Desktop::Light));
    let view = m.status().unwrap();
    assert_eq!(view.install.state, "failed");
    assert!(view.install.needs_linux_feature);
}

#[test]
fn an_install_over_an_installed_sandbox_and_bad_answers_never_start() {
    let fake = Arc::new(FakeSandbox::new());
    let m = manager_over(fake.clone(), Arc::new(Log::default()));
    assert_eq!(m.start_install(&answers("ubuntu", &["coding"], &[])).unwrap_err().code, "SANDBOX_ALREADY_INSTALLED");
    assert_eq!(m.start_install(&answers("", &["coding"], &[])).unwrap_err().code, "SANDBOX_BAD_OPTIONS");
    assert!(fake.calls().is_empty());
    assert_eq!(m.status().unwrap().install.state, "idle");
}

// ---- start, stop, reset, remove -----------------------------------------------------------------------

#[test]
fn start_stop_and_reset_go_to_the_sandbox_and_tell_the_page() {
    let log = Arc::new(Log::default());
    let fake = Arc::new(FakeSandbox::new());
    let m = manager_over(fake.clone(), log.clone());
    m.start().unwrap();
    assert_eq!(m.status().unwrap().state, "running");
    m.stop().unwrap();
    assert_eq!(m.status().unwrap().state, "stopped");
    m.start().unwrap();
    m.reset().unwrap();
    assert_eq!(m.status().unwrap().state, "stopped", "a reset leaves it stopped");
    assert_eq!(fake.calls(), vec![Recorded::Start, Recorded::Stop, Recorded::Start, Recorded::Reset("base".to_string())]);
    assert_eq!(*log.changed.lock(), 4);
    // a failure of the sandbox is passed on in its own words
    fake.fail_next(SandboxError::new("SANDBOX_NO_SNAPSHOT", "There is no saved reset point to go back to."));
    assert_eq!(m.reset().unwrap_err().message, "There is no saved reset point to go back to.");
    assert!(m.start().is_ok(), "the lane is free again after an error");
}

#[test]
fn remove_needs_the_word_typed_and_nothing_happens_without_it() {
    let fake = Arc::new(FakeSandbox::new());
    let m = manager_over(fake.clone(), Arc::new(Log::default()));
    for typed in ["", "yes", "removed", "rem ove", "delete"] {
        let error = m.remove(typed).unwrap_err();
        assert_eq!(error.code, "SANDBOX_CONFIRM_WORD", "{typed:?}");
    }
    assert!(fake.calls().is_empty());
    m.remove("  Remove ").unwrap();
    assert_eq!(fake.calls(), vec![Recorded::Remove]);
    assert_eq!(m.status().unwrap().state, "not_installed");
}

#[test]
fn the_idle_stop_minutes_are_kept_between_1_and_240() {
    let fake = Arc::new(FakeSandbox::new());
    let m = manager_over(fake, Arc::new(Log::default()));
    assert_eq!(m.set_idle_minutes(30), 30);
    assert_eq!(m.status().unwrap().idle_minutes, 30);
    assert_eq!(m.set_idle_minutes(0), 1);
    assert_eq!(m.set_idle_minutes(100_000), 240);
    assert_eq!(m.status().unwrap().idle_minutes, 240);
}

// ---- Windows' Linux feature ----------------------------------------------------------------------------

#[test]
fn the_linux_feature_token_is_single_use_and_expires() {
    let now = Arc::new(AtomicI64::new(1_000));
    let clock = {
        let now = now.clone();
        Box::new(move || now.load(Ordering::SeqCst)) as Box<dyn Fn() -> i64 + Send + Sync>
    };
    let m = Manager::with_clock(Arc::new(FakeSandbox::not_installed()), name(), None, None, Arc::new(NoEvents), clock);
    // nothing asked yet
    assert_eq!(m.take_linux_feature_ask("x").unwrap_err().code, "SANDBOX_NOT_CONFIRMED");
    let ask = m.ask_linux_feature();
    assert!(ask.title.contains("Linux feature"));
    assert!(ask.body.contains("administrator") && ask.body.contains("restart"));
    assert_eq!(ask.token.len(), 32);
    m.take_linux_feature_ask(&ask.token).unwrap();
    assert!(m.take_linux_feature_ask(&ask.token).is_err(), "used once");
    // a newer ask replaces an older one
    let first = m.ask_linux_feature();
    let second = m.ask_linux_feature();
    assert!(m.take_linux_feature_ask(&first.token).is_err());
    assert!(m.take_linux_feature_ask(&second.token).is_err(), "a wrong try also closes the dialog");
    // ten minutes later it is too late
    let late = m.ask_linux_feature();
    now.fetch_add(10 * 60 * 1000 + 1, Ordering::SeqCst);
    assert!(m.take_linux_feature_ask(&late.token).is_err());
    let fresh = m.ask_linux_feature();
    now.fetch_add(9 * 60 * 1000, Ordering::SeqCst);
    assert!(m.take_linux_feature_ask(&fresh.token).is_ok());
}

#[test]
fn the_manager_never_turns_the_feature_on_by_itself() {
    // The Sandbox trait has no way to do it: a PC without the feature just fails the install with the feature error.
    let fake = Arc::new(FakeSandbox::not_installed().without_linux_feature());
    let m = manager_over(fake.clone(), Arc::new(Log::default()));
    let _ = m.start_install(&answers("ubuntu", &["coding"], &[]));
    let _ = m.ask_linux_feature();
    assert!(fake.calls().is_empty(), "nothing reached the sandbox");
}
