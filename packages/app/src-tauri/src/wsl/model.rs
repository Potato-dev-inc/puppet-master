//! The data model of Linux Inside: timing and size limits, the error codes, the probe and status
//! types, and the JSON the Settings page and PaneInfo get. Pure data: no process starts here
//! (see `probe.rs`). Facts from the probe never leave this PC; `DistroFacts` has no serde derive.

use crate::wsl::path::Mounts;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const LIST_TIMEOUT_MS: u64 = 5_000;
pub const PROBE_TIMEOUT_MS: u64 = 25_000;
/// The whole stop call.
pub const STOP_TIMEOUT_MS: u64 = 8_000;
/// TERM, then KILL after this long, inside the stop script.
pub const STOP_GRACE_MS: u64 = 2_000;
pub const LIST_FRESH_MS: u64 = 30_000;
pub const FACTS_FRESH_MS: u64 = 10 * 60_000;
/// Extra time a Linux pane gets for its first ready screen (a cold distro takes seconds to boot).
pub const STARTUP_EXTRA_MS: u64 = 20_000;
/// Bytes read from any `wsl.exe` call.
pub const OUTPUT_CAP: usize = 64 * 1024;
pub const IGNORED_DISTRO_PREFIXES: &[&str] = &["docker-desktop", "rancher-desktop"];
/// Shown with a Copy button only. PM never passes these to a runner.
pub const INSTALL_COMMAND: &str = "wsl --install";
pub const ADD_DISTRO_COMMAND: &str = "wsl --install -d Ubuntu";

pub const WSL_NOT_INSTALLED: &str = "WSL_NOT_INSTALLED";
pub const WSL_NO_DISTRO: &str = "WSL_NO_DISTRO";
pub const WSL_DISTRO_MISSING: &str = "WSL_DISTRO_MISSING";
pub const WSL_DISTRO_NOT_V2: &str = "WSL_DISTRO_NOT_V2";
pub const WSL_AGENT_NOT_FOUND: &str = "WSL_AGENT_NOT_FOUND";
pub const WSL_AGENT_NOT_SUPPORTED: &str = "WSL_AGENT_NOT_SUPPORTED";
pub const WSL_PATH_NOT_MAPPABLE: &str = "WSL_PATH_NOT_MAPPABLE";
pub const WSL_COPY_NOT_YET: &str = "WSL_COPY_NOT_YET";
pub const WSL_HEADLESS_NOT_YET: &str = "WSL_HEADLESS_NOT_YET";
pub const WSL_ACCOUNT_NOT_YET: &str = "WSL_ACCOUNT_NOT_YET";
pub const WSL_GUARD_NOT_SUPPORTED: &str = "WSL_GUARD_NOT_SUPPORTED";
pub const WSL_ENV_REFUSED: &str = "WSL_ENV_REFUSED";
pub const WSL_PROBE_TIMEOUT: &str = "WSL_PROBE_TIMEOUT";
pub const WSL_START_FAILED: &str = "WSL_START_FAILED";

/// Every error code above (the tests check the prefix and uniqueness).
pub const ERROR_CODES: &[&str] = &[
    WSL_NOT_INSTALLED,
    WSL_NO_DISTRO,
    WSL_DISTRO_MISSING,
    WSL_DISTRO_NOT_V2,
    WSL_AGENT_NOT_FOUND,
    WSL_AGENT_NOT_SUPPORTED,
    WSL_PATH_NOT_MAPPABLE,
    WSL_COPY_NOT_YET,
    WSL_HEADLESS_NOT_YET,
    WSL_ACCOUNT_NOT_YET,
    WSL_GUARD_NOT_SUPPORTED,
    WSL_ENV_REFUSED,
    WSL_PROBE_TIMEOUT,
    WSL_START_FAILED,
];

/// A distro name: `^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$`. WSL's own rule is looser; anything else is
/// not offered. Deserialising goes through the same check, so a bad name never gets in from JSON.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct DistroName(String);

impl DistroName {
    pub fn parse(s: &str) -> Option<Self> {
        let mut chars = s.chars();
        let first = chars.next()?;
        let ok = first.is_ascii_alphanumeric()
            && s.len() <= 64
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        ok.then(|| Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for DistroName {
    type Error = &'static str;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::parse(&s).ok_or("not a valid distro name")
    }
}

/// What the Settings page shows for the whole WSL install.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum WslStatus {
    NotWindows,
    /// `command` is `INSTALL_COMMAND`.
    NotInstalled { command: String },
    /// `command` is `ADD_DISTRO_COMMAND`.
    NoDistro { command: String },
    Ready { distros: Vec<Distro> },
    /// `line` is the first non-empty output line, redacted, at most 200 characters.
    Broken { line: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Distro {
    pub name: String,
    pub default: bool,
    pub version: u8,
    /// The state word as `wsl.exe` printed it (localised; display only).
    pub state: String,
}

/// What the probe learned about one distro. Cached in memory and never sent off the PC.
#[derive(Debug, Clone, PartialEq)]
pub struct DistroFacts {
    pub distro: String,
    pub checked_at_ms: u64,
    pub uid: u32,
    /// `/home/you`.
    pub home: String,
    /// `\\wsl.localhost\Ubuntu\home\you` (or the `\\wsl$\...` form), from `wslpath -w ~`.
    pub home_windows: String,
    /// `/mnt/`, from `wslpath -u 'C:\'`. Always ends with `/`.
    pub mount_root: String,
    /// The login shell's PATH.
    pub path: String,
    /// `2.43.0`.
    pub git: Option<String>,
    /// True when the WSLInterop binfmt entry exists.
    pub interop: bool,
    /// `nat` or `mirrored`, from `wslinfo --networking-mode`.
    pub networking: Option<String>,
    /// Bare program name -> absolute path (a found path always starts with `/`).
    pub programs: BTreeMap<String, Option<String>>,
}

/// Where a Linux pane runs, resolved at spawn time.
#[derive(Debug, Clone, PartialEq)]
pub struct LinuxPlace {
    pub distro: String,
    pub linux_cwd: String,
    pub program_abs: String,
    pub path: String,
    pub mounts: Mounts,
    pub home: String,
    pub home_windows: String,
}

/// The `place` field of `PaneInfo` (snake_case like the rest of it). It is display and chat facts
/// only: it grants nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanePlace {
    /// `linux`.
    pub kind: String,
    pub distro: String,
    /// The folder as the agent sees it, for example `/mnt/d/work/shop`.
    pub cwd: String,
    /// For the UI's Linux -> Windows path mapping.
    pub mount_root: String,
    /// For example `\\wsl.localhost\Ubuntu`.
    pub unc_root: String,
    #[serde(default)]
    pub home_windows: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl PanePlace {
    pub fn startup_extra_ms(&self) -> u64 {
        STARTUP_EXTRA_MS
    }
}

/// One row of Settings > Agents.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPlaceRow {
    pub agent_id: String,
    pub label: String,
    /// `None` means Windows.
    pub distro: Option<String>,
    /// `None` means that distro was not checked yet.
    pub found: Option<bool>,
    /// A plain sentence when Linux cannot be chosen.
    pub why_not: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn distro_name_accepts_plain_names() {
        for ok in ["Ubuntu", "Ubuntu-24.04", "my_distro.1", "a"] {
            assert!(DistroName::parse(ok).is_some(), "should accept {ok:?}");
        }
        assert!(DistroName::parse(&"b".repeat(64)).is_some(), "64 characters is the limit");
        let name = DistroName::parse("Ubuntu-24.04").unwrap();
        assert_eq!(name.as_str(), "Ubuntu-24.04");
    }

    #[test]
    fn distro_name_rejects_bad_names() {
        for bad in ["", "-x", "a b", "a/b", ".."] {
            assert!(DistroName::parse(bad).is_none(), "should reject {bad:?}");
        }
        assert!(DistroName::parse(&"c".repeat(65)).is_none(), "65 characters is too many");
        // The JSON path uses the same check.
        assert!(serde_json::from_str::<DistroName>("\"a b\"").is_err());
        assert!(serde_json::from_str::<DistroName>("\"Ubuntu\"").is_ok());
    }

    #[test]
    fn status_json_uses_stable_tags() {
        assert_eq!(serde_json::to_value(WslStatus::NotWindows).unwrap(), json!({"state": "not_windows"}));
        assert_eq!(
            serde_json::to_value(WslStatus::NotInstalled { command: INSTALL_COMMAND.into() }).unwrap(),
            json!({"state": "not_installed", "command": "wsl --install"})
        );
        assert_eq!(
            serde_json::to_value(WslStatus::NoDistro { command: ADD_DISTRO_COMMAND.into() }).unwrap(),
            json!({"state": "no_distro", "command": "wsl --install -d Ubuntu"})
        );
        let ready = WslStatus::Ready {
            distros: vec![Distro { name: "Ubuntu".into(), default: true, version: 2, state: "Running".into() }],
        };
        assert_eq!(
            serde_json::to_value(ready).unwrap(),
            json!({"state": "ready", "distros": [{"name": "Ubuntu", "default": true, "version": 2, "state": "Running"}]})
        );
        assert_eq!(
            serde_json::to_value(WslStatus::Broken { line: "boom".into() }).unwrap(),
            json!({"state": "broken", "line": "boom"})
        );
    }

    #[test]
    fn pane_place_json_omits_empty_warnings() {
        let mut place = PanePlace {
            kind: "linux".into(),
            distro: "Ubuntu".into(),
            cwd: "/mnt/d/work/shop".into(),
            mount_root: "/mnt/".into(),
            unc_root: r"\\wsl.localhost\Ubuntu".into(),
            home_windows: r"\\wsl.localhost\Ubuntu\home\you".into(),
            warnings: vec![],
        };
        assert_eq!(
            serde_json::to_value(&place).unwrap(),
            json!({
                "kind": "linux",
                "distro": "Ubuntu",
                "cwd": "/mnt/d/work/shop",
                "mount_root": "/mnt/",
                "unc_root": r"\\wsl.localhost\Ubuntu",
                "home_windows": r"\\wsl.localhost\Ubuntu\home\you"
            })
        );
        place.warnings = vec!["Line endings differ.".into()];
        assert_eq!(serde_json::to_value(&place).unwrap()["warnings"], json!(["Line endings differ."]));
        assert_eq!(place.startup_extra_ms(), STARTUP_EXTRA_MS);
    }

    #[test]
    fn error_codes_start_with_wsl_and_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for code in ERROR_CODES {
            assert!(code.starts_with("WSL_"), "{code} must start with WSL_");
            assert!(seen.insert(*code), "{code} is listed twice");
        }
        assert_eq!(seen.len(), 14);
    }
}
