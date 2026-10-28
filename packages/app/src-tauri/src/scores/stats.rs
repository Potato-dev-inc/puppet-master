//! Rates, the 90% Wilson interval, sample tiers and the `ahead` rule (SC-2).
//!
//! Pure functions only. No I/O, no clock, no agent names. The `ahead` rule is the only comparison
//! the page makes: a row is marked only when its interval is clearly separated from every other
//! row with enough runs, so small samples almost never show a mark.

use super::model::{Rate, Sample, MIN_SAMPLE, STEADY_SAMPLE};

/// z for a two-sided 90% interval.
const Z: f64 = 1.645;

/// The sample tier of a rate with `total` runs: `none` (0), `not_enough` (1..4), `early` (5..9),
/// `steady` (10 or more).
pub fn sample_of(total: u32) -> Sample {
    if total == 0 {
        Sample::None
    } else if total < MIN_SAMPLE {
        Sample::NotEnough
    } else if total < STEADY_SAMPLE {
        Sample::Early
    } else {
        Sample::Steady
    }
}

/// The 90% Wilson score interval of `count` out of `total`, as fractions from 0.0 to 1.0.
/// `count` is clamped to `total`. With no runs the interval is the whole range (0.0, 1.0).
pub fn wilson(count: u32, total: u32) -> (f32, f32) {
    if total == 0 {
        return (0.0, 1.0);
    }
    let n = total as f64;
    let p = count.min(total) as f64 / n;
    let z2 = Z * Z;
    let denom = 1.0 + z2 / n;
    let centre = (p + z2 / (2.0 * n)) / denom;
    let half = Z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt() / denom;
    let low = (centre - half).clamp(0.0, 1.0);
    let high = (centre + half).clamp(0.0, 1.0);
    (low as f32, high as f32)
}

/// One rate: `count` of `total`, with its sample tier and interval. `count` is clamped to `total`.
pub fn rate(count: u32, total: u32) -> Rate {
    let count = count.min(total);
    let (low, high) = wilson(count, total);
    Rate { count, total, sample: sample_of(total), low, high }
}

/// Median of the values (sorts the vector in place). With an even count it is the lower half of
/// the two middle values' average, rounded down. `None` for an empty vector.
pub fn median_u64(v: &mut Vec<u64>) -> Option<u64> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    let mid = v.len() / 2;
    if v.len() % 2 == 1 {
        Some(v[mid])
    } else {
        // `v[mid] >= v[mid - 1]` after sorting, so this cannot overflow.
        Some(v[mid - 1] + (v[mid] - v[mid - 1]) / 2)
    }
}

/// Same as [`median_u64`] for `u32` values.
pub fn median_u32(v: &mut Vec<u32>) -> Option<u32> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    let mid = v.len() / 2;
    if v.len() % 2 == 1 {
        Some(v[mid])
    } else {
        Some(v[mid - 1] + (v[mid] - v[mid - 1]) / 2)
    }
}

/// The `ahead` rule (spec 3.3). `rates` pairs a row index with its rate.
///
/// Only rows with at least `MIN_SAMPLE` runs take part, and at least two of them must. A row is
/// ahead when its interval is strictly better than the interval of every other such row: its low
/// is above their high (higher is better), or its high is below their low (`lower_is_better`).
/// Rows with fewer runs are ignored. Returns the row indices in input order.
pub fn ahead(rates: &[(usize, Rate)], lower_is_better: bool) -> Vec<usize> {
    let eligible: Vec<(usize, &Rate)> = rates
        .iter()
        .filter(|(_, r)| r.total >= MIN_SAMPLE)
        .map(|(row, r)| (*row, r))
        .collect();
    if eligible.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (pos, (row, mine)) in eligible.iter().enumerate() {
        let clear = eligible.iter().enumerate().all(|(other_pos, (_, other))| {
            other_pos == pos
                || if lower_is_better { mine.high < other.low } else { mine.low > other.high }
        });
        if clear {
            out.push(*row);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.005
    }

    #[test]
    fn sample_tiers_at_the_edges() {
        assert_eq!(sample_of(0), Sample::None);
        assert_eq!(sample_of(1), Sample::NotEnough);
        assert_eq!(sample_of(4), Sample::NotEnough);
        assert_eq!(sample_of(5), Sample::Early);
        assert_eq!(sample_of(9), Sample::Early);
        assert_eq!(sample_of(10), Sample::Steady);
        assert_eq!(sample_of(500), Sample::Steady);
    }

    #[test]
    fn wilson_known_values() {
        // 8 of 10 at 90%: 0.541 .. 0.931 (checked against the formula in double precision).
        let (low, high) = wilson(8, 10);
        assert!(close(low, 0.541), "low was {low}");
        assert!(close(high, 0.931), "high was {high}");
        // 10 of 10 reaches 1.0 at the top and is clamped there.
        let (low, high) = wilson(10, 10);
        assert!(close(low, 0.787), "low was {low}");
        assert_eq!(high, 1.0);
        // 0 of 10 starts at 0.0.
        let (low, high) = wilson(0, 10);
        assert_eq!(low, 0.0);
        assert!(close(high, 0.213), "high was {high}");
    }

    #[test]
    fn wilson_with_no_runs_is_the_whole_range() {
        assert_eq!(wilson(0, 0), (0.0, 1.0));
    }

    #[test]
    fn wilson_clamps_count_to_total() {
        assert_eq!(wilson(12, 10), wilson(10, 10));
    }

    #[test]
    fn rate_clamps_and_takes_its_tier() {
        let r = rate(12, 10);
        assert_eq!((r.count, r.total), (10, 10));
        assert_eq!(r.sample, Sample::Steady);
        let r = rate(3, 4);
        assert_eq!(r.sample, Sample::NotEnough);
        let r = rate(0, 0);
        assert_eq!((r.count, r.total, r.sample), (0, 0, Sample::None));
        assert_eq!((r.low, r.high), (0.0, 1.0));
    }

    #[test]
    fn median_of_empty_is_none() {
        assert_eq!(median_u64(&mut vec![]), None);
        assert_eq!(median_u32(&mut vec![]), None);
    }

    #[test]
    fn median_odd_and_even() {
        assert_eq!(median_u64(&mut vec![9, 1, 5]), Some(5));
        assert_eq!(median_u64(&mut vec![4, 1, 3, 2]), Some(2));
        assert_eq!(median_u64(&mut vec![1, 4]), Some(2));
        assert_eq!(median_u32(&mut vec![7]), Some(7));
        assert_eq!(median_u32(&mut vec![10, 3, 4, 1]), Some(3));
        // Large values do not overflow.
        assert_eq!(median_u64(&mut vec![u64::MAX, u64::MAX - 2]), Some(u64::MAX - 1));
        assert_eq!(median_u32(&mut vec![u32::MAX, u32::MAX]), Some(u32::MAX));
    }

    #[test]
    fn ahead_needs_two_rows_with_enough_runs() {
        assert!(ahead(&[], false).is_empty());
        assert!(ahead(&[(0, rate(10, 10))], false).is_empty());
        // The second row has only 4 runs: it does not count, so the first row is alone.
        assert!(ahead(&[(0, rate(10, 10)), (1, rate(4, 4))], false).is_empty());
    }

    #[test]
    fn ahead_when_clearly_separated() {
        // 10/10 (0.787..1.0) against 5/10 (0.269..0.731): clearly separated.
        assert_eq!(ahead(&[(0, rate(10, 10)), (1, rate(5, 10))], false), vec![0]);
        // The row order is kept in the answer.
        assert_eq!(ahead(&[(3, rate(5, 10)), (7, rate(10, 10))], false), vec![7]);
    }

    #[test]
    fn not_ahead_when_the_intervals_overlap() {
        // 9/10 (0.652..0.977) against 8/10 (0.541..0.931): overlap, no mark.
        assert!(ahead(&[(0, rate(9, 10)), (1, rate(8, 10))], false).is_empty());
        // Equal rows never mark anyone.
        assert!(ahead(&[(0, rate(7, 10)), (1, rate(7, 10))], false).is_empty());
    }

    #[test]
    fn lower_is_better_flips_the_direction() {
        // 0 of 10 undone (0.000..0.213) against 5 of 10 (0.269..0.731): row 0 is clearly lower.
        assert_eq!(ahead(&[(0, rate(0, 10)), (1, rate(5, 10))], true), vec![0]);
        // Read as "higher is better", the same pair marks row 1 instead.
        assert_eq!(ahead(&[(0, rate(0, 10)), (1, rate(5, 10))], false), vec![1]);
        // 10 of 10 against 0 of 10, lower is better: only the 0 of 10 row is clearly lower.
        assert_eq!(ahead(&[(0, rate(10, 10)), (1, rate(0, 10))], true), vec![1]);
    }

    #[test]
    fn rows_with_a_small_sample_are_ignored_for_the_others() {
        // Row 2 has 3 runs; rows 0 and 1 still compare with each other and row 0 is clear.
        let rows = [(0, rate(10, 10)), (1, rate(5, 10)), (2, rate(0, 3))];
        assert_eq!(ahead(&rows, false), vec![0]);
    }
}
