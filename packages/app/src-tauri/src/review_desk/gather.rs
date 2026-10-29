//! Review Desk: gather (SPEC n33 section 3.5 A and B). Reads every source through [`DeskIo`], turns each into items
//! with the pure converters, then fills what only the live layer can know and finishes every item the same way:
//! pane liveness, group, risk, fingerprint. Last it orders the list. All of it is read-only.
//!
//! What the converters leave for this file (RD9 notes):
//! - Night Shift: the Keep target branch, the changed files and the secret, CI and home-folder scans of the copy.
//! - Issues: the repository folder, the changed file names (the push facts only count them), a live pane, a title
//!   cleaned with the live redaction, and for a `Held` run the whole scan (its facts are empty).
//! - Copies and missions: the agent label, the turn count, the group.
//! - Folder: the exact changes between the save points when they exist, and the scans of those changes.
//! - Everyone: a pane id is kept only while that pane is open, the 14-day window, the 200-item cap.
//!
//! A source that cannot be read adds one plain line to `notes` and the others still show (S15 keeps this off the
//! UI thread: the commands call it from `spawn_blocking`).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::live::{CopyRead, DeskIo};
use super::model::{strip_controls, Clean, Facts, FileRow, Item, ItemKey, Keep, Marks, TITLE_MAX, WINDOW_MS};
use super::risk;
use super::sources::copies::{from_copies, status_of, tidy, CopyInput};
use super::sources::folder::{from_turns, PaneFacts};
use super::sources::group::group_of;
use super::sources::issues::from_issues;
use super::sources::night::from_night;
use super::view::{desk_view, DeskView};
use crate::issue_intake::model::{IssueRun, RunState};
use crate::issue_intake::pushcheck::{ci_paths, secret_paths};
use crate::missions::model::HANDLE_PREFIX;
use crate::night_shift::model::NightRun;
use crate::pty::status::PaneStatus;
use crate::turn_summary::model::TurnSummary;
use crate::worktrees::service::ListItem;

/// Used for the fix pane of an item whose pane is not open any more.
pub const PANE_CLOSED: &str = "The agent's pane is closed.";
const NIGHT_NOTE: &str = "Could not read Night Shift. Its tasks are not listed.";
const ISSUE_NOTE: &str = "Could not read Assign an issue. Its runs are not listed.";
const COPIES_NOTE: &str = "Could not read your separate copies. They are not listed.";
const AGENT_FALLBACK: &str = "Agent";

/// The items in order, and the plain notes of the sources that failed.
pub struct Gathered {
    pub items: Vec<Item>,
    pub notes: Vec<String>,
}

type Turns = Vec<(TurnSummary, Option<PathBuf>)>;

/// The full desk, ordered and capped at `ITEMS_CAP`.
pub fn gather(io: &dyn DeskIo) -> DeskView {
    let gathered = gather_items(io);
    desk_view(&gathered.items, gathered.notes)
}

/// The same, as items (the actions need them, not the view).
pub fn gather_items(io: &dyn DeskIo) -> Gathered {
    collect(io, None)
}

/// One item, read again now: the key's source only is filled. `None` = it is not waiting any more.
pub fn gather_one(io: &dyn DeskIo, key: &ItemKey) -> Option<Item> {
    collect(io, Some(key)).items.into_iter().next()
}

fn collect(io: &dyn DeskIo, only: Option<&ItemKey>) -> Gathered {
    let now = io.now_ms();
    let clean = io.clean();
    let mut notes: Vec<String> = Vec::new();
    let mut items: Vec<Item> = Vec::new();
    let mut owned: HashSet<String> = HashSet::new();

    let copies = io.copies().unwrap_or_else(|_| {
        notes.push(COPIES_NOTE.to_string());
        Vec::new()
    });
    let mut panes = io.panes();
    let wants = |key: &ItemKey| only.is_none_or(|wanted| wanted == key);
    let hint = only.map(ItemKey::source_hint);

    // Night Shift owns its copies (every try's handle), so the copies stage skips them.
    match io.night_run() {
        Ok(Some(run)) => {
            let (night_items, handles) = from_night(&run, &clean);
            owned.extend(handles);
            for mut item in night_items.into_iter().filter(|item| wants(&item.key)) {
                fill_night(io, &run, &mut item, &copies, &clean);
                items.push(item);
            }
        }
        Ok(None) => {}
        Err(_) => notes.push(NIGHT_NOTE.to_string()),
    }

    // Issue runs likewise.
    match io.issue_runs() {
        Ok(runs) => {
            let (issue_items, handles) = from_issues(&runs);
            owned.extend(handles);
            for mut item in issue_items.into_iter().filter(|item| wants(&item.key)) {
                fill_issue(io, &runs, &mut item, &copies, &clean);
                items.push(item);
            }
        }
        Err(_) => notes.push(ISSUE_NOTE.to_string()),
    }

    if hint.is_none() || hint == Some("copy") {
        items.extend(copy_items(io, &copies, &owned, only, &clean));
    }

    if hint.is_none() || hint == Some("folder") {
        let (turns, closed) = folder_turns(io, &copies, &panes, now);
        panes.extend(closed);
        let marks = Marks::load(&io.marks_root());
        let from_folder = from_turns(&turns, &panes, &marks, &|pane, started| io.checkpoint_before(pane, started), now);
        for mut item in from_folder.into_iter().filter(|item| wants(&item.key)) {
            fill_folder(io, &turns, &mut item, &clean);
            items.push(item);
        }
    }

    finish(&mut items, &panes, now);
    if only.is_none() {
        risk::order(&mut items);
    }
    Gathered { items, notes }
}

// ------------------------------------------------------------------------------------------------ Night Shift

fn list_item_of<'a>(copies: &'a [ListItem], handle: &str) -> Option<&'a ListItem> {
    copies.iter().find(|copy| copy.entry.worktree.handle == handle)
}

fn fill_night(io: &dyn DeskIo, run: &NightRun, item: &mut Item, copies: &[ListItem], clean: &Clean) {
    let Some(id) = item.key.as_str().strip_prefix("night:") else { return };
    let Some(task) = run.tasks.iter().find(|task| task.spec.id == id) else { return };
    let Some(copy) = task.outcome.as_ref().and_then(|outcome| outcome.copy.as_ref()) else { return };
    let listed = list_item_of(copies, &copy.handle);

    let read = io.copy_read(&task.spec.project, &copy.handle, listed.and_then(|l| l.summary));
    apply_read(&mut item.facts, &read, clean, true);

    // The Keep target is the project's current branch, read now. Unknown keeps "your current branch".
    if let (Keep::NightKeep { into, .. }, Some(branch)) = (&mut item.keep, io.keep_target(&task.spec.project)) {
        let branch = tidy(&branch, clean);
        if !branch.is_empty() {
            *into = branch;
        }
    }
    if item.pane_id.is_none() {
        item.pane_id = listed.and_then(|l| l.pane_id.clone());
    }
    if item.agent_label.is_empty() {
        item.agent_label = listed.map_or_else(|| AGENT_FALLBACK.to_string(), |l| agent_label(io, l));
    }
}

// ------------------------------------------------------------------------------------------------ issues

fn fill_issue(io: &dyn DeskIo, runs: &[IssueRun], item: &mut Item, copies: &[ListItem], clean: &Clean) {
    let Some(id) = item.key.as_str().strip_prefix("issue:") else { return };
    let Some(run) = runs.iter().find(|run| run.id == id) else { return };

    // The title: the converter used the pure redactor; the live one also masks env secrets and the home folder.
    item.title = title_line(clean, &item.title);

    let folder = io.issue_folder(&run.repo_id);
    if let Some(folder) = &folder {
        item.project = folder.clone();
    }
    let listed = run.handle.as_deref().and_then(|handle| list_item_of(copies, handle));
    if let (Some(folder), Some(handle)) = (&folder, &run.handle) {
        let read = io.copy_read(folder, handle, listed.and_then(|l| l.summary));
        // An AskPush run already carries the push check's counts and scans; only the names are missing.
        // A Held run carries nothing, so it gets everything from the copy.
        let held = matches!(run.state, RunState::Held { .. });
        apply_read(&mut item.facts, &read, clean, held);
    }
    item.pane_id = listed.and_then(|l| l.pane_id.clone());
}

// ------------------------------------------------------------------------------------------------ copies

fn agent_label(io: &dyn DeskIo, copy: &ListItem) -> String {
    match copy.agent_type.as_deref().map(|kind| io.label_of(kind)) {
        Some(label) if !label.trim().is_empty() => label,
        _ => AGENT_FALLBACK.to_string(),
    }
}

fn copy_items(
    io: &dyn DeskIo,
    copies: &[ListItem],
    owned: &HashSet<String>,
    only: Option<&ItemKey>,
    clean: &Clean,
) -> Vec<Item> {
    let mut inputs: Vec<CopyInput> = Vec::new();
    let mut turns: HashMap<String, u32> = HashMap::new();
    for copy in copies {
        let worktree = &copy.entry.worktree;
        let handle = worktree.handle.as_str();
        if copy.state != "unreviewed" || !copy.entry.exists || owned.contains(handle) {
            continue;
        }
        if only.is_some_and(|wanted| wanted.as_str() != format!("copy:{handle}")) {
            continue;
        }
        let read = io.copy_read(&worktree.repo, handle, copy.summary);
        let op = io.latest_op(&worktree.repo, handle);
        let mission = handle
            .strip_prefix(HANDLE_PREFIX)
            .and_then(|mission_id| io.mission(&worktree.repo, mission_id));
        if let Some(op) = &op {
            turns.insert(handle.to_string(), op.turns);
        }
        inputs.push(CopyInput {
            item: copy.clone(),
            changes: read.changes,
            op: op.map(|op| op.facts),
            mission,
            scan: read.scan,
        });
    }
    let mut out = from_copies(&inputs, owned, clean);
    for item in &mut out {
        let Some(handle) = item.key.as_str().strip_prefix("copy:") else { continue };
        if let Some(copy) = list_item_of(copies, handle) {
            item.agent_label = agent_label(io, copy);
        }
        if let Some(count) = turns.get(handle) {
            item.turns = (*count).max(1);
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------ folder

/// Every turn of the window, each with the separate copy it ran in (`Some`) or `None` for the pane's own folder;
/// and the panes that have turns but are not open any more (they still own work waiting for a look). A closed
/// pane is listed only when its turns recorded the folder they ran in.
fn folder_turns(io: &dyn DeskIo, copies: &[ListItem], open: &[PaneFacts], now: u64) -> (Turns, Vec<PaneFacts>) {
    let roots: Vec<PathBuf> = copies.iter().map(|copy| PathBuf::from(&copy.entry.worktree.path)).collect();
    let mut inside: HashMap<PathBuf, Option<PathBuf>> = HashMap::new();
    let mut closed: Vec<PaneFacts> = Vec::new();
    let mut turns: Turns = Vec::new();
    for (turn, project) in io.turns(now.saturating_sub(WINDOW_MS)) {
        // The open pane's folder is the best answer; else the folder the turn history recorded.
        let folder = open.iter().find(|pane| pane.id == turn.pane_id).map(|pane| pane.cwd.clone()).or(project);
        if let Some(folder) = &folder {
            let is_open = open.iter().any(|pane| pane.id == turn.pane_id);
            if !is_open && !closed.iter().any(|pane| pane.id == turn.pane_id) {
                closed.push(PaneFacts {
                    id: turn.pane_id.clone(),
                    cwd: folder.clone(),
                    title: String::new(),
                    status: PaneStatus::Idle,
                    parent: None,
                    alive: false,
                });
            }
        }
        let copy = folder.and_then(|folder| inside.entry(folder.clone()).or_insert_with(|| copy_root_of(&folder, &roots)).clone());
        turns.push((turn, copy));
    }
    (turns, closed)
}

/// The copy whose checkout holds `folder`, if any.
fn copy_root_of(folder: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    roots.iter().find(|root| crate::project_path::workspace_covers(root, folder)).cloned()
}

/// `(pane id, first turn number)` from `folder:<pane>:<lo>-<hi>`.
fn folder_parts(key: &ItemKey) -> Option<(&str, u32)> {
    let rest = key.as_str().strip_prefix("folder:")?;
    let (pane, range) = rest.split_once(':')?;
    let (lo, _) = range.split_once('-')?;
    Some((pane, lo.parse().ok()?))
}

/// The save points that bracket an item's work: the one before its first turn and the newest "after".
fn range_for(io: &dyn DeskIo, turns: &Turns, key: &ItemKey) -> Option<(String, String, String)> {
    let (pane, lo) = folder_parts(key)?;
    let first = turns.iter().map(|(turn, _)| turn).find(|turn| turn.pane_id == pane && turn.turn == lo)?;
    let from = io.checkpoint_before(pane, first.started_at_ms)?;
    let to = io.checkpoint_last_after(pane)?;
    Some((pane.to_string(), from, to))
}

/// The same, reading the turns itself (for the detail).
pub fn folder_range(io: &dyn DeskIo, key: &ItemKey) -> Option<(String, String, String)> {
    let turns = io.turns(io.now_ms().saturating_sub(WINDOW_MS));
    let turns: Turns = turns.into_iter().map(|(turn, _)| (turn, None)).collect();
    range_for(io, &turns, key)
}

fn fill_folder(io: &dyn DeskIo, turns: &Turns, item: &mut Item, clean: &Clean) {
    if let Some((pane, from, to)) = range_for(io, turns, &item.key) {
        let read = io.range_read(&pane, &from, &to);
        if read.changes.is_some() {
            apply_read(&mut item.facts, &read, clean, true);
            return;
        }
    }
    // No save points: only the names the turn summaries kept, and no line scan.
    let raw: Vec<String> = item.facts.files.iter().flatten().map(|row| row.path.clone()).collect();
    for row in item.facts.files.iter_mut().flatten() {
        row.path = tidy(&row.path, clean);
    }
    item.facts.secret_files = secret_paths(&raw).iter().map(|path| tidy(path, clean)).collect();
    item.facts.ci_files = ci_paths(&raw).iter().map(|path| tidy(path, clean)).collect();
    item.facts.lines_scanned = false;
}

// ------------------------------------------------------------------------------------------------ shared

/// Puts what was read from a copy or a range into the facts. File rows always; with `full` also the totals, the
/// secret and CI file names, and the line scan.
fn apply_read(facts: &mut Facts, read: &CopyRead, clean: &Clean, full: bool) {
    let Some(changes) = &read.changes else { return };
    facts.files = Some(
        changes
            .files
            .iter()
            .map(|file| FileRow {
                path: tidy(&file.path, clean),
                status: status_of(&file.status),
                added: file.additions,
                deleted: file.deletions,
                binary: file.binary,
            })
            .collect(),
    );
    if !full {
        return;
    }
    facts.files_total = u32::try_from(changes.files_changed).unwrap_or(u32::MAX);
    facts.added = changes.additions;
    facts.deleted = changes.deletions;
    // Found on the raw paths (a cleaned name could hide the pattern), shown cleaned.
    let raw: Vec<String> = changes.files.iter().map(|file| file.path.clone()).collect();
    facts.secret_files = secret_paths(&raw).iter().map(|path| tidy(path, clean)).collect();
    facts.ci_files = ci_paths(&raw).iter().map(|path| tidy(path, clean)).collect();
    match &read.scan {
        Some(scan) => {
            let lines = |lines: &[(String, u32)]| -> Vec<(String, u32)> {
                lines.iter().map(|(path, line)| (tidy(path, clean), *line)).collect()
            };
            facts.secret_lines = lines(&scan.secret_lines);
            facts.home_lines = lines(&scan.home_lines);
            facts.lines_scanned = scan.scanned;
        }
        None => {
            facts.secret_lines.clear();
            facts.home_lines.clear();
            facts.lines_scanned = false;
        }
    }
}

/// A title as one cleaned line, at most `TITLE_MAX` characters.
pub fn title_line(clean: &Clean, raw: &str) -> String {
    let cleaned = clean.apply(raw);
    let line = cleaned.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    strip_controls(line).trim().chars().take(TITLE_MAX).collect::<String>().trim_end().to_string()
}

/// The same finish for every item: window, pane liveness, fix pane, group, risk, fingerprint.
fn finish(items: &mut Vec<Item>, panes: &[PaneFacts], now: u64) {
    let floor = now.saturating_sub(WINDOW_MS);
    items.retain(|item| item.finished_at_ms >= floor);
    let open: HashSet<&str> = panes.iter().filter(|pane| pane.alive).map(|pane| pane.id.as_str()).collect();
    for item in items.iter_mut() {
        // A pane id may point at a pane that was closed since: keep it only while it is open.
        item.pane_id = item.pane_id.take().filter(|pane| open.contains(pane.as_str()));
        item.fix = match &item.pane_id {
            Some(pane) => Ok(pane.clone()),
            None => Err(PANE_CLOSED.to_string()),
        };
        item.group = item.pane_id.as_deref().and_then(|pane| group_of(panes, pane));
        item.risk = risk::risk(&item.facts, &|path| crate::tamper::path_role(path));
        item.fingerprint = risk::fingerprint(&item.key, &item.facts);
    }
}

/// `(project, handle)` of the copy an item is about: where its patch is read from. `None` for folder items.
pub fn copy_ref_of(io: &dyn DeskIo, item: &Item) -> Option<(String, String)> {
    if let Keep::Merge { project, handle, .. } = &item.keep {
        return Some((project.clone(), handle.clone()));
    }
    if let Some(id) = item.key.as_str().strip_prefix("night:") {
        let run = io.night_run().ok().flatten()?;
        let task = run.tasks.iter().find(|task| task.spec.id == id)?;
        let copy = task.outcome.as_ref()?.copy.as_ref()?;
        return Some((task.spec.project.clone(), copy.handle.clone()));
    }
    if let Some(id) = item.key.as_str().strip_prefix("issue:") {
        let runs = io.issue_runs().ok()?;
        let run = runs.iter().find(|run| run.id == id)?;
        return Some((io.issue_folder(&run.repo_id)?, run.handle.clone()?));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_parts_reads_the_pane_and_the_first_turn() {
        let key = ItemKey::parse("folder:pane-7:3-9").unwrap();
        assert_eq!(folder_parts(&key), Some(("pane-7", 3)));
        assert_eq!(folder_parts(&ItemKey::parse("copy:h").unwrap()), None);
    }

    #[test]
    fn a_title_is_one_clean_line() {
        let clean = Clean { text: Box::new(|raw| raw.replace("secret", "[x]")) };
        assert_eq!(title_line(&clean, "\n  #4 fix secret  \nsecond line"), "#4 fix [x]");
        assert_eq!(title_line(&clean, &"a".repeat(300)).chars().count(), TITLE_MAX);
    }
}
