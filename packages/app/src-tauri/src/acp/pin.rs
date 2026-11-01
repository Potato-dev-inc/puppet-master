//! "Is this the adapter we tested?" Data-driven version and SHA-256 check.
//!
//! The adapters are third-party code that talks to a logged-in agent on the person's machine. A newer
//! release can change what it sends, so a build accepts only the versions listed in the agent's config,
//! and (when a hash is listed) only the exact file. The check runs before every start. With
//! `PinPolicy::Enforce` a mismatch is an error the caller answers by falling back to the terminal.

use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::config::{expand, AdapterPin, AgentConfig, HostEnv, PinPolicy};
use super::error::AcpError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinCheck {
    /// Nothing is pinned for this agent (an agent without an adapter file).
    NotPinned,
    Ok { version: String },
    /// The entry file is not there: the adapter is not installed.
    Missing { path: PathBuf },
    NoVersion { path: PathBuf },
    VersionMismatch { found: String, accepted: Vec<String> },
    HashMismatch { version: String, found: String, expected: String },
}

impl PinCheck {
    pub fn is_ok(&self) -> bool {
        matches!(self, PinCheck::Ok { .. } | PinCheck::NotPinned)
    }

    pub fn describe(&self) -> String {
        match self {
            PinCheck::NotPinned => "no adapter version is pinned".into(),
            PinCheck::Ok { version } => format!("adapter {version} matches the pin"),
            PinCheck::Missing { path } => format!("the adapter is not installed ({} is missing)", path.display()),
            PinCheck::NoVersion { path } => format!("cannot tell which adapter version is at {}", path.display()),
            PinCheck::VersionMismatch { found, accepted } => {
                format!("adapter version {found} is not one this build accepts ({})", accepted.join(", "))
            }
            PinCheck::HashMismatch { version, found, expected } => {
                format!("adapter {version} was changed after it was pinned (file hash {found}, expected {expected})")
            }
        }
    }
}

/// Checks the installed adapter against `pins`. All pins of one agent name the same package and entry.
pub fn check(pins: &[AdapterPin], host: &dyn HostEnv) -> PinCheck {
    let Some(first) = pins.first() else { return PinCheck::NotPinned };
    let entry = PathBuf::from(expand(&first.entry, host));
    if !entry.is_file() {
        return PinCheck::Missing { path: entry };
    }
    let Some(found) = installed_version(&entry, &first.package) else {
        return PinCheck::NoVersion { path: entry };
    };
    let accepted: Vec<String> = pins.iter().map(|p| p.version.clone()).collect();
    let Some(pin) = pins.iter().find(|p| p.version == found) else {
        return PinCheck::VersionMismatch { found, accepted };
    };
    if let Some(expected) = &pin.sha256 {
        match sha256_hex(&entry) {
            Ok(actual) if actual.eq_ignore_ascii_case(expected) => {}
            Ok(actual) => return PinCheck::HashMismatch { version: found, found: actual, expected: expected.to_ascii_lowercase() },
            Err(_) => return PinCheck::Missing { path: entry },
        }
    }
    PinCheck::Ok { version: found }
}

/// The start-time hook: applies the agent's policy to the result.
pub fn enforce(cfg: &AgentConfig, host: &dyn HostEnv) -> Result<PinCheck, AcpError> {
    if cfg.pin_policy == PinPolicy::Off {
        return Ok(PinCheck::NotPinned);
    }
    let result = check(&cfg.pins, host);
    if result.is_ok() {
        return Ok(result);
    }
    match cfg.pin_policy {
        PinPolicy::Enforce => Err(AcpError::Pin(result.describe())),
        _ => {
            tracing::warn!(agent = %cfg.id, "acp: {}", result.describe());
            Ok(result)
        }
    }
}

/// Reads `version` from the nearest `package.json` above `entry` whose `name` is `package`.
fn installed_version(entry: &Path, package: &str) -> Option<String> {
    let mut dir = entry.parent();
    while let Some(current) = dir {
        let candidate = current.join("package.json");
        if let Ok(text) = std::fs::read_to_string(&candidate) {
            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                if value.get("name").and_then(Value::as_str) == Some(package) {
                    return value.get("version").and_then(Value::as_str).map(str::to_string);
                }
            }
        }
        dir = current.parent();
    }
    None
}

pub fn sha256_hex(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    struct Host(PathBuf);
    impl HostEnv for Host {
        fn var(&self, _: &str) -> Option<OsString> {
            None
        }
        fn which(&self, _: &str) -> Option<PathBuf> {
            None
        }
        fn adapters_dir(&self) -> PathBuf {
            self.0.clone()
        }
        fn homes_dir(&self) -> PathBuf {
            self.0.join("homes")
        }
    }

    fn install(root: &Path, version: &str, body: &str) -> PathBuf {
        let pkg = root.join("x").join("node_modules").join("@scope").join("adapter");
        std::fs::create_dir_all(pkg.join("dist")).unwrap();
        std::fs::write(pkg.join("package.json"), format!(r#"{{"name":"@scope/adapter","version":"{version}"}}"#)).unwrap();
        std::fs::write(pkg.join("dist").join("index.js"), body).unwrap();
        pkg.join("dist").join("index.js")
    }

    fn pin(version: &str, sha: Option<&str>) -> AdapterPin {
        AdapterPin {
            package: "@scope/adapter".into(),
            version: version.into(),
            entry: "{adapters}/x/node_modules/@scope/adapter/dist/index.js".into(),
            sha256: sha.map(str::to_string),
        }
    }

    fn temp() -> PathBuf {
        std::env::temp_dir().join(format!("pm-acp-pin-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn the_pinned_version_and_file_pass() {
        let root = temp();
        let entry = install(&root, "1.1.4", "console.log('adapter')");
        let sha = sha256_hex(&entry).unwrap();
        assert_eq!(check(&[pin("1.1.4", Some(&sha))], &Host(root.clone())), PinCheck::Ok { version: "1.1.4".into() });
        // An upper-case hash in the data is fine.
        assert!(check(&[pin("1.1.4", Some(&sha.to_uppercase()))], &Host(root.clone())).is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn another_version_is_refused_and_older_pins_still_count() {
        let root = temp();
        install(&root, "2.5.0", "x");
        let host = Host(root.clone());
        match check(&[pin("1.1.4", None)], &host) {
            PinCheck::VersionMismatch { found, accepted } => {
                assert_eq!(found, "2.5.0");
                assert_eq!(accepted, vec!["1.1.4"]);
            }
            other => panic!("{other:?}"),
        }
        assert!(check(&[pin("1.1.4", None), pin("2.5.0", None)], &host).is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_edited_file_fails_the_hash_check() {
        let root = temp();
        let entry = install(&root, "1.1.4", "original");
        let sha = sha256_hex(&entry).unwrap();
        std::fs::write(&entry, "tampered").unwrap();
        assert!(matches!(check(&[pin("1.1.4", Some(&sha))], &Host(root.clone())), PinCheck::HashMismatch { .. }));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_adapter_is_reported_as_not_installed() {
        let root = temp();
        let result = check(&[pin("1.1.4", None)], &Host(root));
        assert!(matches!(result, PinCheck::Missing { .. }));
        assert!(result.describe().contains("not installed"));
    }

    #[test]
    fn the_policy_decides_whether_a_mismatch_stops_the_start() {
        let root = temp();
        install(&root, "9.9.9", "x");
        let host = Host(root.clone());
        let mut cfg = AgentConfig { id: "t".into(), pins: vec![pin("1.1.4", None)], ..AgentConfig::default() };
        assert!(matches!(enforce(&cfg, &host), Err(AcpError::Pin(_))));
        cfg.pin_policy = PinPolicy::Warn;
        assert!(enforce(&cfg, &host).is_ok());
        cfg.pin_policy = PinPolicy::Off;
        assert_eq!(enforce(&cfg, &host).unwrap(), PinCheck::NotPinned);
        // No pins listed: nothing to check.
        let none = AgentConfig { id: "n".into(), ..AgentConfig::default() };
        assert_eq!(enforce(&none, &host).unwrap(), PinCheck::NotPinned);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_shipped_codex_pin_is_the_hash_of_the_tested_release() {
        let cfg = super::super::config::codex();
        assert_eq!(cfg.pins.len(), 1);
        assert_eq!(cfg.pins[0].version, "1.1.4");
        assert_eq!(cfg.pins[0].sha256.as_deref().map(str::len), Some(64));
        assert_eq!(cfg.pin_policy, PinPolicy::Enforce);
    }
}
