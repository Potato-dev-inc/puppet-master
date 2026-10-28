//! The catalog check (SPEC 3.7): the rules every shipped Agent Store entry must pass.
//!
//! `check_catalog` takes the raw catalog files (`(path, text)`, as `embedded::FILES` lists them) and
//! returns every problem with the rule it breaks. Every rule is an error: an entry with a problem is
//! refused, never shipped with a warning, and Add refuses the same things again. Rule 10 is the one
//! warning: `warnings` reports it and nothing refuses on it. Rule 11 needs the test definitions
//! on disk, so it is `drift`. The only file this module writes is the temporary folder the fixtures
//! are checked in, which it removes again.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

use chrono::NaiveDate;
use regex::Regex;

use super::entry::{parse_entry, validate_entry, EntryFile, Status, Tier};
use crate::tui::conformance_rules;
use crate::tui::declarative::check::{check_dir, FixtureResult};
use crate::tui::declarative::loader::load_text;
use crate::tui::declarative::schema::{parse_definition, DefError, Definition};
use crate::tui::declarative::sources::Origin;
use crate::tui::declarative::validate::Source;
use crate::tui::declarative::wizard::bypass::hits_in_definition;
use crate::tui::declarative::DeclarativeProfile;

/// Rule 9: the largest fixture, the largest `entry.toml` and the largest whole catalog, in bytes.
const FIXTURE_MAX_BYTES: usize = 2 * 1024 * 1024;
const ENTRY_MAX_BYTES: usize = 4 * 1024 * 1024;
const CATALOG_MAX_BYTES: usize = 8 * 1024 * 1024;
/// Rule 10: a `checked.date` older than this many days is a warning.
const STALE_DAYS: i64 = 180;
/// Rule 6: the captures a verified definition needs, by stem (each also needs `<stem>.expect.toml`).
const TUI_CAPTURES: &[&str] = &["tui_idle", "tui_working", "tui_reply"];
const HEADLESS_CAPTURES: &[&str] = &["headless_success", "headless_error"];
/// Rule 8: user folder names the privacy scan accepts. The same list as `scripts/check-no-personal-paths.mjs`.
const ALLOWED_USER_NAMES: &[&str] = &[
    "you", "you-pc", "me", "runneradmin", "name", "username", "user", "yourname", "dev", "someone",
    "someoneelse", "ana", "anabel", "a", "x", "m", "test", "example", "public", "default",
];
/// Rule 8: local parts accepted in front of a consumer mail address (the same list as the node script).
const ALLOWED_MAIL_LOCALS: &[&str] = &["you", "me", "user", "example", "test", "noreply", "no-reply"];
/// Rule 8: a user folder name holding one of these is a placeholder such as `%USERNAME%` or `<name>`.
const PLACEHOLDER_CHARS: &str = "%$<>{}*[~";

/// One problem: the entry it is about (`""` for the whole catalog), the rule it breaks (SPEC 3.7)
/// and the message in plain words. A privacy message never repeats the personal value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckProblem {
    pub id: String,
    pub rule: u8,
    pub message: String,
}

fn problem(id: &str, rule: u8, message: impl Into<String>) -> CheckProblem {
    CheckProblem { id: id.to_string(), rule, message: message.into() }
}

/// The part of a path after `agents/<id>/`, or `None` for a file outside a folder.
fn inside_folder(path: &str) -> Option<&str> {
    path.strip_prefix("agents/")?.split_once('/').map(|(_, inside)| inside)
}

/// The catalog id a path belongs to (`agents/<id>/...`), or `""`.
fn folder_id(path: &str) -> &str {
    path.strip_prefix("agents/").and_then(|rest| rest.split_once('/')).map_or("", |(id, _)| id)
}

/// The files of one entry folder.
#[derive(Default)]
struct Folder<'a> {
    entry: Option<&'a str>,
    definition: Option<&'a str>,
    /// `(file name, text)` in the order listed.
    fixtures: Vec<(&'a str, &'a str)>,
}

/// Groups the files by folder. A file that is not part of a folder's shape, or is listed twice, is rule 1.
fn group<'a>(files: &[(&'a str, &'a str)], out: &mut Vec<CheckProblem>) -> BTreeMap<&'a str, Folder<'a>> {
    let mut folders: BTreeMap<&'a str, Folder<'a>> = BTreeMap::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for &(path, text) in files {
        if !seen.insert(path) {
            out.push(problem(folder_id(path), 1, format!("{path}: listed twice")));
            continue;
        }
        let Some((id, inside)) = path.strip_prefix("agents/").and_then(|rest| rest.split_once('/')) else {
            out.push(problem("", 1, format!("{path}: catalog files live under agents/<id>/")));
            continue;
        };
        let folder = folders.entry(id).or_default();
        match inside {
            "entry.toml" => folder.entry = Some(text),
            "definition.toml" => folder.definition = Some(text),
            _ => match inside.strip_prefix("fixtures/") {
                Some(name) if !name.is_empty() && !name.contains(['/', '\\']) && name != "." && name != ".." => {
                    folder.fixtures.push((name, text));
                }
                _ => out.push(problem(
                    id,
                    1,
                    format!("{path}: not a catalog file (use entry.toml, definition.toml or fixtures/<name>)"),
                )),
            },
        }
    }
    folders
}

/// Every rule that applies to the files of the catalog. An empty list means the catalog may ship.
/// `code_ids` are the ids of the compiled-in profiles (a store entry may never take one, except a
/// built-in entry, which must).
pub fn check_catalog(files: &[(&str, &str)], code_ids: &[&str]) -> Vec<CheckProblem> {
    let mut out = Vec::new();
    let folders = group(files, &mut out);
    for (id, folder) in &folders {
        check_folder(id, folder, code_ids, &mut out);
    }
    for &(path, text) in files {
        privacy(path, text, &mut out);
    }
    sizes(files, &mut out);
    out
}

fn check_folder(id: &str, folder: &Folder<'_>, code_ids: &[&str], out: &mut Vec<CheckProblem>) {
    let entry_path = format!("agents/{id}/entry.toml");
    let Some(entry_text) = folder.entry else {
        out.push(problem(id, 2, format!("{entry_path}: file missing")));
        return;
    };
    let entry = match parse_entry(entry_text) {
        Ok(entry) => entry,
        Err(message) => {
            out.push(problem(id, 2, format!("{entry_path}: {message}")));
            return;
        }
    };
    for message in validate_entry(&entry) {
        out.push(problem(id, 2, format!("{entry_path}: {message}")));
    }
    if entry.store.id != id {
        out.push(problem(id, 1, format!("store.id `{}` must equal the folder name `{id}`", entry.store.id)));
    }
    let is_code = code_ids.contains(&id);
    if entry.store.builtin {
        // A built-in agent's definition and fixtures live in code, so rule 6 does not apply to it.
        if !is_code {
            out.push(problem(id, 1, "a built-in entry must name a compiled-in profile with the same id"));
        }
        if folder.definition.is_some() {
            out.push(problem(id, 1, format!("agents/{id}/definition.toml: a built-in entry has no definition file")));
        }
        if !folder.fixtures.is_empty() {
            out.push(problem(id, 1, format!("agents/{id}/fixtures: a built-in entry has no fixtures here")));
        }
    } else {
        if is_code {
            out.push(problem(id, 1, format!("`{id}` is a compiled-in profile id; a store entry cannot take it")));
        }
        if let Some((def, profile)) = check_store_definition(id, folder, &entry, code_ids, out) {
            if entry.store.tier == Tier::Verified {
                check_verified(id, folder, &entry, &def, profile.as_ref(), out);
            }
        }
    }
    if entry.store.tier == Tier::Starter {
        check_starter(id, folder, &entry, out);
    }
}

/// Rules 1 (the id inside), 3, 4 and 5 for the definition of a store entry. Returns the parsed
/// definition and, when the file loads clean, its profile. `None` when the file does not parse.
fn check_store_definition(
    id: &str,
    folder: &Folder<'_>,
    entry: &EntryFile,
    code_ids: &[&str],
    out: &mut Vec<CheckProblem>,
) -> Option<(Definition, Option<DeclarativeProfile>)> {
    let path = format!("agents/{id}/definition.toml");
    let Some(text) = folder.definition else {
        out.push(problem(id, 3, format!("{path}: file missing (a store entry needs one)")));
        return None;
    };
    // Rule 3: the same load as a user definition, with the same size and field rules.
    let loaded = load_text(Path::new(&path), text, text.len(), Origin::new(Source::User), None, code_ids);
    for error in &loaded.errors {
        out.push(problem(id, 3, format!("{path}: {}", describe(error))));
    }
    // A file that does not parse was reported above; the rules below need the parsed text.
    let def = parse_definition(text).ok()?;
    if def.agent.id != id {
        out.push(problem(id, 1, format!("{path}: agent.id `{}` must equal the folder name `{id}`", def.agent.id)));
    }
    if entry.store.tier == Tier::Starter && !def.agent.experimental {
        out.push(problem(id, 3, format!("{path}: a starter definition must set agent.experimental = true")));
    }
    if let Some(reason) = forbidden_in_store(&def) {
        out.push(problem(id, 4, format!("{path}: refused in a store definition: {reason}")));
    }
    check_install(id, entry, &def, out);
    let profile = if loaded.errors.is_empty() {
        match DeclarativeProfile::new(def.clone()) {
            Ok(profile) => Some(profile),
            Err(error) => {
                out.push(problem(id, 3, format!("{path}: the profile does not build: {}", describe(&error))));
                None
            }
        }
    } else {
        None
    };
    Some((def, profile))
}

fn describe(error: &DefError) -> String {
    if error.path.is_empty() {
        format!("{}: {}", error.code, error.message)
    } else {
        format!("{} {}: {}", error.code, error.path, error.message)
    }
}

/// Rule 4 (SPEC 3.1 item 7): what a store definition may never carry, in plain words, or `None` when
/// it may. A store definition may not carry `[mcp_host]` (it writes into the agent's own settings),
/// `[hooks]`, `[env] set`, `allow_interpreter`, or any argv or environment value the wizard refuses
/// (`wizard::bypass`). AS-11's add-time check calls this same function.
pub fn forbidden_in_store(def: &Definition) -> Option<String> {
    if def.mcp_host.is_some() {
        return Some("[mcp_host] writes into the agent's own settings file".into());
    }
    if def.hooks.is_some() {
        return Some("[hooks] changes how the agent reports its work".into());
    }
    if !def.env.set.is_empty() {
        return Some("[env] set changes the environment the agent starts in".into());
    }
    if def.agent.allow_interpreter {
        return Some("allow_interpreter lets the agent run a shell".into());
    }
    hits_in_definition(def).first().map(|hit| format!("`{}` is refused, {}", hit.token, hit.why))
}

/// Rule 5: where the install comes from. The source host is the vendor's own host or a listed script
/// host, a script URL's host is listed, and every npm or uv package has its publisher repository.
fn check_install(id: &str, entry: &EntryFile, def: &Definition, out: &mut Vec<CheckProblem>) {
    let Some(spec) = def.install.as_ref().and_then(|table| table.windows.as_ref()) else { return };
    let publisher = &entry.publisher;
    let listed = |host: Option<&str>| {
        host.is_some_and(|h| publisher.script_hosts.iter().any(|script| script.eq_ignore_ascii_case(h)))
    };
    let source = host_of(&spec.source);
    if !same_host(source, host_of(&entry.vendor.homepage)) && !listed(source) {
        out.push(problem(
            id,
            5,
            format!(
                "install source host `{}` is neither the vendor's homepage host nor in publisher.script_hosts",
                source.unwrap_or("")
            ),
        ));
    }
    if let Some(url) = &spec.script_url {
        if !listed(host_of(url)) {
            out.push(problem(id, 5, "install.windows.script_url host is not in publisher.script_hosts"));
        }
    }
    if spec.npm.is_some() && publisher.npm_repo.is_empty() {
        out.push(problem(id, 5, "an npm install needs publisher.npm_repo"));
    }
    if spec.uv.is_some() && publisher.pypi_repo.is_empty() {
        out.push(problem(id, 5, "a uv install needs publisher.pypi_repo"));
    }
}

/// The host of an https address: `https://host/path` gives `host`.
fn host_of(url: &str) -> Option<&str> {
    url.strip_prefix("https://").map(|rest| rest.split(['/', '?', '#']).next().unwrap_or(""))
}

fn same_host(a: Option<&str>, b: Option<&str>) -> bool {
    matches!((a, b), (Some(x), Some(y)) if x.eq_ignore_ascii_case(y))
}

/// Rule 6: a verified entry has a fixtures folder whose captures pass `check_dir`, the captures its
/// kind needs (each with an `.expect.toml`), a `[profile] tested` range, `windows_tested = true`, and
/// it passes every conformance rule for its built profile.
fn check_verified(
    id: &str,
    folder: &Folder<'_>,
    entry: &EntryFile,
    def: &Definition,
    profile: Option<&DeclarativeProfile>,
    out: &mut Vec<CheckProblem>,
) {
    if folder.fixtures.is_empty() {
        out.push(problem(id, 6, format!("agents/{id}/fixtures: a verified entry needs fixtures (captures with .expect.toml files)")));
    }
    if !entry.checked.windows_tested {
        out.push(problem(id, 6, "checked.windows_tested must be true for a verified entry"));
    }
    if def.profile.tested.is_none() {
        out.push(problem(id, 6, "a verified definition needs [profile] tested (the CLI versions it was checked on)"));
    }
    let required = if def.tui.is_some() { TUI_CAPTURES } else { HEADLESS_CAPTURES };
    for stem in required {
        if !has_capture(&folder.fixtures, stem) {
            out.push(problem(id, 6, format!("no capture named {stem}.*")));
        }
        let expect = format!("{stem}.expect.toml");
        if !folder.fixtures.iter().any(|(name, _)| *name == expect) {
            out.push(problem(id, 6, format!("no {expect} next to the {stem} capture")));
        }
    }
    let Some(profile) = profile else { return };
    if !folder.fixtures.is_empty() {
        match run_fixtures(&folder.fixtures, profile) {
            Ok(results) => {
                for result in results.iter().filter(|result| !result.passed) {
                    out.push(problem(id, 6, format!("fixture {}: {}", result.fixture, result.failures.join("; "))));
                }
            }
            Err(message) => out.push(problem(id, 6, message)),
        }
    }
    for (name, rule) in conformance_rules::ALL {
        if let Err(message) = rule(profile) {
            out.push(problem(id, 6, format!("conformance rule {name}: {message}")));
        }
    }
}

fn stem_of(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

fn has_capture(fixtures: &[(&str, &str)], stem: &str) -> bool {
    fixtures.iter().any(|(name, _)| !name.ends_with(".expect.toml") && stem_of(name) == stem)
}

/// Writes the fixtures into a fresh temporary folder, runs `check_dir` on it and removes the folder.
fn run_fixtures(fixtures: &[(&str, &str)], profile: &DeclarativeProfile) -> Result<Vec<FixtureResult>, String> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir()
        .join(format!("pm-store-check-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make a temporary folder for the fixtures: {e}"))?;
    let written = fixtures.iter().try_for_each(|(name, text)| {
        std::fs::write(dir.join(name), text).map_err(|e| format!("cannot write the fixture {name}: {e}"))
    });
    let results = written.map(|()| check_dir(profile, &dir));
    let _ = std::fs::remove_dir_all(&dir);
    results
}

/// Rule 7: a starter has no fixtures folder, and its entry text never says it is tested or verified.
fn check_starter(id: &str, folder: &Folder<'_>, entry: &EntryFile, out: &mut Vec<CheckProblem>) {
    if !folder.fixtures.is_empty() {
        out.push(problem(id, 7, format!("agents/{id}/fixtures: a starter entry has no fixtures folder (it is not tested)")));
    }
    let fields = [
        ("store.tagline", entry.store.tagline.as_str()),
        ("vendor.name", entry.vendor.name.as_str()),
        ("vendor.license", entry.vendor.license.as_str()),
        ("sign_in.summary", entry.sign_in.summary.as_str()),
        ("uninstall.note", entry.uninstall.note.as_str()),
    ];
    for (field, text) in fields {
        let lower = text.to_lowercase();
        if lower.contains("tested") || lower.contains("verified") {
            out.push(problem(id, 7, format!("{field} says tested or verified; a starter is not tested, so its text may not say so")));
        }
    }
}

struct Patterns {
    user_dir: Regex,
    mail: Regex,
    tokens: Vec<Regex>,
}

fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(|| Patterns {
        user_dir: Regex::new(r#"(?i)(?:^|[^A-Za-z0-9_])users[\\/]+([^\\/\s'"`<>|*?:;,)\]}]+)"#)
            .expect("user folder pattern"),
        mail: Regex::new(
            r"(?i)([A-Za-z0-9._%+\-]+)@(?:gmail|googlemail|outlook|hotmail|live|yahoo|icloud|proton|protonmail)\.(?:com|me)\b",
        )
        .expect("mail pattern"),
        tokens: [
            r"\bsk-[A-Za-z0-9_\-]{20,}",
            r"\b(?:gh[pousr]_|github_pat_)[A-Za-z0-9_]{30,}",
            r"\bAKIA[0-9A-Z]{16}\b",
            r"\bxox[abposr]-[A-Za-z0-9\-]{10,}",
            r"\bAIza[0-9A-Za-z_\-]{30,}",
            r"\beyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}",
            r#"(?i)\b(?:api[_-]?key|access[_-]?token|secret|password|passwd)\s*[=:]\s*["']?[A-Za-z0-9_\-./+]{20,}"#,
        ]
        .iter()
        .map(|pattern| Regex::new(pattern).expect("token pattern"))
        .collect(),
    })
}

/// The kinds of personal data one line holds: a real-looking user folder, a consumer email address,
/// a token-shaped string. Placeholders (`C:\Users\you\`, `you@example.com`) are not hits.
fn privacy_hits(line: &str) -> Vec<&'static str> {
    let p = patterns();
    let mut hits = Vec::new();
    let folder = p.user_dir.captures_iter(line).any(|c| {
        let name = &c[1];
        !name.contains(|ch: char| PLACEHOLDER_CHARS.contains(ch))
            && !ALLOWED_USER_NAMES.contains(&name.to_lowercase().as_str())
    });
    if folder {
        hits.push("a real-looking user folder name");
    }
    if p.mail.captures_iter(line).any(|c| !ALLOWED_MAIL_LOCALS.contains(&c[1].to_lowercase().as_str())) {
        hits.push("an email address");
    }
    if p.tokens.iter().any(|re| re.is_match(line)) {
        hits.push("a token-shaped string");
    }
    hits
}

/// Rule 8: no personal data in any catalog file (entry, definition or fixture).
fn privacy(path: &str, text: &str, out: &mut Vec<CheckProblem>) {
    for (index, line) in text.lines().enumerate() {
        for kind in privacy_hits(line) {
            out.push(problem(folder_id(path), 8, format!("{path}:{}: {kind} (the value is not shown)", index + 1)));
        }
    }
}

/// Rule 9: each fixture at most 2 MB, each `entry.toml` at most 4 MB, the whole catalog at most 8 MB.
fn sizes(files: &[(&str, &str)], out: &mut Vec<CheckProblem>) {
    let mut total = 0usize;
    for &(path, text) in files {
        total += text.len();
        if path.ends_with("/entry.toml") && text.len() > ENTRY_MAX_BYTES {
            out.push(problem(folder_id(path), 9, format!("{path}: an entry file is at most 4 MB")));
        }
        if inside_folder(path).is_some_and(|inside| inside.starts_with("fixtures/")) && text.len() > FIXTURE_MAX_BYTES {
            out.push(problem(folder_id(path), 9, format!("{path}: a fixture is at most 2 MB")));
        }
    }
    if total > CATALOG_MAX_BYTES {
        out.push(problem("", 9, format!("the catalog is {total} bytes; the limit is 8 MB")));
    }
}

/// Rule 10 (warning only): a `checked.date` more than 180 days before `today`, and a deprecated entry
/// with no `replaced_by`. Nothing is refused for these, so `check_catalog` does not return them.
pub fn warnings(files: &[(&str, &str)], today: NaiveDate) -> Vec<CheckProblem> {
    let mut out = Vec::new();
    for &(path, text) in files {
        if !path.ends_with("/entry.toml") {
            continue;
        }
        let Ok(entry) = parse_entry(text) else { continue };
        let id = entry.store.id.as_str();
        if let Ok(date) = NaiveDate::parse_from_str(&entry.checked.date, "%Y-%m-%d") {
            let age = (today - date).num_days();
            if age > STALE_DAYS {
                out.push(problem(id, 10, format!("checked.date is {age} days old; check the facts again")));
            }
        }
        if entry.store.status == Status::Deprecated && entry.store.replaced_by.is_empty() {
            out.push(problem(id, 10, "status is deprecated but replaced_by is empty"));
        }
    }
    out
}

/// Rule 11: a catalog copy of a test definition must be byte-identical to `tests/agent-definitions/<id>.toml`.
/// `definitions` is `(file name, text)` from that folder. A catalog entry with no such file is not compared.
pub fn drift(files: &[(&str, &str)], definitions: &[(&str, &str)]) -> Vec<CheckProblem> {
    let mut out = Vec::new();
    for &(name, text) in definitions {
        let Some(id) = name.strip_suffix(".toml") else { continue };
        let path = format!("agents/{id}/definition.toml");
        if let Some(&(_, shipped)) = files.iter().find(|(p, _)| *p == path) {
            if shipped != text {
                out.push(problem(id, 11, format!("{path} differs from tests/agent-definitions/{name}; copy the file again")));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_store::embedded;

    /// The compiled-in profile ids the tests use (the real list is the registry's).
    const CODES: &[&str] = &["claude", "codex", "cursor_agent", "opencode", "shell"];

    /// A valid `entry.toml` for `id`, with the tier and `windows_tested` given.
    fn entry_toml(id: &str, tier: &str, windows_tested: bool) -> String {
        format!(
            "schema_version = 1\n\n[store]\nid = \"{id}\"\ntagline = \"A test agent.\"\ntier = \"{tier}\"\n\
             categories = [\"general\"]\nstatus = \"active\"\nfirst_listed = \"2026-10-28\"\n\n\
             [vendor]\nname = \"Test Vendor\"\nhomepage = \"https://example.com/{id}\"\nlicense = \"Apache-2.0\"\n\
             license_kind = \"open_source\"\n\n[sign_in]\nkind = \"none\"\nsummary = \"No sign-in needed.\"\n\n\
             [checked]\ndate = \"2026-10-28\"\nversion = \"1.0.0\"\nhow = \"vendor_docs\"\n\
             windows_tested = {windows_tested}\n\n[publisher]\nnpm_repo = \"https://example.com/{id}\"\n"
        )
    }

    /// A minimal valid generic, experimental definition with a Windows npm install.
    fn definition_toml(id: &str) -> String {
        format!(
            "schema_version = 1\n\n[agent]\nid = \"{id}\"\nlabel = \"Test {id}\"\nkind = \"generic\"\n\
             experimental = true\n\n[binary]\nnames = [\"{id}\"]\n\n\
             [install.windows]\nsource = \"https://example.com/{id}\"\nnpm = \"test-{id}-cli\"\n"
        )
    }

    /// A starter entry folder: its entry and its definition, no fixtures.
    fn starter(id: &str) -> Vec<(String, String)> {
        vec![
            (format!("agents/{id}/entry.toml"), entry_toml(id, "starter", false)),
            (format!("agents/{id}/definition.toml"), definition_toml(id)),
        ]
    }

    fn refs(files: &[(String, String)]) -> Vec<(&str, &str)> {
        files.iter().map(|(path, text)| (path.as_str(), text.as_str())).collect()
    }

    fn check(files: &[(String, String)]) -> Vec<CheckProblem> {
        check_catalog(&refs(files), CODES)
    }

    /// Asserts that the catalog is refused under `rule`, with `needle` in the message.
    fn refused(files: &[(String, String)], rule: u8, needle: &str) {
        let problems = check(files);
        assert!(
            problems.iter().any(|p| p.rule == rule && p.message.contains(needle)),
            "expected rule {rule} with `{needle}`, got {problems:#?}"
        );
    }

    fn clean(files: &[(String, String)]) {
        let problems = check(files);
        assert!(problems.is_empty(), "expected no problems, got {problems:#?}");
    }

    /// Replaces the first `from` in the file at `path` with `to`.
    fn edit(files: &mut [(String, String)], path: &str, from: &str, to: &str) {
        let file = files.iter_mut().find(|(p, _)| p.as_str() == path).expect("file in the test catalog");
        assert!(file.1.contains(from), "test text not in {path}: {from}");
        file.1 = file.1.replacen(from, to, 1);
    }

    fn set_text(files: &mut [(String, String)], path: &str, text: &str) {
        let file = files.iter_mut().find(|(p, _)| p.as_str() == path).expect("file in the test catalog");
        file.1 = text.to_string();
    }

    fn add(files: &mut Vec<(String, String)>, path: &str, text: &str) {
        files.push((path.to_string(), text.to_string()));
    }

    #[test]
    fn a_clean_starter_entry_passes_every_rule() {
        clean(&starter("alpha"));
    }

    #[test]
    fn a_built_in_entry_needs_its_compiled_in_profile_and_no_definition() {
        let mut files = vec![(
            "agents/claude/entry.toml".to_string(),
            entry_toml("claude", "starter", false).replace("[store]\n", "[store]\nbuiltin = true\n"),
        )];
        clean(&files);
        add(&mut files, "agents/claude/definition.toml", &definition_toml("claude"));
        refused(&files, 1, "has no definition file");
        let files = vec![(
            "agents/alpha/entry.toml".to_string(),
            entry_toml("alpha", "starter", false).replace("[store]\n", "[store]\nbuiltin = true\n"),
        )];
        refused(&files, 1, "compiled-in profile");
    }

    /// A built-in agent is `entry.toml` only: its tier is `verified` and it has no fixtures or
    /// `windows_tested` (its captures and conformance live in code), so rule 6 does not apply to it.
    #[test]
    fn a_built_in_verified_entry_is_exempt_from_the_fixture_and_definition_rules() {
        let files = vec![(
            "agents/codex/entry.toml".to_string(),
            entry_toml("codex", "verified", false).replace("[store]\n", "[store]\nbuiltin = true\n"),
        )];
        clean(&files);
    }

    #[test]
    fn rule_1_the_folder_name_must_equal_the_store_id() {
        let mut files = starter("alpha");
        edit(&mut files, "agents/alpha/entry.toml", "id = \"alpha\"", "id = \"beta\"");
        refused(&files, 1, "must equal the folder name");
    }

    #[test]
    fn rule_1_a_compiled_in_id_cannot_be_a_store_entry() {
        refused(&starter("claude"), 1, "compiled-in profile id");
    }

    #[test]
    fn rule_1_stray_and_misplaced_files_are_refused() {
        let mut files = starter("alpha");
        add(&mut files, "agents/alpha/notes.md", "x");
        refused(&files, 1, "not a catalog file");
        let mut files = starter("alpha");
        add(&mut files, "other/entry.toml", "x");
        refused(&files, 1, "live under agents/");
    }

    #[test]
    fn rule_2_the_entry_must_parse_and_validate() {
        let mut files = starter("alpha");
        edit(&mut files, "agents/alpha/entry.toml", "tier = \"starter\"", "tier = \"gold\"");
        refused(&files, 2, "store.tier");

        let mut files = starter("alpha");
        edit(&mut files, "agents/alpha/entry.toml", "schema_version = 1", "schema_version = 1\nsurprise = 1");
        refused(&files, 2, "surprise");

        let mut files = starter("alpha");
        edit(&mut files, "agents/alpha/entry.toml", "homepage = \"https://example.com/alpha\"", "homepage = \"http://example.com/alpha\"");
        refused(&files, 2, "vendor.homepage");

        let files = vec![starter("alpha").remove(1)];
        refused(&files, 2, "entry.toml: file missing");
    }

    #[test]
    fn rule_3_the_definition_must_load_and_a_starter_must_be_experimental() {
        let mut files = starter("alpha");
        edit(&mut files, "agents/alpha/definition.toml", "schema_version = 1", "schema_version = 99");
        refused(&files, 3, "agents/alpha/definition.toml:");

        let files = vec![starter("alpha").remove(0)];
        refused(&files, 3, "file missing");

        let mut files = starter("alpha");
        edit(&mut files, "agents/alpha/definition.toml", "experimental = true", "experimental = false");
        refused(&files, 3, "experimental = true");
    }

    #[test]
    fn rule_4_a_store_definition_cannot_carry_env_set_hooks_or_interpreters() {
        let mut files = starter("alpha");
        let def = format!("{}\n[env]\nset = {{ TEST_FLAG = \"1\" }}\n", definition_toml("alpha"));
        set_text(&mut files, "agents/alpha/definition.toml", &def);
        refused(&files, 4, "[env] set");

        let mut files = starter("alpha");
        edit(&mut files, "agents/alpha/definition.toml", "kind = \"generic\"", "kind = \"generic\"\nallow_interpreter = true");
        refused(&files, 4, "allow_interpreter");

        let mut files = starter("alpha");
        let def = format!("{}\n[hooks]\nschema = 1\ninstall = \"launch_flag\"\nentry = {{ x = \"y\" }}\n", definition_toml("alpha"));
        set_text(&mut files, "agents/alpha/definition.toml", &def);
        refused(&files, 4, "[hooks]");

        let mut files = starter("alpha");
        let def = format!("{}\n[tui]\nargv = [\"alpha\", \"--yolo\"]\n", definition_toml("alpha"));
        set_text(&mut files, "agents/alpha/definition.toml", &def);
        refused(&files, 4, "--yolo");
    }

    #[test]
    fn forbidden_in_store_names_each_refused_setting_and_passes_a_plain_definition() {
        let plain = parse_definition(&definition_toml("alpha")).expect("parses");
        assert_eq!(forbidden_in_store(&plain), None);
        let env = parse_definition(&format!("{}\n[env]\nset = {{ A = \"1\" }}\n", definition_toml("alpha"))).expect("parses");
        assert!(forbidden_in_store(&env).is_some_and(|r| r.contains("[env] set")));
        let interp = parse_definition(&definition_toml("alpha").replace("kind = \"generic\"", "kind = \"generic\"\nallow_interpreter = true")).expect("parses");
        assert!(forbidden_in_store(&interp).is_some_and(|r| r.contains("allow_interpreter")));
    }

    #[test]
    fn rule_5_the_install_must_come_from_the_vendor_or_a_listed_host() {
        let mut files = starter("alpha");
        edit(&mut files, "agents/alpha/definition.toml", "source = \"https://example.com/alpha\"", "source = \"https://other.example.org/alpha\"");
        refused(&files, 5, "install source host");

        let mut files = starter("alpha");
        edit(
            &mut files,
            "agents/alpha/definition.toml",
            "npm = \"test-alpha-cli\"",
            "npm = \"test-alpha-cli\"\nscript_url = \"https://get.example.org/install.ps1\"",
        );
        refused(&files, 5, "script_url host");

        let mut files = starter("alpha");
        edit(&mut files, "agents/alpha/entry.toml", "npm_repo = \"https://example.com/alpha\"", "npm_repo = \"\"");
        refused(&files, 5, "publisher.npm_repo");

        let mut files = starter("alpha");
        edit(&mut files, "agents/alpha/definition.toml", "npm = \"test-alpha-cli\"", "uv = \"test-alpha\"");
        refused(&files, 5, "publisher.pypi_repo");
    }

    #[test]
    fn rule_6_a_verified_entry_needs_fixtures_windows_tested_and_a_profile_range() {
        let files = vec![
            ("agents/alpha/entry.toml".to_string(), entry_toml("alpha", "verified", true)),
            ("agents/alpha/definition.toml".to_string(), definition_toml("alpha")),
        ];
        refused(&files, 6, "needs fixtures");
        refused(&files, 6, "[profile] tested");
        refused(&files, 6, "headless_success.*");

        let files = vec![
            ("agents/alpha/entry.toml".to_string(), entry_toml("alpha", "verified", false)),
            ("agents/alpha/definition.toml".to_string(), definition_toml("alpha")),
        ];
        refused(&files, 6, "windows_tested");
    }

    /// The kilo copy the repo ships as a test definition (it carries its own `[profile] tested`
    /// range, the same bytes as the catalog copy), and its fixtures from `tui/declarative/fixtures/kilo`.
    fn kilo_verified() -> Vec<(String, String)> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let definition = std::fs::read_to_string(root.join("tests/agent-definitions/kilo.toml")).expect("kilo definition");
        let mut files = vec![
            ("agents/kilo/entry.toml".to_string(), entry_toml("kilo", "verified", true)),
            ("agents/kilo/definition.toml".to_string(), definition),
        ];
        let dir = root.join("src/tui/declarative/fixtures/kilo");
        for item in std::fs::read_dir(&dir).expect("kilo fixtures").flatten() {
            if item.file_type().is_ok_and(|t| t.is_file()) {
                let name = item.file_name().to_string_lossy().into_owned();
                let text = std::fs::read_to_string(item.path()).expect("readable fixture");
                files.push((format!("agents/kilo/fixtures/{name}"), text));
            }
        }
        files
    }

    #[test]
    fn rule_6_the_kilo_captures_pass_and_a_broken_expectation_is_refused() {
        let mut files = kilo_verified();
        // kilo has a [tui] table, so its trio of captures is checked by `check_dir` and the conformance rules.
        clean(&files);
        let expect = files
            .iter_mut()
            .find(|(p, _)| p == "agents/kilo/fixtures/tui_idle.expect.toml")
            .expect("tui_idle expectation");
        expect.1.push_str("\nnot toml [[\n");
        refused(&files, 6, "fixture tui_idle.txt");
    }

    #[test]
    fn rule_7_a_starter_has_no_fixtures_and_never_says_tested_or_verified() {
        let mut files = starter("alpha");
        add(&mut files, "agents/alpha/fixtures/tui_idle.txt", "idle\n");
        refused(&files, 7, "no fixtures folder");

        let mut files = starter("alpha");
        edit(&mut files, "agents/alpha/entry.toml", "tagline = \"A test agent.\"", "tagline = \"Fully tested and verified.\"");
        refused(&files, 7, "tested or verified");
    }

    #[test]
    fn rule_8_no_personal_folder_email_or_token_in_any_file() {
        let name = ["ali", "ce"].concat();
        let mut files = starter("alpha");
        add(&mut files, "agents/alpha/fixtures/notes.txt", &format!("C:\\Users\\{name}\\AppData\\x\n"));
        refused(&files, 8, "a real-looking user folder name");

        let mut files = starter("alpha");
        add(&mut files, "agents/alpha/fixtures/notes.txt", &format!("owner: {}@gmail.com\n", "alice"));
        refused(&files, 8, "an email address");

        let mut files = starter("alpha");
        add(&mut files, "agents/alpha/fixtures/notes.txt", &format!("key {}\n", format!("sk-{}", "a".repeat(24))));
        refused(&files, 8, "a token-shaped string");
    }

    #[test]
    fn rule_8_the_placeholders_are_not_personal_data() {
        let mut files = starter("alpha");
        add(&mut files, "agents/alpha/fixtures/notes.txt", "C:\\Users\\you\\.claude\nmail you@example.com\n");
        assert!(!check(&files).iter().any(|p| p.rule == 8), "{:#?}", check(&files));
    }

    #[test]
    fn rule_9_fixtures_and_the_whole_catalog_have_size_limits() {
        let mut files = starter("alpha");
        add(&mut files, "agents/alpha/fixtures/big.txt", &"x".repeat(FIXTURE_MAX_BYTES + 1));
        refused(&files, 9, "a fixture is at most 2 MB");

        // Three fixtures of exactly 2 MB and one that fills the rest of the 8 MB allowance: exactly
        // 8 MB is fine, one more byte is refused.
        let mut files = starter("alpha");
        let base: usize = files.iter().map(|(_, text)| text.len()).sum();
        for n in 0..3 {
            add(&mut files, &format!("agents/alpha/fixtures/f{n}.txt"), &"x".repeat(FIXTURE_MAX_BYTES));
        }
        add(&mut files, "agents/alpha/fixtures/f3.txt", &"x".repeat(CATALOG_MAX_BYTES - base - 3 * FIXTURE_MAX_BYTES));
        assert!(!check(&files).iter().any(|p| p.rule == 9), "{:#?}", check(&files));
        add(&mut files, "agents/alpha/fixtures/f4.txt", "x");
        refused(&files, 9, "the catalog is");
    }

    #[test]
    fn rule_10_stale_dates_and_deprecated_entries_only_warn() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 28).expect("date");
        let mut files = starter("alpha");
        edit(&mut files, "agents/alpha/entry.toml", "date = \"2026-10-28\"", "date = \"2026-01-01\"");
        let warned = warnings(&refs(&files), today);
        assert!(warned.iter().any(|p| p.rule == 10 && p.message.contains("days old")), "{warned:#?}");
        assert!(check(&files).is_empty(), "a warning must not refuse: {:#?}", check(&files));

        let mut files = starter("alpha");
        edit(&mut files, "agents/alpha/entry.toml", "status = \"active\"", "status = \"deprecated\"");
        let warned = warnings(&refs(&files), today);
        assert!(warned.iter().any(|p| p.rule == 10 && p.message.contains("replaced_by")), "{warned:#?}");
        assert!(check(&files).is_empty());

        assert!(warnings(&refs(&starter("alpha")), today).is_empty());
    }

    #[test]
    fn rule_11_a_copy_of_a_test_definition_must_match_it_byte_for_byte() {
        let files = starter("kilo");
        let same = definition_toml("kilo");
        assert!(drift(&refs(&files), &[("kilo.toml", same.as_str())]).is_empty());
        let changed = format!("{same}# edited\n");
        let found = drift(&refs(&files), &[("kilo.toml", changed.as_str())]);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!(found[0].rule, 11);
        assert_eq!(found[0].id, "kilo");
    }

    /// The shipped catalog (`embedded::FILES`) must pass every rule. Once entries land this runs over
    /// all of them; while the catalog is empty it still runs the rules over an empty file list.
    #[test]
    fn the_shipped_catalog_has_no_problems() {
        let code_ids = crate::tui::registry::registry().code_ids();
        let problems = check_catalog(embedded::FILES, &code_ids);
        assert!(problems.is_empty(), "{problems:#?}");

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/agent-definitions");
        let mut definitions: Vec<(String, String)> = std::fs::read_dir(&root)
            .expect("tests/agent-definitions exists")
            .flatten()
            .map(|item| item.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".toml"))
            .map(|name| {
                let text = std::fs::read_to_string(root.join(&name)).expect("readable test definition");
                (name, text)
            })
            .collect();
        definitions.sort();
        let borrowed: Vec<(&str, &str)> = definitions.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();
        let drifted = drift(embedded::FILES, &borrowed);
        assert!(drifted.is_empty(), "{drifted:#?}");
    }
}
