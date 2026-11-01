//! The install of an adapter, with a fake runner that writes files instead of starting npm.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

use super::*;
use crate::acp::config::{AddonInfo, AdapterPin};

struct Tmp(PathBuf);
impl Tmp {
    fn new() -> Tmp {
        Tmp(std::env::temp_dir().join(format!("pm-acp-install-{}", uuid::Uuid::new_v4())))
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Host(PathBuf);
impl HostEnv for Host {
    fn var(&self, _: &str) -> Option<OsString> {
        None
    }
    fn which(&self, _: &str) -> Option<PathBuf> {
        None
    }
    fn adapters_dir(&self) -> PathBuf {
        self.0.join("acp").join("adapters")
    }
    fn homes_dir(&self) -> PathBuf {
        self.0.join("acp").join("homes")
    }
}

const GOOD: &str = "console.log('the adapter we tested');";
const ENTRY: &str = "{adapters}/demo/node_modules/@scope/demo-acp/dist/index.js";

fn sha(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn cfg() -> AgentConfig {
    AgentConfig {
        id: "demo".into(),
        label: "Demo".into(),
        command: "node".into(),
        pins: vec![AdapterPin { package: "@scope/demo-acp".into(), version: "1.2.3".into(), entry: ENTRY.into(), sha256: Some(sha(GOOD)) }],
        addon: AddonInfo { size_mb: Some(12), signin_hint: "Sign in in the terminal.".into() },
        ..AgentConfig::default()
    }
}

type Call = (Vec<String>, PathBuf, String);

/// What the fake npm does: the files a real install of the package would leave.
struct Fake {
    npm: Option<PathBuf>,
    /// The entry file's content the "download" has.
    content: String,
    version: String,
    code: i32,
    lines: Vec<String>,
    calls: Mutex<Vec<Call>>,
    runs: AtomicUsize,
}

impl Fake {
    fn ok() -> Fake {
        Fake {
            npm: Some(PathBuf::from("npm")),
            content: GOOD.into(),
            version: "1.2.3".into(),
            code: 0,
            lines: vec!["npm http fetch GET 200 https://registry.npmjs.org/@scope%2fdemo-acp 120ms".into()],
            calls: Mutex::new(Vec::new()),
            runs: AtomicUsize::new(0),
        }
    }
}

impl Runner for Fake {
    fn find_npm(&self) -> Option<PathBuf> {
        self.npm.clone()
    }
    fn run(&self, _npm: &Path, args: &[String], cwd: &Path, on_line: &mut dyn FnMut(&str)) -> Result<Option<i32>, String> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        let manifest = std::fs::read_to_string(cwd.join("package.json")).unwrap_or_default();
        self.calls.lock().push((args.to_vec(), cwd.to_path_buf(), manifest));
        for line in &self.lines {
            on_line(line);
        }
        if self.code == 0 {
            let pkg = cwd.join("node_modules").join("@scope").join("demo-acp");
            std::fs::create_dir_all(pkg.join("dist")).unwrap();
            std::fs::write(pkg.join("package.json"), format!(r#"{{"name":"@scope/demo-acp","version":"{}"}}"#, self.version)).unwrap();
            std::fs::write(pkg.join("dist").join("index.js"), &self.content).unwrap();
        }
        Ok(Some(self.code))
    }
}

fn install(addons: &Addons, host: &Host, fake: &Fake) -> Result<String, InstallError> {
    addons.begin("demo").unwrap();
    addons.install_blocking(&cfg(), host, fake, &|| {})
}

#[test]
fn an_install_writes_the_pinned_package_runs_npm_safely_and_checks_the_file() {
    let tmp = Tmp::new();
    let host = Host(tmp.0.clone());
    let addons = Addons::new();
    let before = addons.status(&cfg(), &host);
    assert_eq!(before.state, AddonState::NotInstalled);
    assert_eq!(before.size_mb, Some(12));
    assert_eq!(before.pinned_version.as_deref(), Some("1.2.3"));

    let fake = Fake::ok();
    assert_eq!(install(&addons, &host, &fake).unwrap(), "1.2.3");

    let calls = fake.calls.lock();
    let (args, cwd, manifest) = &calls[0];
    assert_eq!(args.iter().map(String::as_str).collect::<Vec<_>>(), NPM_ARGS);
    assert!(args.iter().any(|a| a == "--ignore-scripts"), "install scripts never run");
    assert!(!args.iter().any(|a| a == "-g" || a == "--global" || a.starts_with("--ignore-scripts=") || a.starts_with("--prefix")), "never global: {args:?}");
    assert_eq!(cwd, &host.adapters_dir().join("demo"), "npm runs inside the add-on's own folder");
    let json: serde_json::Value = serde_json::from_str(manifest).unwrap();
    assert_eq!(json["dependencies"]["@scope/demo-acp"], "1.2.3", "the exact version, no range");
    assert_eq!(json["dependencies"].as_object().unwrap().len(), 1);
    assert_eq!(json["private"], true);

    let after = addons.status(&cfg(), &host);
    assert_eq!(after.state, AddonState::Installed);
    assert_eq!(after.version.as_deref(), Some("1.2.3"));
    assert!(pin::enforce(&cfg(), &host).is_ok(), "the start-time check accepts what the install made");
}

#[test]
fn progress_lines_are_kept_while_installing_and_the_status_says_installing() {
    let tmp = Tmp::new();
    let host = Host(tmp.0.clone());
    let addons = Arc::new(Addons::new());
    let seen: Arc<Mutex<Vec<AddonStatus>>> = Arc::default();
    addons.begin("demo").unwrap();
    let (a2, seen2, host2) = (addons.clone(), seen.clone(), Host(tmp.0.clone()));
    let changed = move || seen2.lock().push(a2.status(&cfg(), &host2));
    addons.install_blocking(&cfg(), &host, &Fake::ok(), &changed).unwrap();
    let seen = seen.lock();
    assert!(seen.iter().any(|s| s.state == AddonState::Installing && s.lines.iter().any(|l| l.contains("registry.npmjs.org"))));
    assert!(seen.iter().any(|s| s.state == AddonState::Installing && s.lines[0].starts_with("Downloading")));
    assert_eq!(seen.last().unwrap().state, AddonState::Installed, "the last event is the finished state");
}

#[test]
fn a_second_install_of_the_same_agent_is_refused_while_one_runs() {
    let addons = Addons::new();
    addons.begin("demo").unwrap();
    assert_eq!(addons.begin("demo").unwrap_err(), InstallError::Busy);
    assert!(addons.is_installing("demo"));
    assert!(addons.begin("other").is_ok(), "another agent is independent");
}

#[test]
fn a_file_that_is_not_the_pinned_one_is_refused_and_nothing_is_removed() {
    let tmp = Tmp::new();
    let host = Host(tmp.0.clone());
    let addons = Addons::new();
    let mut fake = Fake::ok();
    fake.content = "console.log('someone changed this');".into();
    let error = install(&addons, &host, &fake).unwrap_err();
    match &error {
        InstallError::PinMismatch { version, found, expected } => {
            assert_eq!(version, "1.2.3");
            assert_eq!(expected, &sha(GOOD));
            assert_eq!(found, &sha(&fake.content));
        }
        other => panic!("{other:?}"),
    }
    assert!(error.to_string().contains("nothing was removed"), "{error}");
    let entry = host.adapters_dir().join("demo/node_modules/@scope/demo-acp/dist/index.js");
    assert!(entry.is_file(), "the files are left where they are");
    let status = addons.status(&cfg(), &host);
    assert_eq!(status.state, AddonState::Failed);
    assert!(status.detail.contains("not the file this app was tested with"), "{}", status.detail);
    assert!(pin::enforce(&cfg(), &host).is_err(), "the start-time check keeps refusing it");
}

#[test]
fn a_wrong_version_is_not_marked_installed() {
    let tmp = Tmp::new();
    let host = Host(tmp.0.clone());
    let addons = Addons::new();
    let mut fake = Fake::ok();
    fake.version = "9.9.9".into();
    let error = install(&addons, &host, &fake).unwrap_err();
    assert!(matches!(error, InstallError::Unusable(_)), "{error:?}");
    assert_eq!(addons.status(&cfg(), &host).state, AddonState::Failed);
}

#[test]
fn without_node_it_says_so_and_changes_nothing() {
    let tmp = Tmp::new();
    let host = Host(tmp.0.clone());
    let addons = Addons::new();
    let mut fake = Fake::ok();
    fake.npm = None;
    let error = install(&addons, &host, &fake).unwrap_err();
    assert_eq!(error, InstallError::NodeMissing);
    assert!(error.to_string().contains("Node.js"), "{error}");
    assert_eq!(fake.runs.load(Ordering::SeqCst), 0);
    assert!(!host.adapters_dir().join("demo").exists(), "no folder is made");
    let status = addons.status(&cfg(), &host);
    assert_eq!(status.state, AddonState::Failed);
    assert!(status.detail.contains("Node.js"));
}

#[test]
fn a_failed_npm_reports_its_last_lines_and_a_retry_works() {
    let tmp = Tmp::new();
    let host = Host(tmp.0.clone());
    let addons = Addons::new();
    let mut fake = Fake::ok();
    fake.code = 1;
    fake.lines = vec!["npm error code ENOTFOUND".into(), "npm error network request failed".into()];
    let error = install(&addons, &host, &fake).unwrap_err();
    assert!(matches!(&error, InstallError::Npm { code: Some(1), tail } if tail.contains("ENOTFOUND") && tail.contains("network")), "{error:?}");
    assert_eq!(addons.status(&cfg(), &host).state, AddonState::Failed);

    // The failure is remembered only until the next try.
    assert_eq!(install(&addons, &host, &Fake::ok()).unwrap(), "1.2.3");
    assert_eq!(addons.status(&cfg(), &host).state, AddonState::Installed);
}

#[test]
fn removing_moves_the_folder_to_removed_and_deletes_nothing() {
    let tmp = Tmp::new();
    let host = Host(tmp.0.clone());
    let addons = Addons::new();
    install(&addons, &host, &Fake::ok()).unwrap();
    let moved = addons.remove(&cfg(), &host).unwrap().expect("something was moved");
    assert!(!host.adapters_dir().join("demo").exists());
    assert_eq!(moved.parent().unwrap(), tmp.0.join("acp").join(".removed"));
    assert!(moved.join("node_modules/@scope/demo-acp/dist/index.js").is_file(), "the files are all still there");
    assert_eq!(addons.status(&cfg(), &host).state, AddonState::NotInstalled);
    // Nothing installed: nothing to move.
    assert_eq!(addons.remove(&cfg(), &host).unwrap(), None);
    // Installing again and removing again keeps both copies.
    install(&addons, &host, &Fake::ok()).unwrap();
    let again = addons.remove(&cfg(), &host).unwrap().unwrap();
    assert_ne!(again, moved);
    assert!(moved.exists() && again.exists());
}

#[test]
fn removing_is_refused_while_installing() {
    let tmp = Tmp::new();
    let host = Host(tmp.0.clone());
    let addons = Addons::new();
    addons.begin("demo").unwrap();
    assert_eq!(addons.remove(&cfg(), &host).unwrap_err(), InstallError::Busy);
}

#[test]
fn an_older_install_is_moved_aside_before_the_new_one() {
    let tmp = Tmp::new();
    let host = Host(tmp.0.clone());
    let addons = Addons::new();
    let mut old = Fake::ok();
    old.version = "1.0.0".into();
    let _ = install(&addons, &host, &old); // leaves a 1.0.0 folder (not usable)
    assert_eq!(addons.status(&cfg(), &host).state, AddonState::Failed);
    assert_eq!(install(&addons, &host, &Fake::ok()).unwrap(), "1.2.3");
    let kept: Vec<_> = std::fs::read_dir(tmp.0.join("acp").join(".removed")).unwrap().collect();
    assert_eq!(kept.len(), 1, "the old copy was moved, not deleted");
}

#[test]
fn a_definition_that_cannot_be_checked_or_is_odd_is_not_installed() {
    let host = Host(PathBuf::from("C:\\data"));
    let mut no_hash = cfg();
    no_hash.pins[0].sha256 = None;
    assert!(matches!(plan(&no_hash, &host), Err(InstallError::NotInstallable(why)) if why.contains("SHA-256")));
    assert!(!installable(&no_hash));

    let mut bad_name = cfg();
    bad_name.pins[0].package = "left pad; calc".into();
    assert!(matches!(plan(&bad_name, &host), Err(InstallError::NotInstallable(_))));

    let mut range = cfg();
    range.pins[0].version = "^1.2.3".into();
    assert!(matches!(plan(&range, &host), Err(InstallError::NotInstallable(_))));

    for entry in ["C:/elsewhere/index.js", "{adapters}/../escape/index.js", "{adapters}/a b/index.js", "{adapters}/index.js/../../x"] {
        let mut odd = cfg();
        odd.pins[0].entry = entry.into();
        let result = plan(&odd, &host);
        assert!(matches!(result, Err(InstallError::NotInstallable(_))), "{entry}: {result:?}");
    }

    let mut off = cfg();
    off.disabled = true;
    assert!(plan(&off, &host).is_err() && !installable(&off));

    let none = AgentConfig { id: "x".into(), ..AgentConfig::default() };
    assert!(plan(&none, &host).is_err() && !installable(&none));
}

#[test]
fn the_newest_pin_is_the_one_installed() {
    let host = Host(PathBuf::from("C:\\data"));
    let mut two = cfg();
    let mut newer = two.pins[0].clone();
    newer.version = "1.3.0".into();
    two.pins.push(newer);
    assert_eq!(plan(&two, &host).unwrap().version, "1.3.0");
}

#[test]
fn the_start_path_can_tell_a_missing_add_on_from_a_broken_one() {
    let tmp = Tmp::new();
    let host = Host(tmp.0.clone());
    assert!(is_missing(&cfg(), &host));
    let addons = Addons::new();
    install(&addons, &host, &Fake::ok()).unwrap();
    assert!(!is_missing(&cfg(), &host));
    let mut off = cfg();
    off.pin_policy = PinPolicy::Off;
    let empty = Tmp::new();
    let empty_host = Host(empty.0.clone());
    assert!(!is_missing(&off, &empty_host), "no pin check, no install prompt");
    assert!(!is_missing(&AgentConfig::default(), &empty_host), "no pins: nothing to install");
}

#[test]
fn the_built_in_agents_can_be_installed_from_their_tables() {
    let host = Host(PathBuf::from("C:\\data"));
    let mut seen = 0;
    for cfg in crate::acp::config::builtin_agents().into_iter().filter(installable) {
        let plan = plan(&cfg, &host).unwrap_or_else(|e| panic!("{}: {e}", cfg.id));
        assert!(plan.dir.starts_with(host.adapters_dir()), "{}", cfg.id);
        assert!(plan.entry.starts_with(&plan.dir), "{}: the entry file is inside the add-on's folder", cfg.id);
        assert!(plan.size_mb.is_some(), "{}: the prompt says about how big", cfg.id);
        assert!(!cfg.addon.signin_hint.is_empty(), "{}", cfg.id);
        assert!(!cfg.addon.signin_hint.contains("Claude"), "branding rule");
        seen += 1;
    }
    assert!(seen >= 2, "claude and codex both have an add-on to install");
}

#[test]
fn npm_is_found_next_to_node() {
    let tmp = Tmp::new();
    std::fs::create_dir_all(&tmp.0).unwrap();
    let node = tmp.0.join(if cfg!(windows) { "node.exe" } else { "node" });
    std::fs::write(&node, "").unwrap();
    assert_eq!(npm_next_to(Some(&node)), None, "no npm beside it");
    let npm = tmp.0.join(if cfg!(windows) { "npm.cmd" } else { "npm" });
    std::fs::write(&npm, "").unwrap();
    assert_eq!(npm_next_to(Some(&node)), Some(npm));
    assert_eq!(npm_next_to(None), None);
}

#[test]
fn progress_lines_are_one_short_clean_line() {
    assert_eq!(clean_line("  npm http fetch\u{1b}  "), "npm http fetch");
    let long = "x".repeat(500);
    assert!(clean_line(&long).chars().count() <= MAX_LINE_CHARS + 1);
}

/// One real install of each built-in adapter into a folder that is NOT the app's data. Run by hand:
/// `PM_ACP_INSTALL_PROOF_DIR=D:\pm-acp-install-proof cargo test --lib acp::install::tests::real_install_proof -- --ignored --nocapture`
/// It starts npm (hidden) and downloads the pinned packages; no agent turn is made.
#[test]
#[ignore = "downloads the real adapters with npm; run by hand"]
fn real_install_proof() {
    let Some(root) = std::env::var_os("PM_ACP_INSTALL_PROOF_DIR").map(PathBuf::from) else { panic!("set PM_ACP_INSTALL_PROOF_DIR") };
    let host = Host(root);
    let addons = Addons::new();
    for cfg in crate::acp::config::builtin_agents().into_iter().filter(installable) {
        addons.begin(&cfg.id).unwrap();
        let started = std::time::Instant::now();
        let result = addons.install_blocking(&cfg, &host, &RealRunner, &|| {});
        let status = addons.status(&cfg, &host);
        println!("{}: {:?} in {:?} -> {:?} {:?} {}", cfg.id, result, started.elapsed(), status.state, status.version, status.detail);
        assert!(result.is_ok(), "{}: {:?}", cfg.id, result);
        assert_eq!(status.state, AddonState::Installed);
        assert!(pin::enforce(&cfg, &host).is_ok(), "{}: the start-time check accepts it", cfg.id);
    }
}
