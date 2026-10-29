//! Look-alike names against the popular lists (SPEC-n33 section 3.3).
//!
//! A name is a look-alike when it is one small typo away from a popular name: a separator change,
//! look-alike characters (`l0dash`), an adjacent swap, one letter, or two letters (long names only).
//! Candidates are tried in rank order and the table's rules in order, so the most used name wins.
//! Pure: no network, no disk, no screen.

use super::model::{Ecosystem, How, Lookalike};
use super::popular;

/// Popular names shorter than this are never a look-alike target.
const POPULAR_MIN_LEN: usize = 4;
/// A one-edit look-alike needs a name at least this long.
const ONE_EDIT_MIN_LEN: usize = 5;
/// A two-edit look-alike needs a name at least this long.
const TWO_EDIT_MIN_LEN: usize = 10;

/// The look-alike of `norm` (already normalized for `eco`) in the ecosystem's popular list.
pub fn check(eco: Ecosystem, norm: &str) -> Option<Lookalike> {
    check_in(popular::list(eco), eco, norm)
}

/// The pure core of `check`. `list` is in rank order (most used first) and holds names normalized
/// for `eco`; `norm` is the candidate, normalized the same way.
pub fn check_in(list: &[String], eco: Ecosystem, norm: &str) -> Option<Lookalike> {
    // A name that is itself popular is never a look-alike of another popular name.
    if list.iter().any(|p| p == norm) {
        return None;
    }
    for popular in list {
        if popular.len() < POPULAR_MIN_LEN || norm.len().abs_diff(popular.len()) > 2 {
            continue;
        }
        if let Some(how) = how_for(eco, norm, popular) {
            return Some(Lookalike { popular: popular.clone(), how });
        }
    }
    None
}

/// How `n` differs from the popular name `p`, checking the table's rules in order. `n != p` holds.
fn how_for(eco: Ecosystem, n: &str, p: &str) -> Option<How> {
    if matches!(eco, Ecosystem::Npm | Ecosystem::Gems) && strip_separators(n) == strip_separators(p) {
        return Some(How::Separators);
    }
    if fold_lookalikes(n) == fold_lookalikes(p) {
        return Some(How::LookAlikeChars);
    }
    let dist = strsim::osa_distance(n, p);
    if dist == 1 && n.len() >= ONE_EDIT_MIN_LEN {
        return Some(if is_adjacent_swap(n, p) { How::Swapped } else { How::OneLetter });
    }
    if dist == 2 && n.len() >= TWO_EDIT_MIN_LEN {
        return Some(How::TwoLetters);
    }
    None
}

/// `-`, `_` and `.` removed.
fn strip_separators(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '-' | '_' | '.')).collect()
}

/// The look-alike folding: `rn` reads as `m`, `vv` as `w`, and `0 1 5 3` as `o l s e`.
fn fold_lookalikes(s: &str) -> String {
    s.replace("rn", "m")
        .replace("vv", "w")
        .chars()
        .map(|c| match c {
            '0' => 'o',
            '1' => 'l',
            '5' => 's',
            '3' => 'e',
            other => other,
        })
        .collect()
}

/// True when `a` and `b` have the same length and differ by exactly one swap of adjacent characters.
fn is_adjacent_swap(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len() != b.len() {
        return false;
    }
    let Some(i) = (0..a.len()).find(|&i| a[i] != b[i]) else {
        return false;
    };
    i + 1 < a.len() && a[i] == b[i + 1] && a[i + 1] == b[i] && a[i + 2..] == b[i + 2..]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ten popular names in rank order, as a fixture list.
    fn fixture() -> Vec<String> {
        ["lodash", "react", "express", "requests", "numpy", "serde", "tokio", "left-pad", "colors", "chalk"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn one(popular: &str, how: How) -> Option<Lookalike> {
        Some(Lookalike { popular: popular.to_string(), how })
    }

    #[test]
    fn swapped_adjacent_letters() {
        assert_eq!(check_in(&fixture(), Ecosystem::Npm, "lodahs"), one("lodash", How::Swapped));
    }

    #[test]
    fn one_letter_missing_npm() {
        assert_eq!(check_in(&fixture(), Ecosystem::Npm, "expres"), one("express", How::OneLetter));
    }

    #[test]
    fn one_letter_missing_pypi() {
        assert_eq!(check_in(&fixture(), Ecosystem::Pypi, "reqests"), one("requests", How::OneLetter));
    }

    #[test]
    fn one_letter_substituted_pypi() {
        assert_eq!(check_in(&fixture(), Ecosystem::Pypi, "numpi"), one("numpy", How::OneLetter));
    }

    #[test]
    fn underscore_for_dash_is_separators_on_npm() {
        assert_eq!(check_in(&fixture(), Ecosystem::Npm, "left_pad"), one("left-pad", How::Separators));
    }

    #[test]
    fn digit_for_letter_is_look_alike_characters() {
        assert_eq!(check_in(&fixture(), Ecosystem::Npm, "l0dash"), one("lodash", How::LookAlikeChars));
    }

    #[test]
    fn a_popular_name_itself_is_never_a_look_alike() {
        assert_eq!(check_in(&fixture(), Ecosystem::Npm, "react"), None);
    }

    #[test]
    fn short_names_are_never_look_alikes() {
        assert_eq!(check_in(&fixture(), Ecosystem::Npm, "rea"), None);
    }

    #[test]
    fn popular_names_under_four_letters_are_never_targets() {
        // Separators would match "a-b" to "ab", and look-alike characters "1o1" to "lol".
        assert_eq!(check_in(&["ab".to_string()], Ecosystem::Npm, "a-b"), None);
        assert_eq!(check_in(&["lol".to_string()], Ecosystem::Npm, "1o1"), None);
        // Control: the same rule passes for a name of four letters.
        assert_eq!(check_in(&["abcd".to_string()], Ecosystem::Npm, "a-bcd"), one("abcd", How::Separators));
    }

    #[test]
    fn two_letters_only_for_names_of_ten_or_more() {
        // Two substitutions in a 10-letter name.
        assert_eq!(
            check_in(&["typescript".to_string()], Ecosystem::Npm, "tqpescrxpt"),
            one("typescript", How::TwoLetters)
        );
        // The same two-letter gap in a 5-letter name is not enough.
        assert_eq!(check_in(&["tokio".to_string()], Ecosystem::Npm, "tkiox"), None);
    }

    #[test]
    fn tie_goes_to_the_higher_ranked_name() {
        // "reactt" is one letter away from both "reacts" and "react".
        let reacts_first = ["reacts", "react"].map(String::from);
        let react_first = ["react", "reacts"].map(String::from);
        assert_eq!(check_in(&reacts_first, Ecosystem::Npm, "reactt"), one("reacts", How::OneLetter));
        assert_eq!(check_in(&react_first, Ecosystem::Npm, "reactt"), one("react", How::OneLetter));
    }

    #[test]
    fn scoped_npm_names_compare_the_full_name() {
        let list = ["@types/node".to_string()];
        assert_eq!(check_in(&list, Ecosystem::Npm, "@types/nodee"), one("@types/node", How::OneLetter));
    }

    #[test]
    fn adjacent_swap_helper_is_exact() {
        assert!(is_adjacent_swap("lodahs", "lodash"));
        assert!(!is_adjacent_swap("lodash", "lodash"));
        assert!(!is_adjacent_swap("abcd", "dcba"));
        assert!(!is_adjacent_swap("abc", "abcd"));
        assert!(is_adjacent_swap("ab", "ba"));
    }

    #[test]
    fn real_list_finds_lodash_and_skips_popular_names() {
        assert_eq!(check(Ecosystem::Npm, "lodahs"), one("lodash", How::Swapped));
        assert_eq!(check(Ecosystem::Npm, "l0dash"), one("lodash", How::LookAlikeChars));
        assert_eq!(check(Ecosystem::Npm, "lodash"), None);
        assert_eq!(check(Ecosystem::Go, "github.com/x/lodahs"), None);
    }
}
