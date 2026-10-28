//! Every sentence Package check shows (SPEC-n33 section 3.10). Plain English: facts and numbers only.
//!
//! Callers pick a sentence and pass in numbers, ages or names. No registry text is ever copied in
//! here, so a hostile registry answer cannot put words on the card. A test at the bottom of this file
//! scans the code above it for banned verdict words.

use super::model::{How, Lookalike, PackageItem, Source};

pub const CHECKING: &str = "Checking…";
pub const STATUS_JUST_NOW: &str = "Checked just now";
pub const STATUS_PARTLY: &str = "Partly checked";
pub const STATUS_COULD_NOT: &str = "Could not check";
pub const STATUS_OFF: &str = "Lookups are off";
pub const LOOKUPS_OFF: &str = "Not checked: package lookups are off";
pub const OWN_SERVER: &str = "Not checked: this project uses its own package server";
pub const LOCAL: &str = "Installs from a folder or file on this computer. Not checked.";
pub const NOT_PLAIN: &str = "The name is not a plain package name. Not checked.";
pub const CUT: &str = "The name may be cut off on screen. Not checked.";
pub const TOO_MANY_PACKAGES: &str = "Not checked: too many packages in one command";
pub const TOO_MANY_LOOKUPS: &str = "Could not check: too many lookups right now. Try again in a minute.";
pub const NO_SCRIPT: &str = "Runs a script when installed";
pub const NO_WHEEL: &str = "No ready-built files: installing runs its setup code";
pub const DEPRECATED: &str = "Its authors marked it as deprecated";
pub const YANKED: &str = "This version was pulled back by its authors";
pub const TOO_BIG: &str = "Some facts are missing: the registry's answer was too large";
pub const NO_SOURCE_LINK: &str = "No link to its source code";
pub const SETTINGS_LABEL: &str = "Check packages before installs";
pub const SETTINGS_NOTE: &str = "Before an agent installs a package, look up its age, downloads and install scripts on its public registry (npm, PyPI, crates.io, RubyGems, Go). Only the package name is sent.";
pub const SETTINGS_OFF_NOTE: &str = "Look-alike names are still checked on this computer.";
pub const FOOTER_OFF: &str = "Look-alike names are still checked on this computer. Turn lookups on in Settings › General › Safety.";

/// A duration in words: "less than an hour", "23 hours", "59 days", "24 months", "2 years".
/// Takes milliseconds.
pub fn age(ms: u64) -> String {
    const MINUTE: u64 = 60_000;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    if ms < HOUR {
        return "less than an hour".to_string();
    }
    if ms < DAY {
        return plural(ms / HOUR, "hour", "hours");
    }
    let days = ms / DAY;
    if days < 60 {
        return plural(days, "day", "days");
    }
    if days < 730 {
        return plural(days / 30, "month", "months");
    }
    plural(days / 365, "year", "years")
}

fn plural(n: u64, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// A count in words: "1,234", "52K" (floor), "52.3M", "1.2B".
pub fn count(n: u64) -> String {
    if n < 10_000 {
        return group_thousands(n);
    }
    if n < 1_000_000 {
        return format!("{}K", n / 1_000);
    }
    if n < 1_000_000_000 {
        return format!("{:.1}M", n as f64 / 1_000_000.0);
    }
    format!("{:.1}B", n as f64 / 1_000_000_000.0)
}

fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The look-alike difference in words, as the card shows it.
pub fn how_text(how: How) -> &'static str {
    match how {
        How::OneLetter => "1 letter different",
        How::TwoLetters => "2 letters different",
        How::Swapped => "2 letters swapped",
        How::Separators => "only - _ . differ",
        How::LookAlikeChars => "look-alike characters",
    }
}

/// Look closer, offline: "Looks like lodash (2 letters swapped)".
pub fn lookalike_line(popular: &str, how: How) -> String {
    format!("Looks like {popular} ({})", how_text(how))
}

/// Look closer: "Not found on npm. The name may be made up."
pub fn not_found_line(registry: &str) -> String {
    format!("Not found on {registry}. The name may be made up.")
}

/// Look closer: "Installs from a link, not from npm: github.com".
pub fn link_line(registry: &str, host: &str) -> String {
    format!("Installs from a link, not from {registry}: {host}")
}

/// Look closer: "New: first published 3 days ago". `age` is the output of `age()`.
pub fn new_line(age: &str) -> String {
    format!("New: first published {age} ago")
}

/// Look closer: "Newest version came out 2 hours ago".
pub fn fresh_version_line(age: &str) -> String {
    format!("Newest version came out {age} ago")
}

/// Look closer, for a pinned version: "This version came out 2 hours ago".
pub fn fresh_pinned_line(age: &str) -> String {
    format!("This version came out {age} ago")
}

/// Look closer: "Few downloads: 12 a week". `n` is the output of `count()`.
pub fn few_weekly_line(n: &str) -> String {
    format!("Few downloads: {n} a week")
}

/// Look closer, for crates.io (an estimate): "Few downloads: about 12 a week".
pub fn few_weekly_about_line(n: &str) -> String {
    format!("Few downloads: about {n} a week")
}

/// Look closer, for RubyGems: "Few downloads: 900 in total".
pub fn few_total_line(n: &str) -> String {
    format!("Few downloads: {n} in total")
}

/// Look closer: a version has an install script ("Runs a script when installed").
pub fn install_script_line() -> &'static str {
    NO_SCRIPT
}

/// Look closer: PyPI version has no ready-built wheel.
pub fn no_wheel_line() -> &'static str {
    NO_WHEEL
}

/// Plain: "First published 2 years ago".
pub fn first_published_line(age: &str) -> String {
    format!("First published {age} ago")
}

/// Plain: "52 downloads a week".
pub fn weekly_line(n: &str) -> String {
    format!("{n} downloads a week")
}

/// Plain, for crates.io: "About 52 downloads a week".
pub fn about_weekly_line(n: &str) -> String {
    format!("About {n} downloads a week")
}

/// Plain, for RubyGems: "52K downloads in total".
pub fn total_line(n: &str) -> String {
    format!("{n} downloads in total")
}

/// Plain: "1 maintainer" or "3 maintainers".
pub fn maintainers_line(n: u32) -> String {
    if n == 1 {
        "1 maintainer".to_string()
    } else {
        format!("{n} maintainers")
    }
}

/// Plain, offline list hit, shown only when the lookup did not succeed.
pub fn well_known_line(rank_size: usize, registry: &str) -> String {
    format!("Well known: one of the {rank_size} most used {registry} packages")
}

/// Status line while the lookup runs: "Checking npm, PyPI…".
pub fn status_checking(registries: &str) -> String {
    format!("Checking {registries}…")
}

/// Status line: "Checked 4 minutes ago". `age` is the output of `age()`.
pub fn status_checked_ago(age: &str) -> String {
    format!("Checked {age} ago")
}

/// Header with a Look closer row: "Look closer · 1 of 3".
pub fn look_count(k: u32, n: u32) -> String {
    format!("Look closer · {k} of {n}")
}

/// Footer: "Facts from the public npm registry. Only the package names were sent. Puppet Master never answers for you."
pub fn footer(registries: &str) -> String {
    format!(
        "Facts from the public {registries} registry. Only the package names were sent. Puppet Master never answers for you."
    )
}

/// Footer: "1 more package was not checked" / "8 more packages were not checked".
pub fn more_footer(n: u32) -> String {
    if n == 1 {
        "1 more package was not checked".to_string()
    } else {
        format!("{n} more packages were not checked")
    }
}

/// Footer: "1 name looked like a secret, so it was not shown or checked".
pub fn hidden_footer(n: u32) -> String {
    if n == 1 {
        "1 name looked like a secret, so it was not shown or checked".to_string()
    } else {
        format!("{n} names looked like a secret, so they were not shown or checked")
    }
}

/// Details link on the card: "+5 more in Details".
pub fn more_in_details(n: u32) -> String {
    format!("+{n} more in Details")
}

/// Not checked: this ecosystem has no lookup in v1.
pub fn unsupported_line(tool: &str) -> String {
    format!("Not checked: Puppet Master can't look up {tool} packages yet")
}

/// Not checked: a list file was named (`-r requirements.txt`). `file` is the basename.
pub fn list_file_line(file: &str) -> String {
    format!("Installs the packages listed in {file}. The list was not checked.")
}

/// Failed: the registry could not be reached.
pub fn unreachable_line(registry: &str) -> String {
    format!("Could not check: {registry} could not be reached")
}

/// Failed: the registry did not answer in time.
pub fn timeout_line(registry: &str) -> String {
    format!("Could not check: {registry} did not answer in time")
}

/// Failed: the registry sent an answer that cannot be read.
pub fn unreadable_line(registry: &str) -> String {
    format!("Could not check: {registry} sent an answer Puppet Master can't read")
}

/// Failed: the registry refused the request.
pub fn refused_line(registry: &str) -> String {
    format!("Could not check: {registry} refused the request")
}

/// The first paint of a card row: the offline facts first, then "Checking…" or the one sentence
/// that says why nothing is checked. Returns the text and whether it has a Look closer fact.
pub fn first_line(item: &PackageItem) -> (String, bool) {
    let mut parts: Vec<String> = Vec::new();
    let mut look = false;
    if let Some(Lookalike { popular, how }) = &item.lookalike {
        parts.push(lookalike_line(popular, *how));
        look = true;
    }
    if let Source::Link { host } = &item.source {
        parts.push(link_line(item.ecosystem.registry_label(), host));
        look = true;
    }
    if item.lookup {
        parts.push(CHECKING.to_string());
    } else if let Some(sentence) = not_checked_sentence(item) {
        parts.push(sentence);
    }
    (parts.join(" · "), look)
}

fn not_checked_sentence(item: &PackageItem) -> Option<String> {
    match &item.source {
        Source::Local => Some(LOCAL.to_string()),
        Source::ListFile { file } => Some(list_file_line(file)),
        Source::NotPlain => Some(NOT_PLAIN.to_string()),
        Source::PrivateServer => Some(OWN_SERVER.to_string()),
        Source::Link { .. } => item.cut.then(|| CUT.to_string()),
        Source::Registry => {
            if item.cut {
                Some(CUT.to_string())
            } else if !item.ecosystem.looks_up() {
                Some(unsupported_line(&item.tool))
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::Ecosystem;
    use super::*;

    const MINUTE: u64 = 60_000;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;

    #[test]
    fn age_boundaries() {
        assert_eq!(age(59 * MINUTE), "less than an hour");
        assert_eq!(age(HOUR), "1 hour");
        assert_eq!(age(23 * HOUR), "23 hours");
        assert_eq!(age(DAY), "1 day");
        assert_eq!(age(59 * DAY), "59 days");
        assert_eq!(age(60 * DAY), "2 months");
        assert_eq!(age(729 * DAY), "24 months");
        assert_eq!(age(730 * DAY), "2 years");
    }

    #[test]
    fn count_boundaries() {
        assert_eq!(count(0), "0");
        assert_eq!(count(1_234), "1,234");
        assert_eq!(count(9_999), "9,999");
        assert_eq!(count(10_000), "10K");
        assert_eq!(count(999_999), "999K");
        assert_eq!(count(1_000_000), "1.0M");
        assert_eq!(count(52_300_000), "52.3M");
        assert_eq!(count(1_200_000_000), "1.2B");
    }

    #[test]
    fn plural_forms_in_small_sentences() {
        assert_eq!(maintainers_line(1), "1 maintainer");
        assert_eq!(maintainers_line(3), "3 maintainers");
        assert_eq!(more_footer(1), "1 more package was not checked");
        assert_eq!(hidden_footer(1), "1 name looked like a secret, so it was not shown or checked");
    }

    #[test]
    fn lookalike_how_texts() {
        assert_eq!(lookalike_line("lodash", How::Swapped), "Looks like lodash (2 letters swapped)");
        assert_eq!(how_text(How::Separators), "only - _ . differ");
        assert_eq!(how_text(How::LookAlikeChars), "look-alike characters");
    }

    fn sample(source: Source, lookup: bool, cut: bool, lookalike: Option<Lookalike>) -> PackageItem {
        PackageItem {
            ecosystem: Ecosystem::Npm,
            tool: "npm".into(),
            name: "lodahs".into(),
            spec: None,
            source,
            global: false,
            runs_now: false,
            lookalike,
            popular: false,
            cut,
            lookup,
            first: String::new(),
            look: false,
        }
    }

    #[test]
    fn first_line_paints_lookalike_then_checking() {
        let item = sample(
            Source::Registry,
            true,
            false,
            Some(Lookalike { popular: "lodash".into(), how: How::Swapped }),
        );
        assert_eq!(first_line(&item), ("Looks like lodash (2 letters swapped) · Checking…".to_string(), true));
    }

    #[test]
    fn first_line_says_why_nothing_is_checked() {
        let winget = PackageItem {
            ecosystem: Ecosystem::Winget,
            tool: "winget".into(),
            ..sample(Source::Registry, false, false, None)
        };
        assert_eq!(first_line(&winget).0, "Not checked: Puppet Master can't look up winget packages yet");
        let local = sample(Source::Local, false, false, None);
        assert_eq!(first_line(&local), (LOCAL.to_string(), false));
        let link = sample(Source::Link { host: "github.com".into() }, false, false, None);
        assert_eq!(
            first_line(&link),
            ("Installs from a link, not from npm: github.com".to_string(), true)
        );
        let cut = sample(Source::Registry, false, true, None);
        assert_eq!(first_line(&cut), (CUT.to_string(), false));
    }

    /// Banned verdict words. Matched as whole words; "OK" is matched in capitals only.
    const BANNED: [&str; 9] =
        ["safe", "verified", "trusted", "malicious", "malware", "approved", "secure", "clean", "OK"];

    #[test]
    fn words_rs_has_no_banned_words() {
        let source = include_str!("words.rs");
        // Only the code above the test module is scanned, so the list above is not its own hit.
        let code = source.split("#[cfg(test)]").next().expect("source has code");
        let tokens: Vec<&str> =
            code.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).collect();
        for word in BANNED {
            let hit = tokens.iter().any(|t| if word == "OK" { *t == word } else { t.eq_ignore_ascii_case(word) });
            assert!(!hit, "words.rs must not contain the banned word {word}");
        }
    }
}
