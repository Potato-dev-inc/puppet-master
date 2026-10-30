//! `Move <name> to its own folder`: repairs a companion whose home is too broad (`broad.rs`), for example
//! the Desktop, where every agent below the folder reads the companion's brief.
//!
//! The move is copy, verify, switch, then remove the originals:
//! 1. the new folder must be absent or empty, and a folder of its own;
//! 2. the pane is closed (so nothing writes while the files are copied);
//! 3. the companion's own files are copied and compared byte for byte: `SOUL.md`, `USER.md`, `MEMORY.md`,
//!    `memory/pending.md`, `memory/journal/`, `skills/`, `today.md`, `days/` and `.puppet-master/companion.json`.
//!    Nothing else in the old folder is read, copied or touched;
//! 4. the home is completed and the brief is written in the new folder;
//! 5. only the companion block is cut out of the old instruction file (the file is removed only when nothing
//!    but whitespace is left; the `.bak` copy and every other file stay);
//! 6. the profile points at the new folder, and the pane starts again there;
//! 7. the copied originals are removed, each only if it still equals its copy.
//!
//! Any error before step 6 puts everything back: the new folder is emptied again, the old brief block is
//! written back, the profile is unchanged and the pane starts again where it was.
//!
//! Nothing here names an agent: the instruction files come from the profile through `Host::instruction_files`.

use std::fs;
use std::path::{Path, PathBuf};

use super::broad;
use super::brief::{self, Removed};
use super::files;
use super::model::{CompanionError, CompanionProfile, CompanionResult, CompanionView, FeedKind, MemoryFile};
use super::service::{Inner, Service};
use super::{skills, store, today};

const CODE: &str = "COMPANION_REHOME_FAILED";

fn fail(message: impl Into<String>) -> CompanionError {
    CompanionError::new(CODE, message)
}

/// The companion's own files inside a home folder (relative, `/`-separated).
fn unit_files() -> Vec<String> {
    let mut list: Vec<String> = MemoryFile::ALL.iter().map(|f| f.file_name().to_string()).collect();
    list.push(files::PENDING_REL.to_string());
    list.push(today::FILE_NAME.to_string());
    list.push(files::MARKER_REL.to_string());
    list
}

/// The companion's own folders inside a home folder.
fn unit_dirs() -> Vec<String> {
    vec![files::JOURNAL_REL.to_string(), skills::SKILLS_REL.to_string(), today::DAYS_DIR.to_string()]
}

/// What a copy made. Relative paths, `/`-separated.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Copied {
    pub files: Vec<String>,
    /// Folders, parents before children.
    pub dirs: Vec<String>,
}

fn at(base: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(base.to_path_buf(), |path, part| path.join(part))
}

/// The new folder must be absent or empty. True when this call created it.
pub fn prepare_new(new: &Path) -> CompanionResult<bool> {
    if !new.is_absolute() {
        return Err(fail("The new folder must be a full path."));
    }
    crate::path_limit::check(new).map_err(|_| fail("The new folder path is too long for Windows."))?;
    if new.exists() {
        if !new.is_dir() {
            return Err(fail("The new folder is a file. Pick another folder."));
        }
        let empty = fs::read_dir(new).map_err(|e| fail(format!("Could not open the new folder: {e}.")))?.next().is_none();
        if !empty {
            return Err(fail("The new folder is not empty. Pick an empty folder."));
        }
        return Ok(false);
    }
    fs::create_dir_all(new).map_err(|e| fail(format!("Could not create the new folder: {e}.")))?;
    Ok(true)
}

/// Puts the new folder back to what `prepare_new` found: removed when it was created, emptied when it was empty.
/// Only for a folder that this move filled.
pub fn undo_new(new: &Path, created: bool) {
    if created {
        let _ = fs::remove_dir_all(new);
        return;
    }
    if let Ok(entries) = fs::read_dir(new) {
        for entry in entries.flatten() {
            let path = entry.path();
            let _ = if path.is_dir() { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
        }
    }
}

/// Copies the companion's own files from `old` to `new` and compares every file. On an error nothing in `old`
/// changed; `new` may hold a partial copy (the caller undoes it).
pub fn copy_home(old: &Path, new: &Path) -> CompanionResult<Copied> {
    let mut copied = Copied::default();
    for rel in unit_files() {
        let from = at(old, &rel);
        if !from.is_file() {
            continue;
        }
        copy_one(&from, &at(new, &rel))?;
        copied.files.push(rel);
    }
    for rel in unit_dirs() {
        let from = at(old, &rel);
        if from.is_dir() {
            copy_dir(&from, &at(new, &rel), &rel, &mut copied)?;
        }
    }
    for rel in &copied.files {
        if !same_bytes(&at(old, rel), &at(new, rel)) {
            return Err(fail(format!("The copy of {rel} does not match the original. Nothing was moved.")));
        }
    }
    Ok(copied)
}

fn copy_one(from: &Path, to: &Path) -> CompanionResult<()> {
    if let Some(dir) = to.parent() {
        fs::create_dir_all(dir).map_err(|e| fail(format!("Could not create {}: {e}.", dir.display())))?;
    }
    fs::copy(from, to).map_err(|e| fail(format!("Could not copy {}: {e}.", from.display())))?;
    Ok(())
}

/// Copies a folder tree. Links are skipped, not followed.
fn copy_dir(from: &Path, to: &Path, rel: &str, copied: &mut Copied) -> CompanionResult<()> {
    fs::create_dir_all(to).map_err(|e| fail(format!("Could not create {}: {e}.", to.display())))?;
    copied.dirs.push(rel.to_string());
    let entries = fs::read_dir(from).map_err(|e| fail(format!("Could not read {}: {e}.", from.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| fail(format!("Could not read {}: {e}.", from.display())))?;
        let kind = entry.file_type().map_err(|e| fail(format!("Could not read {}: {e}.", from.display())))?;
        if kind.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let child = format!("{rel}/{name}");
        if kind.is_dir() {
            copy_dir(&entry.path(), &to.join(&name), &child, copied)?;
        } else if kind.is_file() {
            copy_one(&entry.path(), &to.join(&name))?;
            copied.files.push(child);
        }
    }
    Ok(())
}

fn same_bytes(a: &Path, b: &Path) -> bool {
    match (fs::read(a), fs::read(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Removes the originals that were copied, each only if it still equals its copy, then the folders that are
/// empty now. Best effort: a file that cannot be removed stays (a harmless duplicate). Returns how many stayed.
pub fn drop_originals(old: &Path, new: &Path, copied: &Copied) -> usize {
    let mut stayed = 0;
    for rel in &copied.files {
        let from = at(old, rel);
        if same_bytes(&from, &at(new, rel)) {
            if fs::remove_file(&from).is_err() {
                stayed += 1;
            }
        } else {
            stayed += 1;
        }
    }
    // Deepest first. `remove_dir` only removes an empty folder, so anything else in them stays.
    for rel in copied.dirs.iter().rev() {
        let _ = fs::remove_dir(at(old, rel));
    }
    for rel in ["memory", ".puppet-master"] {
        let _ = fs::remove_dir(old.join(rel));
    }
    stayed
}

impl Service {
    /// Moves a companion with a broad home to a folder of its own. `Err` leaves everything as it was.
    pub fn rehome(&self, id: &str) -> CompanionResult<CompanionView> {
        let now = self.host.now_ms();
        let mut inner = self.inner.lock();
        let index = inner
            .profiles
            .iter()
            .position(|p| p.id == id && !p.archived)
            .ok_or_else(|| CompanionError::new("COMPANION_NOT_FOUND", "No companion has that id."))?;
        let before = inner.profiles[index].clone();
        let places = self.host.places();
        let old = PathBuf::from(&before.folder);
        let problem = broad::problem_of(&old, &before.id, &places)
            .ok_or_else(|| fail(format!("{} already has a folder of its own.", before.name)))?;
        let new = PathBuf::from(&problem.suggested_folder);
        let created = prepare_new(&new)?;
        // The new place must itself be a folder of its own (a default folder inside a project would not be).
        if let Err(e) = broad::check_home(&new, &before.name, &before.id, &places) {
            undo_new(&new, created);
            return Err(e);
        }
        let instruction = self.instruction_files_of(&before);
        if let Err(e) = self.check_old_markers(&old, &instruction) {
            undo_new(&new, created);
            return Err(e);
        }

        // Nothing may write into the old folder while its files are copied: close the pane first. The lock is
        // held, so the keeper does not start it again until `restart_core` below.
        let awake = inner.slots.get(id).is_some_and(|slot| slot.keeper.want());
        self.stop_pane(&mut inner, &before);

        match self.rehome_steps(&mut inner, index, &before, &old, &new, &instruction) {
            Ok(copied) => {
                let stayed = drop_originals(&old, &new, &copied);
                if stayed > 0 {
                    tracing::warn!(companion = %before.id, stayed, "some originals stayed in the old folder");
                }
                if awake {
                    self.restart_core(&mut inner, index, now, Some("Moved to a folder of its own."));
                } else {
                    // A sleeping companion stays asleep; it starts in the new folder when it wakes.
                    self.feed(&inner.profiles[index].clone(), FeedKind::Restarted, now, "Moved to a folder of its own.");
                }
                Ok(self.finish(&mut inner, id, now))
            }
            Err(e) => {
                undo_new(&new, created);
                if awake {
                    self.restart_core(&mut inner, index, now, None);
                }
                self.publish_if_changed(&mut inner, now);
                Err(e)
            }
        }
    }

    /// Steps 3 to 6. On an error here the old brief is back and the profile is unchanged; the caller empties the new folder.
    fn rehome_steps(
        &self,
        inner: &mut Inner,
        index: usize,
        before: &CompanionProfile,
        old: &Path,
        new: &Path,
        instruction: &[Vec<crate::tui::InstructionFile>],
    ) -> CompanionResult<Copied> {
        let copied = copy_home(old, new)?;
        let mut next = before.clone();
        next.folder = new.display().to_string();
        files::ensure_home(&self.root, &next)?;
        self.write_brief(&next).map_err(fail)?;

        let cut = cut_blocks(old, instruction);
        let cut = match cut {
            Ok(cut) => cut,
            Err(e) => {
                // A half-done cut is put back by writing the old brief again.
                let _ = self.write_brief(before);
                return Err(e);
            }
        };

        inner.profiles[index] = next;
        if let Err(e) = store::save(&self.root, &inner.profiles) {
            inner.profiles[index] = before.clone();
            if cut {
                let _ = self.write_brief(before);
            }
            return Err(e);
        }
        Ok(copied)
    }

    /// The declared instruction files of the agent now and of the agent it moved from (the brief may sit in either).
    fn instruction_files_of(&self, profile: &CompanionProfile) -> Vec<Vec<crate::tui::InstructionFile>> {
        let mut lists = vec![self.host.instruction_files(&profile.agent_type)];
        if let Some(previous) = &profile.previous_agent {
            let list = self.host.instruction_files(&previous.agent_type);
            if brief::target(&list).0 != brief::target(&lists[0]).0 {
                lists.push(list);
            }
        }
        lists
    }

    /// Refuses before anything moves when a companion marker in the old instruction file is damaged.
    fn check_old_markers(&self, old: &Path, instruction: &[Vec<crate::tui::InstructionFile>]) -> CompanionResult<()> {
        for list in instruction {
            let (rel, _) = brief::target(list);
            if let Ok(bytes) = fs::read(old.join(&rel)) {
                if crate::one_brain::block_state_between(&bytes, brief::COMPANION_START, brief::COMPANION_END)
                    == crate::one_brain::BlockState::Malformed
                {
                    return Err(CompanionError::new(
                        "COMPANION_MARKER_DAMAGED",
                        format!("The companion markers in {rel} are damaged. Fix or delete them, then try again."),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Closes the companion's pane the keeper's way (not read as "the person closed it").
    fn stop_pane(&self, inner: &mut Inner, profile: &CompanionProfile) {
        let pane_id = super::service::pane_id_of(&profile.id);
        if let Some(slot) = inner.slots.get_mut(&profile.id) {
            slot.keeper.note_own_kill();
        }
        self.host.kill(&pane_id);
        self.host.release_reader(&pane_id, &profile.id);
    }
}

/// Cuts the companion block out of each old instruction file. True when any file changed.
fn cut_blocks(old: &Path, instruction: &[Vec<crate::tui::InstructionFile>]) -> CompanionResult<bool> {
    let mut changed = false;
    for list in instruction {
        changed |= brief::remove_block(old, list)? != Removed::NoBlock;
    }
    Ok(changed)
}
