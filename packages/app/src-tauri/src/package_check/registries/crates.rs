//! crates.io lookup (SPEC-n33 section 3.6). One GET: the crate document, `/api/v1/crates/<name>`.
//!
//! Only numbers, dates and flags are kept. `published_by` (who published a version) is never parsed
//! (S12), and no text from the registry is kept.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::package_check::model::{
    base_for, Downloads, Facts, FetchError, Fetcher, Got, Outcome, Registry, VersionFacts, MAX_DOC_BYTES,
    MAX_VERSIONS_KEPT,
};
use crate::prompt_risk::packages::model::Ecosystem;

/// `recent_downloads` counts the last 90 days, about 13 weeks.
const WEEKS_IN_RECENT: u64 = 13;
/// Longest version string kept as a key. Real versions are far shorter.
const MAX_VERSION_LEN: usize = 64;

#[derive(Deserialize)]
struct Doc {
    #[serde(rename = "crate")]
    krate: Option<Krate>,
    #[serde(default)]
    versions: Vec<Version>,
}

#[derive(Deserialize)]
struct Krate {
    created_at: Option<String>,
    recent_downloads: Option<u64>,
    newest_version: Option<String>,
    max_stable_version: Option<String>,
    repository: Option<String>,
}

#[derive(Deserialize)]
struct Version {
    num: Option<String>,
    created_at: Option<String>,
    #[serde(default)]
    yanked: bool,
}

/// Look up `name` on crates.io. Only the package name is in the URL.
pub fn lookup(f: &dyn Fetcher, name: &str) -> Outcome {
    if !Ecosystem::Crates.valid_name(name) {
        return Outcome::Failed(FetchError::NotAllowed);
    }
    let url = format!("{}/api/v1/crates/{name}", base_for(Registry::Crates));
    match f.get(&url, "application/json", MAX_DOC_BYTES) {
        Ok(Got::Ok(body)) => match facts(&body) {
            Some(facts) => Outcome::Found(facts),
            None => Outcome::Failed(FetchError::Unreadable),
        },
        Ok(Got::NotFound) => Outcome::NotFound,
        Err(error) => Outcome::Failed(error),
    }
}

/// The facts in a crates.io crate document. None when the document does not have that shape.
fn facts(body: &[u8]) -> Option<Facts> {
    let doc: Doc = serde_json::from_slice(body).ok()?;
    let krate = doc.krate?;
    let mut versions = BTreeMap::new();
    // The API lists the newest version first, so the cap keeps the newest ones.
    for version in doc.versions.into_iter().take(MAX_VERSIONS_KEPT) {
        let Some(num) = version.num.filter(|num| !num.is_empty() && num.len() <= MAX_VERSION_LEN) else {
            continue;
        };
        let facts = VersionFacts {
            published_ms: version.created_at.as_deref().and_then(to_ms),
            yanked: version.yanked,
            ..VersionFacts::default()
        };
        versions.insert(num, facts);
    }
    Some(Facts {
        ecosystem: Ecosystem::Crates,
        created_ms: krate.created_at.as_deref().and_then(to_ms),
        newest: krate.newest_version.or(krate.max_stable_version),
        tags: BTreeMap::new(),
        versions,
        downloads: krate.recent_downloads.map(|recent| Downloads::AboutWeekly(recent / WEEKS_IN_RECENT)),
        maintainers: None,
        // A missing or empty `repository` means the registry lists no source link.
        source_link: Some(krate.repository.is_some_and(|url| !url.trim().is_empty())),
        big: false,
    })
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

    const FIXTURE: &str = include_str!("../fixtures/crate.json");
    /// 2019-06-02T08:15:30.123Z, the crate's first publish.
    const FIRST_MS: u64 = 1_559_463_330_123;

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

    fn crate_url(name: &str) -> String {
        format!("{}/api/v1/crates/{name}", base_for(Registry::Crates))
    }

    fn fixture_facts() -> Facts {
        facts(FIXTURE.as_bytes()).expect("fixture parses")
    }

    #[test]
    fn about_weekly_is_recent_downloads_over_13() {
        // The fixture's recent_downloads is 26,000.
        assert_eq!(fixture_facts().downloads, Some(Downloads::AboutWeekly(2_000)));
    }

    #[test]
    fn created_newest_and_source_link_come_from_the_crate_object() {
        let facts = fixture_facts();
        assert_eq!(facts.ecosystem, Ecosystem::Crates);
        assert_eq!(facts.created_ms, Some(FIRST_MS));
        assert_eq!(facts.newest.as_deref(), Some("1.4.0"));
        assert_eq!(facts.source_link, Some(true));
        assert_eq!(facts.maintainers, None);
        assert!(facts.tags.is_empty());
        assert!(!facts.big);
    }

    #[test]
    fn versions_keep_publish_time_and_the_yanked_flag() {
        let facts = fixture_facts();
        assert_eq!(facts.versions.len(), 4);
        assert!(facts.versions["1.3.1"].yanked);
        assert!(!facts.versions["1.4.0"].yanked);
        assert_eq!(facts.versions["1.0.0"].published_ms, Some(FIRST_MS));
        assert_eq!(facts.versions["1.3.0"].install_script, None);
        assert_eq!(facts.versions["1.3.0"].prebuilt, None);
        assert!(!facts.versions["1.3.0"].deprecated);
    }

    #[test]
    fn the_publisher_is_never_kept() {
        let doc: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        // The fixture does carry the publisher, so the test below means something.
        assert_eq!(doc["versions"][0]["published_by"]["login"], "made-up-publisher");
        let text = format!("{:?}", fixture_facts());
        assert!(!text.contains("made-up-publisher"));
        assert!(!text.contains("Made Up Person"));
    }

    #[test]
    fn a_missing_or_empty_repository_means_no_source_link() {
        let mut doc: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        doc["crate"].as_object_mut().unwrap().remove("repository");
        assert_eq!(facts(doc.to_string().as_bytes()).unwrap().source_link, Some(false));

        doc["crate"]["repository"] = serde_json::Value::String("  ".into());
        assert_eq!(facts(doc.to_string().as_bytes()).unwrap().source_link, Some(false));
    }

    #[test]
    fn newest_falls_back_to_max_stable_version() {
        let mut doc: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        doc["crate"].as_object_mut().unwrap().remove("newest_version");
        assert_eq!(facts(doc.to_string().as_bytes()).unwrap().newest.as_deref(), Some("1.4.0"));
    }

    #[test]
    fn versions_are_capped_newest_first() {
        let versions: Vec<serde_json::Value> = (0..MAX_VERSIONS_KEPT + 5)
            .map(|i| serde_json::json!({ "num": format!("0.0.{i}"), "created_at": "2020-01-01T00:00:00Z" }))
            .collect();
        let doc = serde_json::json!({ "crate": { "created_at": "2020-01-01T00:00:00Z" }, "versions": versions });
        let facts = facts(doc.to_string().as_bytes()).unwrap();
        assert_eq!(facts.versions.len(), MAX_VERSIONS_KEPT);
        assert!(facts.versions.contains_key("0.0.0"));
        assert!(!facts.versions.contains_key(&format!("0.0.{}", MAX_VERSIONS_KEPT + 4)));
    }

    #[test]
    fn lookup_makes_one_get_for_the_crate_document() {
        let fake = Fake::default().answer(crate_url("pm-demo-crate"), Ok(Got::Ok(FIXTURE.as_bytes().to_vec())));
        let outcome = lookup(&fake, "pm-demo-crate");
        assert_eq!(outcome, Outcome::Found(fixture_facts()));
        let calls = fake.calls();
        assert_eq!(calls.len(), 1);
        assert!(calls.len() <= MAX_GETS_PER_PACKAGE);
        assert_eq!(calls[0], (crate_url("pm-demo-crate"), "application/json".to_string(), MAX_DOC_BYTES));
    }

    #[test]
    fn not_found_and_transport_errors_map_to_outcomes() {
        let url = crate_url("pm-demo-crate");
        let fake = Fake::default().answer(url.clone(), Ok(Got::NotFound));
        assert_eq!(lookup(&fake, "pm-demo-crate"), Outcome::NotFound);

        for error in [
            FetchError::Timeout,
            FetchError::TooBig,
            FetchError::TooMany,
            FetchError::Refused,
            FetchError::Unreachable,
        ] {
            let fake = Fake::default().answer(url.clone(), Err(error));
            assert_eq!(lookup(&fake, "pm-demo-crate"), Outcome::Failed(error));
        }
    }

    #[test]
    fn a_body_that_is_not_a_crate_document_fails_the_package() {
        let url = crate_url("pm-demo-crate");
        for body in ["not json", "{}", r#"{"crate": {"created_at": 5}}"#] {
            let fake = Fake::default().answer(url.clone(), Ok(Got::Ok(body.as_bytes().to_vec())));
            assert_eq!(lookup(&fake, "pm-demo-crate"), Outcome::Failed(FetchError::Unreadable), "{body}");
        }
    }

    #[test]
    fn a_name_that_is_not_a_crate_name_makes_no_request() {
        let fake = Fake::default();
        assert_eq!(lookup(&fake, "../x"), Outcome::Failed(FetchError::NotAllowed));
        assert_eq!(lookup(&fake, "has space"), Outcome::Failed(FetchError::NotAllowed));
        assert!(fake.calls().is_empty());
    }
}
