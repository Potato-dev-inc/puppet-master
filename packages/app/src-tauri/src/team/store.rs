//! What the Team section shows, kept in `<app data>/team/state.json`:
//! - **drafts**: "Tell the team?" for my own pushes, waiting for my decision (never sent on their own);
//! - **cards**: teammates' events, after validation;
//! - **jobs**: "Queue my agent" merges and their results;
//! - **seen**: event ids already handled (the replay guard), kept until the event expires.
//!
//! [`ingest`] is the only way a teammate's event gets in, and it takes only a
//! [`Verified`](super::secure::Verified) event: one whose envelope signature, roster membership,
//! role and decryption were checked by `secure::Session::open` first (or a fact from the person's
//! own `gh`). It can add or update a card and nothing else: no job, no git command, no agent run.
//! Every action is a separate local command the person clicks.
//!
//! Also kept here (none of it secret): the replay guard and gap counters, my send counter, the
//! relay cursor, my open invites (their codes are in the credential store) and join requests
//! waiting for my approval.

use super::config::TeamConfig;
use super::event::{validate_inbound, DiffStat, TeamEvent};
use super::invite::{Invite, JoinRequest};
use super::secure::{SeqTracker, Verified};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const MAX_CARDS: usize = 100;
const MAX_DRAFTS: usize = 20;
const MAX_JOBS: usize = 50;
/// Event ids the replay guard keeps (each until its event expires, at most a week).
pub const MAX_SEEN: usize = 20_000;
const MAX_PROCESSED_JOINS: usize = 1_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub id: String,
    /// The event as it would be sent (the note is added on send).
    pub event: TeamEvent,
    /// Local repository the push happened in.
    pub repo: String,
    pub created_at_ms: u64,
    /// Set when an agent drafted it through the MCP tool `team_notify` (the label it gave, or
    /// "An agent"). `None`: Puppet Master saw my own push. Either way only my click sends it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drafted_by: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardState {
    New,
    Pulled,
    Queued,
    Dismissed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    /// The event id.
    pub id: String,
    pub event: TeamEvent,
    pub received_at_ms: u64,
    pub state: CardState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    /// Waiting for the chosen pane to finish its turn.
    Waiting,
    /// The merge stopped on conflicts and the chosen agent would run commands without asking
    /// (security review M6): it waits for the person to start it in a visible pane.
    NeedsStart,
    /// Merging in the separate copy, or the agent is resolving conflicts there.
    Working,
    /// Done in the separate copy; Apply / See changes / Discard.
    Ready,
    Failed,
    Applied,
    Discarded,
}

impl JobState {
    pub fn is_open(self) -> bool {
        matches!(self, Self::Waiting | Self::NeedsStart | Self::Working | Self::Ready | Self::Failed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub card_id: String,
    /// The local repository (the origin of the separate copy).
    pub project: String,
    /// The pane whose turn the job waits for; its agent type does the work.
    #[serde(default)]
    pub pane_id: Option<String>,
    pub agent_type: String,
    pub agent_label: String,
    /// Teammate's display name (already cleaned).
    pub who: String,
    pub their_branch: String,
    pub commit: String,
    #[serde(default)]
    pub note: Option<String>,
    /// My branch the update is for.
    pub into: String,
    pub state: JobState,
    /// Run handle = separate-copy handle.
    pub handle: String,
    #[serde(default)]
    pub copy_branch: Option<String>,
    /// Files that conflicted when merging.
    #[serde(default)]
    pub conflicts: Vec<String>,
    /// An agent run was needed (there were conflicts).
    #[serde(default)]
    pub agent_ran: bool,
    #[serde(default)]
    pub stats: Option<DiffStat>,
    /// Plain sentence for the card (why it failed, what Apply did).
    #[serde(default)]
    pub message: Option<String>,
    /// `now`: start at once; `idle`: when the pane is not working.
    #[serde(default)]
    pub when: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamStore {
    #[serde(default)]
    pub drafts: Vec<Draft>,
    #[serde(default)]
    pub cards: Vec<Card>,
    #[serde(default)]
    pub jobs: Vec<Job>,
    /// Event id → when it may be forgotten.
    #[serde(default)]
    pub seen: BTreeMap<String, u64>,
    /// Envelope replay guard and per-device counters (gap detection).
    #[serde(default)]
    pub seq: SeqTracker,
    /// The counter my next envelope carries.
    #[serde(default)]
    pub next_seq: u64,
    /// Relay mailbox position already read.
    #[serde(default)]
    pub cursor: u64,
    /// Invites I made (admin).
    #[serde(default)]
    pub invites: Vec<Invite>,
    /// Join requests that passed the code check, waiting for my yes or no (admin).
    #[serde(default)]
    pub joins: Vec<PendingJoin>,
    /// Hashes of join boxes already looked at (so a wrong code counts once).
    #[serde(default)]
    pub processed_joins: std::collections::BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingJoin {
    pub id: String,
    pub request: JoinRequest,
    pub received_at_ms: u64,
    /// Shown with the request: more than one person had this invite's code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// On every request for an invite whose code reached more than one person.
pub const CODE_SHARED: &str = "Someone else also used this invite's code. Only let in the person you invited.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ingested {
    /// A new card, or an existing card with the same dedupe key updated in place.
    Card(String),
    Ignored(&'static str),
}

fn addressed_to_me(event: &TeamEvent, config: &TeamConfig) -> bool {
    event.to.is_empty()
        || event
            .to
            .iter()
            .any(|to| to == &config.me.id || to.eq_ignore_ascii_case(&config.me.name))
}

/// Turn one verified event into a card, or drop it. Pure: no I/O.
pub fn ingest(store: &mut TeamStore, config: &TeamConfig, verified: Verified, now_ms: u64) -> Ingested {
    store.seen.retain(|_, until| *until > now_ms);
    // A GitHub fact about my own pull request is still news to me (CI went red on my PR).
    let local_fact = verified.member().is_none();
    let mut event = match validate_inbound(verified.into_event(), now_ms) {
        Ok(event) => event,
        Err(why) => return Ingested::Ignored(why.0),
    };
    if !local_fact {
        // Which card a teammate's event lands on follows from its own facts, never from a key the
        // sender chose (security review L4).
        event.dedupe_key = super::event::event_dedupe_key(&event);
    }
    if event.team != config.team {
        return Ingested::Ignored("another team");
    }
    if !local_fact && !config.me.id.is_empty() && event.actor.member == config.me.id {
        return Ingested::Ignored("my own event");
    }
    if store.seen.contains_key(&event.id) {
        return Ingested::Ignored("already seen");
    }
    store.remember_seen(event.id.clone(), event.expires_at_ms);
    if !event.kind.shown_as_card() {
        return Ingested::Ignored("kind not shown yet");
    }
    if !addressed_to_me(&event, config) {
        return Ingested::Ignored("addressed to someone else");
    }
    if let Some(existing) = store.cards.iter_mut().find(|c| c.event.dedupe_key == event.dedupe_key) {
        // A card GitHub's own facts made is never replaced by a teammate's words about it.
        if !local_fact && existing.event.source == super::event::EventSource::Github {
            return Ingested::Ignored("GitHub's card already shows this");
        }
        // A dismissed card stays gone until the state changes (a new commit, a new PR state).
        let same_state = existing.event.subject.commit == event.subject.commit
            && existing.event.kind == event.kind
            && existing.event.subject.pr.as_ref().map(|p| &p.state) == event.subject.pr.as_ref().map(|p| &p.state);
        if existing.state == CardState::Dismissed && same_state {
            return Ingested::Ignored("dismissed");
        }
        if existing.event.at_ms > event.at_ms {
            return Ingested::Ignored("older than the card");
        }
        // A fact without a note keeps the teammate's note from the earlier intent.
        let note = event.note.clone().or_else(|| existing.event.note.clone());
        existing.id = event.id.clone();
        existing.event = event;
        existing.event.note = note;
        existing.received_at_ms = now_ms;
        existing.state = CardState::New;
        return Ingested::Card(existing.id.clone());
    }
    let id = event.id.clone();
    store.cards.push(Card { id: id.clone(), event, received_at_ms: now_ms, state: CardState::New });
    if store.cards.len() > MAX_CARDS {
        let extra = store.cards.len() - MAX_CARDS;
        store.cards.drain(..extra);
    }
    Ingested::Card(id)
}

impl TeamStore {
    /// The replay guard remembers an event id until it expires, and at most [`MAX_SEEN`] ids
    /// (the ones expiring soonest go first).
    pub fn remember_seen(&mut self, id: String, until: u64) {
        self.seen.insert(id, until);
        if self.seen.len() > MAX_SEEN {
            let mut by_time: Vec<(u64, String)> = self.seen.iter().map(|(k, v)| (*v, k.clone())).collect();
            by_time.sort();
            for (_, id) in by_time.into_iter().take(self.seen.len() - MAX_SEEN * 9 / 10) {
                self.seen.remove(&id);
            }
        }
    }

    /// Join boxes already looked at matter only while one of my invites is open: a box for a
    /// closed invite is never counted. Forget them all once none is (security review L7).
    pub fn prune_processed_joins(&mut self, any_open_invite: bool) {
        if !any_open_invite || self.processed_joins.len() > MAX_PROCESSED_JOINS {
            self.processed_joins.clear();
        }
    }

    /// A join request whose code checked out, for invite `invite_id`. The first waits for the
    /// admin; one from someone else after it means the code leaked, and every request for that
    /// invite is flagged. The same person asking again is not a new request. True when added.
    pub fn add_join_request(&mut self, invite_id: &str, request: JoinRequest, now_ms: u64) -> bool {
        if self.joins.iter().any(|p| p.request.member == request.member) {
            return false;
        }
        let shared = self.invites.iter().any(|i| i.id == invite_id && i.proved);
        if let Some(i) = self.invites.iter_mut().find(|i| i.id == invite_id) {
            i.proved = true;
        }
        if shared {
            for p in self.joins.iter_mut().filter(|p| p.request.invite == invite_id) {
                p.warning = Some(CODE_SHARED.into());
            }
        }
        self.joins.push(PendingJoin {
            id: uuid::Uuid::new_v4().to_string(),
            request,
            received_at_ms: now_ms,
            warning: shared.then(|| CODE_SHARED.to_string()),
        });
        true
    }

    pub fn add_draft(&mut self, draft: Draft) {
        // A newer push of the same branch replaces the unsent draft about it.
        self.drafts.retain(|d| d.event.dedupe_key != draft.event.dedupe_key);
        self.drafts.push(draft);
        if self.drafts.len() > MAX_DRAFTS {
            let extra = self.drafts.len() - MAX_DRAFTS;
            self.drafts.drain(..extra);
        }
    }

    pub fn take_draft(&mut self, id: &str) -> Option<Draft> {
        let at = self.drafts.iter().position(|d| d.id == id)?;
        Some(self.drafts.remove(at))
    }

    pub fn card(&self, id: &str) -> Option<&Card> {
        self.cards.iter().find(|c| c.id == id)
    }

    pub fn card_mut(&mut self, id: &str) -> Option<&mut Card> {
        self.cards.iter_mut().find(|c| c.id == id)
    }

    pub fn job_mut(&mut self, id: &str) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|j| j.id == id)
    }

    pub fn add_job(&mut self, job: Job) {
        self.jobs.push(job);
        if self.jobs.len() > MAX_JOBS {
            // Forget the oldest finished ones first.
            if let Some(at) = self.jobs.iter().position(|j| !j.state.is_open()) {
                self.jobs.remove(at);
            }
        }
    }

    pub fn load(path: &Path) -> Self {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }
}
