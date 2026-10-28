//! The fixed risk table (SPEC-n33 section 3.3). Pure: facts in, reasons out. No I/O, no network, no
//! model, no score. The order is this table and nothing else. Tier 1 is shown first; each flag that
//! applies adds plain reasons, and the item's tier is its riskiest flag. Names in the sentences come
//! from `Facts`, which the sources have already cleaned (secrets masked, home folder as `~`).

use super::model::{Facts, Flag, Item, ItemKey, Level, Reason, Risk, BIG_FILES, BIG_LINES, ITEMS_CAP, NAMES_SHOWN};
use crate::tamper::Role;

/// Package lists and lock files, matched on the file name (case-insensitive). `*` stands for any run of
/// characters. Language-neutral data, no agent names.
pub const PACKAGE_FILES: &[&str] = &[
    "package.json", "package-lock.json", "npm-shrinkwrap.json", "yarn.lock", "pnpm-lock.yaml", "bun.lock",
    "bun.lockb", "Cargo.toml", "Cargo.lock", "pyproject.toml", "poetry.lock", "uv.lock", "Pipfile", "Pipfile.lock",
    "requirements*.txt", "go.mod", "go.sum", "Gemfile", "Gemfile.lock", "composer.json", "composer.lock", "pom.xml",
    "build.gradle", "build.gradle.kts", "*.csproj", "packages.config",
];

pub fn is_package_file(path: &str) -> bool {
    let name = base_name(path).to_ascii_lowercase();
    PACKAGE_FILES.iter().any(|pattern| wildcard_match(&pattern.to_ascii_lowercase(), &name))
}

/// The table, tier by tier, in reason order. `role` says what a path is (`tamper::Role`), or None.
pub fn flags(facts: &Facts, role: &dyn Fn(&str) -> Option<Role>) -> Vec<Reason> {
    let mut out = Vec::new();

    // Tier 1: the tests were weakened (only where a snapshot exists).
    if let Some(first) = facts.weakened.first() {
        let more = more_suffix(facts.weakened.len() - 1);
        out.push(reason(Flag::TestsWeakened, format!("The tests were weakened: {}{more}", tidy(first))));
    }

    // Tier 2: something sensitive.
    if !facts.secret_files.is_empty() {
        out.push(reason(Flag::Sensitive, format!("A file may hold a secret: {}", listed(&facts.secret_files))));
    }
    if !facts.secret_lines.is_empty() {
        out.push(reason(Flag::Sensitive, format!("A line looks like a secret: {}", located(&facts.secret_lines))));
    }
    if !facts.ci_files.is_empty() {
        out.push(reason(Flag::Sensitive, format!("CI files changed: {}", listed(&facts.ci_files))));
    }
    if !facts.home_lines.is_empty() {
        out.push(reason(Flag::Sensitive, format!("A line has your home folder: {}", located(&facts.home_lines))));
    }

    // Tier 3: checks ran and not all passed.
    if let Some(checks) = &facts.checks {
        if checks.passed < checks.total {
            let failed = if checks.failed.is_empty() { String::new() } else { format!(" ({})", listed(&checks.failed)) };
            out.push(reason(
                Flag::ChecksFailed,
                format!("Checks failed: {} of {} passed{failed}", grouped(checks.passed.into()), grouped(checks.total.into())),
            ));
        }
    }

    // Tiers 4 and 5 read the changed paths. A package list is never a "check setting", even though its role is Config.
    if let Some(files) = &facts.files {
        let (mut tests, mut settings, mut packages) = (Vec::new(), Vec::new(), Vec::new());
        for file in files {
            let package = is_package_file(&file.path);
            match role(&file.path) {
                Some(Role::Test | Role::Snapshot) => tests.push(file.path.clone()),
                Some(Role::Config) if !package => settings.push(file.path.clone()),
                _ => {}
            }
            if package {
                packages.push(file.path.clone());
            }
        }
        if !tests.is_empty() {
            out.push(reason(Flag::TestsChanged, format!("Test files changed: {}", listed(&tests))));
        }
        if !settings.is_empty() {
            out.push(reason(Flag::TestsChanged, format!("Check settings changed: {}", listed(&settings))));
        }
        if !packages.is_empty() {
            out.push(reason(Flag::PackagesChanged, format!("A package list changed: {}", listed(&packages))));
        }
    }

    // Tier 6: a big change, or Puppet Master could not see the files.
    let lines = u64::from(facts.added) + u64::from(facts.deleted);
    if facts.files_total >= BIG_FILES || lines >= u64::from(BIG_LINES) {
        out.push(reason(
            Flag::Big,
            format!(
                "Big change: {} files, +{} \u{2212}{}",
                grouped(facts.files_total.into()),
                grouped(facts.added.into()),
                grouped(facts.deleted.into())
            ),
        ));
    }
    if facts.files.is_none() {
        out.push(reason(Flag::Unknown, "Puppet Master could not see which files changed".to_string()));
    }

    out
}

pub fn risk(facts: &Facts, role: &dyn Fn(&str) -> Option<Role>) -> Risk {
    let reasons = flags(facts, role);
    let tier = reasons.iter().map(|reason| tier_of(reason.flag)).min().unwrap_or(7);
    Risk { tier, level: level_of(tier), reasons }
}

/// Riskiest tier first. Inside a tier: more reasons, then bigger (added + deleted), then older
/// (`finished_at_ms`), then the key. Keeps at most `ITEMS_CAP`.
pub fn order(items: &mut Vec<Item>) {
    items.sort_by(|a, b| {
        a.risk
            .tier
            .cmp(&b.risk.tier)
            .then_with(|| b.risk.reasons.len().cmp(&a.risk.reasons.len()))
            .then_with(|| size(b).cmp(&size(a)))
            .then_with(|| a.finished_at_ms.cmp(&b.finished_at_ms))
            .then_with(|| a.key.as_str().cmp(b.key.as_str()))
    });
    items.truncate(ITEMS_CAP);
}

/// What the person saw: a short hash of the key and the facts. The title is not part of it.
pub fn fingerprint(key: &ItemKey, facts: &Facts) -> String {
    let body = serde_json::to_string(facts).unwrap_or_default();
    let digest = <sha2::Sha256 as sha2::Digest>::digest(format!("{}\n{body}", key.as_str()).as_bytes());
    digest[..6].iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn level_word(level: Level) -> &'static str {
    match level {
        Level::High => "High risk",
        Level::Check => "Check first",
        Level::Low => "Low risk",
    }
}

fn tier_of(flag: Flag) -> u8 {
    match flag {
        Flag::TestsWeakened => 1,
        Flag::Sensitive => 2,
        Flag::ChecksFailed => 3,
        Flag::TestsChanged => 4,
        Flag::PackagesChanged => 5,
        Flag::Big | Flag::Unknown => 6,
    }
}

fn level_of(tier: u8) -> Level {
    match tier {
        1 | 2 => Level::High,
        3..=6 => Level::Check,
        _ => Level::Low,
    }
}

fn size(item: &Item) -> u64 {
    u64::from(item.facts.added) + u64::from(item.facts.deleted)
}

fn reason(flag: Flag, sentence: String) -> Reason {
    Reason { flag, sentence }
}

/// The first `NAMES_SHOWN` names, then ` and N more`.
fn listed<T: AsRef<str>>(names: &[T]) -> String {
    let shown: Vec<String> = names.iter().take(NAMES_SHOWN).map(|name| tidy(name.as_ref())).collect();
    format!("{}{}", shown.join(", "), more_suffix(names.len().saturating_sub(NAMES_SHOWN)))
}

fn located(lines: &[(String, u32)]) -> String {
    let places: Vec<String> = lines.iter().map(|(path, line)| format!("{path}:{line}")).collect();
    listed(&places)
}

fn more_suffix(more: usize) -> String {
    if more == 0 { String::new() } else { format!(" and {more} more") }
}

/// Removes control characters and bidi overrides. Display safety only; secrets are masked by the sources.
fn tidy(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() && !matches!(*c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
        .collect()
}

fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn base_name(path: &str) -> &str {
    path.rsplit(|c: char| c == '/' || c == '\\').next().unwrap_or(path)
}

/// A pattern with at most one `*`. `name` is already lower-case.
fn wildcard_match(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, tail)) => name.len() >= head.len() + tail.len() && name.starts_with(head) && name.ends_with(tail),
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::{CheckFacts, FileRow, Keep, Source, Throw};
    use super::*;

    fn row(path: &str, added: u32, deleted: u32) -> FileRow {
        FileRow { path: path.to_string(), status: 'M', added, deleted, binary: false }
    }

    /// Facts for these changed files. Totals come from the rows; the other fields are empty.
    fn facts_of(files: Vec<FileRow>) -> Facts {
        Facts {
            files_total: files.len() as u32,
            added: files.iter().map(|file| file.added).sum(),
            deleted: files.iter().map(|file| file.deleted).sum(),
            files: Some(files),
            lines_scanned: true,
            ..Facts::default()
        }
    }

    /// The path roles the real rules give for these names (the fake stands in for `tamper::path_role`).
    fn roles(path: &str) -> Option<Role> {
        let name = base_name(path).to_ascii_lowercase();
        if name.contains(".test.") || name.contains(".spec.") || name.starts_with("test_") {
            Some(Role::Test)
        } else if name.ends_with(".snap") {
            Some(Role::Snapshot)
        } else if name.starts_with("jest.config") || name == "pytest.ini" || name == "package.json" {
            Some(Role::Config)
        } else {
            None
        }
    }

    fn item(key: &str, facts: Facts, finished_at_ms: u64) -> Item {
        let key = ItemKey::parse(key).expect("valid test key");
        let risk_value = risk(&facts, &roles);
        Item {
            fingerprint: fingerprint(&key, &facts),
            key,
            source: Source::Copy,
            title: "Fixture change".to_string(),
            agent_label: "Fixture agent".to_string(),
            account: None,
            where_label: "Separate copy of main".to_string(),
            project: r"C:\Users\you\app".to_string(),
            pane_id: None,
            finished_at_ms,
            turns: 1,
            group: None,
            facts,
            risk: risk_value,
            keep: Keep::No { why: "fixture".to_string() },
            throw: Throw::No { why: "fixture".to_string() },
            fix: Err("fixture".to_string()),
        }
    }

    fn sentences(risk_value: &Risk) -> Vec<&str> {
        risk_value.reasons.iter().map(|reason| reason.sentence.as_str()).collect()
    }

    #[test]
    fn idea_order_puts_tamper_then_packages_then_small() {
        let small = item("copy:small", facts_of(vec![row("src/util.ts", 1, 0)]), 100);
        let packages = item("copy:packages", facts_of(vec![row("package-lock.json", 40, 2)]), 200);
        let tamper = item(
            "copy:tamper",
            Facts { weakened: vec!["2 tests were skipped in cart.test.ts".to_string()], ..facts_of(vec![row("src/cart.ts", 3, 1)]) },
            300,
        );
        let mut items = vec![small, packages, tamper];
        order(&mut items);
        let keys: Vec<&str> = items.iter().map(|item| item.key.as_str()).collect();
        assert_eq!(keys, ["copy:tamper", "copy:packages", "copy:small"]);
    }

    #[test]
    fn tier1_tests_weakened() {
        let facts = Facts {
            weakened: vec!["2 tests were skipped in cart.test.ts".to_string(), "1 test was removed in money.test.ts".to_string()],
            ..facts_of(vec![row("src/cart.test.ts", 1, 1)])
        };
        let found = risk(&facts, &roles);
        assert_eq!(found.tier, 1);
        assert_eq!(found.level, Level::High);
        assert_eq!(found.reasons[0].flag, Flag::TestsWeakened);
        assert_eq!(sentences(&found)[0], "The tests were weakened: 2 tests were skipped in cart.test.ts and 1 more");
    }

    #[test]
    fn tier2_sensitive_four_kinds() {
        let facts = Facts {
            secret_files: vec![".env".to_string()],
            secret_lines: vec![("src/config.ts".to_string(), 12)],
            ci_files: vec![".github/workflows/ci.yml".to_string()],
            home_lines: vec![("notes.md".to_string(), 3)],
            ..facts_of(vec![row("src/config.ts", 1, 0)])
        };
        let found = risk(&facts, &roles);
        assert_eq!(found.tier, 2);
        assert_eq!(found.level, Level::High);
        assert!(found.reasons.iter().all(|reason| reason.flag == Flag::Sensitive));
        assert_eq!(
            sentences(&found),
            [
                "A file may hold a secret: .env",
                "A line looks like a secret: src/config.ts:12",
                "CI files changed: .github/workflows/ci.yml",
                "A line has your home folder: notes.md:3",
            ]
        );
    }

    #[test]
    fn tier3_checks_failed() {
        let facts = Facts {
            checks: Some(CheckFacts { passed: 3, total: 5, failed: vec!["lint".to_string(), "unit".to_string()] }),
            ..facts_of(vec![row("src/cart.ts", 1, 0)])
        };
        let found = risk(&facts, &roles);
        assert_eq!(found.tier, 3);
        assert_eq!(found.level, Level::Check);
        assert_eq!(sentences(&found), ["Checks failed: 3 of 5 passed (lint, unit)"]);
    }

    #[test]
    fn tier4_tests_changed() {
        let facts = facts_of(vec![
            row("src/cart.test.ts", 1, 0),
            row("src/money.test.ts", 1, 0),
            row("jest.config.js", 1, 0),
            row("src/app.ts", 1, 0),
        ]);
        let found = risk(&facts, &roles);
        assert_eq!(found.tier, 4);
        assert_eq!(found.level, Level::Check);
        assert_eq!(
            sentences(&found),
            ["Test files changed: src/cart.test.ts, src/money.test.ts", "Check settings changed: jest.config.js"]
        );
    }

    #[test]
    fn tier5_packages_changed() {
        let found = risk(&facts_of(vec![row("package-lock.json", 40, 2), row("Cargo.toml", 1, 0)]), &roles);
        assert_eq!(found.tier, 5);
        assert_eq!(found.level, Level::Check);
        assert_eq!(sentences(&found), ["A package list changed: package-lock.json, Cargo.toml"]);
    }

    #[test]
    fn tier6_big_change() {
        let facts = Facts { files_total: 31, added: 1_204, deleted: 88, ..facts_of(vec![row("src/a.ts", 1, 0)]) };
        let found = risk(&facts, &roles);
        assert_eq!(found.tier, 6);
        assert_eq!(found.level, Level::Check);
        assert_eq!(sentences(&found), ["Big change: 31 files, +1,204 \u{2212}88"]);
    }

    #[test]
    fn tier7_no_flags() {
        let found = risk(&facts_of(vec![row("src/util.ts", 1, 0)]), &roles);
        assert_eq!(found.tier, 7);
        assert_eq!(found.level, Level::Low);
        assert!(found.reasons.is_empty());
        assert_eq!(level_word(found.level), "Low risk");
    }

    #[test]
    fn unknown_when_files_none() {
        let facts = Facts { files: None, lines_scanned: false, ..Facts::default() };
        let found = risk(&facts, &roles);
        assert_eq!(found.tier, 6);
        assert_eq!(found.level, Level::Check);
        assert_eq!(found.reasons[0].flag, Flag::Unknown);
        assert_eq!(sentences(&found), ["Puppet Master could not see which files changed"]);
    }

    #[test]
    fn tie_break_more_reasons_first() {
        // Both tier 4. The first has two reasons and is small; the second has one reason and is big.
        let two = item(
            "copy:b-two",
            facts_of(vec![row("src/a.test.ts", 1, 0), row("jest.config.js", 1, 0)]),
            500,
        );
        let one = item("copy:a-one", facts_of(vec![row("src/x.test.ts", 200, 100)]), 100);
        let mut items = vec![one, two];
        order(&mut items);
        assert_eq!(items[0].key.as_str(), "copy:b-two");
    }

    #[test]
    fn tie_break_bigger_first() {
        let small = item("copy:small", facts_of(vec![row("src/y.test.ts", 1, 1)]), 100);
        let big = item("copy:big", facts_of(vec![row("src/x.test.ts", 200, 100)]), 500);
        let mut items = vec![small, big];
        order(&mut items);
        assert_eq!(items[0].key.as_str(), "copy:big");
    }

    #[test]
    fn tie_break_older_first() {
        let newer = item("copy:a", facts_of(vec![row("src/y.test.ts", 1, 1)]), 200);
        let older = item("copy:b", facts_of(vec![row("src/x.test.ts", 1, 1)]), 100);
        let mut items = vec![newer, older];
        order(&mut items);
        assert_eq!(items[0].key.as_str(), "copy:b");
    }

    #[test]
    fn tie_break_key_last() {
        let later = item("copy:b", facts_of(vec![row("src/x.test.ts", 1, 1)]), 100);
        let first = item("copy:a", facts_of(vec![row("src/y.test.ts", 1, 1)]), 100);
        let mut items = vec![later, first];
        order(&mut items);
        assert_eq!(items[0].key.as_str(), "copy:a");
    }

    #[test]
    fn package_json_counts_as_packages_not_checks() {
        // package.json has the Config role, but it is a package list: never "Check settings changed".
        let found = risk(&facts_of(vec![row("package.json", 2, 0)]), &roles);
        assert_eq!(found.tier, 5);
        assert!(found.reasons.iter().all(|reason| reason.flag != Flag::TestsChanged));
        assert_eq!(sentences(&found), ["A package list changed: package.json"]);
    }

    #[test]
    fn jest_config_counts_as_checks() {
        let found = risk(&facts_of(vec![row("jest.config.js", 2, 0)]), &roles);
        assert_eq!(found.tier, 4);
        assert!(found.reasons.iter().all(|reason| reason.flag != Flag::PackagesChanged));
        assert_eq!(sentences(&found), ["Check settings changed: jest.config.js"]);
    }

    #[test]
    fn names_cut_at_three_then_and_n_more() {
        let files = vec![
            row("src/a.test.ts", 1, 0),
            row("src/b.test.ts", 1, 0),
            row("src/c.test.ts", 1, 0),
            row("src/d.test.ts", 1, 0),
            row("src/e.test.ts", 1, 0),
        ];
        let found = risk(&facts_of(files), &roles);
        assert_eq!(sentences(&found), ["Test files changed: src/a.test.ts, src/b.test.ts, src/c.test.ts and 2 more"]);
    }

    #[test]
    fn fingerprint_changes_with_file_row_not_title() {
        let key = ItemKey::parse("copy:a").expect("valid key");
        let before = facts_of(vec![row("src/x.ts", 1, 0)]);
        let after = facts_of(vec![row("src/x.ts", 2, 0)]);
        assert_ne!(fingerprint(&key, &before), fingerprint(&key, &after));
        assert_eq!(fingerprint(&key, &before).len(), 12);

        let first = item("copy:a", before.clone(), 100);
        let mut renamed = item("copy:a", before, 100);
        renamed.title = "Another title".to_string();
        assert_eq!(first.fingerprint, renamed.fingerprint);
    }

    #[test]
    fn level_word_is_plain_text() {
        assert_eq!(level_word(Level::High), "High risk");
        assert_eq!(level_word(Level::Check), "Check first");
        assert_eq!(level_word(Level::Low), "Low risk");
    }

    #[test]
    fn order_caps_at_items_cap() {
        let mut items: Vec<Item> =
            (0..=ITEMS_CAP).map(|i| item(&format!("copy:k{i}"), facts_of(vec![row("src/x.ts", 1, 0)]), i as u64)).collect();
        order(&mut items);
        assert_eq!(items.len(), ITEMS_CAP);
    }

    #[test]
    fn risk_module_is_pure_and_imports_only_model_and_role() {
        // Only the code before the test module counts.
        let src = include_str!("risk.rs");
        let code = src.split("\n#[cfg(test)]").next().unwrap_or(src);
        for banned in ["std::fs", "std::process", "reqwest", "agent_runs", "second_opinion"] {
            assert!(!code.contains(banned), "risk.rs must not use {banned}");
        }
        for line in code.lines().filter(|line| line.starts_with("use ")) {
            assert!(
                line.starts_with("use super::model::") || line == "use crate::tamper::Role;",
                "unexpected import in risk.rs: {line}"
            );
        }
    }
}
