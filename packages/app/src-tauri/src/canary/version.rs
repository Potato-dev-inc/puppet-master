//! Reading a CLI's version out of its `--version` line, and judging it against a profile's tested
//! range. Pure; nothing here names an agent.

use crate::tui::TestedRange;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;

/// First `major.minor[.patch][-pre]` in the text (`codex-cli 0.159.3`, `2.1.299 (Claude Code)`,
/// `2026.01.05-abc123`). A missing patch counts as 0.
pub fn parse_version(text: &str) -> Option<semver::Version> {
    static RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(\d+)\.(\d+)(?:\.(\d+))?(-[0-9A-Za-z][0-9A-Za-z.\-]*)?").expect("version regex"));
    let caps = RE.captures(text)?;
    let major: u64 = caps.get(1)?.as_str().parse().ok()?;
    let minor: u64 = caps.get(2)?.as_str().parse().ok()?;
    let patch: u64 = caps.get(3).map_or(Some(0), |m| m.as_str().parse().ok())?;
    let mut text = format!("{major}.{minor}.{patch}");
    if let Some(pre) = caps.get(4) {
        text.push_str(pre.as_str());
    }
    semver::Version::parse(&text).ok().or_else(|| semver::Version::parse(&format!("{major}.{minor}.{patch}")).ok())
}

/// How a version stands against a profile's tested range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Standing {
    /// Inside the tested range.
    Tested,
    /// The profile declares a range and the installed version is outside it.
    Untested,
    /// The profile declares no range: nothing to judge against.
    Unpinned,
    /// A range is declared but the installed version could not be read: not judged.
    Unknown,
}

pub fn standing(range: Option<&TestedRange>, version: Option<&str>) -> Standing {
    let Some(range) = range else { return Standing::Unpinned };
    let Some(version) = version.and_then(parse_version) else { return Standing::Unknown };
    if range.contains(&version) {
        Standing::Tested
    } else {
        Standing::Untested
    }
}

/// The version as a short label for the UI (`0.162.1`), from whatever the CLI printed.
pub fn label(text: &str) -> String {
    parse_version(text).map_or_else(|| text.trim().to_string(), |v| v.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(text: &str) -> TestedRange {
        TestedRange::parse(text).unwrap()
    }

    #[test]
    fn reads_the_version_out_of_each_clis_line() {
        assert_eq!(parse_version("codex-cli 0.159.3").unwrap().to_string(), "0.159.3");
        assert_eq!(parse_version("2.1.299 (Claude Code)").unwrap().to_string(), "2.1.299");
        assert_eq!(parse_version("2026.01.05-abc123").unwrap().to_string(), "2026.1.5-abc123");
        assert_eq!(parse_version("tool v1.4").unwrap().to_string(), "1.4.0");
        assert_eq!(parse_version("codex-cli 0.160.0-alpha.2").unwrap().to_string(), "0.160.0-alpha.2");
        assert_eq!(parse_version("no digits here"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn judges_a_version_against_the_range() {
        let r = range(">=0.159, <0.170");
        assert_eq!(standing(Some(&r), Some("codex-cli 0.159.3")), Standing::Tested);
        assert_eq!(standing(Some(&r), Some("codex-cli 0.169.9")), Standing::Tested);
        assert_eq!(standing(Some(&r), Some("codex-cli 0.158.9")), Standing::Untested);
        assert_eq!(standing(Some(&r), Some("codex-cli 0.170.0")), Standing::Untested);
        assert_eq!(standing(Some(&r), Some("codex-cli 1.0.0")), Standing::Untested);
    }

    #[test]
    fn a_prerelease_inside_the_numbers_is_untested() {
        let r = range(">=0.159, <0.170");
        assert_eq!(standing(Some(&r), Some("codex-cli 0.160.0-alpha.2")), Standing::Untested);
    }

    #[test]
    fn no_range_or_no_readable_version_is_never_untested() {
        assert_eq!(standing(None, Some("0.1.0")), Standing::Unpinned);
        let r = range(">=1");
        assert_eq!(standing(Some(&r), None), Standing::Unknown);
        assert_eq!(standing(Some(&r), Some("not a version")), Standing::Unknown);
    }

    #[test]
    fn a_range_that_is_not_a_range_is_rejected() {
        assert!(TestedRange::parse("").is_err());
        assert!(TestedRange::parse("newer than last week").is_err());
        assert_eq!(range(" >=0.150, <0.170 ").as_str(), ">=0.150, <0.170");
    }
}
