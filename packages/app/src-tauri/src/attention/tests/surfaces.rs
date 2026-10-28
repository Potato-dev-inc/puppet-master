//! One count everywhere. Before attention, the Inbox, the phone, the tray badge and the away digest
//! counted "needs you" four ways (5 / 5 / 3 / 3 on this very fixture). Now each of them reads the
//! store's one number.

use crate::ambient::glyph::Aggregate;
use crate::attention::model::*;
use crate::attention::store::{PromptRec, Store};

fn pane(id: &str, status: &str, agent: &str) -> PaneFact {
    PaneFact {
        id: id.into(),
        status: status.into(),
        heading: Some(agent.into()),
        agent_label: Some(agent.into()),
        project: Some("proj".into()),
    }
}

/// Two approvals, one errored shell, one running pane that looks stuck, one question, one Ask-bar
/// answer nobody saw, and one finished run.
fn mixed() -> (Store, Facts) {
    let mut store = Store::default();
    for id in ["ap1", "ap2"] {
        store.on_prompt(id, true, Some(PromptRec { prompt_id: Some(format!("p-{id}")), risk_level: Some("low".into()), ..PromptRec::default() }));
    }
    store.raise(
        Raised { key: "done:run".into(), kind: Kind::Done, pane_id: None, title: "Claude Code finished".into(), sentence: None, expires_at_ms: None, actions: vec![] },
        1_000,
    );
    let facts = Facts {
        panes: vec![
            pane("ap1", "waiting_input", "Claude Code"),
            pane("ap2", "waiting_input", "Codex"),
            pane("ps", "error", "PowerShell"),
            pane("run", "running", "Claude Code"),
            pane("idle", "idle", "Codex"),
        ],
        questions: vec![QuestionFact { id: "q1".into(), created_at_ms: Some(0), ..QuestionFact::default() }],
        stuck_pane_ids: vec!["run".into()],
        notices: vec![NoticeFact { id: "ask-1".into(), at_ms: Some(5), text: None }],
        ..Facts::default()
    };
    (store, facts)
}

#[test]
fn one_count_everywhere() {
    let (mut store, facts) = mixed();
    store.recompute(&facts, 10_000);
    let counts = store.counts();
    // 2 approvals + 1 error + 1 stuck + 1 question = 5. The Ask-bar answer and the finished run
    // are "Ready for you": they carry no number.
    assert_eq!(counts, Counts { blocking: 5, ready: 2 });

    // The tray badge and tooltip. Both read the same attention count.
    let statuses: Vec<&str> = facts.panes.iter().map(|p| p.status.as_str()).collect();
    let tray = crate::ambient::tray_icon::aggregate_for(statuses.iter().copied(), counts.blocking);
    assert_eq!(tray.attention(), counts.blocking, "tray badge");
    assert_eq!(
        crate::tray::summary_line(&statuses, &[], counts.blocking),
        "Puppet Master \u{b7} 5 need you \u{b7} 1 error \u{b7} 1 working",
        "tray tooltip"
    );

    // The away digest.
    let digest = crate::turn_summary::needs_you_from(store.snapshot());
    assert_eq!(digest.len() as u32, counts.blocking, "digest Needs you");

    // The phone's counter is the snapshot's number (`counts.blocking` over the wire).
    let wire = serde_json::to_value(store.snapshot()).unwrap();
    assert_eq!(wire["counts"]["blocking"], counts.blocking);
}

#[test]
fn the_old_status_count_would_have_said_three() {
    // What the tray and the digest used to say: waiting_input + error panes only.
    let (_, facts) = mixed();
    let by_status = Aggregate::from_statuses(facts.panes.iter().map(|p| p.status.as_str()));
    assert_eq!(by_status.attention(), 3);
}

#[test]
fn the_digest_lists_things_in_attention_order_and_names_what_has_no_pane() {
    let (mut store, facts) = mixed();
    store.recompute(&facts, 10_000);
    let lines = crate::turn_summary::needs_you_from(store.snapshot());
    assert!(lines.iter().all(|l| l.id.is_some()));
    let error = lines.iter().find(|l| l.status == "error").expect("the errored shell");
    assert_eq!(error.pane_id, "ps");
    assert_eq!(error.agent, "PowerShell");
    let question = lines.iter().find(|l| l.pane_id.is_empty()).expect("a question has no pane");
    assert_eq!(question.status, "waiting_input");
    assert_eq!(question.agent, "An agent");
}
