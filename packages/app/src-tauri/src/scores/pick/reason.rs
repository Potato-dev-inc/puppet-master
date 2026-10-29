//! The one sentence of the answer, the "Why this one" lines and the short labels, in simple English.
//! The wording is the table of spec section 3.7, so every string is built from templates. The only
//! dynamic pieces are labels (cut to `MAX_LABEL_CHARS`), counts, and the reset text that the caller
//! formats through `when`. Nothing here takes a folder, a path or a pane id.

use super::model::{Basis, Cand, Metric, Skip, UsualKind, MAX_LABEL_CHARS, METRIC_ORDER};
use crate::scores::model::{Sample, MIN_SAMPLE};

/// The longest answer sentence. A last resort: the templates are shorter with short labels.
const MAX_REASON_CHARS: usize = 220;

/// What the sentence needs. Built by `compose`, and every field is a fact the module already has.
#[derive(Clone, Copy, Debug)]
pub struct SentenceInput<'a> {
    pub basis: Basis,
    /// The candidate that is picked (`None` only when nothing is ready).
    pub chosen: Option<&'a Cand>,
    /// The other candidate that was compared (`Clear` only).
    pub rival: Option<&'a Cand>,
    /// The measure that decided (`Clear`) or was compared (`CloseCall`).
    pub metric: Option<Metric>,
    /// Which usual agent was used for a fallback.
    pub usual: Option<UsualKind>,
    /// The sample of the deciding rate (`Early` adds " so far").
    pub sample: Sample,
    /// A limited candidate that is named in the sentence.
    pub blocked: Option<(&'a Cand, &'a Skip)>,
    /// The first skipped candidate (used when nothing is ready).
    pub first_skipped: Option<(&'a Cand, &'a Skip)>,
}

/// Makes text safe for one line: control characters become spaces, and runs of spaces collapse to
/// one, so the ends are trimmed too.
fn clean(text: &str) -> String {
    let spaced: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A label for a sentence: cleaned, and cut to `MAX_LABEL_CHARS` characters. A cut label ends with
/// "...", and the cut never splits a character.
pub fn short(label: &str) -> String {
    let text = clean(label);
    if text.chars().count() <= MAX_LABEL_CHARS {
        return text;
    }
    let kept: String = text.chars().take(MAX_LABEL_CHARS - 3).collect();
    format!("{}...", kept.trim_end())
}

/// The reset text for a time, cleaned for one line.
fn reset(ms: u64, when: &dyn Fn(u64) -> String) -> String {
    clean(&when(ms))
}

/// Ends a text with one full stop. A text that already ends with a full stop keeps it.
fn period(text: &str) -> String {
    if text.ends_with('.') {
        text.to_string()
    } else {
        format!("{text}.")
    }
}

/// The plain text of a skipped candidate, as shown in "Why this one".
pub fn skip_text(skip: &Skip, when: &dyn Fn(u64) -> String) -> String {
    match skip {
        Skip::NotReady(text) => clean(text),
        Skip::OutOfUsage(Some(ms)) => format!("Out of usage until {}", reset(*ms, when)),
        Skip::OutOfUsage(None) => "Out of usage".to_string(),
        Skip::SharedPlan { with } => format!("Shares a plan with {}, which is out of usage", short(with)),
    }
}

/// One line of "Why this one" for a candidate with a row (or without one).
///
/// A skipped candidate needs `skip_text`, which takes the reset time. This function cannot format
/// a time, so for a limited candidate it returns only "Out of usage". For a candidate that cannot
/// run it returns the plain reason. For a ready candidate it shows the counts of the deciding
/// measure (`metric`, or the first measure with runs), with " (too few to count)" under 5 runs.
pub fn line_of(cand: &Cand, metric: Option<Metric>) -> String {
    if let Err(reason) = &cand.ready {
        return clean(reason);
    }
    if cand.limited_until.is_some() {
        return "Out of usage".to_string();
    }
    let Some(ev) = cand.evidence else {
        return "No runs here yet".to_string();
    };
    let measure = match metric {
        Some(m) => m,
        None => match METRIC_ORDER.iter().copied().find(|m| ev.rate(*m).total > 0) {
            Some(m) => m,
            None => return "No runs here yet".to_string(),
        },
    };
    let rate = ev.rate(measure);
    let words = match measure {
        Metric::ChecksGreen => "checks green",
        Metric::Merged => "copies kept",
        Metric::EndedOk => "turns ended OK",
    };
    let few = if rate.total < MIN_SAMPLE { " (too few to count)" } else { "" };
    format!("{} of {} {}{}", rate.count, rate.total, words, few)
}

/// The verb of the Clear sentence for each measure.
fn verb(m: Metric) -> &'static str {
    match m {
        Metric::ChecksGreen => "passed its checks",
        Metric::Merged => "had its separate copy kept",
        Metric::EndedOk => "ended without an error",
    }
}

/// The counts of one candidate for one measure: (count, total). No row means (0, 0).
fn counts(cand: &Cand, m: Metric) -> (u32, u32) {
    cand.evidence.map(|ev| (ev.rate(m).count, ev.rate(m).total)).unwrap_or((0, 0))
}

/// "X is out of usage until 17:00" (or without a time, or "shares a plan"), for a blocked label.
fn blocked_clause(cand: &Cand, skip: &Skip, when: &dyn Fn(u64) -> String) -> String {
    let label = short(&cand.label);
    match skip {
        Skip::OutOfUsage(Some(ms)) => format!("{} is out of usage until {}", label, reset(*ms, when)),
        Skip::OutOfUsage(None) => format!("{} is out of usage", label),
        Skip::SharedPlan { .. } => format!("{} shares a plan that is out of usage", label),
        Skip::NotReady(_) => format!("{} is not ready", label),
    }
}

/// "your usual one, X" or "the first one ready, X".
fn usual_phrase(kind: Option<UsualKind>, label: &str) -> String {
    match kind.unwrap_or(UsualKind::Chosen) {
        UsualKind::Chosen | UsualKind::Last => format!("your usual one, {label}"),
        UsualKind::First => format!("the first one ready, {label}"),
    }
}

/// Clear: the chosen one's counts, the rival's counts, or the blocked clause in front.
fn clear(input: &SentenceInput<'_>, chosen: &Cand, when: &dyn Fn(u64) -> String) -> String {
    let metric = input.metric.unwrap_or(Metric::ChecksGreen);
    let (count, total) = counts(chosen, metric);
    let so_far = if input.sample == Sample::Early { " so far" } else { "" };
    let mine = format!(
        "{} {} {} of {} times here{}",
        short(&chosen.label),
        verb(metric),
        count,
        total,
        so_far
    );
    match (input.blocked, input.rival) {
        (Some((x, skip)), _) => format!("{}, and {}.", blocked_clause(x, skip, when), mine),
        (None, Some(rival)) => {
            let (rival_count, rival_total) = counts(rival, metric);
            format!("{}; {} {} of {}.", mine, short(&rival.label), rival_count, rival_total)
        }
        (None, None) => format!("{}.", mine),
    }
}

/// OnlyOne: "Only X is ready.", with the reason when another one is out of usage.
fn only_one(input: &SentenceInput<'_>, chosen: &Cand, when: &dyn Fn(u64) -> String) -> String {
    let label = short(&chosen.label);
    match input.blocked {
        Some((x, skip)) => format!("Only {} is ready, because {}.", label, blocked_clause(x, skip, when)),
        None => format!("Only {label} is ready."),
    }
}

/// TooEarly and CloseCall: the usual one is named, or the blocked clause comes first.
fn unsure(lead: &str, input: &SentenceInput<'_>, chosen: &Cand, when: &dyn Fn(u64) -> String) -> String {
    let label = short(&chosen.label);
    match input.blocked {
        Some((x, skip)) => format!("{}, so {}.", blocked_clause(x, skip, when), label),
        None => format!("{}, so {}.", lead, usual_phrase(input.usual, &label)),
    }
}

/// NoneReady: the first skipped agent and its reason, or "no agent at all".
fn none_ready(input: &SentenceInput<'_>, when: &dyn Fn(u64) -> String) -> String {
    match input.first_skipped {
        Some((cand, skip)) => {
            let reason = skip_text(skip, when);
            if reason.is_empty() {
                format!("No agent is ready: {}.", short(&cand.label))
            } else {
                format!("No agent is ready: {} - {}", short(&cand.label), period(&reason))
            }
        }
        None => "No agent is installed yet.".to_string(),
    }
}

/// Last resort: one line, at most `MAX_REASON_CHARS` characters. A longer text is cut with "...".
fn fit(text: &str) -> String {
    let one_line = clean(text);
    if one_line.chars().count() <= MAX_REASON_CHARS {
        return one_line;
    }
    let kept: String = one_line.chars().take(MAX_REASON_CHARS - 3).collect();
    format!("{}...", kept.trim_end())
}

/// The one sentence of the answer (spec 3.7). Every basis has its own template; the reset time
/// comes from `when`. If a non-`NoneReady` basis has no chosen candidate, the sentence says so.
pub fn sentence(input: &SentenceInput<'_>, when: &dyn Fn(u64) -> String) -> String {
    let text = match (input.basis, input.chosen) {
        (Basis::NoneReady, _) => none_ready(input, when),
        (_, None) => "No agent is ready.".to_string(),
        (Basis::Clear, Some(chosen)) => clear(input, chosen, when),
        (Basis::OnlyOne, Some(chosen)) => only_one(input, chosen, when),
        (Basis::TooEarly, Some(chosen)) => {
            unsure("Too early to tell your agents apart here", input, chosen, when)
        }
        (Basis::CloseCall, Some(chosen)) => {
            unsure("Your agents look about the same here so far", input, chosen, when)
        }
    };
    fit(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handoff::keep_going::model::Target;
    use crate::scores::stats::rate;
    use super::super::model::Evidence;

    fn ev(checks: (u32, u32), merged: (u32, u32), ended: (u32, u32)) -> Evidence {
        Evidence {
            turns: 10,
            checks_green: rate(checks.0, checks.1),
            merged: rate(merged.0, merged.1),
            ended_ok: rate(ended.0, ended.1),
        }
    }

    fn cand(label: &str, evidence: Option<Evidence>) -> Cand {
        Cand {
            target: Target { agent_id: label.to_string(), account: None },
            label: label.to_string(),
            vendor: None,
            ready: Ok(()),
            limited_until: None,
            evidence,
        }
    }

    fn limited(mut c: Cand) -> Cand {
        c.limited_until = Some(None);
        c
    }

    fn base<'a>(basis: Basis) -> SentenceInput<'a> {
        SentenceInput {
            basis,
            chosen: None,
            rival: None,
            metric: None,
            usual: None,
            sample: Sample::Steady,
            blocked: None,
            first_skipped: None,
        }
    }

    fn at_1700(_: u64) -> String {
        "17:00".to_string()
    }

    fn say(input: &SentenceInput<'_>) -> String {
        sentence(input, &at_1700)
    }

    fn checks_9_of_10() -> Evidence {
        ev((9, 10), (4, 6), (9, 10))
    }

    fn checks_5_of_10() -> Evidence {
        ev((5, 10), (2, 6), (8, 10))
    }

    #[test]
    fn clear_steady_names_the_chosen_and_the_rival_counts() {
        let a = cand("fake_a", Some(checks_9_of_10()));
        let b = cand("fake_b", Some(checks_5_of_10()));
        let input = SentenceInput {
            chosen: Some(&a),
            rival: Some(&b),
            metric: Some(Metric::ChecksGreen),
            ..base(Basis::Clear)
        };
        assert_eq!(say(&input), "fake_a passed its checks 9 of 10 times here; fake_b 5 of 10.");
    }

    #[test]
    fn clear_early_adds_so_far_before_the_rival() {
        let a = cand("fake_a", Some(ev((6, 7), (1, 2), (6, 7))));
        let b = cand("fake_b", Some(ev((1, 5), (1, 2), (4, 5))));
        let input = SentenceInput {
            chosen: Some(&a),
            rival: Some(&b),
            metric: Some(Metric::ChecksGreen),
            sample: Sample::Early,
            ..base(Basis::Clear)
        };
        assert_eq!(say(&input), "fake_a passed its checks 6 of 7 times here so far; fake_b 1 of 5.");
    }

    #[test]
    fn clear_merged_uses_the_copy_wording() {
        let a = cand("fake_a", Some(ev((9, 10), (4, 6), (9, 10))));
        let b = cand("fake_b", Some(ev((9, 10), (1, 6), (9, 10))));
        let input = SentenceInput {
            chosen: Some(&a),
            rival: Some(&b),
            metric: Some(Metric::Merged),
            ..base(Basis::Clear)
        };
        assert_eq!(say(&input), "fake_a had its separate copy kept 4 of 6 times here; fake_b 1 of 6.");
    }

    #[test]
    fn clear_ended_ok_uses_the_without_an_error_wording() {
        let a = cand("fake_a", Some(ev((9, 10), (4, 6), (18, 20))));
        let b = cand("fake_b", Some(ev((9, 10), (4, 6), (15, 20))));
        let input = SentenceInput {
            chosen: Some(&a),
            rival: Some(&b),
            metric: Some(Metric::EndedOk),
            ..base(Basis::Clear)
        };
        assert_eq!(say(&input), "fake_a ended without an error 18 of 20 times here; fake_b 15 of 20.");
    }

    #[test]
    fn clear_without_a_rival_ends_after_the_counts() {
        let a = cand("fake_a", Some(checks_9_of_10()));
        let input = SentenceInput { chosen: Some(&a), metric: Some(Metric::ChecksGreen), ..base(Basis::Clear) };
        assert_eq!(say(&input), "fake_a passed its checks 9 of 10 times here.");
    }

    #[test]
    fn clear_blocked_with_a_time_names_the_limit_first() {
        let a = cand("fake_a", Some(checks_9_of_10()));
        let b = limited(cand("fake_b", Some(checks_5_of_10())));
        let skip = Skip::OutOfUsage(Some(1_000));
        let input = SentenceInput {
            chosen: Some(&a),
            rival: Some(&b),
            metric: Some(Metric::ChecksGreen),
            blocked: Some((&b, &skip)),
            ..base(Basis::Clear)
        };
        assert_eq!(
            say(&input),
            "fake_b is out of usage until 17:00, and fake_a passed its checks 9 of 10 times here."
        );
    }

    #[test]
    fn clear_blocked_without_a_time_has_no_until() {
        let a = cand("fake_a", Some(checks_9_of_10()));
        let b = limited(cand("fake_b", None));
        let skip = Skip::OutOfUsage(None);
        let input = SentenceInput {
            chosen: Some(&a),
            metric: Some(Metric::ChecksGreen),
            blocked: Some((&b, &skip)),
            ..base(Basis::Clear)
        };
        assert_eq!(say(&input), "fake_b is out of usage, and fake_a passed its checks 9 of 10 times here.");
    }

    #[test]
    fn clear_blocked_shared_plan_names_the_plan() {
        let a = cand("fake_a", Some(checks_9_of_10()));
        let b = limited(cand("fake_b", None));
        let skip = Skip::SharedPlan { with: "fake_c".to_string() };
        let input = SentenceInput {
            chosen: Some(&a),
            metric: Some(Metric::ChecksGreen),
            blocked: Some((&b, &skip)),
            ..base(Basis::Clear)
        };
        assert_eq!(
            say(&input),
            "fake_b shares a plan that is out of usage, and fake_a passed its checks 9 of 10 times here."
        );
    }

    #[test]
    fn only_one_is_ready() {
        let a = cand("fake_a", None);
        let input = SentenceInput { chosen: Some(&a), ..base(Basis::OnlyOne) };
        assert_eq!(say(&input), "Only fake_a is ready.");
    }

    #[test]
    fn only_one_with_a_limited_other_names_it() {
        let a = cand("fake_a", None);
        let b = limited(cand("fake_b", None));
        let skip = Skip::OutOfUsage(Some(1_000));
        let input = SentenceInput { chosen: Some(&a), blocked: Some((&b, &skip)), ..base(Basis::OnlyOne) };
        assert_eq!(say(&input), "Only fake_a is ready, because fake_b is out of usage until 17:00.");
    }

    #[test]
    fn too_early_names_the_usual_one_of_each_kind() {
        let a = cand("fake_a", None);
        for (kind, phrase) in [
            (UsualKind::Chosen, "your usual one, fake_a"),
            (UsualKind::Last, "your usual one, fake_a"),
            (UsualKind::First, "the first one ready, fake_a"),
        ] {
            let input = SentenceInput { chosen: Some(&a), usual: Some(kind), ..base(Basis::TooEarly) };
            assert_eq!(
                say(&input),
                format!("Too early to tell your agents apart here, so {phrase}.")
            );
        }
    }

    #[test]
    fn close_call_names_the_usual_one() {
        let a = cand("fake_a", None);
        let input = SentenceInput { chosen: Some(&a), usual: Some(UsualKind::Chosen), ..base(Basis::CloseCall) };
        assert_eq!(say(&input), "Your agents look about the same here so far, so your usual one, fake_a.");
    }

    #[test]
    fn too_early_and_close_call_with_a_limited_one_say_so() {
        let a = cand("fake_a", None);
        let b = limited(cand("fake_b", None));
        let skip = Skip::OutOfUsage(Some(1_000));
        for basis in [Basis::TooEarly, Basis::CloseCall] {
            let input = SentenceInput {
                chosen: Some(&a),
                usual: Some(UsualKind::Chosen),
                blocked: Some((&b, &skip)),
                ..base(basis)
            };
            assert_eq!(say(&input), "fake_b is out of usage until 17:00, so fake_a.");
        }
    }

    #[test]
    fn none_ready_names_the_first_skipped_agent_and_its_reason() {
        let a = limited(cand("fake_a", None));
        let skip = Skip::OutOfUsage(Some(1_000));
        let input = SentenceInput { first_skipped: Some((&a, &skip)), ..base(Basis::NoneReady) };
        assert_eq!(say(&input), "No agent is ready: fake_a - Out of usage until 17:00.");
    }

    #[test]
    fn none_ready_keeps_a_reason_that_already_ends_with_a_full_stop() {
        let a = cand("fake_a", None);
        let skip = Skip::NotReady("Approve this agent in Settings > Agents first.".to_string());
        let input = SentenceInput { first_skipped: Some((&a, &skip)), ..base(Basis::NoneReady) };
        assert_eq!(
            say(&input),
            "No agent is ready: fake_a - Approve this agent in Settings > Agents first."
        );
    }

    #[test]
    fn none_ready_with_a_shared_plan_reason() {
        let a = cand("fake_b", None);
        let skip = Skip::SharedPlan { with: "fake_c".to_string() };
        let input = SentenceInput { first_skipped: Some((&a, &skip)), ..base(Basis::NoneReady) };
        assert_eq!(
            say(&input),
            "No agent is ready: fake_b - Shares a plan with fake_c, which is out of usage."
        );
    }

    #[test]
    fn none_ready_with_no_agent_at_all() {
        assert_eq!(say(&base(Basis::NoneReady)), "No agent is installed yet.");
    }

    #[test]
    fn a_missing_chosen_candidate_says_no_agent_is_ready() {
        assert_eq!(say(&base(Basis::Clear)), "No agent is ready.");
    }

    #[test]
    fn skip_text_for_each_skip() {
        let when = |ms: u64| format!("t{ms}");
        assert_eq!(skip_text(&Skip::NotReady("Not installed on this computer".into()), &when), "Not installed on this computer");
        assert_eq!(skip_text(&Skip::OutOfUsage(Some(1234)), &when), "Out of usage until t1234");
        assert_eq!(skip_text(&Skip::OutOfUsage(None), &when), "Out of usage");
        assert_eq!(
            skip_text(&Skip::SharedPlan { with: "fake_b".into() }, &when),
            "Shares a plan with fake_b, which is out of usage"
        );
    }

    #[test]
    fn line_of_shows_the_deciding_measure_with_its_counts() {
        let checks = cand("fake_a", Some(ev((9, 10), (4, 6), (18, 20))));
        assert_eq!(line_of(&checks, Some(Metric::ChecksGreen)), "9 of 10 checks green");
        assert_eq!(line_of(&checks, Some(Metric::Merged)), "4 of 6 copies kept");
        assert_eq!(line_of(&checks, Some(Metric::EndedOk)), "18 of 20 turns ended OK");
    }

    #[test]
    fn line_of_adds_too_few_to_count_under_five_runs() {
        let few = cand("fake_a", Some(ev((2, 3), (1, 1), (3, 4))));
        assert_eq!(line_of(&few, Some(Metric::ChecksGreen)), "2 of 3 checks green (too few to count)");
        assert_eq!(line_of(&few, Some(Metric::Merged)), "1 of 1 copies kept (too few to count)");
    }

    #[test]
    fn line_of_has_no_too_few_suffix_at_five_runs_or_more() {
        let early = cand("fake_a", Some(ev((6, 7), (1, 5), (5, 5))));
        assert_eq!(line_of(&early, Some(Metric::ChecksGreen)), "6 of 7 checks green");
        assert_eq!(line_of(&early, Some(Metric::Merged)), "1 of 5 copies kept");
    }

    #[test]
    fn line_of_without_a_metric_uses_the_first_measure_with_runs() {
        let no_checks = cand("fake_a", Some(ev((0, 0), (4, 6), (0, 0))));
        assert_eq!(line_of(&no_checks, None), "4 of 6 copies kept");
        let all_zero = cand("fake_a", Some(ev((0, 0), (0, 0), (0, 0))));
        assert_eq!(line_of(&all_zero, None), "No runs here yet");
    }

    #[test]
    fn line_of_with_a_metric_and_no_runs_says_too_few() {
        let none = cand("fake_a", Some(ev((0, 0), (0, 0), (3, 4))));
        assert_eq!(line_of(&none, Some(Metric::Merged)), "0 of 0 copies kept (too few to count)");
    }

    #[test]
    fn line_of_for_a_ready_candidate_without_a_row() {
        assert_eq!(line_of(&cand("fake_a", None), Some(Metric::ChecksGreen)), "No runs here yet");
    }

    #[test]
    fn line_of_for_a_candidate_that_cannot_run_is_its_reason() {
        let mut c = cand("fake_a", Some(checks_9_of_10()));
        c.ready = Err("Not installed on this computer".to_string());
        assert_eq!(line_of(&c, Some(Metric::ChecksGreen)), "Not installed on this computer");
    }

    #[test]
    fn line_of_for_a_limited_candidate_says_out_of_usage_without_a_time() {
        let c = limited(cand("fake_a", Some(checks_9_of_10())));
        assert_eq!(line_of(&c, Some(Metric::ChecksGreen)), "Out of usage");
    }

    #[test]
    fn short_keeps_a_label_of_forty_characters() {
        let label = "a".repeat(40);
        assert_eq!(short(&label), label);
    }

    #[test]
    fn short_cuts_a_longer_label_to_forty_characters_with_dots() {
        assert_eq!(short(&"a".repeat(41)), format!("{}...", "a".repeat(37)));
        let long = short(&"b".repeat(200));
        assert_eq!(long.chars().count(), 40);
        assert!(long.ends_with("..."));
    }

    #[test]
    fn short_never_splits_a_character() {
        let accented = "é".repeat(60);
        let cut = short(&accented);
        assert_eq!(cut.chars().count(), 40);
        assert_eq!(cut, format!("{}...", "é".repeat(37)));
    }

    #[test]
    fn short_turns_control_characters_into_spaces() {
        assert_eq!(short("  fake_a\nb\t"), "fake_a b");
    }

    #[test]
    fn a_long_label_is_cut_inside_the_sentence() {
        let a = cand(&"x".repeat(200), Some(checks_9_of_10()));
        let input = SentenceInput { chosen: Some(&a), metric: Some(Metric::ChecksGreen), ..base(Basis::Clear) };
        let text = say(&input);
        assert!(!text.contains(&"x".repeat(41)));
        assert!(text.starts_with(&format!("{}... passed its checks", "x".repeat(37))));
    }

    #[test]
    fn the_sentence_is_one_line_even_with_a_newline_in_a_label() {
        let a = cand("fake_a\r\nnext", Some(checks_9_of_10()));
        let b = cand("fake_b\nmore", Some(checks_5_of_10()));
        let input = SentenceInput {
            chosen: Some(&a),
            rival: Some(&b),
            metric: Some(Metric::ChecksGreen),
            ..base(Basis::Clear)
        };
        let text = say(&input);
        assert!(!text.contains('\n') && !text.contains('\r'));
        assert_eq!(text, "fake_a next passed its checks 9 of 10 times here; fake_b more 5 of 10.");
    }

    #[test]
    fn the_sentence_is_capped_at_220_characters() {
        let a = cand("fake_a", Some(checks_9_of_10()));
        let b = limited(cand("fake_b", None));
        let skip = Skip::OutOfUsage(Some(1_000));
        let input = SentenceInput {
            chosen: Some(&a),
            metric: Some(Metric::ChecksGreen),
            blocked: Some((&b, &skip)),
            ..base(Basis::Clear)
        };
        let long_time = |_: u64| "y".repeat(300);
        let text = sentence(&input, &long_time);
        assert_eq!(text.chars().count(), 220);
        assert!(text.ends_with("..."));
    }

    #[test]
    fn a_short_sentence_is_not_cut() {
        let a = cand("fake_a", None);
        let input = SentenceInput { chosen: Some(&a), ..base(Basis::OnlyOne) };
        assert_eq!(say(&input), "Only fake_a is ready.");
    }

    #[test]
    fn no_function_takes_a_folder_a_path_or_a_pane() {
        let source = include_str!("reason.rs");
        for line in source.lines().map(str::trim_start).filter(|l| l.starts_with("pub ")) {
            let lower = line.to_ascii_lowercase();
            for word in ["folder", "path", "pane", "project"] {
                assert!(!lower.contains(word), "public item takes {word}: {line}");
            }
        }
    }

}
