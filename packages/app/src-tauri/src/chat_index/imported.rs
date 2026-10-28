//! The record of one old chat that was read into the index (`import_chats`). Its items are in `chat_items` with
//! `imported = 1` and the pane id `import:<agent>` (see `import_pane_id`).

/// The pane id of an agent's imported rows. No real pane has this id, so a search limited to panes never finds them.
pub fn import_pane_id(agent: &str) -> String {
    format!("import:{agent}")
}

/// One imported chat. `folder` is the folder NAME. `cwd` is the folder with the home folder as `~` (kept on this PC
/// only, to resume the chat later; never in a search result). `stamp` is `<len>:<mtime_ms>` or the database's update
/// time: the same stamp means the chat did not change since it was read. `items` is how many entries were kept for it.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedChatRow {
    pub agent: String,
    pub session: String,
    pub folder: String,
    pub cwd: String,
    pub title: String,
    pub updated_ms: i64,
    pub stamp: String,
    pub items: usize,
    pub cut_short: bool,
    pub imported_ms: i64,
}
