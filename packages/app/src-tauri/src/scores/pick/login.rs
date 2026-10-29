//! The usual saved login of an agent in this project, and the evidence of one Scores row. Only
//! the numbers are copied: no other field of the row is read (so `undone` and `asked_you` never
//! reach a pick).

use super::model::Evidence;
use crate::scores::model::ScoreRow;

/// One row of this project's history for one agent: the login (`None` = the agent's own login) and
/// its turns. Nothing else is needed to choose the usual login.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowFact {
    pub account_id: Option<String>,
    pub turns: u32,
}

/// The usual login of an agent here (rule 3.3). Among the rows whose login is the agent's own, or a
/// saved login that `valid` accepts (`accounts::validate_for_run`), and that have at least one turn,
/// the row with the most turns wins. A tie goes to the agent's own login, then to the smaller id.
/// Each row is judged on its own; rows are never summed over logins.
///
/// `None` means the agent's own login. That is also the answer with no usable rows. A saved login
/// with no history in this project is never suggested, because it is not in `rows`.
pub fn usual_login(rows: &[RowFact], valid: &dyn Fn(&str) -> bool) -> Option<String> {
    let mut best: Option<&RowFact> = None;
    for row in rows {
        if row.turns == 0 || !login_ok(row, valid) {
            continue;
        }
        best = match best {
            Some(current) if !beats(row, current) => Some(current),
            _ => Some(row),
        };
    }
    best.and_then(|row| row.account_id.clone())
}

/// The agent's own login always passes. A saved login must pass `valid`.
fn login_ok(row: &RowFact, valid: &dyn Fn(&str) -> bool) -> bool {
    match row.account_id.as_deref() {
        None => true,
        Some(id) => valid(id),
    }
}

/// True when `a` is the better usual login than `b`: more turns; then the agent's own login; then
/// the smaller saved login id.
fn beats(a: &RowFact, b: &RowFact) -> bool {
    if a.turns != b.turns {
        return a.turns > b.turns;
    }
    match (a.account_id.as_deref(), b.account_id.as_deref()) {
        (None, Some(_)) => true,
        (Some(_), None) => false,
        (Some(x), Some(y)) => x < y,
        (None, None) => false,
    }
}

/// The evidence of one Scores row: its turns and the three measures the pick compares. Nothing
/// else is copied.
pub fn evidence_of(row: &ScoreRow) -> Evidence {
    Evidence { turns: row.turns, checks_green: row.checks_green, merged: row.merged, ended_ok: row.ended_ok }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scores::model::Sample;
    use crate::scores::stats::rate;

    fn fact(account: Option<&str>, turns: u32) -> RowFact {
        RowFact { account_id: account.map(str::to_string), turns }
    }

    fn any_login(_: &str) -> bool {
        true
    }

    #[test]
    fn most_turns_wins_between_own_and_saved_login() {
        let rows = [fact(None, 3), fact(Some("work"), 7)];
        assert_eq!(usual_login(&rows, &any_login), Some("work".to_string()));
        let rows = [fact(Some("work"), 2), fact(None, 9)];
        assert_eq!(usual_login(&rows, &any_login), None);
    }

    #[test]
    fn tie_goes_to_the_agents_own_login() {
        let rows = [fact(Some("work"), 5), fact(None, 5)];
        assert_eq!(usual_login(&rows, &any_login), None);
        let rows = [fact(None, 5), fact(Some("work"), 5)];
        assert_eq!(usual_login(&rows, &any_login), None);
    }

    #[test]
    fn tie_of_two_saved_logins_goes_to_the_smaller_id() {
        let rows = [fact(Some("zeta"), 4), fact(Some("alpha"), 4)];
        assert_eq!(usual_login(&rows, &any_login), Some("alpha".to_string()));
        let rows = [fact(Some("alpha"), 4), fact(Some("zeta"), 4)];
        assert_eq!(usual_login(&rows, &any_login), Some("alpha".to_string()));
    }

    #[test]
    fn removed_login_is_ignored() {
        let valid = |id: &str| id != "gone";
        let rows = [fact(Some("gone"), 20), fact(Some("work"), 2)];
        assert_eq!(usual_login(&rows, &valid), Some("work".to_string()));
        let rows = [fact(Some("gone"), 20), fact(None, 1)];
        assert_eq!(usual_login(&rows, &valid), None);
        let rows = [fact(Some("gone"), 20)];
        assert_eq!(usual_login(&rows, &valid), None);
    }

    #[test]
    fn zero_turn_rows_are_ignored() {
        let rows = [fact(Some("work"), 0), fact(None, 2)];
        assert_eq!(usual_login(&rows, &any_login), None);
        let rows = [fact(Some("work"), 0), fact(Some("other"), 1)];
        assert_eq!(usual_login(&rows, &any_login), Some("other".to_string()));
        let rows = [fact(Some("work"), 0)];
        assert_eq!(usual_login(&rows, &any_login), None);
    }

    #[test]
    fn no_rows_gives_the_agents_own_login() {
        assert_eq!(usual_login(&[], &any_login), None);
    }

    #[test]
    fn login_with_history_only_in_other_projects_cannot_win() {
        // The caller reads this project only. A login used only in another project is not a row
        // here, so it is not in the input and cannot be chosen.
        let rows = [fact(None, 2)];
        assert_eq!(usual_login(&rows, &any_login), None);
    }

    fn sample_row() -> ScoreRow {
        ScoreRow {
            agent_id: "fake_a".into(),
            agent_label: "fake_a".into(),
            account_id: Some("acc_1".into()),
            account_label: "Work".into(),
            broke_tests: 0,
            turns: 12,
            enough: true,
            ended_ok: rate(11, 12),
            checks_green: rate(9, 10),
            merged: rate(4, 6),
            undone: rate(1, 12),
            asked_you: rate(3, 12),
            per_merged: None,
            waiting_review: 0,
            turn_ms_median: None,
            lines_median: None,
            ahead: vec![],
        }
    }

    #[test]
    fn evidence_copies_the_turns_and_the_three_measures() {
        let row = sample_row();
        let ev = evidence_of(&row);
        assert_eq!(ev.turns, 12);
        assert_eq!(ev.checks_green, rate(9, 10));
        assert_eq!(ev.merged, rate(4, 6));
        assert_eq!(ev.ended_ok, rate(11, 12));
    }

    #[test]
    fn evidence_has_no_undone_or_asked_you_field() {
        // Compile-time guard: the pattern names every field, so a new field (such as `undone`)
        // breaks this test until it is handled here on purpose.
        let Evidence { turns, checks_green, merged, ended_ok } = evidence_of(&sample_row());
        assert_eq!(turns, 12);
        assert_eq!(checks_green.sample, Sample::Steady);
        assert_eq!(merged.count, 4);
        assert_eq!(ended_ok.total, 12);
    }
}
