//! PyPI lookup (SPEC-n33 section 3.6): the project's simple index (JSON), one GET.
//!
//! Only the project name is in the URL (PEP 503 form). Facts come from the file names and upload
//! times of the index. PyPI publishes no download counts, maintainers or source links here.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::package_check::model::{
    base_for, Ecosystem, Facts, FetchError, Fetcher, Got, Outcome, Registry, VersionFacts, MAX_DOC_BYTES,
    MAX_VERSIONS_KEPT,
};

/// The simple index, JSON form (PEP 691).
const ACCEPT_SIMPLE: &str = "application/vnd.pypi.simple.v1+json";
/// A version text longer than this is not kept (its file is skipped).
const MAX_VERSION_LEN: usize = 64;

/// The simple index URL for a project. The name is PEP 503 normalized: `Demo.Sdist_Only` -> `demo-sdist-only`.
fn simple_url(name: &str) -> String {
    format!("{}/simple/{}/", base_for(Registry::Pypi), Ecosystem::Pypi.normalize(name))
}

/// Looks one PyPI project up. One GET.
pub fn lookup(f: &dyn Fetcher, name: &str) -> Outcome {
    if !Ecosystem::Pypi.valid_name(name) {
        return Outcome::Failed(FetchError::NotAllowed);
    }
    let body = match f.get(&simple_url(name), ACCEPT_SIMPLE, MAX_DOC_BYTES) {
        Ok(Got::Ok(body)) => body,
        Ok(Got::NotFound) => return Outcome::NotFound,
        Err(e) => return Outcome::Failed(e),
    };
    let Ok(doc) = serde_json::from_slice::<SimpleDoc>(&body) else {
        return Outcome::Failed(FetchError::Unreadable);
    };
    Outcome::Found(facts_from(name, doc.files))
}

/// One distribution file of the project, read from the index.
struct Upload {
    version: String,
    wheel: bool,
    yanked: bool,
    at: Option<u64>,
}

fn facts_from(name: &str, files: Vec<SimpleFile>) -> Facts {
    let project = Ecosystem::Pypi.normalize(name);
    // Files that are not this project's distributions (or have an odd version) are skipped.
    let uploads: Vec<Upload> = files
        .into_iter()
        .filter_map(|file| {
            let version = version_from_filename(&file.filename, &project)?;
            Some(Upload {
                version,
                wheel: file.filename.ends_with(".whl"),
                yanked: is_yanked(&file.yanked),
                at: file.upload_time.as_deref().and_then(parse_ms),
            })
        })
        .collect();

    // The project's first upload of any file.
    let created_ms = uploads.iter().filter_map(|u| u.at).min();
    // The version of the newest upload that is not yanked.
    let newest = uploads
        .iter()
        .filter(|u| !u.yanked)
        .filter_map(|u| u.at.map(|at| (at, &u.version)))
        .max_by_key(|(at, _)| *at)
        .map(|(_, version)| version.clone());

    let mut by_version: BTreeMap<&str, Vec<&Upload>> = BTreeMap::new();
    for upload in &uploads {
        by_version.entry(upload.version.as_str()).or_default().push(upload);
    }
    let mut listed: Vec<(Option<u64>, String, VersionFacts)> = by_version
        .into_iter()
        .map(|(version, files)| {
            let published_ms = files.iter().filter_map(|u| u.at).min();
            let facts = VersionFacts {
                published_ms,
                install_script: None,
                prebuilt: Some(files.iter().any(|u| u.wheel)),
                deprecated: false,
                yanked: files.iter().all(|u| u.yanked),
            };
            (published_ms, version.to_string(), facts)
        })
        .collect();
    // Newest first; a version with no time sorts last. Only the newest `MAX_VERSIONS_KEPT` are kept.
    listed.sort_by(|a, b| b.0.cmp(&a.0));
    listed.truncate(MAX_VERSIONS_KEPT);
    let versions: BTreeMap<String, VersionFacts> =
        listed.into_iter().map(|(_, version, facts)| (version, facts)).collect();

    Facts {
        ecosystem: Ecosystem::Pypi,
        created_ms,
        newest,
        tags: BTreeMap::new(),
        versions,
        downloads: None,
        maintainers: None,
        source_link: None,
        big: false,
    }
}

/// The version a distribution file is for. A wheel is `name-version-...whl`; an sdist is
/// `name-version.tar.gz` (or `.tar.bz2`, `.zip`). For an sdist the name may itself contain `-`, so the
/// first split whose left side is this project's name is used. None for a file of another project.
fn version_from_filename(filename: &str, project: &str) -> Option<String> {
    let version = if let Some(stem) = filename.strip_suffix(".whl") {
        let mut parts = stem.split('-');
        if Ecosystem::Pypi.normalize(parts.next()?) != project {
            return None;
        }
        parts.next()?.to_string()
    } else {
        let stem = [".tar.gz", ".tar.bz2", ".zip"].into_iter().find_map(|ext| filename.strip_suffix(ext))?;
        stem.match_indices('-')
            .find(|(i, _)| Ecosystem::Pypi.normalize(&stem[..*i]) == project)
            .map(|(i, _)| stem[i + 1..].to_string())?
    };
    let plain = !version.is_empty()
        && version.len() <= MAX_VERSION_LEN
        && version.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '!' | '-' | '_'));
    plain.then_some(version)
}

/// PEP 592: `yanked` is a boolean or a reason string. A reason string means yanked.
fn is_yanked(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Bool(yanked) => *yanked,
        serde_json::Value::String(_) => true,
        _ => false,
    }
}

/// An RFC 3339 upload time (`2023-01-10T08:00:00.000000Z`) as ms since the epoch.
fn parse_ms(text: &str) -> Option<u64> {
    let at = chrono::DateTime::parse_from_rfc3339(text).ok()?;
    u64::try_from(at.timestamp_millis()).ok()
}

#[derive(Deserialize)]
struct SimpleDoc {
    #[serde(default)]
    files: Vec<SimpleFile>,
}

#[derive(Deserialize)]
struct SimpleFile {
    filename: String,
    #[serde(default, rename = "upload-time")]
    upload_time: Option<String>,
    #[serde(default)]
    yanked: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const PYPI_WHEELS: &str = include_str!("../fixtures/pypi_wheels.json");
    const PYPI_SDIST_ONLY: &str = include_str!("../fixtures/pypi_sdist_only.json");

    /// Fake network: one answer per (url, Accept). Any other request is `Unreachable`. Every request
    /// is logged as (url, Accept, byte cap).
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
    fn simple_url_uses_the_pep503_name() {
        assert_eq!(
            simple_url("Demo.Sdist_Only"),
            format!("{}/simple/demo-sdist-only/", base_for(Registry::Pypi))
        );
    }

    #[test]
    fn version_comes_from_wheel_and_sdist_file_names() {
        let cases = [
            ("demo_widget-1.0.0-py3-none-any.whl", Some("1.0.0")),
            ("demo_widget-1.0.0-1-py3-none-any.whl", Some("1.0.0")), // with a build tag
            ("demo-widget-1.0.0.tar.gz", Some("1.0.0")),
            ("demo-widget-1.0-1.tar.gz", Some("1.0-1")), // a dash inside the version
            ("Demo.Widget-2.0.0.zip", Some("2.0.0")),    // sdist name in another spelling
            ("demo-widget-3.0.0.tar.bz2", Some("3.0.0")),
            ("other_pkg-1.0.0-py3-none-any.whl", None),  // another project
            ("other-pkg-1.0.0.tar.gz", None),
            ("demo-widget-1.0.0.exe", None),             // not a distribution
            ("README.txt", None),
            ("demo-widget-1.0.0 x.tar.gz", None),       // a space in the version
            ("demo-widget-.tar.gz", None),              // empty version
        ];
        for (file, want) in cases {
            assert_eq!(version_from_filename(file, "demo-widget").as_deref(), want, "file {file}");
        }
    }

    #[test]
    fn wheels_give_dates_newest_non_yanked_version_and_yanks() {
        let name = "demo-widget";
        let fake = Fake::new(vec![(simple_url(name), ACCEPT_SIMPLE, ok(PYPI_WHEELS))]);
        let facts = found(lookup(&fake, name));
        assert_eq!(facts.ecosystem, Ecosystem::Pypi);
        assert_eq!(facts.created_ms, Some(1_673_337_600_000)); // earliest upload: 2023-01-10T08:00Z
        // 1.2.0 is the newest upload, but every file of it is yanked.
        assert_eq!(facts.newest.as_deref(), Some("1.1.0"));
        // README.txt is not a distribution, so only three versions.
        assert_eq!(facts.versions.len(), 3);
        let first = &facts.versions["1.0.0"];
        assert_eq!(first.published_ms, Some(1_673_337_600_000));
        assert_eq!(first.prebuilt, Some(true));
        assert!(!first.yanked);
        let second = &facts.versions["1.1.0"];
        assert_eq!(second.prebuilt, Some(true));
        assert!(!second.yanked);
        let yanked = &facts.versions["1.2.0"];
        assert!(yanked.yanked);
        assert_eq!(yanked.prebuilt, Some(true));
        assert_eq!(facts.downloads, None);
        assert_eq!(facts.maintainers, None);
        assert_eq!(facts.source_link, None);
        assert!(!facts.big);
        assert_eq!(fake.asked().len(), 1);
    }

    #[test]
    fn sdist_only_project_is_not_prebuilt_and_uses_the_normalized_url() {
        let name = "Demo.Sdist_Only";
        let fake = Fake::new(vec![(simple_url(name), ACCEPT_SIMPLE, ok(PYPI_SDIST_ONLY))]);
        let facts = found(lookup(&fake, name));
        assert_eq!(fake.asked()[0].0, format!("{}/simple/demo-sdist-only/", base_for(Registry::Pypi)));
        assert_eq!(fake.asked()[0].1, ACCEPT_SIMPLE);
        assert_eq!(fake.asked()[0].2, MAX_DOC_BYTES);
        assert_eq!(facts.created_ms, Some(1_706_832_000_000)); // 2024-02-02T00:00Z, the 0.3.0 zip
        assert_eq!(facts.newest.as_deref(), Some("0.4.0"));
        assert_eq!(facts.versions["0.4.0"].prebuilt, Some(false));
        assert_eq!(facts.versions["0.3.0"].prebuilt, Some(false));
        assert!(!facts.versions["0.4.0"].yanked);
    }

    #[test]
    fn not_found_project_is_not_found() {
        let name = "no-such-zinc-project";
        let fake = Fake::new(vec![(simple_url(name), ACCEPT_SIMPLE, Ok(Got::NotFound))]);
        assert_eq!(lookup(&fake, name), Outcome::NotFound);
        assert_eq!(fake.asked().len(), 1);
    }

    #[test]
    fn failed_call_fails_the_package() {
        let name = "demo-widget";
        let fake = Fake::new(vec![(simple_url(name), ACCEPT_SIMPLE, Err(FetchError::Timeout))]);
        assert_eq!(lookup(&fake, name), Outcome::Failed(FetchError::Timeout));
    }

    #[test]
    fn names_that_are_not_plain_pypi_names_make_no_request() {
        for bad in ["../x", "has space", "-leading", "a/b"] {
            let fake = Fake::new(vec![]);
            assert_eq!(lookup(&fake, bad), Outcome::Failed(FetchError::NotAllowed), "name {bad}");
            assert!(fake.asked().is_empty(), "name {bad} sent a request");
        }
    }

    #[test]
    fn unreadable_index_fails_the_package() {
        let name = "demo-widget";
        let fake = Fake::new(vec![(simple_url(name), ACCEPT_SIMPLE, ok("<html>not json</html>"))]);
        assert_eq!(lookup(&fake, name), Outcome::Failed(FetchError::Unreadable));
    }

    #[test]
    fn version_map_is_capped_and_the_newest_kept() {
        // Built in code, not a fixture: MAX_VERSIONS_KEPT + 3 made-up wheels, one year apart.
        const N: u32 = MAX_VERSIONS_KEPT as u32 + 3;
        let files: Vec<String> = (0..N)
            .map(|i| {
                format!(
                    "{{\"filename\":\"demo_widget-0.0.{i}-py3-none-any.whl\",\"upload-time\":\"{:04}-01-01T00:00:00.000Z\"}}",
                    2000 + i
                )
            })
            .collect();
        let body = format!("{{\"files\":[{}]}}", files.join(","));
        let name = "demo-widget";
        let fake = Fake::new(vec![(simple_url(name), ACCEPT_SIMPLE, ok(&body))]);
        let facts = found(lookup(&fake, name));
        assert_eq!(facts.versions.len(), MAX_VERSIONS_KEPT);
        assert!(facts.versions.contains_key(&format!("0.0.{}", N - 1)));
        assert!(!facts.versions.contains_key("0.0.0"));
        assert_eq!(facts.newest.as_deref(), Some(format!("0.0.{}", N - 1).as_str()));
    }
}
