//! Companion skills: "how I like my weekly report", learned once and kept.
//!
//! A skill is a plain Markdown file in the home folder, `skills/<slug>.md`: a `# Title` line, a blank
//! line and the body. At most `MAX_SKILLS` of them, each at most `SKILL_CAP` characters.
//!
//! The agent never writes one. It proposes it in `memory/pending.md` (`- skill: <title>` and an indented
//! body, parsed by `memory.rs`), and only the person's Approve (or an Edit and Save) creates the file.
//! Every skill the person approves or saves also gets an approved copy in app data. A file that has no
//! approved copy, or differs from it, was changed outside Puppet Master: it is shown with a warning and
//! the brief does not list it until the person has looked at it and saved it.
//!
//! Removing a skill moves the file into `skills/.removed/`. Nothing here deletes a file.
//! Nothing here names an agent.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::memory::{flag_for, write_home};
use super::model::{
    char_len, cut_chars, unique_slug, valid_id, CompanionError, CompanionResult, MAX_SKILLS, SKILL_CAP, SKILL_TITLE_CAP,
};
use super::{brief::SkillLine, store};
use crate::redact;

pub const SKILLS_REL: &str = "skills";
const REMOVED_DIR: &str = ".removed";
/// How many files a listing reads at most. A folder with more is a damaged or hostile one.
const LIST_LIMIT: usize = 100;

/// One skill in a list. No body: the phone and the brief only ever see titles.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillRef {
    pub slug: String,
    pub title: String,
    /// Characters in the file.
    pub chars: usize,
    /// No approved copy, or the file differs from it: it changed outside Puppet Master.
    pub unreviewed: bool,
    /// "Has a link" | "Sounds like an instruction" | None. Flags never block.
    pub flagged: Option<String>,
}

/// One skill opened for reading or editing (desktop only).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillFull {
    pub slug: String,
    pub title: String,
    pub body: String,
    pub chars: usize,
    pub unreviewed: bool,
    pub flagged: Option<String>,
}

pub fn skills_dir(home: &Path) -> PathBuf {
    home.join(SKILLS_REL)
}

/// `skills/<slug>.md`, refused for a slug that is not `[a-z0-9-]` (so no path can escape).
pub fn skill_path(home: &Path, slug: &str) -> CompanionResult<PathBuf> {
    if !valid_id(slug) {
        return Err(not_found());
    }
    Ok(skills_dir(home).join(format!("{slug}.md")))
}

/// A title for one line: control characters and angle brackets removed (so it can never close a marker
/// comment in the brief), spaces collapsed, redacted, cut to `SKILL_TITLE_CAP`.
pub fn clean_title(raw: &str) -> String {
    let plain: String = raw.chars().filter(|c| !c.is_control() && *c != '<' && *c != '>').collect();
    let plain = plain.trim().trim_start_matches('#').trim();
    let one_line = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    let redacted = redact::text(&one_line).into_inner();
    cut_chars(redacted.trim(), SKILL_TITLE_CAP)
}

/// A body: CRLF to LF, control characters other than newline removed (a tab becomes two spaces), redacted,
/// trimmed, and cut to `limit` characters.
pub fn clean_body(raw: &str, limit: usize) -> String {
    let unix = raw.replace("\r\n", "\n").replace('\r', "\n");
    let plain: String = unix
        .chars()
        .flat_map(|c| match c {
            '\t' => vec![' ', ' '],
            c if c.is_control() && c != '\n' => Vec::new(),
            c => vec![c],
        })
        .collect();
    let redacted = redact::text(plain.trim()).into_inner();
    cut_chars(redacted.trim(), limit)
}

/// The text of a skill file.
pub fn render(title: &str, body: &str) -> String {
    format!("# {title}\n\n{body}\n")
}

/// Title and body of a skill file: the first non-empty line is the title (`#` signs removed).
pub fn parse_file(text: &str) -> (String, String) {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut lines = text.lines();
    let mut title = String::new();
    for line in lines.by_ref() {
        if !line.trim().is_empty() {
            title = clean_title(line);
            break;
        }
    }
    (title, lines.collect::<Vec<_>>().join("\n").trim().to_string())
}

/// What the person's request must satisfy before a skill file is written.
fn check(title: &str, body: &str) -> CompanionResult<()> {
    if title.is_empty() {
        return Err(invalid("A skill needs a title."));
    }
    if body.is_empty() {
        return Err(invalid("A skill needs some steps."));
    }
    if char_len(&render(title, body)) > SKILL_CAP {
        return Err(CompanionError::new(
            "COMPANION_FILE_TOO_LONG",
            format!("A skill can have at most {SKILL_CAP} characters."),
        ));
    }
    Ok(())
}

/// Every skill in `skills/`, by slug. Hidden folders (`.removed`) and files that are not `<slug>.md` are skipped.
pub fn list(root: &Path, id: &str, home: &Path) -> CompanionResult<Vec<SkillRef>> {
    let mut slugs = slugs_in(home)?;
    slugs.truncate(LIST_LIMIT);
    let mut out = Vec::with_capacity(slugs.len());
    for slug in slugs {
        if let Some(full) = read(root, id, home, &slug)? {
            out.push(SkillRef { slug: full.slug, title: full.title, chars: full.chars, unreviewed: full.unreviewed, flagged: full.flagged });
        }
    }
    Ok(out)
}

fn slugs_in(home: &Path) -> CompanionResult<Vec<String>> {
    let dir = skills_dir(home);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(folder_error(&dir, &e)),
    };
    let mut slugs: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let stem = name.strip_suffix(".md")?;
            valid_id(stem).then(|| stem.to_string())
        })
        .collect();
    slugs.sort();
    Ok(slugs)
}

/// Opens one skill. None when there is no such file.
pub fn read(root: &Path, id: &str, home: &Path, slug: &str) -> CompanionResult<Option<SkillFull>> {
    let Some(text) = store::read_optional(&skill_path(home, slug)?)? else {
        return Ok(None);
    };
    let (title, body) = parse_file(&text);
    let approved = store::read_optional(&store::approved_skill_path(root, id, slug)?)?;
    let title = if title.is_empty() { slug.to_string() } else { title };
    Ok(Some(SkillFull {
        slug: slug.to_string(),
        flagged: flag_for(&format!("{title}\n{body}")),
        chars: char_len(&text),
        unreviewed: approved.as_deref() != Some(text.as_str()),
        title,
        body,
    }))
}

pub fn get(root: &Path, id: &str, home: &Path, slug: &str) -> CompanionResult<SkillFull> {
    read(root, id, home, slug)?.ok_or_else(not_found)
}

/// Creates a skill from an approved proposal (or from an edited one). The slug comes from the title and
/// never replaces another skill. At most `MAX_SKILLS`; each at most `SKILL_CAP` characters.
pub fn add(root: &Path, id: &str, home: &Path, title: &str, body: &str) -> CompanionResult<SkillFull> {
    let title = clean_title(title);
    let body = clean_body(body, SKILL_CAP * 2);
    check(&title, &body)?;
    let taken = slugs_in(home)?;
    if taken.len() >= MAX_SKILLS {
        return Err(CompanionError::new(
            "COMPANION_SKILLS_FULL",
            format!("You can have at most {MAX_SKILLS} skills. Remove one first."),
        ));
    }
    let taken_refs: Vec<&str> = taken.iter().map(String::as_str).collect();
    let slug = unique_slug(&title, &taken_refs);
    write_skill(root, id, home, &slug, &title, &body)?;
    get(root, id, home, &slug)
}

/// The desktop edit of an existing skill. The slug (and so the path the brief lists) stays.
pub fn save(root: &Path, id: &str, home: &Path, slug: &str, title: &str, body: &str) -> CompanionResult<SkillFull> {
    if !skill_path(home, slug)?.is_file() {
        return Err(not_found());
    }
    let title = clean_title(title);
    let body = clean_body(body, SKILL_CAP * 2);
    check(&title, &body)?;
    write_skill(root, id, home, slug, &title, &body)?;
    get(root, id, home, slug)
}

/// Removes a skill by moving its file to `skills/.removed/`. A name that is taken there gets `-2`, `-3`, ...
pub fn remove(home: &Path, slug: &str) -> CompanionResult<()> {
    let from = skill_path(home, slug)?;
    if !from.is_file() {
        return Err(not_found());
    }
    let dir = skills_dir(home).join(REMOVED_DIR);
    fs::create_dir_all(&dir).map_err(|e| folder_error(&dir, &e))?;
    let mut to = dir.join(format!("{slug}.md"));
    let mut n = 2;
    while to.exists() {
        to = dir.join(format!("{slug}-{n}.md"));
        n += 1;
    }
    fs::rename(&from, &to).map_err(|e| folder_error(&from, &e))
}

/// The approved skills for the brief: title and path, no body. A skill that changed outside Puppet Master
/// (or was never approved) is left out until the person has looked at it.
pub fn brief_lines(root: &Path, id: &str, home: &Path) -> CompanionResult<Vec<SkillLine>> {
    Ok(list(root, id, home)?
        .into_iter()
        .filter(|s| !s.unreviewed)
        .map(|s| SkillLine { title: s.title, path: format!("{SKILLS_REL}/{}.md", s.slug) })
        .collect())
}

fn write_skill(root: &Path, id: &str, home: &Path, slug: &str, title: &str, body: &str) -> CompanionResult<()> {
    let text = render(title, body);
    fs::create_dir_all(skills_dir(home)).map_err(|e| folder_error(&skills_dir(home), &e))?;
    write_home(&skill_path(home, slug)?, &text)?;
    store::write_private_text(&store::approved_skill_path(root, id, slug)?, &text)
}

fn not_found() -> CompanionError {
    CompanionError::new("COMPANION_SKILL_NOT_FOUND", "No skill has that name.")
}

fn invalid(message: &str) -> CompanionError {
    CompanionError::new("COMPANION_SKILL_INVALID", message)
}

fn folder_error(path: &Path, e: &std::io::Error) -> CompanionError {
    CompanionError::new("COMPANION_FOLDER_INVALID", format!("Could not use {}: {e}.", path.display()))
}
