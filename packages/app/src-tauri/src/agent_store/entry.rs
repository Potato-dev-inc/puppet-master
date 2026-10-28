//! `entry.toml`: the Store card data for one agent.
//!
//! Every table uses `deny_unknown_fields`, so a typo is an error and never a silent gap.
//! `parse_entry` reads the TOML and names the field that is wrong. `validate_entry` checks the
//! plain rules (lengths, closed lists, https addresses, plain text, dates). Rules that need the
//! other files of an entry (fixtures, the definition, publisher hosts) live in `check.rs`.

use serde::{Deserialize, Serialize};

use crate::tui::install::is_https_url;

/// The only `schema_version` this code reads.
pub const SCHEMA_VERSION: u32 = 1;
/// The longest tagline a card shows.
pub const TAGLINE_MAX: usize = 90;
/// The closed list of `categories` an entry may use.
pub const CATEGORIES: &[&str] =
    &["general", "open_source", "cloud_account", "bring_your_key", "local_models"];

/// One `entry.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryFile {
    pub schema_version: u32,
    pub store: StoreTable,
    pub vendor: VendorTable,
    pub sign_in: SignInTable,
    pub checked: CheckedTable,
    #[serde(default)]
    pub publisher: PublisherTable,
    #[serde(default)]
    pub uninstall: UninstallTable,
}

/// `[store]`: the card itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreTable {
    /// Equals the definition's `agent.id` and the folder name.
    pub id: String,
    pub tagline: String,
    pub tier: Tier,
    #[serde(default)]
    pub builtin: bool,
    pub categories: Vec<String>,
    pub status: Status,
    /// A catalog id that replaces this one, or empty.
    #[serde(default)]
    pub replaced_by: String,
    /// A short program name that another program may also use (`cn`, `pi`).
    #[serde(default)]
    pub ambiguous_name: bool,
    /// `YYYY-MM-DD`.
    pub first_listed: String,
}

/// `[vendor]`: who makes the agent and under what terms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VendorTable {
    pub name: String,
    pub homepage: String,
    /// An SPDX id, or `Proprietary`.
    pub license: String,
    pub license_kind: LicenseKind,
    /// Empty when there is no link.
    #[serde(default)]
    pub license_url: String,
    /// Empty when there is no link. Proprietary agents link their terms here.
    #[serde(default)]
    pub terms_url: String,
}

/// `[sign_in]`: how the person signs in. The Store never signs in for them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignInTable {
    pub kind: SignInKind,
    /// One plain sentence, shown as the vendor's wording.
    pub summary: String,
}

/// `[checked]`: when and how the facts were read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckedTable {
    /// `YYYY-MM-DD`.
    pub date: String,
    /// The version the facts were read for.
    pub version: String,
    pub how: How,
    /// False until someone ran the program on Windows and wrote the result down.
    pub windows_tested: bool,
}

/// `[publisher]`: the repository each package must come from. Empty when not used.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublisherTable {
    #[serde(default)]
    pub npm_repo: String,
    #[serde(default)]
    pub pypi_repo: String,
    /// Hosts a `script_url` may use.
    #[serde(default)]
    pub script_hosts: Vec<String>,
}

/// `[uninstall]`: the vendor's removal sentence, for script installs (which the Store cannot undo).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UninstallTable {
    #[serde(default)]
    pub note: String,
}

/// `verified` = "Tested". `starter` = "Basic support".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Verified,
    Starter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LicenseKind {
    OpenSource,
    SourceAvailable,
    Proprietary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignInKind {
    Account,
    ApiKey,
    Either,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Active,
    Deprecated,
}

/// Where the checked facts came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum How {
    RanOnWindows,
    PackageRegistry,
    VendorDocs,
}

/// Parses `entry.toml`. The error starts with the path of the field that is wrong
/// (`store.tier: ...`), or with the unknown key's name.
pub fn parse_entry(text: &str) -> Result<EntryFile, String> {
    let de = toml::Deserializer::new(text);
    serde_path_to_error::deserialize::<_, EntryFile>(de).map_err(|e| {
        let path = e.path().to_string();
        let message = e.inner().message();
        if path == "." {
            message.to_string()
        } else {
            format!("{path}: {message}")
        }
    })
}

/// The plain rules for one parsed entry. An empty list means the entry is fine. Every message
/// starts with the dotted field path.
pub fn validate_entry(e: &EntryFile) -> Vec<String> {
    let mut out = Vec::new();
    if e.schema_version != SCHEMA_VERSION {
        out.push(format!("schema_version: must be {SCHEMA_VERSION}"));
    }

    let s = &e.store;
    if !is_id(&s.id) {
        out.push(
            "store.id: use 1 to 40 lowercase letters, digits or _, starting with a letter".into(),
        );
    }
    plain_text(&mut out, "store.tagline", &s.tagline, 1, TAGLINE_MAX);
    if s.tagline.contains("://") || s.tagline.contains("www.") {
        out.push("store.tagline: no web addresses, the vendor link is shown on its own".into());
    }
    for (i, category) in s.categories.iter().enumerate() {
        if !CATEGORIES.contains(&category.as_str()) {
            out.push(format!("store.categories: unknown category `{category}`"));
        } else if s.categories[..i].contains(category) {
            out.push(format!("store.categories: `{category}` is listed twice"));
        }
    }
    if !s.replaced_by.is_empty() {
        if !is_id(&s.replaced_by) {
            out.push("store.replaced_by: must be another catalog id, or empty".into());
        } else if s.replaced_by == s.id {
            out.push("store.replaced_by: cannot point at the same agent".into());
        }
    }
    if !is_date(&s.first_listed) {
        out.push("store.first_listed: must be a date like 2026-10-28".into());
    }

    let v = &e.vendor;
    plain_text(&mut out, "vendor.name", &v.name, 1, 80);
    https_field(&mut out, "vendor.homepage", &v.homepage, true);
    plain_text(&mut out, "vendor.license", &v.license, 1, 60);
    https_field(&mut out, "vendor.license_url", &v.license_url, false);
    https_field(&mut out, "vendor.terms_url", &v.terms_url, false);

    plain_text(&mut out, "sign_in.summary", &e.sign_in.summary, 1, 200);

    let c = &e.checked;
    if !is_date(&c.date) {
        out.push("checked.date: must be a date like 2026-10-28".into());
    }
    plain_text(&mut out, "checked.version", &c.version, 1, 40);

    let p = &e.publisher;
    https_field(&mut out, "publisher.npm_repo", &p.npm_repo, false);
    https_field(&mut out, "publisher.pypi_repo", &p.pypi_repo, false);
    for host in &p.script_hosts {
        if !is_host(host) {
            out.push(format!("publisher.script_hosts: `{host}` is not a host name"));
        }
    }

    plain_text(&mut out, "uninstall.note", &e.uninstall.note, 0, 300);
    out
}

/// A catalog id: a lowercase letter, then lowercase letters, digits or `_`, at most 40 characters.
fn is_id(s: &str) -> bool {
    let mut chars = s.chars();
    s.len() <= 40
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `YYYY-MM-DD` with a month of 01-12 and a day of 01-31.
fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(|c| c.is_ascii_digit());
    if !(digits(0..4) && digits(5..7) && digits(8..10)) {
        return false;
    }
    let month: u32 = s[5..7].parse().unwrap_or(0);
    let day: u32 = s[8..10].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

/// A host name such as `example.com`: letters, digits, `.` and `-`, with a dot in it.
fn is_host(s: &str) -> bool {
    s.len() <= 253
        && s.contains('.')
        && !s.starts_with(['.', '-'])
        && !s.ends_with(['.', '-'])
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// Plain text that reaches the screen: a length range, no control characters, no markup.
fn plain_text(out: &mut Vec<String>, field: &str, value: &str, min: usize, max: usize) {
    let n = value.chars().count();
    if n < min || n > max {
        if min == 0 {
            out.push(format!("{field}: at most {max} characters"));
        } else {
            out.push(format!("{field}: must be {min} to {max} characters"));
        }
    }
    if value.chars().any(|c| c.is_control()) {
        out.push(format!("{field}: no control characters"));
    }
    if value.contains(['<', '>']) {
        out.push(format!("{field}: plain text only, no < or > markup"));
    }
}

/// An https address. `required = false` allows empty.
fn https_field(out: &mut Vec<String>, field: &str, value: &str, required: bool) {
    if value.is_empty() && !required {
        return;
    }
    if !is_https_url(value) {
        out.push(format!("{field}: must be an https:// address"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
schema_version = 1

[store]
id = "qwen"
tagline = "Alibaba's terminal coding agent."
tier = "starter"
builtin = false
categories = ["open_source"]
status = "active"
replaced_by = ""
ambiguous_name = false
first_listed = "2026-10-28"

[vendor]
name = "Alibaba Cloud (Qwen)"
homepage = "https://github.com/QwenLM/qwen-code"
license = "Apache-2.0"
license_kind = "open_source"
license_url = "https://github.com/QwenLM/qwen-code/blob/main/LICENSE"

[sign_in]
kind = "account"
summary = "Open it and type /auth, then follow the screen."

[checked]
date = "2026-10-28"
version = "0.26.2"
how = "package_registry"
windows_tested = false

[publisher]
npm_repo = "https://github.com/QwenLM/qwen-code"
pypi_repo = ""
script_hosts = ["qwen-code-assets.oss-cn-hangzhou.aliyuncs.com"]

[uninstall]
note = ""
"#;

    /// `VALID` with one line swapped for another.
    fn with(from: &str, to: &str) -> String {
        assert!(VALID.contains(from), "test text not in the valid entry: {from}");
        VALID.replacen(from, to, 1)
    }

    fn problems(text: &str) -> Vec<String> {
        validate_entry(&parse_entry(text).expect("parses"))
    }

    #[test]
    fn a_full_valid_entry_parses_and_passes() {
        let e = parse_entry(VALID).expect("parses");
        assert_eq!(e.store.id, "qwen");
        assert_eq!(e.store.tier, Tier::Starter);
        assert_eq!(e.vendor.license_kind, LicenseKind::OpenSource);
        assert_eq!(e.sign_in.kind, SignInKind::Account);
        assert_eq!(e.checked.how, How::PackageRegistry);
        assert_eq!(e.publisher.script_hosts.len(), 1);
        assert!(validate_entry(&e).is_empty(), "{:?}", validate_entry(&e));
    }

    #[test]
    fn a_minimal_builtin_entry_needs_no_publisher_or_uninstall_tables() {
        let text = VALID
            .split("[publisher]")
            .next()
            .expect("split")
            .to_string();
        let e = parse_entry(&text).expect("parses without the optional tables");
        assert!(e.publisher.npm_repo.is_empty());
        assert!(validate_entry(&e).is_empty());
    }

    #[test]
    fn an_unknown_key_is_refused_and_named() {
        let err = parse_entry(&with("builtin = false", "builtin = false\nsurprise = 1"))
            .expect_err("unknown key");
        assert!(err.contains("surprise"), "{err}");
    }

    #[test]
    fn a_bad_tier_is_refused_and_names_the_field() {
        let err = parse_entry(&with("tier = \"starter\"", "tier = \"gold\"")).expect_err("bad tier");
        assert!(err.contains("store.tier"), "{err}");
    }

    #[test]
    fn an_http_homepage_is_refused_and_names_the_field() {
        let p = problems(&with(
            "homepage = \"https://github.com/QwenLM/qwen-code\"",
            "homepage = \"http://github.com/QwenLM/qwen-code\"",
        ));
        assert!(p.iter().any(|m| m.starts_with("vendor.homepage:")), "{p:?}");
    }

    #[test]
    fn a_91_character_tagline_is_refused_and_names_the_field() {
        let long = "x".repeat(91);
        let p = problems(&with(
            "tagline = \"Alibaba's terminal coding agent.\"",
            &format!("tagline = \"{long}\""),
        ));
        assert!(p.iter().any(|m| m.starts_with("store.tagline:")), "{p:?}");
    }

    #[test]
    fn a_90_character_tagline_is_allowed() {
        let ok = "x".repeat(90);
        let p = problems(&with(
            "tagline = \"Alibaba's terminal coding agent.\"",
            &format!("tagline = \"{ok}\""),
        ));
        assert!(p.is_empty(), "{p:?}");
    }

    #[test]
    fn a_control_character_is_refused_and_names_the_field() {
        let p = problems(&with(
            "tagline = \"Alibaba's terminal coding agent.\"",
            "tagline = \"bad\\u0007bell\"",
        ));
        assert!(p.iter().any(|m| m.starts_with("store.tagline:") && m.contains("control")), "{p:?}");
    }

    #[test]
    fn a_bad_date_is_refused_and_names_the_field() {
        for bad in ["2026/10/28", "2026-13-01", "2026-10-32", "26-10-28"] {
            let p = problems(&with("date = \"2026-10-28\"", &format!("date = \"{bad}\"")));
            assert!(p.iter().any(|m| m.starts_with("checked.date:")), "{bad}: {p:?}");
        }
    }

    #[test]
    fn an_unknown_category_and_a_duplicate_are_refused() {
        let p = problems(&with("categories = [\"open_source\"]", "categories = [\"fun\"]"));
        assert!(p.iter().any(|m| m.starts_with("store.categories:")), "{p:?}");
        let p = problems(&with(
            "categories = [\"open_source\"]",
            "categories = [\"general\", \"general\"]",
        ));
        assert!(p.iter().any(|m| m.contains("listed twice")), "{p:?}");
    }

    #[test]
    fn a_bad_id_and_a_self_replacement_are_refused() {
        let p = problems(&with("id = \"qwen\"", "id = \"Qwen Code\""));
        assert!(p.iter().any(|m| m.starts_with("store.id:")), "{p:?}");
        let p = problems(&with("replaced_by = \"\"", "replaced_by = \"qwen\""));
        assert!(p.iter().any(|m| m.starts_with("store.replaced_by:")), "{p:?}");
    }

    #[test]
    fn markup_in_plain_text_is_refused() {
        let p = problems(&with(
            "summary = \"Open it and type /auth, then follow the screen.\"",
            "summary = \"<b>Sign in</b>\"",
        ));
        assert!(p.iter().any(|m| m.starts_with("sign_in.summary:")), "{p:?}");
    }

    #[test]
    fn a_bad_script_host_is_refused() {
        let p = problems(&with(
            "script_hosts = [\"qwen-code-assets.oss-cn-hangzhou.aliyuncs.com\"]",
            "script_hosts = [\"not a host\"]",
        ));
        assert!(p.iter().any(|m| m.starts_with("publisher.script_hosts:")), "{p:?}");
    }

    #[test]
    fn a_wrong_schema_version_is_refused() {
        let p = problems(&with("schema_version = 1", "schema_version = 2"));
        assert!(p.iter().any(|m| m.starts_with("schema_version:")), "{p:?}");
    }
}
