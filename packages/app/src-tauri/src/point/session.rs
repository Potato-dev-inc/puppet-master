//! The one live Point session: picks, drafts and the state the Point box shows (PF-10).
//!
//! Pure state, no windows and no clock of its own: every call that cares about time takes `now_ms`. The
//! session holds up to `MAX_PICKS` picks waiting for a request. It ends `SESSION_TTL_MS` after the last
//! pick or removal. Shots of picks that are dropped are returned to the caller, which deletes them (a
//! sent message keeps its shots, because the agent was told to open them).

use super::target::Target;
use super::{Pick, Shot, SourceRef, MAX_PICKS, SESSION_TTL_MS};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

/// The Point box shows this when a fourth element pushes out the oldest one.
pub const REPLACED_NOTICE: &str = "Only 3 elements fit in one message. The oldest one was replaced.";

/// The Point box's note when the preview refused a link. Only the host is named.
pub fn blocked_notice(host: &str) -> String {
    format!("Blocked a link to {host}. The preview only shows your app.")
}

/// What the Point box gets (event `point://state`, command `point_state`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PointState {
    pub picks: Vec<PickView>,
    pub targets: Vec<Target>,
    pub default_target: Option<String>,
    pub notice: Option<String>,
}

/// One pick as the Point box shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickView {
    pub id: String,
    /// "localhost:5173/pricing".
    pub page: String,
    pub title: String,
    /// "button.card-cta".
    pub label: String,
    pub text: String,
    pub component: Option<String>,
    pub source: Option<SourceRef>,
    /// `data:image/png;base64,...`, only when the picture is at most 600 KB.
    pub thumb: Option<String>,
    pub shot_path: Option<String>,
    pub no_shot_reason: Option<String>,
}

/// What `add_pick` changed besides adding the pick.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Added {
    /// Shots of picks that went away (an expired session, or the oldest pick pushed out by a fourth).
    pub dropped_shots: Vec<PathBuf>,
}

/// The picks, drafts and notice of the one Point session.
#[derive(Debug, Default)]
pub struct Session {
    picks: Vec<Pick>,
    /// Text for a pane's Chat composer ("Edit in Chat"), taken once by that pane.
    drafts: HashMap<String, String>,
    notice: Option<String>,
    /// The last link the preview refused (http or https only), for "Open in browser".
    blocked_url: Option<String>,
    seq: u32,
    /// Time of the last pick or removal. 0 while the session has no picks.
    active_ms: u64,
    /// Thumbnails by pick id, filled the first time the state is built.
    thumbs: HashMap<String, Option<String>>,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// The next number for a pick id and a screenshot name. Never repeats within a run.
    pub fn next_seq(&mut self) -> u32 {
        self.seq += 1;
        self.seq
    }

    pub fn picks(&self) -> &[Pick] {
        &self.picks
    }

    pub fn is_empty(&self) -> bool {
        self.picks.is_empty()
    }

    pub fn find(&self, id: &str) -> Option<&Pick> {
        self.picks.iter().find(|p| p.id == id)
    }

    /// True when the picks have been waiting longer than `SESSION_TTL_MS`.
    pub fn is_expired(&self, now_ms: u64) -> bool {
        !self.picks.is_empty() && now_ms.saturating_sub(self.active_ms) > SESSION_TTL_MS
    }

    /// Ends an expired session. `Some(shots)` when picks were dropped (delete those files).
    pub fn expire(&mut self, now_ms: u64) -> Option<Vec<PathBuf>> {
        self.is_expired(now_ms).then(|| self.discard())
    }

    /// Adds a pick. An expired session is dropped first. A fourth pick pushes out the oldest one.
    pub fn add_pick(&mut self, pick: Pick, now_ms: u64) -> Added {
        let mut added = Added::default();
        if let Some(shots) = self.expire(now_ms) {
            added.dropped_shots = shots;
        }
        // A new pick clears the last note: a refused link or a replaced pick is old news.
        self.notice = None;
        self.blocked_url = None;
        while self.picks.len() >= MAX_PICKS {
            let oldest = self.picks.remove(0);
            self.thumbs.remove(&oldest.id);
            added.dropped_shots.extend(shot_path(&oldest));
            self.notice = Some(REPLACED_NOTICE.to_string());
        }
        self.picks.push(pick);
        self.active_ms = now_ms;
        added
    }

    /// Removes one pick. The caller deletes its shot.
    pub fn remove(&mut self, id: &str, now_ms: u64) -> Option<Pick> {
        let at = self.picks.iter().position(|p| p.id == id)?;
        let pick = self.picks.remove(at);
        self.thumbs.remove(&pick.id);
        self.active_ms = now_ms;
        Some(pick)
    }

    /// Ends the session and returns the shots of its picks to delete. Drafts stay (they were made
    /// from picks that were sent on already).
    pub fn discard(&mut self) -> Vec<PathBuf> {
        let shots = self.picks.iter().filter_map(shot_path).collect();
        self.discard_keep_files();
        shots
    }

    /// Ends the session but keeps the shot files: a sent message names them.
    pub fn discard_keep_files(&mut self) {
        self.picks.clear();
        self.thumbs.clear();
        self.notice = None;
        self.blocked_url = None;
        self.active_ms = 0;
    }

    /// Notes that the preview refused a link. Only http and https links can be opened in the
    /// browser later; any other scheme is shown in the notice by host but not kept.
    pub fn set_blocked(&mut self, host: &str, url: &str) {
        self.notice = Some(blocked_notice(host));
        self.blocked_url = is_web_url(url).then(|| url.to_string());
    }

    /// The last refused link that may be opened in the browser.
    pub fn blocked_url(&self) -> Option<&str> {
        self.blocked_url.as_deref()
    }

    /// Stores the text "Edit in Chat" made for a pane's composer. The newest one wins.
    pub fn put_draft(&mut self, pane_id: &str, text: String) {
        self.drafts.insert(pane_id.to_string(), text);
    }

    /// Hands the pane's draft over once.
    pub fn take_draft(&mut self, pane_id: &str) -> Option<String> {
        self.drafts.remove(pane_id)
    }

    /// The state for the Point box. An expired session shows no picks. Thumbnails are read from
    /// the shot files.
    pub fn state(&mut self, now_ms: u64, targets: Vec<Target>, default_target: Option<String>) -> PointState {
        self.state_with(now_ms, targets, default_target, &read_thumb)
    }

    /// Same as `state`, with the thumbnail loader given (a fake in tests).
    pub fn state_with(
        &mut self,
        now_ms: u64,
        targets: Vec<Target>,
        default_target: Option<String>,
        load_thumb: &dyn Fn(&Shot) -> Option<String>,
    ) -> PointState {
        let expired = self.is_expired(now_ms);
        let mut picks = Vec::new();
        if !expired {
            for pick in &self.picks {
                let thumb = match &pick.shot {
                    Some(shot) => self.thumbs.entry(pick.id.clone()).or_insert_with(|| load_thumb(shot)).clone(),
                    None => None,
                };
                picks.push(view_of(pick, thumb));
            }
        }
        PointState { picks, targets, default_target, notice: if expired { None } else { self.notice.clone() } }
    }
}

/// The one live session.
pub static SESSION: Lazy<Mutex<Session>> = Lazy::new(|| Mutex::new(Session::new()));

fn shot_path(pick: &Pick) -> Option<PathBuf> {
    pick.shot.as_ref().map(|s| s.abs_path.clone())
}

fn view_of(pick: &Pick, thumb: Option<String>) -> PickView {
    PickView {
        id: pick.id.clone(),
        page: pick.facts.page.clone(),
        title: pick.facts.title.clone(),
        label: pick.facts.label.clone(),
        text: pick.facts.text.clone(),
        component: pick.facts.component.clone(),
        source: pick.source.clone(),
        thumb,
        shot_path: pick.shot.as_ref().map(|s| s.rel_path.clone()),
        no_shot_reason: pick.no_shot_reason.map(str::to_string),
    }
}

fn read_thumb(shot: &Shot) -> Option<String> {
    let bytes = std::fs::read(&shot.abs_path).ok()?;
    super::shot::thumb_data_url(&bytes)
}

fn is_web_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://")) && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}
