//! Finds the turn a clip is made from, and the time window of that turn.
//!
//! A turn is one `User` item and the items after it, up to the next `User` item. It is finished when it
//! has a `TurnEnd` or a later `User` item exists. Its window is the real start and end of the turn
//! summary that overlaps the turn's chat items the most. Items without times (a screen source) can only
//! be clipped when the turn is the newest one.

use chrono::DateTime;

use crate::session_chat::model::ChatItem;
use crate::turn_summary::TurnSummary;

/// Refusal for a turn that has not ended yet.
pub const STILL_RUNNING: &str = "This turn is still running. Make the clip when it ends.";
/// Refusal for an older turn whose chat items carry no times.
pub const NO_TIMES: &str = "This turn has no times, so it cannot be clipped. Only the newest turn can.";
/// Refusal when a turn has no times and there is no turn summary to take the window from.
pub const NOTHING: &str = "There is nothing to show for this turn.";

/// One turn of a chat: the `User` item and the items up to the next one.
pub struct Turn<'a> {
    /// The items of the turn, starting with its `User` item.
    pub items: &'a [ChatItem],
    /// The `User` item that starts the turn.
    pub user: Option<&'a ChatItem>,
    /// True when the turn has a `TurnEnd` or a later `User` item exists.
    pub finished: bool,
    /// True when this is the last turn of the chat. Only the newest turn can be clipped without times.
    pub newest: bool,
}

/// Finds the turn that starts with the `User` item `user_item_id`, or the last turn when it is `None`.
pub fn find_turn<'a>(items: &'a [ChatItem], user_item_id: Option<&str>) -> Result<Turn<'a>, String> {
    let users: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| matches!(item, ChatItem::User { .. }))
        .map(|(index, _)| index)
        .collect();
    let last = *users.last().ok_or_else(|| "There is no message in this chat to clip.".to_string())?;
    let start = match user_item_id {
        None => last,
        Some(id) => *users
            .iter()
            .find(|&&index| items[index].id() == id)
            .ok_or_else(|| "This message is not in this chat.".to_string())?,
    };
    let next = users.iter().copied().find(|&index| index > start);
    let end = next.unwrap_or(items.len());
    let turn_items = &items[start..end];
    let finished = next.is_some() || turn_items.iter().any(|item| matches!(item, ChatItem::TurnEnd { .. }));
    Ok(Turn { items: turn_items, user: Some(&items[start]), finished, newest: start == last })
}

/// The time window `[start_ms, end_ms]` of a finished turn (unix ms), with the summary it came from.
///
/// With times: the summary that overlaps the turn's items the most. With no overlapping summary, the
/// window is the first to the last item time and the summary is `None`. Without times: only the newest
/// turn can be clipped, with the newest summary.
pub fn window_for(turn: &Turn, summaries: &[TurnSummary]) -> Result<(u64, u64, Option<TurnSummary>), String> {
    if !turn.finished {
        return Err(STILL_RUNNING.to_string());
    }
    let times: Vec<u64> = turn.items.iter().filter_map(item_ms).collect();
    let (Some(&first), Some(&last)) = (times.iter().min(), times.iter().max()) else {
        if !turn.newest {
            return Err(NO_TIMES.to_string());
        }
        return match summaries.iter().max_by_key(|s| s.ended_at_ms) {
            Some(s) => Ok((s.started_at_ms, s.ended_at_ms, Some(s.clone()))),
            None => Err(NOTHING.to_string()),
        };
    };
    let best = summaries
        .iter()
        .filter_map(|s| {
            let lo = first.max(s.started_at_ms);
            let hi = last.min(s.ended_at_ms);
            (lo <= hi).then(|| (hi - lo, s.started_at_ms, s))
        })
        .max_by_key(|(overlap, started, _)| (*overlap, *started));
    match best {
        Some((_, _, s)) => Ok((s.started_at_ms, end_with_answer(s, turn, summaries), Some(s.clone()))),
        None => Ok((first, last, None)),
    }
}

/// The most the window may grow past the summary's end to reach the agent's last item.
const ANSWER_GRACE_MS: u64 = 120_000;

/// The summary ends when the pane first rests (its status turns idle). An agent can rest on the screen a
/// moment before it draws its answer, so the clip used to stop on a spinner line and skip the answer.
/// The chat's own times are the agent's record of when the turn really ended: the window reaches the last
/// item of the turn when that is later. It stops at the start of the next summary of the pane, and at
/// `ANSWER_GRACE_MS` past the summary's end.
fn end_with_answer(chosen: &TurnSummary, turn: &Turn, summaries: &[TurnSummary]) -> u64 {
    let last_item = turn
        .items
        .iter()
        .filter(|item| matches!(item, ChatItem::TurnEnd { .. } | ChatItem::Assistant { .. } | ChatItem::ToolCall { .. } | ChatItem::ToolResult { .. }))
        .filter_map(item_ms)
        .max()
        .unwrap_or(0);
    let mut end = chosen.ended_at_ms.max(last_item.min(chosen.ended_at_ms.saturating_add(ANSWER_GRACE_MS)));
    let next_start = summaries
        .iter()
        .filter(|s| s.started_at_ms >= chosen.ended_at_ms && s.started_at_ms > chosen.started_at_ms)
        .map(|s| s.started_at_ms)
        .min();
    if let Some(next) = next_start {
        end = end.min(next.max(chosen.ended_at_ms));
    }
    end
}

/// The unix time of a chat item in milliseconds. `None` when the item has no time or it does not parse.
pub(crate) fn item_ms(item: &ChatItem) -> Option<u64> {
    let at = match item {
        ChatItem::User { at, .. }
        | ChatItem::Assistant { at, .. }
        | ChatItem::ToolCall { at, .. }
        | ChatItem::ToolResult { at, .. }
        | ChatItem::TurnEnd { at, .. }
        | ChatItem::Usage { at, .. }
        | ChatItem::Session { at, .. }
        | ChatItem::Plan { at, .. }
        | ChatItem::Notice { at, .. } => at.as_deref()?,
    };
    let ms = DateTime::parse_from_rfc3339(at).ok()?.timestamp_millis();
    u64::try_from(ms).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_chat::model::TurnOutcome;
    use crate::turn_summary::model::EndState;

    fn user(id: &str, at: Option<&str>) -> ChatItem {
        ChatItem::User { id: id.into(), at: at.map(Into::into), text: "do it".into() }
    }

    fn reply(id: &str, at: Option<&str>) -> ChatItem {
        ChatItem::Assistant { id: id.into(), at: at.map(Into::into), text: "done".into(), model: None, final_answer: true }
    }

    fn end(id: &str, at: Option<&str>) -> ChatItem {
        ChatItem::TurnEnd { id: id.into(), at: at.map(Into::into), outcome: TurnOutcome::Completed, duration_ms: None }
    }

    fn summary(started_at_ms: u64, ended_at_ms: u64) -> TurnSummary {
        TurnSummary {
            pane_id: "pane-1".into(),
            agent: "Test Agent".into(),
            agent_id: None,
            account: None,
            turn: 1,
            started_at_ms,
            ended_at_ms,
            duration_ms: ended_at_ms - started_at_ms,
            files: None,
            approvals: Vec::new(),
            end: EndState { kind: "idle".into(), prompt: None },
            receipt: None,
            sentence: "Done.".into(),
        }
    }

    /// Unix ms of an RFC 3339 text, for building expected values.
    fn ms(text: &str) -> u64 {
        DateTime::parse_from_rfc3339(text).unwrap().timestamp_millis() as u64
    }

    #[test]
    fn last_turn_is_used_when_no_id_is_given() {
        let items = vec![user("u1", None), reply("a1", None), end("e1", None), user("u2", None), reply("a2", None)];
        let turn = find_turn(&items, None).unwrap();
        assert_eq!(turn.user.unwrap().id(), "u2");
        assert_eq!(turn.items.len(), 2);
        assert!(turn.newest);
        assert!(!turn.finished, "the last turn has no TurnEnd and no later user item");
    }

    #[test]
    fn a_turn_is_found_by_its_user_item_id() {
        let items = vec![user("u1", None), reply("a1", None), end("e1", None), user("u2", None), reply("a2", None)];
        let turn = find_turn(&items, Some("u1")).unwrap();
        assert_eq!(turn.user.unwrap().id(), "u1");
        assert_eq!(turn.items.len(), 3, "the turn stops before the next user item");
        assert!(turn.finished, "it has a TurnEnd");
        assert!(!turn.newest);
    }

    #[test]
    fn an_older_turn_without_turn_end_is_finished_by_the_next_user_item() {
        let items = vec![user("u1", None), reply("a1", None), user("u2", None), reply("a2", None), end("e2", None)];
        let turn = find_turn(&items, Some("u1")).unwrap();
        assert_eq!(turn.items.len(), 2);
        assert!(turn.finished);
    }

    #[test]
    fn unknown_id_and_chat_without_user_items_are_refused() {
        let items = vec![user("u1", None), reply("a1", None)];
        assert_eq!(find_turn(&items, Some("nope")).err().unwrap(), "This message is not in this chat.");
        // The id of an agent item is not a turn start.
        assert!(find_turn(&items, Some("a1")).is_err());
        let none: Vec<ChatItem> = vec![reply("a1", None)];
        assert!(find_turn(&none, None).is_err());
        assert!(find_turn(&[], None).is_err());
    }

    #[test]
    fn unfinished_turn_is_refused_with_the_exact_sentence() {
        let items = vec![user("u1", Some("2026-10-28T10:00:00Z")), reply("a1", Some("2026-10-28T10:00:05Z"))];
        let turn = find_turn(&items, None).unwrap();
        let err = window_for(&turn, &[summary(ms("2026-10-28T10:00:00Z"), ms("2026-10-28T10:00:09Z"))]).err().unwrap();
        assert_eq!(err, "This turn is still running. Make the clip when it ends.");
    }

    #[test]
    fn the_summary_with_the_largest_overlap_is_chosen() {
        let items = vec![
            user("u1", Some("2026-10-28T10:00:00Z")),
            reply("a1", Some("2026-10-28T10:00:20Z")),
            end("e1", Some("2026-10-28T10:00:25Z")),
        ];
        let turn = find_turn(&items, None).unwrap();
        let early = summary(ms("2026-10-28T10:00:05Z"), ms("2026-10-28T10:00:10Z")); // 5 s overlap
        let main = summary(ms("2026-10-28T10:00:00Z"), ms("2026-10-28T10:00:24Z")); // 24 s overlap
        let summaries = vec![early, main.clone()];
        let (start, end, chosen) = window_for(&turn, &summaries).unwrap();
        // The window reaches the turn end item (25 s), a second after the summary's end.
        assert_eq!((start, end), (main.started_at_ms, ms("2026-10-28T10:00:25Z")));
        assert_eq!(chosen, Some(main));
    }

    #[test]
    fn the_window_reaches_the_last_item_when_the_summary_ended_before_it() {
        let items = vec![
            user("u1", Some("2026-10-28T10:00:00Z")),
            reply("a1", Some("2026-10-28T10:00:12Z")),
            end("e1", Some("2026-10-28T10:00:13Z")),
        ];
        let turn = find_turn(&items, None).unwrap();
        let s = summary(ms("2026-10-28T10:00:00Z"), ms("2026-10-28T10:00:10Z"));
        let (start, end_ms, _) = window_for(&turn, std::slice::from_ref(&s)).unwrap();
        assert_eq!(start, s.started_at_ms);
        assert_eq!(end_ms, ms("2026-10-28T10:00:13Z"));
    }

    #[test]
    fn the_window_never_runs_into_the_next_turn_or_past_the_grace() {
        let items = vec![
            user("u1", Some("2026-10-28T10:00:00Z")),
            end("e1", Some("2026-10-28T10:00:30Z")),
        ];
        let turn = find_turn(&items, None).unwrap();
        let first = summary(ms("2026-10-28T10:00:00Z"), ms("2026-10-28T10:00:10Z"));
        let next = summary(ms("2026-10-28T10:00:20Z"), ms("2026-10-28T10:00:25Z"));
        let (_, end_ms, _) = window_for(&turn, &[first.clone(), next]).unwrap();
        assert_eq!(end_ms, ms("2026-10-28T10:00:20Z"), "stops where the next summary starts");
        let (_, end_ms, _) = window_for(&turn, &[first]).unwrap();
        assert_eq!(end_ms, ms("2026-10-28T10:00:30Z"));
        // A turn end hours later is not followed.
        let far = vec![user("u1", Some("2026-10-28T10:00:00Z")), end("e1", Some("2026-10-28T14:00:00Z"))];
        let turn = find_turn(&far, None).unwrap();
        let s = summary(ms("2026-10-28T10:00:00Z"), ms("2026-10-28T10:00:10Z"));
        let (_, end_ms, _) = window_for(&turn, std::slice::from_ref(&s)).unwrap();
        assert_eq!(end_ms, s.ended_at_ms + ANSWER_GRACE_MS);
    }

    #[test]
    fn a_summary_that_does_not_overlap_is_not_used() {
        let items = vec![
            user("u1", Some("2026-10-28T10:00:00Z")),
            reply("a1", Some("2026-10-28T10:00:03Z")),
            end("e1", Some("2026-10-28T10:00:04Z")),
        ];
        let turn = find_turn(&items, None).unwrap();
        let other = summary(ms("2026-10-27T09:00:00Z"), ms("2026-10-27T09:01:00Z"));
        let (start, end, chosen) = window_for(&turn, &[other]).unwrap();
        assert_eq!(start, ms("2026-10-28T10:00:00Z"), "falls back to the first item time");
        assert_eq!(end, ms("2026-10-28T10:00:04Z"), "to the last item time");
        assert!(chosen.is_none());
    }

    #[test]
    fn no_times_and_newest_turn_uses_the_newest_summary() {
        let items = vec![user("u1", None), reply("a1", None), end("e1", None)];
        let turn = find_turn(&items, None).unwrap();
        let older = summary(1_000, 2_000);
        let newer = summary(3_000, 4_000);
        let (start, end, chosen) = window_for(&turn, &[newer.clone(), older]).unwrap();
        assert_eq!((start, end), (3_000, 4_000));
        assert_eq!(chosen, Some(newer));
    }

    #[test]
    fn no_times_and_older_turn_is_refused_with_the_exact_sentence() {
        let items = vec![user("u1", None), reply("a1", None), user("u2", None), reply("a2", None)];
        let turn = find_turn(&items, Some("u1")).unwrap();
        let err = window_for(&turn, &[summary(1_000, 2_000)]).err().unwrap();
        assert_eq!(err, "This turn has no times, so it cannot be clipped. Only the newest turn can.");
    }

    #[test]
    fn no_times_and_no_summary_says_there_is_nothing_to_show() {
        let items = vec![user("u1", None), end("e1", None)];
        let turn = find_turn(&items, None).unwrap();
        assert_eq!(window_for(&turn, &[]).err().unwrap(), "There is nothing to show for this turn.");
    }

    #[test]
    fn times_without_any_summary_use_the_item_times() {
        let items = vec![
            user("u1", Some("2026-10-28T10:00:00Z")),
            end("e1", Some("2026-10-28T10:00:07Z")),
        ];
        let turn = find_turn(&items, None).unwrap();
        let (start, end, chosen) = window_for(&turn, &[]).unwrap();
        assert_eq!((start, end), (ms("2026-10-28T10:00:00Z"), ms("2026-10-28T10:00:07Z")));
        assert!(chosen.is_none());
    }

    #[test]
    fn item_times_parse_with_z_offsets_and_fractions() {
        let utc = ChatItem::User { id: "u".into(), at: Some("2026-10-28T10:00:00Z".into()), text: String::new() };
        let offset = ChatItem::User { id: "u".into(), at: Some("2026-10-28T12:00:00+02:00".into()), text: String::new() };
        let fraction = ChatItem::User { id: "u".into(), at: Some("2026-10-28T10:00:00.250Z".into()), text: String::new() };
        let bad = ChatItem::User { id: "u".into(), at: Some("yesterday".into()), text: String::new() };
        assert_eq!(item_ms(&utc), item_ms(&offset));
        assert_eq!(item_ms(&fraction), item_ms(&utc).map(|t| t + 250));
        assert_eq!(item_ms(&bad), None);
        assert_eq!(item_ms(&ChatItem::User { id: "u".into(), at: None, text: String::new() }), None);
    }

    #[test]
    fn an_item_time_inside_a_summary_window_matches_it_with_an_offset_time() {
        let items = vec![
            user("u1", Some("2026-10-28T12:00:01+02:00")), // 10:00:01 UTC
            end("e1", Some("2026-10-28T10:00:02Z")),
        ];
        let turn = find_turn(&items, None).unwrap();
        let s = summary(ms("2026-10-28T10:00:00Z"), ms("2026-10-28T10:00:10Z"));
        let (start, end, chosen) = window_for(&turn, std::slice::from_ref(&s)).unwrap();
        assert_eq!((start, end), (s.started_at_ms, s.ended_at_ms));
        assert_eq!(chosen, Some(s));
    }
}
