//! The official ACP agent registry: "is there a ready-made adapter or ACP-speaking build for this agent?".
//!
//! Source: `https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json`
//! (`{version, agents: [{id, name, version, description, repository, website, authors, license, distribution:
//! {npx: {package, args}, uvx: {...}, binary: {"<os>-<arch>": {archive, cmd, args, sha256}}}}], extensions}`).
//!
//! * the file is data from outside: it is parsed tolerantly (unknown fields are ignored, an entry that cannot be read
//!   is skipped, the others stay) and every text in it is only ever shown, never run or interpreted;
//! * loading order: a cache younger than 24 h, else the live file (10 s, 2 MiB at most), else an older cache, else the
//!   snapshot compiled into the app (`fixtures/registry_snapshot.json`), so it works offline;
//! * an app agent is matched to an entry by data only (the program name against the entries' program and package
//!   names, then normalised id or name equality); no agent is named in this code;
//! * the distribution for this computer: `npx` with an exact `package@version` first, else `binary` for this
//!   platform's key when the registry lists a SHA-256; `uvx` is not supported yet.
//!
//! The HTTP client is behind [`Http`] so the tests never touch the network.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, SystemTime};

use serde::Deserialize;
use serde_json::Value;

pub const REGISTRY_URL: &str = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
/// The registry as of the build (offline fallback and test fixture).
pub const SNAPSHOT: &str = include_str!("fixtures/registry_snapshot.json");
pub const MAX_REGISTRY_BYTES: u64 = 2 * 1024 * 1024;
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
pub const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
pub const MAX_REDIRECTS: usize = 5;
const CACHE_FILE: &str = "acp_registry_cache.json";

// ---- HTTP (behind a trait) ---------------------------------------------------------------------------

pub trait Http: Send + Sync {
    /// The body of `url`, at most `max_bytes` (more is an error).
    fn get(&self, url: &str, max_bytes: u64, timeout: Duration) -> Result<Vec<u8>, String>;
    /// Writes the body of `url` to `dest`, at most `max_bytes`.
    fn download(&self, url: &str, max_bytes: u64, timeout: Duration, dest: &Path) -> Result<(), String>;
}

/// `https` only, no credentials in the address.
pub fn https_ok(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("https://") else { return false };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    !authority.is_empty() && !authority.contains('@') && !url.chars().any(|c| c.is_control() || c == ' ')
}

pub struct RealHttp;

impl RealHttp {
    fn open(&self, url: &str, timeout: Duration) -> Result<Box<dyn Read + Send + Sync>, String> {
        let agent = ureq::AgentBuilder::new().redirects(0).timeout_connect(Duration::from_secs(10)).timeout(timeout).user_agent("puppet-master").build();
        let mut current = url.to_string();
        for _ in 0..=MAX_REDIRECTS {
            if !https_ok(&current) {
                return Err("only https downloads are allowed".into());
            }
            match agent.get(&current).call() {
                Ok(response) if (300..400).contains(&response.status()) => {
                    let location = response.header("location").ok_or("a redirect without a location")?.to_string();
                    current = if https_ok(&location) {
                        location
                    } else if location.starts_with('/') {
                        let rest = &current["https://".len()..];
                        let host = rest.split(['/', '?', '#']).next().unwrap_or("");
                        format!("https://{host}{location}")
                    } else {
                        return Err("a redirect to a place that is not https".into());
                    };
                }
                Ok(response) => return Ok(Box::new(response.into_reader())),
                Err(ureq::Error::Status(code, _)) => return Err(format!("the server answered {code}")),
                Err(_) => return Err("could not reach the server".into()),
            }
        }
        Err("too many redirects".into())
    }
}

impl Http for RealHttp {
    fn get(&self, url: &str, max_bytes: u64, timeout: Duration) -> Result<Vec<u8>, String> {
        let reader = self.open(url, timeout)?;
        let mut body = Vec::new();
        reader.take(max_bytes + 1).read_to_end(&mut body).map_err(|e| format!("download failed: {e}"))?;
        if body.len() as u64 > max_bytes {
            return Err("the download is larger than allowed".into());
        }
        Ok(body)
    }

    fn download(&self, url: &str, max_bytes: u64, timeout: Duration, dest: &Path) -> Result<(), String> {
        let reader = self.open(url, timeout)?;
        let mut file = std::fs::File::create(dest).map_err(|e| format!("cannot write {}: {e}", dest.display()))?;
        let copied = std::io::copy(&mut reader.take(max_bytes + 1), &mut file).map_err(|e| format!("download failed: {e}"))?;
        if copied > max_bytes {
            drop(file);
            let _ = std::fs::remove_file(dest);
            return Err("the download is larger than allowed".into());
        }
        Ok(())
    }
}

// ---- the registry file --------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct NpxDist {
    /// `name@exact-version`.
    pub package: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct BinaryDist {
    pub archive: String,
    /// The program inside the archive.
    pub cmd: String,
    pub args: Vec<String>,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Distribution {
    pub npx: Option<NpxDist>,
    /// Only noted: `uvx` is not supported yet.
    pub uvx: Option<Value>,
    pub binary: BTreeMap<String, BinaryDist>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RegistryAgent {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub repository: String,
    pub website: String,
    pub authors: Vec<String>,
    pub license: String,
    pub distribution: Distribution,
}

#[derive(Debug, Clone, Default)]
pub struct Registry {
    pub version: String,
    pub agents: Vec<RegistryAgent>,
}

/// Parses the file. An entry that cannot be read is skipped. Not JSON or no `agents` list is an error.
pub fn parse(text: &str) -> Result<Registry, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| format!("the registry is not JSON: {e}"))?;
    let list = value.get("agents").and_then(Value::as_array).ok_or("the registry has no agents list")?;
    let agents: Vec<RegistryAgent> = list.iter().filter_map(|v| serde_json::from_value::<RegistryAgent>(v.clone()).ok()).filter(|a| !a.id.trim().is_empty()).collect();
    Ok(Registry { version: value.get("version").and_then(Value::as_str).unwrap_or("").to_string(), agents })
}

/// Where the data came from: the live file, a cache or the snapshot compiled in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Live,
    Cache,
    Snapshot,
}

pub fn snapshot() -> Registry {
    parse(SNAPSHOT).unwrap_or_default()
}

/// The registry with the fallback chain: a fresh cache, the live file (then cached), an old cache, the snapshot.
/// `cache_dir` is where the cache file lives (a temp folder in tests).
pub fn load(http: &dyn Http, cache_dir: &Path, now: SystemTime) -> (Registry, Origin) {
    let cache = cache_dir.join(CACHE_FILE);
    let cached = std::fs::read_to_string(&cache).ok().and_then(|text| parse(&text).ok().map(|r| (r, text)));
    let age = std::fs::metadata(&cache).and_then(|m| m.modified()).ok().map(|t| now.duration_since(t).unwrap_or_default());
    if let (Some((registry, _)), Some(age)) = (&cached, age) {
        if age < CACHE_TTL {
            return (registry.clone(), Origin::Cache);
        }
    }
    let live = http.get(REGISTRY_URL, MAX_REGISTRY_BYTES, FETCH_TIMEOUT).and_then(|bytes| String::from_utf8(bytes).map_err(|_| "not text".to_string())).and_then(|text| parse(&text).map(|r| (r, text)));
    match live {
        Ok((registry, text)) if !registry.agents.is_empty() => {
            let _ = std::fs::create_dir_all(cache_dir);
            let _ = crate::settings_patch::write_atomic(&cache, text.as_bytes());
            (registry, Origin::Live)
        }
        _ => match cached {
            Some((registry, _)) => (registry, Origin::Cache),
            None => (snapshot(), Origin::Snapshot),
        },
    }
}

/// The real registry for the app: the app data folder's cache, the real network.
pub fn load_default() -> (Registry, Origin) {
    load(&RealHttp, &crate::app_paths::app_data_dir().join("acp"), SystemTime::now())
}

// ---- matching an app agent to an entry -----------------------------------------------------------------

/// The file name of a program, lower case, without a folder, `./` or a Windows launcher extension.
pub fn program_stem(program: &str) -> String {
    let file = program.trim().rsplit(['/', '\\']).next().unwrap_or("").to_ascii_lowercase();
    match file.rsplit_once('.') {
        Some((stem, ext)) if matches!(ext, "exe" | "cmd" | "bat") => stem.to_string(),
        _ => file,
    }
}

/// `name@1.2.3` -> `(name, 1.2.3)`; the `@` of a scope is not a separator.
pub fn split_package(spec: &str) -> Option<(&str, &str)> {
    let at = spec.rfind('@').filter(|i| *i > 0)?;
    Some((&spec[..at], &spec[at + 1..]))
}

/// Lower case letters and digits only, a trailing `acp` dropped (`claude-acp` and `claude` are one name).
fn normalized(text: &str) -> String {
    let flat: String = text.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase();
    match flat.strip_suffix("acp") {
        Some(rest) if !rest.is_empty() => rest.to_string(),
        _ => flat,
    }
}

/// Every program or package name an entry is known by.
fn names_of(agent: &RegistryAgent) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(npx) = &agent.distribution.npx {
        let package = split_package(&npx.package).map(|(n, _)| n).unwrap_or(&npx.package);
        names.push(package.to_ascii_lowercase());
        names.push(package.rsplit('/').next().unwrap_or(package).to_ascii_lowercase());
    }
    for binary in agent.distribution.binary.values() {
        names.push(program_stem(&binary.cmd));
    }
    names.retain(|n| !n.is_empty());
    names
}

/// The entry for an app agent: `program` is the program the app starts for it, `id` and `label` the app's own names.
/// (a) the program name equals a program or package name of an entry, else (b) a normalised id or name is equal.
/// Several entries of the first kind: the one whose id or name equals the app's, else the shortest id.
pub fn find_match<'a>(registry: &'a Registry, program: &str, id: &str, label: &str) -> Option<&'a RegistryAgent> {
    let stem = program_stem(program);
    let (id_n, label_n, program_n) = (normalized(id), normalized(label), normalized(&stem));
    let same_name = |a: &RegistryAgent| (!id_n.is_empty() && normalized(&a.id) == id_n) || (!label_n.is_empty() && normalized(&a.name) == label_n);
    let by_program: Vec<&RegistryAgent> = if stem.is_empty() { Vec::new() } else { registry.agents.iter().filter(|a| names_of(a).iter().any(|n| *n == stem)).collect() };
    let pick = |mut list: Vec<&'a RegistryAgent>| -> Option<&'a RegistryAgent> {
        list.sort_by(|a, b| (!same_name(a), a.id.len(), &a.id).cmp(&(!same_name(b), b.id.len(), &b.id)));
        list.into_iter().next()
    };
    if let Some(found) = pick(by_program) {
        return Some(found);
    }
    pick(registry.agents.iter().filter(|a| same_name(a) || (!program_n.is_empty() && normalized(&a.id) == program_n)).collect())
}

/// [`find_match`] and [`choose`] together for this computer.
pub fn resolve<'a>(registry: &'a Registry, program: &str, id: &str, label: &str, platform: &str) -> Option<(&'a RegistryAgent, Choice)> {
    let agent = find_match(registry, program, id, label)?;
    let choice = choose(agent, platform);
    Some((agent, choice))
}

/// Text from the registry made safe to show: control characters dropped, cut to `max` characters. (It is only ever
/// shown as plain text.)
pub fn display_text(text: &str, max: usize) -> String {
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    let clean = clean.trim();
    if clean.chars().count() > max {
        let cut: String = clean.chars().take(max).collect();
        format!("{cut}...")
    } else {
        clean.to_string()
    }
}

// ---- which distribution ----------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    TarGz,
    TarBz2,
    /// The download is the program itself.
    Raw,
}

pub fn archive_kind(url: &str) -> ArchiveKind {
    let path = url.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
    if path.ends_with(".zip") {
        ArchiveKind::Zip
    } else if path.ends_with(".tar.gz") || path.ends_with(".tgz") {
        ArchiveKind::TarGz
    } else if path.ends_with(".tar.bz2") || path.ends_with(".tbz2") || path.ends_with(".tbz") {
        ArchiveKind::TarBz2
    } else {
        ArchiveKind::Raw
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Npx { package: String, version: String, args: Vec<String> },
    Binary { platform: String, archive: String, kind: ArchiveKind, cmd: String, args: Vec<String>, sha256: String },
    /// Nothing this build can install, in a sentence.
    Unsupported(String),
}

/// This computer's key in `distribution.binary`.
pub fn platform_key() -> String {
    platform_key_for(std::env::consts::OS, std::env::consts::ARCH)
}

pub fn platform_key_for(os: &str, arch: &str) -> String {
    let os = if os == "macos" { "darwin" } else { os };
    let arch = if arch == "arm64" { "aarch64" } else { arch };
    format!("{os}-{arch}")
}

/// A program path inside an archive: relative, no `..`.
pub fn relative_safe(path: &str) -> bool {
    let p = path.replace('\\', "/");
    !p.is_empty()
        && !p.starts_with('/')
        && !p.contains('\0')
        && !(p.len() >= 2 && p.as_bytes()[1] == b':')
        && !p.split('/').any(|part| part == "..")
        && p.trim_start_matches("./").chars().any(|c| c != '/')
}

pub fn choose(agent: &RegistryAgent, platform: &str) -> Choice {
    let mut why: Vec<String> = Vec::new();
    if let Some(npx) = &agent.distribution.npx {
        match split_package(&npx.package) {
            Some((name, version)) if super::install::valid_package_name(name) && super::install::valid_version(version) => {
                return Choice::Npx { package: name.to_string(), version: version.to_string(), args: npx.args.clone() };
            }
            _ => why.push("its npm package is not pinned to an exact version".into()),
        }
    }
    if let Some(binary) = agent.distribution.binary.get(platform) {
        let sha = binary.sha256.as_deref().map(str::trim).filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()));
        let kind = archive_kind(&binary.archive);
        if !https_ok(&binary.archive) {
            why.push("its download is not an https address".into());
        } else if !relative_safe(&binary.cmd) {
            why.push("its program path is not safe".into());
        } else if sha.is_none() {
            why.push("the registry lists no checksum for its download, so it could not be checked".into());
        } else if kind == ArchiveKind::TarBz2 {
            why.push("its download is a .tar.bz2 archive, which is not supported yet".into());
        } else if let Some(sha) = sha {
            return Choice::Binary {
                platform: platform.to_string(),
                archive: binary.archive.clone(),
                kind,
                cmd: binary.cmd.clone(),
                args: binary.args.clone(),
                sha256: sha.to_ascii_lowercase(),
            };
        }
    } else if !agent.distribution.binary.is_empty() {
        why.push(format!("it has no download for this computer ({platform})"));
    }
    if agent.distribution.uvx.is_some() {
        why.push("it needs uvx, which is not supported yet".into());
    }
    if why.is_empty() {
        why.push("the registry lists no way to run it".into());
    }
    Choice::Unsupported(why.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    pub(crate) struct FakeHttp {
        pub body: Mutex<Result<Vec<u8>, String>>,
        pub calls: Mutex<Vec<String>>,
    }

    impl FakeHttp {
        pub fn ok(text: &str) -> Self {
            Self { body: Mutex::new(Ok(text.as_bytes().to_vec())), calls: Mutex::new(Vec::new()) }
        }
        pub fn offline() -> Self {
            Self { body: Mutex::new(Err("offline".into())), calls: Mutex::new(Vec::new()) }
        }
    }

    impl Http for FakeHttp {
        fn get(&self, url: &str, _: u64, _: Duration) -> Result<Vec<u8>, String> {
            self.calls.lock().push(url.to_string());
            self.body.lock().clone()
        }
        fn download(&self, url: &str, _: u64, _: Duration, dest: &Path) -> Result<(), String> {
            self.calls.lock().push(url.to_string());
            let body = self.body.lock().clone()?;
            std::fs::write(dest, body).map_err(|e| e.to_string())
        }
    }

    fn temp() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("pm-acp-registry-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn the_snapshot_parses_and_has_every_kind_of_entry() {
        let registry = snapshot();
        assert_eq!(registry.version, "1.0.0");
        assert_eq!(registry.agents.len(), 43);
        assert!(registry.agents.iter().any(|a| a.distribution.npx.is_some()));
        assert!(registry.agents.iter().any(|a| !a.distribution.binary.is_empty()));
        assert!(registry.agents.iter().any(|a| a.distribution.uvx.is_some()));
    }

    #[test]
    fn parsing_ignores_unknown_fields_and_skips_unreadable_entries() {
        let text = r#"{"version":"9","surprise":1,"agents":[
            {"id":"a","name":"A","new_field":{"x":1},"distribution":{"npx":{"package":"a@1.0.0","args":["--acp"],"env":{"K":"v"}},"future":{}}},
            {"id":"broken","authors":"not a list"},
            {"id":"","name":"no id"},
            7
        ],"extensions":[]}"#;
        let registry = parse(text).unwrap();
        assert_eq!(registry.version, "9");
        assert_eq!(registry.agents.len(), 1);
        assert_eq!(registry.agents[0].distribution.npx.as_ref().unwrap().args, vec!["--acp".to_string()]);
        assert!(parse("{ nope").is_err());
        assert!(parse(r#"{"version":"1"}"#).is_err());
    }

    #[test]
    fn matching_uses_program_names_then_normalised_ids_and_never_agent_names() {
        let registry = snapshot();
        // (a) the program name against a binary program name
        assert_eq!(find_match(&registry, "opencode.exe", "x1", "X1").unwrap().id, "opencode");
        // several entries share the program: the shortest id wins without a name hint
        assert!(find_match(&registry, "C:\\bin\\opencode.cmd", "zz", "ZZ").is_some());
        // (a) against an npm package name
        assert_eq!(find_match(&registry, "cline", "x", "X").unwrap().id, "cline");
        assert_eq!(find_match(&registry, "auggie", "x", "X").unwrap().id, "auggie");
        // (b) normalised id: `<name>-acp` is the adapter of `<name>`
        assert_eq!(find_match(&registry, "no-such-program", "claude", "Claude").unwrap().id, "claude-acp");
        assert_eq!(find_match(&registry, "no-such-program", "codex", "Codex").unwrap().id, "codex-acp");
        // nothing
        assert!(find_match(&registry, "no-such-program", "unknown-thing", "Unknown Thing").is_none());
        assert!(find_match(&registry, "", "", "").is_none());
    }

    #[test]
    fn the_distribution_is_chosen_per_platform() {
        let registry = snapshot();
        let by_id = |id: &str| registry.agents.iter().find(|a| a.id == id).unwrap().clone();
        // npx wins when both exist
        match choose(&by_id("kilo"), "windows-x86_64") {
            Choice::Npx { package, version, args } => {
                assert_eq!(package, "@kilocode/cli");
                assert!(!version.is_empty());
                assert_eq!(args, vec!["acp".to_string()]);
            }
            other => panic!("{other:?}"),
        }
        // binary with a checksum for this platform
        match choose(&by_id("goose"), "windows-x86_64") {
            Choice::Binary { kind, sha256, archive, .. } => {
                assert_eq!(kind, ArchiveKind::Zip);
                assert_eq!(sha256.len(), 64);
                assert!(archive.starts_with("https://"));
            }
            other => panic!("{other:?}"),
        }
        // a binary for another platform key
        assert!(matches!(choose(&by_id("amp-acp"), "linux-x86_64"), Choice::Binary { kind: ArchiveKind::TarGz, .. }));
        // bz2 is not supported yet
        match choose(&by_id("goose"), "darwin-aarch64") {
            Choice::Unsupported(why) => assert!(why.contains("bz2")),
            other => panic!("{other:?}"),
        }
        // no download for this platform
        match choose(&by_id("goose"), "freebsd-x86_64") {
            Choice::Unsupported(why) => assert!(why.contains("freebsd-x86_64")),
            other => panic!("{other:?}"),
        }
        // no checksum: refused
        match choose(&by_id("vtcode"), "windows-x86_64") {
            Choice::Unsupported(why) => assert!(why.contains("checksum")),
            other => panic!("{other:?}"),
        }
        // uvx is out of scope
        match choose(&by_id("fast-agent"), "windows-x86_64") {
            Choice::Unsupported(why) => assert!(why.contains("uvx")),
            other => panic!("{other:?}"),
        }
        assert_eq!(platform_key_for("macos", "aarch64"), "darwin-aarch64");
        assert_eq!(platform_key_for("windows", "x86_64"), "windows-x86_64");
    }

    #[test]
    fn a_floating_npm_version_or_an_unsafe_binary_is_refused() {
        let text = r#"{"agents":[
          {"id":"a","distribution":{"npx":{"package":"a","args":[]}}},
          {"id":"b","distribution":{"npx":{"package":"b@latest"}}},
          {"id":"c","distribution":{"binary":{"windows-x86_64":{"archive":"http://x.test/c.zip","cmd":"c.exe","sha256":"0000000000000000000000000000000000000000000000000000000000000000"}}}},
          {"id":"d","distribution":{"binary":{"windows-x86_64":{"archive":"https://x.test/d.zip","cmd":"../d.exe","sha256":"0000000000000000000000000000000000000000000000000000000000000000"}}}}
        ]}"#;
        for agent in parse(text).unwrap().agents {
            assert!(matches!(choose(&agent, "windows-x86_64"), Choice::Unsupported(_)), "{}", agent.id);
        }
    }

    #[test]
    fn https_and_path_rules() {
        assert!(https_ok("https://github.com/x/y/releases/download/v1/a.zip"));
        assert!(!https_ok("http://github.com/a.zip"));
        assert!(!https_ok("https://user:pw@github.com/a.zip"));
        assert!(!https_ok("file:///c:/a.zip"));
        assert!(!https_ok("https://"));
        assert!(relative_safe("bin/tool.exe") && relative_safe("./tool"));
        assert!(!relative_safe("../tool") && !relative_safe("/tool") && !relative_safe("C:\\tool") && !relative_safe("a/../../b") && !relative_safe(""));
        assert_eq!(archive_kind("https://x/a.tar.gz?x=1"), ArchiveKind::TarGz);
        assert_eq!(archive_kind("https://x/a-linux-amd64"), ArchiveKind::Raw);
    }

    #[test]
    fn the_fallback_chain_works_offline_and_caches_a_live_file() {
        let dir = temp();
        let now = SystemTime::now();
        // offline, no cache: the snapshot
        let offline = FakeHttp::offline();
        let (registry, origin) = load(&offline, &dir, now);
        assert_eq!((origin, registry.agents.len()), (Origin::Snapshot, 43));
        // live works: parsed and cached
        let live = FakeHttp::ok(r#"{"version":"2","agents":[{"id":"only","name":"Only"}]}"#);
        let (registry, origin) = load(&live, &dir, now);
        assert_eq!((origin, registry.agents.len()), (Origin::Live, 1));
        assert_eq!(live.calls.lock().as_slice(), [REGISTRY_URL.to_string()]);
        // a fresh cache is used without asking the network
        let (registry, origin) = load(&offline, &dir, now);
        assert_eq!((origin, registry.version.as_str()), (Origin::Cache, "2"));
        assert_eq!(offline.calls.lock().len(), 1, "only the first (cache-less) load asked");
        // a stale cache is asked again; offline: the stale cache is better than the snapshot
        let later = now + CACHE_TTL + Duration::from_secs(60);
        let (registry, origin) = load(&offline, &dir, later);
        assert_eq!((origin, registry.version.as_str()), (Origin::Cache, "2"));
        assert_eq!(offline.calls.lock().len(), 2);
        // a live file that is junk or empty is ignored
        let junk = FakeHttp::ok("<html>captive portal</html>");
        assert_eq!(load(&junk, &dir, later).1, Origin::Cache);
        let _ = std::fs::remove_dir_all(&dir);
        let empty = FakeHttp::ok(r#"{"agents":[]}"#);
        assert_eq!(load(&empty, &dir, later).1, Origin::Snapshot);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
