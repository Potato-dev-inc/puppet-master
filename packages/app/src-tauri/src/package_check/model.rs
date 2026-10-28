//! Shapes and numbers for the registry lookups (SPEC-n33 section 3.5): thresholds, registry URLs, the
//! fetch trait, the facts read from a registry, and what the desktop card receives.
//!
//! Nothing in this file sends a request. `http.rs` is the only file that talks to the network.

#[cfg(any(test, feature = "dev-fake-tui"))]
use std::sync::Mutex;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
pub use crate::prompt_risk::packages::model::{Ecosystem, PackageItem, PackageList, Source};

// Thresholds and limits. Every number the card uses lives here.
/// First published less than this many days ago: "Look closer".
pub const NEW_DAYS: u64 = 30;
/// Newest (or pinned) version younger than this many hours ...
pub const FRESH_VERSION_HOURS: u64 = 24;
/// ... on a package older than this many days: "Look closer" (take-over pattern).
pub const FRESH_MIN_AGE_DAYS: u64 = 90;
/// npm and crates.io weekly downloads below this: "Few downloads".
pub const FEW_WEEKLY: u64 = 1_000;
/// RubyGems all-time downloads below this: "Few downloads".
pub const FEW_TOTAL: u64 = 10_000;
/// Packages looked up per card; the rest show "too many packages".
pub const MAX_LOOKUPS: usize = 8;
/// Lookups running at once.
pub const LOOKUP_THREADS: usize = 4;
pub const CONNECT_MS: u64 = 2_000;
/// One GET, whole call.
pub const GET_MS: u64 = 4_000;
/// Whole card. Rows still unfinished at this point show "did not answer in time".
pub const DEADLINE_MS: u64 = 8_000;
/// GETs for one package, at most.
pub const MAX_GETS_PER_PACKAGE: usize = 3;
/// Largest registry document read in full (npm doc, PyPI simple JSON, crates.io crate, RubyGems versions).
pub const MAX_DOC_BYTES: usize = 3 * 1024 * 1024;
/// Largest small answer read (npm downloads, RubyGems gem, Go list/info).
pub const MAX_SMALL_BYTES: usize = 256 * 1024;
pub const TTL_FOUND_MS: u64 = 6 * 3_600_000;
pub const TTL_NOT_FOUND_MS: u64 = 120_000;
pub const TTL_FAILED_MS: u64 = 30_000;
pub const CACHE_CAP: usize = 500;
/// GETs per minute, whole app.
pub const RATE_PER_MIN: u32 = 60;
pub const MAX_VERSIONS_KEPT: usize = 5_000;
pub const USER_AGENT_PREFIX: &str = "PuppetMaster-PackageCheck/";

/// The public registries the lookups may call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Registry {
    NpmDoc,
    NpmDownloads,
    Pypi,
    Crates,
    Gems,
    Go,
}

/// The only hosts a lookup may call (https).
pub const PUBLIC_BASE: &[(Registry, &str)] = &[
    (Registry::NpmDoc, "https://registry.npmjs.org"),
    (Registry::NpmDownloads, "https://api.npmjs.org"),
    (Registry::Pypi, "https://pypi.org"),
    (Registry::Crates, "https://crates.io"),
    (Registry::Gems, "https://rubygems.org"),
    (Registry::Go, "https://proxy.golang.org"),
];

#[cfg(any(test, feature = "dev-fake-tui"))]
static TEST_BASES: Mutex<Vec<(Registry, String)>> = Mutex::new(Vec::new());

/// The base URL a lookup calls: a constant from `PUBLIC_BASE`, or a test base.
pub fn base_for(r: Registry) -> String {
    if let Some(base) = test_base(r) {
        return base;
    }
    PUBLIC_BASE
        .iter()
        .find(|(x, _)| *x == r)
        .map(|(_, base)| (*base).to_string())
        .unwrap_or_default()
}

#[cfg(any(test, feature = "dev-fake-tui"))]
fn test_base(r: Registry) -> Option<String> {
    let bases = TEST_BASES.lock().ok()?;
    bases.iter().find(|(x, _)| *x == r).map(|(_, base)| base.clone())
}

#[cfg(not(any(test, feature = "dev-fake-tui")))]
fn test_base(_r: Registry) -> Option<String> {
    None
}

/// Point a registry at a local fake server. Only `http://127.0.0.1:<port>` is accepted; anything
/// else panics before any state changes.
#[cfg(any(test, feature = "dev-fake-tui"))]
pub fn set_base_for_tests(r: Registry, base: &str) {
    assert!(
        base.starts_with("http://127.0.0.1:"),
        "a test registry base must be http://127.0.0.1:<port>"
    );
    if let Ok(mut bases) = TEST_BASES.lock() {
        bases.retain(|(x, _)| *x != r);
        bases.push((r, base.to_string()));
    }
}

/// The package's page on its registry, for the "Open on npm" link. None where there is no page.
pub fn page_url(eco: Ecosystem, name: &str) -> Option<String> {
    Some(match eco {
        Ecosystem::Npm => format!("https://www.npmjs.com/package/{name}"),
        Ecosystem::Pypi => format!("https://pypi.org/project/{}/", eco.normalize(name)),
        Ecosystem::Crates => format!("https://crates.io/crates/{name}"),
        Ecosystem::Gems => format!("https://rubygems.org/gems/{name}"),
        Ecosystem::Go => format!("https://pkg.go.dev/{name}"),
        Ecosystem::Winget | Ecosystem::Choco | Ecosystem::Scoop => return None,
    })
}

/// What a fetch answered with, for a status code the lookup understands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Got {
    /// 200 with the body (at most the byte cap).
    Ok(Vec<u8>),
    /// 404 or 410.
    NotFound,
}

/// Why a fetch gave no usable answer. The text is never shown; the card maps it to a sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchError {
    Unreachable,
    Timeout,
    TooMany,
    Refused,
    Unreadable,
    TooBig,
    NotAllowed,
}

/// The network seam. `http::UreqFetcher` is the real one; tests pass a fake.
pub trait Fetcher: Send + Sync {
    /// GET `url` (https and on the allow-list, or a test base). `accept` is the Accept header.
    fn get(&self, url: &str, accept: &str, max_bytes: usize) -> Result<Got, FetchError>;
}

/// The download count a registry gives, and how exact it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Downloads {
    Weekly(u64),
    AboutWeekly(u64),
    Total(u64),
}

/// Facts about one version of a package.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VersionFacts {
    pub published_ms: Option<u64>,
    /// npm: `scripts.preinstall|install|postinstall`, or `hasInstallScript`.
    pub install_script: Option<bool>,
    /// PyPI: a `.whl` exists for this version.
    pub prebuilt: Option<bool>,
    /// npm `deprecated` present (the text itself is never kept).
    pub deprecated: bool,
    /// PyPI: all files yanked; crates.io: yanked.
    pub yanked: bool,
}

/// What one registry said about one package. Only numbers, dates and flags; no names of people,
/// no text from the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub ecosystem: Ecosystem,
    pub created_ms: Option<u64>,
    /// npm `dist-tags.latest`; other registries: the newest by publish time.
    pub newest: Option<String>,
    /// npm dist-tags (tag -> version).
    pub tags: BTreeMap<String, String>,
    /// Capped at `MAX_VERSIONS_KEPT` (newest kept).
    pub versions: BTreeMap<String, VersionFacts>,
    pub downloads: Option<Downloads>,
    /// npm only: the COUNT of maintainers. Names are never read into memory.
    pub maintainers: Option<u32>,
    /// `Some(false)` = the registry lists no repository or source link.
    pub source_link: Option<bool>,
    /// The answer was over the cap, so some facts are missing.
    pub big: bool,
}

/// The result of one lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Found(Facts),
    NotFound,
    Failed(FetchError),
}

/// A cached outcome and when it was fetched (ms since the epoch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheHit {
    pub outcome: Outcome,
    pub fetched_ms: u64,
}

// What the desktop gets (camelCase JSON, except the enum values).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RowState {
    Checked,
    NotFound,
    Failed,
    NotChecked,
}

/// One sentence in a row's details. `look` marks a Look closer fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FactLine {
    pub look: bool,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageRow {
    pub name: String,
    pub ecosystem: Ecosystem,
    pub registry_label: String,
    pub state: RowState,
    pub look: bool,
    /// Up to three segments joined by " · ".
    pub summary: String,
    pub lines: Vec<FactLine>,
    pub page_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageCheckView {
    pub prompt_id: String,
    pub off: bool,
    pub status_text: String,
    /// Number of rows with a Look closer fact.
    pub look: u32,
    pub rows: Vec<PackageRow>,
    pub more: u32,
    pub hidden: u32,
    pub footer: String,
    /// Any row failed or was not found: the card shows "Check again".
    pub can_retry: bool,
}

/// The one setting: "Check packages before installs" (on by default).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "yes")]
    pub lookups: bool,
}

fn yes() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self { lookups: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_for_returns_the_https_constants() {
        for (registry, base) in PUBLIC_BASE {
            assert!(base.starts_with("https://"), "{base} must be https");
            assert_eq!(base_for(*registry), *base);
        }
        assert_eq!(base_for(Registry::NpmDoc), "https://registry.npmjs.org");
        assert_eq!(base_for(Registry::Go), "https://proxy.golang.org");
    }

    #[test]
    #[should_panic(expected = "http://127.0.0.1")]
    fn set_base_for_tests_refuses_anything_but_loopback() {
        set_base_for_tests(Registry::NpmDoc, "https://x");
    }

    #[test]
    fn page_url_per_ecosystem() {
        assert_eq!(page_url(Ecosystem::Npm, "lodash").as_deref(), Some("https://www.npmjs.com/package/lodash"));
        assert_eq!(page_url(Ecosystem::Pypi, "Foo_Bar").as_deref(), Some("https://pypi.org/project/foo-bar/"));
        assert_eq!(page_url(Ecosystem::Choco, "nodejs"), None);
    }

    #[test]
    fn settings_default_is_on() {
        assert!(Settings::default().lookups);
        let from_empty: Settings = serde_json::from_str("{}").unwrap();
        assert!(from_empty.lookups);
    }
}
