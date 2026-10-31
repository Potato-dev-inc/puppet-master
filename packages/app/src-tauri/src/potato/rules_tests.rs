//! `rules::decide` as a table of facts to a decision. Phase 1 has the app rows; the rows for an outside
//! surface are here too (the rule is data, not new code), so a later surface only adds rows.

use super::protocol::{Tools, Trust};
use super::rules::*;

fn app() -> Facts<'static> {
    Facts { surface: "app", sender: Sender::App, channel: Channel::NotAChannel, mentioned: true, replied_to_bot: false, from_bot: false, age_ms: 0, text_is_empty: false }
}

/// An outside surface; the name is only a label here.
fn outside() -> Facts<'static> {
    Facts { surface: "outer", sender: Sender::Other, channel: Channel::NotAChannel, mentioned: false, replied_to_bot: false, from_bot: false, age_ms: 1_000, text_is_empty: false }
}

const OWNER_PC: Decision = Decision::Send { trust: Trust::OwnerPc, tools: Tools::Full, folder_rule: FolderRule::Owner };
const OWNER_REMOTE: Decision = Decision::Send { trust: Trust::OwnerRemote, tools: Tools::Full, folder_rule: FolderRule::Owner };
const OTHERS: Decision = Decision::Send { trust: Trust::Others, tools: Tools::NoMemory, folder_rule: FolderRule::None };

fn open() -> Caps {
    Caps::default()
}

fn decide_with(f: Facts, config: Config, caps: Caps) -> Decision {
    decide(&f, &config, &caps)
}

#[test]
fn the_app_rows() {
    // The person at the keyboard: owner_pc, full tools, may ask for helpers.
    assert_eq!(decide_with(app(), Config::default(), open()), OWNER_PC);
    // ... but not an empty message
    assert_eq!(decide_with(Facts { text_is_empty: true, ..app() }, Config::default(), open()), Decision::Drop(Why::Empty));
    // The app surface is only ever the person at the keyboard.
    assert_eq!(decide_with(Facts { sender: Sender::Other, ..app() }, Config::default(), open()), Decision::Drop(Why::UnknownSurface));
    assert_eq!(decide_with(Facts { sender: Sender::PairedOwner, ..app() }, Config::default(), open()), Decision::Drop(Why::UnknownSurface));
    // ... and a typed message can only come from the app surface.
    assert_eq!(decide_with(Facts { surface: "outer", ..app() }, Config::default(), open()), Decision::Drop(Why::UnknownSurface));
}

#[test]
fn the_switches_come_first() {
    let off = Caps { disabled: true, ..open() };
    let paused = Caps { paused: true, ..open() };
    let over = Caps { over_limit: true, ..open() };
    let fast = Caps { sends_last_minute: SENDS_PER_MINUTE, ..open() };
    assert_eq!(decide_with(app(), Config::default(), off), Decision::Drop(Why::Disabled));
    assert_eq!(decide_with(app(), Config::default(), paused), Decision::Drop(Why::Paused));
    assert_eq!(decide_with(app(), Config::default(), over), Decision::Drop(Why::OverLimit));
    assert_eq!(decide_with(app(), Config::default(), fast), Decision::Drop(Why::RateLimited));
    // 19 in the last minute is still fine, 20 is not
    assert_eq!(decide_with(app(), Config::default(), Caps { sends_last_minute: SENDS_PER_MINUTE - 1, ..open() }), OWNER_PC);
    // the order: off beats everything
    assert_eq!(decide_with(app(), Config::default(), Caps { disabled: true, paused: true, over_limit: true, sends_last_minute: 99 }), Decision::Drop(Why::Disabled));
    // an outside message is held to the same switches
    assert_eq!(decide_with(Facts { sender: Sender::PairedOwner, ..outside() }, Config::default(), over), Decision::Drop(Why::OverLimit));
}

#[test]
fn a_direct_message_from_the_paired_owner_is_owner_remote_with_full_tools() {
    assert_eq!(decide_with(Facts { sender: Sender::PairedOwner, ..outside() }, Config::default(), open()), OWNER_REMOTE);
}

#[test]
fn a_direct_message_from_anyone_else_is_dropped_unless_the_person_let_others_in() {
    assert_eq!(decide_with(outside(), Config::default(), open()), Decision::Drop(Why::NotAllowed));
    // allowed: they are `others`, with no memory tools and never a folder
    assert_eq!(decide_with(outside(), Config { others_may_dm: true }, open()), OTHERS);
}

#[test]
fn another_bot_and_an_old_message_are_dropped_before_anything_else() {
    assert_eq!(decide_with(Facts { from_bot: true, sender: Sender::PairedOwner, ..outside() }, Config::default(), open()), Decision::Drop(Why::FromBot));
    assert_eq!(decide_with(Facts { age_ms: MAX_AGE_MS + 1, sender: Sender::PairedOwner, ..outside() }, Config::default(), open()), Decision::Drop(Why::Stale));
    assert_eq!(decide_with(Facts { age_ms: MAX_AGE_MS, sender: Sender::PairedOwner, ..outside() }, Config::default(), open()), OWNER_REMOTE);
}

#[test]
fn a_channel_needs_a_mention_and_an_approval() {
    let in_channel = |channel, mentioned, replied, sender| Facts { channel, mentioned, replied_to_bot: replied, sender, ..outside() };
    let cfg = Config::default();
    // not approved: a mention is remembered as pending, nothing else is
    assert_eq!(decide_with(in_channel(Channel::Unknown, true, false, Sender::Other), cfg, open()), Decision::Pending);
    assert_eq!(decide_with(in_channel(Channel::Pending, false, true, Sender::Other), cfg, open()), Decision::Pending);
    assert_eq!(decide_with(in_channel(Channel::Unknown, false, false, Sender::Other), cfg, open()), Decision::Drop(Why::NotMentioned));
    // approved, "only me": the owner is answered, anyone else is not
    assert_eq!(decide_with(in_channel(Channel::Approved(Who::OnlyMe), true, false, Sender::PairedOwner), cfg, open()), OWNER_REMOTE);
    assert_eq!(decide_with(in_channel(Channel::Approved(Who::OnlyMe), true, false, Sender::Other), cfg, open()), Decision::Drop(Why::NotAllowed));
    // approved, "anyone": others are answered with no memory and no folder
    assert_eq!(decide_with(in_channel(Channel::Approved(Who::Anyone), true, false, Sender::Other), cfg, open()), OTHERS);
    assert_eq!(decide_with(in_channel(Channel::Approved(Who::Anyone), false, true, Sender::Other), cfg, open()), OTHERS);
    // approved but not mentioned: silence, even for the owner
    assert_eq!(decide_with(in_channel(Channel::Approved(Who::Anyone), false, false, Sender::PairedOwner), cfg, open()), Decision::Drop(Why::NotMentioned));
}

#[test]
fn others_never_get_memory_or_a_folder() {
    for decision in [
        decide_with(outside(), Config { others_may_dm: true }, open()),
        decide_with(Facts { channel: Channel::Approved(Who::Anyone), mentioned: true, ..outside() }, Config::default(), open()),
    ] {
        let Decision::Send { trust, tools, folder_rule } = decision else { panic!("{decision:?}") };
        assert_eq!((trust, tools, folder_rule), (Trust::Others, Tools::NoMemory, FolderRule::None));
    }
}
