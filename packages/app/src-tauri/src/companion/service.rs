//! The companion service: the keepers of every companion, run over a `Host` (spec sections 4 to 6).
//!
//! One `tick` (every second, from `live.rs`) does, for each companion that is not archived:
//! 1. check-ins that are due (`schedule::due`) become messages to send, the daily cap and the
//!    "one waiting at a time" rule applied;
//! 2. the keeper looks at the pane and says what to do (`keeper::Keeper::step`): start it (brief
//!    first), close a dead one, give up, or switch to sleeping because the person closed the pane;
//! 3. the state and the line for the face are derived (`keeper::derive`);
//! 4. waiting messages (primer, check-ins, tidy-up) go to the pane's outbox. The service never types
//!    into a pane and never answers a dialog: a gate is the person's, through the attention card;
//! 5. a finished turn becomes a `reply` line in the feed, the nap runs at night, new memory proposals
//!    are counted (and kept at once when the person turned review off, flagged ones excepted).
//!
//! When any view changed, the host is told once. The other half of the service (create, edit, memory
//! and sleep/wake) is in `manage.rs`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

use super::host::{Host, SpawnSpec};
use super::keeper::{self, Action, DeriveInput, Keeper, NapInput, Pane, Status};
use super::model::{
    CompanionProfile, CompanionState, CompanionView, FeedItem, FeedKind, NeedsRef, NextCheckIn,
};
use super::store::{self, Runtime};
use super::{agents, brief, files, memory, schedule, view};

/// How often a companion's pending list is read.
const PENDING_EVERY_MS: i64 = 3_000;
/// After a turn ends, how long the reply is given to reach the session file before it is read.
const REPLY_AFTER_MS: i64 = 1_500;
/// How long the reader is tried before the one-line turn summary is used instead.
const REPLY_GIVE_UP_MS: i64 = 8_000;
/// A message that could not be handed to the outbox for this long is dropped (the outbox has its own 2 h).
const OUTGOING_GIVE_UP_MS: i64 = 2 * 60 * 60 * 1000;
/// The text every check-in message starts with.
pub const CHECKIN_PREFIX: &str = "[Check-in:";

/// The pane id of a companion.
pub fn pane_id_of(id: &str) -> String {
    format!("companion-{id}")
}

/// Whether a pane id is a companion's (the person opens it from the sidebar; it never gets a window by itself).
pub fn is_pane_id(pane_id: &str) -> bool {
    pane_id.starts_with("companion-")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum OutKind {
    /// First message after a start for an agent that has no instruction file of its own.
    Primer,
    /// A scheduled message; the label.
    Checkin(String),
    Tidy,
}

#[derive(Clone, Debug)]
pub(super) struct Outgoing {
    pub kind: OutKind,
    pub text: String,
    pub queued_ms: i64,
}

/// A turn ended and its reply has not been read yet.
#[derive(Clone, Debug)]
pub(super) struct ReplyWait {
    pub since_ms: i64,
}

/// Everything the service keeps in memory for one companion.
pub(super) struct Slot {
    pub keeper: Keeper,
    pub runtime: Runtime,
    pub outgoing: Vec<Outgoing>,
    pub state: CompanionState,
    pub line: String,
    pub needs: Option<NeedsRef>,
    /// A live pane exists.
    pub pane_there: bool,
    pub pending: u32,
    pub pending_seen: bool,
    pub pending_checked_ms: i64,
    pub prev_state: Option<CompanionState>,
    pub reply_wait: Option<ReplyWait>,
    pub last_reply_id: Option<String>,
    /// The next reply is the answer to the primer: not worth a feed line.
    pub skip_reply: bool,
    /// The coming start has its own feed line (nap, restart): no second "woke" line.
    pub quiet_start: bool,
    pub agent_label: String,
}

pub(super) struct Inner {
    pub profiles: Vec<CompanionProfile>,
    pub slots: HashMap<String, Slot>,
    pub last_views: Vec<CompanionView>,
}

pub struct Service {
    pub(super) host: Arc<dyn Host>,
    pub(super) root: PathBuf,
    pub(super) inner: Mutex<Inner>,
}

impl Slot {
    pub(super) fn new(profile: &CompanionProfile, runtime: Runtime, agent_label: String) -> Self {
        let want = profile.auto_start && !runtime.sleeping && !profile.archived;
        Slot {
            keeper: Keeper::new(want),
            runtime,
            outgoing: Vec::new(),
            state: if want { CompanionState::Starting } else { CompanionState::Sleeping },
            line: if want { keeper::STARTING_LINE } else { keeper::SLEEPING_LINE }.to_string(),
            needs: None,
            pane_there: false,
            pending: 0,
            pending_seen: false,
            pending_checked_ms: i64::MIN / 2,
            prev_state: None,
            reply_wait: None,
            last_reply_id: None,
            skip_reply: false,
            quiet_start: false,
            agent_label,
        }
    }
}

impl Service {
    /// Loads the saved companions from `root` (the app data folder). A damaged `companions.json` starts
    /// with none and is left untouched on disk.
    pub fn new(host: Arc<dyn Host>, root: PathBuf) -> Self {
        let profiles = match store::load(&root) {
            Ok(list) => list,
            Err(e) => {
                tracing::warn!(error = %e, "companions.json could not be read; starting with none");
                Vec::new()
            }
        };
        let mut slots = HashMap::new();
        for profile in &profiles {
            let runtime = store::runtime_load(&root, &profile.id).unwrap_or_default();
            let label = host.agent_label(&profile.agent_type);
            slots.insert(profile.id.clone(), Slot::new(profile, runtime, label));
        }
        Service { host, root, inner: Mutex::new(Inner { profiles, slots, last_views: Vec::new() }) }
    }

    /// One pass over every companion. Cheap when nothing changes; call it about once a second.
    pub fn tick(&self) {
        let mut inner = self.inner.lock();
        let now = self.host.now_ms();
        for i in 0..inner.profiles.len() {
            if !inner.profiles[i].archived {
                self.tick_one(&mut inner, i, now);
            }
        }
        self.publish_if_changed(&mut inner, now);
    }

    pub(super) fn tick_one(&self, inner: &mut Inner, i: usize, now: i64) {
        let profile = inner.profiles[i].clone();
        let pane_id = pane_id_of(&profile.id);
        let offset = self.host.offset_secs();
        let Some(slot) = inner.slots.get_mut(&profile.id) else { return };
        let mut runtime_dirty = false;
        let mut finished_once: Vec<String> = Vec::new();

        // 1. check-ins
        runtime_dirty |= schedule::ensure_anchors(&mut slot.runtime.last_runs, &profile.checkins, now);
        for due in schedule::due(now, offset, &profile.checkins, &slot.runtime.last_runs) {
            self.handle_due(&profile, slot, &pane_id, &due, now, offset);
            runtime_dirty = true;
            if profile.checkins.iter().any(|c| c.id == due.id && matches!(c.when, super::model::When::Once { .. })) {
                finished_once.push(due.id.clone());
            }
        }

        // 2. the keeper
        let seen = if slot.keeper.want() || slot.pane_there { self.host.pane(&pane_id) } else { Pane::Missing };
        let mut todo: std::collections::VecDeque<Action> = slot.keeper.step(now, seen).into();
        while let Some(action) = todo.pop_front() {
            runtime_dirty |= self.run_action(&profile, slot, &pane_id, action, now, &mut todo);
        }

        // 3. the face
        self.derive_state(&profile, slot, &pane_id, now);

        // 4. messages, 5. the rest
        self.flush_outgoing(&profile, slot, &pane_id, now);
        runtime_dirty |= self.maybe_nap(&profile, slot, &pane_id, now, offset);
        self.read_reply(&profile, slot, &pane_id, now);
        self.refresh_pending(&profile, slot, now);
        slot.prev_state = Some(slot.state);

        if runtime_dirty {
            self.save_runtime(&profile.id, &slot.runtime);
        }
        if !finished_once.is_empty() {
            for checkin in inner.profiles[i].checkins.iter_mut().filter(|c| finished_once.contains(&c.id)) {
                checkin.enabled = false;
            }
            if let Err(e) = store::save(&self.root, &inner.profiles) {
                tracing::warn!(error = %e, "could not save companions.json");
            }
        }
    }

    // ---- 1. check-ins -------------------------------------------------------------------------

    fn handle_due(&self, profile: &CompanionProfile, slot: &mut Slot, pane_id: &str, due: &schedule::Due, now: i64, offset: i32) {
        slot.runtime.last_runs.insert(due.id.clone(), now);
        let label = &due.label;
        if due.call == schedule::Call::Missed {
            let at = schedule::clock_text(due.slot_ms, offset);
            self.feed(profile, FeedKind::Missed, now, &format!("Missed \"{label}\" at {at}. Puppet Master was closed."));
            return;
        }
        let today = schedule::today_key(now, offset);
        if !schedule::under_cap(&slot.runtime.sent_today, &today) {
            self.feed(profile, FeedKind::Missed, now, &format!("Skipped \"{label}\": the daily limit was reached."));
            return;
        }
        if slot.keeper.is_stopped() {
            self.feed(profile, FeedKind::Missed, now, &format!("Skipped \"{label}\": {} is stopped.", profile.name));
            return;
        }
        let waiting_in_outbox = self.host.waiting(pane_id).iter().any(|text| text.starts_with(CHECKIN_PREFIX));
        let waiting_here = slot.outgoing.iter().any(|o| matches!(o.kind, OutKind::Checkin(_)));
        if waiting_in_outbox || waiting_here {
            self.feed(profile, FeedKind::Missed, now, &format!("Skipped \"{label}\": the last check-in is still waiting."));
            return;
        }
        schedule::note_send(&mut slot.runtime.sent_today, &today);
        if !slot.keeper.want() {
            slot.runtime.sleeping = false;
            slot.keeper.wake();
            slot.quiet_start = false;
        }
        slot.outgoing.push(Outgoing {
            kind: OutKind::Checkin(label.clone()),
            text: format!("{CHECKIN_PREFIX} {label}] {}", due.text),
            queued_ms: now,
        });
    }

    // ---- 2. the keeper's actions ----------------------------------------------------------------

    /// Runs one action. Returns true when `runtime` changed.
    fn run_action(
        &self,
        profile: &CompanionProfile,
        slot: &mut Slot,
        pane_id: &str,
        action: Action,
        now: i64,
        todo: &mut std::collections::VecDeque<Action>,
    ) -> bool {
        match action {
            Action::Spawn { restart } => {
                match self.start(profile, slot, pane_id, now) {
                    Ok(()) => {
                        slot.keeper.spawned(now);
                        let (kind, text) = if restart {
                            (FeedKind::Restarted, "Restarted after it stopped.")
                        } else if std::mem::take(&mut slot.quiet_start) {
                            return false;
                        } else {
                            (FeedKind::Woke, "Woke up.")
                        };
                        self.feed(profile, kind, now, text);
                    }
                    Err(reason) => {
                        tracing::warn!(companion = %profile.id, %reason, "companion did not start");
                        todo.extend(slot.keeper.spawn_failed(now));
                    }
                }
                false
            }
            Action::KillPane => {
                self.host.kill(pane_id);
                self.host.release_reader(pane_id, &profile.id);
                slot.outgoing.retain(|o| o.kind != OutKind::Primer);
                slot.reply_wait = None;
                false
            }
            Action::PersonClosed => {
                self.host.release_reader(pane_id, &profile.id);
                slot.runtime.sleeping = true;
                slot.outgoing.retain(|o| o.kind != OutKind::Primer);
                self.feed(profile, FeedKind::Slept, now, "You closed its terminal, so it went to sleep.");
                true
            }
            Action::RaiseStopped => {
                self.host.raise_stopped(&profile.id, &profile.name);
                self.feed(profile, FeedKind::NeedsYou, now, "Stopped after 3 restarts. Open the terminal to see why.");
                false
            }
        }
    }

    /// Writes the brief and starts the pane. The error is the reason, for the log.
    fn start(&self, profile: &CompanionProfile, slot: &mut Slot, pane_id: &str, now: i64) -> Result<(), String> {
        agents::check_launch_args(&profile.launch_args).map_err(|e| e.to_string())?;
        self.host.agent_ok(&profile.agent_type, &profile.folder)?;
        files::ensure_home(&self.root, profile).map_err(|e| e.to_string())?;
        let home = Path::new(&profile.folder);
        let texts = memory::snapshot(&self.root, &profile.id, home).map_err(|e| e.to_string())?;
        let instruction_files = self.host.instruction_files(&profile.agent_type);
        brief::write(home, &brief::compose(&profile.name, &texts), &instruction_files).map_err(|e| e.to_string())?;
        self.host.spawn(&SpawnSpec {
            pane_id: pane_id.to_string(),
            agent_type: profile.agent_type.clone(),
            cwd: profile.folder.clone(),
            extra_args: profile.launch_args.clone(),
            account: profile.account.clone(),
        })?;
        slot.outgoing.retain(|o| o.kind != OutKind::Primer);
        if brief::needs_primer(&instruction_files) {
            let (target, _) = brief::target(&instruction_files);
            slot.outgoing.insert(0, Outgoing { kind: OutKind::Primer, text: brief::primer(&target), queued_ms: now });
            slot.skip_reply = true;
        }
        slot.last_reply_id = None;
        Ok(())
    }

    // ---- 3. the face ----------------------------------------------------------------------------

    fn derive_state(&self, profile: &CompanionProfile, slot: &mut Slot, pane_id: &str, now: i64) {
        let pane = if slot.keeper.want() { self.host.pane(pane_id) } else { Pane::Missing };
        let status = match pane {
            Pane::Alive(status) => Some(status),
            Pane::Missing | Pane::Dead => None,
        };
        slot.pane_there = status.is_some();
        let blocking = status.and_then(|_| self.host.blocking(pane_id));
        let sign_in = status.is_some_and(|s| blocking.is_none() && (s != Status::Running || !slot.keeper.booted()))
            && self.host.sign_in_gate(pane_id);
        let queued = if status.is_some() { self.host.waiting(pane_id).len() } else { 0 };
        let activity = status.filter(|s| *s == Status::Running).and_then(|_| self.host.activity(pane_id));
        let (state, line) = keeper::derive(&DeriveInput {
            name: &profile.name,
            agent_label: &slot.agent_label,
            sleeping: !slot.keeper.want(),
            stopped: slot.keeper.is_stopped(),
            pane: status,
            booted: slot.keeper.booted(),
            blocking: blocking.as_ref().map(|b| b.title.as_str()),
            sign_in,
            queued,
            activity: activity.as_deref(),
        });
        slot.needs = blocking.map(|b| NeedsRef { item_id: b.item_id, title: b.title });
        if state == CompanionState::NeedsYou && slot.prev_state != Some(CompanionState::NeedsYou) {
            self.feed(profile, FeedKind::NeedsYou, now, &line);
        }
        if slot.prev_state == Some(CompanionState::Working) && state == CompanionState::Idle {
            slot.reply_wait = Some(ReplyWait { since_ms: now });
        }
        slot.state = state;
        slot.line = line;
    }

    // ---- 4. messages ----------------------------------------------------------------------------

    fn flush_outgoing(&self, profile: &CompanionProfile, slot: &mut Slot, pane_id: &str, now: i64) {
        if !slot.pane_there || slot.state == CompanionState::NeedsYou {
            slot.outgoing.retain(|o| {
                let stale = now - o.queued_ms > OUTGOING_GIVE_UP_MS;
                if stale {
                    if let OutKind::Checkin(label) = &o.kind {
                        self.feed(profile, FeedKind::Missed, now, &format!("Skipped \"{label}\": {} could not take it.", profile.name));
                    }
                }
                !stale
            });
            return;
        }
        while let Some(next) = slot.outgoing.first().cloned() {
            match self.host.send(pane_id, &next.text) {
                Ok(()) => {
                    slot.outgoing.remove(0);
                    if let OutKind::Checkin(label) = &next.kind {
                        self.feed(profile, FeedKind::Checkin, now, &format!("Asked \"{label}\""));
                    }
                }
                Err(reason) => {
                    tracing::debug!(companion = %profile.id, %reason, "outbox did not take the message yet");
                    break;
                }
            }
        }
    }

    // ---- 5. nap, replies, memory ------------------------------------------------------------------

    /// The nap: close the pane the keeper's own way and let the next step start it with a new brief.
    fn maybe_nap(&self, profile: &CompanionProfile, slot: &mut Slot, pane_id: &str, now: i64, offset: i32) -> bool {
        if slot.state != CompanionState::Idle {
            return false;
        }
        let due = keeper::nap_due(&NapInput {
            now_ms: now,
            offset_secs: offset,
            nap_at: profile.nap_at,
            last_nap_ms: slot.runtime.last_nap_ms,
            idle_since_ms: slot.keeper.idle_since_ms(),
            queued: self.host.waiting(pane_id).len() + slot.outgoing.len(),
        });
        if !due {
            return false;
        }
        slot.keeper.note_own_kill();
        self.host.kill(pane_id);
        self.host.release_reader(pane_id, &profile.id);
        slot.reply_wait = None;
        slot.quiet_start = true;
        slot.runtime.last_nap_ms = Some(now);
        self.feed(profile, FeedKind::Napped, now, "Took a nap. Woke up with fresh notes.");
        true
    }

    /// After a turn: the agent's own last words go to the feed; the one-line turn summary if they never come.
    fn read_reply(&self, profile: &CompanionProfile, slot: &mut Slot, pane_id: &str, now: i64) {
        let Some(wait) = slot.reply_wait.clone() else { return };
        if now - wait.since_ms < REPLY_AFTER_MS {
            return;
        }
        let fresh = |reply: &super::host::Reply, known: &Option<String>| known.as_deref() != Some(reply.id.as_str());
        let found = match self.host.last_reply(pane_id, &profile.id) {
            Some(reply) if fresh(&reply, &slot.last_reply_id) => Some(reply),
            _ if now - wait.since_ms >= REPLY_GIVE_UP_MS => self.host.last_sentence(pane_id).filter(|r| fresh(r, &slot.last_reply_id)),
            _ => return,
        };
        slot.reply_wait = None;
        let Some(reply) = found else { return };
        slot.last_reply_id = Some(reply.id.clone());
        if std::mem::take(&mut slot.skip_reply) {
            return;
        }
        let text = reply.text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !text.is_empty() {
            self.feed(profile, FeedKind::Reply, now, &text);
        }
    }

    /// Counts new proposals; keeps the unflagged ones at once when the person turned review off.
    fn refresh_pending(&self, profile: &CompanionProfile, slot: &mut Slot, now: i64) {
        if now - slot.pending_checked_ms < PENDING_EVERY_MS {
            return;
        }
        slot.pending_checked_ms = now;
        let home = Path::new(&profile.folder);
        let Ok(mut items) = memory::read_pending(home) else { return };
        if !profile.memory_review && slot.state == CompanionState::Idle {
            for id in memory::auto_keep_ids(&items) {
                if memory::decide(&self.root, &profile.id, home, &id, memory::Decision::Keep).is_err() {
                    break;
                }
            }
            items = memory::read_pending(home).unwrap_or(items);
        }
        let count = items.len() as u32;
        if slot.pending_seen && count > slot.pending {
            self.feed(profile, FeedKind::Memory, now, &format!("{} wants to remember something.", profile.name));
        }
        slot.pending = count;
        slot.pending_seen = true;
    }

    // ---- views ------------------------------------------------------------------------------------

    pub(super) fn view_of(&self, inner: &Inner, i: usize, now: i64) -> CompanionView {
        let profile = &inner.profiles[i];
        let offset = self.host.offset_secs();
        let live = match inner.slots.get(&profile.id) {
            Some(slot) => view::Live {
                state: slot.state,
                state_line: slot.line.clone(),
                pane_id: slot.pane_there.then(|| pane_id_of(&profile.id)),
                needs: slot.needs.clone(),
                pending_memory: slot.pending,
                next_checkin: schedule::next(now, offset, &profile.checkins, &slot.runtime.last_runs),
                restarts: slot.keeper.restarts() as u32,
                agent_label: slot.agent_label.clone(),
            },
            None => view::Live {
                state: CompanionState::Sleeping,
                state_line: keeper::SLEEPING_LINE.to_string(),
                pane_id: None,
                needs: None,
                pending_memory: 0,
                next_checkin: None::<NextCheckIn>,
                restarts: 0,
                agent_label: profile.agent_type.clone(),
            },
        };
        view::build(profile, &live)
    }

    /// The desktop views (with folders) of every companion that is not archived.
    pub(super) fn all_views(&self, inner: &Inner, now: i64) -> Vec<CompanionView> {
        (0..inner.profiles.len()).filter(|i| !inner.profiles[*i].archived).map(|i| self.view_of(inner, i, now)).collect()
    }

    pub(super) fn publish_if_changed(&self, inner: &mut Inner, now: i64) {
        let views = self.all_views(inner, now);
        if views != inner.last_views {
            inner.last_views = views.clone();
            self.host.publish(&views);
        }
    }

    // ---- small helpers ----------------------------------------------------------------------------

    /// A feed line. The home folder never appears in it.
    pub(super) fn feed(&self, profile: &CompanionProfile, kind: FeedKind, now: i64, text: &str) {
        let clean = view::scrub(text, Some(&profile.folder));
        if let Err(e) = store::feed_append(&self.root, &profile.id, &FeedItem::new(kind, now, &clean)) {
            tracing::warn!(companion = %profile.id, error = %e, "could not write a feed line");
        }
    }

    pub(super) fn save_runtime(&self, id: &str, runtime: &Runtime) {
        if let Err(e) = store::runtime_save(&self.root, id, runtime) {
            tracing::warn!(companion = %id, error = %e, "could not save runtime.json");
        }
    }
}
