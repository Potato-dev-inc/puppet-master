//! Moving a companion to another agent (the person's other subscription) and back.
//!
//! The companion is its files: SOUL.md, USER.md, MEMORY.md, `today.md` and `memory/pending.md` in its home
//! folder. A move touches none of them. It stops the pane through the keeper (the keeper's own kill, so the
//! pane is not read as "the person closed it"), switches `agent_type`, rewrites the brief for the new agent
//! (`start` does that before every spawn) and starts again in the same home. Same name, same face.
//!
//! - A move never interrupts. While the companion is working, waiting for a card or still starting up,
//!   the move waits in `runtime.json` (`pending_move`) and the view says so; it runs when the companion is free.
//! - A timed move (`until_ms`) goes back to `previous_agent` by itself. If that agent is still at its usage
//!   limit then, the companion stays and the feed says so; the time is pushed to the reset.
//! - The args and the account belong to the agent they were set for: a move leaves them behind with
//!   `previous_agent` and a move back restores them.
//!
//! Nothing here names an agent: the registry decides (`Host::agent_ok`), the display name is `Host::agent_label`.

use super::model::{validate_profile, CompanionError, CompanionProfile, CompanionResult, CompanionState, FeedKind, PreviousAgent};
use super::schedule;
use super::service::{pane_id_of, Inner, Service};
use super::store::{self, PendingMove};

/// When the old agent is still at its limit and does not say when it ends, look again after this long.
pub const STILL_LIMITED_RETRY_MS: i64 = 30 * 60 * 1000;

/// A checked ask to move: what `companion_update` made of `agentType` and `untilMs`.
#[derive(Clone, Debug)]
pub(super) struct MoveAsk {
    pub target: String,
    /// `None` = the move stays.
    pub until_ms: Option<i64>,
    /// The ask named `untilMs` (so "no end" was meant, not "unchanged").
    pub until_given: bool,
}

impl Service {
    /// Checks a patch's `agentType` / `untilMs` before anything is saved. `None`: the patch asks for no move.
    pub(super) fn check_move(
        &self,
        profile: &CompanionProfile,
        agent_type: Option<&str>,
        until: Option<Option<i64>>,
        now: i64,
    ) -> CompanionResult<Option<MoveAsk>> {
        if agent_type.is_none() && until.is_none() {
            return Ok(None);
        }
        let target = agent_type.map(str::trim).unwrap_or(profile.agent_type.as_str()).to_string();
        if target.is_empty() {
            return Err(CompanionError::new("COMPANION_AGENT_UNAVAILABLE", "Chat can't send messages to this agent yet."));
        }
        let until_ms = until.flatten();
        if let Some(at) = until_ms {
            if at <= now {
                return Err(CompanionError::new("COMPANION_MOVE_TIME_PAST", "That time has already passed. Pick a later time."));
            }
        }
        if target != profile.agent_type {
            // Refused now, even when the move will wait: a typo must not sit in the queue.
            let home = profile.previous_agent.as_ref().filter(|p| p.agent_type == target);
            super::agents::check_launch_args(home.map_or(&[][..], |p| p.launch_args.as_slice()))?;
            self.host
                .agent_ok(&target, &profile.folder)
                .map_err(|reason| CompanionError::new("COMPANION_AGENT_UNAVAILABLE", reason))?;
        }
        Ok(Some(MoveAsk { target, until_ms, until_given: until.is_some() }))
    }

    /// Runs a checked ask: now when the companion is free, else it waits as `pending_move`.
    pub(super) fn request_move(&self, inner: &mut Inner, index: usize, now: i64, ask: MoveAsk) -> CompanionResult<()> {
        let profile = inner.profiles[index].clone();
        if ask.target == profile.agent_type {
            // The same agent: it cancels a move that was waiting, and may change the end of a timed move.
            if let Some(slot) = inner.slots.get_mut(&profile.id) {
                if slot.runtime.pending_move.take().is_some() {
                    self.save_runtime(&profile.id, &slot.runtime);
                }
            }
            if ask.until_given && profile.previous_agent.is_some() && profile.until_ms != ask.until_ms {
                inner.profiles[index].until_ms = ask.until_ms;
                if let Err(e) = store::save(&self.root, &inner.profiles) {
                    inner.profiles[index] = profile;
                    return Err(e);
                }
            }
            return Ok(());
        }
        if self.is_free(inner, &profile) {
            return self.apply_move(inner, index, now, &ask.target, ask.until_ms);
        }
        if let Some(slot) = inner.slots.get_mut(&profile.id) {
            slot.runtime.pending_move = Some(PendingMove { agent_type: ask.target, until_ms: ask.until_ms });
            self.save_runtime(&profile.id, &slot.runtime);
        }
        Ok(())
    }

    /// Nothing is running that a move would cut off: asleep, stopped, no pane yet, or idle with nothing waiting.
    fn is_free(&self, inner: &Inner, profile: &CompanionProfile) -> bool {
        let Some(slot) = inner.slots.get(&profile.id) else { return true };
        // Only a dismissible startup notice is on the pane (it reads `running`): stopping it cuts nothing off,
        // unless a message is still on its way into it.
        if slot.notice_open
            && slot.pane_there
            && matches!(slot.state, CompanionState::Working | CompanionState::Starting)
            && slot.outgoing.is_empty()
            && self.host.waiting(&pane_id_of(&profile.id)).is_empty()
        {
            return true;
        }
        match slot.state {
            CompanionState::Sleeping | CompanionState::Stopped => true,
            // Waiting to start (after a crash, or a start that fails) has no pane to cut off.
            CompanionState::Starting => !slot.pane_there,
            CompanionState::Idle => slot.outgoing.is_empty() && self.host.waiting(&pane_id_of(&profile.id)).is_empty(),
            CompanionState::Working | CompanionState::NeedsYou => false,
        }
    }

    /// Does the move: saves the profile, closes the pane the keeper's way and starts it again on the new
    /// agent in the same home. A sleeping companion only changes agent; it starts there when woken.
    pub(super) fn apply_move(&self, inner: &mut Inner, index: usize, now: i64, target: &str, until: Option<i64>) -> CompanionResult<()> {
        let before = inner.profiles[index].clone();
        let mut next = before.clone();
        let back = before.previous_agent.clone().filter(|p| p.agent_type == target);
        let going_back = back.is_some();
        if let Some(home) = back {
            next.agent_type = home.agent_type;
            next.launch_args = home.launch_args;
            next.account = home.account;
            next.previous_agent = None;
            next.until_ms = None;
        } else {
            // In a timed stay the first agent stays the one to go back to; after a lasting move the agent
            // it left is the new "previous".
            let timed_stay = before.previous_agent.is_some() && before.until_ms.is_some();
            if !timed_stay {
                next.previous_agent = Some(PreviousAgent {
                    agent_type: before.agent_type.clone(),
                    launch_args: before.launch_args.clone(),
                    account: before.account.clone(),
                });
            }
            next.agent_type = target.to_string();
            next.launch_args = Vec::new();
            next.account = None;
            next.until_ms = until;
        }
        validate_profile(&next)?;
        self.check_agent(&next)?;

        let id = before.id.clone();
        inner.profiles[index] = next.clone();
        if let Err(e) = store::save(&self.root, &inner.profiles) {
            inner.profiles[index] = before;
            return Err(e);
        }
        let label = self.host.agent_label(&next.agent_type);
        let awake = inner.slots.get(&id).is_some_and(|slot| slot.keeper.want());
        if let Some(slot) = inner.slots.get_mut(&id) {
            slot.agent_label = label.clone();
            slot.runtime.pending_move = None;
            self.save_runtime(&id, &slot.runtime);
        }
        let offset = self.host.offset_secs();
        let text = if going_back {
            format!("{} moved back to {label}", next.name)
        } else {
            match next.until_ms {
                Some(at) => format!("{} moved to {label} until {}", next.name, until_text(at, now, offset)),
                None => format!("{} moved to {label}", next.name),
            }
        };
        self.feed(&next, FeedKind::Moved, now, &text);
        self.host.reply_pushed(&next.id, &next.name, &text, next.push_replies);
        if awake {
            self.restart_core(inner, index, now, None);
        }
        Ok(())
    }

    /// Called every tick before the companion's own step: a timed move that ended starts its way back,
    /// and a move that waited runs once the companion is free.
    pub(super) fn move_step(&self, inner: &mut Inner, index: usize, now: i64) {
        let profile = inner.profiles[index].clone();
        if let (Some(until), Some(home)) = (profile.until_ms, profile.previous_agent.clone()) {
            let on_its_way = inner
                .slots
                .get(&profile.id)
                .and_then(|slot| slot.runtime.pending_move.as_ref())
                .is_some_and(|pending| pending.agent_type == home.agent_type);
            if now >= until && !on_its_way {
                self.time_is_up(inner, index, now, &home);
            }
        }
        let profile = inner.profiles[index].clone();
        let Some(pending) = inner.slots.get(&profile.id).and_then(|slot| slot.runtime.pending_move.clone()) else { return };
        if !self.is_free(inner, &profile) {
            return;
        }
        if let Err(e) = self.apply_move(inner, index, now, &pending.agent_type, pending.until_ms) {
            // It cannot run any more (the agent was removed, the gate says no). Drop it and say so.
            if let Some(slot) = inner.slots.get_mut(&profile.id) {
                slot.runtime.pending_move = None;
                self.save_runtime(&profile.id, &slot.runtime);
            }
            let label = self.host.agent_label(&pending.agent_type);
            self.feed(&profile, FeedKind::Moved, now, &format!("{} could not move to {label}: {}", profile.name, e.message));
            if profile.previous_agent.as_ref().is_some_and(|p| p.agent_type == pending.agent_type) {
                // It stays where it is; the timed move is over.
                inner.profiles[index].until_ms = None;
                if store::save(&self.root, &inner.profiles).is_err() {
                    inner.profiles[index] = profile;
                }
            }
        }
    }

    /// The end of a timed move: back to the old agent, unless that agent is still at its limit.
    fn time_is_up(&self, inner: &mut Inner, index: usize, now: i64, home: &PreviousAgent) {
        let profile = inner.profiles[index].clone();
        if let Some(limit) = self.host.agent_limit(&home.agent_type, home.account.as_deref()) {
            let again = limit.reset_at_ms.filter(|at| *at > now).map_or(now + STILL_LIMITED_RETRY_MS, |at| at + 60_000);
            inner.profiles[index].until_ms = Some(again);
            if let Err(e) = store::save(&self.root, &inner.profiles) {
                inner.profiles[index] = profile;
                tracing::warn!(error = %e, "could not save companions.json");
                return;
            }
            let home_label = self.host.agent_label(&home.agent_type);
            let here = self.host.agent_label(&profile.agent_type);
            let text = format!("{home_label} is still out of usage. {} stays on {here}.", profile.name);
            self.feed(&profile, FeedKind::Moved, now, &text);
            return;
        }
        if let Some(slot) = inner.slots.get_mut(&profile.id) {
            slot.runtime.pending_move = Some(PendingMove { agent_type: home.agent_type.clone(), until_ms: None });
            self.save_runtime(&profile.id, &slot.runtime);
        }
    }
}

/// `17:00` for a moment later today, `Thu 17:00` for another day.
fn until_text(at_ms: i64, now: i64, offset: i32) -> String {
    if schedule::local_date(at_ms, offset) == schedule::local_date(now, offset) {
        schedule::clock_text(at_ms, offset)
    } else {
        schedule::day_clock_text(at_ms, offset)
    }
}
