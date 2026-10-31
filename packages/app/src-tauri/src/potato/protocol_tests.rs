//! Channel protocol v1, Puppet Master side: every type parses, the caps hold, an unknown type is ignored.
//! The wire examples here are what `packages/potato-brain/src/channels/potato.ts` and
//! `test/fake-brain.mjs` really send.

use serde_json::{json, Value};

use super::protocol::*;

fn event(v: Value) -> Option<Event> {
    parse_event(&v)
}

fn post(events: Vec<Value>) -> Vec<u8> {
    serde_json::to_vec(&json!({ "v": 1, "events": events })).unwrap()
}

#[test]
fn every_event_type_the_brain_sends_parses() {
    assert_eq!(
        event(json!({"v":1,"type":"hello","protocol":1,"version":"0.1.0","provider":"deepseek","packs":["fish","potato"],"surfaces":["app"]})),
        Some(Event::Hello { protocol: 1, version: "0.1.0".into(), provider: "deepseek".into(), packs: vec!["fish".into(), "potato".into()], surfaces: vec!["app".into()] })
    );
    assert_eq!(event(json!({"v":1,"type":"typing","conversationId":"app:dm:nimbus","on":true})), Some(Event::Typing { conversation_id: "app:dm:nimbus".into(), on: true }));
    assert_eq!(
        event(json!({"v":1,"type":"reply","conversationId":"app:dm:nimbus","inReplyTo":"m-1","segments":["one","two"],"kaomoji":"(^_^)","tone":"warm"})),
        Some(Event::Reply { conversation_id: "app:dm:nimbus".into(), in_reply_to: "m-1".into(), segments: vec!["one".into(), "two".into()], kaomoji: Some("(^_^)".into()), tone: Some("warm".into()) })
    );
    assert_eq!(
        event(json!({"v":1,"type":"mood","mood":"genki","intensity":2,"energy":60,"hunger":30})),
        Some(Event::Mood { mood: "genki".into(), intensity: 2, energy: 60, hunger: 30 })
    );
    // camelCase usage names, exactly as the brain writes them
    assert_eq!(
        event(json!({"v":1,"type":"usage","model":"fake","promptTokens":10,"completionTokens":5,"cacheHitTokens":0,"cacheMissTokens":10})),
        Some(Event::Usage { model: "fake".into(), prompt_tokens: 10, completion_tokens: 5, cache_hit_tokens: 0, cache_miss_tokens: 10 })
    );
    assert_eq!(
        event(json!({"v":1,"type":"helper_request","requestId":"h-1","conversationId":"app:dm:nimbus","task":"add a readme","project":"demo-a"})),
        Some(Event::HelperRequest { request_id: "h-1".into(), conversation_id: "app:dm:nimbus".into(), task: "add a readme".into(), project: Some("demo-a".into()) })
    );
    // `error` carries conversationId and inReplyTo
    assert_eq!(
        event(json!({"v":1,"type":"error","code":"PROVIDER_AUTH","conversationId":"app:dm:nimbus","inReplyTo":"m-1"})),
        Some(Event::Error { code: ErrorCode::ProviderAuth, conversation_id: Some("app:dm:nimbus".into()), in_reply_to: Some("m-1".into()) })
    );
}

#[test]
fn every_error_code_maps_and_an_unknown_one_is_internal() {
    for (word, code) in [
        ("PROVIDER_AUTH", ErrorCode::ProviderAuth),
        ("PROVIDER_LIMIT", ErrorCode::ProviderLimit),
        ("PROVIDER_DOWN", ErrorCode::ProviderDown),
        ("NETWORK", ErrorCode::Network),
        ("PERSONA_TOO_BIG", ErrorCode::PersonaTooBig),
        ("SURFACE_AUTH", ErrorCode::SurfaceAuth),
        ("INTERNAL", ErrorCode::Internal),
    ] {
        assert_eq!(ErrorCode::parse(word), code);
        assert_eq!(code.as_str(), word);
    }
    assert_eq!(ErrorCode::parse("<response body with a key>"), ErrorCode::Internal);
}

#[test]
fn an_unknown_type_is_ignored_and_never_fails_the_post() {
    let body = post(vec![
        json!({"v":1,"type":"from_the_future","x":1}),
        json!({"v":1,"type":"typing","conversationId":"app:dm:nimbus","on":false}),
        // the phase 2 `inbound` is not for this version: it parses as unknown, nothing acts on it
        json!({"v":1,"type":"inbound","surface":"x","conversationId":"x:dm:a:b","text":"hi"}),
    ]);
    let events = parse_post(&body).unwrap();
    assert_eq!(events.len(), 3);
    assert!(matches!(&events[0], Event::Unknown(t) if t == "from_the_future"));
    assert!(matches!(&events[1], Event::Typing { on: false, .. }));
    assert!(matches!(&events[2], Event::Unknown(t) if t == "inbound"));
}

#[test]
fn a_malformed_known_event_is_dropped_on_its_own() {
    let body = post(vec![
        json!({"v":1,"type":"typing","on":true}),                                    // no conversation
        json!({"v":1,"type":"reply","conversationId":"app:dm:a","segments":[]}),     // nothing to say
        json!({"v":1,"type":"reply","conversationId":"app:dm:a","segments":["  "]}), // only blanks
        json!({"v":1,"type":"mood","mood":"furious","intensity":1}),                 // not a mood
        json!({"v":1,"type":"helper_request","requestId":"h","conversationId":"app:dm:a","task":"  "}),
        json!({"type":"typing","conversationId":"app:dm:a","on":true}),              // no v
        json!({"v":2,"type":"typing","conversationId":"app:dm:a","on":true}),        // wrong v
        json!({"v":1,"type":"typing","conversationId":"app:dm:a","on":true}),        // the one good one
    ]);
    let events = parse_post(&body).unwrap();
    assert_eq!(events.len(), 1, "{events:?}");
}

#[test]
fn a_post_has_at_most_32_events_and_64_kb() {
    let typing = json!({"v":1,"type":"typing","conversationId":"app:dm:a","on":true});
    assert_eq!(parse_post(&post(vec![typing.clone(); MAX_EVENTS])).unwrap().len(), MAX_EVENTS);
    assert_eq!(parse_post(&post(vec![typing.clone(); MAX_EVENTS + 1])), Err(PostError::Invalid));
    let big = vec![b' '; MAX_POST_BYTES + 1];
    assert_eq!(parse_post(&big), Err(PostError::TooBig));
    assert_eq!(parse_post(b"not json"), Err(PostError::Invalid));
    assert_eq!(parse_post(br#"{"v":2,"events":[]}"#), Err(PostError::Invalid));
    assert_eq!(parse_post(br#"{"v":1}"#), Err(PostError::Invalid));
    assert_eq!(parse_post(br#"{"v":1,"events":{}}"#), Err(PostError::Invalid));
}

#[test]
fn a_reply_has_at_most_6_segments_of_2000_characters() {
    let long = "x".repeat(MAX_SEGMENT_CHARS + 500);
    let segments: Vec<Value> = (0..10).map(|_| json!(long)).collect();
    let Some(Event::Reply { segments, .. }) = event(json!({"v":1,"type":"reply","conversationId":"app:dm:a","inReplyTo":"m","segments":segments})) else { panic!("a reply") };
    assert_eq!(segments.len(), MAX_SEGMENTS);
    assert!(segments.iter().all(|s| s.chars().count() == MAX_SEGMENT_CHARS));
}

#[test]
fn a_helper_task_is_cut_to_600_and_usage_counts_cannot_be_absurd() {
    let task = "t".repeat(2000);
    let Some(Event::HelperRequest { task, .. }) = event(json!({"v":1,"type":"helper_request","requestId":"h","conversationId":"app:dm:a","task":task})) else { panic!("a request") };
    assert_eq!(task.chars().count(), MAX_TASK);
    let Some(Event::Usage { prompt_tokens, completion_tokens, .. }) =
        event(json!({"v":1,"type":"usage","model":"m","promptTokens":1e18,"completionTokens":-5,"cacheHitTokens":"many","cacheMissTokens":null}))
    else {
        panic!("usage")
    };
    assert_eq!(prompt_tokens, 10_000_000);
    assert_eq!(completion_tokens, 0);
}

#[test]
fn a_send_item_is_the_shape_the_brain_reads() {
    let item = send_item(&SendFacts {
        id: "m-1",
        conversation_id: "app:dm:nimbus",
        surface: "app",
        from_id: "owner",
        from_name: "you",
        trust: Trust::OwnerPc,
        text: &"a".repeat(MAX_SEND_TEXT + 10),
        context: None,
        tools: Tools::Full,
    });
    assert_eq!(item["v"], 1);
    assert_eq!(item["type"], "send");
    assert_eq!(item["from"], json!({"id":"owner","name":"you","trust":"owner_pc"}));
    assert_eq!(item["tools"], "full");
    assert_eq!(item["text"].as_str().unwrap().chars().count(), MAX_SEND_TEXT);
    assert!(item.get("context").is_none());
    let with = send_item(&SendFacts { context: Some("fenced"), trust: Trust::Others, tools: Tools::NoMemory, ..facts() });
    assert_eq!(with["context"], "fenced");
    assert_eq!(with["from"]["trust"], "others");
    assert_eq!(with["tools"], "no_memory");
}

fn facts() -> SendFacts<'static> {
    SendFacts { id: "m", conversation_id: "app:dm:a", surface: "app", from_id: "owner", from_name: "you", trust: Trust::OwnerPc, text: "hi", context: None, tools: Tools::Full }
}

#[test]
fn the_other_inbox_items_carry_v_and_their_fields() {
    assert_eq!(persona_item("potato"), json!({"v":1,"type":"persona","pack":"potato"}));
    assert_eq!(pause_item(PauseReason::DailyLimit), json!({"v":1,"type":"pause","reason":"daily_limit"}));
    assert_eq!(pause_item(PauseReason::StoppedByYou)["reason"], "stopped_by_you");
    assert_eq!(pause_item(PauseReason::KeyMissing)["reason"], "key_missing");
    assert_eq!(resume_item(), json!({"v":1,"type":"resume"}));
    assert_eq!(shutdown_item(), json!({"v":1,"type":"shutdown"}));
    let update = helper_update_item("h-1", HelperStatus::Finished, &"s".repeat(3000));
    assert_eq!(update["type"], "helper_update");
    assert_eq!(update["requestId"], "h-1");
    assert_eq!(update["status"], "finished");
    assert_eq!(update["summary"].as_str().unwrap().chars().count(), MAX_SUMMARY);
    for (status, word) in [
        (HelperStatus::Started, "started"),
        (HelperStatus::Declined, "declined"),
        (HelperStatus::Finished, "finished"),
        (HelperStatus::Failed, "failed"),
        (HelperStatus::Stopped, "stopped"),
        (HelperStatus::Refused, "refused"),
    ] {
        assert_eq!(status.as_str(), word);
    }
}

#[test]
fn trust_words_are_the_wire_words() {
    assert_eq!(Trust::OwnerPc.as_str(), "owner_pc");
    assert_eq!(Trust::OwnerRemote.as_str(), "owner_remote");
    assert_eq!(Trust::Others.as_str(), "others");
    assert!(Trust::OwnerPc.is_owner() && Trust::OwnerRemote.is_owner() && !Trust::Others.is_owner());
    assert_eq!(Tools::NoMemory.as_str(), "no_memory");
}

/// The fake brain script and the real channel are the fixed wire format: this reads the script's own
/// hello and send handling from the file and checks the names this side expects are the names it writes.
#[test]
fn the_fake_brain_writes_the_names_this_side_reads() {
    let script = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../potato-brain/test/fake-brain.mjs")).expect("fake-brain.mjs");
    for name in ["promptTokens", "completionTokens", "cacheHitTokens", "cacheMissTokens", "inReplyTo", "conversationId", "X-PM-Potato-Token", "potato/brain/events", "potato/brain/inbox"] {
        assert!(script.contains(name), "the fake brain no longer says {name}");
    }
}
