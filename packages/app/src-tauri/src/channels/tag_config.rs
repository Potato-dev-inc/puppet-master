//! Tag Your Agent: the saved settings of the group bot (the `tagBot` part of `telegram.json`).
//!
//! Everything here defaults to the safe choice: the switch is off, no group is approved, and a group
//! that is approved lets only the paired person tag the bot. A group the bot was tagged in but the person
//! has not approved is only remembered as "pending" (its id and title), so the person can approve it
//! on the PC. `sanitize` runs on load and on save: the file can be edited by hand, so every limit is
//! enforced there and not only where the app writes.

use crate::push::triggers::{sanitize as clean_text, truncate_chars};
use serde::{Deserialize, Serialize};

/// The most groups the person can approve.
pub const MAX_GROUPS: usize = 20;
/// The most pending groups kept; the newest stay.
pub const MAX_PENDING: usize = 10;
/// The longest style note.
pub const MAX_STYLE_CHARS: usize = 200;
/// The longest group title kept.
pub const MAX_TITLE_CHARS: usize = 80;
/// The longest folder path kept.
const MAX_FOLDER_CHARS: usize = 400;
/// The longest agent id kept.
const MAX_AGENT_CHARS: usize = 64;

/// Answers a day, for the whole bot.
pub const DEFAULT_DAILY_CAP: u32 = 30;
/// Answers an hour for one person, in a group where anyone may tag.
pub const DEFAULT_PER_PERSON_CAP: u32 = 5;
const MAX_DAILY_CAP: u32 = 500;
const MAX_PER_PERSON_CAP: u32 = 60;

/// Who may tag the bot in a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GroupWho {
    /// Only the paired Telegram user. The default.
    #[default]
    OnlyMe,
    /// Anybody in the group, with a per-person hourly cap.
    Anyone,
}

/// A group the person approved on the PC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TagGroup {
    pub chat_id: i64,
    pub title: String,
    pub who: GroupWho,
    /// The agent that answers in this group: an id from the registry's read-only list. `None`: the first
    /// installed one that can answer.
    pub agent: Option<String>,
    /// The project folder the (read-only) agent looks at. `None`: a neutral empty folder.
    pub folder: Option<String>,
    /// How the bot should sound in this group (at most 200 characters), set by the person on the PC.
    pub style: String,
}

impl Default for TagGroup {
    fn default() -> Self {
        Self { chat_id: 0, title: String::new(), who: GroupWho::OnlyMe, agent: None, folder: None, style: String::new() }
    }
}

/// A group that tagged the bot and waits for the person's yes. Only its id and title are kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PendingGroup {
    pub chat_id: i64,
    pub title: String,
    pub seen_at_ms: u64,
}

/// The group bot's settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TagBotConfig {
    /// The feature switch. Off: groups are dropped, exactly as before this feature existed.
    pub enabled: bool,
    /// Answers a day for the whole bot.
    pub daily_cap: u32,
    /// Answers an hour for one person (groups where anyone may tag).
    pub per_person_cap: u32,
    pub groups: Vec<TagGroup>,
    /// Oldest first, so the newest is last.
    pub pending: Vec<PendingGroup>,
}

impl Default for TagBotConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            daily_cap: DEFAULT_DAILY_CAP,
            per_person_cap: DEFAULT_PER_PERSON_CAP,
            groups: Vec::new(),
            pending: Vec::new(),
        }
    }
}

impl TagBotConfig {
    pub fn group(&self, chat_id: i64) -> Option<&TagGroup> {
        self.groups.iter().find(|g| g.chat_id == chat_id)
    }
}

/// A one-line text with hidden characters out, trimmed and cut.
fn line(raw: &str, max: usize) -> String {
    let kept: String = clean_text(raw).chars().filter(|c| !c.is_control()).collect();
    truncate_chars(kept.trim(), max)
}

fn some_line(raw: Option<String>, max: usize) -> Option<String> {
    raw.map(|v| line(&v, max)).filter(|v| !v.is_empty())
}

/// The folder a group's questions may use: none for a group open to anyone (no sandbox exists yet), else
/// the group's own. Used when the ask is made and again when it runs.
pub fn folder_for(group: &TagGroup) -> Option<String> {
    if group.who == GroupWho::Anyone {
        None
    } else {
        group.folder.clone()
    }
}

/// Slashes one way, no trailing slash, lower case: enough to compare two folders typed by a person.
fn norm_path(path: &str) -> String {
    path.trim().replace('\\', "/").trim_end_matches('/').to_lowercase()
}

/// A folder the group bot may look at. Never the home folder, a folder that contains it (a drive root,
/// `C:\Users`), or a relative path: a group member could make the agent read whatever is in there.
pub fn folder_is_allowed(folder: &str, home: Option<&str>) -> bool {
    let norm = norm_path(folder);
    let bytes = norm.as_bytes();
    let drive_path = bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/';
    let absolute = folder.trim_start().starts_with(['/', '\\']) || drive_path;
    // `norm` is empty for "/" (a root). A bare `c:` (from "C:\") is not a drive path, so it is refused above.
    let root = norm.is_empty();
    if !absolute || root || norm.split('/').any(|part| part == "..") {
        return false;
    }
    match home.map(norm_path).filter(|h| !h.is_empty()) {
        Some(home) => home != norm && !home.starts_with(&format!("{norm}/")),
        None => true,
    }
}

/// Enforce every limit. Used on load and on save.
pub fn sanitize(mut cfg: TagBotConfig) -> TagBotConfig {
    cfg.daily_cap = cfg.daily_cap.clamp(1, MAX_DAILY_CAP);
    cfg.per_person_cap = cfg.per_person_cap.clamp(1, MAX_PER_PERSON_CAP);
    let mut seen = std::collections::HashSet::new();
    cfg.groups.retain(|g| g.chat_id != 0 && seen.insert(g.chat_id));
    cfg.groups.truncate(MAX_GROUPS);
    for g in &mut cfg.groups {
        g.title = line(&g.title, MAX_TITLE_CHARS);
        g.agent = some_line(g.agent.take(), MAX_AGENT_CHARS);
        g.folder = some_line(g.folder.take(), MAX_FOLDER_CHARS);
        // Anything other people can trigger must not see a project folder until there is a sandbox.
        if g.who == GroupWho::Anyone && g.folder.take().is_some() {
            tracing::warn!(chat_id = g.chat_id, "a group open to anyone cannot have a project folder; the folder was dropped");
        }
        g.style = line(&g.style, MAX_STYLE_CHARS);
    }
    let mut seen = std::collections::HashSet::new();
    cfg.pending.retain(|p| p.chat_id != 0 && seen.insert(p.chat_id) && !cfg.groups.iter().any(|g| g.chat_id == p.chat_id));
    for p in &mut cfg.pending {
        p.title = line(&p.title, MAX_TITLE_CHARS);
    }
    if cfg.pending.len() > MAX_PENDING {
        let extra = cfg.pending.len() - MAX_PENDING;
        cfg.pending.drain(..extra);
    }
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_safe_choice() {
        let cfg = TagBotConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.daily_cap, 30);
        assert_eq!(cfg.per_person_cap, 5);
        assert!(cfg.groups.is_empty() && cfg.pending.is_empty());
        assert_eq!(TagGroup::default().who, GroupWho::OnlyMe);
    }

    #[test]
    fn a_file_without_the_part_takes_the_defaults() {
        let parsed: TagBotConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, TagBotConfig::default());
        let parsed: TagBotConfig = serde_json::from_str(r#"{"groups":[{"chatId":-5,"title":"Crew"}]}"#).unwrap();
        assert_eq!(parsed.groups[0].who, GroupWho::OnlyMe);
        assert!(!parsed.enabled);
    }

    #[test]
    fn who_uses_snake_case_names() {
        assert_eq!(serde_json::to_string(&GroupWho::OnlyMe).unwrap(), "\"only_me\"");
        assert_eq!(serde_json::to_string(&GroupWho::Anyone).unwrap(), "\"anyone\"");
    }

    #[test]
    fn caps_are_clamped() {
        let cfg = sanitize(TagBotConfig { daily_cap: 0, per_person_cap: 9999, ..TagBotConfig::default() });
        assert_eq!(cfg.daily_cap, 1);
        assert_eq!(cfg.per_person_cap, MAX_PER_PERSON_CAP);
        assert_eq!(sanitize(TagBotConfig { daily_cap: 100_000, ..TagBotConfig::default() }).daily_cap, MAX_DAILY_CAP);
    }

    #[test]
    fn the_style_note_is_cut_and_cleaned() {
        let group = TagGroup { chat_id: -5, style: format!("be brief\u{202E}{}", "x".repeat(500)), ..TagGroup::default() };
        let cfg = sanitize(TagBotConfig { groups: vec![group], ..TagBotConfig::default() });
        let style = &cfg.groups[0].style;
        assert!(style.chars().count() <= MAX_STYLE_CHARS, "{}", style.len());
        assert!(!style.contains('\u{202E}'));
    }

    #[test]
    fn pending_keeps_the_newest_ten() {
        let pending: Vec<PendingGroup> =
            (1..=14).map(|i| PendingGroup { chat_id: -i, title: format!("g{i}"), seen_at_ms: i as u64 }).collect();
        let cfg = sanitize(TagBotConfig { pending, ..TagBotConfig::default() });
        assert_eq!(cfg.pending.len(), MAX_PENDING);
        assert_eq!(cfg.pending.first().unwrap().chat_id, -5);
        assert_eq!(cfg.pending.last().unwrap().chat_id, -14);
    }

    #[test]
    fn duplicates_and_an_approved_group_in_pending_are_dropped() {
        let cfg = sanitize(TagBotConfig {
            groups: vec![
                TagGroup { chat_id: -1, title: "A".into(), ..TagGroup::default() },
                TagGroup { chat_id: -1, title: "again".into(), ..TagGroup::default() },
                TagGroup { chat_id: 0, ..TagGroup::default() },
            ],
            pending: vec![
                PendingGroup { chat_id: -1, title: "A".into(), seen_at_ms: 1 },
                PendingGroup { chat_id: -2, title: "B".into(), seen_at_ms: 2 },
                PendingGroup { chat_id: -2, title: "B2".into(), seen_at_ms: 3 },
            ],
            ..TagBotConfig::default()
        });
        assert_eq!(cfg.groups.len(), 1);
        assert_eq!(cfg.pending.iter().map(|p| p.chat_id).collect::<Vec<_>>(), vec![-2]);
    }

    #[test]
    fn the_home_folder_and_its_parents_are_never_allowed() {
        let home = Some("C:\\Users\\someone");
        assert!(!folder_is_allowed("C:\\Users\\someone", home));
        assert!(!folder_is_allowed("c:/users/someone/", home));
        assert!(!folder_is_allowed("C:\\Users", home));
        assert!(!folder_is_allowed("C:\\", home));
        assert!(!folder_is_allowed("C:", home));
        assert!(!folder_is_allowed("/", Some("/home/someone")));
        assert!(!folder_is_allowed("/home", Some("/home/someone")));
        assert!(folder_is_allowed("C:\\Users\\someone\\code\\shop", home));
        assert!(folder_is_allowed("/home/someone/code", Some("/home/someone")));
        assert!(folder_is_allowed("D:\\work\\shop", home));
    }

    #[test]
    fn relative_and_dotdot_folders_are_not_allowed() {
        assert!(!folder_is_allowed("shop", None));
        assert!(!folder_is_allowed("..\\shop", None));
        assert!(!folder_is_allowed("C:\\work\\..\\Users\\someone", Some("C:\\Users\\someone")));
        assert!(!folder_is_allowed("", None));
    }

    #[test]
    fn a_loaded_file_with_anyone_and_a_folder_keeps_anyone_and_drops_the_folder() {
        let raw = r#"{"tagBot": {"groups": [
            {"chatId": -5, "title": "Open", "who": "anyone", "folder": "D:\\work\\shop"},
            {"chatId": -6, "title": "Mine", "who": "only_me", "folder": "D:\\work\\shop"}]}}"#;
        let cfg = crate::channels::config::parse(raw);
        assert_eq!(cfg.tag_bot.groups[0].who, GroupWho::Anyone);
        assert_eq!(cfg.tag_bot.groups[0].folder, None);
        assert_eq!(cfg.tag_bot.groups[1].folder.as_deref(), Some("D:\\work\\shop"));
    }

    #[test]
    fn saving_anyone_with_a_folder_drops_the_folder() {
        let group = TagGroup { chat_id: -5, who: GroupWho::Anyone, folder: Some("D:\\work\\shop".into()), ..TagGroup::default() };
        let cfg = sanitize(TagBotConfig { groups: vec![group], ..TagBotConfig::default() });
        assert_eq!(cfg.groups[0].who, GroupWho::Anyone);
        assert_eq!(cfg.groups[0].folder, None);
    }

    #[test]
    fn folder_for_never_gives_an_anyone_group_a_folder() {
        let mut group = TagGroup { chat_id: -5, folder: Some("D:\\work\\shop".into()), ..TagGroup::default() };
        assert_eq!(folder_for(&group).as_deref(), Some("D:\\work\\shop"));
        group.who = GroupWho::Anyone;
        assert_eq!(folder_for(&group), None);
    }

    #[test]
    fn blank_agent_and_folder_become_none() {
        let group = TagGroup { chat_id: -3, agent: Some("  ".into()), folder: Some("".into()), ..TagGroup::default() };
        let cfg = sanitize(TagBotConfig { groups: vec![group], ..TagBotConfig::default() });
        assert_eq!(cfg.groups[0].agent, None);
        assert_eq!(cfg.groups[0].folder, None);
    }
}
