//! Setup tests: the person's actions on the PC and the phone's off switch.

use crate::audit::{Kind as AuditKind, Who};
use crate::channels::host::Host;
use crate::channels::fake_bot::{rig, text_from, wait_for, Rig, TOKEN, TOKEN_SECRET};
use crate::channels::test_host::{FakeHost, PAIRED_ID, T0};
use crate::channels::telegram::api::ApiError;
use crate::secrets::{MemoryBackend, SecretBackend};
use std::sync::atomic::Ordering;

/// Nobody paired yet, no token.
fn fresh() -> Rig {
    rig(FakeHost::unpaired())
}

/// Paired, switched off, token in the store.
fn paired_off() -> Rig {
    let host = FakeHost::new();
    host.edit_config(|c| {
        c.enabled = false;
        c.enabled_at_ms = None;
    });
    rig(host).with_token()
}

fn stored(r: &Rig) -> Option<String> {
    r.secrets.get(&crate::channels::token::key_name_for(None)).unwrap()
}

#[test]
fn a_good_token_is_checked_with_the_bot_and_kept_in_the_store_only() {
    let r = fresh();
    let status = r.rt.set_token(&format!("  {TOKEN}\n")).unwrap();
    assert_eq!(status.bot_username.as_deref(), Some("anna_pm_bot"));
    assert!(status.token_set);
    assert_eq!(stored(&r).as_deref(), Some(TOKEN));
    assert_eq!(r.host.config().bot_id, Some(777));
    let json = serde_json::to_string(&status).unwrap();
    assert!(!json.contains(TOKEN_SECRET), "the page never gets the token back: {json}");
    assert!(!r.rt.alive(), "checking a token starts no poller");
}

#[test]
fn a_token_with_the_wrong_shape_is_refused_before_any_web_call() {
    let r = fresh();
    let err = r.rt.set_token("not a token").unwrap_err();
    assert_eq!(err, crate::channels::token::BAD_FORMAT);
    assert_eq!(r.bot.me_calls.load(Ordering::SeqCst), 0);
    assert_eq!(stored(&r), None);
}

#[test]
fn no_credential_store_means_no_telegram() {
    let r = fresh();
    // Rebuild the runtime over an unusable store.
    let host = std::sync::Arc::new(FakeHost::unpaired());
    let deps = crate::channels::runtime::Deps {
        host,
        secrets: std::sync::Arc::new(MemoryBackend::unavailable()),
        profile: None,
        api_for: std::sync::Arc::new({
            let bot = r.bot.clone();
            move |_t: &str| bot.clone() as std::sync::Arc<dyn crate::channels::telegram::api::Api>
        }),
        timing: crate::channels::fake_bot::FAST,
        audit: std::sync::Arc::new(|_, _, _, _| {}),
        turns: std::sync::Arc::new(|_| Vec::new()),
    };
    let rt = crate::channels::runtime::Runtime::new(deps);
    assert_eq!(rt.set_token(TOKEN).unwrap_err(), crate::channels::token::NO_STORE);
    assert!(!rt.status().store_available);
    assert_eq!(r.bot.me_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn a_token_telegram_does_not_accept_is_not_kept() {
    let r = fresh();
    *r.bot.me.lock() = Err(ApiError::Unauthorized);
    let err = r.rt.set_token(TOKEN).unwrap_err();
    assert_eq!(err, "Telegram did not accept this token.");
    assert_eq!(stored(&r), None);
}

#[test]
fn a_network_failure_is_a_plain_sentence_without_the_token() {
    let r = fresh();
    *r.bot.me.lock() = Err(ApiError::Network);
    let err = r.rt.set_token(TOKEN).unwrap_err();
    assert!(err.contains("Could not reach Telegram"));
    assert!(!err.contains(TOKEN_SECRET));
    assert_eq!(stored(&r), None);
}

#[test]
fn a_bot_with_a_webhook_is_refused_and_the_webhook_is_left_alone() {
    let r = fresh();
    *r.bot.webhook.lock() = Some("https://example.com/hook".into());
    let err = r.rt.set_token(TOKEN).unwrap_err();
    assert!(err.starts_with("This bot already sends its messages to a website"));
    assert!(!err.contains("example.com"));
    assert_eq!(stored(&r), None);
}

#[test]
fn a_token_of_another_bot_forgets_the_paired_chat() {
    let r = paired_off();
    r.host.edit_config(|c| c.bot_id = Some(555));
    r.rt.set_token(TOKEN).unwrap();
    let cfg = r.host.config();
    assert_eq!(cfg.paired, None, "a chat paired to another bot is gone");
    assert_eq!(cfg.bot_id, Some(777));
    assert!(!cfg.enabled);
    assert!(r.audit_kinds().contains(&AuditKind::ChannelUnpaired));
}

#[test]
fn the_same_bot_again_keeps_the_pairing() {
    let r = paired_off();
    r.rt.set_token(TOKEN).unwrap();
    assert!(r.host.config().paired.is_some());
}

#[test]
fn making_a_code_needs_a_checked_token() {
    let r = fresh();
    assert!(r.rt.pair_start().is_err());
    r.rt.set_token(TOKEN).unwrap();
    let status = r.rt.pair_start().unwrap();
    let pairing = status.pairing.expect("a code is shown");
    assert_eq!(pairing.code.len(), 8);
    assert!(pairing.waiting_for.is_none());
    assert!(r.rt.alive(), "the poller reads /start in pairing mode");
    r.rt.stop();
}

#[test]
fn a_paired_install_cannot_make_a_second_code() {
    let r = paired_off();
    assert!(r.rt.pair_start().unwrap_err().contains("Disconnect"));
}

#[test]
fn pairing_needs_the_code_and_the_click_and_leaves_the_channel_off() {
    let r = fresh();
    r.rt.set_token(TOKEN).unwrap();
    let code = r.rt.pair_start().unwrap().pairing.unwrap().code;
    r.bot.push_updates(vec![text_from(PAIRED_ID, 1, &format!("/start {code}"))]);
    wait_for("the PC to be asked", || r.rt.status().pairing.is_some_and(|p| p.waiting_for.is_some()));
    assert!(r.host.config().paired.is_none(), "the code alone pairs nobody");

    let status = r.rt.pair_decide(true).unwrap();
    let paired = status.paired.expect("paired after the click");
    assert_eq!(paired.first_name.as_deref(), Some("Anna"));
    assert!(!status.enabled, "off until the person turns it on (D-5)");
    assert!(status.pairing.is_none());
    assert!(!r.rt.alive(), "the pairing poller ends");
    assert!(r.bot.texts().iter().any(|t| t.starts_with("Connected.")), "{:?}", r.bot.texts());
    assert_eq!(r.bot.menus.load(Ordering::SeqCst), 1, "the command menu is set after pairing");
    assert!(r.host.audit_kinds().contains(&AuditKind::ChannelPaired));
}

#[test]
fn not_me_pairs_nobody_and_sets_no_menu() {
    let r = fresh();
    r.rt.set_token(TOKEN).unwrap();
    let code = r.rt.pair_start().unwrap().pairing.unwrap().code;
    r.bot.push_updates(vec![text_from(PAIRED_ID, 1, &format!("/start {code}"))]);
    wait_for("the PC to be asked", || r.rt.status().pairing.is_some_and(|p| p.waiting_for.is_some()));
    let status = r.rt.pair_decide(false).unwrap();
    assert!(status.paired.is_none());
    assert!(status.pairing.is_none());
    assert_eq!(r.bot.menus.load(Ordering::SeqCst), 0);
    assert!(!r.bot.texts().iter().any(|t| t.starts_with("Connected.")));
}

#[test]
fn turning_on_needs_a_paired_chat_and_a_token() {
    let r = fresh().with_token();
    assert!(r.rt.set_enabled(true).is_err());
    let no_token = rig(FakeHost::new());
    assert!(no_token.rt.set_enabled(true).is_err());
}

#[test]
fn turning_on_saves_the_time_audits_and_starts_the_poller() {
    let r = paired_off();
    r.host.set_now(T0 + 7_000);
    let status = r.rt.set_enabled(true).unwrap();
    assert!(status.enabled);
    assert_eq!(r.host.config().enabled_at_ms, Some(T0 + 7_000), "older messages are never acted on");
    wait_for("the poller", || r.rt.listening());
    let audits = r.audits.lock().clone();
    assert!(audits.iter().any(|(k, who, _)| *k == AuditKind::ChannelOn && *who == Who::Desktop));
    r.rt.stop();
}

#[test]
fn turning_off_stops_the_poller_and_audits() {
    let r = paired_off();
    r.rt.set_enabled(true).unwrap();
    wait_for("the poller", || r.rt.listening());
    let status = r.rt.set_enabled(false).unwrap();
    assert!(!status.enabled);
    assert_eq!(status.state, "off");
    assert!(!r.rt.listening());
    assert!(r.audit_kinds().contains(&AuditKind::ChannelOff));
}

#[test]
fn control_can_never_be_given_to_the_chat() {
    let r = paired_off();
    let err = r.rt.set_options(&["watch".into(), "control".into()], false, false).unwrap_err();
    assert_eq!(err, super::CONTROL_NOT_ALLOWED);
    assert!(r.rt.set_options(&["fly".into()], false, false).is_err(), "unknown names are refused too");
    assert!(!r.host.config().caps.has(crate::mobile_pairing::Cap::Control));
}

#[test]
fn options_are_saved() {
    let r = paired_off();
    let status = r.rt.set_options(&["watch".into(), "type".into()], true, true).unwrap();
    assert_eq!(status.caps, vec!["watch", "type"]);
    assert!(status.notify_runs && status.short_mode);
    let cfg = r.host.config();
    assert!(cfg.notify_runs && cfg.short_mode);
}

#[test]
fn disconnect_forgets_the_token_the_chat_and_the_poller() {
    let r = paired_off();
    r.rt.set_enabled(true).unwrap();
    wait_for("the poller", || r.rt.listening());
    let status = r.rt.forget().unwrap();
    assert_eq!(stored(&r), None, "the token is deleted from the store");
    assert!(!status.token_set && status.paired.is_none() && !status.enabled);
    assert_eq!(status.bot_username, None);
    assert!(!r.rt.alive());
    assert!(r.audit_kinds().contains(&AuditKind::ChannelUnpaired));
}

#[test]
fn disconnect_also_drops_a_chat_that_was_waiting_for_the_click() {
    let r = fresh();
    r.rt.set_token(TOKEN).unwrap();
    let code = r.rt.pair_start().unwrap().pairing.unwrap().code;
    r.bot.push_updates(vec![text_from(PAIRED_ID, 1, &format!("/start {code}"))]);
    wait_for("the PC to be asked", || r.rt.engine.pairing_waiting().is_some());
    r.rt.forget().unwrap();
    assert!(r.rt.engine.pairing_waiting().is_none(), "an old waiting chat can never be paired later");
}

// ---- the phone's off switch ------------------------------------------------------------------

#[test]
fn the_phone_can_switch_it_off_and_the_audit_names_the_device() {
    let r = paired_off();
    r.rt.set_enabled(true).unwrap();
    wait_for("the poller", || r.rt.listening());
    assert!(r.rt.turn_off_from_phone("dev-1").unwrap());
    assert!(!r.host.config().enabled);
    assert!(!r.rt.alive());
    let audits = r.audits.lock().clone();
    assert!(audits.iter().any(|(k, who, _)| *k == AuditKind::ChannelOff && *who == Who::Device(Some("dev-1".into()))));
}

#[test]
fn switching_off_twice_is_harmless_and_audited_once() {
    let r = paired_off();
    r.rt.set_enabled(true).unwrap();
    assert!(r.rt.turn_off_from_phone("dev-1").unwrap());
    assert!(!r.rt.turn_off_from_phone("dev-1").unwrap());
    let offs = r.audits.lock().iter().filter(|(k, who, _)| *k == AuditKind::ChannelOff && matches!(who, Who::Device(_))).count();
    assert_eq!(offs, 1);
}

#[test]
fn the_phone_view_has_no_chat_names_and_no_caps() {
    let r = paired_off();
    r.rt.set_enabled(true).unwrap();
    wait_for("the poller", || r.rt.listening());
    wait_for("connected", || r.rt.phone_view().connected);
    let value = serde_json::to_value(r.rt.phone_view()).unwrap();
    let keys: Vec<&String> = value.as_object().unwrap().keys().collect();
    assert_eq!(keys.len(), 3, "{keys:?}");
    for key in ["on", "connected", "botUsername"] {
        assert!(value.get(key).is_some());
    }
    r.rt.stop();
}
