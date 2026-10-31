//! `decide`: the one pure function that says what happens to a message, whatever surface it came from
//! (spec section 2). Facts about the message, the saved choices and the day's caps go in; one of
//! `Drop`, `Pending` or `Send { trust, tools, folder_rule }` comes out.
//!
//! Phase 1 only has the app: the person typing in Puppet Master. It is `owner_pc`, gets full tools
//! and may ask for helpers. The rows for an outside surface are in here already, as data in the
//! `Facts` (is it a direct message, who is the sender, was the bot mentioned, is the channel
//! approved), so adding a surface later adds rows to the table in the tests, not new rule code.
//!
//! The rules that matter for safety, in the order they are checked:
//!
//! 1. nothing is answered while Potato Bot is off, while the bot is paused, or past its daily limit;
//! 2. at most 20 sends per minute per bot;
//! 3. an empty message, a message from another bot and an old message (over 10 minutes) are dropped;
//! 4. the paired owner is `owner_remote` and may use full tools; anyone else is `others`, gets
//!    `no_memory`, and never a folder (`FolderRule::None`);
//! 5. in a channel the bot answers only a mention or a reply to itself, only in an approved channel
//!    (an unknown one waits as `Pending`), and an approved "only me" channel answers only the owner.
//!
//! Nothing here names an agent or a chat app.

use super::protocol::{Tools, Trust};

pub const SENDS_PER_MINUTE: u32 = 20;
pub const MAX_AGE_MS: i64 = 10 * 60 * 1000;

/// Who sent the message.
/// The rows for an outside surface are data in the table of `rules_tests.rs`; no surface feeds them yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum Sender {
    /// The person at this computer, typing in the app.
    App,
    /// The paired owner on an outside surface.
    PairedOwner,
    /// Anyone else.
    Other,
}

/// Who may talk to the bot in an approved channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum Who {
    OnlyMe,
    Anyone,
}

/// What is known of the channel a message came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum Channel {
    /// A direct message, or the app itself: there is no channel.
    NotAChannel,
    Approved(Who),
    /// Remembered, waiting for the person's yes on the PC.
    Pending,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facts<'a> {
    /// `app`, or the name of an outside surface.
    pub surface: &'a str,
    pub sender: Sender,
    pub channel: Channel,
    pub mentioned: bool,
    pub replied_to_bot: bool,
    pub from_bot: bool,
    pub age_ms: i64,
    pub text_is_empty: bool,
}

/// The saved choices.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Config {
    /// "Let others DM": off unless the person turned it on.
    pub others_may_dm: bool,
}

/// The day's state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Caps {
    /// Potato Bot is switched off (the kill switch).
    pub disabled: bool,
    /// The bot is paused (the person stopped it, or no key).
    pub paused: bool,
    /// Today's dollar limit is used up.
    pub over_limit: bool,
    pub sends_last_minute: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderRule {
    /// The owner may ask for a helper in a folder the person picks on the PC.
    Owner,
    /// Never a folder: a helper request is refused with no card.
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    Disabled,
    Paused,
    OverLimit,
    RateLimited,
    Empty,
    Stale,
    FromBot,
    NotAllowed,
    NotMentioned,
    UnknownSurface,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Drop(Why),
    /// Remember the channel and ask the person on the PC. Nothing is sent to the brain.
    Pending,
    Send { trust: Trust, tools: Tools, folder_rule: FolderRule },
}

pub fn decide(facts: &Facts, config: &Config, caps: &Caps) -> Decision {
    if caps.disabled {
        return Decision::Drop(Why::Disabled);
    }
    if caps.paused {
        return Decision::Drop(Why::Paused);
    }
    if caps.over_limit {
        return Decision::Drop(Why::OverLimit);
    }
    if caps.sends_last_minute >= SENDS_PER_MINUTE {
        return Decision::Drop(Why::RateLimited);
    }
    if facts.text_is_empty {
        return Decision::Drop(Why::Empty);
    }
    if facts.sender == Sender::App {
        // The person at the keyboard. Only the app surface can be this.
        return if facts.surface == "app" {
            Decision::Send { trust: Trust::OwnerPc, tools: Tools::Full, folder_rule: FolderRule::Owner }
        } else {
            Decision::Drop(Why::UnknownSurface)
        };
    }
    if facts.surface == "app" {
        // Nobody else types into the app.
        return Decision::Drop(Why::UnknownSurface);
    }
    if facts.from_bot {
        return Decision::Drop(Why::FromBot);
    }
    if facts.age_ms > MAX_AGE_MS {
        return Decision::Drop(Why::Stale);
    }
    let owner = facts.sender == Sender::PairedOwner;
    let (trust, tools, folder_rule) = if owner {
        (Trust::OwnerRemote, Tools::Full, FolderRule::Owner)
    } else {
        (Trust::Others, Tools::NoMemory, FolderRule::None)
    };
    match facts.channel {
        Channel::NotAChannel => {
            if owner || config.others_may_dm {
                Decision::Send { trust, tools, folder_rule }
            } else {
                Decision::Drop(Why::NotAllowed)
            }
        }
        Channel::Unknown | Channel::Pending => {
            if facts.mentioned || facts.replied_to_bot {
                Decision::Pending
            } else {
                Decision::Drop(Why::NotMentioned)
            }
        }
        Channel::Approved(who) => {
            if !(facts.mentioned || facts.replied_to_bot) {
                Decision::Drop(Why::NotMentioned)
            } else if who == Who::OnlyMe && !owner {
                Decision::Drop(Why::NotAllowed)
            } else {
                Decision::Send { trust, tools, folder_rule }
            }
        }
    }
}
