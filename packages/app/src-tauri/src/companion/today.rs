//! `today.md`: what the person's other agents finished today, one line per turn, in the companion's
//! home folder (next to SOUL.md, USER.md and MEMORY.md). The companion reads it to answer "what did
//! my agents do today?".
//!
//! The person turns it on per companion (`CompanionProfile::share_today`, off by default); when it is
//! off nothing here runs. A line is `HH:MM · <agent> · <folder name> · <one sentence>`:
//!
//! - the sentence is the turn summary's sentence (made from recorded facts), never raw chat text;
//! - the folder is a name, never a path;
//! - the whole line goes through `redact::text`, so a key that slipped into a sentence is masked
//!   before it reaches the disk (the bridge and the memory routes only ever serve redacted text);
//! - a helper (a pane another agent started) is `↳ <parent> › <agent>`;
//! - companion panes (this one and the others) never appear.
//!
//! The first line of the file says which local day it holds. When the day changes, the old text is
//! copied to `days/YYYY-MM-DD.md` and `today.md` starts again. Nothing is ever deleted: old day files
//! stay where they are, and the brief only tells the companion to look at the last 7.
//!
//! Pure except for the file functions, and those take the folder; the clock is passed in as values.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::host::FinishedTurn;
use super::schedule;
use super::service::is_pane_id;

pub const FILE_NAME: &str = "today.md";
pub const DAYS_DIR: &str = "days";
/// A sentence is cut to this many characters.
pub const SENTENCE_CAP: usize = 200;

const HEADER_PREFIX: &str = "# What my agents did today · ";

/// The first line of `today.md` for a local day (`YYYY-MM-DD`).
pub fn header(day: &str) -> String {
    format!("{HEADER_PREFIX}{day}")
}

/// The day a `today.md` first line names.
pub fn header_day(first_line: &str) -> Option<String> {
    let day = first_line.trim().strip_prefix(HEADER_PREFIX)?.trim();
    let ok = day.len() == 10
        && day.char_indices().all(|(i, c)| if i == 4 || i == 7 { c == '-' } else { c.is_ascii_digit() });
    ok.then(|| day.to_string())
}

pub fn path(home: &Path) -> PathBuf {
    home.join(FILE_NAME)
}

/// Whether a finished turn belongs in the file: another agent's pane (a companion's own pane never
/// does), a turn that really ended (a pane waiting at an approval will end later), with a sentence.
pub fn include(turn: &FinishedTurn) -> bool {
    !is_pane_id(&turn.pane_id) && turn.end != "waiting_input" && !turn.sentence.trim().is_empty()
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The line for one turn, redacted. `offset_secs` is the local zone.
pub fn line(turn: &FinishedTurn, offset_secs: i32) -> String {
    let time = format!("{:0>5}", schedule::clock_text(turn.ended_at_ms, offset_secs));
    let agent = match &turn.parent_label {
        Some(parent) => format!("↳ {} › {}", one_line(parent), one_line(&turn.agent)),
        None => one_line(&turn.agent),
    };
    let folder = match one_line(&turn.folder_name) {
        name if name.is_empty() => "unknown folder".to_string(),
        name => name,
    };
    let sentence: String = one_line(&turn.sentence).chars().take(SENTENCE_CAP).collect();
    let raw = format!("{time} · {agent} · {folder} · {sentence}");
    crate::redact::text(&raw).into_inner()
}

/// Local midnight of the day `now_ms` is in (epoch ms).
pub fn day_start_ms(now_ms: i64, offset_secs: i32) -> i64 {
    let date = schedule::local_date(now_ms, offset_secs);
    schedule::instant_on(date, super::model::HhMm { h: 0, m: 0 }, offset_secs).unwrap_or(now_ms)
}

fn write_atomic(target: &Path, text: &str) -> io::Result<()> {
    let tmp = PathBuf::from(format!("{}.pm-tmp", target.display()));
    fs::write(&tmp, text)?;
    fs::rename(&tmp, target).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// Makes sure `today.md` holds `day`: creates it with its first line, or, when it holds an earlier
/// day, copies that text to `days/<that day>.md` and starts the new day. Returns true when the file
/// was started or rolled over.
pub fn roll_over(home: &Path, day: &str) -> io::Result<bool> {
    fs::create_dir_all(home)?;
    let file = path(home);
    let old = match fs::read_to_string(&file) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    if let Some(text) = &old {
        let first = text.lines().next().unwrap_or("");
        if header_day(first).as_deref() == Some(day) {
            return Ok(false);
        }
        if !text.trim().is_empty() {
            archive(home, header_day(first), day, text)?;
        }
    }
    write_atomic(&file, &format!("{}\n", header(day)))?;
    Ok(true)
}

/// Copies an earlier day's text into `days/`. A day that already has a file gets the lines added to it.
fn archive(home: &Path, old_day: Option<String>, today: &str, text: &str) -> io::Result<()> {
    let dir = home.join(DAYS_DIR);
    fs::create_dir_all(&dir)?;
    let name = match old_day {
        Some(day) => format!("{day}.md"),
        None => format!("undated-{today}.md"),
    };
    let target = dir.join(name);
    match fs::read_to_string(&target) {
        Ok(existing) => {
            // The day already has a file (the clock moved back, or a copy was made by hand): add the
            // lines that are not in it yet, keep its own first line.
            let known: HashSet<&str> = existing.lines().collect();
            let mut merged = existing.trim_end().to_string();
            for line in text.lines().skip(1).filter(|l| !l.trim().is_empty() && !known.contains(l)) {
                merged.push('\n');
                merged.push_str(line);
            }
            merged.push('\n');
            write_atomic(&target, &merged)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => write_atomic(&target, text),
        Err(e) => Err(e),
    }
}

/// The lines (not the first line) already in `today.md`.
pub fn existing_lines(home: &Path) -> HashSet<String> {
    fs::read_to_string(path(home))
        .map(|text| text.lines().skip(1).filter(|l| !l.trim().is_empty()).map(str::to_string).collect())
        .unwrap_or_default()
}

/// Adds lines to the end of `today.md` (which `roll_over` has made).
pub fn append(home: &Path, lines: &[String]) -> io::Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    let file = path(home);
    let mut text = fs::read_to_string(&file)?;
    if !text.ends_with('\n') {
        text.push('\n');
    }
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    write_atomic(&file, &text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn turn(pane: &str, sentence: &str) -> FinishedTurn {
        FinishedTurn {
            pane_id: pane.into(),
            turn: 1,
            ended_at_ms: chrono::Utc.with_ymd_and_hms(2026, 10, 28, 9, 5, 0).unwrap().timestamp_millis(),
            end: "idle".into(),
            agent: "Codex".into(),
            folder_name: "my-app".into(),
            parent_label: None,
            sentence: sentence.into(),
        }
    }

    fn temp() -> PathBuf {
        super::super::test_host::temp_dir("pm-today")
    }

    #[test]
    fn a_line_is_time_agent_folder_sentence() {
        let line = line(&turn("pane-1", "Changed 3 files and the checks passed."), 0);
        assert_eq!(line, "09:05 · Codex · my-app · Changed 3 files and the checks passed.");
    }

    #[test]
    fn the_time_is_local_and_padded() {
        // UTC+8: 09:05 UTC is 17:05; UTC-5 shows 04:05 with a leading zero.
        assert!(line(&turn("p", "x"), 8 * 3600).starts_with("17:05 · "));
        assert!(line(&turn("p", "x"), -5 * 3600).starts_with("04:05 · "));
    }

    #[test]
    fn a_helper_names_its_parent() {
        let mut t = turn("pane-2", "Read the logs.");
        t.parent_label = Some("Claude Code".into());
        assert_eq!(line(&t, 0), "09:05 · ↳ Claude Code › Codex · my-app · Read the logs.");
    }

    #[test]
    fn newlines_and_long_sentences_stay_on_one_line() {
        let long = format!("Did a\nthing {}", "x".repeat(400));
        let l = line(&turn("p", &long), 0);
        assert!(!l.contains('\n'));
        assert!(l.chars().count() < 60 + SENTENCE_CAP);
        assert!(l.contains("Did a thing"));
    }

    #[test]
    fn an_unknown_folder_gets_a_word_not_a_path() {
        let mut t = turn("p", "Done.");
        t.folder_name = String::new();
        assert!(line(&t, 0).contains(" · unknown folder · "));
    }

    #[test]
    fn a_key_in_the_sentence_is_masked() {
        // Built at run time so the secret scanners of this repository see no key-shaped literal.
        let key = format!("sk-ant-api03-{}", "Zq9Xv3LmN7pRt2YhB5cWd8KfJ4sG6aUe1oIxTn0V");
        let l = line(&turn("p", &format!("Set the key to {key} in the config.")), 0);
        assert!(!l.contains(&key));
        assert!(!l.contains("Zq9Xv3LmN7pRt2"));
    }

    #[test]
    fn companion_panes_and_unfinished_turns_are_left_out() {
        assert!(include(&turn("pane-1", "Done.")));
        assert!(!include(&turn("companion-mochi", "Done.")));
        assert!(!include(&turn("companion-bit", "Done.")));
        let mut waiting = turn("pane-1", "Waiting for approval.");
        waiting.end = "waiting_input".into();
        assert!(!include(&waiting));
        assert!(!include(&turn("pane-1", "   ")));
        let mut failed = turn("pane-1", "It stopped with an error.");
        failed.end = "error".into();
        assert!(include(&failed));
    }

    #[test]
    fn header_round_trips_and_rejects_junk() {
        assert_eq!(header_day(&header("2026-10-28")).as_deref(), Some("2026-10-28"));
        assert_eq!(header_day("# something else"), None);
        assert_eq!(header_day(&header("28-10-2026")), None);
        assert_eq!(header_day(""), None);
    }

    #[test]
    fn midnight_is_local() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 10, 28, 20, 0, 0).unwrap().timestamp_millis();
        // UTC+8: 20:00 UTC is 04:00 on the 29th, so local midnight was 16:00 UTC on the 28th.
        let start = day_start_ms(now, 8 * 3600);
        assert_eq!(start, chrono::Utc.with_ymd_and_hms(2026, 10, 28, 16, 0, 0).unwrap().timestamp_millis());
        assert_eq!(day_start_ms(now, 0), chrono::Utc.with_ymd_and_hms(2026, 10, 28, 0, 0, 0).unwrap().timestamp_millis());
    }

    #[test]
    fn roll_over_creates_the_file_then_leaves_the_same_day_alone() {
        let home = temp();
        assert!(roll_over(&home, "2026-10-28").unwrap());
        assert_eq!(fs::read_to_string(path(&home)).unwrap(), format!("{}\n", header("2026-10-28")));
        append(&home, &["09:05 · Codex · app · Done.".to_string()]).unwrap();
        assert!(!roll_over(&home, "2026-10-28").unwrap());
        assert!(fs::read_to_string(path(&home)).unwrap().contains("Done."));
        assert!(!home.join(DAYS_DIR).exists());
    }

    #[test]
    fn a_new_day_moves_yesterday_into_days_and_starts_again() {
        let home = temp();
        roll_over(&home, "2026-10-28").unwrap();
        append(&home, &["09:05 · Codex · app · Done.".to_string()]).unwrap();
        assert!(roll_over(&home, "2026-10-29").unwrap());
        let moved = fs::read_to_string(home.join(DAYS_DIR).join("2026-10-28.md")).unwrap();
        assert!(moved.contains("09:05 · Codex · app · Done."));
        assert_eq!(fs::read_to_string(path(&home)).unwrap(), format!("{}\n", header("2026-10-29")));
        assert!(home.join(DAYS_DIR).join("2026-10-28.md").exists());
    }

    #[test]
    fn rolling_over_never_deletes_and_adds_to_an_existing_day_file() {
        let home = temp();
        fs::create_dir_all(home.join(DAYS_DIR)).unwrap();
        fs::write(home.join(DAYS_DIR).join("2026-10-28.md"), format!("{}\n08:00 · A · b · First.\n", header("2026-10-28"))).unwrap();
        fs::write(path(&home), format!("{}\n08:00 · A · b · First.\n09:00 · A · b · Second.\n", header("2026-10-28"))).unwrap();
        roll_over(&home, "2026-10-29").unwrap();
        let day = fs::read_to_string(home.join(DAYS_DIR).join("2026-10-28.md")).unwrap();
        assert_eq!(day.matches("First.").count(), 1);
        assert!(day.contains("Second."));
    }

    #[test]
    fn a_file_with_no_known_first_line_is_kept_under_an_undated_name() {
        let home = temp();
        fs::write(path(&home), "notes by hand\n").unwrap();
        roll_over(&home, "2026-10-29").unwrap();
        assert!(fs::read_to_string(home.join(DAYS_DIR).join("undated-2026-10-29.md")).unwrap().contains("notes by hand"));
    }

    #[test]
    fn existing_lines_skip_the_first_line() {
        let home = temp();
        roll_over(&home, "2026-10-28").unwrap();
        append(&home, &["a".to_string(), "b".to_string()]).unwrap();
        let known = existing_lines(&home);
        assert_eq!(known.len(), 2);
        assert!(known.contains("a") && known.contains("b"));
    }
}
