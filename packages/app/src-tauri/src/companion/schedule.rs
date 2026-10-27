//! Check-in times (spec section 5): which check-ins are due, when the next one is, the "Puppet Master
//! was closed" rule and the daily cap. Pure: the clock and the local zone come in as values, so the
//! tests pin both. The zone is a fixed offset in seconds (`chrono::Local` gives it to the live code
//! on every tick, so a change of daylight time shows up one tick later).

use std::collections::BTreeMap;

use chrono::{Datelike, Duration, FixedOffset, NaiveDate, TimeZone, Utc};

use super::model::{CheckIn, HhMm, NextCheckIn, When, MAX_SCHEDULED_SENDS_PER_DAY, MIN_EVERY_MINUTES};
use super::store::SentToday;

/// A check-in whose time passed less than this long ago still runs (Puppet Master may have been busy
/// or just opened). A timed check-in that passed longer ago becomes a "missed" line in the feed.
pub const LATE_RUN_MS: i64 = 2 * 60 * 60 * 1000;

/// What to do with a due check-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    /// Send it now.
    Run,
    /// Too late: write a "missed" line and do not send.
    Missed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Due {
    pub id: String,
    pub label: String,
    pub text: String,
    /// The time it was meant to run (epoch ms).
    pub slot_ms: i64,
    pub call: Call,
}

fn zone(offset_secs: i32) -> FixedOffset {
    FixedOffset::east_opt(offset_secs).unwrap_or_else(|| FixedOffset::east_opt(0).expect("zero offset"))
}

/// The local calendar date of `now_ms`.
pub fn local_date(now_ms: i64, offset_secs: i32) -> NaiveDate {
    let utc = Utc.timestamp_millis_opt(now_ms).single().unwrap_or_default();
    utc.with_timezone(&zone(offset_secs)).date_naive()
}

/// `YYYY-MM-DD` of the local day: the key of the daily send count.
pub fn today_key(now_ms: i64, offset_secs: i32) -> String {
    local_date(now_ms, offset_secs).format("%Y-%m-%d").to_string()
}

/// The instant (epoch ms) of `at` on a local date.
pub fn instant_on(date: NaiveDate, at: HhMm, offset_secs: i32) -> Option<i64> {
    let naive = date.and_hms_opt(u32::from(at.h), u32::from(at.m), 0)?;
    zone(offset_secs).from_local_datetime(&naive).single().map(|dt| dt.timestamp_millis())
}

/// Local clock text of an instant, `8:00` or `14:05`.
pub fn clock_text(ms: i64, offset_secs: i32) -> String {
    let utc = Utc.timestamp_millis_opt(ms).single().unwrap_or_default();
    utc.with_timezone(&zone(offset_secs)).format("%-H:%M").to_string()
}

/// The time of day a clock-based `When` runs on `date`, if it runs that day.
fn clock_on(when: &When, date: NaiveDate) -> Option<HhMm> {
    let monday_first = date.weekday().num_days_from_monday();
    match when {
        When::Daily { at } => Some(*at),
        When::Weekdays { at } if monday_first < 5 => Some(*at),
        When::Weekly { day, at } if monday_first == u32::from(*day) => Some(*at),
        _ => None,
    }
}

/// The latest scheduled instant at or before `now_ms` for a clock-based `When`.
fn last_clock_slot(when: &When, now_ms: i64, offset_secs: i32) -> Option<i64> {
    let today = local_date(now_ms, offset_secs);
    (0..=7).find_map(|back| {
        let date = today - Duration::days(back);
        let slot = instant_on(date, clock_on(when, date)?, offset_secs)?;
        (slot <= now_ms).then_some(slot)
    })
}

/// The first scheduled instant after `now_ms` for a clock-based `When`.
fn next_clock_slot(when: &When, now_ms: i64, offset_secs: i32) -> Option<i64> {
    let today = local_date(now_ms, offset_secs);
    (0..=8).find_map(|ahead| {
        let date = today + Duration::days(ahead);
        let slot = instant_on(date, clock_on(when, date)?, offset_secs)?;
        (slot > now_ms).then_some(slot)
    })
}

fn every_ms(minutes: u32) -> i64 {
    i64::from(minutes.max(MIN_EVERY_MINUTES)) * 60_000
}

/// Gives every check-in a starting point (`last_runs`): a new one counts from now, so it never fires
/// for a time before the person made it. Entries of deleted check-ins are dropped. True when changed.
pub fn ensure_anchors(last_runs: &mut BTreeMap<String, i64>, checkins: &[CheckIn], now_ms: i64) -> bool {
    let before = last_runs.clone();
    last_runs.retain(|id, _| checkins.iter().any(|c| &c.id == id));
    for checkin in checkins {
        last_runs.entry(checkin.id.clone()).or_insert(now_ms);
    }
    *last_runs != before
}

/// The enabled check-ins that are due at `now_ms`, in list order. A check-in appears at most once: a
/// clock-based one only for its latest slot (three missed mornings are one line, not three), so after
/// a long closed time it is either run once (slot less than 2 h ago) or reported missed.
pub fn due(now_ms: i64, offset_secs: i32, checkins: &[CheckIn], last_runs: &BTreeMap<String, i64>) -> Vec<Due> {
    let mut out = Vec::new();
    for checkin in checkins.iter().filter(|c| c.enabled) {
        let Some(&base) = last_runs.get(&checkin.id) else { continue };
        let slot = match &checkin.when {
            // Counted from the last run, so being late is normal: it always runs.
            When::Every { minutes } => {
                let at = base + every_ms(*minutes);
                if at > now_ms {
                    continue;
                }
                out.push(Due { id: checkin.id.clone(), label: checkin.label.clone(), text: checkin.text.clone(), slot_ms: at, call: Call::Run });
                continue;
            }
            When::Once { at_ms } => (*at_ms > base && *at_ms <= now_ms).then_some(*at_ms),
            clock => last_clock_slot(clock, now_ms, offset_secs).filter(|slot| *slot > base),
        };
        let Some(slot_ms) = slot else { continue };
        let call = if now_ms - slot_ms <= LATE_RUN_MS { Call::Run } else { Call::Missed };
        out.push(Due { id: checkin.id.clone(), label: checkin.label.clone(), text: checkin.text.clone(), slot_ms, call });
    }
    out
}

/// The soonest time any enabled check-in will run, after `now_ms`.
pub fn next(now_ms: i64, offset_secs: i32, checkins: &[CheckIn], last_runs: &BTreeMap<String, i64>) -> Option<NextCheckIn> {
    checkins
        .iter()
        .filter(|c| c.enabled)
        .filter_map(|checkin| {
            let at_ms = match &checkin.when {
                When::Every { minutes } => (*last_runs.get(&checkin.id)? + every_ms(*minutes)).max(now_ms),
                When::Once { at_ms } => (*at_ms > now_ms).then_some(*at_ms)?,
                clock => next_clock_slot(clock, now_ms, offset_secs)?,
            };
            Some(NextCheckIn { id: checkin.id.clone(), label: checkin.label.clone(), at_ms })
        })
        .min_by_key(|next| next.at_ms)
}

/// Another scheduled message may go out today.
pub fn under_cap(sent: &SentToday, today: &str) -> bool {
    sent.date != today || sent.count < MAX_SCHEDULED_SENDS_PER_DAY
}

/// Counts one scheduled message for `today` (a new day starts the count over).
pub fn note_send(sent: &mut SentToday, today: &str) {
    if sent.date != today {
        *sent = SentToday { date: today.to_string(), count: 0 };
    }
    sent.count += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 3_600_000;
    const MIN: i64 = 60_000;
    const OFFSET: i32 = 8 * 3600;

    /// 2026-10-28 is a Wednesday. `local(h, m)` is that day at h:m in the +08:00 test zone.
    fn local(day: u32, h: u32, m: u32) -> i64 {
        let date = NaiveDate::from_ymd_opt(2026, 10, day).unwrap();
        instant_on(date, HhMm { h: h as u8, m: m as u8 }, OFFSET).unwrap()
    }

    fn checkin(id: &str, when: When) -> CheckIn {
        CheckIn { id: id.into(), label: format!("Check {id}"), text: "Hi".into(), when, enabled: true }
    }

    fn at(h: u8, m: u8) -> HhMm {
        HhMm { h, m }
    }

    fn runs(pairs: &[(&str, i64)]) -> BTreeMap<String, i64> {
        pairs.iter().map(|(id, ms)| (id.to_string(), *ms)).collect()
    }

    #[test]
    fn local_date_and_clock_follow_the_zone() {
        // 20:00 UTC on the 27th is 04:00 on the 28th at +08:00.
        let ms = Utc.with_ymd_and_hms(2026, 10, 27, 20, 0, 0).unwrap().timestamp_millis();
        assert_eq!(today_key(ms, OFFSET), "2026-10-28");
        assert_eq!(today_key(ms, 0), "2026-10-27");
        assert_eq!(clock_text(ms, OFFSET), "4:00");
        assert_eq!(clock_text(local(28, 14, 5), OFFSET), "14:05");
    }

    #[test]
    fn a_daily_check_in_is_due_once_after_its_time() {
        let c = [checkin("0a0a0a0a", When::Daily { at: at(8, 0) })];
        let base = local(27, 12, 0);
        let last = runs(&[("0a0a0a0a", base)]);
        assert!(due(local(28, 7, 59), OFFSET, &c, &last).is_empty(), "not yet");
        let hit = due(local(28, 8, 0), OFFSET, &c, &last);
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].slot_ms, local(28, 8, 0));
        assert_eq!(hit[0].call, Call::Run);
        // After it ran (last run = its slot), the same slot is not due again.
        let after = runs(&[("0a0a0a0a", local(28, 8, 0))]);
        assert!(due(local(28, 8, 30), OFFSET, &c, &after).is_empty());
        assert_eq!(due(local(29, 8, 1), OFFSET, &c, &after).len(), 1, "and the next day it is");
    }

    #[test]
    fn weekdays_skip_saturday_and_sunday_and_weekly_picks_its_day() {
        let wd = [checkin("0a0a0a0a", When::Weekdays { at: at(9, 0) })];
        let base = runs(&[("0a0a0a0a", local(30, 12, 0))]); // Friday the 30th, after its 9:00 slot
        // Saturday the 31st and Sunday Nov 1st have no slot; Monday Nov 2nd has.
        assert!(due(local(31, 9, 30), OFFSET, &wd, &base).is_empty());
        let monday = Utc.with_ymd_and_hms(2026, 11, 2, 1, 30, 0).unwrap().timestamp_millis();
        assert_eq!(due(monday, OFFSET, &wd, &base).len(), 1);

        // Weekly on day 2 (Wednesday): the 28th is a Wednesday.
        let weekly = [checkin("0b0b0b0b", When::Weekly { day: 2, at: at(18, 0) })];
        let base = runs(&[("0b0b0b0b", local(27, 0, 0))]);
        assert!(due(local(28, 17, 59), OFFSET, &weekly, &base).is_empty());
        assert_eq!(due(local(28, 18, 0), OFFSET, &weekly, &base).len(), 1);
        // Thursday: the slot is still the Wednesday one (within 2 h it would run; later it is missed).
        let thursday = due(local(29, 9, 0), OFFSET, &weekly, &base);
        assert_eq!(thursday.len(), 1);
        assert_eq!(thursday[0].call, Call::Missed);
    }

    #[test]
    fn every_has_a_minimum_of_thirty_minutes_and_counts_from_the_last_run() {
        // 5 minutes in the data is read as 30 (the model refuses it, this is the second line of defence).
        let c = [checkin("0a0a0a0a", When::Every { minutes: 5 })];
        let last = runs(&[("0a0a0a0a", local(28, 10, 0))]);
        assert!(due(local(28, 10, 29), OFFSET, &c, &last).is_empty());
        let hit = due(local(28, 10, 30), OFFSET, &c, &last);
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].call, Call::Run);

        let hourly = [checkin("0a0a0a0a", When::Every { minutes: 60 })];
        assert!(due(local(28, 10, 59), OFFSET, &hourly, &last).is_empty());
        assert_eq!(due(local(28, 11, 0), OFFSET, &hourly, &last).len(), 1);
        // A long closed time still runs it once, never as "missed".
        let late = due(local(28, 20, 0), OFFSET, &hourly, &last);
        assert_eq!((late.len(), late[0].call), (1, Call::Run));
    }

    #[test]
    fn once_runs_when_its_time_comes_and_never_again() {
        let when = When::Once { at_ms: local(28, 15, 0) };
        let c = [checkin("0a0a0a0a", when)];
        let created = runs(&[("0a0a0a0a", local(28, 9, 0))]);
        assert!(due(local(28, 14, 59), OFFSET, &c, &created).is_empty());
        assert_eq!(due(local(28, 15, 0), OFFSET, &c, &created).len(), 1);
        let sent = runs(&[("0a0a0a0a", local(28, 15, 0))]);
        assert!(due(local(28, 16, 0), OFFSET, &c, &sent).is_empty());
        // Made after its time: it never runs.
        let made_late = runs(&[("0a0a0a0a", local(28, 16, 0))]);
        assert!(due(local(28, 17, 0), OFFSET, &c, &made_late).is_empty());
    }

    #[test]
    fn closed_for_less_than_two_hours_runs_once_and_longer_is_missed() {
        let c = [checkin("0a0a0a0a", When::Daily { at: at(8, 0) })];
        let last = runs(&[("0a0a0a0a", local(27, 8, 0))]);
        let soon = due(local(28, 9, 59), OFFSET, &c, &last);
        assert_eq!((soon.len(), soon[0].call), (1, Call::Run), "1 h 59 min late still runs");
        let exactly = due(local(28, 10, 0), OFFSET, &c, &last);
        assert_eq!(exactly[0].call, Call::Run, "exactly 2 h still runs");
        let old = due(local(28, 10, 1), OFFSET, &c, &last);
        assert_eq!((old.len(), old[0].call), (1, Call::Missed));
        assert_eq!(old[0].slot_ms, local(28, 8, 0));
        // Three days closed: still one line for the latest slot, not three.
        let days = due(local(31, 12, 0), OFFSET, &c, &last);
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].slot_ms, local(31, 8, 0));
    }

    #[test]
    fn disabled_and_unanchored_check_ins_are_never_due() {
        let mut off = checkin("0a0a0a0a", When::Daily { at: at(8, 0) });
        off.enabled = false;
        let last = runs(&[("0a0a0a0a", local(27, 0, 0)), ("0c0c0c0c", local(27, 0, 0))]);
        assert!(due(local(28, 9, 0), OFFSET, &[off], &last).is_empty());
        let fresh = [checkin("0c0c0c0c", When::Daily { at: at(8, 0) })];
        assert!(due(local(28, 9, 0), OFFSET, &fresh, &BTreeMap::new()).is_empty(), "no anchor yet");
    }

    #[test]
    fn anchors_start_new_check_ins_from_now_and_drop_deleted_ones() {
        let c = [checkin("0a0a0a0a", When::Daily { at: at(8, 0) }), checkin("0b0b0b0b", When::Every { minutes: 60 })];
        let mut last = runs(&[("0a0a0a0a", 5), ("gone0000", 9)]);
        assert!(ensure_anchors(&mut last, &c, 1_000));
        assert_eq!(last, runs(&[("0a0a0a0a", 5), ("0b0b0b0b", 1_000)]));
        assert!(!ensure_anchors(&mut last, &c, 2_000), "nothing changes the second time");
        // A check-in made at 10:00 whose 08:00 slot passed is not due: it counts from its anchor.
        let mut fresh = BTreeMap::new();
        let made = [checkin("0a0a0a0a", When::Daily { at: at(8, 0) })];
        ensure_anchors(&mut fresh, &made, local(28, 10, 0));
        assert!(due(local(28, 10, 1), OFFSET, &made, &fresh).is_empty());
    }

    #[test]
    fn next_is_the_soonest_enabled_time_after_now() {
        let c = [
            checkin("0a0a0a0a", When::Daily { at: at(8, 0) }),
            checkin("0b0b0b0b", When::Weekdays { at: at(7, 0) }),
            checkin("0c0c0c0c", When::Once { at_ms: local(28, 9, 30) }),
        ];
        let last = runs(&[("0a0a0a0a", local(27, 8, 0)), ("0b0b0b0b", local(27, 7, 0)), ("0c0c0c0c", local(27, 0, 0))]);
        let n = next(local(28, 7, 30), OFFSET, &c, &last).unwrap();
        assert_eq!((n.id.as_str(), n.at_ms), ("0a0a0a0a", local(28, 8, 0)));
        let n = next(local(28, 8, 30), OFFSET, &c, &last).unwrap();
        assert_eq!((n.id.as_str(), n.at_ms), ("0c0c0c0c", local(28, 9, 30)), "the once check-in is next");
        let n = next(local(28, 10, 0), OFFSET, &c, &last).unwrap();
        assert_eq!((n.id.as_str(), n.at_ms), ("0b0b0b0b", local(29, 7, 0)));
        assert_eq!(n.label, "Check 0b0b0b0b");
        assert!(next(local(28, 10, 0), OFFSET, &[], &last).is_none());
    }

    #[test]
    fn next_for_every_counts_from_the_last_run_and_skips_disabled() {
        let mut c = [checkin("0a0a0a0a", When::Every { minutes: 90 })];
        let last = runs(&[("0a0a0a0a", local(28, 10, 0))]);
        assert_eq!(next(local(28, 10, 10), OFFSET, &c, &last).unwrap().at_ms, local(28, 10, 0) + 90 * MIN);
        // Already due: the next time is now, never in the past.
        assert_eq!(next(local(28, 13, 0), OFFSET, &c, &last).unwrap().at_ms, local(28, 13, 0));
        c[0].enabled = false;
        assert!(next(local(28, 10, 10), OFFSET, &c, &last).is_none());
        assert_eq!(HOUR, 60 * MIN);
    }

    #[test]
    fn the_daily_cap_counts_per_local_day() {
        let today = "2026-10-28";
        let mut sent = SentToday::default();
        for n in 0..MAX_SCHEDULED_SENDS_PER_DAY {
            assert!(under_cap(&sent, today), "send {n} is allowed");
            note_send(&mut sent, today);
        }
        assert_eq!(sent.count, 24);
        assert!(!under_cap(&sent, today), "the 25th is not");
        assert!(under_cap(&sent, "2026-10-29"), "a new day starts over");
        note_send(&mut sent, "2026-10-29");
        assert_eq!((sent.date.as_str(), sent.count), ("2026-10-29", 1));
    }
}
