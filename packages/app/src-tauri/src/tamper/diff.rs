//! Line diffs between a file at the start and now. Whitespace at the line ends and indentation
//! never count as a change, so a formatter's reflow shows up as equal lines.

use similar::{capture_diff_slices, Algorithm, DiffOp, TextDiff};

use super::lexer::split_lines;

/// Two lines that replaced each other in one hunk: `(old line, new line)`, both 1-based.
pub type Pair = (u32, u32);

#[derive(Debug, Default, Clone)]
pub struct LineDiff {
    /// New-file line numbers that were added (blank lines left out).
    pub added: Vec<u32>,
    /// Old-file line numbers that were removed (blank lines left out).
    pub removed: Vec<u32>,
    /// Lines that replaced each other, matched by similarity inside each changed hunk.
    pub pairs: Vec<Pair>,
}

impl LineDiff {
    pub fn plus(&self) -> u32 {
        self.added.len() as u32
    }

    pub fn minus(&self) -> u32 {
        self.removed.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}

/// Diff `old` against `new`. Either side can be `None` (a file that did not exist, or was deleted).
pub fn diff_text(old: Option<&str>, new: Option<&str>) -> LineDiff {
    let old_lines = old.map(split_lines).unwrap_or_default();
    let new_lines = new.map(split_lines).unwrap_or_default();
    diff_lines(&old_lines, &new_lines)
}

/// The same, over lines already split. Line numbers are positions in the slices, 1-based.
pub fn diff_lines(old_lines: &[&str], new_lines: &[&str]) -> LineDiff {
    let old_norm: Vec<&str> = old_lines.iter().map(|l| l.trim()).collect();
    let new_norm: Vec<&str> = new_lines.iter().map(|l| l.trim()).collect();
    let mut out = LineDiff::default();
    for op in capture_diff_slices(Algorithm::Myers, &old_norm, &new_norm) {
        let (old_range, new_range): (std::ops::Range<usize>, std::ops::Range<usize>) = match op {
            DiffOp::Equal { .. } => continue,
            DiffOp::Delete { old_index, old_len, new_index } => (old_index..old_index + old_len, new_index..new_index),
            DiffOp::Insert { old_index, new_index, new_len } => (old_index..old_index, new_index..new_index + new_len),
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                (old_index..old_index + old_len, new_index..new_index + new_len)
            }
        };
        let olds: Vec<usize> = old_range.filter(|i| !old_norm[*i].is_empty()).collect();
        let news: Vec<usize> = new_range.filter(|i| !new_norm[*i].is_empty()).collect();
        out.removed.extend(olds.iter().map(|i| *i as u32 + 1));
        out.added.extend(news.iter().map(|i| *i as u32 + 1));
        pair_hunk(&olds, &news, &old_norm, &new_norm, &mut out.pairs);
    }
    out
}

/// Match each new line of a hunk with the most similar unused old line (at least 0.4 alike).
fn pair_hunk(olds: &[usize], news: &[usize], old_norm: &[&str], new_norm: &[&str], pairs: &mut Vec<Pair>) {
    if olds.is_empty() || news.is_empty() {
        return;
    }
    let mut used = vec![false; olds.len()];
    for &n in news {
        let mut best: Option<(usize, f64)> = None;
        for (slot, &o) in olds.iter().enumerate() {
            if used[slot] {
                continue;
            }
            let score = strsim::sorensen_dice(old_norm[o], new_norm[n]);
            if score >= 0.4 && best.map_or(true, |(_, s)| score > s) {
                best = Some((slot, score));
            }
        }
        if let Some((slot, _)) = best {
            used[slot] = true;
            pairs.push((olds[slot] as u32 + 1, n as u32 + 1));
        }
    }
}

/// How alike two files are, 0.0 to 1.0 (by lines). Used to tell a renamed file from a deleted one.
pub fn similarity(a: &str, b: &str) -> f32 {
    TextDiff::from_lines(a, b).ratio()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indentation_is_not_a_change_and_replacements_are_paired() {
        let old = "a\n  expect(x).toBe(10.01);\nb\n";
        let new = "a\n    expect(x).toBeDefined();\nb\n";
        let d = diff_text(Some(old), Some(new));
        assert_eq!(d.added, [2]);
        assert_eq!(d.removed, [2]);
        assert_eq!(d.pairs, [(2, 2)]);
        let same = diff_text(Some("a\n  b\n"), Some("a\nb\n"));
        assert!(same.is_empty());
    }

    #[test]
    fn missing_sides_are_all_added_or_all_removed() {
        assert_eq!(diff_text(None, Some("a\n\nb\n")).added, [1, 3]);
        assert_eq!(diff_text(Some("a\nb\n"), None).removed, [1, 2]);
    }

    #[test]
    fn similar_files_are_alike() {
        let a = "one\ntwo\nthree\nfour\n";
        assert!(similarity(a, "one\ntwo\nthree\nfour\nfive\n") > 0.6);
        assert!(similarity(a, "x\ny\nz\n") < 0.2);
    }
}
