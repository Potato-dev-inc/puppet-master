//! The `CompanionView` the desktop and the phone read (spec sections 2.1 and 11).
//!
//! The desktop copy (Tauri) carries the home folder. The phone copy (bridge routes and the `companion`
//! event) never does: it also has the folder's name removed from every sentence and goes through the
//! same redaction as the chat.

use super::model::{CompanionProfile, CompanionState, CompanionView, NeedsRef, NextCheckIn, QueuedMove};

/// What the service knows about a companion right now, besides its saved profile.
#[derive(Clone, Debug)]
pub struct Live {
    pub state: CompanionState,
    pub state_line: String,
    /// `companion-<id>` while a pane exists.
    pub pane_id: Option<String>,
    pub needs: Option<NeedsRef>,
    pub pending_memory: u32,
    pub next_checkin: Option<NextCheckIn>,
    pub restarts: u32,
    pub agent_label: String,
    /// The display name of the agent it moved from.
    pub previous_agent_label: Option<String>,
    pub queued_move: Option<QueuedMove>,
}

/// The last path segment of a folder (either slash), or the whole text when it has none.
pub fn folder_name(folder: &str) -> String {
    folder
        .trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(folder)
        .to_string()
}

/// The desktop view: everything, with the folder.
pub fn build(profile: &CompanionProfile, live: &Live) -> CompanionView {
    CompanionView {
        id: profile.id.clone(),
        name: profile.name.clone(),
        avatar: profile.avatar.clone(),
        persona: profile.persona.clone(),
        agent_type: profile.agent_type.clone(),
        agent_label: live.agent_label.clone(),
        folder: Some(profile.folder.clone()),
        folder_name: folder_name(&profile.folder),
        state: live.state,
        state_line: live.state_line.clone(),
        pane_id: live.pane_id.clone(),
        needs: live.needs.clone(),
        pending_memory: live.pending_memory,
        next_checkin: live.next_checkin.clone(),
        checkins: Some(profile.checkins.clone()),
        restarts: live.restarts,
        auto_start: profile.auto_start,
        push_replies: profile.push_replies,
        share_today: profile.share_today,
        previous_agent_label: live.previous_agent_label.clone(),
        previous_agent: profile.previous_agent.as_ref().map(|p| p.agent_type.clone()),
        until_ms: profile.until_ms,
        queued_move: live.queued_move.clone(),
    }
}

/// The copy that leaves this computer: no `folder`, and the folder's path never inside a sentence.
pub fn phone_copy(view: &CompanionView) -> CompanionView {
    let mut copy = view.clone();
    let folder = copy.folder.take();
    copy.checkins = None;
    let clean = |text: &str| scrub(text, folder.as_deref());
    copy.state_line = clean(&copy.state_line);
    copy.needs = copy.needs.map(|needs| NeedsRef { title: clean(&needs.title), ..needs });
    copy.next_checkin = copy.next_checkin.map(|next| NextCheckIn { label: clean(&next.label), ..next });
    copy
}

/// Redacts a sentence and takes the home folder out of it.
pub fn scrub(text: &str, folder: Option<&str>) -> String {
    let mut text = text.to_string();
    if let Some(folder) = folder.filter(|f| !f.trim().is_empty()) {
        text = text.replace(folder, "its folder");
        // The same path with the other slash, the way a terminal may print it.
        text = text.replace(&folder.replace('\\', "/"), "its folder");
    }
    crate::redact::text(&text).into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> CompanionProfile {
        CompanionProfile {
            id: "mochi".into(),
            name: "Mochi".into(),
            avatar: "mochi".into(),
            persona: "calm".into(),
            agent_type: "claude".into(),
            launch_args: vec![],
            account: None,
            folder: "C:\\Users\\you\\PuppetMaster\\companions\\mochi".into(),
            auto_start: true,
            memory_review: true,
            push_replies: true,
            share_today: false,
            nap_at: None,
            checkins: vec![],
            previous_agent: None,
            until_ms: None,
            created_ms: 1,
            archived: false,
        }
    }

    fn live() -> Live {
        Live {
            state: CompanionState::NeedsYou,
            state_line: "Needs you: Trust C:\\Users\\you\\PuppetMaster\\companions\\mochi?".into(),
            pane_id: Some("companion-mochi".into()),
            needs: Some(NeedsRef { item_id: "i1".into(), title: "Trust C:\\Users\\you\\PuppetMaster\\companions\\mochi?".into() }),
            pending_memory: 2,
            next_checkin: Some(NextCheckIn { id: "0a0a0a0a".into(), label: "Morning".into(), at_ms: 5 }),
            restarts: 1,
            agent_label: "Claude Code".into(),
            previous_agent_label: None,
            queued_move: None,
        }
    }

    #[test]
    fn the_desktop_view_has_the_folder_and_its_last_segment() {
        let view = build(&profile(), &live());
        assert_eq!(view.folder.as_deref(), Some("C:\\Users\\you\\PuppetMaster\\companions\\mochi"));
        assert_eq!(view.folder_name, "mochi");
        assert_eq!(view.checkins, Some(vec![]), "the desktop view carries the check-in list");
        assert_eq!(view.pane_id.as_deref(), Some("companion-mochi"));
        assert_eq!((view.pending_memory, view.restarts), (2, 1));
    }

    #[test]
    fn the_phone_view_has_no_folder_anywhere() {
        let phone = phone_copy(&build(&profile(), &live()));
        assert!(phone.folder.is_none());
        assert!(phone.checkins.is_none());
        let json = serde_json::to_string(&phone).unwrap();
        assert!(!json.contains("\"folder\""), "the folder key is not sent: {json}");
        assert!(!json.contains("C:\\\\Users"), "no folder path in any sentence: {json}");
        assert!(!json.contains("PuppetMaster"), "{json}");
        assert!(phone.state_line.contains("its folder"));
        assert_eq!(phone.needs.unwrap().title, "Trust its folder?");
        assert_eq!(phone.folder_name, "mochi", "the last segment alone may go");
    }

    #[test]
    fn the_phone_copy_goes_through_redaction() {
        let mut l = live();
        l.state_line = "Needs you: key sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into();
        let phone = phone_copy(&build(&profile(), &l));
        assert!(!phone.state_line.contains("AAAAAAAAAAAAAAAA"), "{}", phone.state_line);
    }

    #[test]
    fn folder_names_handle_both_slashes_and_trailing_ones() {
        assert_eq!(folder_name("C:\\a\\b\\"), "b");
        assert_eq!(folder_name("/home/you/c"), "c");
        assert_eq!(folder_name("solo"), "solo");
    }
}
