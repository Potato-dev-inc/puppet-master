//! "Notify me" for each bot (spec P7): when it replies, when it needs me, or never. One line per bot in
//! `<potato>/notify.json`: `{ "v": 1, "bots": { "<bot>": "reply" } }`. A bot with no line gets `reply`, the same default
//! a companion's replies have.
//!
//! `reply` sends the phones a push when the bot replies. It goes through the push hub's companion path (one message a
//! minute per bot, Focus Time respected, the same "New reply" words) via `push::notify_bot_reply`. `needs_you` is
//! stored now and sent later: the pushes for approvals and helper cards come with piece P6. `never` sends nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::store::{self, files_lock, StoreResult};

const FILE: &str = "notify.json";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Notify {
    #[default]
    Reply,
    NeedsYou,
    Never,
}

impl Notify {
    /// The wire word the panel sends: `reply`, `needs_you` or `never`.
    pub fn parse(word: &str) -> Option<Notify> {
        match word {
            "reply" => Some(Notify::Reply),
            "needs_you" => Some(Notify::NeedsYou),
            "never" => Some(Notify::Never),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Default, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
struct File {
    v: u32,
    bots: BTreeMap<String, Notify>,
}

fn path(root: &Path) -> PathBuf {
    root.join(FILE)
}

fn read(root: &Path) -> StoreResult<File> {
    Ok(store::read_json::<File>(&path(root), FILE)?.unwrap_or_default())
}

/// The choice of one bot (the bot must exist).
pub fn get(root: &Path, bot: &str) -> StoreResult<Notify> {
    store::require_bot(root, bot)?;
    Ok(read(root)?.bots.get(bot).copied().unwrap_or_default())
}

/// Saves the choice of one bot and answers it.
pub fn set(root: &Path, bot: &str, value: Notify) -> StoreResult<Notify> {
    let _files = files_lock();
    store::require_bot(root, bot)?;
    let mut file = read(root)?;
    file.v = store::STORE_VERSION;
    file.bots.insert(bot.to_string(), value);
    store::write_json(&path(root), &file, FILE)?;
    Ok(value)
}

/// Whether a reply of this bot should push the phones. A damaged file or an unknown bot pushes nothing.
pub fn pushes_replies(root: &Path, bot: &str) -> bool {
    get(root, bot).is_ok_and(|choice| choice == Notify::Reply)
}
