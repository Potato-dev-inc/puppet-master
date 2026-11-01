//! The fallback brains: the rules on their own, the settings file that keeps them, and the engine on fakes (a
//! provider limit moves the failed message to the next brain; an auth error does not; the next run starts the main
//! brain again).

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use serde_json::json;

use super::fallback::{chain, clean, next_step};
use super::protocol::ErrorCode;
use super::service::SECRET_DEEPSEEK;
use super::service_tests::Rig;
use super::store::{self, BrainKind, Settings};
use super::wire_names;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-potato-fallback-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn only_a_limit_or_a_down_model_switches_and_only_to_a_next_brain() {
    assert_eq!(next_step(2, 0, ErrorCode::ProviderLimit), Some(1));
    assert_eq!(next_step(2, 0, ErrorCode::ProviderDown), Some(1));
    assert_eq!(next_step(2, 1, ErrorCode::ProviderLimit), None, "no brain left after the last one");
    assert_eq!(next_step(1, 0, ErrorCode::ProviderDown), None, "no fallback configured");
    for code in [ErrorCode::ProviderAuth, ErrorCode::Network, ErrorCode::PersonaTooBig, ErrorCode::SurfaceAuth, ErrorCode::Internal] {
        assert_eq!(next_step(2, 0, code), None, "{code:?} needs the person, not a fallback");
    }
}

#[test]
fn the_fallbacks_are_kept_in_order_without_agents_repeats_or_the_main_brain() {
    assert_eq!(clean(BrainKind::Deepseek, &[BrainKind::Go, BrainKind::Agents, BrainKind::Go, BrainKind::Deepseek]), vec![BrainKind::Go]);
    assert_eq!(clean(BrainKind::Go, &[BrainKind::Agents]), Vec::<BrainKind>::new());
    assert_eq!(clean(BrainKind::Go, &[BrainKind::Deepseek]), vec![BrainKind::Deepseek]);
}

#[test]
fn the_chain_is_the_main_brain_then_the_usable_fallbacks() {
    assert_eq!(chain(BrainKind::Deepseek, &[BrainKind::Go], true), vec![BrainKind::Deepseek, BrainKind::Go]);
    assert_eq!(chain(BrainKind::Deepseek, &[BrainKind::Go], false), vec![BrainKind::Deepseek, BrainKind::Go], "Go needs no DeepSeek key");
    assert_eq!(chain(BrainKind::Go, &[BrainKind::Deepseek], false), vec![BrainKind::Go], "a DeepSeek fallback without its key is skipped");
    assert_eq!(chain(BrainKind::Go, &[BrainKind::Deepseek], true), vec![BrainKind::Go, BrainKind::Deepseek]);
    assert_eq!(chain(BrainKind::Go, &[], true), vec![BrainKind::Go]);
}

#[test]
fn the_settings_file_keeps_the_fallbacks_clean_and_an_old_file_has_none() {
    let root = temp("settings");
    // an old settings file, written before the list existed
    std::fs::write(store::settings_path(&root), r#"{"v":1,"brain":"deepseek","defaultPersona":"fish","dailyLimitCents":50}"#).unwrap();
    assert!(store::load_settings(&root).unwrap().fallbacks.is_empty());

    store::save_settings(&root, &Settings { brain: BrainKind::Deepseek, fallbacks: vec![BrainKind::Agents, BrainKind::Go, BrainKind::Go, BrainKind::Deepseek], ..Settings::default() }).unwrap();
    let saved = store::load_settings(&root).unwrap();
    assert_eq!(saved.fallbacks, vec![BrainKind::Go]);
    let json = serde_json::to_value(&saved).unwrap();
    assert_eq!(json["fallbacks"], json!(["go"]));
}

/// Settings with a Go fallback behind the DeepSeek main brain (the rig's key is set).
fn with_go_fallback(name: &str) -> Rig {
    let rig = Rig::new(name);
    rig.svc.set_settings(Settings { fallbacks: vec![BrainKind::Go], ..Settings::default() }).unwrap();
    rig
}

fn error_event(code: &str, in_reply_to: &str) -> serde_json::Value {
    json!({"v":1,"type":"error","code":code,"conversationId":"app:dm:nimbus","inReplyTo":in_reply_to})
}

#[test]
fn a_provider_limit_moves_the_failed_message_to_the_fallback_brain() {
    let rig = with_go_fallback("fb-limit");
    let sent = rig.svc.send("nimbus", "hello there").unwrap();
    assert!(sent.delivered);
    assert_eq!(rig.spawned(), 1);
    assert_eq!(rig.launcher.spawned.lock()[0].provider, wire_names::DEEPSEEK);
    rig.hello();

    rig.post(vec![error_event("PROVIDER_LIMIT", &sent.id)]);

    assert!(rig.child(0).killed.load(Ordering::SeqCst), "the DeepSeek brain was stopped");
    assert_eq!(rig.spawned(), 2, "the fallback brain started");
    assert_eq!(rig.launcher.spawned.lock()[1].provider, wire_names::GO, "with its own provider");
    assert!(!rig.launcher.spawned.lock()[1].has_key, "and without the DeepSeek key");
    let (items, _) = rig.inbox(0);
    assert!(items.iter().any(|item| item["id"] == sent.id.as_str()), "the failed message is in the new brain's inbox");
    assert!(rig.notes().iter().any(|n| n == "Nimbus's model is busy or out of balance. Trying your next fallback."), "{:?}", rig.notes());
    assert!(rig.svc.state().unwrap().bots.iter().all(|b| b.last_error.is_none()), "the switch is not shown as an error");
}

#[test]
fn a_down_model_switches_too_and_the_next_run_starts_the_main_brain_again() {
    let rig = with_go_fallback("fb-down");
    let sent = rig.svc.send("nimbus", "one").unwrap();
    rig.hello();
    rig.post(vec![error_event("PROVIDER_DOWN", &sent.id)]);
    assert_eq!(rig.launcher.spawned.lock().last().unwrap().provider, wire_names::GO);
    rig.hello();

    // the fallback brain answers, then is asked to leave and leaves
    rig.answer_all();
    rig.svc.stop_bot("nimbus");
    rig.child(1).exit_with(0);
    rig.svc.tick();

    rig.svc.send("nimbus", "two").unwrap();
    assert_eq!(rig.spawned(), 3);
    assert_eq!(rig.launcher.spawned.lock()[2].provider, wire_names::DEEPSEEK, "a new run tries the main brain first");
}

#[test]
fn an_auth_error_does_not_switch_brains() {
    let rig = with_go_fallback("fb-auth");
    let sent = rig.svc.send("nimbus", "hi").unwrap();
    rig.hello();
    rig.post(vec![error_event("PROVIDER_AUTH", &sent.id)]);
    assert_eq!(rig.spawned(), 1);
    assert!(!rig.child(0).killed.load(Ordering::SeqCst));
    assert!(rig.notes().iter().any(|n| n.contains("could not use the model key")));
}

#[test]
fn without_a_fallback_a_limit_is_only_shown() {
    let rig = Rig::new("fb-none");
    let sent = rig.svc.send("nimbus", "hi").unwrap();
    rig.hello();
    rig.post(vec![error_event("PROVIDER_LIMIT", &sent.id)]);
    assert_eq!(rig.spawned(), 1);
    assert!(rig.notes().iter().any(|n| n.contains("out of balance or too busy")));
}

#[test]
fn a_deepseek_fallback_without_its_key_is_not_used() {
    let rig = Rig::new("fb-nokey");
    rig.svc.set_settings(Settings { brain: BrainKind::Go, fallbacks: vec![BrainKind::Deepseek], ..Settings::default() }).unwrap();
    rig.host.secrets.lock().remove(SECRET_DEEPSEEK);
    let sent = rig.svc.send("nimbus", "hi").unwrap();
    assert!(sent.delivered);
    rig.hello();
    rig.post(vec![error_event("PROVIDER_DOWN", &sent.id)]);
    assert_eq!(rig.spawned(), 1, "no key, so no switch");
}

#[test]
fn the_chain_starts_from_the_bots_own_brain_when_it_has_one() {
    // Settings: DeepSeek, then Go. This bot picked Go itself, so Go is the first choice and the list has nothing left for it.
    let rig = with_go_fallback("fb-own-go");
    rig.svc.set_talk_brain("nimbus", Some(BrainKind::Go)).unwrap();
    let sent = rig.svc.send("nimbus", "hello there").unwrap();
    assert_eq!(rig.launcher.spawned.lock()[0].provider, wire_names::GO, "the bot's own brain runs first");
    rig.hello();
    rig.post(vec![error_event("PROVIDER_LIMIT", &sent.id)]);
    assert_eq!(rig.spawned(), 1, "Go is already the first choice, so there is no brain to fall back to");
    assert!(!rig.child(0).killed.load(Ordering::SeqCst));
    assert!(rig.notes().iter().any(|n| n.contains("out of balance or too busy")), "{:?}", rig.notes());
}

#[test]
fn an_own_brain_equal_to_the_default_still_gets_the_fallback_list() {
    // The bot picked DeepSeek itself (the same as the default): the Go fallback still follows it.
    let rig = with_go_fallback("fb-own-ds");
    rig.svc.set_talk_brain("nimbus", Some(BrainKind::Deepseek)).unwrap();
    let sent = rig.svc.send("nimbus", "hello there").unwrap();
    assert_eq!(rig.launcher.spawned.lock()[0].provider, wire_names::DEEPSEEK);
    rig.hello();
    rig.post(vec![error_event("PROVIDER_LIMIT", &sent.id)]);
    assert_eq!(rig.spawned(), 2);
    assert_eq!(rig.launcher.spawned.lock()[1].provider, wire_names::GO);
}

#[test]
fn the_default_brain_is_not_a_fallback_for_a_bot_with_its_own_brain() {
    // Settings: Go as the brain, DeepSeek as the fallback. This bot picked DeepSeek: the Settings brain (Go) is not added behind it.
    let rig = Rig::new("fb-own-first");
    rig.svc.set_settings(Settings { brain: BrainKind::Go, fallbacks: vec![BrainKind::Deepseek], ..Settings::default() }).unwrap();
    rig.svc.set_talk_brain("nimbus", Some(BrainKind::Deepseek)).unwrap();
    let sent = rig.svc.send("nimbus", "hi").unwrap();
    assert_eq!(rig.launcher.spawned.lock()[0].provider, wire_names::DEEPSEEK);
    rig.hello();
    rig.post(vec![error_event("PROVIDER_DOWN", &sent.id)]);
    assert_eq!(rig.spawned(), 1);
}
