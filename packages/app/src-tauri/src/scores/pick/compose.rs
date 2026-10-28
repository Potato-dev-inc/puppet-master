//! Puts the steps together into one `Pick`: the skips, the decision, the limits-cleared check for
//! the blocked mention, the choice, the one sentence and the "Why this one" lines. Takes no folder,
//! pane id or task text; the labels are trusted registry data.

use super::model::{
    Basis, Cand, Choice, Considered, ConsideredState, Metric, Pick, Skip, Usual, MAX_CONSIDERED, PICK_DAYS,
};
use super::reason::{self, SentenceInput};
use super::ready::{self, Split};
use super::select;
use crate::scores::model::Sample;

/// The three plain notes under "Why this one" (spec 3.7). The fourth, for a history that could not
/// be read, is added by `live.rs`, the only place that knows about the history.
fn notes() -> Vec<String> {
    vec![
        format!("Counted from your own turns in this project, last {PICK_DAYS} days."),
        "Puppet Master only knows about limits it has seen.".to_string(),
        "It looks at the whole project, not at what you type.".to_string(),
    ]
}

/// The answer for the candidates (spec 3.1 to 3.7). `when` formats a reset time.
pub fn pick(cands: &[Cand], usual: &Usual, when: &dyn Fn(u64) -> String) -> Pick {
    let split = ready::split(cands);
    let decision = select::decide(&split.ready, usual);
    let chosen = decision.as_ref().map(|d| split.ready[d.index]);
    let chosen_index = chosen.and_then(|c| index_of(cands, c));
    let blocked = if decision.is_some() { blocked_by_limits(cands, &split, usual, chosen_index) } else { None };
    let blocked_pair = blocked.as_ref().map(|(index, skip)| (&cands[*index], skip));

    let basis = decision.as_ref().map_or(Basis::NoneReady, |d| d.basis);
    let metric = decision.as_ref().and_then(|d| d.metric);
    // Only a clear pick has a deciding rate; its sample decides "so far" and the early chip.
    let sample = match (basis, chosen, metric) {
        (Basis::Clear, Some(c), Some(m)) => c.evidence.map_or(Sample::None, |ev| ev.rate(m).sample),
        _ => Sample::None,
    };
    let input = SentenceInput {
        basis,
        chosen,
        rival: decision.as_ref().and_then(|d| d.rival).map(|i| split.ready[i]),
        metric,
        usual: decision.as_ref().and_then(|d| d.usual),
        sample,
        blocked: blocked_pair,
        first_skipped: split.skipped.first().map(|(cand, skip)| (*cand, skip)),
    };
    Pick {
        choice: chosen.map(|c| Choice {
            agent_id: c.target.agent_id.clone(),
            account_id: c.target.account.clone(),
            label: c.label.clone(),
        }),
        reason: reason::sentence(&input, when),
        basis,
        metric,
        sample,
        considered: considered(&split, chosen, metric, when),
        notes: notes(),
    }
}

/// The position of `cand` in `cands`. Candidates are compared by address, so two equal copies are
/// never mistaken for each other.
fn index_of(cands: &[Cand], cand: &Cand) -> Option<usize> {
    cands.iter().position(|c| std::ptr::eq(c, cand))
}

/// The candidate that a pick would choose with every limit cleared, when it is not the pick and it
/// is skipped for usage now (spec 3.5). Returns its position in `cands` and its skip. Clearing a
/// limit can only add ready candidates, so the same decision runs again on a copy.
fn blocked_by_limits(cands: &[Cand], split: &Split<'_>, usual: &Usual, chosen: Option<usize>) -> Option<(usize, Skip)> {
    let cleared: Vec<Cand> = cands.iter().map(|c| Cand { limited_until: None, ..c.clone() }).collect();
    let cleared_split = ready::split(&cleared);
    let decision = select::decide(&cleared_split.ready, usual)?;
    let index = index_of(&cleared, cleared_split.ready[decision.index])?;
    if Some(index) == chosen {
        return None;
    }
    let (_, skip) = split.skipped.iter().find(|(cand, _)| std::ptr::eq(*cand, &cands[index]))?;
    match skip {
        Skip::OutOfUsage(_) | Skip::SharedPlan { .. } => Some((index, skip.clone())),
        Skip::NotReady(_) => None,
    }
}

/// "Why this one": the chosen one first, then the other ready ones, then the skipped ones (each in
/// input order). Capped at `MAX_CONSIDERED` lines.
fn considered(
    split: &Split<'_>,
    chosen: Option<&Cand>,
    metric: Option<Metric>,
    when: &dyn Fn(u64) -> String,
) -> Vec<Considered> {
    let others = split.ready.iter().copied().filter(|c| chosen.map_or(true, |x| !std::ptr::eq(x, *c)));
    let mut lines: Vec<Considered> = chosen
        .into_iter()
        .chain(others)
        .map(|cand| Considered {
            label: cand.label.clone(),
            state: ConsideredState::Ready,
            line: reason::line_of(cand, metric),
        })
        .collect();
    lines.extend(split.skipped.iter().map(|(cand, skip)| Considered {
        label: cand.label.clone(),
        state: ConsideredState::Skipped,
        line: reason::skip_text(skip, when),
    }));
    lines.truncate(MAX_CONSIDERED);
    lines
}

#[cfg(test)]
mod tests {
    use super::super::model::{Evidence, Metric, Usual};
    use super::*;
    use crate::handoff::keep_going::model::Target;
    use crate::scores::stats::rate;
    use serde_json::json;

    /// A reset time that is always 17:00.
    fn at(_ms: u64) -> String {
        "17:00".to_string()
    }

    /// Evidence with `checks` on every measure (the same counts for merged and ended OK).
    fn ev(checks: (u32, u32)) -> Evidence {
        let r = rate(checks.0, checks.1);
        Evidence { turns: checks.1, checks_green: r, merged: r, ended_ok: r }
    }

    /// A ready agent with no login, no vendor, no limit and no history.
    fn fake(agent: &str) -> Cand {
        Cand {
            target: Target { agent_id: agent.to_string(), account: None },
            label: agent.to_string(),
            vendor: None,
            ready: Ok(()),
            limited_until: None,
            evidence: None,
        }
    }

    fn with(mut cand: Cand, checks: (u32, u32)) -> Cand {
        cand.evidence = Some(ev(checks));
        cand
    }

    fn out(mut cand: Cand, until: Option<u64>) -> Cand {
        cand.limited_until = Some(until);
        cand
    }

    fn vendor(mut cand: Cand, name: &str) -> Cand {
        cand.vendor = Some(name.to_string());
        cand
    }

    fn usual_ids(preferred: Option<&str>, last: Option<&str>) -> Usual {
        Usual { preferred: preferred.map(str::to_string), last: last.map(str::to_string) }
    }

    fn pick_of(cands: &[Cand], usual: &Usual) -> Pick {
        pick(cands, usual, &at)
    }

    fn chosen_id(p: &Pick) -> Option<&str> {
        p.choice.as_ref().map(|c| c.agent_id.as_str())
    }

    #[test]
    fn a_limited_usual_agent_is_skipped_and_the_sentence_says_so() {
        let cands = [out(fake("fake_a"), Some(1_000)), fake("fake_b")];
        let p = pick_of(&cands, &usual_ids(Some("fake_a"), None));
        assert_eq!(chosen_id(&p), Some("fake_b"));
        assert_eq!(p.basis, Basis::OnlyOne);
        assert_eq!(p.reason, "Only fake_b is ready, because fake_a is out of usage until 17:00.");
        assert_eq!(p.considered[1].state, ConsideredState::Skipped);
        assert_eq!(p.considered[1].line, "Out of usage until 17:00");
    }

    #[test]
    fn four_runs_each_is_too_early_so_the_usual_agent_is_used() {
        let cands = [with(fake("fake_a"), (4, 4)), with(fake("fake_b"), (4, 4))];
        let p = pick_of(&cands, &usual_ids(Some("fake_b"), None));
        assert_eq!(chosen_id(&p), Some("fake_b"));
        assert_eq!(p.basis, Basis::TooEarly);
        assert_eq!(p.metric, None);
        assert_eq!(p.sample, Sample::None);
        assert_eq!(p.reason, "Too early to tell your agents apart here, so your usual one, fake_b.");
    }

    #[test]
    fn a_tie_goes_to_the_usual_agent() {
        let cands = [with(fake("fake_a"), (10, 10)), with(fake("fake_b"), (10, 10))];
        let p = pick_of(&cands, &usual_ids(None, Some("fake_b")));
        assert_eq!(chosen_id(&p), Some("fake_b"));
        assert_eq!(p.basis, Basis::CloseCall);
        assert_eq!(p.metric, Some(Metric::ChecksGreen));
        assert_eq!(p.reason, "Your agents look about the same here so far, so your usual one, fake_b.");
    }

    #[test]
    fn a_clear_leader_is_picked_when_the_usual_agent_is_limited() {
        // Fake A is the usual agent and is out of usage; the clear leader is fake B (10 of 10).
        let cands = [
            out(with(fake("fake_a"), (9, 10)), Some(1_000)),
            with(fake("fake_b"), (10, 10)),
            with(fake("fake_c"), (5, 10)),
        ];
        let p = pick_of(&cands, &usual_ids(Some("fake_a"), None));
        assert_eq!(chosen_id(&p), Some("fake_b"));
        assert_eq!(p.basis, Basis::Clear);
        assert_eq!(p.metric, Some(Metric::ChecksGreen));
        assert_eq!(p.sample, Sample::Steady);
        assert_eq!(
            p.reason,
            "fake_a is out of usage until 17:00, and fake_b passed its checks 10 of 10 times here."
        );
        let labels: Vec<&str> = p.considered.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["fake_b", "fake_c", "fake_a"]);
    }

    #[test]
    fn a_blocked_agent_is_not_named_when_the_unblocked_pick_is_the_same() {
        let cands = [
            out(with(fake("fake_a"), (10, 10)), Some(1_000)),
            with(fake("fake_b"), (10, 10)),
            with(fake("fake_c"), (5, 10)),
        ];
        let p = pick_of(&cands, &usual_ids(Some("fake_b"), None));
        assert_eq!(chosen_id(&p), Some("fake_b"));
        assert_eq!(p.reason, "fake_b passed its checks 10 of 10 times here; fake_c 5 of 10.");
        assert_eq!(p.considered[2].label, "fake_a");
        assert_eq!(p.considered[2].line, "Out of usage until 17:00");
    }

    #[test]
    fn a_shared_plan_with_a_limited_agent_leaves_nothing_ready() {
        let cands = [
            vendor(out(fake("fake_a"), Some(1_000)), "vendor_x"),
            vendor(fake("fake_b"), "vendor_x"),
        ];
        let p = pick_of(&cands, &usual_ids(None, None));
        assert_eq!(p.choice, None);
        assert_eq!(p.basis, Basis::NoneReady);
        assert_eq!(p.reason, "No agent is ready: fake_a - Out of usage until 17:00.");
        assert_eq!(p.considered[1].line, "Shares a plan with fake_a, which is out of usage");
        assert_eq!(p.considered[1].state, ConsideredState::Skipped);
    }

    #[test]
    fn a_shared_plan_does_not_block_an_agent_of_another_plan() {
        let cands = [
            vendor(out(fake("fake_a"), Some(1_000)), "vendor_x"),
            vendor(fake("fake_b"), "vendor_x"),
            vendor(fake("fake_c"), "vendor_y"),
        ];
        let p = pick_of(&cands, &usual_ids(None, None));
        assert_eq!(chosen_id(&p), Some("fake_c"));
        assert_eq!(p.reason, "Only fake_c is ready, because fake_a is out of usage until 17:00.");
        let lines: Vec<&str> = p.considered.iter().map(|c| c.line.as_str()).collect();
        assert_eq!(
            lines,
            [
                "No runs here yet",
                "Out of usage until 17:00",
                "Shares a plan with fake_a, which is out of usage"
            ]
        );
    }

    #[test]
    fn when_nothing_is_ready_there_is_no_choice_and_one_sentence() {
        let mut not_ready = fake("fake_a");
        not_ready.ready = Err("Not installed on this computer".to_string());
        let mut unapproved = fake("fake_a");
        unapproved.ready = Err("Approve this agent in Settings > Agents first.".to_string());
        let cases = [
            (not_ready, "No agent is ready: fake_a - Not installed on this computer."),
            (unapproved, "No agent is ready: fake_a - Approve this agent in Settings > Agents first."),
            (out(fake("fake_a"), Some(1_000)), "No agent is ready: fake_a - Out of usage until 17:00."),
        ];
        for (cand, text) in cases {
            let p = pick_of(&[cand], &usual_ids(None, None));
            assert_eq!(p.choice, None);
            assert_eq!(p.basis, Basis::NoneReady);
            assert_eq!(p.reason, text);
        }
    }

    #[test]
    fn with_no_agent_at_all_the_sentence_says_none_is_installed() {
        let p = pick_of(&[], &usual_ids(None, None));
        assert_eq!(p.choice, None);
        assert_eq!(p.basis, Basis::NoneReady);
        assert_eq!(p.reason, "No agent is installed yet.");
        assert!(p.considered.is_empty());
    }

    #[test]
    fn the_reason_is_one_line_of_at_most_220_characters() {
        let long = fake(&format!("fake_a\n{}", "x".repeat(200)));
        let other = fake("fake_b\r\nnext");
        let cands = [with(long, (10, 10)), with(other, (5, 10))];
        let p = pick_of(&cands, &usual_ids(None, None));
        assert!(!p.reason.contains('\n') && !p.reason.contains('\r'));
        assert!(p.reason.chars().count() <= 220);

        // A reset text of 300 characters is cut to the 220-character limit with "...".
        let cands = [
            out(with(fake("fake_a"), (9, 10)), Some(1_000)),
            with(fake("fake_b"), (10, 10)),
            with(fake("fake_c"), (5, 10)),
        ];
        let long_time = |_: u64| "y".repeat(300);
        let p = pick(&cands, &usual_ids(Some("fake_a"), None), &long_time);
        assert_eq!(p.reason.chars().count(), 220);
        assert!(p.reason.ends_with("..."));
        assert!(!p.reason.contains('\n'));
    }

    #[test]
    fn the_sample_of_a_clear_pick_is_the_leaders_sample() {
        let early = [with(fake("fake_a"), (5, 5)), with(fake("fake_b"), (0, 5))];
        let p = pick_of(&early, &usual_ids(None, None));
        assert_eq!(p.basis, Basis::Clear);
        assert_eq!(p.sample, Sample::Early);
        assert_eq!(p.reason, "fake_a passed its checks 5 of 5 times here so far; fake_b 0 of 5.");

        let steady = [with(fake("fake_a"), (10, 10)), with(fake("fake_b"), (5, 10))];
        assert_eq!(pick_of(&steady, &usual_ids(None, None)).sample, Sample::Steady);

        let too_early = [with(fake("fake_a"), (4, 4)), with(fake("fake_b"), (4, 4))];
        assert_eq!(pick_of(&too_early, &usual_ids(None, None)).sample, Sample::None);
    }

    #[test]
    fn the_choice_names_the_saved_login_only_when_there_is_one() {
        let mut saved = fake("fake_a");
        saved.target.account = Some("acc_1".to_string());
        saved.label = "fake_a . Work".to_string();
        let choice = pick_of(&[saved], &usual_ids(None, None)).choice.unwrap();
        assert_eq!(choice.account_id.as_deref(), Some("acc_1"));
        assert_eq!(choice.label, "fake_a . Work");

        let own = pick_of(&[fake("fake_a")], &usual_ids(None, None)).choice.unwrap();
        assert_eq!(own.account_id, None);
    }

    #[test]
    fn why_this_one_lists_the_chosen_then_ready_then_skipped() {
        let cands = [
            out(fake("fake_a"), Some(1_000)),
            fake("fake_b"),
            fake("fake_c"),
            {
                let mut d = fake("fake_d");
                d.ready = Err("Not installed on this computer".to_string());
                d
            },
        ];
        let p = pick_of(&cands, &usual_ids(None, None));
        let labels: Vec<&str> = p.considered.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["fake_b", "fake_c", "fake_a", "fake_d"]);
        let states: Vec<ConsideredState> = p.considered.iter().map(|c| c.state).collect();
        assert_eq!(
            states,
            [ConsideredState::Ready, ConsideredState::Ready, ConsideredState::Skipped, ConsideredState::Skipped]
        );
        assert_eq!(p.considered[3].line, "Not installed on this computer");
    }

    #[test]
    fn why_this_one_is_capped_at_max_considered() {
        let cands: Vec<Cand> = (0..30).map(|i| fake(&format!("fake_{i:02}"))).collect();
        let p = pick_of(&cands, &usual_ids(None, None));
        assert_eq!(p.considered.len(), MAX_CONSIDERED);
        assert_eq!(p.considered[0].label, "fake_00");
    }

    #[test]
    fn the_notes_are_the_three_plain_sentences() {
        let p = pick_of(&[fake("fake_a")], &usual_ids(None, None));
        assert_eq!(
            p.notes,
            [
                "Counted from your own turns in this project, last 90 days.",
                "Puppet Master only knows about limits it has seen.",
                "It looks at the whole project, not at what you type.",
            ]
        );
    }

    #[test]
    fn the_answer_has_the_shape_of_the_typescript_type() {
        let cands = [with(fake("fake_a"), (10, 10)), with(fake("fake_b"), (5, 10))];
        let v = serde_json::to_value(pick_of(&cands, &usual_ids(None, None))).unwrap();
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["basis", "choice", "considered", "metric", "notes", "reason", "sample"]);
        assert_eq!(v["basis"], json!("clear"));
        assert_eq!(v["metric"], json!("checks_green"));
        assert_eq!(v["sample"], json!("steady"));
        assert_eq!(v["choice"], json!({ "agentId": "fake_a", "label": "fake_a" }));
        assert_eq!(v["considered"][0]["state"], json!("ready"));
        assert_eq!(v["considered"][1]["line"], json!("5 of 10 checks green"));
    }

    #[test]
    fn pick_takes_no_folder_pane_or_task() {
        let source = include_str!("compose.rs");
        let signature = source.lines().find(|l| l.starts_with("pub fn pick(")).unwrap();
        for word in ["folder", "pane", "task", "prompt", "path", "project"] {
            assert!(!signature.contains(word), "public signature takes {word}: {signature}");
        }
    }
}
