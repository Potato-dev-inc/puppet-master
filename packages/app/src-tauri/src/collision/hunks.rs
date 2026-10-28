//! Collision Alert: parses `git diff -U0` output into old-side line spans, and decides whether two
//! sets of spans overlap. Pure: no process, no file read. Only line numbers are used, never text.

use super::model::{FileHunks, Span, CLASH_GAP_LINES, MAX_HUNKS_PER_FILE};

/// Parses the `git diff -U0` output of one file into old-side line spans.
///
/// A hunk header `@@ -a[,b] +c[,d] @@` gives `Span{a, a+b-1}`; a missing `b` is 1; `b == 0` is an
/// insertion after line `a`, so it gives `Span{a, a}`. A binary file sets `binary`. A header that
/// cannot be read, or more than `MAX_HUNKS_PER_FILE` hunks, or `truncated` (the caller's cap), sets
/// `cut`: the lines cannot be proven apart, so the caller treats the file as "not compared".
pub fn parse_u0(patch: &str, truncated: bool) -> FileHunks {
    let mut spans = Vec::new();
    let mut binary = false;
    let mut malformed = false;
    for raw in patch.split('\n') {
        // CRLF output (Windows) leaves a trailing carriage return on every line.
        let line = raw.trim_end_matches('\r');
        if line.starts_with("@@ -") {
            match header_span(line) {
                Some(span) => spans.push(span),
                None => malformed = true,
            }
        } else if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
            binary = true;
        }
    }
    let mut cut = truncated || malformed;
    if spans.len() > MAX_HUNKS_PER_FILE {
        spans.truncate(MAX_HUNKS_PER_FILE);
        cut = true;
    }
    spans.sort_by_key(|s| (s.start, s.end));
    FileHunks { spans, binary, cut }
}

/// One hunk header `@@ -a[,b] +c[,d] @@ ...` as the old-side span. `None` when it is not readable.
fn header_span(line: &str) -> Option<Span> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, new) = rest.split_once(" +")?;
    if !new.contains(" @@") {
        return None;
    }
    let (start, count) = match old.split_once(',') {
        Some((a, b)) => (a.parse::<u32>().ok()?, b.parse::<u32>().ok()?),
        None => (old.parse::<u32>().ok()?, 1),
    };
    if count == 0 {
        return Some(Span { start, end: start });
    }
    Some(Span {
        start,
        end: start.saturating_add(count - 1),
    })
}

/// Do the two files change overlapping lines (or lines touching each other)?
///
/// Spans are compared on the old side, so both files must start from the same version. Two spans
/// clash when they overlap or touch: `a.start <= b.end + GAP && b.start <= a.end + GAP`, where
/// `GAP` is `CLASH_GAP_LINES` (1). One unchanged line between two changes does not clash.
/// Returns the union of the first clashing pair in line order, or `None` when there is no clash or
/// when either side is binary (the caller decides binary clashes itself). A `cut` side is not
/// special here: its known spans are compared, and the caller reports "not compared" when nothing
/// is found.
pub fn clash(a: &FileHunks, b: &FileHunks) -> Option<Span> {
    if a.binary || b.binary {
        return None;
    }
    let xs = sorted(&a.spans);
    let ys = sorted(&b.spans);
    let (mut i, mut j) = (0, 0);
    while i < xs.len() && j < ys.len() {
        let (x, y) = (xs[i], ys[j]);
        let clashes = x.start <= y.end.saturating_add(CLASH_GAP_LINES)
            && y.start <= x.end.saturating_add(CLASH_GAP_LINES);
        if clashes {
            return Some(Span {
                start: x.start.min(y.start),
                end: x.end.max(y.end),
            });
        }
        // No clash: the one that ends first cannot touch any later span on the other side.
        if y.start > x.end.saturating_add(CLASH_GAP_LINES) {
            i += 1;
        } else {
            j += 1;
        }
    }
    None
}

fn sorted(spans: &[Span]) -> Vec<Span> {
    let mut v = spans.to_vec();
    v.sort_by_key(|s| (s.start, s.end));
    v
}

/// The words for a span: `Lines 40-80`, or `Line 40` when it is one line. Line 0 (an insertion at
/// the top of the file) is shown as line 1.
pub fn span_text(s: Span) -> String {
    let start = s.start.max(1);
    let end = s.end.max(start);
    if start == end {
        format!("Line {start}")
    } else {
        format!("Lines {start}-{end}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hunks(spans: &[(u32, u32)]) -> FileHunks {
        FileHunks {
            spans: spans
                .iter()
                .map(|&(start, end)| Span { start, end })
                .collect(),
            binary: false,
            cut: false,
        }
    }

    fn span(start: u32, end: u32) -> Option<Span> {
        Some(Span { start, end })
    }

    #[test]
    fn touching_lines_clash() {
        assert_eq!(clash(&hunks(&[(5, 5)]), &hunks(&[(6, 6)])), span(5, 6));
        assert_eq!(clash(&hunks(&[(6, 6)]), &hunks(&[(5, 5)])), span(5, 6));
        assert_eq!(clash(&hunks(&[(5, 5)]), &hunks(&[(5, 5)])), span(5, 5));
    }

    #[test]
    fn one_unchanged_line_between_does_not_clash() {
        assert_eq!(clash(&hunks(&[(5, 5)]), &hunks(&[(7, 7)])), None);
        assert_eq!(clash(&hunks(&[(5, 6)]), &hunks(&[(8, 9)])), None);
        assert_eq!(clash(&hunks(&[(8, 9)]), &hunks(&[(5, 6)])), None);
    }

    #[test]
    fn insertion_points_clash_only_when_touching() {
        // An insertion after line 5 is {5, 5}.
        let after_five = hunks(&[(5, 5)]);
        assert_eq!(clash(&after_five, &hunks(&[(5, 5)])), span(5, 5));
        assert_eq!(clash(&after_five, &hunks(&[(6, 6)])), span(5, 6));
        assert_eq!(clash(&after_five, &hunks(&[(7, 7)])), None);
        assert_eq!(clash(&after_five, &hunks(&[(1, 3)])), None);
    }

    #[test]
    fn many_small_hunks_against_one_big_one() {
        let small: Vec<(u32, u32)> = (0..100).map(|i| (i * 10 + 1, i * 10 + 1)).collect();
        // The first clash in line order is with the span at line 1; the union covers both.
        assert_eq!(clash(&hunks(&small), &hunks(&[(0, 2000)])), span(0, 2000));
        // One small change between two small ones: only that pair is compared.
        assert_eq!(
            clash(
                &hunks(&[(1, 1), (10, 10), (20, 20), (30, 30)]),
                &hunks(&[(9, 11)])
            ),
            span(9, 11)
        );
        // Lines between the small changes do not clash with the big one.
        assert_eq!(
            clash(&hunks(&[(2, 2), (4, 4), (6, 6)]), &hunks(&[(8, 9)])),
            None
        );
    }

    #[test]
    fn header_without_counts_is_one_line() {
        let parsed = parse_u0("@@ -7 +7 @@\n-old\n+new\n", false);
        assert_eq!(parsed.spans, vec![Span { start: 7, end: 7 }]);
        assert_eq!(parse_u0("@@ -7,1 +7 @@\n", false).spans, parsed.spans);
    }

    #[test]
    fn insertion_header_gives_the_line_it_follows() {
        let parsed = parse_u0("@@ -5,0 +6,2 @@\n+a\n+b\n", false);
        assert_eq!(parsed.spans, vec![Span { start: 5, end: 5 }]);
        assert_eq!(
            parse_u0("@@ -0,0 +1 @@\n", false).spans,
            vec![Span { start: 0, end: 0 }]
        );
    }

    #[test]
    fn range_header_gives_all_its_lines() {
        let parsed = parse_u0("@@ -40,41 +40,3 @@ fn main() {\n", false);
        assert_eq!(parsed.spans, vec![Span { start: 40, end: 80 }]);
        assert!(!parsed.binary && !parsed.cut);
    }

    #[test]
    fn binary_files_are_flagged_and_never_clash() {
        let parsed = parse_u0("Binary files a/logo.png and b/logo.png differ\n", false);
        assert!(parsed.binary);
        assert!(parsed.spans.is_empty());
        assert_eq!(clash(&parsed, &hunks(&[(1, 100)])), None);
        assert_eq!(clash(&hunks(&[(1, 100)]), &parsed), None);

        let patch = parse_u0("diff --git a/x b/x\nGIT binary patch\nliteral 12\n", false);
        assert!(patch.binary);
    }

    #[test]
    fn cut_at_501_hunks_keeps_the_first_500() {
        let patch: String = (0..501u32)
            .map(|i| format!("@@ -{} +{} @@\n", i * 2 + 1, i))
            .collect();
        let parsed = parse_u0(&patch, false);
        assert!(parsed.cut);
        assert_eq!(parsed.spans.len(), MAX_HUNKS_PER_FILE);
        assert_eq!(parsed.spans[0], Span { start: 1, end: 1 });
        assert_eq!(
            parsed.spans[499],
            Span {
                start: 999,
                end: 999
            }
        );

        let exact: String = (0..500u32)
            .map(|i| format!("@@ -{} +{} @@\n", i * 2 + 1, i))
            .collect();
        let parsed = parse_u0(&exact, false);
        assert!(!parsed.cut);
        assert_eq!(parsed.spans.len(), 500);
    }

    #[test]
    fn truncated_patch_is_cut() {
        let parsed = parse_u0("@@ -3 +3 @@\n-a\n+b\n", true);
        assert!(parsed.cut);
        assert_eq!(parsed.spans, vec![Span { start: 3, end: 3 }]);
    }

    #[test]
    fn unreadable_header_is_cut_not_guessed() {
        let parsed = parse_u0("@@ -x,2 +3 @@\n", false);
        assert!(parsed.cut);
        assert!(parsed.spans.is_empty());
        assert!(parse_u0("@@ -3,2\n", false).cut);
    }

    #[test]
    fn crlf_lines_and_no_newline_marker_are_safe() {
        let patch =
            "@@ -3,2 +3,2 @@\r\n-a\r\n\\ No newline at end of file\r\n+c\r\n@@ -9 +9 @@\r\n";
        let parsed = parse_u0(patch, false);
        assert_eq!(
            parsed.spans,
            vec![Span { start: 3, end: 4 }, Span { start: 9, end: 9 }]
        );
        assert!(!parsed.cut && !parsed.binary);
    }

    #[test]
    fn empty_patch_has_no_spans() {
        let parsed = parse_u0("", false);
        assert!(parsed.spans.is_empty());
        assert!(!parsed.binary && !parsed.cut);
        assert_eq!(clash(&parsed, &hunks(&[(1, 10)])), None);
        assert_eq!(clash(&parsed, &parsed), None);
    }

    #[test]
    fn first_clashing_pair_gives_the_union_of_that_pair_only() {
        let a = hunks(&[(1, 2), (10, 12)]);
        let b = hunks(&[(2, 3), (11, 11)]);
        assert_eq!(clash(&a, &b), span(1, 3));
    }

    #[test]
    fn span_text_one_line_and_range() {
        assert_eq!(span_text(Span { start: 40, end: 80 }), "Lines 40-80");
        assert_eq!(span_text(Span { start: 40, end: 40 }), "Line 40");
        assert_eq!(span_text(Span { start: 0, end: 0 }), "Line 1");
        assert_eq!(span_text(Span { start: 0, end: 3 }), "Lines 1-3");
    }
}
