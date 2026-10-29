//! Data model for Package check's offline part: what one command installs, and the card's first
//! paint of it. Plain data only: no network, no disk, no screen.
//!
//! Name rules (`Ecosystem::valid_name`) are the only checks on a name's shape. Anything a name is
//! checked against (look-alike lists, registry facts) lives in other modules.

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Items kept per command; the rest are counted in `PackageList::more`.
pub const MAX_PACKAGES: usize = 20;
/// Longest name kept. Used for every ecosystem (the stricter of the per-ecosystem limits wins).
pub const MAX_NAME_LEN: usize = 214;
/// Longest version or range text kept as `spec`.
pub const MAX_SPEC_LEN: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ecosystem {
    Npm,
    Pypi,
    Crates,
    Go,
    Gems,
    Winget,
    Choco,
    Scoop,
}

static NPM_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(@[a-z0-9][a-z0-9._~-]*/)?[a-z0-9][a-z0-9._~-]*$").expect("npm name regex")
});
static PYPI_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Za-z0-9]([A-Za-z0-9._-]*[A-Za-z0-9])?$").expect("pypi name regex"));
static CRATES_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Za-z][A-Za-z0-9_-]{0,63}$").expect("crates name regex"));
static GO_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[a-z0-9.-]+\.[a-z]{2,}(/[A-Za-z0-9._~+-]+)+$").expect("go module regex")
});
static GEMS_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,99}$").expect("gems name regex"));
static WINGET_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$").expect("winget id regex"));
static CHOCO_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,99}$").expect("choco id regex"));
static SCOOP_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^([A-Za-z0-9_.-]+/)?[A-Za-z0-9][A-Za-z0-9._-]{0,99}$").expect("scoop name regex")
});

impl Ecosystem {
    /// The name people read in a sentence: "Not found on npm."
    pub fn registry_label(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Pypi => "PyPI",
            Self::Crates => "crates.io",
            Self::Go => "the Go module mirror",
            Self::Gems => "RubyGems",
            Self::Winget => "winget",
            Self::Choco => "Chocolatey",
            Self::Scoop => "Scoop",
        }
    }

    /// Packages of this ecosystem are looked up on a public registry (v1).
    pub fn looks_up(self) -> bool {
        matches!(self, Self::Npm | Self::Pypi | Self::Crates | Self::Go | Self::Gems)
    }

    /// This ecosystem has an offline top list of names (look-alike check and "well known" fact).
    pub fn has_popular_list(self) -> bool {
        matches!(self, Self::Npm | Self::Pypi | Self::Crates)
    }

    /// The form two spellings are compared in: npm lowercase; PyPI PEP 503 (runs of `-_.` become
    /// `-`, lowercase); crates lowercase with `_` as `-`; gems lowercase; the rest as typed.
    pub fn normalize(self, name: &str) -> String {
        match self {
            Self::Npm | Self::Gems => name.to_lowercase(),
            Self::Pypi => {
                let mut out = String::with_capacity(name.len());
                let mut in_sep = false;
                for c in name.chars() {
                    if c == '-' || c == '_' || c == '.' {
                        if !in_sep {
                            out.push('-');
                        }
                        in_sep = true;
                    } else {
                        out.push(c.to_ascii_lowercase());
                        in_sep = false;
                    }
                }
                out
            }
            Self::Crates => name.to_lowercase().replace('_', "-"),
            Self::Go | Self::Winget | Self::Choco | Self::Scoop => name.to_string(),
        }
    }

    /// Longest name this ecosystem keeps, in bytes.
    fn max_len(self) -> usize {
        match self {
            Self::Npm | Self::Go => MAX_NAME_LEN,
            Self::Pypi | Self::Gems | Self::Choco | Self::Scoop => 100,
            Self::Crates => 64,
            Self::Winget => 128,
        }
    }

    /// True when `name` has the shape of a package name in this ecosystem (SPEC-n33 §3.2).
    /// Paths with a `.` or `..` part are never names (Go module paths, Scoop buckets).
    pub fn valid_name(self, name: &str) -> bool {
        if name.is_empty() || name.len() > self.max_len() {
            return false;
        }
        let shape_ok = match self {
            Self::Npm => NPM_RE.is_match(name),
            Self::Pypi => PYPI_RE.is_match(name),
            Self::Crates => CRATES_RE.is_match(name),
            Self::Go => GO_RE.is_match(name),
            Self::Gems => GEMS_RE.is_match(name),
            Self::Winget => WINGET_RE.is_match(name),
            Self::Choco => CHOCO_RE.is_match(name),
            Self::Scoop => SCOOP_RE.is_match(name),
        };
        if !shape_ok {
            return false;
        }
        match self {
            Self::Go | Self::Scoop => !name.split('/').any(|part| part == "." || part == ".."),
            _ => true,
        }
    }
}

/// Where a package comes from, as the command says it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// The ecosystem's public registry (the normal case).
    Registry,
    /// A URL, git+ address, `github:` shorthand or `user/repo`. `host` only, lowercase, no port.
    Link { host: String },
    /// A folder or a file on this computer (`./x`, `C:\x`, `*.tgz`, `-e .`).
    Local,
    /// The package list is a file (`-r requirements.txt`). `file` is the basename only.
    ListFile { file: String },
    /// The command itself names another package server (`--registry`, `--index-url`, ...).
    PrivateServer,
    /// Not a plain package name: `$PKG`, `$(..)`, or a name that fails `valid_name`.
    NotPlain,
}

/// What the parser found, before filtering. Crate-internal use only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRef {
    pub ecosystem: Ecosystem,
    /// Program base name, lowercase, no `.exe` / `.cmd`: "npm", "pnpm", "uvx", "pip", "cargo", ...
    pub tool: String,
    /// The name as typed (an npm alias is resolved to the real name).
    pub name: String,
    /// Version, range or tag as typed, when it fits `^[A-Za-z0-9.+~^<>=!*_| -]{1,64}$`.
    pub spec: Option<String>,
    pub source: Source,
    /// `-g`, `--global`, `--user`, `pipx`, `uv tool`, `cargo install`, `go install`, ...
    pub global: bool,
    /// `npx`, `pnpx`, `bunx`, `dlx`, `npm exec`, `uvx`, `uv tool run`, `pipx run`.
    pub runs_now: bool,
}

/// How a name differs from the popular name it resembles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum How {
    OneLetter,
    TwoLetters,
    Swapped,
    Separators,
    LookAlikeChars,
}

/// A name that looks like a popular package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lookalike {
    pub popular: String,
    pub how: How,
}

/// One package as the card first shows it (no network).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageItem {
    pub ecosystem: Ecosystem,
    pub tool: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    pub source: Source,
    pub global: bool,
    pub runs_now: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lookalike: Option<Lookalike>,
    /// In the offline top list.
    pub popular: bool,
    /// May be cut off at the edge of the screen. Such a name is not checked.
    pub cut: bool,
    /// `source == Registry && ecosystem.looks_up() && !cut`: a registry lookup will run.
    pub lookup: bool,
    /// First-paint summary, e.g. "Looks like lodash (2 letters swapped) · Checking…".
    pub first: String,
    /// The first paint already has a "Look closer" fact.
    pub look: bool,
}

/// Every package one command installs, plus what was left out.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageList {
    pub items: Vec<PackageItem>,
    /// Packages past `MAX_PACKAGES`.
    pub more: u32,
    /// Names that looked like secrets: counted, never shown or checked.
    pub hidden: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_name_table_per_ecosystem() {
        let long = "a".repeat(215);
        let table: [(Ecosystem, [&str; 3], [&str; 3]); 8] = [
            (
                Ecosystem::Npm,
                ["lodash", "@types/node", "left-pad"],
                ["../x", "has space", long.as_str()],
            ),
            (
                Ecosystem::Pypi,
                ["requests", "Django", "zope.interface"],
                ["../x", "has space", long.as_str()],
            ),
            (
                Ecosystem::Crates,
                ["serde", "tokio-util", "serde_json"],
                ["../x", "has space", long.as_str()],
            ),
            (
                Ecosystem::Go,
                ["github.com/BurntSushi/toml", "golang.org/x/tools/gopls", "example.com/a"],
                ["../x", "has space", long.as_str()],
            ),
            (
                Ecosystem::Gems,
                ["rails", "nokogiri", "rspec-core"],
                ["../x", "has space", long.as_str()],
            ),
            (
                Ecosystem::Winget,
                ["Git.Git", "Microsoft.VisualStudioCode", "7zip.7zip"],
                ["../x", "has space", long.as_str()],
            ),
            (
                Ecosystem::Choco,
                ["nodejs", "git.install", "googlechrome"],
                ["../x", "has space", long.as_str()],
            ),
            (
                Ecosystem::Scoop,
                ["main/git", "git", "7zip"],
                ["../x", "has space", long.as_str()],
            ),
        ];
        for (eco, good, bad) in table {
            for name in good {
                assert!(eco.valid_name(name), "{eco:?} should accept {name}");
            }
            for name in bad {
                assert!(!eco.valid_name(name), "{eco:?} should refuse {:.20}", name);
            }
        }
    }

    #[test]
    fn valid_name_refuses_empty_and_go_dot_parts() {
        assert!(!Ecosystem::Npm.valid_name(""));
        assert!(!Ecosystem::Go.valid_name("github.com/x/../y"));
        assert!(!Ecosystem::Scoop.valid_name("./git"));
    }

    #[test]
    fn normalize_pep503_and_crates() {
        assert_eq!(Ecosystem::Pypi.normalize("Foo.Bar_baz"), "foo-bar-baz");
        assert_eq!(Ecosystem::Pypi.normalize("zope..interface"), "zope-interface");
        assert_eq!(Ecosystem::Crates.normalize("Serde_JSON"), "serde-json");
        assert_eq!(Ecosystem::Npm.normalize("Left-Pad"), "left-pad");
        assert_eq!(Ecosystem::Go.normalize("github.com/BurntSushi/toml"), "github.com/BurntSushi/toml");
    }

    #[test]
    fn source_serde_tags() {
        assert_eq!(serde_json::to_value(Source::Registry).unwrap(), serde_json::json!({"kind": "registry"}));
        assert_eq!(
            serde_json::to_value(Source::Link { host: "github.com".into() }).unwrap(),
            serde_json::json!({"kind": "link", "host": "github.com"})
        );
        assert_eq!(
            serde_json::to_value(Source::ListFile { file: "requirements.txt".into() }).unwrap(),
            serde_json::json!({"kind": "list_file", "file": "requirements.txt"})
        );
        assert_eq!(serde_json::to_value(Source::NotPlain).unwrap(), serde_json::json!({"kind": "not_plain"}));
        let back: Source = serde_json::from_value(serde_json::json!({"kind": "private_server"})).unwrap();
        assert_eq!(back, Source::PrivateServer);
    }

    #[test]
    fn package_item_golden_json_camel_case() {
        let item = PackageItem {
            ecosystem: Ecosystem::Npm,
            tool: "npm".into(),
            name: "lodahs".into(),
            spec: Some("^1".into()),
            source: Source::Registry,
            global: false,
            runs_now: false,
            lookalike: Some(Lookalike { popular: "lodash".into(), how: How::Swapped }),
            popular: false,
            cut: false,
            lookup: true,
            first: "Looks like lodash (2 letters swapped) · Checking…".into(),
            look: true,
        };
        let golden = serde_json::json!({
            "ecosystem": "npm",
            "tool": "npm",
            "name": "lodahs",
            "spec": "^1",
            "source": {"kind": "registry"},
            "global": false,
            "runsNow": false,
            "lookalike": {"popular": "lodash", "how": "swapped"},
            "popular": false,
            "cut": false,
            "lookup": true,
            "first": "Looks like lodash (2 letters swapped) · Checking…",
            "look": true
        });
        assert_eq!(serde_json::to_value(&item).unwrap(), golden);
        let back: PackageItem = serde_json::from_value(golden).unwrap();
        assert_eq!(back, item);

        let bare = PackageItem {
            spec: None,
            lookalike: None,
            lookup: false,
            first: "Not checked: Puppet Master can't look up winget packages yet".into(),
            look: false,
            ecosystem: Ecosystem::Winget,
            tool: "winget".into(),
            name: "Git.Git".into(),
            ..item
        };
        let json = serde_json::to_value(&bare).unwrap();
        assert!(json.get("spec").is_none());
        assert!(json.get("lookalike").is_none());
        assert_eq!(json["ecosystem"], "winget");
    }

    #[test]
    fn package_list_default_is_empty() {
        let list = PackageList::default();
        assert!(list.items.is_empty());
        assert_eq!((list.more, list.hidden), (0, 0));
    }
}
