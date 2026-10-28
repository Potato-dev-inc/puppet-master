//! What a person does with a companion: make one, change one, wake, sleep, restart, archive, edit its
//! memory, keep or drop what it proposes, set its check-ins. Every call is one of the Tauri commands or
//! a bridge route (`commands/companion.rs`, `bridge/routes/companions.rs`). The running side is in
//! `service.rs`.

use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize};

use super::model::{
    char_len, new_hex_id, unique_slug, valid_name, validate_profile, CheckIn, CompanionError, CompanionProfile,
    CompanionResult, CompanionView, FeedItem, FeedKind, HhMm, MemoryFile, PendingItem, PendingKind, MAX_COMPANIONS,
};
use super::service::{pane_id_of, Inner, OutKind, Outgoing, Service, Slot};
use super::store::{self, Runtime};
use super::skills::{self, SkillFull, SkillRef};
use super::{agents, brief, files, memory, today, view};

/// `null` and "missing" are different in a patch: `Some(None)` clears the value.
fn double_option<'de, T, D>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Ok(Some(Option::deserialize(d)?))
}

/// What the setup sheet sends to `companion_create`.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub name: String,
    pub avatar: String,
    pub persona: String,
    pub agent_type: String,
    #[serde(default)]
    pub launch_args: Vec<String>,
    #[serde(default)]
    pub account: Option<String>,
    /// Empty = the default folder.
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub auto_start: Option<bool>,
    #[serde(default)]
    pub memory_review: Option<bool>,
    #[serde(default)]
    pub push_replies: Option<bool>,
    #[serde(default)]
    pub share_today: Option<bool>,
    /// Missing = 04:00; `null` = no nap.
    #[serde(default, deserialize_with = "double_option")]
    pub nap_at: Option<Option<HhMm>>,
    #[serde(default)]
    pub checkins: Vec<CheckIn>,
}

/// What `companion_update` changes. A missing field stays as it is.
#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    pub name: Option<String>,
    pub avatar: Option<String>,
    pub persona: Option<String>,
    /// Another agent than the current one is a move (`moving.rs`): it waits until the companion is free.
    pub agent_type: Option<String>,
    /// With a move: when it goes back to the agent it left (epoch ms). `null` = it stays.
    #[serde(default, deserialize_with = "double_option")]
    pub until_ms: Option<Option<i64>>,
    pub launch_args: Option<Vec<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub account: Option<Option<String>>,
    pub folder: Option<String>,
    pub auto_start: Option<bool>,
    pub memory_review: Option<bool>,
    pub push_replies: Option<bool>,
    pub share_today: Option<bool>,
    #[serde(default, deserialize_with = "double_option")]
    pub nap_at: Option<Option<HhMm>>,
    pub checkins: Option<Vec<CheckIn>>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct MemoryCaps {
    pub soul: usize,
    pub user: usize,
    pub memory: usize,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct OutsideChange {
    pub file: MemoryFile,
}

/// The memory panel (desktop) and the phone's memory sheet.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MemoryView {
    pub soul: String,
    pub user: String,
    pub memory: String,
    pub caps: MemoryCaps,
    pub pending: Vec<PendingItem>,
    /// Desktop only: the phone gets an empty list.
    pub outside_changes: Vec<OutsideChange>,
    /// The approved skills, titles only (a body is opened with `skill_get`, desktop only).
    pub skills: Vec<SkillRef>,
}

fn not_found() -> CompanionError {
    CompanionError::new("COMPANION_NOT_FOUND", "No companion has that id.")
}

fn index_of(inner: &Inner, id: &str) -> CompanionResult<usize> {
    inner.profiles.iter().position(|p| p.id == id && !p.archived).ok_or_else(not_found)
}

impl Service {
    // ---- make and change --------------------------------------------------------------------------

    pub fn create(&self, draft: Draft) -> CompanionResult<CompanionView> {
        let now = self.host.now_ms();
        let mut inner = self.inner.lock();
        if inner.profiles.iter().filter(|p| !p.archived).count() >= MAX_COMPANIONS {
            return Err(CompanionError::new(
                "COMPANION_LIMIT",
                format!("You can have at most {MAX_COMPANIONS} companions. Archive one first."),
            ));
        }
        if !valid_name(&draft.name) {
            return Err(CompanionError::new("COMPANION_NAME_INVALID", "The name must be 1 to 24 letters, with no extra spaces."));
        }
        let taken: Vec<&str> = inner.profiles.iter().map(|p| p.id.as_str()).collect();
        let id = unique_slug(&draft.name, &taken);
        let folder = if draft.folder.trim().is_empty() {
            let home = files::user_home()
                .ok_or_else(|| CompanionError::new("COMPANION_FOLDER_INVALID", "Pick a folder for it to live in."))?;
            files::default_folder(&home, &id).display().to_string()
        } else {
            draft.folder.trim().to_string()
        };
        let mut checkins = draft.checkins;
        for checkin in checkins.iter_mut().filter(|c| c.id.is_empty()) {
            checkin.id = new_hex_id(8);
        }
        let profile = CompanionProfile {
            id: id.clone(),
            name: draft.name,
            avatar: draft.avatar,
            persona: draft.persona,
            agent_type: draft.agent_type,
            launch_args: draft.launch_args,
            account: draft.account.filter(|a| !a.trim().is_empty()),
            folder,
            auto_start: draft.auto_start.unwrap_or(true),
            memory_review: draft.memory_review.unwrap_or(true),
            push_replies: draft.push_replies.unwrap_or(true),
            share_today: draft.share_today.unwrap_or(false),
            nap_at: draft.nap_at.unwrap_or(Some(HhMm { h: 4, m: 0 })),
            checkins,
            previous_agent: None,
            until_ms: None,
            created_ms: now,
            archived: false,
            pane_ids: vec![pane_id_of(&id)],
        };
        validate_profile(&profile)?;
        self.check_agent(&profile)?;
        files::ensure_home(&self.root, &profile)?;

        inner.profiles.push(profile.clone());
        if let Err(e) = store::save(&self.root, &inner.profiles) {
            inner.profiles.pop();
            return Err(e);
        }
        let label = self.host.agent_label(&profile.agent_type);
        inner.slots.insert(id.clone(), Slot::new(&profile, Runtime::default(), label));
        let index = inner.profiles.len() - 1;
        self.tick_one(&mut inner, index, now);
        Ok(self.finish(&mut inner, &id, now))
    }

    pub fn update(&self, id: &str, patch: Patch) -> CompanionResult<CompanionView> {
        let now = self.host.now_ms();
        let mut inner = self.inner.lock();
        let index = index_of(&inner, id)?;
        let before = inner.profiles[index].clone();
        // A new agent is a move, not an edit: checked first, run last (it may wait until the companion is free).
        let move_ask = self.check_move(&before, patch.agent_type.as_deref(), patch.until_ms, now)?;
        let mut next = before.clone();
        if let Some(name) = patch.name {
            next.name = name;
        }
        if let Some(avatar) = patch.avatar {
            next.avatar = avatar;
        }
        if let Some(persona) = patch.persona {
            next.persona = persona;
        }
        if let Some(args) = patch.launch_args {
            next.launch_args = args;
        }
        if let Some(account) = patch.account {
            next.account = account.filter(|a| !a.trim().is_empty());
        }
        if let Some(folder) = patch.folder {
            next.folder = folder.trim().to_string();
        }
        if let Some(auto) = patch.auto_start {
            next.auto_start = auto;
        }
        if let Some(review) = patch.memory_review {
            next.memory_review = review;
        }
        if let Some(push) = patch.push_replies {
            next.push_replies = push;
        }
        if let Some(share) = patch.share_today {
            next.share_today = share;
        }
        if let Some(nap) = patch.nap_at {
            next.nap_at = nap;
        }
        if let Some(mut checkins) = patch.checkins {
            for checkin in checkins.iter_mut().filter(|c| c.id.is_empty()) {
                checkin.id = new_hex_id(8);
            }
            next.checkins = checkins;
        }
        validate_profile(&next)?;
        let launch_changed = next.launch_args != before.launch_args
            || next.account != before.account
            || next.folder != before.folder;
        if launch_changed {
            self.check_agent(&next)?;
        }
        if next.folder != before.folder {
            files::ensure_home(&self.root, &next)?;
        }
        inner.profiles[index] = next.clone();
        if let Err(e) = store::save(&self.root, &inner.profiles) {
            inner.profiles[index] = before;
            return Err(e);
        }
        let label = self.host.agent_label(&next.agent_type);
        if let Some(slot) = inner.slots.get_mut(id) {
            slot.agent_label = label;
            if schedule_anchors(slot, &next, now) {
                self.save_runtime(id, &slot.runtime);
            }
        }
        if launch_changed {
            self.restart_locked(&mut inner, index, now);
        } else if next.share_today != before.share_today {
            self.share_today_changed(&mut inner, index, now);
        }
        if let Some(ask) = move_ask {
            self.request_move(&mut inner, index, now, ask)?;
        }
        Ok(self.finish(&mut inner, id, now))
    }

    /// The day view was turned on or off. The brief is rewritten so the next start reads the right rule;
    /// a running agent is told once through the outbox. Turned off, the day list is moved out of the
    /// companion's folder (`archive_today`), so the agent cannot read it any more; nothing is deleted.
    fn share_today_changed(&self, inner: &mut Inner, index: usize, now: i64) {
        let profile = inner.profiles[index].clone();
        if let Err(reason) = self.write_brief(&profile) {
            tracing::warn!(companion = %profile.id, %reason, "could not rewrite the brief after the day view changed");
        }
        let moved = !profile.share_today && self.archive_today(&profile, now);
        let Some(slot) = inner.slots.get_mut(&profile.id) else { return };
        slot.outgoing.retain(|o| o.kind != OutKind::Notice);
        if profile.share_today {
            slot.outgoing.push(Outgoing { kind: OutKind::Notice, text: brief::TODAY_NOTICE.to_string(), queued_ms: now });
        } else if moved {
            slot.outgoing.push(Outgoing { kind: OutKind::Notice, text: brief::TODAY_GONE_NOTICE.to_string(), queued_ms: now });
        }
    }

    /// Moves `today.md` and `days/` out of the companion's home folder into its archive under the app
    /// data (`companions/<id>/archive/<stamp>/`). True when something was moved.
    fn archive_today(&self, profile: &CompanionProfile, now: i64) -> bool {
        let Ok(dir) = store::companion_dir(&self.root, &profile.id) else { return false };
        let to = dir.join(today::ARCHIVE_DIR).join(today::archive_name(now));
        match today::move_out(Path::new(&profile.folder), &to) {
            Ok(moved) => moved,
            Err(e) => {
                tracing::warn!(companion = %profile.id, error = %e, "could not move the day list out of the companion folder");
                false
            }
        }
    }

    /// Sleeps it, closes its pane and hides it. The home folder is never deleted.
    pub fn archive(&self, id: &str) -> CompanionResult<()> {
        let now = self.host.now_ms();
        let mut inner = self.inner.lock();
        let index = index_of(&inner, id)?;
        let profile = inner.profiles[index].clone();
        let pane_id = pane_id_of(id);
        if let Some(slot) = inner.slots.get_mut(id) {
            slot.keeper.sleep();
            slot.keeper.note_own_kill();
            slot.runtime.sleeping = true;
            slot.outgoing.clear();
            self.save_runtime(id, &slot.runtime);
        }
        self.host.kill(&pane_id);
        self.host.release_reader(&pane_id, id);
        self.host.clear_stopped(id);
        self.feed(&profile, FeedKind::Slept, now, "Archived. Its folder was kept.");
        inner.profiles[index].archived = true;
        store::save(&self.root, &inner.profiles)?;
        self.publish_if_changed(&mut inner, now);
        Ok(())
    }

    // ---- sleep, wake, restart ---------------------------------------------------------------------

    pub fn wake(&self, id: &str) -> CompanionResult<CompanionView> {
        let now = self.host.now_ms();
        let mut inner = self.inner.lock();
        let index = index_of(&inner, id)?;
        if let Some(slot) = inner.slots.get_mut(id) {
            slot.runtime.sleeping = false;
            slot.keeper.wake();
            self.save_runtime(id, &slot.runtime);
        }
        self.host.clear_stopped(id);
        self.tick_one(&mut inner, index, now);
        Ok(self.finish(&mut inner, id, now))
    }

    pub fn sleep(&self, id: &str) -> CompanionResult<CompanionView> {
        let now = self.host.now_ms();
        let mut inner = self.inner.lock();
        let index = index_of(&inner, id)?;
        let profile = inner.profiles[index].clone();
        if let Some(slot) = inner.slots.get_mut(id) {
            slot.runtime.sleeping = true;
            slot.keeper.sleep();
            slot.outgoing.clear();
            self.save_runtime(id, &slot.runtime);
        }
        self.host.clear_stopped(id);
        self.feed(&profile, FeedKind::Slept, now, "Went to sleep.");
        self.tick_one(&mut inner, index, now);
        Ok(self.finish(&mut inner, id, now))
    }

    /// Closes the pane (the keeper's own kill), forgets the crashes and starts it again. Also what
    /// `Try again` on the stopped card does.
    pub fn restart(&self, id: &str) -> CompanionResult<CompanionView> {
        let now = self.host.now_ms();
        let mut inner = self.inner.lock();
        let index = index_of(&inner, id)?;
        self.restart_locked(&mut inner, index, now);
        Ok(self.finish(&mut inner, id, now))
    }

    fn restart_locked(&self, inner: &mut Inner, index: usize, now: i64) {
        self.restart_core(inner, index, now, Some("Restarted."));
    }

    /// Closes the pane the keeper's way and starts it again at once. `note`: the feed line, if any.
    pub(super) fn restart_core(&self, inner: &mut Inner, index: usize, now: i64, note: Option<&str>) {
        let profile = inner.profiles[index].clone();
        let pane_id = pane_id_of(&profile.id);
        if let Some(slot) = inner.slots.get_mut(&profile.id) {
            slot.runtime.sleeping = false;
            slot.keeper.retry();
            slot.quiet_start = true;
            slot.outgoing.retain(|o| o.kind != OutKind::Primer);
            slot.reply_wait = None;
            self.save_runtime(&profile.id, &slot.runtime);
        }
        self.host.kill(&pane_id);
        self.host.release_reader(&pane_id, &profile.id);
        self.host.clear_stopped(&profile.id);
        if let Some(note) = note {
            self.feed(&profile, FeedKind::Restarted, now, note);
        }
        self.tick_one(inner, index, now);
    }

    // ---- reading ----------------------------------------------------------------------------------

    /// Every companion that is not archived. `phone`: the copy without folders.
    pub fn views(&self, phone: bool) -> Vec<CompanionView> {
        let inner = self.inner.lock();
        let views = self.all_views(&inner, self.host.now_ms());
        if phone { views.iter().map(view::phone_copy).collect() } else { views }
    }

    pub fn view(&self, id: &str, phone: bool) -> CompanionResult<CompanionView> {
        let inner = self.inner.lock();
        let index = index_of(&inner, id)?;
        let full = self.view_of(&inner, index, self.host.now_ms());
        Ok(if phone { view::phone_copy(&full) } else { full })
    }

    /// The home folder (desktop only: for `Open folder`).
    pub fn folder_of(&self, id: &str) -> CompanionResult<String> {
        let inner = self.inner.lock();
        Ok(inner.profiles[index_of(&inner, id)?].folder.clone())
    }

    /// Newest first, at most 50, only lines after `since_ms`.
    pub fn feed_list(&self, id: &str, since_ms: i64) -> CompanionResult<Vec<FeedItem>> {
        {
            let inner = self.inner.lock();
            index_of(&inner, id)?;
        }
        let mut items = store::feed_read(&self.root, id)?;
        items.retain(|item| item.at_ms > since_ms);
        items.reverse();
        items.truncate(50);
        Ok(items)
    }

    // ---- memory -----------------------------------------------------------------------------------

    pub fn memory_view(&self, id: &str, phone: bool) -> CompanionResult<MemoryView> {
        let inner = self.inner.lock();
        let index = index_of(&inner, id)?;
        self.memory_view_of(&inner.profiles[index], phone)
    }

    fn memory_view_of(&self, profile: &CompanionProfile, phone: bool) -> CompanionResult<MemoryView> {
        let home = Path::new(&profile.folder);
        let read = |file: MemoryFile| -> CompanionResult<String> {
            let text = store::read_optional(&files::file_path(home, file))?.unwrap_or_default();
            Ok(if phone { crate::redact::text(&text).into_inner() } else { text })
        };
        let outside = if phone {
            Vec::new()
        } else {
            memory::outside_changes(&self.root, &profile.id, home)?.into_iter().map(|file| OutsideChange { file }).collect()
        };
        let mut pending = memory::read_pending(home)?;
        if phone {
            for item in &mut pending {
                item.target_line = item.target_line.take().map(|line| crate::redact::text(&line).into_inner());
                // The phone sees that a skill is waiting and its title, never the steps.
                item.body = None;
            }
        }
        Ok(MemoryView {
            soul: read(MemoryFile::Soul)?,
            user: read(MemoryFile::User)?,
            memory: read(MemoryFile::Memory)?,
            caps: MemoryCaps { soul: MemoryFile::Soul.cap(), user: MemoryFile::User.cap(), memory: MemoryFile::Memory.cap() },
            pending,
            outside_changes: outside,
            skills: skills::list(&self.root, &profile.id, home)?,
        })
    }

    /// The desktop edit of one of the three files (the approved copy follows).
    pub fn memory_save(&self, id: &str, file: MemoryFile, text: &str) -> CompanionResult<MemoryView> {
        let now = self.host.now_ms();
        let mut inner = self.inner.lock();
        let index = index_of(&inner, id)?;
        let profile = inner.profiles[index].clone();
        memory::save_file(&self.root, id, Path::new(&profile.folder), file, text)?;
        self.after_memory_change(&mut inner, id, now);
        self.memory_view_of(&profile, false)
    }

    /// Keep or drop one proposal.
    pub fn pending_decide(&self, id: &str, item_id: &str, keep: bool, phone: bool) -> CompanionResult<MemoryView> {
        let now = self.host.now_ms();
        let mut inner = self.inner.lock();
        let index = index_of(&inner, id)?;
        let profile = inner.profiles[index].clone();
        let decision = if keep { memory::Decision::Keep } else { memory::Decision::Drop };
        let home = Path::new(&profile.folder);
        let skill_title = memory::read_pending(home)?
            .into_iter()
            .find(|i| i.id == item_id && i.kind == PendingKind::Skill)
            .map(|i| i.text);
        if phone && skill_title.is_some() {
            return Err(CompanionError::new("COMPANION_SKILL_ON_COMPUTER", "Skills are approved on the computer, where you can read all of the steps."));
        }
        memory::decide(&self.root, id, home, item_id, decision)?;
        if keep {
            match skill_title {
                Some(title) => self.feed(&profile, FeedKind::Memory, now, &format!("Learned a skill: {title}. It will list it at its next start.")),
                None => self.feed(&profile, FeedKind::Memory, now, "Kept a note. It will read it at its next start."),
            }
        }
        self.after_memory_change(&mut inner, id, now);
        self.memory_view_of(&profile, phone)
    }

    /// Approve a proposed skill, with the person's own title and steps when they edited it (desktop only).
    pub fn skill_approve(&self, id: &str, item_id: &str, title: Option<&str>, body: Option<&str>) -> CompanionResult<MemoryView> {
        let now = self.host.now_ms();
        let mut inner = self.inner.lock();
        let index = index_of(&inner, id)?;
        let profile = inner.profiles[index].clone();
        let skill = memory::approve_skill(&self.root, id, Path::new(&profile.folder), item_id, title, body)?;
        self.feed(&profile, FeedKind::Memory, now, &format!("Learned a skill: {}. It will list it at its next start.", skill.title));
        self.after_memory_change(&mut inner, id, now);
        self.memory_view_of(&profile, false)
    }

    /// One skill opened for reading or editing (desktop only).
    pub fn skill_get(&self, id: &str, slug: &str) -> CompanionResult<SkillFull> {
        let inner = self.inner.lock();
        let index = index_of(&inner, id)?;
        skills::get(&self.root, id, Path::new(&inner.profiles[index].folder), slug)
    }

    /// The desktop edit of a skill. Saving a skill that changed outside Puppet Master is how the person approves it.
    pub fn skill_save(&self, id: &str, slug: &str, title: &str, body: &str) -> CompanionResult<MemoryView> {
        let now = self.host.now_ms();
        let mut inner = self.inner.lock();
        let index = index_of(&inner, id)?;
        let profile = inner.profiles[index].clone();
        skills::save(&self.root, id, Path::new(&profile.folder), slug, title, body)?;
        self.after_memory_change(&mut inner, id, now);
        self.memory_view_of(&profile, false)
    }

    /// Removes a skill: the file moves to `skills/.removed/`.
    pub fn skill_remove(&self, id: &str, slug: &str) -> CompanionResult<MemoryView> {
        let now = self.host.now_ms();
        let mut inner = self.inner.lock();
        let index = index_of(&inner, id)?;
        let profile = inner.profiles[index].clone();
        skills::remove(Path::new(&profile.folder), slug)?;
        self.after_memory_change(&mut inner, id, now);
        self.memory_view_of(&profile, false)
    }

    /// `Keep the change` or `Undo` for a file that changed outside Puppet Master.
    pub fn outside_decide(&self, id: &str, file: MemoryFile, keep: bool) -> CompanionResult<MemoryView> {
        let now = self.host.now_ms();
        let mut inner = self.inner.lock();
        let index = index_of(&inner, id)?;
        let profile = inner.profiles[index].clone();
        let decision = if keep { memory::OutsideDecision::Keep } else { memory::OutsideDecision::Undo };
        memory::outside_change_decide(&self.root, id, Path::new(&profile.folder), file, decision)?;
        self.after_memory_change(&mut inner, id, now);
        self.memory_view_of(&profile, false)
    }

    fn after_memory_change(&self, inner: &mut Inner, id: &str, now: i64) {
        if let Some(slot) = inner.slots.get_mut(id) {
            slot.pending_checked_ms = i64::MIN / 2;
        }
        if let Ok(index) = index_of(inner, id) {
            // Read the new count now, so the sidebar number does not lag a few seconds.
            self.tick_one(inner, index, now);
        }
        self.publish_if_changed(inner, now);
    }

    /// `Ask to tidy up`: a message through the outbox. A sleeping companion is woken for it.
    pub fn tidy_up(&self, id: &str) -> CompanionResult<()> {
        let now = self.host.now_ms();
        let mut inner = self.inner.lock();
        let index = index_of(&inner, id)?;
        let profile = inner.profiles[index].clone();
        let used = char_len(&store::read_optional(&files::file_path(Path::new(&profile.folder), MemoryFile::Memory))?.unwrap_or_default());
        if let Some(slot) = inner.slots.get_mut(id) {
            if !slot.outgoing.iter().any(|o| o.kind == OutKind::Tidy) {
                slot.outgoing.push(Outgoing { kind: OutKind::Tidy, text: memory::tidy_up_message(used), queued_ms: now });
            }
            if !slot.keeper.want() && !slot.keeper.is_stopped() {
                slot.runtime.sleeping = false;
                slot.keeper.wake();
                self.save_runtime(id, &slot.runtime);
            }
        }
        self.tick_one(&mut inner, index, now);
        self.publish_if_changed(&mut inner, now);
        Ok(())
    }

    // ---- check-ins --------------------------------------------------------------------------------

    pub fn checkins_set(&self, id: &str, mut checkins: Vec<CheckIn>) -> CompanionResult<CompanionView> {
        let now = self.host.now_ms();
        let mut inner = self.inner.lock();
        let index = index_of(&inner, id)?;
        for checkin in checkins.iter_mut().filter(|c| c.id.is_empty()) {
            checkin.id = new_hex_id(8);
        }
        let before = inner.profiles[index].clone();
        let mut next = before.clone();
        next.checkins = checkins;
        validate_profile(&next)?;
        inner.profiles[index] = next.clone();
        if let Err(e) = store::save(&self.root, &inner.profiles) {
            inner.profiles[index] = before;
            return Err(e);
        }
        if let Some(slot) = inner.slots.get_mut(id) {
            if schedule_anchors(slot, &next, now) {
                self.save_runtime(id, &slot.runtime);
            }
        }
        Ok(self.finish(&mut inner, id, now))
    }

    // ---- shared -----------------------------------------------------------------------------------

    /// The agent can be used here: no bypass flags, the spawn gate and the chat rules pass.
    pub(super) fn check_agent(&self, profile: &CompanionProfile) -> CompanionResult<()> {
        agents::check_launch_args(&profile.launch_args)?;
        self.host
            .agent_ok(&profile.agent_type, &profile.folder)
            .map_err(|reason| CompanionError::new("COMPANION_AGENT_UNAVAILABLE", reason))
    }

    /// Tells the host if any view changed, and returns this companion's desktop view.
    fn finish(&self, inner: &mut Inner, id: &str, now: i64) -> CompanionView {
        self.publish_if_changed(inner, now);
        let index = inner.profiles.iter().position(|p| p.id == id).unwrap_or(0);
        self.view_of(inner, index, now)
    }
}

/// Gives new check-ins their starting point. True when `runtime` changed.
fn schedule_anchors(slot: &mut Slot, profile: &CompanionProfile, now: i64) -> bool {
    super::schedule::ensure_anchors(&mut slot.runtime.last_runs, &profile.checkins, now)
}
