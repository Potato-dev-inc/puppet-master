//! The generic store for a folder of JSONL chat files: `FileHistory`. An agent describes where its files are, how a
//! file names its chat and how to resume it; this code walks the folders, reads the first lines and streams the
//! items. Nothing here names an agent. Reads never write anything.

use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, Read};
use std::path::{Path, PathBuf};
use std::time::{Instant, UNIX_EPOCH};

use super::file_head::{head_lines, pointer_text};
use super::file_walk::collect;
use super::{resume_args_from, valid_session_id, HistoryChat, HistoryEnv, Listing, ListLimits, ReadError, ReadLimits, ReadOutcome, SessionHistory};
use crate::session_chat::declared::path::{expand, Names};
use crate::session_chat::tail::open_lines;
use crate::session_chat::{home_to_tilde, plain_path, ChatItem, SessionFormat};

/// What a file says about its chat, when the spec's own rule is not enough (the id and the folder).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMeta {
    pub id: String,
    /// The folder the chat was started in, as written in the file (`~` is applied by the store).
    pub cwd: Option<String>,
}

/// How a chat file names its chat (its session id).
#[derive(Clone, Copy)]
pub enum SessionKey {
    /// The file name without `.jsonl` or `.jsonl.zst`.
    FileStem,
    /// A JSON pointer (`/session_id`) whose text value is in the first lines of the file.
    Pointer(&'static str),
    /// A function that reads the id (and the folder) from the file, for a format that keeps them in a header.
    Meta(fn(&Path) -> Option<FileMeta>),
}

/// Where and how one agent keeps its chat files. Every field is `'static`, so a spec can be a `static`.
#[derive(Clone, Copy)]
pub struct FileHistorySpec {
    /// Folder templates: `{home}/.claude/projects`. A root that cannot be expanded or is not a folder is left out.
    pub roots: &'static [&'static str],
    /// Built-in only: `(variable, folder under it)`. When the variable is set, `<value>/<folder>` is a root too.
    pub extra_roots_env: &'static [(&'static str, &'static str)],
    /// The file-name glob (`*` and `?`). The name must also end in `.jsonl` or `.jsonl.zst`.
    pub glob: &'static str,
    /// Globs of names to leave out, such as sidechain files (`agent-*.jsonl`).
    pub skip: &'static [&'static str],
    /// Folders walked below a root (0 = the root's own files), at most 4.
    pub depth: usize,
    pub id: SessionKey,
    /// A JSON pointer to the folder the chat started in, looked up in the first lines.
    pub cwd_pointer: Option<&'static str>,
    pub format: &'static dyn SessionFormat,
    /// Resume arguments with the `{session}` placeholder. Empty = the agent has no resume option.
    pub resume: &'static [&'static str],
}

/// One agent's JSONL folder store, described by a spec.
pub struct FileHistory {
    spec: FileHistorySpec,
}

impl FileHistory {
    pub const fn new(spec: FileHistorySpec) -> Self {
        Self { spec }
    }

    /// The real roots that exist, sorted and unique.
    fn roots(&self, env: &HistoryEnv) -> Vec<PathBuf> {
        let mut templates: Vec<PathBuf> = with_names(env, |names| {
            self.spec.roots.iter().filter_map(|template| expand(template, names)).map(PathBuf::from).collect()
        });
        for (variable, folder) in self.spec.extra_roots_env {
            if let Some(dir) = env.var(variable) {
                templates.push(PathBuf::from(dir).join(folder));
            }
        }
        let mut roots: Vec<PathBuf> = templates
            .iter()
            .filter_map(|path| fs::canonicalize(plain_path(&path.to_string_lossy())).ok())
            .filter(|path| path.is_dir())
            .collect();
        roots.sort();
        roots.dedup();
        roots
    }

    /// The session id of one file, the cheapest way the spec allows.
    fn id_of(&self, path: &Path) -> Option<String> {
        match self.spec.id {
            SessionKey::FileStem => file_stem(path),
            SessionKey::Pointer(pointer) => pointer_text(&head_lines(path)?, pointer),
            SessionKey::Meta(meta) => meta(path).map(|found| found.id),
        }
    }

    /// The id and the folder of one file. Errors are `None`: the file is skipped and counted as unreadable.
    fn identify(&self, path: &Path) -> Option<FileMeta> {
        if let SessionKey::Meta(meta) = self.spec.id {
            return meta(path);
        }
        let head = match (self.spec.id, self.spec.cwd_pointer) {
            (SessionKey::Pointer(_), _) | (_, Some(_)) => Some(head_lines(path)?),
            _ => None,
        };
        let id = match self.spec.id {
            SessionKey::FileStem => file_stem(path)?,
            SessionKey::Pointer(pointer) => pointer_text(head.as_deref()?, pointer)?,
            SessionKey::Meta(_) => return None,
        };
        let cwd = self.spec.cwd_pointer.and_then(|pointer| pointer_text(head.as_deref()?, pointer));
        Some(FileMeta { id, cwd })
    }

    /// The one file of this chat. A plain file wins over a compressed copy of the same chat.
    fn locate(&self, roots: &[PathBuf], session: &str) -> Option<PathBuf> {
        let caps = ListLimits::default();
        let walked = collect(&self.spec, roots, caps.max_entries, caps.budget);
        let mut compressed: Option<PathBuf> = None;
        for path in walked.files {
            if self.id_of(&path).as_deref() != Some(session) {
                continue;
            }
            if !is_zst(&path) {
                return Some(path);
            }
            compressed.get_or_insert(path);
        }
        compressed
    }
}

impl SessionHistory for FileHistory {
    fn list(&self, env: &HistoryEnv, limits: &ListLimits) -> Listing {
        let roots = self.roots(env);
        let walked = collect(&self.spec, &roots, limits.max_entries, limits.budget);
        let started = Instant::now();
        let home = env.home().map(|home| home.to_string_lossy().into_owned());
        let mut out = Listing { cut_short: walked.cut_short, ..Listing::default() };
        let mut found: Vec<(bool, HistoryChat)> = Vec::new();
        for path in &walked.files {
            if started.elapsed() >= limits.budget {
                out.cut_short = true;
                break;
            }
            let Ok(meta) = fs::metadata(path) else {
                out.unreadable += 1;
                continue;
            };
            let updated_ms = modified_ms(&meta);
            if limits.since_ms.is_some_and(|since| updated_ms < since) {
                continue;
            }
            if meta.len() > limits.max_file_bytes {
                out.too_big += 1;
                continue;
            }
            let Some(file) = self.identify(path) else {
                out.unreadable += 1;
                continue;
            };
            if !valid_session_id(&file.id) {
                out.unreadable += 1;
                continue;
            }
            let cwd = file.cwd.filter(|cwd| !cwd.is_empty()).map(|cwd| home_to_tilde(&cwd, home.as_deref()));
            let chat = HistoryChat {
                session: file.id,
                cwd,
                updated_ms,
                bytes: meta.len(),
                stamp: format!("{}:{updated_ms}", meta.len()),
            };
            found.push((is_zst(path), chat));
        }
        out.chats = one_per_session(found);
        out.chats.sort_by(|a, b| b.updated_ms.cmp(&a.updated_ms).then_with(|| a.session.cmp(&b.session)));
        if out.chats.len() > limits.max_chats {
            out.chats.truncate(limits.max_chats);
            out.cut_short = true;
        }
        out
    }

    fn read(
        &self,
        env: &HistoryEnv,
        session: &str,
        limits: &ReadLimits,
        sink: &mut dyn FnMut(Vec<ChatItem>) -> bool,
    ) -> Result<ReadOutcome, ReadError> {
        if !valid_session_id(session) {
            return Err(ReadError::Missing);
        }
        let roots = self.roots(env);
        let path = self.locate(&roots, session).ok_or(ReadError::Missing)?;
        let reader = open_lines(&path).map_err(|err| match err.kind() {
            io::ErrorKind::NotFound => ReadError::Missing,
            _ => ReadError::Unreadable,
        })?;
        stream(reader, self.spec.format, limits, sink)
    }

    fn resume_args(&self, session: &str) -> Option<Vec<String>> {
        let template: Vec<String> = self.spec.resume.iter().map(|arg| arg.to_string()).collect();
        resume_args_from(&template, session)
    }

    fn places(&self, env: &HistoryEnv) -> Vec<String> {
        let home = env.home().map(|home| home.to_string_lossy().into_owned());
        let shown = |path: &str| home_to_tilde(&plain_path(path), home.as_deref());
        let mut out: Vec<String> = with_names(env, |names| {
            self.spec.roots.iter().filter_map(|template| expand(template, names)).map(|path| shown(&path)).collect()
        });
        for (variable, folder) in self.spec.extra_roots_env {
            if let Some(dir) = env.var(variable) {
                out.push(shown(&PathBuf::from(dir).join(folder).to_string_lossy()));
            }
        }
        out
    }
}

/// Streams the lines of one chat through the format, in chunks, within the read caps. A line that does not parse
/// (a half-written last line) gives no items. The sink returning false stops the read without `cut_short`.
fn stream(
    mut reader: Box<dyn BufRead>,
    format: &dyn SessionFormat,
    limits: &ReadLimits,
    sink: &mut dyn FnMut(Vec<ChatItem>) -> bool,
) -> Result<ReadOutcome, ReadError> {
    let chunk = limits.chunk.max(1);
    let mut outcome = ReadOutcome { items: 0, cut_short: false };
    let mut pending: Vec<ChatItem> = Vec::new();
    let mut offset: u64 = 0;
    let mut used: u64 = 0;
    loop {
        // One extra byte of room, so a line that goes past the cap is seen as going past it.
        let room = limits.max_decompressed.saturating_sub(used).saturating_add(1);
        let mut line = Vec::new();
        let read = (&mut reader).take(room).read_until(b'\n', &mut line).map_err(|_| ReadError::Unreadable)?;
        if read == 0 {
            break;
        }
        used += read as u64;
        if used > limits.max_decompressed {
            outcome.cut_short = true;
            break;
        }
        let text = String::from_utf8_lossy(&line);
        let items = format.parse_line(text.trim_end_matches(['\n', '\r']), offset);
        offset += read as u64;
        let mut capped = false;
        for item in items {
            if outcome.items >= limits.max_items {
                capped = true;
                break;
            }
            pending.push(item);
            outcome.items += 1;
        }
        if pending.len() >= chunk && !sink(std::mem::take(&mut pending)) {
            return Ok(outcome);
        }
        if capped {
            outcome.cut_short = true;
            break;
        }
    }
    if !pending.is_empty() {
        sink(pending);
    }
    Ok(outcome)
}

/// Keeps one file per session id: a plain file over a compressed one, otherwise the first seen.
pub(super) fn one_per_session(found: Vec<(bool, HistoryChat)>) -> Vec<HistoryChat> {
    let mut slot: HashMap<String, usize> = HashMap::new();
    let mut kept: Vec<(bool, HistoryChat)> = Vec::new();
    for (zst, chat) in found {
        match slot.get(&chat.session) {
            Some(&at) => {
                if kept[at].0 && !zst {
                    kept[at] = (zst, chat);
                }
            }
            None => {
                slot.insert(chat.session.clone(), kept.len());
                kept.push((zst, chat));
            }
        }
    }
    kept.into_iter().map(|(_, chat)| chat).collect()
}

/// Runs `f` with the names a template can use here (home and variables; no pane folder).
fn with_names<T>(env: &HistoryEnv, f: impl FnOnce(&Names<'_>) -> T) -> T {
    let lookup = |name: &str| env.var(name);
    let names = Names { home: env.home(), cwd: Path::new(""), env: &lookup };
    f(&names)
}

/// The session id from a file name: the name without `.jsonl` or `.jsonl.zst`.
fn file_stem(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    for suffix in [".jsonl.zst", ".jsonl"] {
        let cut = name.len().checked_sub(suffix.len())?;
        if name.is_char_boundary(cut) && name[cut..].eq_ignore_ascii_case(suffix) {
            return Some(name[..cut].to_string());
        }
    }
    None
}

fn is_zst(path: &Path) -> bool {
    path.to_string_lossy().to_ascii_lowercase().ends_with(".zst")
}

fn modified_ms(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_millis() as i64)
}
