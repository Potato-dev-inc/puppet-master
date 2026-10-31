//! Tag Your Agent: the person's actions on the PC. A group is approved only here.

use super::GroupUpdate;
use crate::audit::Kind as AuditKind;
use crate::channels::fake_bot::{rig, Rig};
use crate::channels::host::Host;
use crate::channels::tag_config::{GroupWho, PendingGroup, TagGroup, MAX_GROUPS};
use crate::channels::tag_log::GroupAnswerRecord;
use crate::channels::test_host::FakeHost;

const GROUP: i64 = -100_555_000_001;

/// Paired and on, the group bot off, and a group waiting for approval.
fn with_pending() -> Rig {
    let host = FakeHost::new();
    host.edit_config(|c| {
        c.tag_bot.pending.push(PendingGroup { chat_id: GROUP, title: "Test Crew".into(), seen_at_ms: 1 });
    });
    rig(host).with_token()
}

fn update(who: GroupWho) -> GroupUpdate {
    GroupUpdate { who, agent: None, folder: None, style: String::new() }
}

fn approved() -> Rig {
    let r = with_pending();
    r.rt.tag_approve(GROUP).unwrap();
    r
}

#[test]
fn the_view_starts_off_with_the_safe_caps_and_the_agents_the_host_offers() {
    let r = rig(FakeHost::new());
    let view = r.rt.tag_view();
    assert!(view.paired && !view.enabled);
    assert_eq!((view.daily_cap, view.per_person_cap), (30, 5));
    assert!(view.groups.is_empty() && view.pending.is_empty() && view.recent.is_empty());
    assert_eq!(view.agents.len(), 1);
    assert_eq!(view.bot_username.as_deref(), Some("anna_pm_bot"));
    let json = serde_json::to_value(&view).unwrap();
    for key in ["paired", "botUsername", "enabled", "dailyCap", "perPersonCap", "groups", "pending", "agents", "recent"] {
        assert!(json.get(key).is_some(), "missing {key}");
    }
}

#[test]
fn the_switch_needs_a_paired_chat_and_is_audited() {
    let r = rig(FakeHost::unpaired());
    assert!(r.rt.tag_set_enabled(true).is_err());
    assert!(!r.host.config().tag_bot.enabled);

    let r = rig(FakeHost::new());
    assert!(r.rt.tag_set_enabled(true).unwrap().enabled);
    assert!(r.host.config().tag_bot.enabled);
    assert!(r.audit_kinds().contains(&AuditKind::Setting));
    assert!(!r.rt.tag_set_enabled(false).unwrap().enabled);
}

#[test]
fn approving_moves_a_pending_group_in_with_the_safe_defaults() {
    let r = with_pending();
    let view = r.rt.tag_approve(GROUP).unwrap();
    assert!(view.pending.is_empty());
    assert_eq!(view.groups.len(), 1);
    let group = &view.groups[0];
    assert_eq!((group.chat_id, group.title.as_str(), group.who), (GROUP, "Test Crew", GroupWho::OnlyMe));
    assert!(group.agent.is_none() && group.folder.is_none() && group.style.is_empty());
    assert!(r.host.changed_times() >= 1);
    // Approving twice is harmless.
    assert_eq!(r.rt.tag_approve(GROUP).unwrap().groups.len(), 1);
}

#[test]
fn only_a_group_that_asked_can_be_approved() {
    let r = with_pending();
    assert!(r.rt.tag_approve(-100_555_000_099).is_err(), "a group that never tagged the bot cannot be approved by id");
    assert!(r.host.config().tag_bot.groups.is_empty());
}

#[test]
fn at_most_twenty_groups_can_be_approved() {
    let r = with_pending();
    r.host.edit_config(|c| {
        for i in 0..MAX_GROUPS as i64 {
            c.tag_bot.groups.push(TagGroup { chat_id: -200 - i, ..TagGroup::default() });
        }
    });
    assert!(r.rt.tag_approve(GROUP).unwrap_err().contains("20"));
}

#[test]
fn ignore_forgets_a_pending_group() {
    let r = with_pending();
    assert!(r.rt.tag_ignore(GROUP).unwrap().pending.is_empty());
    assert!(r.host.config().tag_bot.groups.is_empty());
}

#[test]
fn who_agent_and_style_are_saved_and_the_style_is_cut() {
    let r = approved();
    let view = r
        .rt
        .tag_update_group(
            GROUP,
            GroupUpdate { who: GroupWho::Anyone, agent: Some("test-agent".into()), folder: None, style: "y".repeat(500) },
        )
        .unwrap();
    assert_eq!(view.groups[0].who, GroupWho::Anyone);
    assert_eq!(view.groups[0].agent.as_deref(), Some("test-agent"));
    assert_eq!(view.groups[0].style.chars().count(), 200);
}

#[test]
fn an_agent_that_cannot_answer_is_refused() {
    let r = approved();
    let err = r
        .rt
        .tag_update_group(GROUP, GroupUpdate { agent: Some("not-a-real-agent".into()), ..update(GroupWho::OnlyMe) })
        .unwrap_err();
    assert!(err.contains("can't answer"), "{err}");
    assert!(r.host.config().tag_bot.groups[0].agent.is_none());
    // A blank agent means "any".
    r.rt.tag_update_group(GROUP, GroupUpdate { agent: Some("  ".into()), ..update(GroupWho::OnlyMe) }).unwrap();
}

#[test]
fn a_folder_must_exist_and_never_be_the_home_folder() {
    let r = approved();
    let missing = r.rt.tag_update_group(GROUP, GroupUpdate { folder: Some("D:\\no\\such\\folder".into()), ..update(GroupWho::OnlyMe) });
    assert!(missing.is_err());
    let home = crate::project_path::home_dir().expect("a home folder").to_string_lossy().into_owned();
    let refused = r.rt.tag_update_group(GROUP, GroupUpdate { folder: Some(home), ..update(GroupWho::OnlyMe) });
    assert!(refused.unwrap_err().contains("home folder"));
    let relative = r.rt.tag_update_group(GROUP, GroupUpdate { folder: Some("shop".into()), ..update(GroupWho::OnlyMe) });
    assert!(relative.is_err());
    assert!(r.host.config().tag_bot.groups[0].folder.is_none());
    // A real folder that is not the home folder is accepted (a fresh temp folder stands in for a project).
    let project = std::env::temp_dir().join(format!("pm-tag-folder-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&project).unwrap();
    let folder = project.to_string_lossy().into_owned();
    let home_path = crate::project_path::home_dir().unwrap();
    if !home_path.starts_with(&project) {
        let view = r.rt.tag_update_group(GROUP, GroupUpdate { folder: Some(folder.clone()), ..update(GroupWho::OnlyMe) }).unwrap();
        assert_eq!(view.groups[0].folder.as_deref(), Some(folder.as_str()));
    }
}

#[test]
fn anyone_together_with_a_folder_is_refused_with_a_plain_error() {
    let r = approved();
    let project = std::env::temp_dir().join(format!("pm-tag-anyone-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&project).unwrap();
    let folder = project.to_string_lossy().into_owned();
    let err = r
        .rt
        .tag_update_group(GROUP, GroupUpdate { folder: Some(folder), ..update(GroupWho::Anyone) })
        .unwrap_err();
    assert_eq!(err, "Only you can use a project folder. Groups open to anyone use an empty folder.");
    let saved = r.host.config().tag_bot.groups[0].clone();
    assert_eq!((saved.who, saved.folder), (GroupWho::OnlyMe, None), "nothing was saved");
    // Anyone with no folder is fine.
    assert_eq!(r.rt.tag_update_group(GROUP, update(GroupWho::Anyone)).unwrap().groups[0].who, GroupWho::Anyone);
}

#[test]
fn only_an_approved_group_can_be_changed() {
    let r = with_pending();
    assert!(r.rt.tag_update_group(GROUP, update(GroupWho::Anyone)).is_err());
}

#[test]
fn removing_a_group_takes_it_out() {
    let r = approved();
    assert!(r.rt.tag_remove(GROUP).unwrap().groups.is_empty());
    assert!(r.host.config().tag_bot.groups.is_empty());
}

#[test]
fn caps_are_saved_and_kept_in_range() {
    let r = rig(FakeHost::new());
    let view = r.rt.tag_set_caps(50, 3).unwrap();
    assert_eq!((view.daily_cap, view.per_person_cap), (50, 3));
    let view = r.rt.tag_set_caps(0, 9999).unwrap();
    assert_eq!(view.daily_cap, 1);
    assert!(view.per_person_cap >= 1 && view.per_person_cap < 9999, "{}", view.per_person_cap);
}

#[test]
fn the_view_lists_recent_answers_newest_first() {
    let r = rig(FakeHost::new());
    for n in 1..=3u64 {
        r.host.record_group_answer(&GroupAnswerRecord {
            at_ms: n,
            group_title: "Test Crew".into(),
            asker: "Sam".into(),
            question: format!("q{n}"),
            answer: Some("a".into()),
            agent: "Test Agent".into(),
            ok: true,
        });
    }
    let order: Vec<u64> = r.rt.tag_view().recent.iter().map(|rec| rec.at_ms).collect();
    assert_eq!(order, vec![3, 2, 1]);
}

#[test]
fn disconnecting_forgets_the_group_settings_too() {
    let r = approved();
    r.rt.tag_set_enabled(true).unwrap();
    r.rt.forget().unwrap();
    let cfg = r.host.config();
    assert!(!cfg.tag_bot.enabled && cfg.tag_bot.groups.is_empty() && cfg.tag_bot.pending.is_empty());
}
