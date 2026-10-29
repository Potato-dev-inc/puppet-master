//! RubyGems lookup (SPEC-n33 section 3.6). Two GETs: the gem document (`/api/v1/gems/<name>.json`) for the
//! total downloads and the newest version, then the version list (`/api/v1/versions/<name>.json`) for the
//! first version's publish time.
//!
//! Only counts, dates and flags are kept. The gem's `authors` text is never parsed (S12).

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::package_check::model::{
    base_for, Downloads, Facts, FetchError, Fetcher, Got, Outcome, Registry, VersionFacts, MAX_DOC_BYTES,
    MAX_SMALL_BYTES, MAX_VERSIONS_KEPT,
};
use crate::prompt_risk::packages::model::Ecosystem;

/// Longest version string kept as a key. Real versions are far shorter.
const MAX_VERSION_LEN: usize = 64;

#[derive(Deserialize)]
struct Gem {
    downloads: Option<u64>,
    version: Option<String>,
    version_created_at: Option<String>,
    source_code_uri: Option<String>,
    homepage_uri: Option<String>,
}

#[derive(Deserialize)]
struct Release {
    number: Option<String>,
    created_at: Option<String>,
}

/// Look up `name` on RubyGems. Only the package name is in the URLs.
pub fn lookup(f: &dyn Fetcher, name: &str) -> Outcome {
    if !Ecosystem::Gems.valid_name(name) {
        return Outcome::Failed(FetchError::NotAllowed);
    }
    let base = base_for(Registry::Gems);
    // First call: a failure here is the package's failure.
    let gem = match f.get(&format!("{base}/api/v1/gems/{name}.json"), "application/json", MAX_SMALL_BYTES) {
        Ok(Got::Ok(body)) => match serde_json::from_slice::<Gem>(&body) {
            Ok(gem) => gem,
            Err(_) => return Outcome::Failed(FetchError::Unreadable),
        },
        Ok(Got::NotFound) => return Outcome::NotFound,
        Err(error) => return Outcome::Failed(error),
    };
    // Second call: a failure here only drops the first-publish fact (`big` when the answer was too large).
    let (releases, big) = fetch_releases(f, &format!("{base}/api/v1/versions/{name}.json"));
    Outcome::Found(facts(gem, &releases, big))
}

/// The version list, newest first as RubyGems lists it. Empty when the call failed.
/// The flag is true only when the answer was over the cap.
fn fetch_releases(f: &dyn Fetcher, url: &str) -> (Vec<Release>, bool) {
    match f.get(url, "application/json", MAX_DOC_BYTES) {
        Ok(Got::Ok(body)) => match serde_json::from_slice::<Vec<Release>>(&body) {
            Ok(list) => (list, false),
            Err(_) => (Vec::new(), false),
        },
        Err(FetchError::TooBig) => (Vec::new(), true),
        _ => (Vec::new(), false),
    }
}

fn facts(gem: Gem, releases: &[Release], big: bool) -> Facts {
    // The first publish is the oldest release, wherever it sits in the list, so it is taken over all of them.
    let created_ms = releases
        .iter()
        .filter_map(|release| release.created_at.as_deref().and_then(to_ms))
        .min();
    let mut versions = BTreeMap::new();
    for release in releases.iter().take(MAX_VERSIONS_KEPT) {
        if let Some(number) = release.number.as_ref().filter(|number| valid_version(number)) {
            let published_ms = release.created_at.as_deref().and_then(to_ms);
            versions.insert(number.clone(), VersionFacts { published_ms, ..VersionFacts::default() });
        }
    }
    // The gem document names the newest version and its time, so they are kept even when the list failed.
    let newest = gem.version.filter(|version| valid_version(version));
    if let Some(version) = &newest {
        let entry: &mut VersionFacts = versions.entry(version.clone()).or_default();
        entry.published_ms = entry.published_ms.or(gem.version_created_at.as_deref().and_then(to_ms));
    }
    Facts {
        ecosystem: Ecosystem::Gems,
        created_ms,
        newest,
        tags: BTreeMap::new(),
        versions,
        downloads: gem.downloads.map(Downloads::Total),
        maintainers: None,
        source_link: Some(present(&gem.source_code_uri) || present(&gem.homepage_uri)),
        big,
    }
}

fn valid_version(version: &str) -> bool {
    !version.is_empty() && version.len() <= MAX_VERSION_LEN
}

fn present(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(|text| !text.trim().is_empty())
}

/// RFC 3339 time to milliseconds since the epoch. None when it does not parse or is before 1970.
fn to_ms(text: &str) -> Option<u64> {
    let at = chrono::DateTime::parse_from_rfc3339(text).ok()?;
    u64::try_from(at.timestamp_millis()).ok()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;
    use crate::package_check::model::MAX_GETS_PER_PACKAGE;

    const GEM: &str = include_str!("../fixtures/gem.json");
    const VERSIONS: &str = include_str!("../fixtures/gem_versions.json");
    /// 2013-08-19T08:00:00Z, the oldest release in the fixture (last in the list).
    const FIRST_MS: u64 = 1_376_899_200_000;
    /// 2024-05-02T10:00:00Z, the fixture's newest version.
    const NEWEST_MS: u64 = 1_714_644_000_000;

    /// Answers by exact URL and records every call (URL, Accept, byte cap), so tests can count GETs.
    #[derive(Default)]
    struct Fake {
        answers: HashMap<String, Result<Got, FetchError>>,
        calls: Mutex<Vec<(String, String, usize)>>,
    }

    impl Fake {
        fn answer(mut self, url: String, answer: Result<Got, FetchError>) -> Self {
            self.answers.insert(url, answer);
            self
        }

        fn calls(&self) -> Vec<(String, String, usize)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Fetcher for Fake {
        fn get(&self, url: &str, accept: &str, max_bytes: usize) -> Result<Got, FetchError> {
            self.calls.lock().unwrap().push((url.to_string(), accept.to_string(), max_bytes));
            // An unexpected URL is refused, so a test notices a request it did not plan.
            self.answers.get(url).cloned().unwrap_or(Err(FetchError::NotAllowed))
        }
    }

    fn gem_url(name: &str) -> String {
        format!("{}/api/v1/gems/{name}.json", base_for(Registry::Gems))
    }

    fn versions_url(name: &str) -> String {
        format!("{}/api/v1/versions/{name}.json", base_for(Registry::Gems))
    }

    /// A fake that answers both calls with the fixtures.
    fn fixture_fake() -> Fake {
        Fake::default()
            .answer(gem_url("pm-demo-gem"), Ok(Got::Ok(GEM.as_bytes().to_vec())))
            .answer(versions_url("pm-demo-gem"), Ok(Got::Ok(VERSIONS.as_bytes().to_vec())))
    }

    fn found(outcome: Outcome) -> Facts {
        match outcome {
            Outcome::Found(facts) => facts,
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn total_downloads_and_the_first_version_time() {
        let facts = found(lookup(&fixture_fake(), "pm-demo-gem"));
        assert_eq!(facts.ecosystem, Ecosystem::Gems);
        assert_eq!(facts.downloads, Some(Downloads::Total(12_345)));
        // The oldest release is last in the list; the minimum finds it wherever it sits.
        assert_eq!(facts.created_ms, Some(FIRST_MS));
        assert_eq!(facts.newest.as_deref(), Some("2.1.0"));
        assert_eq!(facts.versions["2.1.0"].published_ms, Some(NEWEST_MS));
        assert_eq!(facts.versions["0.1.0"].published_ms, Some(FIRST_MS));
        assert_eq!(facts.maintainers, None);
        assert!(!facts.big);
    }

    #[test]
    fn source_link_is_present_when_either_uri_is_set() {
        let facts = found(lookup(&fixture_fake(), "pm-demo-gem"));
        assert_eq!(facts.source_link, Some(true));

        // Only the homepage is set (the fixture's homepage_uri is null).
        let mut gem: serde_json::Value = serde_json::from_str(GEM).unwrap();
        gem["source_code_uri"] = serde_json::Value::Null;
        gem["homepage_uri"] = serde_json::Value::String("https://example.invalid/pm-demo-gem".into());
        let fake = Fake::default()
            .answer(gem_url("pm-demo-gem"), Ok(Got::Ok(gem.to_string().into_bytes())))
            .answer(versions_url("pm-demo-gem"), Ok(Got::Ok(VERSIONS.as_bytes().to_vec())));
        assert_eq!(found(lookup(&fake, "pm-demo-gem")).source_link, Some(true));

        // Both empty: no source link.
        gem["source_code_uri"] = serde_json::Value::String(String::new());
        gem["homepage_uri"] = serde_json::Value::Null;
        let fake = Fake::default()
            .answer(gem_url("pm-demo-gem"), Ok(Got::Ok(gem.to_string().into_bytes())))
            .answer(versions_url("pm-demo-gem"), Ok(Got::Ok(VERSIONS.as_bytes().to_vec())));
        assert_eq!(found(lookup(&fake, "pm-demo-gem")).source_link, Some(false));
    }

    #[test]
    fn two_gets_with_json_accept_and_their_caps() {
        let fake = fixture_fake();
        assert!(matches!(lookup(&fake, "pm-demo-gem"), Outcome::Found(_)));
        let calls = fake.calls();
        assert_eq!(calls.len(), 2);
        assert!(calls.len() <= MAX_GETS_PER_PACKAGE);
        assert_eq!(calls[0], (gem_url("pm-demo-gem"), "application/json".to_string(), MAX_SMALL_BYTES));
        assert_eq!(calls[1], (versions_url("pm-demo-gem"), "application/json".to_string(), MAX_DOC_BYTES));
    }

    #[test]
    fn a_failed_second_call_only_drops_the_first_publish_time() {
        let fake = Fake::default()
            .answer(gem_url("pm-demo-gem"), Ok(Got::Ok(GEM.as_bytes().to_vec())))
            .answer(versions_url("pm-demo-gem"), Err(FetchError::Timeout));
        let facts = found(lookup(&fake, "pm-demo-gem"));
        assert_eq!(facts.created_ms, None);
        assert!(!facts.big);
        // The newest version still has its time from the gem document.
        assert_eq!(facts.versions["2.1.0"].published_ms, Some(NEWEST_MS));
        assert_eq!(facts.downloads, Some(Downloads::Total(12_345)));

        let fake = Fake::default()
            .answer(gem_url("pm-demo-gem"), Ok(Got::Ok(GEM.as_bytes().to_vec())))
            .answer(versions_url("pm-demo-gem"), Ok(Got::NotFound));
        assert_eq!(found(lookup(&fake, "pm-demo-gem")).created_ms, None);
    }

    #[test]
    fn a_version_list_over_the_cap_sets_big() {
        let fake = Fake::default()
            .answer(gem_url("pm-demo-gem"), Ok(Got::Ok(GEM.as_bytes().to_vec())))
            .answer(versions_url("pm-demo-gem"), Err(FetchError::TooBig));
        let facts = found(lookup(&fake, "pm-demo-gem"));
        assert!(facts.big);
        assert_eq!(facts.created_ms, None);
    }

    #[test]
    fn a_first_call_not_found_or_failed_ends_the_lookup() {
        let fake = Fake::default().answer(gem_url("pm-demo-gem"), Ok(Got::NotFound));
        assert_eq!(lookup(&fake, "pm-demo-gem"), Outcome::NotFound);
        assert_eq!(fake.calls().len(), 1);

        let fake = Fake::default().answer(gem_url("pm-demo-gem"), Err(FetchError::Timeout));
        assert_eq!(lookup(&fake, "pm-demo-gem"), Outcome::Failed(FetchError::Timeout));
        assert_eq!(fake.calls().len(), 1);
    }

    #[test]
    fn a_gem_document_that_does_not_parse_fails_the_package() {
        let fake = Fake::default().answer(gem_url("pm-demo-gem"), Ok(Got::Ok(b"{\"downloads\": \"many\"}".to_vec())));
        assert_eq!(lookup(&fake, "pm-demo-gem"), Outcome::Failed(FetchError::Unreadable));
    }

    #[test]
    fn the_authors_text_is_never_kept() {
        let text = format!("{:?}", found(lookup(&fixture_fake(), "pm-demo-gem")));
        assert!(!text.contains("Made Up Author"));
    }

    #[test]
    fn the_first_publish_counts_even_past_the_version_cap() {
        let mut releases: Vec<serde_json::Value> = (0..MAX_VERSIONS_KEPT + 5)
            .map(|i| serde_json::json!({ "number": format!("0.{i}.0"), "created_at": "2020-01-01T00:00:00.000Z" }))
            .collect();
        releases.push(serde_json::json!({ "number": "0.0.1", "created_at": "2013-08-19T08:00:00.000Z" }));
        let versions = serde_json::Value::Array(releases).to_string();
        let fake = Fake::default()
            .answer(gem_url("pm-demo-gem"), Ok(Got::Ok(GEM.as_bytes().to_vec())))
            .answer(versions_url("pm-demo-gem"), Ok(Got::Ok(versions.into_bytes())));
        let facts = found(lookup(&fake, "pm-demo-gem"));
        assert_eq!(facts.created_ms, Some(FIRST_MS));
        // The capped list (MAX_VERSIONS_KEPT) plus the newest version from the gem document.
        assert_eq!(facts.versions.len(), MAX_VERSIONS_KEPT + 1);
        assert!(!facts.versions.contains_key("0.0.1"));
    }

    #[test]
    fn a_name_that_is_not_a_gem_name_makes_no_request() {
        let fake = Fake::default();
        assert_eq!(lookup(&fake, "../x"), Outcome::Failed(FetchError::NotAllowed));
        assert_eq!(lookup(&fake, "has space"), Outcome::Failed(FetchError::NotAllowed));
        assert!(fake.calls().is_empty());
    }
}
