//! Collision Alert: the words of the cards and the banner (SPEC-n33 section 3.5). Pure.
//!
//! Each sentence is built with `format!` arguments or `push_str`, never by filling a template, so a
//! name with `{` or `}` in it is printed exactly as typed. Names and paths arrive already scrubbed
//! (`view` does that). The advice sentences and `FOOTER` live in `advice.rs`: `advice_line` and
//! `footer` call them and repeat none of their words.

use super::advice::{self, FOOTER};
use super::hunks::span_text;
use super::model::{How, Level, Relation, Span, Why};

/// The card title. `first_path` is the file of the strongest overlap, `more_files` counts the others.
pub fn title(a: &str, b: &str, level: Level, first_path: Option<&str>, more_files: usize) -> String {
    if level == Level::SharedFolder {
        return format!("{a} and {b} are working in the same folder");
    }
    let Some(path) = first_path else {
        return format!("{a} and {b} both changed a file");
    };
    match more_files {
        0 => format!("{a} and {b} both changed {path}"),
        1 => format!("{a} and {b} both changed {path} and 1 more file"),
        n => format!("{a} and {b} both changed {path} and {n} more files"),
    }
}

/// The detail sentence for one overlap. `names` are the two pane headings in pane-id order, used by
/// `DeleteVsEdit` (`deleted_by` is an index into them). `shared_paths` are the files listed for a
/// same-folder alert.
pub fn detail(how: &How, names: [&str; 2], shared_paths: &[String]) -> String {
    match how {
        How::Lines(span) => overlap_sentence(*span),
        How::LinesUnknown => "The change is big, so lines were not compared.".to_string(),
        How::BothNew => "Both made a new file with this name.".to_string(),
        How::DeleteVsEdit { deleted_by } => {
            let by = (*deleted_by).min(1);
            format!("{} deleted it. {} changed it.", names[by], names[1 - by])
        }
        How::Binary => "Both changed a file that is not text.".to_string(),
        How::Apart => "Different places in the file. No clash yet.".to_string(),
        How::DifferentStart => "They started from different versions, so lines were not compared.".to_string(),
        How::Shared => shared_sentence(shared_paths),
    }
}

/// Where the two panes work: separate copies of one repo, or one folder.
pub fn place(relation: Relation) -> String {
    match relation {
        Relation::Copies => "In separate copies.",
        Relation::SameFolder => "In the same folder.",
    }
    .to_string()
}

/// The advice line, with the names already scrubbed. Same words as `advice::advice_text`.
pub fn advice_line(relation: Relation, why: Why, first: &str, other: &str) -> String {
    advice::advice_text(relation, why, first, other)
}

/// The short label of one file line, shown after the path (`src/auth.ts - Lines 40-80`).
pub fn file_text(how: &How) -> String {
    match how {
        How::Lines(span) => span_text(*span),
        How::LinesUnknown => "Lines not compared".to_string(),
        How::BothNew => "New in both".to_string(),
        How::DeleteVsEdit { .. } => "Deleted in one, changed in the other".to_string(),
        How::Binary => "Not a text file".to_string(),
        How::Apart => "Different places".to_string(),
        How::DifferentStart => "Different starting versions".to_string(),
        How::Shared => "Changed while both worked".to_string(),
    }
}

/// The last line of every card (`advice::FOOTER`).
pub fn footer() -> String {
    FOOTER.to_string()
}

/// The one-line banner in a chat: `{other} also changed {file} . {text} . Open {other} . Ignore`.
pub fn banner(other: &str, file: &str, file_text: Option<&str>) -> String {
    let middle = match file_text {
        Some(text) => format!(" . {text}"),
        None => String::new(),
    };
    format!("{other} also changed {file}{middle} . Open {other} . Ignore")
}

fn overlap_sentence(span: Span) -> String {
    let text = span_text(span);
    if text.starts_with("Line ") {
        format!("{text} overlaps.")
    } else {
        format!("{text} overlap.")
    }
}

fn shared_sentence(paths: &[String]) -> String {
    if paths.is_empty() {
        return "They can overwrite each other.".to_string();
    }
    format!("They can overwrite each other. Changed while both worked: {}.", paths.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMES: [&str; 2] = ["fake_a", "fake_b"];

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn title_one_file_is_singular_and_plural_by_count() {
        assert_eq!(
            title("fake_a", "fake_b", Level::Clash, Some("src/auth.ts"), 0),
            "fake_a and fake_b both changed src/auth.ts"
        );
        assert_eq!(
            title("fake_a", "fake_b", Level::Clash, Some("src/auth.ts"), 1),
            "fake_a and fake_b both changed src/auth.ts and 1 more file"
        );
        assert_eq!(
            title("fake_a", "fake_b", Level::Clash, Some("src/auth.ts"), 3),
            "fake_a and fake_b both changed src/auth.ts and 3 more files"
        );
    }

    #[test]
    fn title_same_folder_names_no_file() {
        assert_eq!(
            title("fake_a", "fake_b", Level::SharedFolder, Some("src/auth.ts"), 2),
            "fake_a and fake_b are working in the same folder"
        );
    }

    #[test]
    fn title_without_a_file_has_a_fallback() {
        assert_eq!(title("fake_a", "fake_b", Level::Clash, None, 0), "fake_a and fake_b both changed a file");
    }

    #[test]
    fn place_names_both_relations() {
        assert_eq!(place(Relation::Copies), "In separate copies.");
        assert_eq!(place(Relation::SameFolder), "In the same folder.");
    }

    #[test]
    fn detail_has_one_golden_per_how() {
        let none: Vec<String> = Vec::new();
        assert_eq!(detail(&How::Lines(Span { start: 40, end: 80 }), NAMES, &none), "Lines 40-80 overlap.");
        assert_eq!(detail(&How::Lines(Span { start: 40, end: 40 }), NAMES, &none), "Line 40 overlaps.");
        assert_eq!(detail(&How::Lines(Span { start: 0, end: 0 }), NAMES, &none), "Line 1 overlaps.");
        assert_eq!(
            detail(&How::LinesUnknown, NAMES, &none),
            "The change is big, so lines were not compared."
        );
        assert_eq!(detail(&How::BothNew, NAMES, &none), "Both made a new file with this name.");
        assert_eq!(
            detail(&How::DeleteVsEdit { deleted_by: 0 }, NAMES, &none),
            "fake_a deleted it. fake_b changed it."
        );
        assert_eq!(
            detail(&How::DeleteVsEdit { deleted_by: 1 }, NAMES, &none),
            "fake_b deleted it. fake_a changed it."
        );
        assert_eq!(detail(&How::Binary, NAMES, &none), "Both changed a file that is not text.");
        assert_eq!(detail(&How::Apart, NAMES, &none), "Different places in the file. No clash yet.");
        assert_eq!(
            detail(&How::DifferentStart, NAMES, &none),
            "They started from different versions, so lines were not compared."
        );
        assert_eq!(
            detail(&How::Shared, NAMES, &paths(&["src/a.ts", "src/b.ts"])),
            "They can overwrite each other. Changed while both worked: src/a.ts, src/b.ts."
        );
        assert_eq!(detail(&How::Shared, NAMES, &none), "They can overwrite each other.");
    }

    #[test]
    fn advice_line_has_one_golden_per_rule() {
        assert_eq!(
            advice_line(Relation::Copies, Why::AlreadyDone, "fake_a", "fake_b"),
            "Finish fake_a first. It has stopped working. Look at its changes and merge them first. Then fake_b can build on it."
        );
        assert_eq!(
            advice_line(Relation::Copies, Why::SmallerChange, "fake_a", "fake_b"),
            "Finish fake_a first. It has changed fewer files, so it is quicker to finish. Merge it first. Then fake_b can build on it."
        );
        assert_eq!(
            advice_line(Relation::Copies, Why::StartedFirst, "fake_a", "fake_b"),
            "Finish fake_a first. It started first. Merge it first. Then fake_b can build on it."
        );
        assert_eq!(
            advice_line(Relation::SameFolder, Why::AlreadyDone, "fake_a", "fake_b"),
            "fake_a has stopped working. Ask fake_b to read the file again before it changes more."
        );
        assert_eq!(
            advice_line(Relation::SameFolder, Why::StartedFirst, "fake_a", "fake_b"),
            "Let fake_a finish first. Then ask fake_b to read the file again before it changes more."
        );
    }

    #[test]
    fn footer_is_the_exact_sentence_of_advice() {
        assert_eq!(footer(), "Nothing was paused. You decide.");
    }

    #[test]
    fn file_text_has_one_label_per_how() {
        assert_eq!(file_text(&How::Lines(Span { start: 40, end: 80 })), "Lines 40-80");
        assert_eq!(file_text(&How::Lines(Span { start: 50, end: 50 })), "Line 50");
        assert_eq!(file_text(&How::LinesUnknown), "Lines not compared");
        assert_eq!(file_text(&How::BothNew), "New in both");
        assert_eq!(file_text(&How::DeleteVsEdit { deleted_by: 1 }), "Deleted in one, changed in the other");
        assert_eq!(file_text(&How::Binary), "Not a text file");
        assert_eq!(file_text(&How::Apart), "Different places");
        assert_eq!(file_text(&How::DifferentStart), "Different starting versions");
        assert_eq!(file_text(&How::Shared), "Changed while both worked");
    }

    #[test]
    fn banner_with_and_without_a_line_range() {
        assert_eq!(
            banner("fake_b", "src/auth.ts", Some("Lines 40-80")),
            "fake_b also changed src/auth.ts . Lines 40-80 . Open fake_b . Ignore"
        );
        assert_eq!(banner("fake_b", "src/auth.ts", None), "fake_b also changed src/auth.ts . Open fake_b . Ignore");
    }

    #[test]
    fn names_with_braces_are_values_not_templates() {
        assert_eq!(
            title("{a}", "{0}", Level::Clash, Some("src/{b}.ts"), 2),
            "{a} and {0} both changed src/{b}.ts and 2 more files"
        );
        let names = ["{X}", "{Y}"];
        assert_eq!(
            detail(&How::DeleteVsEdit { deleted_by: 0 }, names, &[]),
            "{X} deleted it. {Y} changed it."
        );
        assert_eq!(banner("{Y}", "src/{X}.ts", Some("Line {1}")), "{Y} also changed src/{X}.ts . Line {1} . Open {Y} . Ignore");
    }

    #[test]
    fn copy_names_no_agent() {
        // C12: the code above the tests names no agent. Labels come from the pane facts only.
        let source = include_str!("copy.rs");
        let code = source.split("#[cfg(test)]").next().unwrap_or("");
        let lower = code.to_ascii_lowercase();
        for word in ["claude", "codex", "cursor", "opencode", "agy"] {
            assert!(!lower.contains(word), "generic code names an agent: {word}");
        }
    }
}
