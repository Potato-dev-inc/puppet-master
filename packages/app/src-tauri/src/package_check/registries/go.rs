//! Go module mirror lookup (SPEC-n33 section 3.6). The module path is case-escaped the way the proxy wants it
//! (each upper-case letter becomes `!` and the lower-case letter), then:
//! 1. `<path>/@v/list`: the versions the mirror lists. A 404 or 410 here is "not found".
//! 2. `<path>/@v/<oldest>.info` and 3. `<path>/@v/<newest>.info` (semver order, pre-releases first) for the times.
//! With no tagged version, one `<path>/@latest` instead. At most three GETs.
//!
//! Only the module path, and the versions the mirror itself listed, are in the URLs. No downloads or
//! maintainers exist for the Go proxy, so none are kept.

use std::collections::BTreeMap;

use semver::Version;
use serde::Deserialize;

use crate::package_check::model::{
    base_for, Facts, FetchError, Fetcher, Got, Outcome, Registry, VersionFacts, MAX_SMALL_BYTES,
};
use crate::prompt_risk::packages::model::Ecosystem;

/// Longest list line kept as a version. Real versions are far shorter.
const MAX_VERSION_LEN: usize = 64;

/// A `.info` answer or the `@latest` answer. Only the version and its time are read.
#[derive(Deserialize)]
struct Info {
    #[serde(rename = "Version")]
    version: Option<String>,
    #[serde(rename = "Time")]
    time: Option<String>,
}

/// Look up the module `name` on the Go module mirror. Only the module path and listed versions are sent.
pub fn lookup(f: &dyn Fetcher, name: &str) -> Outcome {
    if !Ecosystem::Go.valid_name(name) {
        return Outcome::Failed(FetchError::NotAllowed);
    }
    let root = format!("{}/{}", base_for(Registry::Go), escape(name));
    let list = match f.get(&format!("{root}/@v/list"), "text/plain", MAX_SMALL_BYTES) {
        Ok(Got::Ok(body)) => body,
        Ok(Got::NotFound) => return Outcome::NotFound,
        Err(error) => return Outcome::Failed(error),
    };
    let Ok(text) = std::str::from_utf8(&list) else {
        return Outcome::Failed(FetchError::Unreadable);
    };
    let versions = versions_in(text);
    let (Some(oldest), Some(newest)) = (versions.first(), versions.last()) else {
        return latest(f, &root);
    };

    // A failed `.info` call drops that time only; the package is still found.
    let oldest_ms = info_time(f, &root, &oldest.1);
    let newest_ms = if newest.1 == oldest.1 { oldest_ms } else { info_time(f, &root, &newest.1) };
    let mut listed = BTreeMap::new();
    listed.insert(oldest.1.clone(), VersionFacts { published_ms: oldest_ms, ..VersionFacts::default() });
    listed.entry(newest.1.clone()).or_default().published_ms = newest_ms;
    Outcome::Found(Facts {
        ecosystem: Ecosystem::Go,
        created_ms: oldest_ms,
        newest: Some(newest.1.clone()),
        tags: BTreeMap::new(),
        versions: listed,
        downloads: None,
        maintainers: None,
        source_link: None,
        big: false,
    })
}

/// The Go proxy escapes upper-case letters: `BurntSushi` becomes `!burnt!sushi`.
pub fn escape(module: &str) -> String {
    let mut out = String::with_capacity(module.len());
    for c in module.chars() {
        if c.is_ascii_uppercase() {
            out.push('!');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// The mirror's `v<semver>` lines, oldest first. Other lines are skipped. Build metadata
/// (`+incompatible`) does not change the order, and a pre-release sorts below its release.
fn versions_in(list: &str) -> Vec<(Version, String)> {
    let mut versions: Vec<(Version, String)> = list
        .lines()
        .map(str::trim)
        .filter(|line| line.len() <= MAX_VERSION_LEN)
        .filter_map(|line| {
            let parsed = Version::parse(line.strip_prefix('v')?).ok()?;
            Some((parsed, line.to_string()))
        })
        .collect();
    versions.sort_by(|a, b| a.0.cmp(&b.0));
    versions
}

/// The one GET for a module with no tagged version: the mirror's latest pseudo-version and its time.
fn latest(f: &dyn Fetcher, root: &str) -> Outcome {
    let body = match f.get(&format!("{root}/@latest"), "application/json", MAX_SMALL_BYTES) {
        Ok(Got::Ok(body)) => body,
        Ok(Got::NotFound) => return Outcome::NotFound,
        // This is the only answer, so its failure is the package's failure.
        Err(error) => return Outcome::Failed(error),
    };
    let Some(info) = serde_json::from_slice::<Info>(&body).ok() else {
        return Outcome::Failed(FetchError::Unreadable);
    };
    let Some(version) = info.version.filter(|version| !version.is_empty() && version.len() <= MAX_VERSION_LEN) else {
        return Outcome::Failed(FetchError::Unreadable);
    };
    // Nothing older is known without a tagged version, so no creation time is claimed.
    let published_ms = info.time.as_deref().and_then(to_ms);
    let mut listed = BTreeMap::new();
    listed.insert(version.clone(), VersionFacts { published_ms, ..VersionFacts::default() });
    Outcome::Found(Facts {
        ecosystem: Ecosystem::Go,
        created_ms: None,
        newest: Some(version),
        tags: BTreeMap::new(),
        versions: listed,
        downloads: None,
        maintainers: None,
        source_link: None,
        big: false,
    })
}

/// The publish time of one listed version. None when the call failed or the answer has no usable time.
fn info_time(f: &dyn Fetcher, root: &str, version: &str) -> Option<u64> {
    let body = match f.get(&format!("{root}/@v/{version}.info"), "application/json", MAX_SMALL_BYTES) {
        Ok(Got::Ok(body)) => body,
        _ => return None,
    };
    let info: Info = serde_json::from_slice(&body).ok()?;
    to_ms(info.time.as_deref()?)
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

    const LIST: &str = include_str!("../fixtures/go_list.txt");
    const INFO: &str = include_str!("../fixtures/go_info.json");
    /// The module in the lookup tests: the escaped path is `github.com/!example!co/!pm!demo!tool`.
    const MODULE: &str = "github.com/ExampleCo/PmDemoTool";
    /// 2014-02-10T13:38:21Z, the oldest version (v0.1.0) in the tests' own answer.
    const OLDEST_TIME: &str = r#"{"Version":"v0.1.0","Time":"2014-02-10T13:38:21Z"}"#;
    const OLDEST_MS: u64 = 1_392_039_501_000;
    /// 2023-03-14T09:26:53Z, the newest version (v2.0.0) in go_info.json.
    const NEWEST_MS: u64 = 1_678_786_013_000;

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

    fn root() -> String {
        format!("{}/github.com/!example!co/!pm!demo!tool", base_for(Registry::Go))
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

    /// The list from the fixture, with the oldest and newest `.info` answers.
    fn fixture_fake() -> Fake {
        Fake::default()
            .answer(format!("{}/@v/list", root()), ok(LIST))
            .answer(format!("{}/@v/v0.1.0.info", root()), ok(OLDEST_TIME))
            .answer(format!("{}/@v/v2.0.0.info", root()), ok(INFO))
    }

    #[test]
    fn escape_lowercases_each_upper_case_letter_behind_a_bang() {
        assert_eq!(escape("github.com/BurntSushi/toml"), "github.com/!burnt!sushi/toml");
        assert_eq!(escape("golang.org/x/net"), "golang.org/x/net");
        assert_eq!(escape("github.com/ExampleCo/PmDemoTool"), "github.com/!example!co/!pm!demo!tool");
    }

    #[test]
    fn versions_sort_by_semver_with_pre_releases_below_their_release() {
        let listed: Vec<String> = versions_in(LIST).into_iter().map(|(_, text)| text).collect();
        assert_eq!(
            listed,
            ["v0.1.0", "v0.9.0", "v1.0.0-beta.2", "v1.9.0", "v1.10.0", "v2.0.0-rc.1", "v2.0.0"]
        );

        let listed: Vec<String> = versions_in("v1.0.0-beta.11\nv1.0.0-beta.2\nv1.0.0\nv1.0.0-rc.1\nnot a version\n")
            .into_iter()
            .map(|(_, text)| text)
            .collect();
        assert_eq!(listed, ["v1.0.0-beta.2", "v1.0.0-beta.11", "v1.0.0-rc.1", "v1.0.0"]);
    }

    #[test]
    fn oldest_and_newest_by_semver_each_get_an_info_call() {
        let fake = fixture_fake();
        let facts = found(lookup(&fake, MODULE));
        assert_eq!(facts.ecosystem, Ecosystem::Go);
        assert_eq!(facts.created_ms, Some(OLDEST_MS));
        assert_eq!(facts.newest.as_deref(), Some("v2.0.0"));
        assert_eq!(facts.versions["v2.0.0"].published_ms, Some(NEWEST_MS));
        assert_eq!(facts.versions["v0.1.0"].published_ms, Some(OLDEST_MS));
        assert_eq!(facts.downloads, None);
        assert_eq!(facts.maintainers, None);
        assert_eq!(facts.source_link, None);
        assert!(!facts.big);

        let calls = fake.calls();
        assert_eq!(calls.len(), 3);
        assert!(calls.len() <= MAX_GETS_PER_PACKAGE);
        assert_eq!(calls[0], (format!("{}/@v/list", root()), "text/plain".to_string(), MAX_SMALL_BYTES));
        assert_eq!(calls[1].0, format!("{}/@v/v0.1.0.info", root()));
        assert_eq!(calls[2].0, format!("{}/@v/v2.0.0.info", root()));
    }

    #[test]
    fn a_single_version_needs_no_second_info_call() {
        let fake = Fake::default()
            .answer(format!("{}/@v/list", root()), ok("v1.2.3\n"))
            .answer(format!("{}/@v/v1.2.3.info", root()), ok(INFO));
        let facts = found(lookup(&fake, MODULE));
        assert_eq!(facts.newest.as_deref(), Some("v1.2.3"));
        assert_eq!(facts.created_ms, Some(NEWEST_MS));
        assert_eq!(fake.calls().len(), 2);
    }

    #[test]
    fn an_empty_list_asks_for_latest_once() {
        let fake = Fake::default()
            .answer(format!("{}/@v/list", root()), ok(""))
            .answer(format!("{}/@latest", root()), ok(INFO));
        let facts = found(lookup(&fake, MODULE));
        // Without a tagged version no creation time is claimed.
        assert_eq!(facts.created_ms, None);
        assert_eq!(facts.newest.as_deref(), Some("v2.0.0"));
        assert_eq!(facts.versions["v2.0.0"].published_ms, Some(NEWEST_MS));
        let calls = fake.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].0, format!("{}/@latest", root()));
    }

    #[test]
    fn a_list_with_no_valid_version_is_treated_as_empty() {
        let fake = Fake::default()
            .answer(format!("{}/@v/list", root()), ok("junk\nv1.2\n"))
            .answer(format!("{}/@latest", root()), ok(INFO));
        assert_eq!(found(lookup(&fake, MODULE)).newest.as_deref(), Some("v2.0.0"));
    }

    #[test]
    fn a_failed_info_call_drops_only_its_time() {
        let fake = Fake::default()
            .answer(format!("{}/@v/list", root()), ok(LIST))
            .answer(format!("{}/@v/v0.1.0.info", root()), Err(FetchError::Timeout))
            .answer(format!("{}/@v/v2.0.0.info", root()), ok(INFO));
        let facts = found(lookup(&fake, MODULE));
        assert_eq!(facts.created_ms, None);
        assert_eq!(facts.newest.as_deref(), Some("v2.0.0"));
        assert_eq!(facts.versions["v2.0.0"].published_ms, Some(NEWEST_MS));
    }

    #[test]
    fn not_found_on_the_list_is_not_found() {
        let fake = Fake::default().answer(format!("{}/@v/list", root()), Ok(Got::NotFound));
        assert_eq!(lookup(&fake, MODULE), Outcome::NotFound);
        assert_eq!(fake.calls().len(), 1);
    }

    #[test]
    fn not_found_or_failure_on_latest_ends_the_lookup() {
        let fake = Fake::default()
            .answer(format!("{}/@v/list", root()), ok(""))
            .answer(format!("{}/@latest", root()), Ok(Got::NotFound));
        assert_eq!(lookup(&fake, MODULE), Outcome::NotFound);

        let fake = Fake::default()
            .answer(format!("{}/@v/list", root()), ok(""))
            .answer(format!("{}/@latest", root()), Err(FetchError::TooMany));
        assert_eq!(lookup(&fake, MODULE), Outcome::Failed(FetchError::TooMany));
    }

    #[test]
    fn a_list_that_is_not_text_fails_the_package() {
        let fake = Fake::default().answer(format!("{}/@v/list", root()), Ok(Got::Ok(vec![0xff, 0xfe, 0x00])));
        assert_eq!(lookup(&fake, MODULE), Outcome::Failed(FetchError::Unreadable));
    }

    #[test]
    fn a_module_path_that_is_not_one_makes_no_request() {
        let fake = Fake::default();
        assert_eq!(lookup(&fake, "github.com/x/../y"), Outcome::Failed(FetchError::NotAllowed));
        assert_eq!(lookup(&fake, "not a path"), Outcome::Failed(FetchError::NotAllowed));
        assert!(fake.calls().is_empty());
    }
}
