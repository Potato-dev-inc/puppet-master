//! Picks the card's facts for each package from the lookup result (SPEC-n33 sections 3.10 and 3.11 B):
//! one row per package, and the view for the whole card. Sentences come from `prompt_risk/packages/words.rs`.
//!
//! Pure: no network, no disk, no screen. The caller passes the clock (`now_ms`). Network facts only add
//! lines here; they never change the risk level, the buttons or the focus (SPEC-n33 S8).

use super::model::{
    page_url, CacheHit, Downloads, Ecosystem, FactLine, FetchError, Facts, Outcome, PackageCheckView,
    PackageItem, PackageList, PackageRow, RowState, Source, VersionFacts, FEW_TOTAL, FEW_WEEKLY,
    FRESH_MIN_AGE_DAYS, FRESH_VERSION_HOURS, NEW_DAYS,
};
use crate::prompt_risk::packages::{popular, words};

const HOUR_MS: u64 = 3_600_000;
const DAY_MS: u64 = 24 * HOUR_MS;
/// A status younger than this reads "Checked just now".
const JUST_NOW_MS: u64 = 60_000;
/// A row summary shows at most this many segments.
const SUMMARY_SEGMENTS: usize = 3;
const SEPARATOR: &str = " · ";

/// The version a card line talks about: the exact version the command names, else an npm tag, else the
/// newest one. The flag is true only for an exact version ("pinned"); a tag or the newest is not a pin.
pub fn choose_version<'a>(f: &'a Facts, spec: Option<&str>) -> (Option<&'a VersionFacts>, bool) {
    if let Some(spec) = spec {
        if let Some(v) = f.versions.get(spec) {
            return (Some(v), true);
        }
        if let Some(v) = f.tags.get(spec).and_then(|version| f.versions.get(version)) {
            return (Some(v), false);
        }
    }
    (f.newest.as_deref().and_then(|newest| f.versions.get(newest)), false)
}

/// One row of the card for one package. `hit` is the lookup's outcome (None counts as a timeout).
/// `own_server` and `off` skip the lookup; the offline facts (look-alike, link) stay in every case.
pub fn row(item: &PackageItem, hit: Option<&CacheHit>, own_server: bool, off: bool, now_ms: u64) -> PackageRow {
    let look = offline_look(item);
    if !item.lookup {
        return make_row(item, RowState::NotChecked, look, not_checked_sentence(item), Vec::new(), Vec::new());
    }
    if off {
        return make_row(item, RowState::NotChecked, look, Some(words::LOOKUPS_OFF.to_string()), Vec::new(), Vec::new());
    }
    if own_server {
        return make_row(item, RowState::NotChecked, look, Some(words::OWN_SERVER.to_string()), Vec::new(), Vec::new());
    }
    match hit.map(|h| &h.outcome) {
        Some(Outcome::Found(facts)) => found_row(item, facts, now_ms, look),
        Some(Outcome::NotFound) => {
            let mut look = look;
            look.push(words::not_found_line(item.ecosystem.registry_label()));
            make_row(item, RowState::NotFound, look, None, Vec::new(), Vec::new())
        }
        Some(Outcome::Failed(err)) => failed_row(item, *err, look),
        // A looked-up package with no answer at all is treated like one that ran past the deadline.
        None => failed_row(item, FetchError::Timeout, look),
    }
}

/// The row for a package past `MAX_LOOKUPS` in one card: it is not looked up. The offline facts stay.
pub fn too_many_row(item: &PackageItem) -> PackageRow {
    make_row(
        item,
        RowState::NotChecked,
        offline_look(item),
        Some(words::TOO_MANY_PACKAGES.to_string()),
        Vec::new(),
        Vec::new(),
    )
}

/// The card for the whole prompt: status, footer and the retry flag. `rows` are the rows for every
/// package in `list` (the caller builds them with `row`). `oldest_fetch_ms` is the oldest cache entry used.
pub fn view(
    prompt_id: &str,
    list: &PackageList,
    rows: Vec<PackageRow>,
    off: bool,
    oldest_fetch_ms: Option<u64>,
    now_ms: u64,
) -> PackageCheckView {
    let answered = rows.iter().filter(|r| matches!(r.state, RowState::Checked | RowState::NotFound)).count();
    let failed = rows.iter().filter(|r| r.state == RowState::Failed).count();
    let status_text = if off {
        words::STATUS_OFF.to_string()
    } else if failed > 0 && answered == 0 {
        words::STATUS_COULD_NOT.to_string()
    } else if failed > 0 {
        words::STATUS_PARTLY.to_string()
    } else if answered > 0 {
        checked_status(oldest_fetch_ms, now_ms)
    } else {
        String::new()
    };

    // The registries that were asked, each name once, in row order.
    let mut labels: Vec<String> = Vec::new();
    for r in rows.iter().filter(|r| r.state != RowState::NotChecked) {
        if !labels.contains(&r.registry_label) {
            labels.push(r.registry_label.clone());
        }
    }
    let mut footer_lines: Vec<String> = Vec::new();
    if off {
        footer_lines.push(words::FOOTER_OFF.to_string());
    } else if !labels.is_empty() {
        footer_lines.push(words::footer(&join_names(&labels)));
    }
    if list.more > 0 {
        footer_lines.push(words::more_footer(list.more));
    }
    if list.hidden > 0 {
        footer_lines.push(words::hidden_footer(list.hidden));
    }

    PackageCheckView {
        prompt_id: prompt_id.to_string(),
        off,
        status_text,
        look: rows.iter().filter(|r| r.look).count() as u32,
        can_retry: rows.iter().any(|r| matches!(r.state, RowState::Failed | RowState::NotFound)),
        rows,
        more: list.more,
        hidden: list.hidden,
        footer: footer_lines.join("\n"),
    }
}

/// Look closer facts from the offline check (L1, then L3), in table order.
fn offline_look(item: &PackageItem) -> Vec<String> {
    let mut look = Vec::new();
    if let Some(lookalike) = &item.lookalike {
        look.push(words::lookalike_line(&lookalike.popular, lookalike.how));
    }
    if let Source::Link { host } = &item.source {
        look.push(words::link_line(item.ecosystem.registry_label(), host));
    }
    look
}

/// The one sentence for a row that is not looked up. Mirrors the not-checked rules of `words::first_line`.
fn not_checked_sentence(item: &PackageItem) -> Option<String> {
    match &item.source {
        Source::Local => Some(words::LOCAL.to_string()),
        Source::ListFile { file } => Some(words::list_file_line(file)),
        Source::NotPlain => Some(words::NOT_PLAIN.to_string()),
        Source::PrivateServer => Some(words::OWN_SERVER.to_string()),
        Source::Link { .. } => item.cut.then(|| words::CUT.to_string()),
        Source::Registry if item.cut => Some(words::CUT.to_string()),
        Source::Registry if !item.ecosystem.looks_up() => Some(words::unsupported_line(&item.tool)),
        Source::Registry => None,
    }
}

/// A row for a package whose lookup answered with facts (L2 never applies here; L4 to L8, P1 to P6).
fn found_row(item: &PackageItem, f: &Facts, now_ms: u64, mut look: Vec<String>) -> PackageRow {
    let mut plain: Vec<String> = Vec::new(); // P1 to P3, the ones a summary may show
    let mut extra: Vec<String> = Vec::new(); // P4 and P6 (details only)
    let (version, pinned) = choose_version(f, item.spec.as_deref());
    let created_age = f.created_ms.map(|created| now_ms.saturating_sub(created));

    // L4 "New" or P1 "First published": one of them, never both.
    if let Some(age_ms) = created_age {
        if age_ms < NEW_DAYS * DAY_MS {
            look.push(words::new_line(&words::age(age_ms)));
        } else {
            plain.push(words::first_published_line(&words::age(age_ms)));
        }
    }

    // L5: the chosen version is very new and the package is not.
    if let (Some(v), Some(age_ms)) = (version, created_age) {
        if let Some(published) = v.published_ms {
            let version_age = now_ms.saturating_sub(published);
            if version_age < FRESH_VERSION_HOURS * HOUR_MS && age_ms >= FRESH_MIN_AGE_DAYS * DAY_MS {
                let ago = words::age(version_age);
                look.push(if pinned { words::fresh_pinned_line(&ago) } else { words::fresh_version_line(&ago) });
            }
        }
    }

    // L6 "Few downloads" or P2 the download count.
    match &f.downloads {
        Some(Downloads::Weekly(n)) if *n < FEW_WEEKLY => look.push(words::few_weekly_line(&words::count(*n))),
        Some(Downloads::Weekly(n)) => plain.push(words::weekly_line(&words::count(*n))),
        Some(Downloads::AboutWeekly(n)) if *n < FEW_WEEKLY => {
            look.push(words::few_weekly_about_line(&words::count(*n)))
        }
        Some(Downloads::AboutWeekly(n)) => plain.push(words::about_weekly_line(&words::count(*n))),
        Some(Downloads::Total(n)) if *n < FEW_TOTAL => look.push(words::few_total_line(&words::count(*n))),
        Some(Downloads::Total(n)) => plain.push(words::total_line(&words::count(*n))),
        None => {}
    }

    // L7 and L8 come from the chosen version.
    if let Some(v) = version {
        if f.ecosystem == Ecosystem::Npm && v.install_script == Some(true) {
            look.push(words::install_script_line().to_string());
        }
        if f.ecosystem == Ecosystem::Pypi && v.prebuilt == Some(false) {
            look.push(words::no_wheel_line().to_string());
        }
        if v.deprecated {
            look.push(words::DEPRECATED.to_string());
        }
        if v.yanked {
            look.push(words::YANKED.to_string());
        }
    }

    // P3 maintainers, P4 no source link, P6 answer too large.
    if let Some(n) = f.maintainers {
        plain.push(words::maintainers_line(n));
    }
    if f.source_link == Some(false) {
        extra.push(words::NO_SOURCE_LINK.to_string());
    }
    if f.big {
        extra.push(words::TOO_BIG.to_string());
    }

    make_row(item, RowState::Checked, look, None, plain, extra)
}

/// A row whose lookup failed: the one failure sentence, and P5 "Well known" (details only).
fn failed_row(item: &PackageItem, err: FetchError, look: Vec<String>) -> PackageRow {
    let mut extra: Vec<String> = Vec::new();
    if item.popular {
        extra.push(words::well_known_line(popular::size(item.ecosystem), item.ecosystem.registry_label()));
    }
    let sentence = failure_sentence(err, item.ecosystem.registry_label());
    make_row(item, RowState::Failed, look, Some(sentence), Vec::new(), extra)
}

fn failure_sentence(err: FetchError, registry: &str) -> String {
    match err {
        FetchError::Unreachable | FetchError::NotAllowed => words::unreachable_line(registry),
        FetchError::Timeout => words::timeout_line(registry),
        FetchError::TooMany => words::TOO_MANY_LOOKUPS.to_string(),
        FetchError::Unreadable | FetchError::TooBig => words::unreadable_line(registry),
        FetchError::Refused => words::refused_line(registry),
    }
}

/// Builds the row. `sentence` is the one sentence of a row that is not checked or failed; the summary is then
/// the Look closer facts plus that sentence. Otherwise the summary is the Look closer facts plus `plain`.
/// Details (`lines`) hold everything: Look closer facts, the sentence, then the plain facts.
fn make_row(
    item: &PackageItem,
    state: RowState,
    look: Vec<String>,
    sentence: Option<String>,
    plain: Vec<String>,
    extra: Vec<String>,
) -> PackageRow {
    let segments: Vec<String> = match &sentence {
        Some(one) => look.iter().cloned().chain(std::iter::once(one.clone())).collect(),
        None => look.iter().cloned().chain(plain.iter().cloned()).collect(),
    };
    let summary = segments.into_iter().take(SUMMARY_SEGMENTS).collect::<Vec<_>>().join(SEPARATOR);

    let mut lines: Vec<FactLine> = look.into_iter().map(|text| FactLine { look: true, text }).collect();
    lines.extend(sentence.into_iter().chain(plain).chain(extra).map(|text| FactLine { look: false, text }));

    PackageRow {
        name: item.name.clone(),
        ecosystem: item.ecosystem,
        registry_label: item.ecosystem.registry_label().to_string(),
        state,
        look: lines.iter().any(|l| l.look),
        summary,
        lines,
        page_url: page_link(item),
    }
}

/// The "Open on ..." link: only for a plain registry name that is not cut.
fn page_link(item: &PackageItem) -> Option<String> {
    match &item.source {
        Source::Registry if !item.cut => page_url(item.ecosystem, &item.name),
        _ => None,
    }
}

/// "Checked just now", or "Checked 2 hours ago" for the oldest entry used.
fn checked_status(oldest_fetch_ms: Option<u64>, now_ms: u64) -> String {
    let ago = oldest_fetch_ms.map(|t| now_ms.saturating_sub(t)).unwrap_or(0);
    if ago < JUST_NOW_MS {
        words::STATUS_JUST_NOW.to_string()
    } else {
        words::status_checked_ago(&words::age(ago))
    }
}

/// "npm", "npm and PyPI", "npm, PyPI and crates.io".
fn join_names(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt_risk::packages::model::{How, Lookalike};
    use std::collections::BTreeMap;

    const NOW: u64 = 1_800_000_000_000;
    const HOUR: u64 = 3_600_000;
    const DAY: u64 = 24 * HOUR;

    /// A plain npm package that is looked up.
    fn npm(name: &str) -> PackageItem {
        PackageItem {
            ecosystem: Ecosystem::Npm,
            tool: "npm".into(),
            name: name.into(),
            spec: None,
            source: Source::Registry,
            global: false,
            runs_now: false,
            lookalike: None,
            popular: false,
            cut: false,
            lookup: true,
            first: String::new(),
            look: false,
        }
    }

    fn hit(outcome: Outcome) -> CacheHit {
        CacheHit { outcome, fetched_ms: NOW }
    }

    fn facts(created_ms: Option<u64>) -> Facts {
        Facts {
            ecosystem: Ecosystem::Npm,
            created_ms,
            newest: None,
            tags: BTreeMap::new(),
            versions: BTreeMap::new(),
            downloads: None,
            maintainers: None,
            source_link: None,
            big: false,
        }
    }

    /// Facts for a package created `created_days` ago, with one version "1.0.0" published `version_age_ms` ago.
    fn settled(created_days: u64, version_age_ms: u64) -> Facts {
        let mut f = facts(Some(NOW - created_days * DAY));
        f.newest = Some("1.0.0".into());
        f.versions.insert(
            "1.0.0".into(),
            VersionFacts { published_ms: Some(NOW - version_age_ms), ..VersionFacts::default() },
        );
        f
    }

    /// The row for `item` when its lookup found `f`.
    fn checked(item: &PackageItem, f: Facts) -> PackageRow {
        row(item, Some(&hit(Outcome::Found(f))), false, false, NOW)
    }

    fn texts(r: &PackageRow) -> Vec<String> {
        r.lines.iter().map(|l| l.text.clone()).collect()
    }

    fn has(r: &PackageRow, text: &str) -> bool {
        r.lines.iter().any(|l| l.text == text)
    }

    fn list(more: u32, hidden: u32) -> PackageList {
        PackageList { items: Vec::new(), more, hidden }
    }

    #[test]
    fn choose_version_exact_then_tag_then_newest() {
        let mut f = facts(None);
        f.versions.insert("1.0.0".into(), VersionFacts { published_ms: Some(1), ..VersionFacts::default() });
        f.versions.insert("2.0.0".into(), VersionFacts { published_ms: Some(2), ..VersionFacts::default() });
        f.tags.insert("next".into(), "1.0.0".into());
        f.newest = Some("2.0.0".into());
        let pick = |spec: Option<&str>| {
            let (v, pinned) = choose_version(&f, spec);
            (v.and_then(|v| v.published_ms), pinned)
        };
        assert_eq!(pick(Some("1.0.0")), (Some(1), true));
        assert_eq!(pick(Some("next")), (Some(1), false));
        assert_eq!(pick(Some("^1")), (Some(2), false));
        assert_eq!(pick(None), (Some(2), false));
        assert_eq!(choose_version(&facts(None), None), (None, false));
    }

    #[test]
    fn l1_and_l3_come_first_and_l2_is_not_found() {
        let lookalike = PackageItem {
            lookalike: Some(Lookalike { popular: "lodash".into(), how: How::Swapped }),
            ..npm("lodahs")
        };
        let r = row(&lookalike, Some(&hit(Outcome::NotFound)), false, false, NOW);
        assert_eq!(r.state, RowState::NotFound);
        assert!(r.look);
        assert_eq!(
            r.summary,
            "Looks like lodash (2 letters swapped) · Not found on npm. The name may be made up."
        );

        let link = PackageItem { source: Source::Link { host: "github.com".into() }, lookup: false, ..npm("x") };
        let r = row(&link, None, false, false, NOW);
        assert_eq!(r.state, RowState::NotChecked);
        assert!(r.look);
        assert_eq!(r.summary, "Installs from a link, not from npm: github.com");
    }

    #[test]
    fn l2_alone_when_not_found() {
        let r = row(&npm("lodahs"), Some(&hit(Outcome::NotFound)), false, false, NOW);
        assert_eq!(r.summary, "Not found on npm. The name may be made up.");
        assert!(r.look);
    }

    #[test]
    fn l4_new_package_edge_29_vs_30_days() {
        let r = checked(&npm("fresh-pkg"), settled(29, 29 * DAY));
        assert!(has(&r, "New: first published 29 days ago"));
        assert!(!has(&r, "First published 29 days ago"));
        assert!(r.look);

        let r = checked(&npm("older-pkg"), settled(30, 30 * DAY));
        assert!(!r.lines.iter().any(|l| l.text.starts_with("New:")));
        assert!(has(&r, "First published 30 days ago"));
        assert!(!r.look);
    }

    #[test]
    fn l5_fresh_version_edges_23_vs_24_hours_and_89_vs_90_days() {
        let fresh = |created_days: u64, version_ms: u64| {
            let r = checked(&npm("lodash"), settled(created_days, version_ms));
            r.lines.iter().any(|l| l.text.starts_with("Newest version came out"))
        };
        assert!(fresh(400, 23 * HOUR));
        assert!(!fresh(400, 24 * HOUR));
        assert!(!fresh(89, 2 * HOUR));
        assert!(fresh(90, 2 * HOUR));

        let r = checked(&npm("lodash"), settled(400, 2 * HOUR));
        assert!(has(&r, "Newest version came out 2 hours ago"));

        let pinned = PackageItem { spec: Some("1.0.0".into()), ..npm("lodash") };
        let r = checked(&pinned, settled(400, 2 * HOUR));
        assert!(has(&r, "This version came out 2 hours ago"));
        assert!(!r.lines.iter().any(|l| l.text.starts_with("Newest version")));
    }

    #[test]
    fn l6_weekly_edge_999_vs_1000() {
        let weekly = |n: u64| {
            let mut f = settled(400, 400 * DAY);
            f.downloads = Some(Downloads::Weekly(n));
            checked(&npm("lodash"), f)
        };
        let few = weekly(999);
        assert!(has(&few, "Few downloads: 999 a week"));
        assert!(few.look);
        let many = weekly(1_000);
        assert!(has(&many, "1,000 downloads a week"));
        assert!(!many.look);
        assert!(!texts(&many).iter().any(|t| t.starts_with("Few downloads")));
    }

    #[test]
    fn l6_about_weekly_for_crates_edge_999_vs_1000() {
        let about = |n: u64| {
            let mut f = settled(400, 400 * DAY);
            f.downloads = Some(Downloads::AboutWeekly(n));
            checked(&PackageItem { ecosystem: Ecosystem::Crates, tool: "cargo".into(), ..npm("serde") }, f)
        };
        assert!(has(&about(999), "Few downloads: about 999 a week"));
        assert!(has(&about(1_000), "About 1,000 downloads a week"));
    }

    #[test]
    fn l6_total_for_gems_edge_9999_vs_10000() {
        let gem = |n: u64| {
            let mut f = settled(400, 400 * DAY);
            f.downloads = Some(Downloads::Total(n));
            let item = PackageItem { ecosystem: Ecosystem::Gems, tool: "gem".into(), ..npm("rails") };
            checked(&item, f)
        };
        assert!(has(&gem(9_999), "Few downloads: 9,999 in total"));
        assert!(has(&gem(10_000), "10K downloads in total"));
    }

    #[test]
    fn l7_install_script_and_missing_wheel() {
        let mut f = settled(400, 400 * DAY);
        f.versions.get_mut("1.0.0").unwrap().install_script = Some(true);
        assert!(has(&checked(&npm("esbuild-like"), f), "Runs a script when installed"));

        let pypi = |prebuilt: Option<bool>| {
            let mut f = settled(400, 400 * DAY);
            f.ecosystem = Ecosystem::Pypi;
            f.versions.get_mut("1.0.0").unwrap().prebuilt = prebuilt;
            checked(&PackageItem { ecosystem: Ecosystem::Pypi, tool: "pip".into(), ..npm("requests") }, f)
        };
        assert!(has(&pypi(Some(false)), "No ready-built files: installing runs its setup code"));
        assert!(!pypi(Some(true)).look);
    }

    #[test]
    fn l8_deprecated_and_yanked() {
        let mut f = settled(400, 400 * DAY);
        f.versions.get_mut("1.0.0").unwrap().deprecated = true;
        assert!(has(&checked(&npm("old-lib"), f), "Its authors marked it as deprecated"));

        let mut f = settled(400, 400 * DAY);
        f.ecosystem = Ecosystem::Pypi;
        f.versions.get_mut("1.0.0").unwrap().yanked = true;
        let item = PackageItem { ecosystem: Ecosystem::Pypi, tool: "pip".into(), ..npm("some-lib") };
        assert!(has(&checked(&item, f), "This version was pulled back by its authors"));
    }

    #[test]
    fn p3_maintainers_one_and_many() {
        let mut f = settled(400, 400 * DAY);
        f.maintainers = Some(1);
        let r = checked(&npm("a"), f);
        assert!(has(&r, "1 maintainer"));
        assert!(!r.look);

        let mut f = settled(400, 400 * DAY);
        f.maintainers = Some(3);
        assert!(has(&checked(&npm("a"), f), "3 maintainers"));
    }

    #[test]
    fn p4_no_source_link_and_p6_answer_too_big() {
        let mut f = settled(400, 400 * DAY);
        f.source_link = Some(false);
        assert!(has(&checked(&npm("a"), f), "No link to its source code"));

        let mut f = settled(400, 400 * DAY);
        f.source_link = Some(true);
        assert!(!has(&checked(&npm("a"), f), "No link to its source code"));

        let mut f = settled(400, 400 * DAY);
        f.big = true;
        assert!(has(&checked(&npm("a"), f), "Some facts are missing: the registry's answer was too large"));
    }

    #[test]
    fn p5_well_known_only_after_a_failed_lookup() {
        let popular_item = PackageItem { popular: true, ..npm("lodash") };
        let good_row = checked(&popular_item, settled(400, 400 * DAY));
        assert!(!texts(&good_row).iter().any(|t| t.starts_with("Well known")));

        let failed = row(&popular_item, Some(&hit(Outcome::Failed(FetchError::Timeout))), false, false, NOW);
        let well = texts(&failed).into_iter().find(|t| t.starts_with("Well known")).expect("P5 line");
        assert!(well.starts_with("Well known: one of the ") && well.ends_with(" most used npm packages"));
        // Details only: the summary keeps the one failure sentence.
        assert_eq!(failed.summary, words::timeout_line("npm"));
    }

    #[test]
    fn not_checked_rows_keep_their_sentence() {
        let off = row(&npm("lodash"), Some(&hit(Outcome::Found(settled(400, 400 * DAY)))), false, true, NOW);
        assert_eq!(off.state, RowState::NotChecked);
        assert_eq!(off.summary, words::LOOKUPS_OFF);

        let own = row(&npm("acme-tools"), Some(&hit(Outcome::Found(settled(400, 400 * DAY)))), true, false, NOW);
        assert_eq!(own.state, RowState::NotChecked);
        assert_eq!(own.summary, words::OWN_SERVER);

        let lookalike = PackageItem {
            lookalike: Some(Lookalike { popular: "lodash".into(), how: How::Swapped }),
            ..npm("lodahs")
        };
        let r = row(&lookalike, None, false, true, NOW);
        assert_eq!(r.summary, "Looks like lodash (2 letters swapped) · Not checked: package lookups are off");

        // Items that are never looked up keep their own sentence, even with lookups off.
        let local = PackageItem { source: Source::Local, lookup: false, ..npm("./x") };
        let r = row(&local, None, false, true, NOW);
        assert_eq!(r.summary, words::LOCAL);
        assert_eq!(r.page_url, None);

        let list_file = PackageItem {
            source: Source::ListFile { file: "requirements.txt".into() },
            lookup: false,
            ..npm("x")
        };
        let r = row(&list_file, None, false, false, NOW);
        assert_eq!(r.summary, "Installs the packages listed in requirements.txt. The list was not checked.");

        let not_plain = PackageItem { source: Source::NotPlain, lookup: false, ..npm("$PKG") };
        assert_eq!(row(&not_plain, None, false, false, NOW).summary, words::NOT_PLAIN);

        let private = PackageItem { source: Source::PrivateServer, lookup: false, ..npm("x") };
        assert_eq!(row(&private, None, false, false, NOW).summary, words::OWN_SERVER);

        let cut = PackageItem { cut: true, lookup: false, ..npm("lodash") };
        assert_eq!(row(&cut, None, false, false, NOW).summary, words::CUT);

        let winget = PackageItem { ecosystem: Ecosystem::Winget, tool: "winget".into(), lookup: false, ..npm("Git.Git") };
        let r = row(&winget, None, false, false, NOW);
        assert_eq!(r.summary, "Not checked: Puppet Master can't look up winget packages yet");
        assert_eq!(r.page_url, None);
    }

    #[test]
    fn too_many_row_is_not_checked() {
        let r = too_many_row(&npm("lodash"));
        assert_eq!(r.state, RowState::NotChecked);
        assert_eq!(r.summary, "Not checked: too many packages in one command");
        assert_eq!(texts(&r), vec!["Not checked: too many packages in one command".to_string()]);
    }

    #[test]
    fn failed_lookups_say_why() {
        let cases = [
            (FetchError::Unreachable, "Could not check: npm could not be reached"),
            (FetchError::NotAllowed, "Could not check: npm could not be reached"),
            (FetchError::Timeout, "Could not check: npm did not answer in time"),
            (FetchError::TooMany, "Could not check: too many lookups right now. Try again in a minute."),
            (FetchError::Unreadable, "Could not check: npm sent an answer Puppet Master can't read"),
            (FetchError::TooBig, "Could not check: npm sent an answer Puppet Master can't read"),
            (FetchError::Refused, "Could not check: npm refused the request"),
        ];
        for (err, text) in cases {
            let r = row(&npm("lodash"), Some(&hit(Outcome::Failed(err))), false, false, NOW);
            assert_eq!(r.state, RowState::Failed, "{err:?}");
            assert_eq!(r.summary, text, "{err:?}");
        }
        // No answer at all counts as a timeout.
        let r = row(&npm("lodash"), None, false, false, NOW);
        assert_eq!(r.state, RowState::Failed);
        assert_eq!(r.summary, "Could not check: npm did not answer in time");
    }

    #[test]
    fn summary_takes_look_facts_first_and_stops_at_three() {
        let lookalike = PackageItem {
            lookalike: Some(Lookalike { popular: "lodash".into(), how: How::Swapped }),
            ..npm("lodahs")
        };
        let mut f = settled(400, 400 * DAY);
        f.downloads = Some(Downloads::Weekly(500));
        f.versions.get_mut("1.0.0").unwrap().install_script = Some(true);
        f.versions.get_mut("1.0.0").unwrap().deprecated = true;
        let r = checked(&lookalike, f);
        // Details keep all five: L1, L6, L7, L8 and P1. The summary shows the first three.
        assert_eq!(r.lines.len(), 5);
        assert_eq!(
            r.summary,
            "Looks like lodash (2 letters swapped) · Few downloads: 500 a week · Runs a script when installed"
        );
    }

    #[test]
    fn summary_plain_facts_in_order_then_cap() {
        let mut f = settled(400, 400 * DAY);
        f.downloads = Some(Downloads::Weekly(5_000));
        f.maintainers = Some(2);
        f.source_link = Some(false);
        let r = checked(&npm("left-pad"), f);
        assert_eq!(r.summary, "First published 13 months ago · 5,000 downloads a week · 2 maintainers");
        assert_eq!(r.lines.len(), 4);
        assert!(has(&r, "No link to its source code"));
        assert!(!r.look);
    }

    #[test]
    fn page_url_only_for_a_plain_registry_name() {
        let r = row(&npm("lodash"), Some(&hit(Outcome::NotFound)), false, false, NOW);
        assert_eq!(r.page_url.as_deref(), Some("https://www.npmjs.com/package/lodash"));
        let cut = PackageItem { cut: true, lookup: false, ..npm("lodash") };
        assert_eq!(row(&cut, None, false, false, NOW).page_url, None);
    }

    #[test]
    fn view_header_counts_look_rows() {
        let plain = checked(&npm("left-pad"), settled(400, 400 * DAY));
        let lookalike = PackageItem {
            lookalike: Some(Lookalike { popular: "lodash".into(), how: How::Swapped }),
            ..npm("lodahs")
        };
        let look = row(&lookalike, Some(&hit(Outcome::NotFound)), false, false, NOW);
        let local = PackageItem { source: Source::Local, lookup: false, ..npm("./x") };
        let other = row(&local, None, false, false, NOW);
        let v = view("p1", &list(0, 0), vec![plain, look, other], false, Some(NOW), NOW);
        assert_eq!(v.look, 1);
        assert_eq!(words::look_count(v.look, v.rows.len() as u32), "Look closer · 1 of 3");
    }

    #[test]
    fn view_status_text_cases() {
        let good_row = checked(&npm("left-pad"), settled(400, 400 * DAY));
        let failed = row(&npm("x"), Some(&hit(Outcome::Failed(FetchError::Timeout))), false, false, NOW);
        let status = |rows: Vec<PackageRow>, off: bool, oldest: Option<u64>| {
            view("p", &list(0, 0), rows, off, oldest, NOW).status_text
        };
        assert_eq!(status(vec![good_row.clone()], false, Some(NOW - 10_000)), "Checked just now");
        assert_eq!(status(vec![good_row.clone()], false, Some(NOW - 2 * HOUR)), "Checked 2 hours ago");
        assert_eq!(status(vec![good_row.clone(), failed.clone()], false, Some(NOW)), "Partly checked");
        assert_eq!(status(vec![failed], false, Some(NOW)), "Could not check");
        assert_eq!(status(vec![good_row], true, Some(NOW)), "Lookups are off");
    }

    #[test]
    fn view_footer_lists_each_registry_once_and_the_extra_lines() {
        let npm_ok = checked(&npm("left-pad"), settled(400, 400 * DAY));
        let npm_missing = row(&npm("lodahs"), Some(&hit(Outcome::NotFound)), false, false, NOW);
        let pypi_item = PackageItem { ecosystem: Ecosystem::Pypi, tool: "pip".into(), ..npm("requests") };
        let pypi = row(&pypi_item, Some(&hit(Outcome::Found(settled(400, 400 * DAY)))), false, false, NOW);
        let local = PackageItem { source: Source::Local, lookup: false, ..npm("./x") };
        let other = row(&local, None, false, false, NOW);
        let v = view("p1", &list(1, 2), vec![npm_ok, npm_missing, pypi, other], false, Some(NOW), NOW);
        assert_eq!(
            v.footer,
            "Facts from the public npm and PyPI registry. Only the package names were sent. Puppet Master never answers for you.\n1 more package was not checked\n2 names looked like a secret, so they were not shown or checked"
        );
        assert_eq!((v.more, v.hidden), (1, 2));
        assert_eq!(v.prompt_id, "p1");
    }

    #[test]
    fn view_footer_when_lookups_are_off() {
        let r = row(&npm("lodash"), None, false, true, NOW);
        let v = view("p", &list(0, 0), vec![r], true, None, NOW);
        assert!(v.off);
        assert_eq!(v.footer, words::FOOTER_OFF);
        assert!(!v.can_retry);
    }

    #[test]
    fn view_can_retry_when_a_row_failed_or_was_not_found() {
        let good_row = checked(&npm("left-pad"), settled(400, 400 * DAY));
        let v = view("p", &list(0, 0), vec![good_row.clone()], false, Some(NOW), NOW);
        assert!(!v.can_retry);
        let missing = row(&npm("lodahs"), Some(&hit(Outcome::NotFound)), false, false, NOW);
        assert!(view("p", &list(0, 0), vec![good_row.clone(), missing], false, Some(NOW), NOW).can_retry);
        let failed = row(&npm("x"), Some(&hit(Outcome::Failed(FetchError::Refused))), false, false, NOW);
        assert!(view("p", &list(0, 0), vec![good_row, failed], false, Some(NOW), NOW).can_retry);
    }
}
