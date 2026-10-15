//! The store behind `GET /attention`: mirrors the facts pushed in (pane status, the prompt a pane
//! shows, how many times it asked), merges the items producers raised, runs `derive`, orders and
//! dedupes the result, keeps tombstones for what just left, and stamps a revision on every change.
//!
//! Pure and clock-injected (`now_ms` is an argument), so everything here is tested with a virtual
//! clock. The process-wide instance and the tick thread live in `mod.rs`.

use super::copy;
use super::derive::derive;
use super::model::*;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

/// What a pane's current prompt looked like when it was read (from `pty://prompt`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptRec {
    pub prompt_id: Option<String>,
    pub kind: Option<String>,
    pub summary: Option<String>,
    pub risk_level: Option<String>,
    pub headline: Option<String>,
    pub choices: Vec<ChoiceFact>,
    /// Up to six plain sentences behind the risk.
    pub reasons: Vec<String>,
    /// Which time this pane asked (1, 2, ...). Set by `Store::on_prompt`.
    pub occurrence: u32,
}

impl PromptRec {
    /// The fingerprint of this prompt as occurrence `occurrence`: what a card shows and what an
    /// answer re-checks against the live screen (`attention::fingerprint`).
    pub fn fingerprint(&self, occurrence: u32) -> String {
        super::fingerprint::fingerprint(
            self.prompt_id.as_deref().unwrap_or(""),
            occurrence,
            self.kind.as_deref().unwrap_or("unknown"),
            self.summary.as_deref(),
            self.choices.iter().map(|c| c.id.as_str()),
            self.risk_level.as_deref().and_then(Level::parse).map(Level::as_str),
        )
    }

    fn to_fact(&self) -> PromptFact {
        PromptFact {
            risk: self.risk_level.clone().map(|level| RiskFact { level: Some(level) }),
            prompt_id: self.prompt_id.clone(),
            kind: self.kind.clone(),
            summary: self.summary.clone(),
            occurrence: Some(self.occurrence),
            headline: self.headline.clone(),
            choices: self.choices.clone(),
            reasons: self.reasons.clone(),
        }
    }
}

#[derive(Clone)]
pub struct Store {
    rev: u64,
    /// pane id -> (status, since ms)
    statuses: HashMap<String, (String, u64)>,
    prompts: HashMap<String, PromptRec>,
    /// pane id -> how many times the pane has asked
    occurrences: HashMap<String, u32>,
    /// key -> (what the producer raised, when it first did)
    raised: BTreeMap<String, (Raised, u64)>,
    /// How an item left, noted by whoever resolved it (key -> resolution), used for its tombstone.
    hints: HashMap<String, Resolution>,
    last_open: Vec<Item>,
    /// (tombstone, expires at ms)
    tombstones: Vec<(Item, u64)>,
    snapshot: Snapshot,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            rev: 0,
            statuses: HashMap::new(),
            prompts: HashMap::new(),
            occurrences: HashMap::new(),
            raised: BTreeMap::new(),
            hints: HashMap::new(),
            last_open: Vec::new(),
            tombstones: Vec::new(),
            snapshot: Snapshot { v: 1, rev: 0, counts: Counts::default(), items: Vec::new() },
        }
    }
}

/// `base64url(sha256(kind | subject | occurrence))[..10]`: opaque, URL-safe, stable for one
/// occurrence, never containing a pane id.
pub fn item_id(kind: Kind, subject: &str, occurrence: u32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_str().as_bytes());
    hasher.update(b"|");
    hasher.update(subject.as_bytes());
    hasher.update(b"|");
    hasher.update(occurrence.to_string().as_bytes());
    let digest = hasher.finalize();
    URL_SAFE_NO_PAD.encode(digest)[..10].to_string()
}

impl Store {
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    pub fn counts(&self) -> Counts {
        self.snapshot.counts
    }

    // --- facts mirrored from the rest of the app ------------------------------------------------

    /// A pane reported its status. Remembers since when it has had this status.
    pub fn note_status(&mut self, pane_id: &str, status: &str, now_ms: u64) {
        match self.statuses.get(pane_id) {
            Some((current, _)) if current == status => {}
            _ => {
                self.statuses.insert(pane_id.to_string(), (status.to_string(), now_ms));
            }
        }
        if status != "waiting_input" {
            self.prompts.remove(pane_id);
        }
    }

    /// How many times this pane has asked (0 = never): the occurrence of the prompt it shows now.
    pub fn occurrence(&self, pane_id: &str) -> u32 {
        self.occurrences.get(pane_id).copied().unwrap_or(0)
    }

    pub fn forget_pane(&mut self, pane_id: &str) {
        self.statuses.remove(pane_id);
        self.prompts.remove(pane_id);
        self.occurrences.remove(pane_id);
    }

    /// The prompt tracker (`pane_prompt::next_prompt_event`) emitted an event for a pane: it entered
    /// `waiting_input`, its prompt changed (`waiting`, with the new prompt if one was parsed) or it
    /// left (`!waiting`). Every event with `waiting` starts a new occurrence, so the same command
    /// asked twice is two items.
    pub fn on_prompt(&mut self, pane_id: &str, waiting: bool, prompt: Option<PromptRec>) {
        if !waiting {
            self.prompts.remove(pane_id);
            return;
        }
        let n = self.occurrences.entry(pane_id.to_string()).or_default();
        *n += 1;
        let mut rec = prompt.unwrap_or_default();
        rec.occurrence = *n;
        self.prompts.insert(pane_id.to_string(), rec);
    }

    // --- producers ------------------------------------------------------------------------------

    /// A producer (budget, limit, consent, tamper, setup, review, done) raises an item. Raising the
    /// same key again updates it in place and keeps its `since`.
    pub fn raise(&mut self, mut raised: Raised, now_ms: u64) {
        if raised.expires_at_ms.is_none() {
            raised.expires_at_ms = match raised.kind {
                Kind::Consent => Some(now_ms + CONSENT_TTL_MS),
                Kind::Done => Some(now_ms + DONE_TTL_MS),
                _ => None,
            };
        }
        let since = self.raised.get(&raised.key).map(|(_, since)| *since).unwrap_or(now_ms);
        self.hints.remove(&raised.key);
        self.raised.insert(raised.key.clone(), (raised, since));
    }

    /// The producer is done with an item. `resolution` says how it ended (for the tombstone).
    pub fn clear(&mut self, key: &str, resolution: Option<Resolution>) -> bool {
        let removed = self.raised.remove(key).is_some();
        if removed {
            if let Some(resolution) = resolution {
                self.hints.insert(key.to_string(), resolution);
            }
        }
        removed
    }

    /// Someone answered the item with this key (a card, the lock screen, the desktop). The next
    /// tombstone for it says so.
    pub fn note_resolution(&mut self, key: &str, resolution: Resolution) {
        self.hints.insert(key.to_string(), resolution);
    }

    // --- the snapshot ---------------------------------------------------------------------------

    /// When the store next needs a `recompute` even if nothing else happens: a tombstone to drop,
    /// a question or consent to expire.
    pub fn next_deadline(&self) -> Option<u64> {
        let tombs = self.tombstones.iter().map(|(_, until)| *until);
        let expiring = self.snapshot.items.iter().filter(|i| i.state == State::Open).filter_map(|i| i.expires_at_ms);
        tombs.chain(expiring).min()
    }

    /// Rebuild the snapshot from the live facts. Returns true (and bumps `rev`) when it changed.
    pub fn recompute(&mut self, live: &Facts, now_ms: u64) -> bool {
        let facts = self.overlay(live);

        // Producer items that ran out of time leave with an `expired` resolution.
        let timed_out: Vec<String> = self
            .raised
            .iter()
            .filter(|(_, (r, _))| r.expires_at_ms.is_some_and(|at| at <= now_ms))
            .map(|(key, _)| key.clone())
            .collect();
        for key in timed_out {
            self.raised.remove(&key);
            self.hints.insert(key, Resolution { state: "expired".into(), at_ms: now_ms, ..Resolution::default() });
        }

        let mut items: Vec<Item> = derive(&facts).iter().map(|d| build_item(d, &facts)).collect();
        for (raised, since) in self.raised.values() {
            items.push(raised_item(raised, *since, &facts));
        }
        keep_one_pane_state(&mut items);
        let mut seen = HashSet::new();
        items.retain(|item| seen.insert(item.key.clone()));
        sort_items(&mut items);
        cap_items(&mut items);

        self.update_tombstones(&items, &facts, now_ms);
        self.last_open = items.clone();

        let mut all = items;
        let blocking_open = all.iter().filter(|i| i.lane == Lane::Blocking).count();
        let tombs: Vec<Item> = self.tombstones.iter().map(|(t, _)| t.clone()).collect();
        for (offset, tomb) in tombs.into_iter().enumerate() {
            all.insert(blocking_open + offset, tomb);
        }

        let counts = Counts {
            blocking: all.iter().filter(|i| i.state == State::Open && i.lane == Lane::Blocking).count() as u32,
            ready: all.iter().filter(|i| i.state == State::Open && i.lane == Lane::Ready).count() as u32,
        };
        if all == self.snapshot.items && counts == self.snapshot.counts {
            return false;
        }
        self.rev += 1;
        self.snapshot = Snapshot { v: 1, rev: self.rev, counts, items: all };
        true
    }

    /// Fill in what the live facts do not carry: when each pane entered its status and what its
    /// prompt looks like.
    fn overlay(&self, live: &Facts) -> Facts {
        let mut facts = live.clone();
        for pane in &facts.panes {
            if !facts.status_since.contains_key(&pane.id) {
                if let Some((status, since)) = self.statuses.get(&pane.id) {
                    if *status == pane.status {
                        facts.status_since.insert(pane.id.clone(), *since);
                    }
                }
            }
            if pane.status == "waiting_input" && !facts.prompts.contains_key(&pane.id) {
                if let Some(rec) = self.prompts.get(&pane.id) {
                    facts.prompts.insert(pane.id.clone(), Some(rec.to_fact()));
                }
            }
        }
        facts
    }

    fn update_tombstones(&mut self, now_open: &[Item], facts: &Facts, now_ms: u64) {
        let open_ids: HashSet<&str> = now_open.iter().map(|i| i.id.as_str()).collect();
        let open_keys: HashSet<&str> = now_open.iter().map(|i| i.key.as_str()).collect();
        let live_panes: HashSet<&str> = facts.panes.iter().map(|p| p.id.as_str()).collect();
        self.tombstones.retain(|(t, until)| *until > now_ms && !open_ids.contains(t.id.as_str()));
        for prev in &self.last_open {
            if prev.lane != Lane::Blocking || open_ids.contains(prev.id.as_str()) {
                continue;
            }
            let resolution = self.hints.remove(&prev.key).unwrap_or_else(|| {
                let expired = prev.expires_at_ms.is_some_and(|at| at <= now_ms);
                let (state, reason) = if expired {
                    ("expired", None)
                } else if prev.pane.as_ref().is_some_and(|p| !live_panes.contains(p.id.as_str())) {
                    ("gone", Some("pane_closed"))
                } else if open_keys.contains(prev.key.as_str()) {
                    ("gone", Some("changed"))
                } else {
                    ("gone", Some("moved_on"))
                };
                Resolution { state: state.into(), reason: reason.map(str::to_string), at_ms: now_ms, ..Resolution::default() }
            });
            let mut tomb = prev.clone();
            tomb.state = State::Resolved;
            tomb.resolution = Some(resolution);
            tomb.actions.clear();
            self.tombstones.retain(|(t, _)| t.id != tomb.id);
            self.tombstones.push((tomb, now_ms + TOMBSTONE_MS));
        }
        // Notes about items that never showed up again are stale after a minute.
        self.hints.retain(|_, r| now_ms.saturating_sub(r.at_ms) < 60_000);
    }
}

/// A pane shows one dialog at a time, so it has at most ONE pane-state item: a trust gate beats an
/// approval, which beats an error, which beats "stuck" (a waiting pane is not stuck, and an errored
/// pane's loop signal is noise). `deriveNeedsYou` could emit both `error:x` and `stuck:x`; counting
/// them twice made one problem look like two.
fn keep_one_pane_state(items: &mut Vec<Item>) {
    let precedence = |kind: Kind| match kind {
        Kind::TrustGate => 0,
        Kind::Approval => 1,
        Kind::Error => 2,
        _ => 3,
    };
    let mut best: HashMap<String, (u8, usize)> = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        if !item.kind.is_pane_state() {
            continue;
        }
        let Some(pane) = &item.pane else { continue };
        let rank = precedence(item.kind);
        match best.get(&pane.id) {
            Some((current, _)) if *current <= rank => {}
            _ => {
                best.insert(pane.id.clone(), (rank, index));
            }
        }
    }
    let mut index = 0;
    items.retain(|item| {
        let keep = !item.kind.is_pane_state() || item.pane.as_ref().map_or(true, |p| best.get(&p.id).is_some_and(|(_, i)| *i == index));
        index += 1;
        keep
    });
}

fn sort_items(items: &mut [Item]) {
    items.sort_by(|a, b| {
        let lane = |i: &Item| if i.lane == Lane::Blocking { 0 } else { 1 };
        lane(a).cmp(&lane(b)).then_with(|| {
            if a.lane == Lane::Blocking {
                // Rank, then oldest first (unknown age last), then id: a total order.
                a.rank
                    .cmp(&b.rank)
                    .then_with(|| a.since_ms.unwrap_or(u64::MAX).cmp(&b.since_ms.unwrap_or(u64::MAX)))
                    .then_with(|| a.id.cmp(&b.id))
            } else {
                // Ready lane: newest first.
                b.since_ms.unwrap_or(0).cmp(&a.since_ms.unwrap_or(0)).then_with(|| a.id.cmp(&b.id))
            }
        })
    });
}

/// More than `MAX_ITEMS`: the oldest ready items go. Blocking items are never dropped.
fn cap_items(items: &mut Vec<Item>) {
    while items.len() > MAX_ITEMS {
        match items.iter().rposition(|i| i.lane == Lane::Ready) {
            Some(index) => {
                items.remove(index);
            }
            None => break,
        }
    }
}

fn pane_ref(pane_id: &str, facts: &Facts) -> Option<PaneRef> {
    let pane = facts.panes.iter().find(|p| p.id == pane_id)?;
    let agent = pane.agent_label.clone();
    let heading = pane.heading.clone().or_else(|| agent.clone()).unwrap_or_else(|| "Agent".into());
    Some(PaneRef { id: pane.id.clone(), heading, agent, project: pane.project.clone() })
}

fn agent_of(pane: &Option<PaneRef>) -> String {
    pane.as_ref().and_then(|p| p.agent.clone()).unwrap_or_else(|| "An agent".into())
}

fn build_item(d: &Derived, facts: &Facts) -> Item {
    let pane = d.pane_id.as_deref().and_then(|id| pane_ref(id, facts));
    let subject = d.id.split_once(':').map(|(_, rest)| rest).unwrap_or(&d.id);
    let prompt_fact = d.pane_id.as_deref().and_then(|id| facts.prompts.get(id)).and_then(|p| p.as_ref());
    let agent = agent_of(&pane);
    let mut item = Item {
        id: String::new(),
        key: d.id.clone(),
        kind: d.kind,
        lane: lane_of(d.kind),
        rank: rank_of(d.kind, d.risk),
        state: State::Open,
        since_ms: d.since,
        expires_at_ms: None,
        pane,
        title: copy::title_for(d.kind, &agent),
        sentence: None,
        risk: None,
        prompt: None,
        actions: Vec::new(),
        resolution: None,
    };
    let mut occurrence = 1;
    let startup_gate = d.kind == Kind::Approval
        && prompt_fact
            .and_then(|p| p.kind.as_deref())
            .is_some_and(|kind| crate::pane_prompt::STARTUP_GATE_KINDS.contains(&kind));
    if startup_gate {
        // Folder trust or a sign-in screen: only the person may pass it, and never from a lock screen.
        // The `key` stays `approval:<pane>` so clients that match on it keep working.
        item.kind = Kind::TrustGate;
        item.lane = lane_of(Kind::TrustGate);
        item.rank = rank_of(Kind::TrustGate, d.risk);
    }
    match d.kind {
        Kind::Approval => {
            item.risk = d.risk.map(|level| RiskView {
                level,
                word: level.word(),
                headline: prompt_fact.and_then(|p| p.headline.clone()),
                reasons: prompt_fact.map(|p| p.reasons.clone()).unwrap_or_default(),
            });
            if let Some(p) = prompt_fact {
                if let Some(headline) = p.headline.as_deref().filter(|h| !h.trim().is_empty()) {
                    item.title = headline.to_string();
                }
                item.sentence = p.summary.clone();
                if startup_gate {
                    let project = item.pane.as_ref().and_then(|p| p.project.clone()).unwrap_or_else(|| "this folder".into());
                    if p.choices.is_empty() {
                        item.title = format!("{agent} needs you on your PC");
                        item.sentence = Some("It's showing a sign-in screen. Puppet Master won't answer it for you.".into());
                    } else {
                        item.title = "Trust this folder?".into();
                        item.sentence = Some(format!("{agent} wants to work in {project}. It can read and change files there."));
                    }
                }
                occurrence = p.occurrence.unwrap_or(1);
                if let Some(prompt_id) = &p.prompt_id {
                    let kind = p.kind.clone().unwrap_or_else(|| "unknown".into());
                    item.prompt = Some(PromptView {
                        prompt_id: prompt_id.clone(),
                        occurrence,
                        fingerprint: super::fingerprint::fingerprint(
                            prompt_id,
                            occurrence,
                            &kind,
                            p.summary.as_deref(),
                            p.choices.iter().map(|c| c.id.as_str()),
                            d.risk.map(Level::as_str),
                        ),
                        // A startup gate leads with "Don't trust" like a high-risk prompt leads with Deny.
                        layout: super::layout::layout(&p.choices, if startup_gate { Some(Level::High) } else { d.risk }),
                        provenance: "screen",
                        kind,
                        summary: p.summary.clone(),
                        choices: p.choices.clone(),
                    });
                }
            }
        }
        Kind::Question => {
            if let Some(q) = facts.questions.iter().find(|q| q.id == subject) {
                item.expires_at_ms = q.expires_at_ms;
                if let Some(text) = q.text.as_deref().filter(|t| !t.trim().is_empty()) {
                    item.title = text.to_string();
                }
            }
        }
        Kind::Budget | Kind::Limit => {
            // "Budget used up" / "Usage limit reached" over the card's own line ("Claude Code used
            // its 20 turns.", "Claude Code ran out of usage. It can continue at 5:00 PM.").
            if let Some(q) = facts.questions.iter().find(|q| q.id == subject) {
                item.expires_at_ms = q.expires_at_ms;
                item.sentence = q.text.clone().filter(|t| !t.trim().is_empty());
            }
        }
        Kind::Answer => {
            if let Some(n) = facts.notices.iter().find(|n| n.id == subject) {
                item.sentence = n.text.clone();
            }
        }
        _ => {}
    }
    item.id = item_id(item.kind, subject, occurrence);
    item
}

fn raised_item(r: &Raised, since: u64, facts: &Facts) -> Item {
    let pane = r.pane_id.as_deref().and_then(|id| pane_ref(id, facts));
    Item {
        id: item_id(r.kind, &r.key, 1),
        key: r.key.clone(),
        kind: r.kind,
        lane: lane_of(r.kind),
        rank: rank_of(r.kind, None),
        state: State::Open,
        since_ms: Some(since),
        expires_at_ms: r.expires_at_ms,
        pane,
        title: r.title.clone(),
        sentence: r.sentence.clone(),
        risk: None,
        prompt: None,
        actions: r.actions.clone(),
        resolution: None,
    }
}
