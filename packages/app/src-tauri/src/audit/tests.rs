use super::*;
use std::fs;

fn temp_log(name: &str) -> AuditLog {
    let dir = std::env::temp_dir().join(format!("pm-audit-{name}-{}-{}", std::process::id(), uuid::Uuid::new_v4()));
    AuditLog::new(dir)
}

const T0: u64 = 1_791_979_200_000; // 2026-10-14 12:00 UTC

fn write_five(log: &AuditLog) {
    for i in 0..5 {
        log.append(Kind::Approval, &Who::Desktop, &format!("Allowed command {i}"), json!({ "n": i }), T0 + i).unwrap();
    }
}

fn file_of(log: &AuditLog) -> std::path::PathBuf {
    log.dir().join("2026-10.jsonl")
}

#[test]
fn a_fresh_log_chains_from_genesis_and_verifies() {
    let log = temp_log("chain");
    write_five(&log);
    let report = log.verify();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.entries, 5);
    let recent = log.recent(3);
    assert_eq!(recent.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![5, 4, 3], "newest first");
    let first: Entry = serde_json::from_str(fs::read_to_string(file_of(&log)).unwrap().lines().next().unwrap()).unwrap();
    assert_eq!(first.prev, GENESIS);
    assert_eq!(recent[0].prev, recent[1].hash, "each line points at the one before");
}

#[test]
fn a_reopened_log_continues_the_same_chain() {
    let log = temp_log("reopen");
    write_five(&log);
    let again = AuditLog::new(log.dir().to_path_buf());
    again.append(Kind::TakeOver, &Who::Agent("session-abcdef123".into()), "Took over a pane", json!({}), T0 + 10).unwrap();
    let report = again.verify();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.entries, 6);
    assert_eq!(again.recent(1)[0].actor.as_deref(), Some("session-"), "an agent is identified by a short id only");
}

#[test]
fn an_edited_line_is_detected_at_that_line() {
    let log = temp_log("edit");
    write_five(&log);
    let path = file_of(&log);
    let text = fs::read_to_string(&path).unwrap();
    let tampered = text.replace("Allowed command 2", "Denied command 2");
    assert_ne!(text, tampered);
    fs::write(&path, tampered).unwrap();
    let report = log.verify();
    assert!(!report.ok);
    let broken = report.broken.expect("a broken line is named");
    assert_eq!(broken.line, 3);
    assert_eq!(report.entries, 2, "the two lines before it still checked out");
    assert!(broken.reason.contains("changed"), "{}", broken.reason);
}

#[test]
fn a_rewritten_line_with_a_fixed_hash_breaks_the_next_link() {
    let log = temp_log("rehash");
    write_five(&log);
    let path = file_of(&log);
    let mut lines: Vec<String> = fs::read_to_string(&path).unwrap().lines().map(str::to_string).collect();
    let mut entry: Entry = serde_json::from_str(&lines[1]).unwrap();
    entry.summary = "Allowed nothing at all".into();
    entry.hash = entry_hash(&entry);
    lines[1] = serde_json::to_string(&entry).unwrap();
    fs::write(&path, lines.join("\n") + "\n").unwrap();
    let report = log.verify();
    assert!(!report.ok);
    assert_eq!(report.broken.unwrap().line, 3, "the line after the rewrite no longer matches");
}

#[test]
fn a_removed_line_is_detected() {
    let log = temp_log("remove");
    write_five(&log);
    let path = file_of(&log);
    let kept: Vec<String> = fs::read_to_string(&path).unwrap().lines().enumerate().filter(|(i, _)| *i != 2).map(|(_, l)| l.to_string()).collect();
    fs::write(&path, kept.join("\n") + "\n").unwrap();
    let report = log.verify();
    assert!(!report.ok);
    assert_eq!(report.broken.unwrap().line, 3);
}

#[test]
fn a_removed_tail_is_detected_through_the_head_file() {
    let log = temp_log("tail");
    write_five(&log);
    let path = file_of(&log);
    let kept: Vec<String> = fs::read_to_string(&path).unwrap().lines().take(3).map(str::to_string).collect();
    fs::write(&path, kept.join("\n") + "\n").unwrap();
    let report = log.verify();
    assert!(!report.ok, "dropping the newest lines must not look like a clean log");
    assert!(report.broken.unwrap().reason.contains("missing"));
}

#[test]
fn the_chain_runs_across_months() {
    let log = temp_log("months");
    log.append(Kind::Pairing, &Who::Desktop, "Made a pairing code", json!({}), T0).unwrap();
    log.append(Kind::Pairing, &Who::Device(Some("dev-1".into())), "A phone paired", json!({}), 1_793_491_201_000).unwrap();
    assert!(log.dir().join("2026-10.jsonl").exists() && log.dir().join("2026-11.jsonl").exists());
    let report = log.verify();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.entries, 2);
    // Editing the first month is caught when the second month's link no longer matches.
    let first = log.dir().join("2026-10.jsonl");
    fs::write(&first, fs::read_to_string(&first).unwrap().replace("Made a pairing code", "Made nothing")).unwrap();
    assert!(!log.verify().ok);
}

#[test]
fn an_empty_or_missing_folder_is_a_valid_empty_log() {
    let log = temp_log("empty");
    assert!(log.verify().ok);
    assert!(log.recent(10).is_empty());
}

#[test]
fn month_names_come_from_utc_dates() {
    assert_eq!(month_of(0), "1970-01");
    assert_eq!(month_of(T0), "2026-10");
    assert_eq!(month_of(1_793_491_201_000), "2026-11");
    assert_eq!(month_of(1_709_251_140_000), "2024-02", "a leap day");
}

#[test]
fn a_command_is_recorded_as_a_fingerprint_not_as_text() {
    let fingerprint = command_fingerprint("C:\\tools\\git.exe push --token=ghp_SECRETSECRET origin main");
    let text = fingerprint.to_string();
    assert!(!text.contains("SECRET") && !text.contains("origin"), "{text}");
    assert_eq!(fingerprint["program"], "git.exe");
    assert!(fingerprint["sha256_prefix"].as_str().is_some_and(|h| h.len() == 8));
}

#[test]
fn a_long_summary_is_cut() {
    let log = temp_log("long");
    let entry = log.append(Kind::Setting, &Who::Desktop, &"x".repeat(5_000), json!({}), T0).unwrap();
    assert_eq!(entry.summary.chars().count(), MAX_SUMMARY_CHARS);
    assert!(log.verify().ok);
}

/// Review R12 3.4: each `AuditLog` cached its own tip, so two writers on one folder (two app
/// instances, or a second recorder) wrote the same `seq` twice and the check reported tampering.
#[test]
fn two_writers_on_one_folder_keep_one_chain() {
    let a = temp_log("two-writers");
    let b = AuditLog::new(a.dir().to_path_buf());
    for i in 0..3 {
        a.append(Kind::Approval, &Who::Desktop, "from a", json!({ "i": i }), T0 + 10 * i).unwrap();
        b.append(Kind::Approval, &Who::Desktop, "from b", json!({ "i": i }), T0 + 10 * i + 5).unwrap();
    }
    let report = a.verify();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.entries, 6);
}

/// Review R12 3.4: the month file came from the entry's own clock, so a clock that moved back
/// across a month boundary appended seq N+1 to the earlier month's file, which sorts first.
#[test]
fn a_clock_that_moves_back_across_a_month_boundary_does_not_break_the_chain() {
    let log = temp_log("clock-back");
    let nov_1 = 1_793_491_200_000; // 2026-11-01 00:00 UTC
    assert_eq!(month_of(nov_1 + 1_000), "2026-11");
    log.append(Kind::Approval, &Who::Desktop, "after midnight", json!({}), nov_1 + 1_000).unwrap();
    log.append(Kind::Approval, &Who::Desktop, "clock stepped back", json!({}), nov_1 - 60_000).unwrap();
    let report = log.verify();
    assert!(report.ok, "{report:?}");
    assert_eq!(report.entries, 2);
    assert!(!log.dir().join("2026-10.jsonl").exists(), "nothing went back into October's file");
}

#[test]
fn keep_going_hand_over_has_its_own_audit_kind() {
    assert_eq!(Kind::KeepGoing.as_str(), "keep_going.handed_over");
}
