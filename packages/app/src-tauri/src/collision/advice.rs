//! Collision Alert: which side to finish first, by fixed rules (SPEC-n33 sections 3.5 and 5 CA-4).
//!
//! Pure: no process, no clock, no file. `advise` picks the side by three rules, first match wins.
//! `advice_text` and `FOOTER` are the plain sentences of the card. Names come in already cleaned
//! (`copy` cleans them); this module only puts them into the sentence.

use super::model::{Advice, Relation, SideFacts, Why, SMALLER_MIN_GAP, SMALLER_RATIO_PCT};

/// The last line of every card. Puppet Master never pauses, stops or merges anything.
pub const FOOTER: &str = "Nothing was paused. You decide.";

/// Which side to finish first: `first` is 0 for `a` and 1 for `b`.
///
/// Rules, first match wins:
/// 1. Exactly one side is not working: that one is first (`AlreadyDone`).
/// 2. `Copies` only: a side with at most `SMALLER_RATIO_PCT` percent of the other side's files,
///    and at least `SMALLER_MIN_GAP` files fewer, is first (`SmallerChange`). `SameFolder` skips
///    this rule, because file counts cannot be separated in one folder.
/// 3. The side that started first. Equal start times: the lower pane id (`StartedFirst`).
pub fn advise(a: &SideFacts, b: &SideFacts, relation: Relation) -> Advice {
    if a.working != b.working {
        let first = if a.working { 1 } else { 0 };
        return Advice { first, why: Why::AlreadyDone };
    }
    if relation == Relation::Copies {
        if smaller_than(a.files, b.files) {
            return Advice { first: 0, why: Why::SmallerChange };
        }
        if smaller_than(b.files, a.files) {
            return Advice { first: 1, why: Why::SmallerChange };
        }
    }
    Advice { first: started_first(a, b), why: Why::StartedFirst }
}

/// True when `small` is at most `SMALLER_RATIO_PCT` percent of `big`, and at least
/// `SMALLER_MIN_GAP` files fewer. Widened to u64 so no count can overflow the product.
fn smaller_than(small: u32, big: u32) -> bool {
    let (small, big) = (u64::from(small), u64::from(big));
    big >= small + u64::from(SMALLER_MIN_GAP) && small * 100 <= big * u64::from(SMALLER_RATIO_PCT)
}

/// The side that started first (0 or 1). Equal start times: the lower pane id.
fn started_first(a: &SideFacts, b: &SideFacts) -> usize {
    if a.started_ms != b.started_ms {
        return if a.started_ms < b.started_ms { 0 } else { 1 };
    }
    if a.pane_id <= b.pane_id {
        0
    } else {
        1
    }
}

/// The advice line of a card. `first` is the name of the side to finish first, `other` the name of
/// the side that waits. The `SameFolder` text is also used for the (unreachable) `SmallerChange`.
pub fn advice_text(relation: Relation, why: Why, first: &str, other: &str) -> String {
    match (relation, why) {
        (Relation::Copies, Why::AlreadyDone) => format!(
            "Finish {first} first. It has stopped working. Look at its changes and merge them first. Then {other} can build on it."
        ),
        (Relation::Copies, Why::SmallerChange) => format!(
            "Finish {first} first. It has changed fewer files, so it is quicker to finish. Merge it first. Then {other} can build on it."
        ),
        (Relation::Copies, Why::StartedFirst) => {
            format!("Finish {first} first. It started first. Merge it first. Then {other} can build on it.")
        }
        (Relation::SameFolder, Why::AlreadyDone) => format!(
            "{first} has stopped working. Ask {other} to read the file again before it changes more."
        ),
        (Relation::SameFolder, Why::SmallerChange | Why::StartedFirst) => format!(
            "Let {first} finish first. Then ask {other} to read the file again before it changes more."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side(pane_id: &str, working: bool, files: u32, started_ms: u64) -> SideFacts {
        SideFacts { pane_id: pane_id.to_string(), working, files, started_ms }
    }

    #[test]
    fn stopped_side_is_first_in_both_orders() {
        let stopped = side("fake_a", false, 9, 5);
        let working = side("fake_b", true, 1, 1);
        for relation in [Relation::Copies, Relation::SameFolder] {
            let first = advise(&stopped, &working, relation);
            assert_eq!(first, Advice { first: 0, why: Why::AlreadyDone });
            let swapped = advise(&working, &stopped, relation);
            assert_eq!(swapped, Advice { first: 1, why: Why::AlreadyDone });
        }
    }

    #[test]
    fn both_working_copies_four_vs_ten_is_smaller_change() {
        let four = side("fake_a", true, 4, 9);
        let ten = side("fake_b", true, 10, 1);
        assert_eq!(advise(&four, &ten, Relation::Copies), Advice { first: 0, why: Why::SmallerChange });
        assert_eq!(advise(&ten, &four, Relation::Copies), Advice { first: 1, why: Why::SmallerChange });
    }

    #[test]
    fn ratio_fails_eight_vs_ten_falls_to_started_first() {
        // 8 is 80% of 10: over the 70% limit, so the gap of 2 does not count.
        let eight = side("fake_a", true, 8, 2);
        let ten = side("fake_b", true, 10, 1);
        assert_eq!(advise(&eight, &ten, Relation::Copies), Advice { first: 1, why: Why::StartedFirst });
    }

    #[test]
    fn gap_fails_five_vs_six_falls_to_started_first() {
        // 5 is within 70% of 6, but only one file fewer: the gap of 2 fails.
        let five = side("fake_a", true, 5, 1);
        let six = side("fake_b", true, 6, 2);
        assert_eq!(advise(&five, &six, Relation::Copies), Advice { first: 0, why: Why::StartedFirst });
    }

    #[test]
    fn zero_vs_three_is_smaller_change() {
        let none = side("fake_a", true, 0, 4);
        let three = side("fake_b", true, 3, 4);
        assert_eq!(advise(&none, &three, Relation::Copies), Advice { first: 0, why: Why::SmallerChange });
        assert_eq!(advise(&three, &none, Relation::Copies), Advice { first: 1, why: Why::SmallerChange });
    }

    #[test]
    fn same_folder_never_uses_file_counts() {
        // The same 4 vs 10 as above, but in one folder: the started-first rule decides.
        let four = side("fake_a", true, 4, 9);
        let ten = side("fake_b", true, 10, 3);
        assert_eq!(advise(&four, &ten, Relation::SameFolder), Advice { first: 1, why: Why::StartedFirst });
    }

    #[test]
    fn equal_start_gives_the_lower_pane_id() {
        let b_pane = side("fake_b", true, 3, 7);
        let a_pane = side("fake_a", true, 3, 7);
        assert_eq!(advise(&b_pane, &a_pane, Relation::SameFolder), Advice { first: 1, why: Why::StartedFirst });
        assert_eq!(advise(&a_pane, &b_pane, Relation::SameFolder), Advice { first: 0, why: Why::StartedFirst });
    }

    #[test]
    fn both_stopped_uses_the_started_first_rule() {
        // Not exactly one side stopped, so rule 2 is checked first; 8 vs 10 fails it.
        let eight = side("fake_a", false, 8, 7);
        let ten = side("fake_b", false, 10, 4);
        assert_eq!(advise(&eight, &ten, Relation::Copies), Advice { first: 1, why: Why::StartedFirst });
    }

    #[test]
    fn advice_is_symmetric_for_the_same_pane() {
        let cases = [
            (side("fake_a", true, 4, 9), side("fake_b", true, 10, 1)),
            (side("fake_a", true, 0, 4), side("fake_b", true, 3, 4)),
            (side("fake_a", true, 8, 2), side("fake_b", true, 10, 1)),
            (side("fake_a", true, 5, 1), side("fake_b", true, 6, 2)),
            (side("fake_a", false, 1, 5), side("fake_b", true, 2, 1)),
            (side("fake_a", false, 8, 7), side("fake_b", false, 10, 4)),
            (side("fake_b", true, 3, 7), side("fake_a", true, 3, 7)),
        ];
        for (a, b) in &cases {
            for relation in [Relation::Copies, Relation::SameFolder] {
                let ab = advise(a, b, relation);
                let ba = advise(b, a, relation);
                let chosen_ab = if ab.first == 0 { &a.pane_id } else { &b.pane_id };
                let chosen_ba = if ba.first == 0 { &b.pane_id } else { &a.pane_id };
                assert_eq!(chosen_ab, chosen_ba, "{} vs {} ({relation:?})", a.pane_id, b.pane_id);
                assert_eq!(ab.why, ba.why, "{} vs {} ({relation:?})", a.pane_id, b.pane_id);
            }
        }
    }

    #[test]
    fn copies_advice_sentences_match_the_table() {
        assert_eq!(
            advice_text(Relation::Copies, Why::AlreadyDone, "Codex", "Claude Code"),
            "Finish Codex first. It has stopped working. Look at its changes and merge them first. Then Claude Code can build on it."
        );
        assert_eq!(
            advice_text(Relation::Copies, Why::SmallerChange, "Codex", "Claude Code"),
            "Finish Codex first. It has changed fewer files, so it is quicker to finish. Merge it first. Then Claude Code can build on it."
        );
        assert_eq!(
            advice_text(Relation::Copies, Why::StartedFirst, "Codex", "Claude Code"),
            "Finish Codex first. It started first. Merge it first. Then Claude Code can build on it."
        );
    }

    #[test]
    fn same_folder_advice_sentences_match_the_table() {
        assert_eq!(
            advice_text(Relation::SameFolder, Why::AlreadyDone, "Codex", "Claude Code"),
            "Codex has stopped working. Ask Claude Code to read the file again before it changes more."
        );
        assert_eq!(
            advice_text(Relation::SameFolder, Why::StartedFirst, "Codex", "Claude Code"),
            "Let Codex finish first. Then ask Claude Code to read the file again before it changes more."
        );
    }

    #[test]
    fn names_are_values_not_templates() {
        // A pane heading with braces must come out as typed, never be read as a placeholder.
        let text = advice_text(Relation::Copies, Why::StartedFirst, "{Y}", "{X}");
        assert_eq!(text, "Finish {Y} first. It started first. Merge it first. Then {X} can build on it.");
    }

    #[test]
    fn footer_is_the_exact_last_line_of_every_card() {
        assert_eq!(FOOTER, "Nothing was paused. You decide.");
        assert!(FOOTER.ends_with("You decide."));
    }

    #[test]
    fn advice_module_names_no_agent() {
        // C12: the code above the tests is generic. Labels come from the pane facts only.
        let source = include_str!("advice.rs");
        let code = source.split("#[cfg(test)]").next().unwrap_or("");
        let lower = code.to_ascii_lowercase();
        for word in ["claude", "codex", "cursor", "opencode", "agy"] {
            assert!(!lower.contains(word), "generic code names an agent: {word}");
        }
    }
}
