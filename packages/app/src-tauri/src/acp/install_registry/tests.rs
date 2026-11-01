//! The registry install with a fake npm and a fake download: nothing is started and nothing leaves the machine.

use super::*;
use crate::acp::install_archive::test_archives::*;
use crate::acp::registry::{ArchiveKind, Http};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::ffi::OsString;

struct Tmp(std::path::PathBuf);
impl Tmp {
    fn new() -> Tmp {
        Tmp(std::env::temp_dir().join(format!("pm-acp-instreg-{}", uuid::Uuid::new_v4())))
    }
    fn host(&self) -> Host {
        Host(self.0.clone())
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Host(std::path::PathBuf);
impl HostEnv for Host {
    fn var(&self, _: &str) -> Option<OsString> {
        None
    }
    fn which(&self, _: &str) -> Option<std::path::PathBuf> {
        None
    }
    fn adapters_dir(&self) -> std::path::PathBuf {
        self.0.join("acp").join("adapters")
    }
    fn homes_dir(&self) -> std::path::PathBuf {
        self.0.join("acp").join("homes")
    }
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Writes what npm would: the package folder, with the manifest the test gives.
struct FakeNpm {
    manifest: String,
    script: Option<(&'static str, &'static str)>,
    has_npm: bool,
    calls: Mutex<Vec<Vec<String>>>,
}

impl FakeNpm {
    fn good() -> Self {
        Self {
            manifest: r#"{"name":"@scope/demo-acp","version":"1.2.3","bin":{"demo-acp":"dist/index.js"}}"#.into(),
            script: Some(("dist/index.js", "console.log('adapter');")),
            has_npm: true,
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl Runner for FakeNpm {
    fn find_npm(&self) -> Option<std::path::PathBuf> {
        self.has_npm.then(|| std::path::PathBuf::from("npm"))
    }
    fn run(&self, _: &Path, args: &[String], cwd: &Path, on_line: &mut dyn FnMut(&str)) -> Result<Option<i32>, String> {
        self.calls.lock().push(args.to_vec());
        let dir = cwd.join("node_modules").join("@scope").join("demo-acp");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("package.json"), &self.manifest).unwrap();
        if let Some((path, text)) = self.script {
            let file = dir.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        on_line("added 1 package");
        Ok(Some(0))
    }
}

struct FakeHttp {
    body: Mutex<Result<Vec<u8>, String>>,
    urls: Mutex<Vec<String>>,
}
impl FakeHttp {
    fn serving(bytes: Vec<u8>) -> Self {
        Self { body: Mutex::new(Ok(bytes)), urls: Mutex::new(Vec::new()) }
    }
}
impl Http for FakeHttp {
    fn get(&self, _: &str, _: u64, _: Duration) -> Result<Vec<u8>, String> {
        self.body.lock().clone()
    }
    fn download(&self, url: &str, _: u64, _: Duration, dest: &Path) -> Result<(), String> {
        self.urls.lock().push(url.to_string());
        std::fs::write(dest, self.body.lock().clone()?).map_err(|e| e.to_string())
    }
}

fn npx_agent() -> (RegistryAgent, Choice) {
    let agent = RegistryAgent { id: "demo-acp".into(), name: "Demo".into(), version: "1.2.3".into(), ..RegistryAgent::default() };
    (agent, Choice::Npx { package: "@scope/demo-acp".into(), version: "1.2.3".into(), args: vec!["--acp".into()] })
}

fn binary_agent(bytes: &[u8], kind: ArchiveKind) -> (RegistryAgent, Choice) {
    let agent = RegistryAgent { id: "bin-agent".into(), name: "Bin".into(), version: "4.5.6".into(), ..RegistryAgent::default() };
    let choice = Choice::Binary {
        platform: "windows-x86_64".into(),
        archive: "https://example.test/bin.zip".into(),
        kind,
        cmd: "./bin/tool.exe".into(),
        args: vec!["serve".into()],
        sha256: sha(bytes),
    };
    (agent, choice)
}

fn store(t: &Tmp) -> OverlayStore {
    OverlayStore::at(t.0.join("acp_overlay.json"))
}

#[test]
fn an_npx_install_pins_the_script_and_a_changed_file_is_refused_at_start() {
    let t = Tmp::new();
    let (agent, choice) = npx_agent();
    let (npm, http) = (FakeNpm::good(), FakeHttp::serving(Vec::new()));
    let mut lines = Vec::new();
    let entry = install_for_agent(&store(&t), "myagent", "My agent", &agent, &choice, &t.host(), &npm, &http, &mut |l| lines.push(l.to_string())).unwrap();
    // no install scripts, exact version in package.json, optional deps stay in
    let args = npm.calls.lock()[0].clone();
    assert!(args.contains(&"--ignore-scripts".to_string()) && !args.iter().any(|a| a.starts_with("--omit=optional")));
    let manifest = std::fs::read_to_string(t.0.join("acp/adapters/reg-demo-acp/package.json")).unwrap();
    assert!(manifest.contains("\"@scope/demo-acp\": \"1.2.3\""), "{manifest}");
    assert_eq!(entry.installed_entry, "reg-demo-acp/node_modules/@scope/demo-acp/dist/index.js");
    assert_eq!(entry.entry_sha256, sha(b"console.log('adapter');"));
    assert!(lines.iter().any(|l| l.starts_with("Downloading")) && lines.last().map(String::as_str) == Some("Done."));
    // stored and readable
    let item = store(&t).get_item("myagent").expect("stored");
    assert_eq!(item.source(), "registry");
    let host = t.host();
    let cfg = crate::acp::overlay::resolve_with(&store(&t), "myagent", None, Some("My agent".into())).unwrap();
    assert_eq!(cfg.command, "node");
    assert_eq!(
        crate::acp::config::expand(&cfg.args[0], &host).replace("\\", "/"),
        format!("{}/reg-demo-acp/node_modules/@scope/demo-acp/dist/index.js", host.adapters_dir().to_string_lossy().replace("\\", "/"))
    );
    assert_eq!(cfg.args[1], "--acp");
    assert!(crate::acp::pin::enforce(&cfg, &host).is_ok());
    // someone edits the script afterwards: the next start is refused
    std::fs::write(host.adapters_dir().join(&entry.installed_entry), "console.log('changed');").unwrap();
    assert!(crate::acp::pin::enforce(&cfg, &host).is_err());
}

#[test]
fn an_npx_install_that_is_not_what_was_asked_for_is_refused() {
    let t = Tmp::new();
    let (agent, choice) = npx_agent();
    let http = FakeHttp::serving(Vec::new());
    let mut wrong_version = FakeNpm::good();
    wrong_version.manifest = r#"{"name":"@scope/demo-acp","version":"9.9.9","bin":"dist/index.js"}"#.into();
    assert!(matches!(install_for_agent(&store(&t), "a", "A", &agent, &choice, &t.host(), &wrong_version, &http, &mut |_| {}), Err(InstallError::Unusable(_))));
    let mut escaping = FakeNpm::good();
    escaping.manifest = r#"{"name":"@scope/demo-acp","version":"1.2.3","bin":"../../../evil.js"}"#.into();
    assert!(matches!(install_for_agent(&store(&t), "a", "A", &agent, &choice, &t.host(), &escaping, &http, &mut |_| {}), Err(InstallError::Unusable(_))));
    let mut native = FakeNpm::good();
    native.manifest = r#"{"name":"@scope/demo-acp","version":"1.2.3","bin":"dist/tool.exe"}"#.into();
    native.script = Some(("dist/tool.exe", "MZ binary"));
    assert!(matches!(install_for_agent(&store(&t), "a", "A", &agent, &choice, &t.host(), &native, &http, &mut |_| {}), Err(InstallError::Unusable(_))));
    let mut no_node = FakeNpm::good();
    no_node.has_npm = false;
    assert_eq!(install_for_agent(&store(&t), "a", "A", &agent, &choice, &t.host(), &no_node, &http, &mut |_| {}).unwrap_err(), InstallError::NodeMissing);
    assert!(store(&t).get_item("a").is_none(), "nothing was written");
}

fn zip_with_tool() -> Vec<u8> {
    zip(&[ZipEntry { name: "bin/tool.exe", data: b"MZ program", mode: 0, deflate: true }, ZipEntry { name: "README.txt", data: b"hi", mode: 0, deflate: false }])
}

#[test]
fn a_binary_install_verifies_the_checksum_unpacks_and_pins_the_program() {
    let t = Tmp::new();
    let bytes = zip_with_tool();
    let (agent, choice) = binary_agent(&bytes, ArchiveKind::Zip);
    let http = FakeHttp::serving(bytes);
    let entry = install_for_agent(&store(&t), "mine", "Mine", &agent, &choice, &t.host(), &FakeNpm::good(), &http, &mut |_| {}).unwrap();
    assert_eq!(http.urls.lock().as_slice(), ["https://example.test/bin.zip".to_string()]);
    assert_eq!(entry.installed_entry, "reg-bin-agent/bin/tool.exe");
    assert_eq!(entry.entry_sha256, sha(b"MZ program"));
    assert_eq!((entry.version.as_str(), entry.kind), ("4.5.6", RegistryKind::Binary));
    let host = t.host();
    let cfg = crate::acp::overlay::resolve_with(&store(&t), "mine", None, Some("Mine".into())).unwrap();
    assert!(cfg.command.starts_with("{adapters}/"));
    assert_eq!(cfg.args, vec!["serve".to_string()]);
    assert!(crate::acp::pin::enforce(&cfg, &host).is_ok());
    // scratch folders are gone
    let leftovers: Vec<_> = std::fs::read_dir(host.adapters_dir()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(leftovers, vec!["reg-bin-agent".to_string()]);
    // a second install replaces the first and keeps the old one under .removed
    install_for_agent(&store(&t), "mine", "Mine", &agent, &choice, &host, &FakeNpm::good(), &http, &mut |_| {}).unwrap();
    assert!(crate::acp::install::removed_root(&host).is_dir());
}

#[test]
fn a_checksum_mismatch_is_refused_before_anything_is_unpacked() {
    let t = Tmp::new();
    let bytes = zip_with_tool();
    let (agent, choice) = binary_agent(&bytes, ArchiveKind::Zip);
    let mut tampered = bytes.clone();
    tampered[10] ^= 0xff;
    let http = FakeHttp::serving(tampered);
    let err = install_for_agent(&store(&t), "mine", "Mine", &agent, &choice, &t.host(), &FakeNpm::good(), &http, &mut |_| {}).unwrap_err();
    assert!(matches!(err, InstallError::Checksum { .. }), "{err:?}");
    assert!(err.to_string().contains("not unpacked"));
    let adapters = t.host().adapters_dir();
    assert!(!adapters.join("reg-bin-agent").exists());
    assert!(std::fs::read_dir(&adapters).map(|d| d.count()).unwrap_or(0) == 0, "no scratch left, nothing unpacked");
    assert!(store(&t).get_item("mine").is_none());
    // a download that fails
    *http.body.lock() = Err("offline".into());
    assert!(matches!(install_for_agent(&store(&t), "mine", "Mine", &agent, &choice, &t.host(), &FakeNpm::good(), &http, &mut |_| {}), Err(InstallError::Download(_))));
}

#[test]
fn a_hostile_archive_with_the_right_checksum_is_still_refused() {
    let t = Tmp::new();
    for bytes in [
        zip(&[ZipEntry { name: "../../evil.txt", data: b"x", mode: 0, deflate: false }, ZipEntry { name: "bin/tool.exe", data: b"MZ", mode: 0, deflate: false }]),
        zip(&[ZipEntry { name: "bin/tool.exe", data: b"target", mode: 0o120777, deflate: false }]),
    ] {
        let (agent, choice) = binary_agent(&bytes, ArchiveKind::Zip);
        let http = FakeHttp::serving(bytes);
        let err = install_for_agent(&store(&t), "mine", "Mine", &agent, &choice, &t.host(), &FakeNpm::good(), &http, &mut |_| {}).unwrap_err();
        assert!(matches!(err, InstallError::Archive(_)), "{err:?}");
    }
    // a tar.gz with a traversal entry
    let tar = tar_gz(&[tar_entry("../evil", b'0', b"x")]);
    let (agent, choice) = binary_agent(&tar, ArchiveKind::TarGz);
    let http = FakeHttp::serving(tar);
    assert!(matches!(install_for_agent(&store(&t), "mine", "Mine", &agent, &choice, &t.host(), &FakeNpm::good(), &http, &mut |_| {}), Err(InstallError::Archive(_))));
    assert!(!t.0.join("acp").join("evil").exists() && !t.0.join("evil").exists());
    assert!(!t.host().adapters_dir().join("reg-bin-agent").exists());
    // the program is missing from the archive
    let bytes = zip(&[ZipEntry { name: "other.exe", data: b"MZ", mode: 0, deflate: false }]);
    let (agent, choice) = binary_agent(&bytes, ArchiveKind::Zip);
    let http = FakeHttp::serving(bytes);
    assert!(matches!(install_for_agent(&store(&t), "mine", "Mine", &agent, &choice, &t.host(), &FakeNpm::good(), &http, &mut |_| {}), Err(InstallError::Unusable(_))));
}

#[test]
fn folder_and_marker_names_are_plain() {
    assert_eq!(folder_for("a b/../c").unwrap(), "reg-a_b____c");
    assert!(folder_for("  ").is_err());
    assert_eq!(marker_name("Foo_Bar"), "pm-registry-foo-bar");
}

/// Real run, never in the normal suite: `cargo test --lib real_npm -- --ignored --nocapture` (needs node and network).
#[test]
#[ignore]
fn real_npm_install_of_a_small_package() {
    let t = Tmp::new();
    let package = std::env::var("PM_REAL_NPM_PACKAGE").unwrap_or_else(|_| "is-odd@3.0.1".into());
    let (name, version) = crate::acp::registry::split_package(&package).unwrap();
    let choice = Choice::Npx { package: name.into(), version: version.into(), args: vec![] };
    let result = install(&choice, "real-test", version, &t.host(), &crate::acp::install::RealRunner, &crate::acp::registry::RealHttp, &mut |l| eprintln!("{l}"));
    eprintln!("{result:?}");
}
