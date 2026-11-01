//! Who holds the sandbox screen: the Rust side of `packages/potato-brain/src/lifted/screen-lease.ts`.
//!
//! The first half is the same four rules as that file's SPEC (keep the two in step):
//!
//! 1. A lease id is `<ownerId>:<fence>`. The fence is the text after the LAST colon, ASCII digits only,
//!    at most 2^53 - 1. A bad id parses as `{ owner: <whole id>, fence: 0 }`.
//! 2. Take: refused without an id; granted when nobody holds, when the id is the holder's own (a
//!    renewal), or when the fence is STRICTLY higher than the holder's.
//! 3. Release: granted with no id, no holder or the same id; otherwise only the same owner with a fence
//!    that is not lower.
//! 4. Expiry: a lease holds the screen while `now < expires`. An expired lease blocks nobody, but its
//!    fence is not forgotten, so an older run cannot come back.
//!
//! The second half is the sandbox's use of them (spec "lease.rs"):
//!
//! - the holder is a bot or the person; a bot lease is bound to a run id (the owner part of its id);
//! - a lease lasts 15 minutes from its last use; a bot action under a lease that is no longer the
//!   holder's (taken over, expired, released) is `LEASE_LOST`;
//! - the person's take-over takes a new, higher fence. A bot never takes the screen from the person: it
//!   waits for "I'm done" or "Skip" (hand back), then takes a new lease;
//! - "Needs you" is the bot asking the person to step in: a card that lasts 30 minutes. A bot asks and then ends its turn
//!   ("I need a code from you"), so a run that ends with its card open keeps its lease and its card ("parked") until the
//!   person answers (take over, Skip), the card runs out, or the same run starts working again in its next turn.
//!
//! Pure: times are passed in as epoch milliseconds, so tests use no clock.

pub const LEASE_TTL_MS: i64 = 15 * 60 * 1000;
pub const NEEDS_YOU_TTL_MS: i64 = 30 * 60 * 1000;
/// The code a bot action gets when its lease is not the holder's any more.
pub const LEASE_LOST: &str = "LEASE_LOST";
pub const MAX_SAFE_FENCE: u64 = 9_007_199_254_740_991;
/// The owner part of the person's lease ids.
pub const PERSON_OWNER: &str = "person";
const REASON_CHARS: usize = 200;

// ---- the spec rules ------------------------------------------------------------------------------

pub fn lease_id(owner: &str, fence: u64) -> String {
    format!("{owner}:{fence}")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedId {
    pub owner: String,
    pub fence: u64,
}

pub fn parse_lease_id(id: &str) -> ParsedId {
    let whole = || ParsedId { owner: id.to_string(), fence: 0 };
    let Some(separator) = id.rfind(':') else { return whole() };
    if separator == 0 {
        return whole();
    }
    let digits = &id[separator + 1..];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return whole();
    }
    match digits.parse::<u64>() {
        Ok(fence) if fence <= MAX_SAFE_FENCE => ParsedId { owner: id[..separator].to_string(), fence },
        _ => whole(),
    }
}

/// Rule 2.
pub fn can_take(existing: Option<&str>, incoming: Option<&str>) -> bool {
    let Some(incoming) = incoming.filter(|i| !i.is_empty()) else { return false };
    match existing.filter(|e| !e.is_empty()) {
        None => true,
        Some(existing) if existing == incoming => true,
        Some(existing) => parse_lease_id(incoming).fence > parse_lease_id(existing).fence,
    }
}

/// Rule 3.
pub fn can_release(existing: Option<&str>, incoming: Option<&str>) -> bool {
    let (Some(existing), Some(incoming)) = (existing.filter(|e| !e.is_empty()), incoming.filter(|i| !i.is_empty())) else {
        return true;
    };
    if existing == incoming {
        return true;
    }
    let (current, next) = (parse_lease_id(existing), parse_lease_id(incoming));
    next.owner == current.owner && next.fence >= current.fence
}

/// Rule 4.
pub fn lease_held(expires_at_ms: Option<i64>, now_ms: i64) -> bool {
    expires_at_ms.is_some_and(|expires| now_ms < expires)
}

// ---- the table -----------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Holder {
    Bot(String),
    Person,
}

/// The bot run that had the screen when the person took over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Displaced {
    pub bot: String,
    pub run_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lease {
    pub id: String,
    pub holder: Holder,
    /// The run the lease is bound to (the owner part of `id`).
    pub run_id: String,
    pub expires_ms: i64,
    /// For the person's lease: who was using the screen when the person took over.
    pub displaced: Option<Displaced>,
}

/// The "Needs you" card: the bot asks the person to step in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NeedsYou {
    pub card_id: String,
    pub bot: String,
    pub run_id: String,
    pub reason: String,
    pub asked_ms: i64,
    pub expires_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeaseError {
    /// The lease is not the holder's any more (taken over, expired or released). The bot stops acting.
    LeaseLost,
    /// The person has the screen; a bot waits for "I'm done" or "Skip".
    PersonHasControl,
    /// No such card (answered, expired or never made).
    NoSuchCard,
    /// Only the bot holding the lease can ask.
    NotTheHolder,
}

impl LeaseError {
    pub fn code(&self) -> &'static str {
        match self {
            LeaseError::LeaseLost => LEASE_LOST,
            LeaseError::PersonHasControl => "LEASE_PERSON_HAS_CONTROL",
            LeaseError::NoSuchCard => "LEASE_NO_SUCH_CARD",
            LeaseError::NotTheHolder => "LEASE_NOT_HOLDER",
        }
    }
}

impl std::fmt::Display for LeaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for LeaseError {}

/// Things `tick` found out by the clock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeaseEvent {
    LeaseExpired { lease_id: String, holder: Holder },
    NeedsYouExpired { card_id: String, bot: String, run_id: String },
}

/// The header words of the Computer panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Banner {
    Free,
    /// "<bot> is using it"
    BotUsing { bot: String },
    /// "You have control"
    PersonHasControl,
    /// "Needs you"
    NeedsYou { bot: String, card_id: String },
}

#[derive(Clone, Debug, Default)]
pub struct ScreenLease {
    current: Option<Lease>,
    /// The id of the last lease that held the screen, even when it has expired or been released (rule 4).
    last_id: Option<String>,
    /// The highest fence ever handed out.
    high_fence: u64,
    card: Option<NeedsYou>,
    cards_made: u64,
    /// The run that asked ended its turn with the card open: both wait for the person (see `end_run`).
    parked: bool,
}

impl ScreenLease {
    pub fn new() -> Self {
        Self::default()
    }

    fn live(&self, now_ms: i64) -> Option<&Lease> {
        self.current.as_ref().filter(|l| lease_held(Some(l.expires_ms), now_ms))
    }

    fn grant(&mut self, holder: Holder, run_id: &str, now_ms: i64, displaced: Option<Displaced>) -> String {
        let fence = self.high_fence + 1;
        let id = lease_id(run_id, fence);
        // By construction the fence is strictly higher than every earlier one, so rule 2 holds; this
        // keeps the code honest if the counter and the rule ever drift apart.
        debug_assert!(can_take(self.last_id.as_deref(), Some(&id)));
        self.high_fence = fence;
        self.last_id = Some(id.clone());
        self.current = Some(Lease { id: id.clone(), holder, run_id: run_id.to_string(), expires_ms: now_ms + LEASE_TTL_MS, displaced });
        self.parked = false;
        id
    }

    /// A bot run takes the screen. A run that already holds it gets its own lease back (renewed). A newer
    /// run takes it from an older bot run. Never from the person.
    pub fn take_for_bot(&mut self, bot: &str, run_id: &str, now_ms: i64) -> Result<String, LeaseError> {
        if let Some(lease) = self.live(now_ms) {
            match &lease.holder {
                Holder::Person => return Err(LeaseError::PersonHasControl),
                Holder::Bot(owner) if owner == bot && lease.run_id == run_id => {
                    let id = lease.id.clone();
                    // The run is working again (a new turn): it is not waiting for the person any more.
                    if std::mem::take(&mut self.parked) {
                        self.card = None;
                    }
                    self.renew(&id, now_ms)?;
                    return Ok(id);
                }
                Holder::Bot(_) => {}
            }
        }
        // A card of another run is not this run's to keep.
        if self.card.as_ref().is_some_and(|c| c.run_id != run_id) {
            self.card = None;
        }
        Ok(self.grant(Holder::Bot(bot.to_string()), run_id, now_ms, None))
    }

    /// The person takes the screen with a new, higher fence. Always works. Answers any open card.
    pub fn take_over(&mut self, now_ms: i64) -> (String, Option<Displaced>) {
        let displaced = match self.live(now_ms) {
            Some(Lease { holder: Holder::Bot(bot), run_id, .. }) => Some(Displaced { bot: bot.clone(), run_id: run_id.clone() }),
            Some(Lease { holder: Holder::Person, displaced, .. }) => displaced.clone(),
            None => self.card.as_ref().map(|c| Displaced { bot: c.bot.clone(), run_id: c.run_id.clone() }),
        };
        self.card = None;
        self.parked = false;
        let id = self.grant(Holder::Person, PERSON_OWNER, now_ms, displaced.clone());
        (id, displaced)
    }

    /// Is `id` the live holder's lease? Does not extend it.
    pub fn check(&self, id: &str, now_ms: i64) -> Result<(), LeaseError> {
        match self.live(now_ms) {
            Some(lease) if lease.id == id => Ok(()),
            _ => Err(LeaseError::LeaseLost),
        }
    }

    /// The check a bot action passes, and the use that keeps the lease alive for 15 more minutes.
    pub fn renew(&mut self, id: &str, now_ms: i64) -> Result<(), LeaseError> {
        self.check(id, now_ms)?;
        if let Some(lease) = self.current.as_mut() {
            lease.expires_ms = now_ms + LEASE_TTL_MS;
            // A card the bot is waiting on keeps its lease for as long as the card lives.
            if let Some(card) = &self.card {
                lease.expires_ms = lease.expires_ms.max(card.expires_ms);
            }
        }
        Ok(())
    }

    /// Rule 3. Returns true when the screen was released (or was not held by anyone).
    pub fn release(&mut self, id: &str) -> bool {
        let existing = self.current.as_ref().map(|l| l.id.as_str());
        if !can_release(existing, Some(id)) {
            return false;
        }
        if let Some(lease) = self.current.take() {
            if self.card.as_ref().is_some_and(|c| c.run_id == lease.run_id) {
                self.card = None;
            }
        }
        true
    }

    /// The run ended (finished, failed or stopped): its lease and its card go. Except a run that ended its turn with a card
    /// open and its lease still live: it is waiting for the person, so both stay (parked) until the person answers, the
    /// card runs out, or the run takes the screen again.
    pub fn end_run(&mut self, run_id: &str, now_ms: i64) {
        let waiting = self.card(now_ms).is_some_and(|c| c.run_id == run_id)
            && self.live(now_ms).is_some_and(|l| l.run_id == run_id && matches!(l.holder, Holder::Bot(_)));
        if waiting {
            self.parked = true;
            return;
        }
        if self.current.as_ref().is_some_and(|l| l.run_id == run_id && matches!(l.holder, Holder::Bot(_))) {
            self.current = None;
        }
        if self.card.as_ref().is_some_and(|c| c.run_id == run_id) {
            self.card = None;
        }
        self.parked = false;
    }

    /// "I'm done": the person gives the screen back. Returns the run that was using it, so its loop can
    /// take a new lease and carry on.
    pub fn hand_back(&mut self, now_ms: i64) -> Option<Displaced> {
        match self.live(now_ms) {
            Some(Lease { holder: Holder::Person, displaced, .. }) => {
                let displaced = displaced.clone();
                self.current = None;
                displaced
            }
            _ => None,
        }
    }

    /// The bot holding `id` asks the person to step in. The card lasts 30 minutes.
    pub fn ask_for_help(&mut self, id: &str, reason: &str, now_ms: i64) -> Result<NeedsYou, LeaseError> {
        self.check(id, now_ms)?;
        let (bot, run_id) = match self.current.as_ref() {
            Some(Lease { holder: Holder::Bot(bot), run_id, .. }) => (bot.clone(), run_id.clone()),
            _ => return Err(LeaseError::NotTheHolder),
        };
        self.cards_made += 1;
        let card = NeedsYou {
            card_id: format!("needs-{}", self.cards_made),
            bot,
            run_id,
            reason: crate::redact::text(reason).into_inner().chars().filter(|c| !c.is_control()).take(REASON_CHARS).collect(),
            asked_ms: now_ms,
            expires_ms: now_ms + NEEDS_YOU_TTL_MS,
        };
        if let Some(lease) = self.current.as_mut() {
            lease.expires_ms = lease.expires_ms.max(card.expires_ms);
        }
        self.card = Some(card.clone());
        Ok(card)
    }

    /// "Skip": the person turns the card down. The bot keeps its lease and is told to move on.
    pub fn skip(&mut self, card_id: &str) -> Result<NeedsYou, LeaseError> {
        match self.card.take() {
            Some(card) if card.card_id == card_id => {
                self.parked = false;
                Ok(card)
            }
            other => {
                self.card = other;
                Err(LeaseError::NoSuchCard)
            }
        }
    }

    pub fn card(&self, now_ms: i64) -> Option<&NeedsYou> {
        self.card.as_ref().filter(|c| now_ms < c.expires_ms)
    }

    /// Clears what the clock ended and says what it was.
    pub fn tick(&mut self, now_ms: i64) -> Vec<LeaseEvent> {
        let mut events = Vec::new();
        if self.card.as_ref().is_some_and(|c| now_ms >= c.expires_ms) {
            if let Some(card) = self.card.take() {
                self.parked = false;
                events.push(LeaseEvent::NeedsYouExpired { card_id: card.card_id, bot: card.bot, run_id: card.run_id });
            }
        }
        if self.current.as_ref().is_some_and(|l| now_ms >= l.expires_ms) {
            if let Some(lease) = self.current.take() {
                events.push(LeaseEvent::LeaseExpired { lease_id: lease.id, holder: lease.holder });
            }
        }
        events
    }

    pub fn banner(&self, now_ms: i64) -> Banner {
        match (self.live(now_ms), self.card(now_ms)) {
            (Some(Lease { holder: Holder::Person, .. }), _) => Banner::PersonHasControl,
            (Some(Lease { holder: Holder::Bot(bot), .. }), Some(card)) if &card.bot == bot => {
                Banner::NeedsYou { bot: card.bot.clone(), card_id: card.card_id.clone() }
            }
            (Some(Lease { holder: Holder::Bot(bot), .. }), _) => Banner::BotUsing { bot: bot.clone() },
            _ => Banner::Free,
        }
    }

    /// The live lease, for the screen links (a control link is valid only while the person's lease is).
    pub fn current(&self, now_ms: i64) -> Option<&Lease> {
        self.live(now_ms)
    }

    pub fn highest_fence(&self) -> u64 {
        self.high_fence
    }
}
