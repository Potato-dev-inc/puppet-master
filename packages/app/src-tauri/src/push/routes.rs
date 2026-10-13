//! HTTP routes of the push feature, as pure functions over the hub (no `AppHandle`, so they are
//! testable in-process).
//!
//! - `GET /push/key`          -> `{public_key}` (the VAPID applicationServerKey)
//! - `POST /push/subscribe`   -> store the caller's PushSubscription
//! - `DELETE /push/subscribe` -> drop the caller's subscription
//! - `POST /push/presence`    -> `{visible}`: the PWA is on screen (suppresses notifications)
//! - `POST /push/act`         -> NOT bearer-authed: redeems an Allow / Deny token from a notification
//!
//! All but the last require a paired device's bearer token; the device is whoever owns that token,
//! never a field of the request, so one device cannot touch another's subscription.

use super::hub::{Grant, PushHub};
use super::store::Subscription;
use crate::mobile_pairing::bearer_token;
use serde::Deserialize;
use serde_json::{json, Value};

pub type Reply = (u16, Value);

fn unauthorized() -> Reply {
    (
        401,
        json!({"error": "device_token_required", "hint": "Push routes need the bearer token of a paired phone."}),
    )
}

/// Which paired device is calling. A loopback caller without a token is not a device.
fn caller(hub: &PushHub, headers: &str) -> Result<String, Reply> {
    let token = bearer_token(headers).ok_or_else(unauthorized)?;
    hub.device_for_token(&token).ok_or_else(unauthorized)
}

#[derive(Deserialize)]
struct SubscribeBody {
    endpoint: String,
    keys: Keys,
}

#[derive(Deserialize)]
struct Keys {
    p256dh: String,
    auth: String,
}

#[derive(Deserialize)]
struct PresenceBody {
    visible: bool,
}

/// Routes that need a device bearer. `None` = not a push route.
pub fn handle(hub: &PushHub, method: &str, segments: &[&str], headers: &str, body: &[u8], now_ms: u64) -> Option<Reply> {
    let result = match (method, segments) {
        ("GET", ["push", "key"]) => caller(hub, headers).map(|_| (200, json!({"public_key": hub.vapid().public_key_b64url()}))),
        ("POST", ["push", "subscribe"]) => caller(hub, headers).and_then(|device| subscribe(hub, &device, body, now_ms)),
        ("DELETE", ["push", "subscribe"]) => caller(hub, headers).and_then(|device| unsubscribe(hub, &device)),
        ("POST", ["push", "presence"]) => caller(hub, headers).and_then(|device| presence(hub, &device, body, now_ms)),
        _ => return None,
    };
    Some(result.unwrap_or_else(|reply| reply))
}

fn subscribe(hub: &PushHub, device: &str, body: &[u8], now_ms: u64) -> Result<Reply, Reply> {
    let parsed: SubscribeBody = serde_json::from_slice(body)
        .map_err(|err| (400, json!({"error": "invalid_subscription", "detail": err.to_string()})))?;
    let sub = Subscription {
        endpoint: parsed.endpoint,
        p256dh: parsed.keys.p256dh,
        auth: parsed.keys.auth,
        created_at: now_ms / 1000,
    };
    hub.store()
        .lock()
        .upsert(device, sub)
        .map_err(|err| (400, json!({"error": "invalid_subscription", "detail": err})))?;
    Ok((200, json!({"ok": true})))
}

fn unsubscribe(hub: &PushHub, device: &str) -> Result<Reply, Reply> {
    let removed = hub
        .store()
        .lock()
        .remove(device)
        .map_err(|err| (500, json!({"error": "store_failed", "detail": err})))?;
    Ok((200, json!({"ok": true, "removed": removed})))
}

fn presence(hub: &PushHub, device: &str, body: &[u8], now_ms: u64) -> Result<Reply, Reply> {
    let parsed: PresenceBody = serde_json::from_slice(body)
        .map_err(|err| (400, json!({"error": "invalid_presence", "detail": err.to_string()})))?;
    hub.set_visible(device, parsed.visible, now_ms);
    Ok((200, json!({"ok": true})))
}

/// What an Allow / Deny tap asks for, once its token checked out.
#[derive(Debug, PartialEq, Eq)]
pub struct Act {
    pub grant: Grant,
    /// `allow_once` or `deny`: never a broad approval.
    pub choice: &'static str,
}

/// `POST /push/act` body `{token, choice: "allow"|"deny"}`. The caller then answers the prompt
/// through `pane_prompt::answer`, which re-checks that the prompt on screen is still `prompt_id`.
pub fn redeem_act(hub: &PushHub, body: &[u8], now_ms: u64) -> Result<Act, Reply> {
    #[derive(Deserialize)]
    struct ActBody {
        token: String,
        choice: String,
    }
    let parsed: ActBody = serde_json::from_slice(body).map_err(|_| (400, json!({"error": "invalid_request"})))?;
    let choice = match parsed.choice.as_str() {
        "allow" | "allow_once" => "allow_once",
        "deny" => "deny",
        _ => return Err((400, json!({"error": "invalid_choice", "hint": "choice must be allow or deny"}))),
    };
    let grant = hub
        .redeem_grant(&parsed.token, now_ms)
        .map_err(|why| (401, json!({"error": why})))?;
    Ok(Act { grant, choice })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::push::hub::testing::{hub_with, FakeDirectory};
    use crate::push::hub::Candidate;
    use crate::push::store::test_subscription;
    use crate::push::triggers::{Kind, PromptInfo};
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use std::sync::Arc;

    fn bearer(token: &str) -> String {
        format!("Authorization: Bearer {token}\r\nHost: pm.example.com\r\n")
    }

    fn sub_body(endpoint: &str) -> Vec<u8> {
        let (sub, _, _) = test_subscription(endpoint);
        json!({"endpoint": sub.endpoint, "expirationTime": null, "keys": {"p256dh": sub.p256dh, "auth": sub.auth}})
            .to_string()
            .into_bytes()
    }

    fn setup(name: &str) -> PushHub {
        hub_with(name, Arc::new(FakeDirectory::new(&[("tok-a", "dev-a"), ("tok-b", "dev-b")])))
    }

    #[test]
    fn every_authed_route_requires_a_known_device_token() {
        let hub = setup("routes-auth");
        for (method, path) in [("GET", "key"), ("POST", "subscribe"), ("DELETE", "subscribe"), ("POST", "presence")] {
            let segs = ["push", path];
            let none = handle(&hub, method, &segs, "Host: x\r\n", b"{}", 1).unwrap();
            assert_eq!(none.0, 401, "{method} {path} without a token");
            let wrong = handle(&hub, method, &segs, &bearer("not-a-device"), b"{}", 1).unwrap();
            assert_eq!(wrong.0, 401, "{method} {path} with an unknown token");
        }
        assert!(handle(&hub, "GET", &["panes"], &bearer("tok-a"), b"", 1).is_none(), "not a push route");
        assert!(handle(&hub, "POST", &["push", "act"], &bearer("tok-a"), b"{}", 1).is_none(), "act is handled separately");
    }

    #[test]
    fn key_then_subscribe_then_unsubscribe_transcript() {
        let hub = setup("routes-flow");
        let key = handle(&hub, "GET", &["push", "key"], &bearer("tok-a"), b"", 1).unwrap();
        assert_eq!(key.0, 200);
        assert_eq!(key.1["public_key"], hub.vapid().public_key_b64url());

        let ok = handle(&hub, "POST", &["push", "subscribe"], &bearer("tok-a"), &sub_body("https://fcm.googleapis.com/x"), 5_000).unwrap();
        assert_eq!((ok.0, ok.1["ok"].clone()), (200, json!(true)));
        assert!(hub.store().lock().get("dev-a").is_some());

        let bad = handle(&hub, "POST", &["push", "subscribe"], &bearer("tok-a"), &sub_body("http://169.254.169.254/latest"), 5_000).unwrap();
        assert_eq!(bad.0, 400);
        let junk = handle(&hub, "POST", &["push", "subscribe"], &bearer("tok-a"), b"nope", 5_000).unwrap();
        assert_eq!(junk.0, 400);

        let gone = handle(&hub, "DELETE", &["push", "subscribe"], &bearer("tok-a"), b"", 6_000).unwrap();
        assert_eq!((gone.0, gone.1["removed"].clone()), (200, json!(true)));
        assert!(hub.store().lock().get("dev-a").is_none());
    }

    #[test]
    fn a_device_can_only_touch_its_own_subscription() {
        let hub = setup("routes-isolation");
        handle(&hub, "POST", &["push", "subscribe"], &bearer("tok-a"), &sub_body("https://fcm.googleapis.com/a"), 1).unwrap();
        // Device B deletes: it removes nothing of A's, whatever the body says.
        let del = handle(&hub, "DELETE", &["push", "subscribe"], &bearer("tok-b"), br#"{"device_id":"dev-a"}"#, 1).unwrap();
        assert_eq!(del.1["removed"], json!(false));
        assert!(hub.store().lock().get("dev-a").is_some());
        // Device B subscribing never overwrites A either.
        handle(&hub, "POST", &["push", "subscribe"], &bearer("tok-b"), &sub_body("https://web.push.apple.com/b"), 1).unwrap();
        let store = hub.store().lock();
        assert_ne!(store.get("dev-a"), store.get("dev-b"));
        assert!(store.get("dev-b").is_some());
    }

    #[test]
    fn presence_marks_the_calling_device_only() {
        let hub = setup("routes-presence");
        let r = handle(&hub, "POST", &["push", "presence"], &bearer("tok-a"), br#"{"visible":true}"#, 10_000).unwrap();
        assert_eq!(r.0, 200);
        assert!(hub.is_visible("dev-a", 10_001));
        assert!(!hub.is_visible("dev-b", 10_001));
        assert_eq!(handle(&hub, "POST", &["push", "presence"], &bearer("tok-a"), br#"{"visible":"yes"}"#, 1).unwrap().0, 400);
    }

    fn token_for(hub: &PushHub, device: &str) -> String {
        let (sub, _, _) = test_subscription("https://fcm.googleapis.com/z");
        hub.store().lock().upsert(device, sub).unwrap();
        let candidate = Candidate {
            kind: Kind::WaitingInput,
            key: "pane-1".into(),
            pane_id: Some("pane-1".into()),
            agent_label: "Kilo".into(),
            prompt: Some(PromptInfo { prompt_id: "pid".into(), text: "Run command: ls".into(), actionable: true }),
        };
        let (_, deliveries) = hub.plan(&candidate, 1_000);
        let payload: Value = serde_json::from_slice(&deliveries[0].payload).unwrap();
        payload["act"]["token"].as_str().unwrap().to_string()
    }

    #[test]
    fn act_redeems_once_and_only_for_allow_or_deny() {
        let hub = setup("routes-act");
        let token = token_for(&hub, "dev-a");
        let bad_choice = redeem_act(&hub, json!({"token": token, "choice": "allow_all"}).to_string().as_bytes(), 2_000).unwrap_err();
        assert_eq!(bad_choice.0, 400, "broad approvals can never come from a notification");
        let act = redeem_act(&hub, json!({"token": token, "choice": "allow"}).to_string().as_bytes(), 2_000).unwrap();
        assert_eq!(act.choice, "allow_once");
        assert_eq!((act.grant.pane_id.as_str(), act.grant.prompt_id.as_str()), ("pane-1", "pid"));
        let again = redeem_act(&hub, json!({"token": token, "choice": "allow"}).to_string().as_bytes(), 2_001).unwrap_err();
        assert_eq!(again, (401, json!({"error": "unknown_or_used_token"})));
        let forged = redeem_act(&hub, json!({"token": URL_SAFE_NO_PAD.encode([7u8; 32]), "choice": "deny"}).to_string().as_bytes(), 2_000).unwrap_err();
        assert_eq!(forged.0, 401);
        assert_eq!(redeem_act(&hub, b"garbage", 1).unwrap_err().0, 400);
    }
}
