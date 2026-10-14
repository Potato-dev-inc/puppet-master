//! The per-pane state machine behind detection: a blocking limit has to hold still before it is
//! acted on (guard 4 of the design), and it stops counting the moment the turn resumes.
//!
//! Pure: no I/O, no clock of its own (`now_ms` is passed in), so the replay corpus drives it on
//! virtual time. `mod.rs` wires it to the live panes.
//!
//! Cost model (the same as `canary::unknown`): `observe` runs for a pane whose screen changed, at
//! most once a second. `tick` runs once a second over the few panes that have a candidate. A silent
//! pane costs nothing.

use super::detect::{LimitEvidence, LimitHit};
use std::collections::HashMap;

/// What the tracker decided, for the caller to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A blocking limit held still for its `stable_ms`: raise the card, park the run.
    Fired { pane_id: String, hit: LimitHit },
    /// A limit that was fired is gone: the agent carried on, or the screen moved on.
    Cleared { pane_id: String, hit: LimitHit, why: Cleared },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cleared {
    /// The turn is running again (the agent continued, by its own wait or by a person).
    Resumed,
    /// The text is no longer on screen.
    Gone,
    /// A different limit replaced it (another reset time, another window).
    Replaced,
}

/// Where a pane stands, for the run supervisor: a run on a pane that is `Pending` or `Parked` is
/// never completed from that screen, because the limit text would be read as the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    Clear,
    /// A limit is on screen and has not yet held for `stable_ms`.
    Pending,
    /// The limit was acted on and is still on screen.
    Parked,
}

struct Candidate {
    hit: LimitHit,
    since_ms: u64,
}

#[derive(Default)]
struct PaneWatch {
    candidate: Option<Candidate>,
    fired: Option<LimitHit>,
    /// The `screen_sha` of a limit the turn has since moved past. The same text still on screen
    /// (scrollback) is not a new limit until it has been gone once.
    suppressed: Option<String>,
    /// A transient retry is on screen (a status line, never a card).
    transient: Option<LimitHit>,
}

#[derive(Default)]
pub struct Tracker {
    panes: HashMap<String, PaneWatch>,
}

impl Tracker {
    /// One look at a pane. `hit` is what the profile's detector made of the screen (`None` for no
    /// limit text, or a screen guarded out); `busy` is the profile's busy marker.
    pub fn observe(&mut self, now_ms: u64, pane_id: &str, hit: Option<LimitHit>, busy: Option<bool>) -> (Standing, Vec<Event>) {
        let mut events = Vec::new();
        let watch = self.panes.entry(pane_id.to_string()).or_default();
        watch.transient = hit.as_ref().filter(|h| h.kind == super::detect::LimitKind::Transient).cloned();
        // Only a usage limit parks a run and raises a card. A vendor menu or an unavailable plan only
        // keeps tasks from being typed (`handoff::limit_gate`); a retry is a status line.
        let blocking = hit.filter(|h| h.kind == super::detect::LimitKind::Usage);
        match blocking {
            // The turn already moved past this text.
            Some(hit) if watch.suppressed.as_deref() == Some(hit.screen_sha.as_str()) => {
                watch.candidate = None;
            }
            Some(hit) => {
                if let Some(fired) = &watch.fired {
                    if fired.screen_sha != hit.screen_sha {
                        let old = watch.fired.take().expect("checked");
                        events.push(Event::Cleared { pane_id: pane_id.to_string(), hit: old, why: Cleared::Replaced });
                    }
                }
                if watch.fired.is_none() {
                    let same = watch.candidate.as_ref().is_some_and(|c| c.hit.screen_sha == hit.screen_sha);
                    if same {
                        // Keep the time it first showed; take the newest read of it.
                        if let Some(c) = watch.candidate.as_mut() {
                            c.hit = hit;
                        }
                    } else {
                        watch.candidate = Some(Candidate { hit, since_ms: now_ms });
                    }
                    Self::promote(watch, pane_id, now_ms, &mut events);
                }
            }
            None => {
                watch.candidate = None;
                if let Some(old) = watch.fired.take() {
                    let resumed = busy == Some(true);
                    if resumed {
                        watch.suppressed = Some(old.screen_sha.clone());
                    }
                    let why = if resumed { Cleared::Resumed } else { Cleared::Gone };
                    events.push(Event::Cleared { pane_id: pane_id.to_string(), hit: old, why });
                } else if busy != Some(true) {
                    // The text was gone while nothing was running: it may come back as a new limit.
                    watch.suppressed = None;
                }
            }
        }
        (Self::standing(watch), events)
    }

    /// Promote candidates that have held still long enough. Called about once a second.
    pub fn tick(&mut self, now_ms: u64) -> Vec<Event> {
        let mut events = Vec::new();
        for (pane_id, watch) in self.panes.iter_mut() {
            Self::promote(watch, pane_id, now_ms, &mut events);
        }
        events
    }

    fn promote(watch: &mut PaneWatch, pane_id: &str, now_ms: u64, events: &mut Vec<Event>) {
        let due = watch.candidate.as_ref().is_some_and(|c| now_ms.saturating_sub(c.since_ms) >= c.hit.stable_ms);
        if due {
            if let Some(Candidate { mut hit, .. }) = watch.candidate.take() {
                hit.evidence = LimitEvidence::Stable;
                watch.fired = Some(hit.clone());
                events.push(Event::Fired { pane_id: pane_id.to_string(), hit });
            }
        }
    }

    fn standing(watch: &PaneWatch) -> Standing {
        if watch.fired.is_some() {
            Standing::Parked
        } else if watch.candidate.is_some() {
            Standing::Pending
        } else {
            Standing::Clear
        }
    }

    /// The limit currently acted on for a pane.
    pub fn parked(&self, pane_id: &str) -> Option<&LimitHit> {
        self.panes.get(pane_id).and_then(|w| w.fired.as_ref())
    }

    pub fn standing_of(&self, pane_id: &str) -> Standing {
        self.panes.get(pane_id).map(Self::standing).unwrap_or(Standing::Clear)
    }

    /// The transient retry on screen, if any.
    pub fn transient(&self, pane_id: &str) -> Option<&LimitHit> {
        self.panes.get(pane_id).and_then(|w| w.transient.as_ref())
    }

    /// Forget panes that no longer exist.
    pub fn retain(&mut self, alive: &dyn Fn(&str) -> bool) {
        self.panes.retain(|id, _| alive(id));
    }
}
