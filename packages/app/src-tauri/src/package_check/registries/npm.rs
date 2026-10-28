//! npm registry lookup (SPEC-n33 section 3.6): the package document, then last week's downloads.
//!
//! Only the package name is in the URL. Facts are read through typed serde structs. Maintainers are
//! counted (`IgnoredAny`), never read. Deprecation text, install scripts and repository links are
//! never kept, only whether they are there.

use std::collections::BTreeMap;

use serde::de::IgnoredAny;
use serde::Deserialize;

use crate::package_check::model::{
    base_for, Downloads, Ecosystem, Facts, FetchError, Fetcher, Got, Outcome, Registry, VersionFacts,
    MAX_DOC_BYTES, MAX_SMALL_BYTES, MAX_VERSIONS_KEPT,
};

/// The full package document.
const ACCEPT_JSON: &str = "application/json";
/// The abbreviated document, for a package too big for the full one.
const ACCEPT_ABBREVIATED: &str = "application/vnd.npm.install-v1+json";

/// The package document URL. A scoped name keeps its `@` and encodes the slash: `@scope%2Fname`.
fn doc_url(name: &str) -> String {
    format!("{}/{}", base_for(Registry::NpmDoc), name.replace('/', "%2F"))
}

/// The last-week downloads URL. A scoped name keeps its slash: `@scope/name`.
fn downloads_url(name: &str) -> String {
    format!("{}/downloads/point/last-week/{}", base_for(Registry::NpmDownloads), name)
}

/// Looks one npm package up. At most two GETs: the document, then the downloads.
pub fn lookup(f: &dyn Fetcher, name: &str) -> Outcome {
    if !Ecosystem::Npm.valid_name(name) {
        return Outcome::Failed(FetchError::NotAllowed);
    }
    let url = doc_url(name);
    let (body, big) = match f.get(&url, ACCEPT_JSON, MAX_DOC_BYTES) {
        Ok(Got::Ok(body)) => (body, false),
        Ok(Got::NotFound) => return Outcome::NotFound,
        // Too big for the full document: the abbreviated one has the versions and tags, no times.
        Err(FetchError::TooBig) => match f.get(&url, ACCEPT_ABBREVIATED, MAX_DOC_BYTES) {
            Ok(Got::Ok(body)) => (body, true),
            Ok(Got::NotFound) => return Outcome::NotFound,
            Err(e) => return Outcome::Failed(e),
        },
        Err(e) => return Outcome::Failed(e),
    };
    let Ok(doc) = serde_json::from_slice::<NpmDoc>(&body) else {
        return Outcome::Failed(FetchError::Unreadable);
    };
    let mut facts = facts_from(doc, big);
    facts.downloads = downloads(f, name);
    Outcome::Found(facts)
}

/// Last week's downloads. Any failure gives None: the other facts still show.
fn downloads(f: &dyn Fetcher, name: &str) -> Option<Downloads> {
    let Ok(Got::Ok(body)) = f.get(&downloads_url(name), ACCEPT_JSON, MAX_SMALL_BYTES) else {
        return None;
    };
    let answer: NpmDownloads = serde_json::from_slice(&body).ok()?;
    Some(Downloads::Weekly(answer.downloads))
}

fn facts_from(doc: NpmDoc, big: bool) -> Facts {
    let times = doc.time;
    let at = |key: &str| times.get(key).and_then(|v| v.as_str()).and_then(parse_ms);
    let mut listed: Vec<(Option<u64>, String, VersionFacts)> = doc
        .versions
        .into_iter()
        .map(|(version, info)| {
            let published_ms = at(version.as_str());
            (published_ms, version, version_facts(info, published_ms))
        })
        .collect();
    // Newest first; a version with no time sorts last. Only the newest `MAX_VERSIONS_KEPT` are kept.
    listed.sort_by(|a, b| b.0.cmp(&a.0));
    listed.truncate(MAX_VERSIONS_KEPT);
    Facts {
        ecosystem: Ecosystem::Npm,
        created_ms: at("created"),
        newest: doc.dist_tags.get("latest").cloned(),
        tags: doc.dist_tags,
        versions: listed.into_iter().map(|(_, version, facts)| (version, facts)).collect(),
        downloads: None,
        maintainers: doc.maintainers.map(|m| m.len() as u32),
        // The abbreviated document has no repository field: then the fact is unknown, not "none".
        source_link: if big { None } else { Some(doc.repository.is_some()) },
        big,
    }
}

fn version_facts(info: NpmVersion, published_ms: Option<u64>) -> VersionFacts {
    let scripts = info
        .scripts
        .is_some_and(|s| s.preinstall.is_some() || s.install.is_some() || s.postinstall.is_some());
    VersionFacts {
        published_ms,
        install_script: Some(scripts || info.has_install_script.unwrap_or(false)),
        prebuilt: None,
        deprecated: info.deprecated.is_some(),
        yanked: false,
    }
}

/// An RFC 3339 time (npm: `2026-10-20T09:00:00.000Z`) as ms since the epoch. None if it does not parse.
fn parse_ms(text: &str) -> Option<u64> {
    let at = chrono::DateTime::parse_from_rfc3339(text).ok()?;
    u64::try_from(at.timestamp_millis()).ok()
}

#[derive(Deserialize)]
struct NpmDoc {
    /// Event and version times. `time.unpublished` is an object, so values are read as JSON and only
    /// text is kept.
    #[serde(default)]
    time: BTreeMap<String, serde_json::Value>,
    #[serde(default, rename = "dist-tags")]
    dist_tags: BTreeMap<String, String>,
    #[serde(default)]
    versions: BTreeMap<String, NpmVersion>,
    /// Only the count is kept. `IgnoredAny` skips every name and email.
    #[serde(default)]
    maintainers: Option<Vec<IgnoredAny>>,
    /// Presence only.
    #[serde(default)]
    repository: Option<IgnoredAny>,
}

#[derive(Deserialize)]
struct NpmVersion {
    #[serde(default)]
    scripts: Option<NpmScripts>,
    #[serde(default, rename = "hasInstallScript")]
    has_install_script: Option<bool>,
    /// Presence only: the deprecation text is never read.
    #[serde(default)]
    deprecated: Option<IgnoredAny>,
}

/// Presence only: the script text is never read.
#[derive(Deserialize)]
struct NpmScripts {
    #[serde(default)]
    preinstall: Option<IgnoredAny>,
    #[serde(default)]
    install: Option<IgnoredAny>,
    #[serde(default)]
    postinstall: Option<IgnoredAny>,
}

#[derive(Deserialize)]
struct NpmDownloads {
    downloads: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const NPM_NEW: &str = include_str!("../fixtures/npm_new.json");
    const NPM_OLD: &str = include_str!("../fixtures/npm_old.json");
    const NPM_ABBREVIATED: &str = include_str!("../fixtures/npm_abbreviated.json");
    const NPM_DOWNLOADS: &str = include_str!("../fixtures/npm_downloads.json");

    /// Fake network: one answer per (url, Accept). Any other request is `Unreachable`. Every request
    /// is logged as (url, Accept, byte cap), so a test can check exactly what would have been sent.
    struct Fake {
        routes: Vec<(String, &'static str, Result<Got, FetchError>)>,
        seen: Mutex<Vec<(String, String, usize)>>,
    }

    impl Fake {
        fn new(routes: Vec<(String, &'static str, Result<Got, FetchError>)>) -> Self {
            Self { routes, seen: Mutex::new(Vec::new()) }
        }

        fn asked(&self) -> Vec<(String, String, usize)> {
            self.seen.lock().unwrap().clone()
        }
    }

    impl Fetcher for Fake {
        fn get(&self, url: &str, accept: &str, max_bytes: usize) -> Result<Got, FetchError> {
            self.seen.lock().unwrap().push((url.to_string(), accept.to_string(), max_bytes));
            self.routes
                .iter()
                .find(|(u, a, _)| u == url && *a == accept)
                .map(|(_, _, answer)| answer.clone())
                .unwrap_or(Err(FetchError::Unreachable))
        }
    }

    fn ok(text: &str) -> Result<Got, FetchError> {
        Ok(Got::Ok(text.as_bytes().to_vec()))
    }

    fn found(outcome: Outcome) -> Facts {
        match outcome {
            Outcome::Found(facts) => facts,
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn plain_and_scoped_document_urls_escape_only_the_slash() {
        assert_eq!(doc_url("zinc-helper-tool"), format!("{}/zinc-helper-tool", base_for(Registry::NpmDoc)));
        assert_eq!(doc_url("@zinc/helper-tool"), format!("{}/@zinc%2Fhelper-tool", base_for(Registry::NpmDoc)));
        assert_eq!(
            downloads_url("@zinc/helper-tool"),
            format!("{}/downloads/point/last-week/@zinc/helper-tool", base_for(Registry::NpmDownloads))
        );
    }

    #[test]
    fn new_package_with_postinstall_is_found() {
        let name = "zinc-helper-tool";
        let fake = Fake::new(vec![
            (doc_url(name), ACCEPT_JSON, ok(NPM_NEW)),
            (downloads_url(name), ACCEPT_JSON, ok(NPM_DOWNLOADS)),
        ]);
        let facts = found(lookup(&fake, name));
        assert_eq!(facts.ecosystem, Ecosystem::Npm);
        assert_eq!(facts.created_ms, Some(1_792_486_800_000)); // 2026-10-20T09:00Z
        assert_eq!(facts.newest.as_deref(), Some("1.0.0"));
        assert_eq!(facts.tags.get("latest").map(String::as_str), Some("1.0.0"));
        assert_eq!(facts.downloads, Some(Downloads::Weekly(42)));
        assert_eq!(facts.maintainers, Some(1));
        assert_eq!(facts.source_link, Some(true));
        assert!(!facts.big);
        let v = &facts.versions["1.0.0"];
        assert_eq!(v.published_ms, Some(1_792_487_100_000)); // 2026-10-20T09:05Z
        assert_eq!(v.install_script, Some(true));
        assert!(!v.deprecated);
        assert!(!v.yanked);
        assert_eq!(fake.asked().len(), 2);
    }

    #[test]
    fn deprecated_latest_version_is_marked() {
        let name = "old-widget-kit";
        let fake = Fake::new(vec![
            (doc_url(name), ACCEPT_JSON, ok(NPM_OLD)),
            (downloads_url(name), ACCEPT_JSON, ok(NPM_DOWNLOADS)),
        ]);
        let facts = found(lookup(&fake, name));
        assert_eq!(facts.created_ms, Some(1_399_024_800_000)); // 2014-05-02T10:00Z
        assert_eq!(facts.newest.as_deref(), Some("2.0.0"));
        assert_eq!(facts.tags.get("legacy").map(String::as_str), Some("1.0.0"));
        assert!(facts.versions["2.0.0"].deprecated);
        assert!(!facts.versions["1.0.0"].deprecated);
        assert_eq!(facts.versions["2.0.0"].install_script, Some(false));
        assert_eq!(facts.maintainers, Some(2));
        // No repository field in the fixture: the fact is "none listed".
        assert_eq!(facts.source_link, Some(false));
    }

    #[test]
    fn scoped_package_uses_escaped_slash_for_document_and_literal_slash_for_downloads() {
        let name = "@zinc/helper-tool";
        let fake = Fake::new(vec![
            (doc_url(name), ACCEPT_JSON, ok(NPM_NEW)),
            (downloads_url(name), ACCEPT_JSON, ok(NPM_DOWNLOADS)),
        ]);
        assert!(matches!(lookup(&fake, name), Outcome::Found(_)));
        let asked = fake.asked();
        assert_eq!(asked[0].0, format!("{}/@zinc%2Fhelper-tool", base_for(Registry::NpmDoc)));
        assert_eq!(asked[0].1, ACCEPT_JSON);
        assert_eq!(asked[0].2, MAX_DOC_BYTES);
        assert_eq!(asked[1].0, format!("{}/downloads/point/last-week/@zinc/helper-tool", base_for(Registry::NpmDownloads)));
        assert_eq!(asked[1].2, MAX_SMALL_BYTES);
    }

    #[test]
    fn too_big_document_is_read_abbreviated_and_marked_big() {
        let name = "big-kit-demo";
        let fake = Fake::new(vec![
            (doc_url(name), ACCEPT_JSON, Err(FetchError::TooBig)),
            (doc_url(name), ACCEPT_ABBREVIATED, ok(NPM_ABBREVIATED)),
            (downloads_url(name), ACCEPT_JSON, ok(NPM_DOWNLOADS)),
        ]);
        let facts = found(lookup(&fake, name));
        assert!(facts.big);
        assert_eq!(facts.created_ms, None);
        assert_eq!(facts.source_link, None);
        assert_eq!(facts.newest.as_deref(), Some("3.1.0"));
        assert_eq!(facts.versions["3.1.0"].install_script, Some(true));
        assert_eq!(facts.versions["3.0.0"].install_script, Some(false));
        assert_eq!(facts.downloads, Some(Downloads::Weekly(42)));
        let asked = fake.asked();
        assert_eq!(asked.len(), 3);
        assert_eq!(asked[1].1, ACCEPT_ABBREVIATED);
        assert_eq!(asked[1].2, MAX_DOC_BYTES);
    }

    #[test]
    fn failed_downloads_keep_the_other_facts() {
        let name = "zinc-helper-tool";
        let fake = Fake::new(vec![
            (doc_url(name), ACCEPT_JSON, ok(NPM_NEW)),
            (downloads_url(name), ACCEPT_JSON, Err(FetchError::Timeout)),
        ]);
        let facts = found(lookup(&fake, name));
        assert_eq!(facts.downloads, None);
        assert_eq!(facts.newest.as_deref(), Some("1.0.0"));
        assert_eq!(facts.versions["1.0.0"].install_script, Some(true));
        assert_eq!(fake.asked().len(), 2);
    }

    #[test]
    fn downloads_with_a_text_number_are_ignored() {
        let name = "zinc-helper-tool";
        let fake = Fake::new(vec![
            (doc_url(name), ACCEPT_JSON, ok(NPM_NEW)),
            (downloads_url(name), ACCEPT_JSON, ok(r#"{"downloads":"many"}"#)),
        ]);
        assert_eq!(found(lookup(&fake, name)).downloads, None);
    }

    #[test]
    fn not_found_document_is_not_found_and_stops_there() {
        let name = "no-such-zinc-package";
        let fake = Fake::new(vec![(doc_url(name), ACCEPT_JSON, Ok(Got::NotFound))]);
        assert_eq!(lookup(&fake, name), Outcome::NotFound);
        assert_eq!(fake.asked().len(), 1);
    }

    #[test]
    fn failed_first_call_fails_the_package_without_downloads() {
        let name = "zinc-helper-tool";
        let fake = Fake::new(vec![(doc_url(name), ACCEPT_JSON, Err(FetchError::Unreachable))]);
        assert_eq!(lookup(&fake, name), Outcome::Failed(FetchError::Unreachable));
        assert_eq!(fake.asked().len(), 1);
    }

    #[test]
    fn names_that_are_not_plain_npm_names_make_no_request() {
        let long = "a".repeat(215);
        for bad in ["../x", "has space", "@scope/", "x?y=1", long.as_str()] {
            let fake = Fake::new(vec![]);
            assert_eq!(lookup(&fake, bad), Outcome::Failed(FetchError::NotAllowed), "name {bad}");
            assert!(fake.asked().is_empty(), "name {bad} sent a request");
        }
    }

    #[test]
    fn unreadable_document_fails_the_package() {
        let name = "zinc-helper-tool";
        let fake = Fake::new(vec![(doc_url(name), ACCEPT_JSON, ok("<html>not json</html>"))]);
        assert_eq!(lookup(&fake, name), Outcome::Failed(FetchError::Unreadable));
    }

    #[test]
    fn version_map_is_capped_and_the_newest_kept() {
        // Built in code, not a fixture: 5,003 made-up versions, one year apart in their times.
        const N: u32 = MAX_VERSIONS_KEPT as u32 + 3;
        let versions: Vec<String> =
            (0..N).map(|i| format!("\"0.0.{i}\":{{\"version\":\"0.0.{i}\"}}")).collect();
        let times: Vec<String> =
            (0..N).map(|i| format!("\"0.0.{i}\":\"{:04}-01-01T00:00:00.000Z\"", 2000 + i)).collect();
        let body = format!(
            "{{\"dist-tags\":{{\"latest\":\"0.0.{}\"}},\"time\":{{{}}},\"versions\":{{{}}}}}",
            N - 1,
            times.join(","),
            versions.join(",")
        );
        let name = "zinc-many-versions";
        let fake = Fake::new(vec![(doc_url(name), ACCEPT_JSON, ok(&body))]);
        let facts = found(lookup(&fake, name));
        assert_eq!(facts.versions.len(), MAX_VERSIONS_KEPT);
        assert!(facts.versions.contains_key(&format!("0.0.{}", N - 1)));
        assert!(!facts.versions.contains_key("0.0.0"));
    }
}
