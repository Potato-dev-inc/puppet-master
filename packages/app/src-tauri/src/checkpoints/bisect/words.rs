//! Every user-facing string of What broke it (SPEC n33 section 3.7), in one place, so the copy is checked
//! once. Functions take the numbers and names they need and return the plain English text.
//!
//! Copy rules: say "save point", "turn", "undo only this turn", "separate copy". Never "bisect", "commit",
//! "ref", "tree", "HEAD" or "sentinel" (a test below checks this). Generic code passes agent labels in; this
//! file never names an agent. The label `UNKNOWN_AGENT` is used when the turn summary is gone.

use super::model::{FileStatsLite, Limits, UnsureWhy};

/// The label of a turn whose summary is gone (SPEC n33 section 3.2).
pub const UNKNOWN_AGENT: &str = "An agent";

/// Longest turn sentence shown in a subtitle. Longer ones are cut with an ellipsis.
const MAX_ASK_CHARS: usize = 80;

fn plural(n: u32, one: &str, many: &str) -> String {
    if n == 1 {
        format!("{n} {one}")
    } else {
        format!("{n} {many}")
    }
}

/// `1 file` or `4 files`.
pub fn count_files(n: u32) -> String {
    plural(n, "file", "files")
}

/// `4 files, +52 −20`.
pub fn files_summary(stats: &FileStatsLite) -> String {
    format!("{}, +{} −{}", count_files(stats.count), stats.added, stats.deleted)
}

/// Cut a sentence to `MAX_ASK_CHARS` characters, with an ellipsis when it was cut.
fn shorten(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= MAX_ASK_CHARS {
        return trimmed.to_string();
    }
    let kept: String = trimmed.chars().take(MAX_ASK_CHARS).collect();
    format!("{}…", kept.trim_end())
}

/// The agent's label as a noun phrase in a sentence, or "the agent" when the label is unknown.
fn named(agent: &str) -> &str {
    if agent == UNKNOWN_AGENT {
        "the agent"
    } else {
        agent
    }
}

/// `Turn 7 by <label> broke the tests`. With an unknown label: `Turn 7 broke the tests`.
pub fn found_title(agent: &str, turn: u32) -> String {
    if agent == UNKNOWN_AGENT {
        format!("Turn {turn} broke the tests")
    } else {
        format!("Turn {turn} by {agent} broke the tests")
    }
}

/// `14:10 · "refactor cart" · 4 files, +52 −20`. The ask and the file counts are left out when missing.
pub fn found_subtitle(time: &str, ask: Option<&str>, files: Option<&FileStatsLite>) -> String {
    let mut parts = vec![time.to_string()];
    if let Some(ask) = ask.map(str::trim).filter(|ask| !ask.is_empty()) {
        parts.push(format!("\"{}\"", shorten(ask)));
    }
    if let Some(stats) = files {
        parts.push(files_summary(stats));
    }
    parts.join(" · ")
}

/// `It broke cart.test.ts and 1 more`, from the failing test names (already capped by `failnames`).
pub fn broke_line(failing: &[String]) -> String {
    match failing {
        [] => "No test name was printed.".to_string(),
        [one] => format!("It broke {one}"),
        [first, rest @ ..] => format!("It broke {first} and {} more", rest.len()),
    }
}

/// `Last time they passed: 13:42`.
pub fn last_good_line(time: &str) -> String {
    format!("Last time they passed: {time}")
}

/// `Undone at 14:31.`
pub fn undone_at(time: &str) -> String {
    format!("Undone at {time}.")
}

pub fn show_changes() -> &'static str {
    "Show what it changed"
}

pub fn show_what_changed() -> &'static str {
    "Show what changed"
}

pub fn undo_only_this_turn() -> &'static str {
    "Undo only this turn"
}

pub fn copy_details() -> &'static str {
    "Copy details"
}

pub fn check_it_again() -> &'static str {
    "Check it again"
}

pub fn keep_looking() -> &'static str {
    "Keep looking"
}

/// `Undo only turn 7?`
pub fn undo_title(turn: u32) -> String {
    format!("Undo only turn {turn}?")
}

/// The undo confirm text (SPEC n33 section 3.7 item 3).
pub fn undo_text(turn: u32, files: u32, agent: &str) -> String {
    format!(
        "This puts back what turn {turn} changed in {}. Work from later turns stays. It doesn't change what {} remembers. A save point is made first, so you can undo this.",
        count_files(files),
        named(agent)
    )
}

/// The memory line shared by the undo confirm and the agents' rules.
pub fn memory_note() -> &'static str {
    "This puts back your files. It doesn't change what the agent remembers."
}

/// `Undo turn 7`.
pub fn undo_button(turn: u32) -> String {
    format!("Undo turn {turn}")
}

pub fn cancel() -> &'static str {
    "Cancel"
}

/// The refusal that replaces the confirm text. Names the first file, and the count of the rest.
pub fn undo_refused(files: &[String]) -> String {
    match files {
        [] => "Can't undo only this turn. A later change touched the same lines. Nothing was changed.".to_string(),
        [one] => format!("Can't undo only this turn. A later change touched the same lines in {one}. Nothing was changed."),
        [first, rest @ ..] => format!(
            "Can't undo only this turn. A later change touched the same lines in {first} and {} more. Nothing was changed.",
            rest.len()
        ),
    }
}

/// The refusal while an agent in the folder is still working.
pub fn undo_running() -> &'static str {
    "An agent is still working in this folder. Undo waits until it stops."
}

/// The save point of the turn was cleaned up.
pub fn undo_gone() -> &'static str {
    "That save point was cleaned up. Undo isn't possible."
}

/// The finding is not a single turn that broke the tests.
pub fn not_undoable() -> &'static str {
    "Only a turn that broke the tests can be undone here."
}

/// The audit line of an undo (`what_broke_it.undone`).
pub fn undo_audit_summary(turn: u32, agent: &str, folder: &str) -> String {
    format!("Undid turn {turn} of {} in {folder}", named(agent))
}

/// The intro of the `Several` result: two turns get the plain words, more get a count-free sentence.
pub fn several_intro(turns: usize) -> String {
    if turns == 2 {
        "Two agents were working at the same time. It was one of these:".to_string()
    } else {
        "Several agent turns were working at the same time. It was one of these:".to_string()
    }
}

pub fn cant_tell_which() -> &'static str {
    "I can't tell which one."
}

/// The `BetweenTurns` result. `good_time` and `bad_time` are the clock times of the last good and first bad points.
pub fn between(good_time: &str, bad_time: &str) -> String {
    format!(
        "It broke between turns, not during one. Something changed the files between {good_time} and {bad_time}: you, an install, or a tool."
    )
}

/// The reason of a `NotSure` result (the title is `not_sure`).
pub fn unsure_reason(why: UnsureWhy) -> &'static str {
    match why {
        UnsureWhy::Flaky => "The tests pass and fail on the same files (they are flaky).",
        UnsureWhy::TimedOut => "A test run took too long.",
        UnsureWhy::NotStarted => "The test command did not start.",
        UnsureWhy::SetupFailed => "Getting the copy ready failed.",
        UnsureWhy::Stopped => "Stopped.",
        UnsureWhy::OutOfTime => "Ran out of time.",
        UnsureWhy::OutOfRuns => "Ran out of runs.",
    }
}

/// `Not sure. Ran out of runs.`
pub fn not_sure(why: UnsureWhy) -> String {
    format!("Not sure. {}", unsure_reason(why))
}

/// `It was one of turns 5 to 8 (<label>, <label>).` The agent labels are listed once each, in order.
pub fn narrowed(from_turn: u32, to_turn: u32, agents: &[String]) -> String {
    let range = format!("It was one of turns {from_turn} to {to_turn}");
    let mut names: Vec<&str> = Vec::new();
    for agent in agents {
        if !names.contains(&agent.as_str()) {
            names.push(agent.as_str());
        }
    }
    if names.is_empty() {
        format!("{range}.")
    } else {
        format!("{range} ({}).", names.join(", "))
    }
}

/// `The tests were already failing at the oldest save point (11:02). I can't look further back.`
pub fn always_red(time: &str) -> String {
    format!("The tests were already failing at the oldest save point ({time}). I can't look further back.")
}

/// `The tests also fail at the save point you picked (11:02). Pick an earlier one, or choose Find it for me.`
pub fn good_was_red(time: &str) -> String {
    format!(
        "The tests also fail at the save point you picked ({time}). Pick an earlier one, or choose Find it for me."
    )
}

/// The clean-copy sentence of a `TestsPass` result (SPEC n33 section 3.5 step 5).
pub fn tests_pass_note() -> &'static str {
    "The tests pass in a clean copy. If they fail in your folder they may need files that save points don't keep (installed packages, .env). Add setup steps for the copy in Settings > Copies."
}

/// The note of an `AlwaysRed` result when no setup steps are set.
pub fn no_setup_note() -> &'static str {
    "No setup steps are set for copies, so an old red result may be missing packages."
}

pub fn open_copies_settings() -> &'static str {
    "Open Copies settings"
}

/// `There is only one save point so far.`
pub fn too_few() -> &'static str {
    "There is only one save point so far."
}

/// `Already looking in my-app.` (`folder` is the folder's last name).
pub fn busy(folder: &str) -> String {
    format!("Already looking in {folder}.")
}

pub fn no_test() -> &'static str {
    "Declare a test command first."
}

pub fn bad_check() -> &'static str {
    "That test command is not on the list."
}

pub fn bad_id() -> &'static str {
    "That result id is not valid."
}

/// `npm was not found on this PC.`
pub fn program_not_found(program: &str) -> String {
    format!("{program} was not found on this PC.")
}

/// `Getting the copy ready failed: npm ci.`
pub fn setup_failed(step: &str) -> String {
    format!("Getting the copy ready failed: {step}.")
}

pub fn too_big() -> &'static str {
    "This folder is too big to copy."
}

pub fn copy_unsafe() -> &'static str {
    "The test copy would be inside your folder. Nothing was run."
}

/// `Trying save point 4 of 9 · run 3 of 16 · 2 min`.
pub fn progress(point: u32, points: u32, run: u32, runs: u32, minutes: u32) -> String {
    format!("Trying save point {point} of {points} · run {run} of {runs} · {minutes} min")
}

pub fn running_note() -> &'static str {
    "Your agents can keep working. This runs at low priority."
}

pub fn setup_intro() -> &'static str {
    "Puppet Master runs your test command on older save points, in a separate copy. Your folder is not touched."
}

pub fn safety_note() -> &'static str {
    "Tests can still reach things outside the folder, like the network, the same way they do when you run them."
}

/// `Stops after 16 runs or 20 minutes.`
pub fn limits_line(limits: Limits) -> String {
    format!(
        "Stops after {} or {}.",
        plural(limits.max_runs, "run", "runs"),
        plural(limits.max_minutes, "minute", "minutes")
    )
}

pub fn what_broke_the_tests() -> &'static str {
    "What broke the tests?"
}

pub fn palette_item() -> &'static str {
    "Find what broke the tests…"
}

pub fn palette_desc() -> &'static str {
    "Look back through the agents' turns."
}

pub fn find_it() -> &'static str {
    "Find it"
}

pub fn find_it_for_me() -> &'static str {
    "Find it for me"
}

pub fn tests_last_passed() -> &'static str {
    "Tests last passed"
}

pub fn test_command_label() -> &'static str {
    "Test command"
}

pub fn change() -> &'static str {
    "Change"
}

pub fn use_this() -> &'static str {
    "Use this"
}

pub fn type_a_command() -> &'static str {
    "Type a command, like npm test"
}

pub fn stop() -> &'static str {
    "Stop"
}

/// `Broke the tests 2 times` / `Broke the tests 1 time`.
pub fn broke_tests_line(times: u32) -> String {
    format!("Broke the tests {}", plural(times, "time", "times"))
}

pub fn conflict_same_lines() -> &'static str {
    "Later work changed the same lines"
}

pub fn conflict_deleted_later() -> &'static str {
    "Later work deleted this file"
}

pub fn conflict_binary() -> &'static str {
    "This is a binary file that changed later"
}

pub fn conflict_unreadable() -> &'static str {
    "The file could not be read"
}

pub fn conflict_created_then_edited() -> &'static str {
    "The turn created this file and later work edited it"
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every string the window can show, built with sample numbers and names.
    fn all_copy() -> Vec<String> {
        let stats = FileStatsLite { count: 4, added: 52, deleted: 20 };
        let names = vec!["cart.test.ts".to_string(), "b.test.ts".to_string(), "c.test.ts".to_string()];
        let agents = vec!["Codex".to_string(), "Claude Code".to_string()];
        let mut out: Vec<String> = vec![
            found_title("Codex", 7),
            found_title(UNKNOWN_AGENT, 7),
            found_subtitle("14:10", Some("tidy the cart"), Some(&stats)),
            files_summary(&stats),
            broke_line(&names),
            broke_line(&[]),
            last_good_line("13:42"),
            undone_at("14:31"),
            undo_title(7),
            undo_text(7, 4, "Codex"),
            undo_text(7, 1, UNKNOWN_AGENT),
            memory_note().to_string(),
            undo_button(7),
            undo_refused(&names),
            undo_refused(&[]),
            undo_audit_summary(7, "Codex", "my-app"),
            several_intro(2),
            several_intro(3),
            between("13:42", "14:02"),
            not_sure(UnsureWhy::Flaky),
            narrowed(5, 8, &agents),
            always_red("11:02"),
            good_was_red("11:02"),
            tests_pass_note().to_string(),
            no_setup_note().to_string(),
            busy("my-app"),
            program_not_found("npm"),
            setup_failed("npm ci"),
            progress(4, 9, 3, 16, 2),
            limits_line(Limits::default()),
            broke_tests_line(2),
            conflict_same_lines().to_string(),
            conflict_deleted_later().to_string(),
            conflict_binary().to_string(),
            conflict_unreadable().to_string(),
            conflict_created_then_edited().to_string(),
            copy_unsafe().to_string(),
            bad_check().to_string(),
            bad_id().to_string(),
            too_big().to_string(),
            too_few().to_string(),
            no_test().to_string(),
            undo_running().to_string(),
            undo_gone().to_string(),
            not_undoable().to_string(),
            safety_note().to_string(),
            setup_intro().to_string(),
            running_note().to_string(),
        ];
        for why in [
            UnsureWhy::Flaky,
            UnsureWhy::TimedOut,
            UnsureWhy::NotStarted,
            UnsureWhy::SetupFailed,
            UnsureWhy::Stopped,
            UnsureWhy::OutOfTime,
            UnsureWhy::OutOfRuns,
        ] {
            out.push(not_sure(why));
        }
        for fixed in [
            show_changes(),
            show_what_changed(),
            undo_only_this_turn(),
            copy_details(),
            check_it_again(),
            keep_looking(),
            cancel(),
            open_copies_settings(),
            what_broke_the_tests(),
            palette_item(),
            palette_desc(),
            find_it(),
            find_it_for_me(),
            tests_last_passed(),
            test_command_label(),
            change(),
            use_this(),
            type_a_command(),
            stop(),
            cant_tell_which(),
        ] {
            out.push(fixed.to_string());
        }
        out
    }

    #[test]
    fn no_string_says_a_forbidden_word() {
        for text in all_copy() {
            let lower = text.to_lowercase();
            for word in ["bisect", "commit", "ref", "tree", "sentinel"] {
                assert!(!lower.contains(word), "{word} in: {text}");
            }
            assert!(!text.contains("HEAD"), "HEAD in: {text}");
        }
    }

    #[test]
    fn numbers_and_names_are_inserted() {
        assert_eq!(found_title("Codex", 7), "Turn 7 by Codex broke the tests");
        assert_eq!(undo_title(7), "Undo only turn 7?");
        assert_eq!(undo_button(7), "Undo turn 7");
        assert_eq!(undone_at("14:31"), "Undone at 14:31.");
        assert_eq!(
            progress(4, 9, 3, 16, 2),
            "Trying save point 4 of 9 · run 3 of 16 · 2 min"
        );
        assert_eq!(between("13:42", "14:02"), "It broke between turns, not during one. Something changed the files between 13:42 and 14:02: you, an install, or a tool.");
        assert_eq!(always_red("11:02"), "The tests were already failing at the oldest save point (11:02). I can't look further back.");
        assert_eq!(good_was_red("11:02"), "The tests also fail at the save point you picked (11:02). Pick an earlier one, or choose Find it for me.");
        assert_eq!(busy("my-app"), "Already looking in my-app.");
        assert_eq!(setup_failed("npm ci"), "Getting the copy ready failed: npm ci.");
        assert_eq!(program_not_found("npm"), "npm was not found on this PC.");
        assert_eq!(undo_audit_summary(7, "Codex", "my-app"), "Undid turn 7 of Codex in my-app");
    }

    #[test]
    fn counts_use_singular_and_plural() {
        assert_eq!(count_files(1), "1 file");
        assert_eq!(count_files(0), "0 files");
        assert_eq!(count_files(4), "4 files");
        assert_eq!(limits_line(Limits::clamped(16, 20)), "Stops after 16 runs or 20 minutes.");
        assert_eq!(limits_line(Limits::clamped(1, 1)), "Stops after 1 run or 1 minute.");
        assert_eq!(broke_tests_line(1), "Broke the tests 1 time");
        assert_eq!(broke_tests_line(2), "Broke the tests 2 times");
    }

    #[test]
    fn subtitle_leaves_out_missing_parts_and_cuts_long_asks() {
        let stats = FileStatsLite { count: 4, added: 52, deleted: 20 };
        assert_eq!(found_subtitle("14:10", Some("tidy the cart"), Some(&stats)), "14:10 · \"tidy the cart\" · 4 files, +52 −20");
        assert_eq!(found_subtitle("14:10", None, None), "14:10");
        assert_eq!(found_subtitle("14:10", Some("  "), None), "14:10");
        let long = "x".repeat(200);
        let cut = found_subtitle("14:10", Some(long.as_str()), None);
        assert!(cut.ends_with("…\""), "{cut}");
        assert!(cut.chars().count() < 100, "{cut}");
    }

    #[test]
    fn an_unknown_agent_reads_as_plain_words() {
        assert_eq!(found_title(UNKNOWN_AGENT, 7), "Turn 7 broke the tests");
        assert!(undo_text(7, 1, UNKNOWN_AGENT).contains("doesn't change what the agent remembers"));
    }

    #[test]
    fn broke_line_names_the_first_failure_and_counts_the_rest() {
        assert_eq!(broke_line(&["cart.test.ts".to_string()]), "It broke cart.test.ts");
        let two = vec!["cart.test.ts".to_string(), "b.test.ts".to_string()];
        assert_eq!(broke_line(&two), "It broke cart.test.ts and 1 more");
        assert_eq!(broke_line(&[]), "No test name was printed.");
    }

    #[test]
    fn narrowed_lists_each_agent_once() {
        let agents = vec!["Codex".to_string(), "Codex".to_string(), "Claude Code".to_string()];
        assert_eq!(narrowed(5, 8, &agents), "It was one of turns 5 to 8 (Codex, Claude Code).");
        assert_eq!(narrowed(5, 8, &[]), "It was one of turns 5 to 8.");
    }

    #[test]
    fn unsure_reasons_have_the_plain_copy() {
        assert_eq!(not_sure(UnsureWhy::Flaky), "Not sure. The tests pass and fail on the same files (they are flaky).");
        assert_eq!(not_sure(UnsureWhy::TimedOut), "Not sure. A test run took too long.");
        assert_eq!(not_sure(UnsureWhy::Stopped), "Not sure. Stopped.");
        assert_eq!(not_sure(UnsureWhy::OutOfTime), "Not sure. Ran out of time.");
        assert_eq!(not_sure(UnsureWhy::OutOfRuns), "Not sure. Ran out of runs.");
    }

    #[test]
    fn undo_refusal_names_the_first_file_and_the_rest() {
        let one = vec!["cart.ts".to_string()];
        assert_eq!(
            undo_refused(&one),
            "Can't undo only this turn. A later change touched the same lines in cart.ts. Nothing was changed."
        );
        let three = vec!["cart.ts".to_string(), "a.ts".to_string(), "b.ts".to_string()];
        assert!(undo_refused(&three).contains("in cart.ts and 2 more."));
    }

    #[test]
    fn found_subtitle_uses_the_turn_sentence_as_given() {
        assert_eq!(found_subtitle("14:10", Some("tidy the cart"), None), "14:10 · \"tidy the cart\"");
    }
}
